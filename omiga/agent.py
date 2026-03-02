"""Container agent runner for Omiga.

Handles spawning container agents, session management, and session-corruption
auto-recovery.
"""
from __future__ import annotations

import logging

import omiga.state as state
from omiga.config import ASSISTANT_NAME, MAIN_GROUP_FOLDER
from omiga.container.runner import (
    ContainerOutput,
    run_container_agent,
    write_groups_snapshot,
    write_tasks_snapshot,
)
from omiga.database import get_all_chats, get_all_tasks, set_session
from omiga.models import AvailableGroup, ContainerInput, RegisteredGroup

logger = logging.getLogger("omiga.agent")

_SESSION_CORRUPTION_MARKERS = (
    # OpenAI-compatible APIs: tool message without preceding tool_calls
    "messages with role \"tool\" must be a response to a preceeding message with \"tool_calls\"",
    "messages with role 'tool' must be a response to a preceeding message with 'tool_calls'",
    # Anthropic: similar orphaned tool_result block
    "tool_result block(s) provided when previous message does not have tool_calls",
)


def is_session_corruption_error(error: str) -> bool:
    """Return True when the error is caused by a corrupted session history."""
    low = (error or "").lower()
    return any(marker.lower() in low for marker in _SESSION_CORRUPTION_MARKERS)


def effective_trigger(channel):
    """Return the trigger pattern for *channel*, preferring the channel's own."""
    from omiga.config import TRIGGER_PATTERN
    return channel.trigger_pattern or TRIGGER_PATTERN


async def run_agent(
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
    registered_jids = set(state._registered_groups.keys())
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
            state._sessions[group.folder] = output.new_session_id
            await set_session(group.folder, output.new_session_id)
        if on_output:
            await on_output(output)

    for attempt in range(2):  # attempt 0 = normal; attempt 1 = fresh session after corruption
        session_id = state._sessions.get(group.folder)
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
                lambda proc, name: state._queue.register_process(chat_jid, proc, name, group.folder),
                _wrapped_on_output if on_output else None,
            )

            if output.new_session_id:
                state._sessions[group.folder] = output.new_session_id
                await set_session(group.folder, output.new_session_id)

            if output.status == "error":
                if attempt == 0 and is_session_corruption_error(output.error or ""):
                    logger.warning(
                        "Session corruption detected for group=%s — clearing session and retrying",
                        group.name,
                    )
                    state._sessions.pop(group.folder, None)
                    await set_session(group.folder, "")
                    continue  # retry with session_id=None
                logger.error("Container agent error: group=%s error=%s", group.name, output.error)
                return "error"

            return "success"

        except Exception as err:
            logger.error("Agent error: group=%s err=%s", group.name, err)
            return "error"

    return "error"
