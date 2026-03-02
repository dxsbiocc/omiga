"""
Router/formatter module for Omiga Python port.

Mirrors src/router.ts — message XML formatting and channel routing.
"""
from __future__ import annotations

import re
from typing import Optional

from omiga.channels.base import Channel
from omiga.models import NewMessage


def escape_xml(s: str) -> str:
    if not s:
        return ""
    return (
        s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def format_messages(messages: list[NewMessage]) -> str:
    """Format a list of messages into an XML block for the container."""
    lines = [
        f'<message sender="{escape_xml(m.sender_name)}" time="{m.timestamp}">'
        f"{escape_xml(m.content)}</message>"
        for m in messages
    ]
    return "<messages>\n" + "\n".join(lines) + "\n</messages>"


def strip_internal_tags(text: str) -> str:
    """Remove <internal>…</internal> reasoning blocks from agent output."""
    return re.sub(r"<internal>[\s\S]*?</internal>", "", text).strip()


def format_outbound(raw_text: str) -> str:
    """Strip internal tags and return cleaned outbound text (empty string if nothing left)."""
    text = strip_internal_tags(raw_text)
    return text


def find_channel(channels: list[Channel], jid: str) -> Optional[Channel]:
    """Return the first channel that owns *jid*, or None."""
    for ch in channels:
        if ch.owns_jid(jid):
            return ch
    return None


async def route_outbound(channels: list[Channel], jid: str, text: str) -> None:
    """Send *text* to *jid* via the owning channel. Raises if none found."""
    ch = find_channel(channels, jid)
    if not ch:
        raise ValueError(f"No channel for JID: {jid}")
    await ch.send_message(jid, text)
