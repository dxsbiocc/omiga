"""
Main entry point for Omiga Python port.

Wires together all subsystems:
  - Database init & state load
  - Channel connection (stub by default)
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
import json
import logging
import os
import signal
import sys
from pathlib import Path
from typing import Optional

from omiga.channels.base import Channel, StubChannel
from omiga.config import (
    ASSISTANT_NAME,
    GROUPS_DIR,
    HTTP_API_HOST,
    HTTP_API_PORT,
    HTTP_API_TOKEN,
    IDLE_TIMEOUT,
    MAIN_GROUP_FOLDER,
    MAIN_GROUP_JID,
    MAIN_GROUP_NAME,
    MESSAGE_DEBOUNCE_SECONDS,
    POLL_INTERVAL,
    TRIGGER_PATTERN,
    get_secret,
)
from omiga.container.runner import (
    ContainerOutput,
    ensure_image,
    run_container_agent,
    write_groups_snapshot,
    write_tasks_snapshot,
)
from omiga.container.runtime import cleanup_orphans, ensure_container_runtime_running
from omiga.database import (
    close_database,
    delete_registered_group,
    get_all_chats,
    get_all_registered_groups,
    get_all_sessions,
    get_all_tasks,
    get_messages_since,
    get_new_messages,
    get_router_state,
    init_database,
    set_registered_group,
    set_router_state,
    set_session,
    store_chat_metadata,
    store_message,
)
from omiga.group_folder import resolve_group_folder_path
from omiga.group_queue import GroupQueue
from omiga.scheduler.ipc import IpcDeps, start_ipc_watcher
from omiga.models import (
    AvailableGroup,
    ChatInfo,
    ContainerInput,
    NewMessage,
    RegisteredGroup,
)
from omiga.api.admin_commands import handle_admin_command, handle_task_command, is_admin_command
from omiga.api.app import create_app, start_api_server
from omiga.logging_setup import configure_logging
from omiga.router import find_channel, format_messages, format_outbound
from omiga.scheduler.task_scheduler import SchedulerDeps, start_scheduler_loop

configure_logging()
logger = logging.getLogger("omiga.main")

# ---------------------------------------------------------------------------
# Global state (mirrors index.ts module-level vars)
# ---------------------------------------------------------------------------
_last_timestamp: str = ""
_sessions: dict[str, str] = {}
_registered_groups: dict[str, RegisteredGroup] = {}
_last_agent_timestamp: dict[str, str] = {}
_message_loop_running: bool = False

# Cache of all known chats, refreshed at startup.  Used by IPC's refresh_groups.
_all_chats_cache: list[ChatInfo] = []

_channels: list[Channel] = []
_queue: GroupQueue = GroupQueue()

# Debounce: maps chat_jid → monotonic deadline before which we don't start a container.
# Cleared once the container is enqueued.
_debounce_deadlines: dict[str, float] = {}

# Shutdown flag: set by signal handler to stop the message loop cleanly.
_shutdown_event: Optional[asyncio.Event] = None


# ---------------------------------------------------------------------------
# State helpers
# ---------------------------------------------------------------------------

async def _load_state() -> None:
    global _last_timestamp, _last_agent_timestamp, _sessions, _registered_groups, _all_chats_cache

    _last_timestamp = (await get_router_state("last_timestamp")) or ""
    raw_agent_ts = await get_router_state("last_agent_timestamp")
    try:
        _last_agent_timestamp = json.loads(raw_agent_ts) if raw_agent_ts else {}
    except Exception:
        logger.warning("Corrupted last_agent_timestamp in DB, resetting")
        _last_agent_timestamp = {}

    _sessions = await get_all_sessions()
    _registered_groups = await get_all_registered_groups()
    _all_chats_cache = await get_all_chats()
    logger.info("State loaded: %d registered groups, %d known chats", len(_registered_groups), len(_all_chats_cache))


async def _save_state() -> None:
    await set_router_state("last_timestamp", _last_timestamp)
    await set_router_state("last_agent_timestamp", json.dumps(_last_agent_timestamp))


async def _unregister_group(jid: str) -> None:
    """Remove a group from the in-memory map and the DB (folder is kept)."""
    _registered_groups.pop(jid, None)
    await delete_registered_group(jid)
    logger.info("Group unregistered: jid=%s", jid)


async def _register_group(jid: str, group: RegisteredGroup) -> None:
    try:
        group_dir = resolve_group_folder_path(group.folder)
    except ValueError as err:
        logger.warning("Rejecting group registration with invalid folder: jid=%s folder=%s err=%s", jid, group.folder, err)
        return

    _registered_groups[jid] = group
    await set_registered_group(jid, group)

    (group_dir / "logs").mkdir(parents=True, exist_ok=True)
    logger.info("Group registered: jid=%s name=%s folder=%s", jid, group.name, group.folder)


def _get_available_groups() -> list[AvailableGroup]:
    """Return all known group chats from the startup cache.

    The cache is populated by _load_state() and reflects the chats table at
    startup.  Channels may update it via store_chat_metadata; the IPC watcher
    calls this function synchronously so we use the in-memory snapshot.
    """
    registered_jids = set(_registered_groups.keys())
    return [
        AvailableGroup(
            jid=c.jid,
            name=c.name,
            last_activity=c.last_message_time,
            is_registered=c.jid in registered_jids,
        )
        for c in _all_chats_cache
        if c.jid != "__group_sync__" and c.is_group
    ]


# ---------------------------------------------------------------------------
# Agent runner
# ---------------------------------------------------------------------------

_SESSION_CORRUPTION_MARKERS = (
    # OpenAI-compatible APIs: tool message without preceding tool_calls
    "messages with role \"tool\" must be a response to a preceeding message with \"tool_calls\"",
    "messages with role 'tool' must be a response to a preceeding message with 'tool_calls'",
    # Anthropic: similar orphaned tool_result block
    "tool_result block(s) provided when previous message does not have tool_calls",
)


def _is_session_corruption_error(error: str) -> bool:
    """Return True when the error is caused by a corrupted session history."""
    low = (error or "").lower()
    return any(marker.lower() in low for marker in _SESSION_CORRUPTION_MARKERS)


async def _run_agent(
    group: RegisteredGroup,
    prompt: str,
    chat_jid: str,
    on_output=None,
) -> str:
    """Spawn a container agent and return 'success' or 'error'.

    If the run fails with a session-corruption error (orphaned tool messages
    left by a previously interrupted run), the session is cleared automatically
    and the agent is retried once with a fresh session.
    """
    is_main = group.folder == MAIN_GROUP_FOLDER

    # Write tasks/groups snapshots
    all_tasks = await get_all_tasks()
    write_tasks_snapshot(
        group.folder,
        is_main,
        [
            {
                "id": t.id,
                "groupFolder": t.group_folder,
                "prompt": t.prompt,
                "schedule_type": t.schedule_type,
                "schedule_value": t.schedule_value,
                "status": t.status,
                "next_run": t.next_run,
            }
            for t in all_tasks
        ],
    )

    all_chats = await get_all_chats()
    registered_jids = set(_registered_groups.keys())
    available_groups = [
        AvailableGroup(
            jid=c.jid,
            name=c.name,
            last_activity=c.last_message_time,
            is_registered=c.jid in registered_jids,
        )
        for c in all_chats
        if c.jid != "__group_sync__" and c.is_group
    ]
    write_groups_snapshot(group.folder, is_main, available_groups, registered_jids)

    async def _wrapped_on_output(output: ContainerOutput) -> None:
        if output.new_session_id:
            _sessions[group.folder] = output.new_session_id
            await set_session(group.folder, output.new_session_id)
        if on_output:
            await on_output(output)

    for attempt in range(2):  # attempt 0 = normal; attempt 1 = fresh session after corruption
        session_id = _sessions.get(group.folder)
        try:
            container_input = ContainerInput(
                prompt=prompt,
                session_id=session_id,
                group_folder=group.folder,
                chat_jid=chat_jid,
                is_main=is_main,
                assistant_name=ASSISTANT_NAME,
            )
            output = await run_container_agent(
                group,
                container_input,
                lambda proc, name: _queue.register_process(chat_jid, proc, name, group.folder),
                _wrapped_on_output if on_output else None,
            )

            if output.new_session_id:
                _sessions[group.folder] = output.new_session_id
                await set_session(group.folder, output.new_session_id)

            if output.status == "error":
                if attempt == 0 and _is_session_corruption_error(output.error or ""):
                    logger.warning(
                        "Session corruption detected for group=%s — clearing session and retrying",
                        group.name,
                    )
                    _sessions.pop(group.folder, None)
                    await set_session(group.folder, "")
                    continue  # retry with session_id=None
                logger.error("Container agent error: group=%s error=%s", group.name, output.error)
                return "error"

            return "success"

        except Exception as err:
            logger.error("Agent error: group=%s err=%s", group.name, err)
            return "error"

    return "error"


def _effective_trigger(channel: Channel):
    """Return the trigger pattern for *channel*, preferring the channel's own."""
    return channel.trigger_pattern or TRIGGER_PATTERN


async def _process_admin_command(
    chat_jid: str,
    channel,
    missed: list,
) -> bool:
    """Check the last message for an admin command.

    * ``/task`` subcommands are handled for ANY registered group.
    * All other admin commands are restricted to MAIN_GROUP_JID.

    Returns True if a command was handled (caller should skip container).
    """
    last = missed[-1] if missed else None
    if not last or not is_admin_command(last.content):
        return False

    cmd_word = last.content.strip().split()[0].lower() if last.content.strip() else ""

    # /task commands work from any registered group
    if cmd_word == "/task":
        reply = await handle_task_command(
            last.content,
            jid=chat_jid,
            registered_groups=_registered_groups,
        )
        if reply is not None:
            await channel.send_message(chat_jid, reply)
            _last_agent_timestamp[chat_jid] = last.timestamp
            await _save_state()
            return True
        return False

    # All other admin commands: main group only
    if chat_jid != MAIN_GROUP_JID:
        return False

    def _register_fn(jid: str, name: str) -> None:
        import re
        folder = re.sub(r"[^a-z0-9_-]", "_", name.lower())[:32] or "group"
        group = RegisteredGroup(
            name=name,
            folder=folder,
            trigger=f"@{ASSISTANT_NAME}",
            added_at=__import__("datetime").datetime.now(
                __import__("datetime").timezone.utc
            ).isoformat(),
            requires_trigger=True,
        )
        asyncio.ensure_future(_register_group(jid, group))

    def _unregister_fn(jid: str) -> None:
        asyncio.ensure_future(_unregister_group(jid))

    tasks = await get_all_tasks()

    reply = await handle_admin_command(
        last.content,
        channels=_channels,
        registered_groups=_registered_groups,
        get_tasks=lambda: tasks,
        register_group_fn=_register_fn,
        unregister_group_fn=_unregister_fn,
    )

    if reply is not None:
        await channel.send_message(chat_jid, reply)
        _last_agent_timestamp[chat_jid] = last.timestamp
        await _save_state()
        return True

    return False  # unknown command — let container handle


async def _process_group_messages(chat_jid: str) -> bool:
    """Process all pending messages for a group (called by GroupQueue)."""
    group = _registered_groups.get(chat_jid)
    if not group:
        return True

    channel = find_channel(_channels, chat_jid)
    if not channel:
        logger.warning("No channel owns JID, skipping: jid=%s", chat_jid)
        return True

    is_main_group = group.folder == MAIN_GROUP_FOLDER
    since = _last_agent_timestamp.get(chat_jid, "")
    missed = await get_messages_since(chat_jid, since, ASSISTANT_NAME)

    if not missed:
        return True

    # Intercept admin commands (main group only — no container spawn)
    if await _process_admin_command(chat_jid, channel, missed):
        return True

    if not is_main_group and group.requires_trigger is not False:
        pat = _effective_trigger(channel)
        has_trigger = any(pat.match(m.content.strip()) for m in missed)
        if not has_trigger:
            logger.debug(
                "No trigger in pending messages: jid=%s pattern=%r contents=%r",
                chat_jid,
                pat.pattern,
                [m.content[:80] for m in missed],
            )
            return True

    prompt = format_messages(missed)
    previous_cursor = _last_agent_timestamp.get(chat_jid, "")
    _last_agent_timestamp[chat_jid] = missed[-1].timestamp
    await _save_state()

    logger.info("Processing messages: group=%s count=%d", group.name, len(missed))

    idle_timer: Optional[asyncio.TimerHandle] = None
    loop = asyncio.get_event_loop()
    idle_timeout_s = IDLE_TIMEOUT / 1000

    def _reset_idle():
        nonlocal idle_timer
        if idle_timer:
            idle_timer.cancel()
        idle_timer = loop.call_later(idle_timeout_s, lambda: _queue.close_stdin(chat_jid))

    await channel.set_typing(chat_jid, True)

    had_error = False
    output_sent = False

    async def _on_output(result: ContainerOutput) -> None:
        nonlocal had_error, output_sent
        if result.result:
            raw = result.result if isinstance(result.result, str) else json.dumps(result.result)
            text = format_outbound(raw)
            logger.info("Agent output: group=%s preview=%s", group.name, raw[:200])
            if text:
                await channel.send_message(chat_jid, text)
                output_sent = True
            _reset_idle()
        if result.status == "success":
            _queue.notify_idle(chat_jid)
        if result.status == "error":
            had_error = True

    status = await _run_agent(group, prompt, chat_jid, _on_output)

    await channel.set_typing(chat_jid, False)
    if idle_timer:
        idle_timer.cancel()

    if status == "error" or had_error:
        if output_sent:
            logger.warning(
                "Agent error after output sent, skipping cursor rollback: group=%s", group.name
            )
            return True
        _last_agent_timestamp[chat_jid] = previous_cursor
        await _save_state()
        logger.warning("Agent error, rolled back cursor: group=%s", group.name)
        await _notify_error(group, chat_jid)
        return False

    return True


# ---------------------------------------------------------------------------
# Message loop
# ---------------------------------------------------------------------------

async def _start_message_loop() -> None:
    global _message_loop_running, _last_timestamp
    if _message_loop_running:
        logger.debug("Message loop already running")
        return
    _message_loop_running = True

    triggers = [
        ch.trigger_pattern.pattern if ch.trigger_pattern else f"@{ASSISTANT_NAME}"
        for ch in _channels
    ]
    logger.info("Omiga running (triggers: %s)", ", ".join(triggers))

    while _shutdown_event is None or not _shutdown_event.is_set():
        try:
            # Check debounce deadlines first — they expire even when there are no
            # new messages, because _last_timestamp was already advanced on the
            # poll that originally detected the messages.
            now = asyncio.get_event_loop().time()
            expired_jids = [jid for jid, dl in list(_debounce_deadlines.items()) if now >= dl]
            for chat_jid in expired_jids:
                del _debounce_deadlines[chat_jid]
                _queue.enqueue_message_check(chat_jid)
                logger.debug("Debounce expired, enqueueing: jid=%s", chat_jid)

            jids = list(_registered_groups.keys())
            messages, new_ts = await get_new_messages(jids, _last_timestamp, ASSISTANT_NAME)

            if messages:
                logger.info("New messages: count=%d", len(messages))
                _last_timestamp = new_ts
                await _save_state()

                by_group: dict[str, list[NewMessage]] = {}
                for msg in messages:
                    by_group.setdefault(msg.chat_jid, []).append(msg)

                for chat_jid, group_messages in by_group.items():
                    group = _registered_groups.get(chat_jid)
                    if not group:
                        continue

                    channel = find_channel(_channels, chat_jid)
                    if not channel:
                        logger.warning("No channel owns JID: %s", chat_jid)
                        continue

                    is_main_group = group.folder == MAIN_GROUP_FOLDER
                    needs_trigger = not is_main_group and group.requires_trigger is not False

                    if needs_trigger:
                        pat = _effective_trigger(channel)
                        has_trigger = any(
                            pat.match(m.content.strip()) for m in group_messages
                        )
                        if not has_trigger:
                            logger.debug(
                                "No trigger in messages: jid=%s pattern=%r contents=%r",
                                chat_jid,
                                pat.pattern,
                                [m.content[:80] for m in group_messages],
                            )
                            continue

                    all_pending = await get_messages_since(
                        chat_jid, _last_agent_timestamp.get(chat_jid, ""), ASSISTANT_NAME
                    )
                    msgs_to_send = all_pending if all_pending else group_messages
                    formatted = format_messages(msgs_to_send)

                    if _queue.send_message(chat_jid, formatted):
                        # Container already running — pipe messages in directly.
                        # No debounce needed; the active container handles batching.
                        _debounce_deadlines.pop(chat_jid, None)
                        logger.debug(
                            "Piped messages to active container: jid=%s count=%d",
                            chat_jid,
                            len(msgs_to_send),
                        )
                        _last_agent_timestamp[chat_jid] = msgs_to_send[-1].timestamp
                        await _save_state()
                        asyncio.ensure_future(
                            _typing_safe(channel, chat_jid, True)
                        )
                    else:
                        # No active container — start debounce window.
                        # Expiry is checked at the top of each poll loop iteration,
                        # independently of whether new messages arrive.
                        if chat_jid not in _debounce_deadlines:
                            _debounce_deadlines[chat_jid] = asyncio.get_event_loop().time() + MESSAGE_DEBOUNCE_SECONDS
                            logger.debug(
                                "Debounce started: jid=%s window=%.1fs",
                                chat_jid, MESSAGE_DEBOUNCE_SECONDS,
                            )

        except Exception as err:
            logger.error("Error in message loop: %s", err)

        await asyncio.sleep(POLL_INTERVAL)


async def _typing_safe(channel: Channel, jid: str, typing: bool) -> None:
    try:
        await channel.set_typing(jid, typing)
    except Exception as err:
        logger.warning("Failed to set typing indicator: jid=%s err=%s", jid, err)


def _recover_pending_messages() -> None:
    """Startup recovery: re-enqueue groups that likely have unprocessed messages.

    A group is considered pending when its per-group agent cursor is behind
    the global last_timestamp cursor, meaning it may have received messages
    that were not yet delivered to a container agent.
    """
    for chat_jid in _registered_groups:
        agent_ts = _last_agent_timestamp.get(chat_jid, "")
        if agent_ts < _last_timestamp:
            logger.debug("Recovering pending messages for jid=%s (agent_ts=%s < global_ts=%s)", chat_jid, agent_ts, _last_timestamp)
            _queue.enqueue_message_check(chat_jid)


# ---------------------------------------------------------------------------
# Channel factory
# ---------------------------------------------------------------------------

def _resolve_proxy(channel_env_key: str) -> str:
    """Return the proxy URL to use for a channel.

    Priority:
      1. Channel-specific env var  (e.g. TELEGRAM_HTTP_PROXY)
      2. System HTTPS_PROXY / ALL_PROXY / HTTP_PROXY env vars
      3. Empty string (direct connection)

    This lets users rely on a system-wide proxy (set once in the shell or
    via a VPN tool) and only override per-channel when needed.
    """
    explicit = get_secret(channel_env_key)
    if explicit:
        return explicit
    return (
        os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy")
        or os.environ.get("ALL_PROXY") or os.environ.get("all_proxy")
        or os.environ.get("HTTP_PROXY") or os.environ.get("http_proxy")
        or ""
    )


async def _build_channels(
    on_message,
    on_chat_meta,
) -> list[Channel]:
    """Instantiate and connect channels based on environment config.

    Priority:
      1. Telegram  — when TELEGRAM_BOT_TOKEN is set
      2. Stub      — always added last as catch-all (useful for testing)

    Returns the connected channel list.
    """
    channels: list[Channel] = []

    telegram_token = get_secret("TELEGRAM_BOT_TOKEN")
    if telegram_token:
        from omiga.channels.telegram import TelegramChannel
        tg = TelegramChannel(
            token=telegram_token,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: _registered_groups,
            http_proxy=_resolve_proxy("TELEGRAM_HTTP_PROXY"),
        )
        await tg.connect()
        channels.append(tg)
        logger.info("Telegram channel active")

    feishu_app_id = get_secret("FEISHU_APP_ID")
    feishu_app_secret = get_secret("FEISHU_APP_SECRET")
    if feishu_app_id and feishu_app_secret:
        from omiga.channels.feishu import FeishuChannel
        fs = FeishuChannel(
            app_id=feishu_app_id,
            app_secret=feishu_app_secret,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: _registered_groups,
        )
        await fs.connect()
        channels.append(fs)
        logger.info("Feishu channel active")

    qq_app_id = get_secret("QQ_APP_ID")
    qq_app_secret = get_secret("QQ_APP_SECRET")
    if qq_app_id and qq_app_secret:
        from omiga.channels.qq import QQChannel
        qq = QQChannel(
            app_id=qq_app_id,
            app_secret=qq_app_secret,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: _registered_groups,
        )
        await qq.connect()
        channels.append(qq)
        logger.info("QQ channel active")

    discord_token = get_secret("DISCORD_BOT_TOKEN")
    if discord_token:
        from omiga.channels.discord_ import DiscordChannel
        dc = DiscordChannel(
            token=discord_token,
            on_message=on_message,
            on_chat_meta=on_chat_meta,
            registered_groups=lambda: _registered_groups,
            http_proxy=_resolve_proxy("DISCORD_HTTP_PROXY"),
        )
        await dc.connect()
        channels.append(dc)
        logger.info("Discord channel active")

    if not channels:
        logger.info("No channel tokens set — using StubChannel")
        stub = StubChannel()
        await stub.connect()
        channels.append(stub)

    return channels


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

async def _bootstrap_main_group() -> None:
    """Auto-register the main group at startup if MAIN_GROUP_JID is configured.

    The main group (folder="main") never requires a trigger word — every
    message is forwarded directly to the agent.  This is the right setting
    for a personal private chat or a dedicated bot channel.

    Registration is skipped when:
      - MAIN_GROUP_JID is not set in .env
      - The JID is already registered (any folder)
      - A group with folder="main" already exists
    """
    if not MAIN_GROUP_JID:
        return

    # Already registered?
    if MAIN_GROUP_JID in _registered_groups:
        return
    if any(g.folder == MAIN_GROUP_FOLDER for g in _registered_groups.values()):
        return

    group = RegisteredGroup(
        name=MAIN_GROUP_NAME,
        folder=MAIN_GROUP_FOLDER,
        trigger=f"@{ASSISTANT_NAME}",
        added_at=__import__("datetime").datetime.now(
            __import__("datetime").timezone.utc
        ).isoformat(),
        requires_trigger=False,  # main group never needs a trigger word
    )
    await _register_group(MAIN_GROUP_JID, group)
    logger.info(
        "Main group auto-registered: jid=%s name=%s (no trigger word required)",
        MAIN_GROUP_JID,
        MAIN_GROUP_NAME,
    )


def _bootstrap_profile() -> None:
    """Create a PROFILE.md template in groups/global/ on first run.

    This file is mounted read-write into every container so the agent can
    update it as it learns about the user.  On first run it contains only
    placeholder values; the agent will fill them in during the first
    conversation (bootstrap flow).
    """
    global_dir = GROUPS_DIR / "global"
    global_dir.mkdir(parents=True, exist_ok=True)
    profile_path = global_dir / "PROFILE.md"
    if profile_path.exists():
        return  # already bootstrapped

    profile_path.write_text(
        """\
# User Profile

> This file is maintained by the AI assistant. It is updated automatically
> as the assistant learns more about you. You can also edit it directly.

## Basic Info

- **Name**: (not yet known)
- **Timezone**: (not yet known)
- **Language preference**: (not yet known)
- **Preferred reply language**: (not yet known)

## Preferences

- **Reply style**: (not yet known — concise / detailed / casual / formal)
- **Programming languages**: (not yet known)
- **Favorite tools / stack**: (not yet known)

## Ongoing Projects

(none recorded yet)

## Interests & Background

(none recorded yet)

## Notes

(none recorded yet)
""",
        encoding="utf-8",
    )
    logger.info(
        "First run: created PROFILE.md template at %s — "
        "the assistant will fill it in during the first conversation.",
        profile_path,
    )


async def _main_async() -> None:
    global _shutdown_event
    _shutdown_event = asyncio.Event()

    # Container runtime
    ensure_container_runtime_running()
    cleanup_orphans()
    await ensure_image()  # auto-build image if missing

    # Bootstrap global user profile on first run
    _bootstrap_profile()

    # Database
    await init_database()
    logger.info("Database initialized")
    await _load_state()

    # Auto-register the main group if configured via MAIN_GROUP_JID
    await _bootstrap_main_group()

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
        assert _shutdown_event is not None
        _shutdown_event.set()
        await _queue.shutdown(10000)
        for ch in _channels:
            try:
                await ch.disconnect()
            except Exception as exc:
                logger.debug("Channel disconnect error (ignored): %s", exc)
        # Cancel background asyncio tasks except this one and the main task.
        # The main task is excluded because it will be awaiting this coroutine
        # after the message loop exits — cancelling it would prevent the await
        # from completing and would leave the event loop open.
        current = asyncio.current_task()
        for task in asyncio.all_tasks():
            if task is not current and task is not _main_task_ref and not task.done():
                task.cancel()
        # Give OS-level threads (e.g. discord.py heartbeat) a moment to exit
        # after client.close() has been called above.  Without this the event
        # loop closes while the heartbeat thread is still alive, causing
        # "RuntimeError: Event loop is closed" noise on exit.
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

    # Build channel list from environment
    _channels.extend(await _build_channels(_on_message, _on_chat_meta))

    # Wire GroupQueue
    _queue.set_process_messages_fn(_process_group_messages)

    # Start subsystems
    start_scheduler_loop(
        SchedulerDeps(
            registered_groups=lambda: _registered_groups,
            get_sessions=lambda: _sessions,
            queue=_queue,
            on_process=lambda jid, proc, name, folder: _queue.register_process(
                jid, proc, name, folder
            ),
            send_message=lambda jid, text: _send_to_channel(jid, text),
        )
    )

    ipc_deps = IpcDeps(
        send_message=lambda jid, text: _send_to_channel(jid, text),
        registered_groups=lambda: _registered_groups,
        register_group=lambda jid, grp: asyncio.ensure_future(_register_group(jid, grp)),
        sync_group_metadata=lambda force: asyncio.sleep(0),  # no-op for stub
        get_available_groups=_get_available_groups,
        write_groups_snapshot=write_groups_snapshot,
    )
    start_ipc_watcher(ipc_deps)

    _recover_pending_messages()

    # Start background health monitor for channel reconnection
    asyncio.create_task(_channel_health_monitor(), name="channel-health-monitor")

    # Start HTTP API server (if enabled)
    if HTTP_API_PORT > 0:
        import re as _re

        async def _api_register(jid: str, name: str, requires_trigger: bool = True) -> None:
            folder = _re.sub(r"[^a-z0-9_-]", "_", name.lower())[:32] or "group"
            from datetime import datetime as _dt, timezone as _tz
            group = RegisteredGroup(
                name=name,
                folder=folder,
                trigger=f"@{ASSISTANT_NAME}",
                added_at=_dt.now(_tz.utc).isoformat(),
                requires_trigger=requires_trigger,
            )
            await _register_group(jid, group)

        api_app = create_app(
            channels_fn=lambda: _channels,
            registered_groups_fn=lambda: _registered_groups,
            all_chats_fn=get_all_chats,
            get_tasks_fn=get_all_tasks,
            run_task_fn=lambda task_id: _queue.enqueue_message_check(task_id),
            register_group_fn=_api_register,
            unregister_group_fn=_unregister_group,
            groups_dir=GROUPS_DIR,
            api_token=HTTP_API_TOKEN,
        )
        await start_api_server(api_app, port=HTTP_API_PORT, host=HTTP_API_HOST)

    await _start_message_loop()

    # The message loop exits as soon as _shutdown_event is set, but
    # _do_shutdown() may still be running (closing channels, waiting for the
    # Discord heartbeat thread, closing the database).  Await it here so that
    # _main_async() doesn't return — and asyncio.run() doesn't close the event
    # loop — until the full shutdown sequence has completed.
    if _shutdown_task_ref is not None and not _shutdown_task_ref.done():
        try:
            await _shutdown_task_ref
        except (asyncio.CancelledError, Exception):
            pass


async def _notify_error(group: RegisteredGroup, chat_jid: str) -> None:
    """Send a brief error notification to the main group when a container fails.

    Only fires when:
    - MAIN_GROUP_JID is configured
    - The failing chat is NOT the main group itself (avoid loops)
    - The main group channel is currently connected
    """
    if not MAIN_GROUP_JID or chat_jid == MAIN_GROUP_JID:
        return
    main_ch = find_channel(_channels, MAIN_GROUP_JID)
    if not main_ch or not main_ch.is_connected():
        return
    from datetime import datetime, timezone
    ts = datetime.now(timezone.utc).strftime("%H:%M")
    text = f"[Omiga] Container error in group '{group.name}' at {ts} UTC — check logs"
    try:
        await main_ch.send_message(MAIN_GROUP_JID, text)
    except Exception as exc:
        logger.error("Failed to send error notification to main group: %s", exc)


async def _channel_health_monitor() -> None:
    """Background task: check channel liveness every 30 s and reconnect if needed."""
    while True:
        await asyncio.sleep(30)
        for ch in _channels:
            try:
                if not ch.is_connected():
                    logger.warning(
                        "Channel '%s' appears disconnected — attempting reconnect", ch.name
                    )
                    await ch.reconnect()
            except Exception as exc:
                logger.error("Channel '%s' reconnect error: %s", ch.name, exc)


async def _send_to_channel(jid: str, raw_text: str) -> None:
    channel = find_channel(_channels, jid)
    if not channel:
        logger.warning("No channel for JID: %s", jid)
        return
    text = format_outbound(raw_text)
    if text:
        await channel.send_message(jid, text)


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
