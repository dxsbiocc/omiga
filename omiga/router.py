"""
Router/formatter module for Omiga Python port.

Mirrors src/router.ts — message XML formatting and channel routing.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Optional

# Discord mention prefix: <@BOT_ID> or <@!BOT_ID> (legacy nickname mention)
_DISCORD_MENTION_RE = re.compile(r"^<@!?\d+>\s*")

# File-send directive written by the agent in its reply:
#   [SEND_FILE: path/to/file.png]
#   [SEND_FILE: path/to/report.pdf | Here is your report]
# path is relative to /workspace/group/ (the group's workspace root).
_FILE_DIRECTIVE_RE = re.compile(
    r"\[SEND_FILE:\s*([^\]|]+?)(?:\s*\|\s*([^\]]*?))?\s*\]",
    re.IGNORECASE,
)


@dataclass
class FileDirective:
    """A [SEND_FILE:] directive extracted from agent output."""
    workspace_rel_path: str   # relative to /workspace/group/, e.g. "output/chart.png"
    caption: str = ""

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


def _clean_content(content: str) -> str:
    """Strip channel-specific trigger prefixes before sending to the container.

    Currently strips Discord @-mention prefixes (<@ID> / <@!ID>) so the
    container receives clean text rather than raw snowflake IDs.
    The original content (with the mention) is stored in the DB so that
    the trigger-pattern check in the message loop still works.
    """
    return _DISCORD_MENTION_RE.sub("", content).strip()


def format_messages(messages: list[NewMessage]) -> str:
    """Format a list of messages into an XML block for the container."""
    lines = []
    for m in messages:
        open_tag = f'<message sender="{escape_xml(m.sender_name)}" time="{m.timestamp}">'
        inner_parts: list[str] = []

        if m.reply_to:
            rt = m.reply_to
            inner_parts.append(
                f'  <reply_to sender="{escape_xml(rt.sender_name)}">'
                f"{escape_xml(rt.content[:200])}</reply_to>"
            )

        if m.attachments:
            for a in m.attachments:
                inner_parts.append(
                    f'  <attachment type="{a.type}" file="{escape_xml(a.local_path)}"'
                    f' filename="{escape_xml(a.filename)}" />'
                )

        clean = _clean_content(m.content)
        if inner_parts:
            body = "\n".join(inner_parts)
            lines.append(f"{open_tag}\n{escape_xml(clean)}\n{body}\n</message>")
        else:
            lines.append(f"{open_tag}{escape_xml(clean)}</message>")
    return "<messages>\n" + "\n".join(lines) + "\n</messages>"


def strip_internal_tags(text: str) -> str:
    """Remove <internal>…</internal> reasoning blocks from agent output."""
    return re.sub(r"<internal>[\s\S]*?</internal>", "", text).strip()


def format_outbound(raw_text: str) -> str:
    """Strip internal tags and return cleaned outbound text (empty string if nothing left)."""
    text = strip_internal_tags(raw_text)
    return text


def parse_file_directives(text: str) -> tuple[str, list[FileDirective]]:
    """Extract [SEND_FILE: path] directives from agent output.

    Returns ``(clean_text, directives)`` where *clean_text* has all
    ``[SEND_FILE: ...]`` markers removed so only prose remains.

    Example::

        text = "Here is the chart you asked for:\\n[SEND_FILE: output/chart.png | Monthly chart]"
        clean, files = parse_file_directives(text)
        # clean  → "Here is the chart you asked for:"
        # files  → [FileDirective("output/chart.png", "Monthly chart")]
    """
    directives: list[FileDirective] = []

    def _replace(m: re.Match) -> str:
        path = m.group(1).strip()
        caption = (m.group(2) or "").strip()
        directives.append(FileDirective(workspace_rel_path=path, caption=caption))
        return ""

    clean = _FILE_DIRECTIVE_RE.sub(_replace, text).strip()
    return clean, directives


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
