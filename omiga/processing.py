"""Message processing pipeline for Omiga.

Handles admin command dispatch, per-group message processing, and error
notifications sent back to the main group.
"""
from __future__ import annotations

import asyncio
import json
import logging
from datetime import datetime, timezone
from typing import Optional

import omiga.state as state
from omiga.agent import effective_trigger, run_agent
from omiga.api.admin_commands import handle_admin_command, handle_task_command, is_admin_command
from omiga.config import ASSISTANT_NAME, IDLE_TIMEOUT, MAIN_GROUP_FOLDER, MAIN_GROUP_JID
from omiga.container.runner import ContainerOutput
from omiga.database import get_all_tasks, get_messages_since
from omiga.group_folder import resolve_group_folder_path
from omiga.models import RegisteredGroup
from omiga.router import find_channel, format_messages, format_outbound, parse_file_directives

logger = logging.getLogger("omiga.processing")


async def notify_error(group: RegisteredGroup, chat_jid: str) -> None:
    """Send a brief error notification to the main group when a container fails.

    Only fires when:
    - MAIN_GROUP_JID is configured
    - The failing chat is NOT the main group itself (avoid loops)
    - The main group channel is currently connected
    """
    if not MAIN_GROUP_JID or chat_jid == MAIN_GROUP_JID:
        return
    main_ch = find_channel(state._channels, MAIN_GROUP_JID)
    if not main_ch or not main_ch.is_connected():
        return
    ts = datetime.now(timezone.utc).strftime("%H:%M")
    text = f"[Omiga] Container error in group '{group.name}' at {ts} UTC — check logs"
    try:
        await main_ch.send_message(MAIN_GROUP_JID, text)
    except Exception as exc:
        logger.error("Failed to send error notification to main group: %s", exc)


async def process_admin_command(
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
            registered_groups=state._registered_groups,
        )
        if reply is not None:
            await channel.send_message(chat_jid, reply)
            state._last_agent_timestamp[chat_jid] = last.timestamp
            await state.save_state()
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
        asyncio.ensure_future(state.register_group(jid, group))

    def _unregister_fn(jid: str) -> None:
        asyncio.ensure_future(state.unregister_group(jid))

    tasks = await get_all_tasks()

    reply = await handle_admin_command(
        last.content,
        channels=state._channels,
        registered_groups=state._registered_groups,
        get_tasks=lambda: tasks,
        register_group_fn=_register_fn,
        unregister_group_fn=_unregister_fn,
    )

    if reply is not None:
        await channel.send_message(chat_jid, reply)
        state._last_agent_timestamp[chat_jid] = last.timestamp
        await state.save_state()
        return True

    return False  # unknown command — let container handle


async def process_group_messages(chat_jid: str) -> bool:
    """Process all pending messages for a group (called by GroupQueue)."""
    group = state._registered_groups.get(chat_jid)
    if not group:
        return True

    channel = find_channel(state._channels, chat_jid)
    if not channel:
        logger.warning("No channel owns JID, skipping: jid=%s", chat_jid)
        return True

    is_main_group = group.folder == MAIN_GROUP_FOLDER
    since = state._last_agent_timestamp.get(chat_jid, "")
    missed = await get_messages_since(chat_jid, since, ASSISTANT_NAME)

    if not missed:
        return True

    # Intercept admin commands (main group only — no container spawn)
    if await process_admin_command(chat_jid, channel, missed):
        return True

    if not is_main_group and group.requires_trigger is not False:
        pat = effective_trigger(channel)
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
    previous_cursor = state._last_agent_timestamp.get(chat_jid, "")
    state._last_agent_timestamp[chat_jid] = missed[-1].timestamp
    await state.save_state()

    logger.info("Processing messages: group=%s count=%d", group.name, len(missed))

    idle_timer: Optional[asyncio.TimerHandle] = None
    loop = asyncio.get_event_loop()
    idle_timeout_s = IDLE_TIMEOUT / 1000

    def _reset_idle():
        nonlocal idle_timer
        if idle_timer:
            idle_timer.cancel()
        idle_timer = loop.call_later(idle_timeout_s, lambda: state._queue.close_stdin(chat_jid))

    await channel.set_typing(chat_jid, True)

    had_error = False
    output_sent = False

    async def _on_output(result: ContainerOutput) -> None:
        nonlocal had_error, output_sent
        if result.result:
            raw = result.result if isinstance(result.result, str) else json.dumps(result.result)
            text = format_outbound(raw)
            logger.info("Agent output: group=%s preview=%s", group.name, raw[:200])

            # Extract [SEND_FILE: ...] directives from the agent's reply.
            # Files are sent first; the remaining text (if any) follows.
            clean_text, file_directives = parse_file_directives(text)

            group_dir = resolve_group_folder_path(group.folder)
            for directive in file_directives:
                host_path = (group_dir / directive.workspace_rel_path).resolve()
                try:
                    host_path.relative_to(group_dir.resolve())
                except ValueError:
                    logger.warning(
                        "send_file: path traversal blocked (group=%s path=%s)",
                        group.name, directive.workspace_rel_path,
                    )
                    continue
                if host_path.is_file():
                    await channel.send_file(chat_jid, host_path, directive.caption)
                    output_sent = True
                else:
                    logger.warning(
                        "send_file: file not found (group=%s path=%s)",
                        group.name, host_path,
                    )

            if clean_text:
                await channel.send_message(chat_jid, clean_text)
                output_sent = True

            _reset_idle()
        if result.status == "success":
            state._queue.notify_idle(chat_jid)
        if result.status == "error":
            had_error = True

    status = await run_agent(group, prompt, chat_jid, _on_output)

    await channel.set_typing(chat_jid, False)
    if idle_timer:
        idle_timer.cancel()

    if status == "error" or had_error:
        # Check if shutting down - skip error notifications during shutdown
        if state._shutdown_event is not None and state._shutdown_event.is_set():
            logger.info(
                "Agent stopped (shutdown): group=%s", group.name
            )
            state._consecutive_errors.pop(chat_jid, None)
            return True

        if output_sent:
            logger.warning(
                "Agent error after output sent, skipping cursor rollback: group=%s", group.name
            )
            state._consecutive_errors.pop(chat_jid, None)
            return True
        consecutive = state._consecutive_errors.get(chat_jid, 0) + 1
        state._consecutive_errors[chat_jid] = consecutive
        if consecutive > state.MAX_ROLLBACK_RETRIES:
            logger.error(
                "Agent error repeated %d times, advancing cursor to avoid infinite loop: group=%s",
                consecutive, group.name,
            )
            state._consecutive_errors.pop(chat_jid, None)
            await notify_error(group, chat_jid)
            return True
        state._last_agent_timestamp[chat_jid] = previous_cursor
        await state.save_state()
        logger.warning(
            "Agent error (attempt %d/%d), rolled back cursor: group=%s",
            consecutive, state.MAX_ROLLBACK_RETRIES, group.name,
        )
        await notify_error(group, chat_jid)
        return False

    state._consecutive_errors.pop(chat_jid, None)
    return True
