"""
QQ Channel for Omiga.

Uses QQ Bot WebSocket for receiving events and HTTP REST API for sending.
No public IP needed for receiving (WebSocket long connection).

Supports three chat types:
  - C2C (private chat):   JID = qq:c2c:{user_openid}
  - Group chat:           JID = qq:group:{group_openid}
  - Guild channel:        JID = qq:channel:{channel_id}

Setup
-----
1. Apply for QQ Bot at https://q.qq.com/
2. Create a bot, get App ID and App Secret
3. Add to .env:
      QQ_APP_ID=your_app_id
      QQ_APP_SECRET=your_app_secret
4. Install: pip install websocket-client aiohttp

Dependencies
------------
    pip install websocket-client aiohttp
"""
from __future__ import annotations

import asyncio
import json
import logging
import os
import threading
import time
from typing import Callable, Optional

from omiga.channels.base import Channel, OnChatMetadata, OnInboundMessage
from omiga.config import ASSISTANT_NAME
from omiga.models import NewMessage

logger = logging.getLogger(__name__)

# QQ Bot WebSocket op codes
_OP_DISPATCH = 0
_OP_HEARTBEAT = 1
_OP_IDENTIFY = 2
_OP_RESUME = 6
_OP_RECONNECT = 7
_OP_INVALID_SESSION = 9
_OP_HELLO = 10
_OP_HEARTBEAT_ACK = 11

# Event intents
_INTENT_GUILD_MESSAGES = 1 << 9
_INTENT_PUBLIC_GUILD_MESSAGES = 1 << 30
_INTENT_DIRECT_MESSAGE = 1 << 12
_INTENT_GROUP_AND_C2C = 1 << 25
_INTENT_GUILD_MEMBERS = 1 << 1

_TOKEN_URL = "https://bots.qq.com/app/getAppAccessToken"
_API_BASE = os.environ.get("QQ_API_BASE", "https://api.sgroup.qq.com").rstrip("/")
_RECONNECT_DELAYS = [1, 2, 5, 10, 30, 60]

# Per-message sequence counter for QQ API dedup
_msg_seq: dict[str, int] = {}
_msg_seq_lock = threading.Lock()


def _next_seq(key: str) -> int:
    with _msg_seq_lock:
        n = _msg_seq.get(key, 0) + 1
        _msg_seq[key] = n
        # Trim to avoid unbounded growth
        if len(_msg_seq) > 2000:
            for k in list(_msg_seq.keys())[:1000]:
                del _msg_seq[k]
        return n


def _iso_now() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat()


class QQChannel(Channel):
    """Omiga channel backed by QQ Bot API.

    Parameters
    ----------
    app_id:
        QQ Bot App ID (QQ_APP_ID in .env)
    app_secret:
        QQ Bot App Secret (QQ_APP_SECRET in .env)
    on_message:
        Called for every inbound message from a registered chat.
    on_chat_meta:
        Called for every chat that sends a message.
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
            import websocket  # noqa: F401
            import aiohttp  # noqa: F401
            self._available = True
        except ImportError as exc:
            logger.error(
                "QQ channel requires websocket-client and aiohttp: "
                "pip install websocket-client aiohttp (%s)", exc
            )
            self._available = False

        self._app_id = app_id
        self._app_secret = app_secret
        self._on_message = on_message
        self._on_chat_meta = on_chat_meta
        self._registered_groups = registered_groups
        self._connected = False

        # Token cache (thread-safe, used from both ws thread and async send)
        self._token: Optional[str] = None
        self._token_expires_at: float = 0.0
        self._token_lock = threading.Lock()

        # HTTP session for async sends
        self._http = None  # aiohttp.ClientSession

        # WebSocket
        self._stop_event = threading.Event()
        self._ws_thread: Optional[threading.Thread] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None

        # Dedup
        self._seen_ids: set[str] = set()
        self._seen_ids_list: list[str] = []

        # Reply context: jid → last message_id (for QQ's msg_id reply threading)
        self._last_msg_id: dict[str, str] = {}

    # ------------------------------------------------------------------
    # Channel ABC
    # ------------------------------------------------------------------

    @property
    def name(self) -> str:
        return "qq"

    @property
    def trigger_pattern(self):
        return None  # uses global TRIGGER_PATTERN

    def owns_jid(self, jid: str) -> bool:
        return jid.startswith("qq:")

    def is_connected(self) -> bool:
        if self._ws_thread is not None and not self._ws_thread.is_alive():
            self._connected = False
        return self._connected

    async def connect(self) -> None:
        if not self._available:
            return
        import aiohttp
        self._loop = asyncio.get_event_loop()
        self._http = aiohttp.ClientSession()

        # Verify credentials
        try:
            token = self._get_token_sync()
            logger.info("QQ channel connected: app_id=%s", self._app_id)
        except Exception as exc:
            logger.error("QQ: failed to get token — %s", exc)
            await self._http.close()
            return

        # Start WebSocket listener in background thread
        self._ws_thread = threading.Thread(
            target=self._run_ws_forever,
            daemon=True,
            name="qq-ws",
        )
        self._ws_thread.start()
        self._connected = True

    async def send_message(self, jid: str, text: str) -> None:
        if not self._connected or not text.strip():
            return
        try:
            token = await self._get_token_async()
            msg_id = self._last_msg_id.get(jid)
            if jid.startswith("qq:group:"):
                group_openid = jid.removeprefix("qq:group:")
                await self._send_group(token, group_openid, text.strip(), msg_id)
            elif jid.startswith("qq:channel:"):
                channel_id = jid.removeprefix("qq:channel:")
                await self._send_channel(token, channel_id, text.strip(), msg_id)
            elif jid.startswith("qq:c2c:"):
                openid = jid.removeprefix("qq:c2c:")
                await self._send_c2c(token, openid, text.strip(), msg_id)
            else:
                logger.warning("QQ: unknown JID format — %s", jid)
        except Exception as exc:
            logger.error("QQ send_message failed jid=%s: %s", jid, exc)

    async def set_typing(self, jid: str, is_typing: bool) -> None:
        pass  # QQ Bot API does not support typing indicators

    async def disconnect(self) -> None:
        self._connected = False
        self._stop_event.set()
        if self._http is not None:
            await self._http.close()
            self._http = None

    async def reconnect(self) -> None:
        """Restart the WebSocket thread when it has exited.

        QQ's ws loop already retries with exponential backoff, but if the
        thread itself exits (e.g. stop_event set externally or unhandled
        exception) the health monitor will call this to revive it.
        """
        if not self._available:
            return
        if self._ws_thread is not None and self._ws_thread.is_alive():
            return

        logger.info("QQ: WebSocket thread died — restarting")
        self._stop_event.clear()
        self._ws_thread = threading.Thread(
            target=self._run_ws_forever,
            daemon=True,
            name="qq-ws",
        )
        self._ws_thread.start()
        self._connected = True
        logger.info("QQ: WebSocket thread restarted")

    # ------------------------------------------------------------------
    # Token management
    # ------------------------------------------------------------------

    def _get_token_sync(self) -> str:
        """Synchronous token fetch (called from WebSocket thread)."""
        with self._token_lock:
            if self._token and time.time() < self._token_expires_at - 300:
                return self._token

        import urllib.request
        req = urllib.request.Request(
            _TOKEN_URL,
            data=json.dumps(
                {"appId": self._app_id, "clientSecret": self._app_secret}
            ).encode(),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = json.loads(resp.read().decode())

        token = data.get("access_token")
        if not token:
            raise RuntimeError(f"QQ: no access_token in response: {data}")
        expires_in = int(data.get("expires_in", 7200))
        with self._token_lock:
            self._token = token
            self._token_expires_at = time.time() + expires_in
        return token

    async def _get_token_async(self) -> str:
        """Asynchronous token fetch (called from async send path)."""
        with self._token_lock:
            if self._token and time.time() < self._token_expires_at - 300:
                return self._token

        assert self._http is not None
        async with self._http.post(
            _TOKEN_URL,
            json={"appId": self._app_id, "clientSecret": self._app_secret},
        ) as resp:
            data = await resp.json()
        token = data.get("access_token")
        if not token:
            raise RuntimeError(f"QQ: no access_token: {data}")
        expires_in = int(data.get("expires_in", 7200))
        with self._token_lock:
            self._token = token
            self._token_expires_at = time.time() + expires_in
        return token

    # ------------------------------------------------------------------
    # Send helpers
    # ------------------------------------------------------------------

    async def _api_post(self, token: str, path: str, body: dict) -> dict:
        assert self._http is not None
        url = f"{_API_BASE}{path}"
        async with self._http.post(
            url,
            json=body,
            headers={
                "Authorization": f"QQBot {token}",
                "Content-Type": "application/json",
            },
        ) as resp:
            data = await resp.json()
            if resp.status >= 400:
                logger.error("QQ API %s %s: %s", path, resp.status, data)
            return data

    async def _send_c2c(self, token: str, openid: str, text: str, msg_id: Optional[str]) -> None:
        seq = _next_seq(f"c2c:{openid}")
        body: dict = {"content": text, "msg_type": 0, "msg_seq": seq}
        if msg_id:
            body["msg_id"] = msg_id
        await self._api_post(token, f"/v2/users/{openid}/messages", body)

    async def _send_group(self, token: str, group_openid: str, text: str, msg_id: Optional[str]) -> None:
        seq = _next_seq(f"group:{group_openid}")
        body: dict = {"content": text, "msg_type": 0, "msg_seq": seq}
        if msg_id:
            body["msg_id"] = msg_id
        await self._api_post(token, f"/v2/groups/{group_openid}/messages", body)

    async def _send_channel(self, token: str, channel_id: str, text: str, msg_id: Optional[str]) -> None:
        body: dict = {"content": text}
        if msg_id:
            body["msg_id"] = msg_id
        await self._api_post(token, f"/channels/{channel_id}/messages", body)

    # ------------------------------------------------------------------
    # WebSocket receive loop (background thread)
    # ------------------------------------------------------------------

    def _run_ws_forever(self) -> None:
        """Reconnecting WebSocket loop with exponential backoff."""
        try:
            import websocket
        except ImportError:
            logger.error("QQ: websocket-client not installed")
            return

        reconnect_idx = 0
        session_id: Optional[str] = None
        last_seq: Optional[int] = None
        token: Optional[str] = None

        while not self._stop_event.is_set():
            try:
                token = self._get_token_sync()
            except Exception as exc:
                logger.warning("QQ: token refresh failed: %s", exc)
                delay = _RECONNECT_DELAYS[min(reconnect_idx, len(_RECONNECT_DELAYS) - 1)]
                reconnect_idx += 1
                self._stop_event.wait(delay)
                continue

            # Get WebSocket gateway URL
            try:
                import urllib.request
                gw_req = urllib.request.Request(
                    f"{_API_BASE}/gateway",
                    headers={"Authorization": f"QQBot {token}"},
                    method="GET",
                )
                with urllib.request.urlopen(gw_req, timeout=15) as resp:
                    gw_data = json.loads(resp.read().decode())
                ws_url = gw_data.get("url")
                if not ws_url:
                    raise RuntimeError(f"No url in gateway response: {gw_data}")
            except Exception as exc:
                logger.warning("QQ: gateway fetch failed: %s", exc)
                delay = _RECONNECT_DELAYS[min(reconnect_idx, len(_RECONNECT_DELAYS) - 1)]
                reconnect_idx += 1
                self._stop_event.wait(delay)
                continue

            logger.info("QQ: connecting to %s", ws_url)
            try:
                ws = websocket.create_connection(ws_url, timeout=30)
            except Exception as exc:
                logger.warning("QQ: ws connect failed: %s", exc)
                delay = _RECONNECT_DELAYS[min(reconnect_idx, len(_RECONNECT_DELAYS) - 1)]
                reconnect_idx += 1
                self._stop_event.wait(delay)
                continue

            heartbeat_interval: Optional[float] = None
            heartbeat_timer: Optional[threading.Timer] = None

            def stop_hb() -> None:
                nonlocal heartbeat_timer
                if heartbeat_timer:
                    heartbeat_timer.cancel()
                    heartbeat_timer = None

            def schedule_hb() -> None:
                nonlocal heartbeat_timer
                stop_hb()
                if heartbeat_interval is None or self._stop_event.is_set():
                    return
                def ping() -> None:
                    if self._stop_event.is_set():
                        return
                    try:
                        ws.send(json.dumps({"op": _OP_HEARTBEAT, "d": last_seq}))
                    except Exception:
                        pass
                    schedule_hb()
                heartbeat_timer = threading.Timer(heartbeat_interval / 1000.0, ping)
                heartbeat_timer.daemon = True
                heartbeat_timer.start()

            try:
                while not self._stop_event.is_set():
                    raw = ws.recv()
                    if not raw:
                        break
                    payload = json.loads(raw)
                    op = payload.get("op")
                    d = payload.get("d") or {}
                    s = payload.get("s")
                    t = payload.get("t")

                    if s is not None:
                        last_seq = s

                    if op == _OP_HELLO:
                        heartbeat_interval = d.get("heartbeat_interval", 45000)
                        if session_id and last_seq is not None:
                            # Resume
                            ws.send(json.dumps({
                                "op": _OP_RESUME,
                                "d": {
                                    "token": f"QQBot {token}",
                                    "session_id": session_id,
                                    "seq": last_seq,
                                },
                            }))
                        else:
                            # Identify
                            intents = (
                                _INTENT_PUBLIC_GUILD_MESSAGES
                                | _INTENT_GUILD_MEMBERS
                                | _INTENT_DIRECT_MESSAGE
                                | _INTENT_GROUP_AND_C2C
                            )
                            ws.send(json.dumps({
                                "op": _OP_IDENTIFY,
                                "d": {
                                    "token": f"QQBot {token}",
                                    "intents": intents,
                                    "shard": [0, 1],
                                },
                            }))
                        schedule_hb()

                    elif op == _OP_DISPATCH:
                        if t == "READY":
                            session_id = d.get("session_id")
                            reconnect_idx = 0
                            logger.info("QQ: ready session_id=%s", session_id)
                        elif t == "RESUMED":
                            logger.info("QQ: session resumed")
                        elif t in ("C2C_MESSAGE_CREATE", "GROUP_AT_MESSAGE_CREATE",
                                   "AT_MESSAGE_CREATE", "DIRECT_MESSAGE_CREATE"):
                            self._dispatch_event(t, d)

                    elif op == _OP_RECONNECT:
                        logger.info("QQ: server requested reconnect")
                        break

                    elif op == _OP_INVALID_SESSION:
                        logger.warning("QQ: invalid session, re-identifying")
                        session_id = None
                        last_seq = None
                        break

            except Exception as exc:
                logger.warning("QQ: ws error: %s", exc)
            finally:
                stop_hb()
                try:
                    ws.close()
                except Exception:
                    pass

            if not self._stop_event.is_set():
                delay = _RECONNECT_DELAYS[min(reconnect_idx, len(_RECONNECT_DELAYS) - 1)]
                reconnect_idx = min(reconnect_idx + 1, len(_RECONNECT_DELAYS) - 1)
                logger.info("QQ: reconnecting in %ss", delay)
                self._stop_event.wait(delay)

    # ------------------------------------------------------------------
    # Event dispatch (called from ws thread → async via run_coroutine_threadsafe)
    # ------------------------------------------------------------------

    def _dispatch_event(self, t: str, d: dict) -> None:
        if self._loop and self._loop.is_running():
            asyncio.run_coroutine_threadsafe(
                self._handle_event(t, d), self._loop
            )

    async def _handle_event(self, t: str, d: dict) -> None:
        try:
            msg_id = d.get("id", "")

            # Dedup
            if msg_id:
                if msg_id in self._seen_ids:
                    return
                self._seen_ids.add(msg_id)
                self._seen_ids_list.append(msg_id)
                if len(self._seen_ids_list) > 1000:
                    oldest = self._seen_ids_list.pop(0)
                    self._seen_ids.discard(oldest)

            ts = d.get("timestamp") or _iso_now()
            content = (d.get("content") or "").strip()
            author = d.get("author") or {}

            if t == "C2C_MESSAGE_CREATE":
                openid = author.get("user_openid") or author.get("id") or ""
                if not openid:
                    return
                sender_name = author.get("nickname") or openid
                jid = f"qq:c2c:{openid}"
                is_group = False

            elif t == "GROUP_AT_MESSAGE_CREATE":
                group_openid = d.get("group_openid") or d.get("group_id") or ""
                if not group_openid:
                    return
                sender_name = author.get("nickname") or author.get("member_openid") or ""
                openid = author.get("member_openid") or author.get("id") or ""
                jid = f"qq:group:{group_openid}"
                is_group = True

            elif t in ("AT_MESSAGE_CREATE", "DIRECT_MESSAGE_CREATE"):
                channel_id = d.get("channel_id") or ""
                guild_id = d.get("guild_id") or ""
                openid = author.get("id") or ""
                sender_name = author.get("username") or openid
                if t == "DIRECT_MESSAGE_CREATE":
                    jid = f"qq:c2c:{openid}"
                    is_group = False
                else:
                    if not channel_id:
                        return
                    jid = f"qq:channel:{channel_id}"
                    is_group = True
            else:
                return

            # Strip @bot mention from content (QQ adds it automatically)
            for prefix in (f"<@!{self._app_id}>", f"<@{self._app_id}>"):
                content = content.replace(prefix, "").strip()

            if not content:
                return

            # Skip messages prefixed with assistant name (our own replies)
            if content.startswith(f"{ASSISTANT_NAME}:"):
                return

            # Store last msg_id for reply threading
            if msg_id:
                self._last_msg_id[jid] = msg_id

            # Chat metadata update
            _fire(self._on_chat_meta(jid, ts, jid.split(":")[-1], "qq", is_group))

            new_msg = NewMessage(
                id=msg_id or ts,
                chat_jid=jid,
                sender=openid if "openid" in dir() else author.get("id", ""),
                sender_name=sender_name,
                content=content,
                timestamp=ts,
                is_from_me=False,
                is_bot_message=False,
            )

            if jid in self._registered_groups():
                _fire(self._on_message(jid, new_msg))
            else:
                logger.debug("QQ message from unregistered jid=%s — metadata only", jid)

        except Exception as exc:
            logger.exception("QQ _handle_event error: %s", exc)


def _fire(coro) -> None:  # type: ignore[type-arg]
    asyncio.ensure_future(coro)
