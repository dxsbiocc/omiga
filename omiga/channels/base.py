"""
Channel abstraction for Omiga Python port.

Mirrors the Channel interface from src/types.ts.
Concrete channel implementations (WhatsApp, Telegram, Slack …) inherit
from this ABC and implement all abstract methods.
"""
from __future__ import annotations

import re
from abc import ABC, abstractmethod
from typing import Any, Callable, Optional

from pathlib import Path

from omiga.models import NewMessage, RegisteredGroup

# Callback types
OnInboundMessage = Callable[[str, NewMessage], None]
OnChatMetadata = Callable[[str, str, Optional[str], Optional[str], Optional[bool]], None]


class Channel(ABC):
    """Abstract base class for Omiga messaging channels."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Human-readable channel name (e.g. 'whatsapp', 'telegram')."""

    @abstractmethod
    async def connect(self) -> None:
        """Establish connection / authenticate."""

    @abstractmethod
    async def send_message(self, jid: str, text: str) -> None:
        """Send *text* to the chat identified by *jid*."""

    @abstractmethod
    def is_connected(self) -> bool:
        """Return True if the channel is currently connected."""

    @abstractmethod
    def owns_jid(self, jid: str) -> bool:
        """Return True if this channel is responsible for *jid*."""

    @abstractmethod
    async def disconnect(self) -> None:
        """Gracefully disconnect from the channel."""

    @property
    def trigger_pattern(self) -> Optional[re.Pattern]:
        """Channel-specific trigger pattern, or None to use the global one.

        Override in subclasses that have their own natural mention syntax
        (e.g. Telegram uses @bot_username, not @ASSISTANT_NAME).
        """
        return None

    async def reconnect(self) -> None:
        """Re-establish a lost connection.

        Called by the channel health monitor when ``is_connected()`` returns
        False.  The default no-op is sufficient for channels that handle
        reconnection internally (e.g. python-telegram-bot).  Override for
        channels that run their own background thread/task and cannot self-
        heal.
        """

    async def send_file(
        self,
        jid: str,
        host_path: Path,
        caption: str = "",
    ) -> None:
        """Send a file to the chat identified by *jid*.

        *host_path* is the absolute path to the file on the host machine.
        *caption* is optional accompanying text shown alongside the file.

        Default implementation falls back to sending the filename as a text
        message so channels that haven't implemented native file sending still
        produce some output.  Override in concrete channel implementations.
        """
        name = host_path.name
        fallback = f"[{name}]" if not caption else f"{caption}\n[{name}]"
        await self.send_message(jid, fallback)

    async def set_typing(self, jid: str, is_typing: bool) -> None:
        """Optional typing indicator — no-op by default."""

    def set_enqueue(self, callback: Optional[Callable[[Any], None]]) -> None:
        """Set the enqueue callback for queue-based message processing.

        This is used by ChannelManager to enqueue inbound messages.
        No-op by default for channels that don't use queue-based processing.
        """
        pass


class StubChannel(Channel):
    """
    Minimal stub channel for standalone testing.

    Owns all JIDs not owned by other channels.
    Prints sent messages to stdout instead of transmitting them.
    """

    def __init__(
        self,
        on_message: Optional[OnInboundMessage] = None,
        on_chat_metadata: Optional[OnChatMetadata] = None,
    ) -> None:
        self._on_message = on_message
        self._on_chat_metadata = on_chat_metadata
        self._connected = False

    @property
    def name(self) -> str:
        return "stub"

    async def connect(self) -> None:
        self._connected = True

    async def send_message(self, jid: str, text: str) -> None:
        print(f"[StubChannel → {jid}] {text}")

    def is_connected(self) -> bool:
        return self._connected

    def owns_jid(self, jid: str) -> bool:
        return True  # stub catches everything

    async def disconnect(self) -> None:
        self._connected = False
