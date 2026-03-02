"""Message polling loop for Omiga.

Drives the main per-tick message fetch, debounce management, and container
dispatch.  Also provides startup recovery for groups with unprocessed messages.
"""
from __future__ import annotations

import asyncio
import logging
from typing import TYPE_CHECKING

import omiga.state as state
from omiga.config import ASSISTANT_NAME, MESSAGE_DEBOUNCE_SECONDS, POLL_INTERVAL
from omiga.database import get_messages_since, get_new_messages
from omiga.processing import process_group_messages
from omiga.router import find_channel, format_messages

if TYPE_CHECKING:
    from omiga.channels.base import Channel

logger = logging.getLogger("omiga.message_loop")


async def typing_safe(channel: Channel, jid: str, typing: bool) -> None:
    try:
        await channel.set_typing(jid, typing)
    except Exception as err:
        logger.warning("Failed to set typing indicator: jid=%s err=%s", jid, err)


def recover_pending_messages() -> None:
    """Startup recovery: re-enqueue groups that likely have unprocessed messages.

    A group is considered pending when its per-group agent cursor is behind
    the global last_timestamp cursor, meaning it may have received messages
    that were not yet delivered to a container agent.
    """
    for chat_jid in state._registered_groups:
        agent_ts = state._last_agent_timestamp.get(chat_jid, "")
        if agent_ts < state._last_timestamp:
            logger.debug(
                "Recovering pending messages for jid=%s (agent_ts=%s < global_ts=%s)",
                chat_jid, agent_ts, state._last_timestamp,
            )
            state._queue.enqueue_message_check(chat_jid)


async def start_message_loop() -> None:
    if state._message_loop_running:
        logger.debug("Message loop already running")
        return
    state._message_loop_running = True

    triggers = [
        ch.trigger_pattern.pattern if ch.trigger_pattern else f"@{ASSISTANT_NAME}"
        for ch in state._channels
    ]
    logger.info("Omiga running (triggers: %s)", ", ".join(triggers))

    while state._shutdown_event is None or not state._shutdown_event.is_set():
        try:
            # Check debounce deadlines first — they expire even when there are no
            # new messages, because _last_timestamp was already advanced on the
            # poll that originally detected the messages.
            now = asyncio.get_event_loop().time()
            expired_jids = [jid for jid, dl in list(state._debounce_deadlines.items()) if now >= dl]
            for chat_jid in expired_jids:
                del state._debounce_deadlines[chat_jid]
                state._queue.enqueue_message_check(chat_jid)
                logger.debug("Debounce expired, enqueueing: jid=%s", chat_jid)

            jids = list(state._registered_groups.keys())
            messages, new_ts = await get_new_messages(jids, state._last_timestamp, ASSISTANT_NAME)

            if messages:
                logger.info("New messages: count=%d", len(messages))
                state._last_timestamp = new_ts
                await state.save_state()

                by_group: dict[str, list] = {}
                for msg in messages:
                    by_group.setdefault(msg.chat_jid, []).append(msg)

                for chat_jid, group_messages in by_group.items():
                    group = state._registered_groups.get(chat_jid)
                    if not group:
                        continue

                    channel = find_channel(state._channels, chat_jid)
                    if not channel:
                        logger.warning("No channel owns JID: %s", chat_jid)
                        continue

                    from omiga.config import MAIN_GROUP_FOLDER
                    from omiga.agent import effective_trigger
                    is_main_group = group.folder == MAIN_GROUP_FOLDER
                    needs_trigger = not is_main_group and group.requires_trigger is not False

                    if needs_trigger:
                        pat = effective_trigger(channel)
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
                        chat_jid, state._last_agent_timestamp.get(chat_jid, ""), ASSISTANT_NAME
                    )
                    msgs_to_send = all_pending if all_pending else group_messages
                    formatted = format_messages(msgs_to_send)

                    if state._queue.send_message(chat_jid, formatted):
                        # Container already running — pipe messages in directly.
                        # No debounce needed; the active container handles batching.
                        state._debounce_deadlines.pop(chat_jid, None)
                        logger.debug(
                            "Piped messages to active container: jid=%s count=%d",
                            chat_jid,
                            len(msgs_to_send),
                        )
                        state._last_agent_timestamp[chat_jid] = msgs_to_send[-1].timestamp
                        await state.save_state()
                        asyncio.ensure_future(
                            typing_safe(channel, chat_jid, True)
                        )
                    else:
                        # No active container — start debounce window.
                        # Expiry is checked at the top of each poll loop iteration,
                        # independently of whether new messages arrive.
                        if chat_jid not in state._debounce_deadlines:
                            state._debounce_deadlines[chat_jid] = (
                                asyncio.get_event_loop().time() + MESSAGE_DEBOUNCE_SECONDS
                            )
                            logger.debug(
                                "Debounce started: jid=%s window=%.1fs",
                                chat_jid, MESSAGE_DEBOUNCE_SECONDS,
                            )

        except Exception as err:
            logger.error("Error in message loop: %s", err)

        await asyncio.sleep(POLL_INTERVAL)
