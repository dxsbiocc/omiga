"""
Feishu (Lark) channel for Omiga.

Uses lark-oapi WebSocket long connection to receive events (no public IP
needed). Sends via Open API with tenant_access_token.

Setup
-----
1. Create a Feishu enterprise app at https://open.feishu.cn/app
2. Enable "Bot" capability
3. Under "Event Subscriptions" → enable WebSocket (长连接) mode
4. Subscribe to "Receive Message" event (im.message.receive_v1)
5. Add to .env:
      FEISHU_APP_ID=cli_xxxxx
      FEISHU_APP_SECRET=xxxxxxxx
6. Add the bot to the target group/chat

JID format
----------
- Group chat:   feishu:{chat_id}   (e.g. feishu:oc_xxxxx)
- Private chat: feishu:{open_id}   (e.g. feishu:ou_xxxxx)

Dependencies
------------
    pip install lark-oapi aiohttp
"""
from __future__ import annotations

import asyncio
import json
import logging
import threading
import time
from typing import Callable, Optional

from omiga.channels.base import Channel, OnChatMetadata, OnInboundMessage
from omiga.config import ASSISTANT_NAME, WHISPER_LANGUAGE
from omiga.channels.media import get_attachment_path
from omiga.models import MediaAttachment, NewMessage, ReplyContext
from omiga.channels.transcription import transcribe_audio

logger = logging.getLogger(__name__)

_TOKEN_REFRESH_BUFFER = 300  # refresh token 5 min before expiry


def _jid(chat_id: str) -> str:
    return f"feishu:{chat_id}"


def _chat_id(jid: str) -> str:
    return jid.removeprefix("feishu:")


def _iso_now() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat()


class FeishuChannel(Channel):
    """Omiga channel backed by Feishu/Lark Bot API.

    Parameters
    ----------
    app_id:
        Feishu app ID (FEISHU_APP_ID in .env)
    app_secret:
        Feishu app secret (FEISHU_APP_SECRET in .env)
    on_message:
        Called for every inbound text message from a registered chat.
    on_chat_meta:
        Called for every chat that sends a message (registered or not).
    registered_groups:
        Callable returning the current dict of ``{jid: RegisteredGroup}``.
    """

    def __init__(
        self,
        app_id: str,
        app_secret: str,
        on_message: OnInboundMessage,
        on_chat_meta: OnChatMetadata,
        registered_groups: Callable,
    ) -> None:
        try:
            import lark_oapi as lark  # noqa: F401
            import aiohttp  # noqa: F401
            self._available = True
        except ImportError as exc:
            logger.error(
                "Feishu channel requires lark-oapi and aiohttp: "
                "pip install lark-oapi aiohttp (%s)", exc
            )
            self._available = False

        self._app_id = app_id
        self._app_secret = app_secret
        self._on_message = on_message
        self._on_chat_meta = on_chat_meta
        self._registered_groups = registered_groups
        self._connected = False

        # Token cache
        self._token: Optional[str] = None
        self._token_expire_at: float = 0.0
        self._token_lock: Optional[asyncio.Lock] = None

        # Dedup: track recently processed message_ids
        self._seen_ids: set[str] = set()
        self._seen_ids_list: list[str] = []  # FIFO, capped at 1000

        # Maps jid → last received message_id for outbound reply threading
        self._last_msg_id: dict[str, str] = {}

        # HTTP session (created on connect)
        self._http = None  # aiohttp.ClientSession

        # WebSocket client (runs in a thread)
        self._ws_client = None
        self._ws_thread: Optional[threading.Thread] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None

    # ------------------------------------------------------------------
    # Channel ABC
    # ------------------------------------------------------------------

    @property
    def name(self) -> str:
        return "feishu"

    @property
    def trigger_pattern(self):
        return None  # uses global TRIGGER_PATTERN

    def owns_jid(self, jid: str) -> bool:
        return jid.startswith("feishu:")

    def is_connected(self) -> bool:
        # If the background thread died (lark_oapi gave up on reconnect),
        # mark ourselves as disconnected so the health monitor notices.
        if self._ws_thread is not None and not self._ws_thread.is_alive():
            self._connected = False
        return self._connected

    async def connect(self) -> None:
        if not self._available:
            return
        import aiohttp
        self._token_lock = asyncio.Lock()
        self._http = aiohttp.ClientSession()
        self._loop = asyncio.get_event_loop()

        # Verify credentials by fetching a token
        try:
            token = await self._get_token()
            logger.info("Feishu channel connected: app_id=%s", self._app_id)
        except Exception as exc:
            logger.error("Feishu: failed to get token — %s", exc)
            await self._http.close()
            return

        # Start WebSocket listener in background thread
        self._ws_thread = threading.Thread(
            target=self._run_ws_thread,
            daemon=True,
            name="feishu-ws",
        )
        self._ws_thread.start()
        self._connected = True

    async def send_message(self, jid: str, text: str) -> None:
        if not self._connected or not text.strip():
            return
        chat_id = _chat_id(jid)
        try:
            token = await self._get_token()
            last_msg_id = self._last_msg_id.get(jid)
            if last_msg_id:
                # Reply to the last received message for native threading
                await self._reply_text(token, last_msg_id, text)
            else:
                # Determine receive_id_type: open_id starts with "ou_", chat_id with "oc_"
                if chat_id.startswith("ou_"):
                    receive_id_type = "open_id"
                else:
                    receive_id_type = "chat_id"
                await self._send_text(token, chat_id, receive_id_type, text)
        except Exception as exc:
            logger.error("Feishu send_message failed jid=%s: %s", jid, exc)

    async def set_typing(self, jid: str, is_typing: bool) -> None:
        # Feishu does not support a direct typing indicator over the Bot API.
        # Could use message reactions (e.g. "Thinking") but keeping it simple.
        pass

    async def disconnect(self) -> None:
        self._connected = False
        if self._ws_client is not None:
            try:
                self._ws_client.close()
            except Exception:
                pass
        if self._http is not None:
            await self._http.close()
            self._http = None

    async def reconnect(self) -> None:
        """Restart the WebSocket thread when it has died.

        lark_oapi handles transient network drops internally (auto_reconnect=True),
        but if it exhausts retries the thread exits.  We detect that via
        is_connected() and restart the thread here.
        """
        if not self._available:
            return
        if self._ws_thread is not None and self._ws_thread.is_alive():
            return  # thread is running — lark_oapi handles reconnect internally

        logger.info("Feishu: WebSocket thread died — restarting")
        # Discard the dead client; _run_ws_thread will create a fresh one
        self._ws_client = None
        self._ws_thread = threading.Thread(
            target=self._run_ws_thread,
            daemon=True,
            name="feishu-ws",
        )
        self._ws_thread.start()
        self._connected = True
        logger.info("Feishu: WebSocket thread restarted")

    # ------------------------------------------------------------------
    # Token management
    # ------------------------------------------------------------------

    async def _get_token(self) -> str:
        now = time.monotonic()
        if self._token and now < self._token_expire_at - _TOKEN_REFRESH_BUFFER:
            return self._token

        assert self._token_lock is not None
        async with self._token_lock:
            now = time.monotonic()
            if self._token and now < self._token_expire_at - _TOKEN_REFRESH_BUFFER:
                return self._token

            assert self._http is not None
            url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
            async with self._http.post(
                url, json={"app_id": self._app_id, "app_secret": self._app_secret}
            ) as resp:
                data = await resp.json(content_type=None)

            if data.get("code") != 0:
                raise RuntimeError(
                    f"Feishu token error code={data.get('code')} msg={data.get('msg')}"
                )
            token = data.get("tenant_access_token")
            if not token:
                raise RuntimeError("Feishu: missing tenant_access_token in response")

            expire = int(data.get("expire", 3600))
            self._token = token
            self._token_expire_at = now + expire
            return token

    # ------------------------------------------------------------------
    # Send helper
    # ------------------------------------------------------------------

    async def _send_text(
        self, token: str, receive_id: str, receive_id_type: str, text: str
    ) -> None:
        url = (
            f"https://open.feishu.cn/open-apis/im/v1/messages"
            f"?receive_id_type={receive_id_type}"
        )
        # Split long messages (Feishu limit: 4000 chars per message)
        _MAX = 4000
        chunks = [text[i : i + _MAX] for i in range(0, len(text), _MAX)] if len(text) > _MAX else [text]
        assert self._http is not None
        for chunk in chunks:
            body = {
                "receive_id": receive_id,
                "msg_type": "text",
                "content": json.dumps({"text": chunk}),
            }
            async with self._http.post(
                url, json=body, headers={"Authorization": f"Bearer {token}"}
            ) as resp:
                if resp.status >= 400:
                    data = await resp.text()
                    logger.error("Feishu send failed %s: %s", resp.status, data[:200])

    async def _reply_text(self, token: str, message_id: str, text: str) -> None:
        """Reply to *message_id* using the Feishu reply API (native threading)."""
        url = f"https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/reply"
        _MAX = 4000
        chunks = [text[i : i + _MAX] for i in range(0, len(text), _MAX)] if len(text) > _MAX else [text]
        assert self._http is not None
        for chunk in chunks:
            body = {
                "msg_type": "text",
                "content": json.dumps({"text": chunk}),
            }
            async with self._http.post(
                url, json=body, headers={"Authorization": f"Bearer {token}"}
            ) as resp:
                if resp.status >= 400:
                    data = await resp.text()
                    logger.error("Feishu reply failed %s: %s", resp.status, data[:200])

    async def _fetch_feishu_message(self, message_id: str) -> Optional[tuple[str, str]]:
        """Fetch a message by ID and return (sender_name, text_content) or None."""
        try:
            token = await self._get_token()
            url = f"https://open.feishu.cn/open-apis/im/v1/messages/{message_id}"
            assert self._http is not None
            async with self._http.get(
                url, headers={"Authorization": f"Bearer {token}"}
            ) as resp:
                if resp.status != 200:
                    return None
                data = await resp.json(content_type=None)
            items = (data.get("data") or {}).get("items") or []
            if not items:
                return None
            item = items[0]
            # Extract sender name
            sender_obj = item.get("sender") or {}
            sender_name = (sender_obj.get("name") or "").strip()
            if not sender_name:
                sid = (sender_obj.get("sender_id") or {}).get("open_id", "")
                sender_name = str(sid)
            # Extract text content from body
            body = item.get("body") or {}
            content_raw = body.get("content", "")
            try:
                content_parsed = json.loads(content_raw)
                text = (content_parsed.get("text") or "").strip()
            except Exception:
                text = content_raw.strip()
            return (sender_name, text[:200])
        except Exception as exc:
            logger.debug("Feishu: failed to fetch message %s: %s", message_id, exc)
            return None

    # ------------------------------------------------------------------
    # Media download helper
    # ------------------------------------------------------------------

    async def _download_feishu_media(
        self,
        message_id: str,
        file_key: str,
        media_type: str,  # "image" or "file"
        dest_path,
    ) -> bool:
        """Download a Feishu media resource and save it to *dest_path*.

        Returns True on success, False on error.
        """
        from pathlib import Path
        dest_path = Path(dest_path)
        try:
            token = await self._get_token()
            url = (
                f"https://open.feishu.cn/open-apis/im/v1/messages"
                f"/{message_id}/resources/{file_key}?type={media_type}"
            )
            assert self._http is not None
            async with self._http.get(
                url, headers={"Authorization": f"Bearer {token}"}
            ) as resp:
                if resp.status != 200:
                    body = await resp.text()
                    logger.error(
                        "Feishu media download failed %s: %s", resp.status, body[:200]
                    )
                    return False
                data = await resp.read()
            dest_path.parent.mkdir(parents=True, exist_ok=True)
            dest_path.write_bytes(data)
            logger.debug("Feishu: downloaded %s → %s (%d bytes)", file_key, dest_path, len(data))
            return True
        except Exception as exc:
            logger.error("Feishu: failed to download %s: %s", file_key, exc)
            return False

    # ------------------------------------------------------------------
    # WebSocket receive (runs in a background thread)
    # ------------------------------------------------------------------

    def _run_ws_thread(self) -> None:
        """Run the lark_oapi WebSocket client in a thread.

        lark_oapi.ws.client captures the asyncio event loop at MODULE import
        time as a module-level variable ``loop``.  Because the module is first
        imported in the main thread, that variable permanently holds the main
        loop.  Calling asyncio.set_event_loop() in this thread has no effect on
        that cached reference.

        Fix: after creating our dedicated thread loop we monkey-patch
        lark_oapi.ws.client.loop so Client.start() uses ours instead.
        """
        try:
            import lark_oapi as lark
            import lark_oapi.ws.client as _lark_ws_client
            from lark_oapi.api.im.v1 import P2ImMessageReceiveV1
        except ImportError:
            logger.error("Feishu: lark_oapi not available")
            return

        # Create a dedicated event loop for this thread and redirect lark_oapi
        # to use it instead of the main loop it captured at import time.
        thread_loop = asyncio.new_event_loop()
        asyncio.set_event_loop(thread_loop)
        _lark_ws_client.loop = thread_loop  # key: patch module-level variable

        # Feishu is a domestic Chinese service — bypass any SOCKS/HTTP proxy so
        # websockets (and requests inside lark_oapi) connect directly.
        # We append to no_proxy rather than clearing all proxy settings so that
        # other outbound connections (Telegram, etc.) are unaffected.
        import os as _os
        _feishu_bypass = "open.feishu.cn,lark.larksuite.com"
        for _var in ("no_proxy", "NO_PROXY"):
            _cur = _os.environ.get(_var, "")
            if "feishu.cn" not in _cur:
                _os.environ[_var] = (_cur + "," + _feishu_bypass if _cur else _feishu_bypass)

        def _on_p2_im_message_receive_v1(data: P2ImMessageReceiveV1) -> None:
            # Route back to the main asyncio loop for DB / channel callbacks.
            if self._loop and self._loop.is_running():
                asyncio.run_coroutine_threadsafe(
                    self._handle_message(data), self._loop
                )

        event_handler = (
            lark.EventDispatcherHandler.builder("", "")
            .register_p2_im_message_receive_v1(_on_p2_im_message_receive_v1)
            .build()
        )

        ws_client = lark.ws.Client(
            self._app_id,
            self._app_secret,
            event_handler=event_handler,
            log_level=lark.LogLevel.WARNING,
        )
        self._ws_client = ws_client
        logger.info("Feishu: WebSocket client starting")
        try:
            ws_client.start()  # blocks until stopped or error
        finally:
            thread_loop.close()

    # ------------------------------------------------------------------
    # Message handler (async, called from thread)
    # ------------------------------------------------------------------

    async def _handle_message(self, data) -> None:
        try:
            event = getattr(data, "event", None)
            if not event:
                return
            message = getattr(event, "message", None)
            sender = getattr(event, "sender", None)
            if not message or not sender:
                return

            # Dedup
            message_id = str(getattr(message, "message_id", "") or "").strip()
            if message_id in self._seen_ids:
                return
            self._seen_ids.add(message_id)
            self._seen_ids_list.append(message_id)
            if len(self._seen_ids_list) > 1000:
                oldest = self._seen_ids_list.pop(0)
                self._seen_ids.discard(oldest)

            # Skip bot messages
            sender_type = getattr(sender, "sender_type", "") or ""
            if sender_type == "bot":
                return

            # Extract sender open_id
            sender_id_obj = getattr(sender, "sender_id", None)
            sender_open_id = (
                str(getattr(sender_id_obj, "open_id", "") or "").strip()
                if sender_id_obj else ""
            )
            sender_name = (getattr(sender, "name", "") or "").strip() or sender_open_id

            # Extract chat info
            chat_id = str(getattr(message, "chat_id", "") or "").strip()
            chat_type = str(getattr(message, "chat_type", "p2p") or "p2p").strip()
            is_group = chat_type == "group"

            # Use chat_id for JID in groups, open_id for p2p
            raw_id = chat_id if is_group else (sender_open_id or chat_id)
            jid = _jid(raw_id)
            ts = _iso_now()

            # Always update chat metadata for group discovery
            _fire(self._on_chat_meta(
                jid, ts,
                chat_id[:20] if is_group else sender_name,
                "feishu",
                is_group,
            ))

            # Track last received message ID for outbound reply threading
            if message_id:
                self._last_msg_id[jid] = message_id

            # Extract text content + any media attachments
            msg_type = str(getattr(message, "message_type", "text") or "text").strip()
            content_raw = getattr(message, "content", None) or ""
            text = ""
            attachments: list[MediaAttachment] = []

            if msg_type == "text":
                try:
                    text = json.loads(content_raw).get("text", "").strip()
                except Exception:
                    text = content_raw.strip()

            elif msg_type == "image":
                try:
                    content_data = json.loads(content_raw)
                    image_key = content_data.get("image_key", "")
                except Exception:
                    image_key = ""
                text = "[image]"
                if image_key and message_id and jid in self._registered_groups():
                    filename = f"{message_id}_image.jpg"
                    att_dir, rel_path = get_attachment_path(
                        jid, filename, self._registered_groups()
                    )
                    ok = await self._download_feishu_media(
                        message_id, image_key, "image", att_dir / filename
                    )
                    if ok:
                        attachments = [MediaAttachment(
                            type="image",
                            filename=filename,
                            mime_type="image/jpeg",
                            local_path=rel_path,
                            url=image_key,
                        )]

            elif msg_type in ("file", "audio", "video"):
                try:
                    content_data = json.loads(content_raw)
                    file_key = content_data.get("file_key", "")
                    orig_name = content_data.get("file_name", "")
                except Exception:
                    file_key = ""
                    orig_name = ""
                ext_map = {"audio": "ogg", "video": "mp4", "file": "bin"}
                ext = orig_name.rsplit(".", 1)[-1] if "." in orig_name else ext_map.get(msg_type, "bin")
                filename = f"{message_id or 'media'}.{ext}"
                mime_map = {"audio": "audio/ogg", "video": "video/mp4", "file": "application/octet-stream"}
                mime = mime_map.get(msg_type, "application/octet-stream")
                text = orig_name or f"[{msg_type}]"
                if file_key and message_id and jid in self._registered_groups():
                    att_dir, rel_path = get_attachment_path(
                        jid, filename, self._registered_groups()
                    )
                    local_path = att_dir / filename
                    ok = await self._download_feishu_media(
                        message_id, file_key, "file", local_path
                    )
                    if ok:
                        attachments = [MediaAttachment(
                            type=msg_type,
                            filename=filename,
                            mime_type=mime,
                            local_path=rel_path,
                            url=file_key,
                        )]
                        # Transcribe audio messages
                        if msg_type == "audio":
                            transcribed = await transcribe_audio(
                                local_path, language=WHISPER_LANGUAGE or None
                            )
                            if transcribed:
                                text = f"[Voice]: {transcribed}"

            else:
                try:
                    text = json.loads(content_raw).get("text", "").strip()
                except Exception:
                    text = content_raw.strip()

            if not text:
                return

            # Skip messages that are our own bot prefix
            if text.startswith(f"{ASSISTANT_NAME}:"):
                return

            is_from_me = sender_type == "bot"

            # Extract reply context from parent_id (Feishu threading)
            reply_to: Optional[ReplyContext] = None
            parent_id = str(getattr(message, "parent_id", "") or "").strip()
            if parent_id and jid in self._registered_groups():
                result = await self._fetch_feishu_message(parent_id)
                if result:
                    rt_sender, rt_content = result
                    reply_to = ReplyContext(
                        message_id=parent_id,
                        sender_name=rt_sender,
                        content=rt_content,
                    )

            new_msg = NewMessage(
                id=message_id or ts,
                chat_jid=jid,
                sender=sender_open_id or "unknown",
                sender_name=sender_name,
                content=text,
                timestamp=ts,
                is_from_me=is_from_me,
                is_bot_message=is_from_me,
                attachments=attachments,
                reply_to=reply_to,
            )

            # Only deliver on_message for registered chats
            if jid in self._registered_groups():
                _fire(self._on_message(jid, new_msg))
            else:
                logger.debug(
                    "Feishu message from unregistered chat jid=%s — metadata only", jid
                )

        except Exception as exc:
            logger.exception("Feishu _handle_message error: %s", exc)


def _fire(coro) -> None:  # type: ignore[type-arg]
    """Schedule a coroutine as fire-and-forget."""
    asyncio.ensure_future(coro)
