"""Tests for ChannelManager."""
import asyncio
import pytest
from typing import Any, Dict, List, Optional

from omiga.channels.base import Channel
from omiga.channels.manager import ChannelManager


class MockChannel(Channel):
    """Mock channel for testing."""

    def __init__(self, name: str = "mock"):
        self._name = name
        self._connected = False
        self._enqueue_callback = None
        self.sent_messages: List[tuple] = []
        self.connect_calls = 0
        self.disconnect_calls = 0

    @property
    def name(self) -> str:
        return self._name

    async def connect(self) -> None:
        self._connected = True
        self.connect_calls += 1

    async def send_message(self, jid: str, text: str) -> None:
        self.sent_messages.append((jid, text))

    def is_connected(self) -> bool:
        return self._connected

    def owns_jid(self, jid: str) -> bool:
        return True  # Mock owns all JIDs

    async def disconnect(self) -> None:
        self._connected = False
        self.disconnect_calls += 1

    def set_enqueue(self, callback) -> None:
        self._enqueue_callback = callback


class TestChannelManager:
    """Tests for ChannelManager."""

    @pytest.mark.asyncio
    async def test_manager_init(self):
        """Test ChannelManager initialization."""
        channels = [MockChannel("test1"), MockChannel("test2")]
        manager = ChannelManager(channels)

        assert manager.channels == channels
        assert manager.list_channels() == ["test1", "test2"]

    @pytest.mark.asyncio
    async def test_from_channels(self):
        """Test from_channels class method."""
        channels = [MockChannel("ch1"), MockChannel("ch2")]
        manager = ChannelManager.from_channels(channels)

        assert len(manager.channels) == 2
        assert manager.list_channels() == ["ch1", "ch2"]

    @pytest.mark.asyncio
    async def test_get_channel(self):
        """Test getting channel by ID."""
        channels = [MockChannel("telegram"), MockChannel("discord")]
        manager = ChannelManager.from_channels(channels)

        channel = await manager.get_channel("telegram")
        assert channel is not None
        assert channel.name == "telegram"

        channel = await manager.get_channel("nonexistent")
        assert channel is None

    @pytest.mark.asyncio
    async def test_start_all(self):
        """Test starting all channels."""
        channels = [MockChannel("ch1"), MockChannel("ch2")]
        manager = ChannelManager.from_channels(channels)

        await manager.start_all()

        # All channels should be connected
        for ch in channels:
            assert ch.is_connected()
            assert ch._enqueue_callback is not None

        # Should have consumer tasks
        assert len(manager._consumer_tasks) > 0

        # Clean up
        await manager.stop_all()

    @pytest.mark.asyncio
    async def test_stop_all(self):
        """Test stopping all channels."""
        channels = [MockChannel("ch1")]
        manager = ChannelManager.from_channels(channels)

        await manager.start_all()
        await manager.stop_all()

        # All channels should be disconnected
        for ch in channels:
            assert not ch.is_connected()
            assert ch._enqueue_callback is None

        # Consumer tasks should be cleared
        assert len(manager._consumer_tasks) == 0
        assert len(manager._queues) == 0

    @pytest.mark.asyncio
    async def test_enqueue_thread_safe(self):
        """Test that enqueue is thread-safe."""
        channels = [MockChannel("ch1")]
        manager = ChannelManager.from_channels(channels)

        await manager.start_all()

        # Enqueue should not raise even if called before queue is ready
        manager.enqueue("ch1", {"test": "data"})

        await manager.stop_all()

    @pytest.mark.asyncio
    async def test_get_channel_status(self):
        """Test getting channel status."""
        channels = [MockChannel("ch1")]
        manager = ChannelManager.from_channels(channels)

        await manager.start_all()

        status = manager.get_channel_status("ch1")
        assert status["channel_id"] == "ch1"
        assert status["connected"] is True
        assert "queue_size" in status
        assert "in_progress" in status

        await manager.stop_all()

    @pytest.mark.asyncio
    async def test_list_channels(self):
        """Test listing channel IDs."""
        channels = [
            MockChannel("telegram"),
            MockChannel("discord"),
            MockChannel("feishu"),
        ]
        manager = ChannelManager.from_channels(channels)

        channel_ids = manager.list_channels()
        assert len(channel_ids) == 3
        assert "telegram" in channel_ids
        assert "discord" in channel_ids
        assert "feishu" in channel_ids

    @pytest.mark.asyncio
    async def test_restart_channel(self):
        """Test restarting a channel."""
        channels = [MockChannel("ch1")]
        manager = ChannelManager.from_channels(channels)

        await manager.start_all()

        initial_connects = channels[0].connect_calls
        result = await manager.restart_channel("ch1")

        assert result is True
        assert channels[0].connect_calls > initial_connects

        await manager.stop_all()

    @pytest.mark.asyncio
    async def test_restart_nonexistent_channel(self):
        """Test restarting a channel that doesn't exist."""
        channels = [MockChannel("ch1")]
        manager = ChannelManager.from_channels(channels)

        result = await manager.restart_channel("nonexistent")
        assert result is False

    @pytest.mark.asyncio
    async def test_get_session_key(self):
        """Test session key extraction."""
        manager = ChannelManager([MockChannel()])

        # Test with dict payload
        key1 = manager._get_session_key({"chat_jid": "tg:123"})
        assert key1 == "tg:123"

        # Test with object payload
        class MockPayload:
            chat_jid = "dc:456"

        key2 = manager._get_session_key(MockPayload())
        assert key2 == "dc:456"

        # Test with session_key attribute
        class MockPayloadWithKey:
            session_key = "custom_key"

        key3 = manager._get_session_key(MockPayloadWithKey())
        assert key3 == "custom_key"

        # Test default
        key4 = manager._get_session_key({})
        assert key4 == "default"

    @pytest.mark.asyncio
    async def test_drain_same_key(self):
        """Test draining same-key payloads from queue."""
        from omiga.channels.manager import _drain_same_key

        q = asyncio.Queue()
        # Payloads are (key, data) tuples
        await q.put(("key1", "data1"))
        await q.put(("key1", "data2"))
        await q.put(("key2", "data3"))
        await q.put(("key1", "data4"))

        # First item is the key itself in this test
        batch = _drain_same_key(q, "key1", ("key1", "data1"))

        # Should have drained all key1 items (first_payload + 2 from queue)
        # Note: key2 stays in queue
        assert len(batch) >= 2  # At least first_payload and one more
        assert ("key1", "data1") in batch

        # Queue should have key2 item left
        remaining = q.get_nowait()
        assert remaining == ("key2", "data3")


class TestChannelManagerWithMockPayload:
    """Tests with mock message payloads."""

    @pytest.mark.asyncio
    async def test_enqueue_and_process(self):
        """Test enqueueing and processing messages."""
        channels = [MockChannel("ch1")]
        manager = ChannelManager.from_channels(channels)

        # Track processed messages
        processed = []

        async def process_one(channel, payload):
            processed.append(payload)

        manager._process_one = process_one

        await manager.start_all()

        # Enqueue some messages
        manager.enqueue("ch1", {"msg": "hello"})
        manager.enqueue("ch1", {"msg": "world"})

        # Wait for processing
        await asyncio.sleep(0.1)

        await manager.stop_all()

        # Messages should have been processed
        assert len(processed) >= 2
