"""Channel Manager for Omiga.

Provides unified management of multiple communication channels with:
- Queue-based message processing
- Concurrent consumer workers per channel
- Message batching and merging
- Graceful start/stop lifecycle
"""
from __future__ import annotations

import asyncio
import logging
from typing import Any, Callable, Dict, List, Optional, Set, Tuple

from omiga.channels.base import Channel

logger = logging.getLogger("omiga.channel_manager")

# Default max size per channel queue
_CHANNEL_QUEUE_MAXSIZE = 1000

# Workers per channel for concurrent session processing
_CONSUMER_WORKERS_PER_CHANNEL = 4


def _drain_same_key(
    q: asyncio.Queue,
    key: str,
    first_payload: Any,
) -> List[Any]:
    """Drain queue of payloads with same key; return batch."""
    batch = [first_payload]
    put_back: List[Any] = []

    while True:
        try:
            p = q.get_nowait()
        except asyncio.QueueEmpty:
            break

        # Extract key from payload
        if isinstance(p, tuple) and len(p) >= 2:
            payload_key = p[0]
        elif isinstance(p, dict):
            payload_key = p.get("chat_jid", "default")
        elif hasattr(p, "chat_jid"):
            payload_key = getattr(p, "chat_jid", "default")
        else:
            payload_key = p

        if payload_key == key:
            batch.append(p)
        else:
            put_back.append(p)

    for p in put_back:
        q.put_nowait(p)

    return batch


class ChannelManager:
    """Manages multiple communication channels with queue-based processing.

    Features:
    - Unified start/stop lifecycle for all channels
    - Per-channel queues with configurable size
    - Multiple consumer workers per channel for parallel processing
    - Thread-safe enqueue for external callers
    - Session-based batching to group related messages
    """

    def __init__(self, channels: List[Channel]):
        """Initialize the channel manager.

        Args:
            channels: List of channel instances to manage
        """
        self.channels = channels
        self._lock = asyncio.Lock()
        self._queues: Dict[str, asyncio.Queue] = {}
        self._consumer_tasks: List[asyncio.Task] = []
        self._loop: Optional[asyncio.AbstractEventLoop] = None

        # Session tracking for batching
        self._in_progress: Set[Tuple[str, str]] = set()
        self._pending: Dict[Tuple[str, str], List[Any]] = {}
        self._key_locks: Dict[Tuple[str, str], asyncio.Lock] = {}

    @classmethod
    def from_channels(
        cls,
        channels: List[Channel],
    ) -> "ChannelManager":
        """Create a channel manager from a list of channels.

        Args:
            channels: List of connected channel instances

        Returns:
            ChannelManager instance
        """
        return cls(channels)

    def _get_session_key(self, payload: Any) -> str:
        """Extract session key from payload for batching.

        Args:
            payload: The message payload

        Returns:
            Session key string
        """
        # Try to get session key from payload
        if hasattr(payload, "session_key"):
            return payload.session_key
        if hasattr(payload, "chat_jid"):
            return payload.chat_jid
        if isinstance(payload, dict) and "chat_jid" in payload:
            return payload["chat_jid"]
        return "default"

    def _make_enqueue_cb(self, channel_id: str) -> Callable[[Any], None]:
        """Return a callback that enqueues payload for the given channel."""

        def cb(payload: Any) -> None:
            self.enqueue(channel_id, payload)

        return cb

    def _enqueue_one(self, channel_id: str, payload: Any) -> None:
        """Run on event loop: enqueue or append to pending if session in progress."""
        q = self._queues.get(channel_id)
        if not q:
            logger.debug("enqueue: no queue for channel=%s", channel_id)
            return

        key = self._get_session_key(payload)

        # If session is in progress, hold in pending
        if (channel_id, key) in self._in_progress:
            self._pending.setdefault((channel_id, key), []).append(payload)
            logger.debug("Session %s/%s in progress, holding in pending", channel_id, key)
            return

        q.put_nowait(payload)

    def enqueue(self, channel_id: str, payload: Any) -> None:
        """Enqueue a payload for the channel. Thread-safe.

        Args:
            channel_id: The channel identifier
            payload: The message payload to enqueue

        Note:
            Call after start_all(). Safe to call from external threads.
        """
        if not self._queues.get(channel_id):
            logger.debug("enqueue: no queue for channel=%s", channel_id)
            return

        if self._loop is None:
            logger.warning("enqueue: loop not set for channel=%s", channel_id)
            return

        self._loop.call_soon_threadsafe(
            self._enqueue_one,
            channel_id,
            payload,
        )

    async def _consume_channel_loop(
        self,
        channel_id: str,
        worker_index: int,
    ) -> None:
        """Run one consumer worker for a channel.

        Args:
            channel_id: The channel identifier
            worker_index: Worker index for logging
        """
        q = self._queues.get(channel_id)
        if not q:
            return

        channel = await self.get_channel(channel_id)
        if not channel:
            return

        while True:
            try:
                payload = await q.get()
                key = self._get_session_key(payload)

                # Get per-key lock for session isolation
                key_lock = self._key_locks.setdefault(
                    (channel_id, key),
                    asyncio.Lock(),
                )

                async with key_lock:
                    # Mark session as in-progress
                    self._in_progress.add((channel_id, key))

                    # Drain queue for same-session messages
                    batch = _drain_same_key(q, key, payload)

                try:
                    # Process the batch
                    await self._process_batch(channel, batch)
                finally:
                    # Mark session complete
                    self._in_progress.discard((channel_id, key))

                    # Flush pending messages back to queue
                    pending = self._pending.pop((channel_id, key), [])
                    for p in pending:
                        q.put_nowait(p)

            except asyncio.CancelledError:
                break
            except Exception:
                logger.exception(
                    "channel consume failed: channel=%s worker=%s",
                    channel_id,
                    worker_index,
                )

    async def _process_batch(
        self,
        channel: Channel,
        batch: List[Any],
    ) -> None:
        """Process a batch of messages for a channel.

        Args:
            channel: The channel instance
            batch: List of message payloads
        """
        # For now, process each message individually
        # Subclasses can override to implement merging logic
        for payload in batch:
            await self._process_one(channel, payload)

    async def _process_one(
        self,
        channel: Channel,
        payload: Any,
    ) -> None:
        """Process a single message payload.

        Args:
            channel: The channel instance
            payload: The message payload
        """
        # Default implementation - channels handle processing internally
        # This is called after the message is dequeued
        pass

    async def get_channel(self, channel_id: str) -> Optional[Channel]:
        """Get a channel by ID.

        Args:
            channel_id: The channel identifier

        Returns:
            The channel instance or None
        """
        for ch in self.channels:
            if ch.name == channel_id:
                return ch
        return None

    async def start_all(self) -> None:
        """Start all channels and consumer workers.

        Sets up:
        - Per-channel queues
        - Consumer worker tasks
        - Calls each channel's start method
        """
        self._loop = asyncio.get_running_loop()

        async with self._lock:
            snapshot = list(self.channels)

        # Set up queues and enqueue callbacks
        for ch in snapshot:
            self._queues[ch.name] = asyncio.Queue(
                maxsize=_CHANNEL_QUEUE_MAXSIZE,
            )
            ch.set_enqueue(self._make_enqueue_cb(ch.name))

        # Start consumer workers
        for ch in snapshot:
            if ch.name in self._queues:
                for w in range(_CONSUMER_WORKERS_PER_CHANNEL):
                    task = asyncio.create_task(
                        self._consume_channel_loop(ch.name, w),
                        name=f"channel_consumer_{ch.name}_{w}",
                    )
                    self._consumer_tasks.append(task)

        logger.info(
            "Starting %d channels with %d workers each",
            len(snapshot),
            _CONSUMER_WORKERS_PER_CHANNEL,
        )

        # Call each channel's start method if it has one
        for ch in snapshot:
            try:
                if hasattr(ch, "start") and callable(ch.start):
                    await ch.start()
                elif hasattr(ch, "connect") and callable(ch.connect):
                    # Some channels use connect instead of start
                    await ch.connect()
            except Exception:
                logger.exception(f"Failed to start channel: {ch.name}")

    async def stop_all(self) -> None:
        """Stop all channels and consumer workers gracefully.

        - Clears in-progress and pending state
        - Cancels all consumer tasks
        - Calls each channel's disconnect method
        """
        # Clear session state
        self._in_progress.clear()
        self._pending.clear()

        # Cancel consumer tasks
        for task in self._consumer_tasks:
            task.cancel()

        if self._consumer_tasks:
            await asyncio.gather(*self._consumer_tasks, return_exceptions=True)
        self._consumer_tasks.clear()
        self._queues.clear()

        # Disconnect channels
        async with self._lock:
            snapshot = list(self.channels)

        for ch in snapshot:
            ch.set_enqueue(None)

        async def _disconnect(ch: Channel) -> None:
            try:
                await ch.disconnect()
            except asyncio.CancelledError:
                pass
            except Exception:
                logger.exception(f"Error stopping channel: {ch.name}")

        await asyncio.gather(*[_disconnect(ch) for ch in snapshot])
        logger.info("All channels stopped")

    async def restart_channel(self, channel_id: str) -> bool:
        """Restart a specific channel.

        Args:
            channel_id: The channel identifier

        Returns:
            True if restarted successfully
        """
        channel = await self.get_channel(channel_id)
        if not channel:
            logger.error("Channel not found: %s", channel_id)
            return False

        logger.info("Restarting channel: %s", channel_id)

        try:
            await channel.disconnect()
            await channel.connect()
            logger.info("Channel restarted: %s", channel_id)
            return True
        except Exception as e:
            logger.error("Failed to restart channel %s: %s", channel_id, e)
            return False

    def list_channels(self) -> List[str]:
        """List all managed channel IDs.

        Returns:
            List of channel identifiers
        """
        return [ch.name for ch in self.channels]

    def get_channel_status(self, channel_id: str) -> Dict[str, Any]:
        """Get status information for a channel.

        Args:
            channel_id: The channel identifier

        Returns:
            Status dictionary with queue size, connected status, etc.
        """
        q = self._queues.get(channel_id)
        channel = None
        for ch in self.channels:
            if ch.name == channel_id:
                channel = ch
                break

        return {
            "channel_id": channel_id,
            "connected": channel.is_connected() if channel else False,
            "queue_size": q.qsize() if q else 0,
            "in_progress": sum(
                1 for (cid, _) in self._in_progress if cid == channel_id
            ),
        }
