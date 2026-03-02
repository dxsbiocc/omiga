"""
Tests for omiga/channels/discord_.py and the Discord trigger-match flow.

Focuses on the bug where <@BOT_ID> mention is stripped before DB storage,
causing the trigger pattern to never match.
"""

from __future__ import annotations

import asyncio
import re
import sys
from unittest.mock import AsyncMock, MagicMock, patch, patch as _patch

import pytest

from omiga.channels.discord_ import DiscordChannel, _jid_channel, _jid_dm
from omiga.config import ASSISTANT_NAME
from omiga.models import NewMessage, RegisteredGroup
from omiga.router import _clean_content, format_messages


# ---------------------------------------------------------------------------
# Pure helper tests
# ---------------------------------------------------------------------------


def test_jid_channel():
    assert _jid_channel(123456) == "discord:ch:123456"


def test_jid_dm():
    assert _jid_dm(999) == "discord:dm:999"


# ---------------------------------------------------------------------------
# trigger_pattern — before bot_id is known
# ---------------------------------------------------------------------------


def _make_channel(**kwargs) -> DiscordChannel:
    return DiscordChannel(
        token="tok",
        on_message=AsyncMock(),
        on_chat_meta=AsyncMock(),
        registered_groups=lambda: {},
        **kwargs,
    )


def test_trigger_pattern_before_ready():
    """Without bot_id, pattern falls back to plain @Name."""
    ch = _make_channel()
    pat = ch.trigger_pattern
    assert pat.match(f"@{ASSISTANT_NAME} hello")
    assert not pat.match("hello")


def test_trigger_pattern_after_ready():
    """After on_ready sets bot_id, pattern also matches <@BOT_ID>."""
    ch = _make_channel()
    ch._bot_id = 1477841266338173041

    pat = ch.trigger_pattern
    # Discord @mention format — note: \b doesn't work after '>', use (\s|$)
    assert pat.match(f"<@{ch._bot_id}> tell me the weather")
    assert pat.match(f"<@!{ch._bot_id}> legacy mention")
    assert pat.match(f"<@{ch._bot_id}>")  # mention-only, no trailing text
    # Plain text trigger still works
    assert pat.match(f"@{ASSISTANT_NAME} plain text")
    # Random text must NOT match
    assert not pat.match("hello world")
    assert not pat.match("no mention here")


# ---------------------------------------------------------------------------
# _clean_content — router strips mention before container sees it
# ---------------------------------------------------------------------------


def test_clean_content_strips_mention():
    assert _clean_content("<@1234567890> tell me the weather") == "tell me the weather"
    assert _clean_content("<@!1234567890> legacy mention") == "legacy mention"


def test_clean_content_no_mention_unchanged():
    assert _clean_content("@Omiga plain") == "@Omiga plain"
    assert _clean_content("hello world") == "hello world"


def test_clean_content_empty():
    assert _clean_content("") == ""
    assert _clean_content("<@123>") == ""


# ---------------------------------------------------------------------------
# Trigger match vs stored content (the core bug scenario)
# ---------------------------------------------------------------------------


def test_trigger_matches_stored_content_with_mention():
    """
    Stored content includes <@BOT_ID>.  The trigger pattern must match it.
    Previously content was stripped before storage → trigger never fired.
    """
    bot_id = 1477841266338173041
    ch = _make_channel()
    ch._bot_id = bot_id

    pat = ch.trigger_pattern
    stored_content = f"<@{bot_id}> what's the weather today?"
    assert pat.match(stored_content.strip()), (
        f"Trigger pattern must match the raw stored content that includes <@BOT_ID>. "
        f"Pattern: {pat.pattern!r}  Content: {stored_content!r}"
    )


def test_format_messages_strips_mention_for_container():
    """
    format_messages() must strip <@BOT_ID> from content so the container
    receives clean text, not raw snowflake IDs.
    """
    bot_id = 1477841266338173041
    msg = NewMessage(
        id="1",
        chat_jid="discord:ch:999",
        sender="u1",
        sender_name="Alice",
        content=f"<@{bot_id}> what is 2+2?",
        timestamp="2026-01-01T00:00:00+00:00",
        is_from_me=False,
        is_bot_message=False,
    )
    xml = format_messages([msg])
    assert (
        f"<@{bot_id}>" not in xml
    ), "Container prompt must not contain raw snowflake IDs"
    assert "what is 2+2?" in xml


def test_format_messages_plain_trigger_unchanged():
    """Plain @Name trigger text is not a Discord mention and must pass through."""
    msg = NewMessage(
        id="2",
        chat_jid="discord:ch:999",
        sender="u1",
        sender_name="Bob",
        content="@Omiga remind me tomorrow",
        timestamp="2026-01-01T00:00:00+00:00",
        is_from_me=False,
        is_bot_message=False,
    )
    xml = format_messages([msg])
    assert "@Omiga remind me tomorrow" in xml


# ---------------------------------------------------------------------------
# handle_message: original content is stored (not stripped)
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_handle_message_stores_original_content():
    """
    _handle_message must store the original content (including <@BOT_ID>)
    so the trigger-pattern check in the message loop works correctly.
    """
    bot_id = 1477841266338173041
    channel_id = 999000111

    on_message = AsyncMock()
    on_chat_meta = AsyncMock()

    group = RegisteredGroup(
        name="Test",
        folder="testgroup",
        trigger="@Omiga",
        added_at="2026-01-01T00:00:00+00:00",
        requires_trigger=True,
    )
    jid = _jid_channel(channel_id)

    ch = DiscordChannel(
        token="tok",
        on_message=on_message,
        on_chat_meta=on_chat_meta,
        registered_groups=lambda: {jid: group},
    )
    ch._bot_id = bot_id
    ch._available = True  # bypass discord import guard

    # Mock the discord module so the test doesn't require discord.py installed
    mock_discord = MagicMock()
    # isinstance(channel, discord.DMChannel) must return False for a text channel
    mock_discord.DMChannel = type("DMChannel", (), {})

    # Build a fake discord.Message (no spec needed — MagicMock handles attr access)
    fake_channel = MagicMock()
    fake_channel.__class__ = type("TextChannel", (), {})  # not DMChannel
    fake_channel.id = channel_id
    fake_channel.name = "general"

    msg = MagicMock()
    msg.id = 42
    msg.author.bot = False
    msg.author.id = 555
    msg.author.display_name = "Alice"
    msg.author.name = "alice"
    msg.content = f"<@{bot_id}> hello bot"
    msg.attachments = []
    msg.reference = None
    msg.channel = fake_channel

    # discord.py is not installed; inject a mock into sys.modules so that
    # the `import discord` inside _handle_message can resolve.
    with patch.dict(sys.modules, {"discord": mock_discord}):
        await ch._handle_message(msg)

    assert on_message.called, "on_message must be called for registered group"
    stored_msg: NewMessage = on_message.call_args[0][1]

    # The stored content must include the raw mention — NOT be stripped
    assert (
        stored_msg.content == f"<@{bot_id}> hello bot"
    ), f"Expected original content with mention, got: {stored_msg.content!r}"
