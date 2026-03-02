"""
Telegram channel for Omiga.

Uses python-telegram-bot v21+ (fully async).

JID format: ``tg:{chat_id}``

  * Supergroups / channels: ``tg:-1001234567890``  (negative IDs)
  * Private DMs / bots:     ``tg:123456789``        (positive IDs)

The bot must be added to a group before messages are delivered.
In groups, all text messages are forwarded.  The main-loop trigger
pattern (@ASSISTANT_NAME) gates whether a container is actually spawned.

Setup
-----
1. Create a bot via @BotFather → get the token.
2. Add ``TELEGRAM_BOT_TOKEN=<token>`` to your .env file.
3. For groups: add the bot, disable privacy mode via @BotFather so it
   receives all messages (not just commands and @mentions).
4. Register the group JID via the IPC ``register_group`` command once the
   bot is running.
"""
from __future__ import annotations

import asyncio
import logging
from datetime import datetime, timezone
from typing import Callable, Optional

from telegram import Bot, Update
from telegram.constants import ChatAction, ChatType
from telegram.error import TelegramError
from telegram.ext import Application, ContextTypes, MessageHandler, filters

from omiga.channels.base import Channel, OnChatMetadata, OnInboundMessage
from omiga.config import ASSISTANT_NAME
from omiga.models import NewMessage, RegisteredGroup

logger = logging.getLogger(__name__)

# Telegram hard limit on outgoing message length
_MAX_MSG_LEN = 4096


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _jid(chat_id: int | str) -> str:
    """Return the Omiga JID for a Telegram chat_id."""
    return f"tg:{chat_id}"


def _chat_id(jid: str) -> int:
    """Extract the integer chat_id from a ``tg:`` JID."""
    return int(jid.removeprefix("tg:"))


def _split_text(text: str, max_len: int = _MAX_MSG_LEN) -> list[str]:
    """Split *text* into chunks ≤ *max_len* characters.

    Tries to split on newlines first so code blocks stay readable.
    Falls back to hard splitting at the character limit.
    """
    if len(text) <= max_len:
        return [text]

    chunks: list[str] = []
    while text:
        if len(text) <= max_len:
            chunks.append(text)
            break
        # Find the last newline before the limit
        split_at = text.rfind("\n", 0, max_len)
        if split_at <= 0:
            split_at = max_len
        chunks.append(text[:split_at])
        text = text[split_at:].lstrip("\n")
    return chunks


def _iso_timestamp(dt: Optional[datetime]) -> str:
    """Return an ISO-8601 UTC string from *dt*, defaulting to now."""
    if dt is None:
        return datetime.now(timezone.utc).isoformat()
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.isoformat()


# ---------------------------------------------------------------------------
# Channel implementation
# ---------------------------------------------------------------------------

class TelegramChannel(Channel):
    """Omiga channel backed by the Telegram Bot API.

    Parameters
    ----------
    token:
        The Telegram bot token (from @BotFather).
    on_message:
        Called for every inbound message from a *registered* chat.
    on_chat_meta:
        Called for every chat that sends a message (registered or not),
        enabling group discovery.
    registered_groups:
        Callable returning the current dict of ``{jid: RegisteredGroup}``.
        Checked at message-receive time so newly registered groups are
        picked up without a restart.
    """

    def __init__(
        self,
        token: str,
        on_message: OnInboundMessage,
        on_chat_meta: OnChatMetadata,
        registered_groups: Callable[[], dict[str, RegisteredGroup]],
    ) -> None:
        self._token = token
        self._on_message = on_message
        self._on_chat_meta = on_chat_meta
        self._registered_groups = registered_groups

        self._app: Optional[Application] = None
        self._bot_id: Optional[int] = None
        self._bot_username: str = ""
        self._connected: bool = False

    # ------------------------------------------------------------------
    # Channel ABC
    # ------------------------------------------------------------------

    @property
    def name(self) -> str:
        return "telegram"

    @property
    def trigger_pattern(self):
        """Match @bot_username (the real Telegram handle) as the trigger.

        Falls back to None (→ global TRIGGER_PATTERN) until the bot has
        connected and we know the actual username.
        """
        if not self._bot_username:
            return None
        import re
        return re.compile(
            rf"^@{re.escape(self._bot_username)}\b", re.IGNORECASE
        )

    # Long-poll duration (seconds).  Must be shorter than whatever timeout your
    # local proxy uses for idle connections.  10 s works with most VPN/proxy
    # setups (Clash, Shadowsocks, V2Ray, etc.).  Increase if you are on a
    # direct connection and want fewer API round-trips.
    _POLL_TIMEOUT: int = 10

    async def connect(self) -> None:
        """Build the application, resolve bot identity, and start polling.

        ``get_updates_read_timeout`` is set slightly above ``_POLL_TIMEOUT``
        so httpx doesn't cancel the request before Telegram has a chance to
        reply.  This avoids the ``RemoteProtocolError`` that local proxies
        trigger when they drop long-lived idle connections.
        """
        # Suppress transient network-drop errors from the Updater's internal
        # retry loop — they are automatically recovered and just add log noise
        # when running behind a proxy.
        logging.getLogger("telegram.ext.Updater").setLevel(logging.CRITICAL)
        logging.getLogger("telegram.ext._utils.networkloop").setLevel(logging.CRITICAL)

        self._app = (
            Application.builder()
            .token(self._token)
            # read timeout must exceed the long-poll timeout
            .get_updates_read_timeout(self._POLL_TIMEOUT + 5)
            .build()
        )
        # Handle all plain-text messages (groups and DMs)
        self._app.add_handler(
            MessageHandler(filters.TEXT & ~filters.COMMAND, self._handle_text)
        )

        await self._app.initialize()

        bot_info = await self._app.bot.get_me()
        self._bot_id = bot_info.id
        self._bot_username = bot_info.username or ""
        logger.info(
            "Telegram connected: @%s (id=%d)", self._bot_username, self._bot_id
        )

        await self._app.start()
        await self._app.updater.start_polling(
            drop_pending_updates=True,
            # Long-poll duration: Telegram waits at most this many seconds for
            # a new update before returning an empty response.  Keeping it
            # below the proxy's idle-connection timeout prevents the proxy from
            # closing the socket mid-request (which causes the NetworkError).
            timeout=self._POLL_TIMEOUT,
        )
        self._connected = True

    async def send_message(self, jid: str, text: str) -> None:
        """Send *text* to the Telegram chat identified by *jid*."""
        if not self._connected or self._app is None:
            logger.warning("Telegram not connected — cannot send to jid=%s", jid)
            return
        chat_id = _chat_id(jid)
        for chunk in _split_text(text):
            try:
                await self._app.bot.send_message(chat_id=chat_id, text=chunk)
            except TelegramError as exc:
                logger.error("Telegram send_message failed jid=%s: %s", jid, exc)

    def is_connected(self) -> bool:
        return self._connected

    def owns_jid(self, jid: str) -> bool:
        return jid.startswith("tg:")

    async def disconnect(self) -> None:
        """Stop polling and gracefully shut down the bot application."""
        self._connected = False
        if self._app is None:
            return
        try:
            if self._app.updater.running:
                await self._app.updater.stop()
            await self._app.stop()
            await self._app.shutdown()
        except Exception as exc:  # pragma: no cover
            logger.warning("Telegram disconnect error: %s", exc)
        finally:
            self._app = None

    async def set_typing(self, jid: str, is_typing: bool) -> None:
        """Broadcast a 'typing…' action (only while is_typing=True)."""
        if not self._connected or self._app is None or not is_typing:
            return
        chat_id = _chat_id(jid)
        try:
            await self._app.bot.send_chat_action(
                chat_id=chat_id, action=ChatAction.TYPING
            )
        except TelegramError as exc:
            logger.debug("Typing indicator failed jid=%s: %s", jid, exc)

    # ------------------------------------------------------------------
    # Internal handler
    # ------------------------------------------------------------------

    async def _handle_text(
        self, update: Update, context: ContextTypes.DEFAULT_TYPE
    ) -> None:
        """Dispatch handler called by python-telegram-bot for each text message."""
        msg = update.effective_message
        chat = update.effective_chat
        user = update.effective_user

        if msg is None or chat is None or not msg.text:
            return

        jid = _jid(chat.id)
        is_group = chat.type in (ChatType.GROUP, ChatType.SUPERGROUP, ChatType.CHANNEL)
        chat_name = chat.title or (user.full_name if user else str(chat.id))
        ts = _iso_timestamp(msg.date)

        # Always update chat metadata — enables group discovery even before
        # the group is registered.
        _fire(self._on_chat_meta(jid, ts, chat_name, "telegram", is_group))

        # Sender info
        sender_id = str(user.id) if user else str(chat.id)
        sender_name = user.full_name if user else chat_name
        is_from_me = bool(user and user.id == self._bot_id)
        is_bot_msg = is_from_me or msg.text.startswith(f"{ASSISTANT_NAME}:")

        new_msg = NewMessage(
            id=str(msg.message_id),
            chat_jid=jid,
            sender=sender_id,
            sender_name=sender_name,
            content=msg.text,
            timestamp=ts,
            is_from_me=is_from_me,
            is_bot_message=is_bot_msg,
        )

        # Only deliver on_message for registered groups (mirrors TypeScript behavior)
        if jid in self._registered_groups():
            _fire(self._on_message(jid, new_msg))
        else:
            logger.debug(
                "Message from unregistered chat jid=%s — updating metadata only", jid
            )


def _fire(coro) -> None:  # type: ignore[type-arg]
    """Schedule a coroutine as a fire-and-forget asyncio task."""
    asyncio.ensure_future(coro)
