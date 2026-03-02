"""
Discord channel for Omiga.

Uses discord.py (fully async) — the bot connects via Discord Gateway
(WebSocket). No public IP required.

Setup
-----
1. Go to https://discord.com/developers/applications → New Application
2. Bot tab → Add Bot → copy the Token
3. Enable "Privileged Gateway Intents":
     - MESSAGE CONTENT INTENT
     - SERVER MEMBERS INTENT  (optional, for member names)
4. OAuth2 → URL Generator:
     - Scopes: bot
     - Bot Permissions: Send Messages, Read Message History, View Channels
   Invite the bot to your server with the generated URL.
5. Add to .env:
     DISCORD_BOT_TOKEN=your_bot_token
6. Optionally set DISCORD_HTTP_PROXY=http://127.0.0.1:7890 if Discord is
   blocked in your region.

JID format
----------
- Server text channel:  discord:ch:{channel_id}   (e.g. discord:ch:1234567890)
- DM:                   discord:dm:{user_id}       (e.g. discord:dm:9876543210)

Dependencies
------------
    pip install discord.py>=2.3
"""
from __future__ import annotations

import asyncio
import json
import logging
from typing import Callable, Optional

from omiga.channels.base import Channel, OnChatMetadata, OnInboundMessage
from omiga.config import ASSISTANT_NAME, WHISPER_LANGUAGE
from omiga.channels.media import get_attachment_path
from omiga.models import MediaAttachment, NewMessage, ReplyContext
from omiga.channels.transcription import transcribe_audio

logger = logging.getLogger(__name__)

_MAX_MSG_LEN = 2000  # Discord hard limit


def _jid_channel(channel_id: int | str) -> str:
    return f"discord:ch:{channel_id}"


def _jid_dm(user_id: int | str) -> str:
    return f"discord:dm:{user_id}"


def _iso_now() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat()


class DiscordChannel(Channel):
    """Omiga channel backed by Discord Bot API (discord.py).

    Parameters
    ----------
    token:
        Bot token from Discord Developer Portal.
    on_message:
        Called for every inbound text message from a registered chat.
    on_chat_meta:
        Called for every chat that sends a message (registered or not).
    registered_groups:
        Callable returning the current ``{jid: RegisteredGroup}`` dict.
    http_proxy:
        Optional HTTP/HTTPS proxy URL (e.g. ``http://127.0.0.1:7890``).
        Discord is blocked in some regions — set this if needed.
    """

    def __init__(
        self,
        token: str,
        on_message: OnInboundMessage,
        on_chat_meta: OnChatMetadata,
        registered_groups: Callable,
        http_proxy: str = "",
    ) -> None:
        try:
            import discord  # noqa: F401
            self._available = True
        except ImportError as exc:
            logger.error(
                "Discord channel requires discord.py: pip install discord.py (%s)", exc
            )
            self._available = False

        self._token = token
        self._on_message = on_message
        self._on_chat_meta = on_chat_meta
        self._registered_groups = registered_groups
        self._http_proxy = http_proxy or ""
        self._connected = False

        self._client = None  # discord.Client
        self._task: Optional[asyncio.Task] = None
        self._bot_id: Optional[int] = None  # set on on_ready, used for trigger pattern

        # Dedup
        self._seen_ids: set[str] = set()
        self._seen_ids_list: list[str] = []
        # Maps jid → (message_id, channel_id) for outbound reply threading
        self._last_msg_ref: dict[str, tuple[int, int]] = {}

    # ------------------------------------------------------------------
    # Channel ABC
    # ------------------------------------------------------------------

    @property
    def name(self) -> str:
        return "discord"

    @property
    def trigger_pattern(self):
        r"""Match both plain-text @Name and Discord's native <@BOT_ID> mention format.

        Note: \b cannot follow '>' (non-word char → non-word char = no boundary),
        so Discord mentions use (?:\s|$) as the boundary instead.
        """
        import re
        from omiga.config import ASSISTANT_NAME
        plain = re.escape(ASSISTANT_NAME)
        if self._bot_id is not None:
            # <@BOT_ID> or <@!BOT_ID> followed by whitespace or end-of-string
            return re.compile(
                rf"^(?:<@!?{self._bot_id}>(?:\s|$)|@{plain}\b)",
                re.IGNORECASE,
            )
        return re.compile(rf"^@{plain}\b", re.IGNORECASE)

    def owns_jid(self, jid: str) -> bool:
        return jid.startswith("discord:")

    def is_connected(self) -> bool:
        # If the gateway task finished (error or clean close), mark disconnected
        if self._task is not None and self._task.done():
            self._connected = False
        return self._connected

    async def connect(self) -> None:
        if not self._available or not self._token:
            return

        import discord

        intents = discord.Intents.default()
        intents.message_content = True
        intents.dm_messages = True
        intents.messages = True
        intents.guilds = True

        proxy_kwargs: dict = {}
        if self._http_proxy:
            proxy_kwargs["proxy"] = self._http_proxy

        self._client = discord.Client(intents=intents, **proxy_kwargs)

        # Register event handlers
        @self._client.event
        async def on_ready():
            self._bot_id = self._client.user.id
            logger.info(
                "Discord channel connected: %s (id=%s)",
                self._client.user,
                self._client.user.id,
            )
            self._connected = True

        @self._client.event
        async def on_message(message: discord.Message):
            await self._handle_message(message)

        # Start the Discord Gateway in a background task (non-blocking)
        self._task = asyncio.create_task(
            self._run(), name="discord-gateway"
        )

    async def send_message(self, jid: str, text: str) -> None:
        if not self._connected or not text.strip() or self._client is None:
            return
        try:
            await self._send_text(jid, text)
        except Exception as exc:
            logger.error("Discord send_message failed jid=%s: %s", jid, exc)

    async def set_typing(self, jid: str, is_typing: bool) -> None:
        # discord.py's typing() context manager is per-send; skip for simplicity
        pass

    async def disconnect(self) -> None:
        self._connected = False
        # Close the client FIRST so discord.py can send a proper DISCONNECT
        # and signal the heartbeat keep-alive thread to stop.
        # Cancelling the gateway task first interrupts client.start() mid-flight
        # and leaves the heartbeat thread alive, which then tries to use the
        # already-closed event loop and causes "RuntimeError: Event loop is closed".
        if self._client is not None:
            try:
                await self._client.close()
            except Exception:
                pass
            self._client = None
        # Now wait for the gateway task to finish naturally (heartbeat thread has
        # stopped by now).  Cancel as a last resort if it takes too long.
        if self._task is not None:
            if not self._task.done():
                try:
                    await asyncio.wait_for(asyncio.shield(self._task), timeout=5)
                except (asyncio.TimeoutError, asyncio.CancelledError, Exception):
                    self._task.cancel()
                    try:
                        await self._task
                    except (asyncio.CancelledError, Exception):
                        pass
            self._task = None

    async def reconnect(self) -> None:
        """Restart the Discord gateway when the background task has ended.

        discord.py handles transient disconnects internally (reconnect=True),
        so this is only called when the task truly exits (fatal error or
        explicit close by the library).  We close the old client and call
        connect() to create a fresh one.
        """
        if not self._available or not self._token:
            return
        if self._task is not None and not self._task.done():
            return  # still running

        logger.info("Discord: gateway task ended — reconnecting")
        # Close the stale client (discord.py clients cannot be restarted)
        if self._client is not None and not self._client.is_closed():
            try:
                await self._client.close()
            except Exception:
                pass
        self._client = None
        self._task = None
        self._connected = False
        await self.connect()
        logger.info("Discord: gateway restarted")

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _run(self) -> None:
        """Run discord.Client.start() — blocks until disconnected."""
        try:
            await self._client.start(self._token, reconnect=True)
        except Exception as exc:
            logger.error("Discord gateway error: %s", exc)
        finally:
            self._connected = False

    async def _send_text(self, jid: str, text: str) -> None:
        """Resolve JID → Discord channel/DM and send (split if > 2000 chars)."""
        import discord

        target: Optional[discord.abc.Messageable] = None

        if jid.startswith("discord:ch:"):
            channel_id = int(jid[len("discord:ch:"):])
            target = self._client.get_channel(channel_id)
            if target is None:
                target = await self._client.fetch_channel(channel_id)

        elif jid.startswith("discord:dm:"):
            user_id = int(jid[len("discord:dm:"):])
            user = self._client.get_user(user_id)
            if user is None:
                user = await self._client.fetch_user(user_id)
            target = user.dm_channel or await user.create_dm()

        if target is None:
            logger.error("Discord: cannot resolve target for jid=%s", jid)
            return

        # Split at 2000-char limit; first chunk replies to last received message
        chunks = [text[i : i + _MAX_MSG_LEN] for i in range(0, len(text), _MAX_MSG_LEN)]
        ref = None
        if jid.startswith("discord:ch:"):  # only reply in server channels, not DMs
            ref_data = self._last_msg_ref.get(jid)
            if ref_data:
                ref_msg_id, ref_ch_id = ref_data
                ref = discord.MessageReference(
                    message_id=ref_msg_id,
                    channel_id=ref_ch_id,
                    fail_if_not_exists=False,
                )
        for i, chunk in enumerate(chunks):
            await target.send(chunk, reference=ref if i == 0 else None)

    # ------------------------------------------------------------------
    # Message handler
    # ------------------------------------------------------------------

    async def _handle_message(self, message) -> None:
        try:
            import discord

            # Skip bot messages
            if message.author.bot:
                return

            # Dedup by message ID
            msg_id = str(message.id)
            if msg_id in self._seen_ids:
                return
            self._seen_ids.add(msg_id)
            self._seen_ids_list.append(msg_id)
            if len(self._seen_ids_list) > 1000:
                oldest = self._seen_ids_list.pop(0)
                self._seen_ids.discard(oldest)

            # Determine JID
            is_dm = isinstance(message.channel, discord.DMChannel)
            if is_dm:
                jid = _jid_dm(message.author.id)
                display_name = str(message.channel.id)
            else:
                jid = _jid_channel(message.channel.id)
                display_name = getattr(message.channel, "name", str(message.channel.id))

            ts = _iso_now()
            sender_id = str(message.author.id)
            sender_name = (
                getattr(message.author, "display_name", None)
                or getattr(message.author, "name", None)
                or sender_id
            )

            # Always update chat metadata
            _fire(self._on_chat_meta(
                jid, ts,
                display_name,
                "discord",
                not is_dm,
            ))

            # Store last message ref for outbound reply threading (channels only)
            if not is_dm:
                self._last_msg_ref[jid] = (message.id, message.channel.id)

            # Extract reply context if this message quotes another
            reply_to: Optional[ReplyContext] = None
            if message.reference and message.reference.message_id:
                try:
                    ref_msg = await message.channel.fetch_message(
                        message.reference.message_id
                    )
                    rt_content = ref_msg.content or ""
                    if not rt_content and getattr(ref_msg, "attachments", None):
                        rt_content = f"[{ref_msg.attachments[0].filename}]"
                    rt_sender = (
                        getattr(ref_msg.author, "display_name", None)
                        or getattr(ref_msg.author, "name", None)
                        or str(ref_msg.author.id)
                    )
                    reply_to = ReplyContext(
                        message_id=str(message.reference.message_id),
                        sender_name=rt_sender,
                        content=rt_content[:200],
                    )
                except Exception as exc:
                    logger.debug("Discord: failed to fetch reference message: %s", exc)

            # Keep original content including any Discord mention (<@BOT_ID>).
            # The trigger pattern matches <@BOT_ID> in the stored content.
            # Mention stripping for the container prompt happens in router.py.
            text = (message.content or "").strip()

            # Skip bot-prefixed messages
            if text.startswith(f"{ASSISTANT_NAME}:"):
                return

            # Download any file attachments (only for registered groups)
            media_attachments: list[MediaAttachment] = []
            if message.attachments and jid in self._registered_groups():
                for att in message.attachments:
                    att_filename = getattr(att, "filename", None) or "file"
                    unique_name = f"{msg_id}_{att_filename}"
                    att_dir, rel_path = get_attachment_path(
                        jid, unique_name, self._registered_groups()
                    )
                    local_path = att_dir / unique_name
                    content_type = getattr(att, "content_type", None) or "application/octet-stream"
                    # Determine media type from content_type
                    if content_type.startswith("image/"):
                        att_type = "image"
                    elif content_type.startswith("audio/"):
                        att_type = "audio"
                    elif content_type.startswith("video/"):
                        att_type = "video"
                    else:
                        att_type = "document"
                    try:
                        data = await att.read()
                        local_path.write_bytes(data)
                        media_att = MediaAttachment(
                            type=att_type,
                            filename=unique_name,
                            mime_type=content_type,
                            local_path=rel_path,
                            url=att.url,
                        )
                        media_attachments.append(media_att)
                        logger.debug("Discord: downloaded %s → %s", att_filename, local_path)
                        # Transcribe audio attachments
                        if att_type == "audio":
                            transcribed = await transcribe_audio(
                                local_path, language=WHISPER_LANGUAGE or None
                            )
                            if transcribed and not text:
                                text = f"[Voice]: {transcribed}"
                    except Exception as exc:
                        logger.error("Discord: failed to download attachment %s: %s", att_filename, exc)

            # Require either text or attachments
            if not text and not media_attachments:
                return

            if not text and media_attachments:
                # Build a placeholder from the first attachment
                text = f"[{media_attachments[0].type}]"

            new_msg = NewMessage(
                id=msg_id,
                chat_jid=jid,
                sender=sender_id,
                sender_name=sender_name,
                content=text,
                timestamp=ts,
                is_from_me=False,
                is_bot_message=False,
                attachments=media_attachments,
                reply_to=reply_to,
            )

            if jid in self._registered_groups():
                _fire(self._on_message(jid, new_msg))
            else:
                logger.debug(
                    "Discord message from unregistered chat jid=%s — metadata only", jid
                )

        except Exception as exc:
            logger.exception("Discord _handle_message error: %s", exc)


def _fire(coro) -> None:  # type: ignore[type-arg]
    """Schedule a coroutine as fire-and-forget."""
    asyncio.ensure_future(coro)
