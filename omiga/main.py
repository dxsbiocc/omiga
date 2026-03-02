"""Main entry point for Omiga.

Wires together all subsystems:
  - Database init & state load
  - Channel connection
  - GroupQueue
  - IPC watcher
  - Scheduler loop
  - Message poll loop
  - Graceful shutdown on SIGTERM/SIGINT

Run with:
    python -m omiga
    # or
    uv run omiga
"""
from __future__ import annotations

import asyncio
import logging
import re as _re
import signal
import sys
from datetime import datetime as _dt, timezone as _tz
from typing import Optional

import omiga.state as state
from omiga.api.app import create_app, start_api_server
from omiga.bootstrap import bootstrap_main_group, bootstrap_profile
from omiga.channel_setup import build_channel_manager
from omiga.channels.manager import ChannelManager
from omiga.config import (
    ASSISTANT_NAME,
    GROUPS_DIR,
    HTTP_API_HOST,
    HTTP_API_PORT,
    HTTP_API_TOKEN,
    MAIN_GROUP_JID,
)
from omiga.container.runner import ensure_image, write_groups_snapshot
from omiga.container.runtime import cleanup_orphans, ensure_container_runtime_running
from omiga.database import (
    close_database,
    get_all_chats,
    get_all_tasks,
    init_database,
    store_chat_metadata,
    store_message,
)
from omiga.logging_setup import configure_logging
from omiga.message_loop import recover_pending_messages, start_message_loop
from omiga.models import NewMessage, RegisteredGroup
from omiga.router import find_channel, format_outbound
from omiga.scheduler.ipc import IpcDeps, start_ipc_watcher, stop_ipc_watcher
from omiga.scheduler.task_scheduler import SchedulerDeps, start_scheduler_loop, stop_scheduler

configure_logging()
logger = logging.getLogger("omiga.main")


async def _channel_health_monitor(channel_manager: ChannelManager) -> None:
    """Background task: check channel liveness every 30 s and reconnect if needed."""
    while True:
        await asyncio.sleep(30)
        for channel_id in channel_manager.list_channels():
            status = channel_manager.get_channel_status(channel_id)
            if not status.get("connected", False):
                logger.warning(
                    "Channel '%s' appears disconnected — attempting reconnect", channel_id
                )
                await channel_manager.restart_channel(channel_id)


async def _send_to_channel(jid: str, raw_text: str) -> None:
    """Send a message to a channel via the ChannelManager."""
    # Find the channel that owns this JID
    channel_manager = state._channel_manager
    if not channel_manager:
        logger.warning("ChannelManager not initialized")
        return

    # Find channel by JID ownership
    target_channel = None
    for ch in channel_manager.channels:
        if ch.owns_jid(jid):
            target_channel = ch
            break

    if not target_channel:
        logger.warning("No channel owns JID: %s", jid)
        return

    text = format_outbound(raw_text)
    if text:
        await target_channel.send_message(jid, text)


async def _main_async() -> None:
    state._shutdown_event = asyncio.Event()

    # Container runtime
    ensure_container_runtime_running()
    cleanup_orphans()
    await ensure_image()  # auto-build image if missing

    # Bootstrap global user profile on first run
    bootstrap_profile()

    # Database
    await init_database()
    logger.info("Database initialized")
    await state.load_state()

    # Auto-register the main group if configured via MAIN_GROUP_JID
    await bootstrap_main_group()

    # Shutdown handler: set event to break the message loop, then clean up
    loop = asyncio.get_event_loop()
    _shutdown_in_progress = False
    _shutdown_task_ref: Optional[asyncio.Task] = None
    # The task wrapping _main_async itself — excluded from bulk-cancellation so
    # that we can await _shutdown_task_ref after the message loop exits.
    _main_task_ref = asyncio.current_task()

    def _signal_handler(sig_name: str) -> None:
        nonlocal _shutdown_in_progress, _shutdown_task_ref
        if _shutdown_in_progress:
            return  # ignore duplicate signals
        _shutdown_in_progress = True
        _shutdown_task_ref = asyncio.ensure_future(_do_shutdown(sig_name))

    async def _do_shutdown(sig_name: str) -> None:
        logger.info("Shutdown signal received: %s — stopping gracefully", sig_name)
        assert state._shutdown_event is not None
        state._shutdown_event.set()
        stop_ipc_watcher()
        stop_scheduler()  # Stop APScheduler
        await state._queue.shutdown(10000)

        # Stop channel manager (handles all channels)
        if state._channel_manager:
            try:
                await state._channel_manager.stop_all()
            except Exception as exc:
                logger.error("Error stopping channel manager: %s", exc)

        # Cancel background asyncio tasks except this one and the main task.
        current = asyncio.current_task()
        for task in asyncio.all_tasks():
            if task is not current and task is not _main_task_ref and not task.done():
                task.cancel()
        # Give OS-level threads a moment to exit after client.close() has been called.
        await asyncio.sleep(1.0)
        await close_database()

    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(
            sig,
            lambda s=sig: _signal_handler(s.name),
        )

    # Shared inbound callbacks used by all channels
    async def _on_message(chat_jid: str, msg: NewMessage) -> None:
        await store_message(msg)

    async def _on_chat_meta(
        chat_jid: str,
        timestamp: str,
        name=None,
        channel_name=None,
        is_group=None,
    ) -> None:
        await store_chat_metadata(chat_jid, timestamp, name, channel_name, is_group)

    # Build channel manager with all channels
    state._channel_manager = await build_channel_manager(_on_message, _on_chat_meta)

    # Also populate state._channels for backwards compatibility
    state._channels.extend(state._channel_manager.channels)

    # Wire GroupQueue
    from omiga.processing import process_group_messages
    state._queue.set_process_messages_fn(process_group_messages)

    # Start channel manager (starts all channels and consumer workers)
    await state._channel_manager.start_all()

    # Start subsystems
    start_scheduler_loop(
        SchedulerDeps(
            registered_groups=lambda: state._registered_groups,
            get_sessions=lambda: state._sessions,
            queue=state._queue,
            on_process=lambda jid, proc, name, folder: state._queue.register_process(
                jid, proc, name, folder
            ),
            send_message=lambda jid, text: _send_to_channel(jid, text),
        )
    )

    ipc_deps = IpcDeps(
        send_message=lambda jid, text: _send_to_channel(jid, text),
        registered_groups=lambda: state._registered_groups,
        register_group=lambda jid, grp: asyncio.ensure_future(state.register_group(jid, grp)),
        sync_group_metadata=lambda force: asyncio.sleep(0),  # no-op for stub
        get_available_groups=state.get_available_groups,
        write_groups_snapshot=write_groups_snapshot,
    )
    start_ipc_watcher(ipc_deps)

    recover_pending_messages()

    # Start background health monitor for channel reconnection
    asyncio.create_task(
        _channel_health_monitor(state._channel_manager),
        name="channel-health-monitor",
    )

    # Start HTTP API server (if enabled)
    if HTTP_API_PORT > 0:
        async def _api_register(jid: str, name: str, requires_trigger: bool = True) -> None:
            folder = _re.sub(r"[^a-z0-9_-]", "_", name.lower())[:32] or "group"
            group = RegisteredGroup(
                name=name,
                folder=folder,
                trigger=f"@{ASSISTANT_NAME}",
                added_at=_dt.now(_tz.utc).isoformat(),
                requires_trigger=requires_trigger,
            )
            await state.register_group(jid, group)

        api_app = create_app(
            channels_fn=lambda: state._channels,
            registered_groups_fn=lambda: state._registered_groups,
            all_chats_fn=get_all_chats,
            get_tasks_fn=get_all_tasks,
            run_task_fn=lambda task_id: state._queue.enqueue_message_check(task_id),
            register_group_fn=_api_register,
            unregister_group_fn=state.unregister_group,
            groups_dir=GROUPS_DIR,
            api_token=HTTP_API_TOKEN,
        )
        await start_api_server(api_app, port=HTTP_API_PORT, host=HTTP_API_HOST)

    await start_message_loop()

    # Await the shutdown task so _main_async() doesn't return — and
    # asyncio.run() doesn't close the event loop — until the full shutdown
    # sequence has completed.
    if _shutdown_task_ref is not None and not _shutdown_task_ref.done():
        try:
            await _shutdown_task_ref
        except (asyncio.CancelledError, Exception):
            pass


def main() -> None:
    try:
        asyncio.run(_main_async())
    except (KeyboardInterrupt, asyncio.CancelledError):
        pass
    except Exception as err:
        logger.critical("Failed to start Omiga: %s", err)
        sys.exit(1)


if __name__ == "__main__":
    main()
