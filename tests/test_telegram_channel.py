"""
Tests for nanoclaw/channels/telegram.py

Uses unittest.mock to avoid requiring a real Telegram bot token.
All python-telegram-bot classes are mocked at the module boundary.
"""
from __future__ import annotations

import asyncio
from datetime import datetime, timezone
from unittest.mock import AsyncMock, MagicMock, patch, call

import pytest

from omiga.channels.telegram import (
    TelegramChannel,
    _chat_id,
    _iso_timestamp,
    _jid,
    _split_text,
)
from omiga.models import NewMessage, RegisteredGroup


# ---------------------------------------------------------------------------
# Pure helper tests (no mocking needed)
# ---------------------------------------------------------------------------

def test_jid_positive():
    assert _jid(123456789) == "tg:123456789"


def test_jid_negative_group():
    assert _jid(-1001234567890) == "tg:-1001234567890"


def test_chat_id_positive():
    assert _chat_id("tg:123456789") == 123456789


def test_chat_id_negative():
    assert _chat_id("tg:-1001234567890") == -1001234567890


def test_iso_timestamp_none_returns_utc_string():
    ts = _iso_timestamp(None)
    # Should be a valid ISO string ending with UTC offset
    assert "T" in ts


def test_iso_timestamp_naive_assumes_utc():
    naive_dt = datetime(2024, 1, 1, 12, 0, 0)
    ts = _iso_timestamp(naive_dt)
    assert ts.startswith("2024-01-01T12:00:00")


def test_iso_timestamp_tz_aware():
    aware_dt = datetime(2024, 6, 15, 8, 30, 0, tzinfo=timezone.utc)
    ts = _iso_timestamp(aware_dt)
    assert "2024-06-15" in ts


@pytest.mark.parametrize("text, max_len, expected_chunks", [
    ("short", 100, 1),
    ("a" * 4096, 4096, 1),
    ("a" * 4097, 4096, 2),
    ("a" * 8192, 4096, 2),
    ("a" * 8193, 4096, 3),
])
def test_split_text_chunk_count(text, max_len, expected_chunks):
    chunks = _split_text(text, max_len)
    assert len(chunks) == expected_chunks
    # Reconstructed text should equal original (modulo stripped newlines at splits)
    assert all(len(c) <= max_len for c in chunks)


def test_split_text_short_message_unchanged():
    msg = "Hello world"
    assert _split_text(msg) == [msg]


def test_split_text_prefers_newline_boundary():
    # "line1\nline2" where line1 fits in 8 chars → split at newline
    text = "line1\nline2"
    chunks = _split_text(text, max_len=8)
    assert chunks[0] == "line1"


# ---------------------------------------------------------------------------
# owns_jid
# ---------------------------------------------------------------------------

def _make_channel(**kwargs) -> TelegramChannel:
    defaults = dict(
        token="test-token",
        on_message=AsyncMock(),
        on_chat_meta=AsyncMock(),
        registered_groups=lambda: {},
    )
    defaults.update(kwargs)
    return TelegramChannel(**defaults)


def test_owns_jid_telegram():
    ch = _make_channel()
    assert ch.owns_jid("tg:123456") is True
    assert ch.owns_jid("tg:-9999999") is True


def test_owns_jid_other_channels():
    ch = _make_channel()
    assert ch.owns_jid("1234@g.us") is False
    assert ch.owns_jid("1234@s.whatsapp.net") is False
    assert ch.owns_jid("dc:guild:channel") is False


def test_not_connected_before_connect():
    ch = _make_channel()
    assert ch.is_connected() is False


# ---------------------------------------------------------------------------
# connect / disconnect
# ---------------------------------------------------------------------------

def _mock_app(bot_id: int = 111, bot_username: str = "testbot"):
    """Return a mock Application with the minimal API surface used."""
    bot_info = MagicMock()
    bot_info.id = bot_id
    bot_info.username = bot_username

    bot = MagicMock()
    bot.get_me = AsyncMock(return_value=bot_info)
    bot.send_message = AsyncMock()
    bot.send_chat_action = AsyncMock()

    updater = MagicMock()
    updater.running = True
    updater.start_polling = AsyncMock()
    updater.stop = AsyncMock()

    app = MagicMock()
    app.bot = bot
    app.updater = updater
    app.initialize = AsyncMock()
    app.start = AsyncMock()
    app.stop = AsyncMock()
    app.shutdown = AsyncMock()
    app.add_handler = MagicMock()
    return app


def _patch_builder(MockApp, mock_app):
    """Wire up the fluent builder chain used in TelegramChannel.connect().

    The builder pattern is:
        Application.builder().token(t).get_updates_read_timeout(n).build()
    Each method returns the builder itself so they can be chained.
    """
    builder = MagicMock()
    # Every fluent method returns the builder itself
    builder.token.return_value = builder
    builder.get_updates_read_timeout.return_value = builder
    builder.build.return_value = mock_app
    MockApp.builder.return_value = builder
    return builder


async def test_connect_sets_connected_and_stores_bot_id():
    ch = _make_channel()
    mock_app = _mock_app(bot_id=42, bot_username="mybot")

    with patch("omiga.channels.telegram.Application") as MockApp:
        _patch_builder(MockApp, mock_app)
        await ch.connect()

    assert ch.is_connected() is True
    assert ch._bot_id == 42
    assert ch._bot_username == "mybot"
    mock_app.updater.start_polling.assert_called_once()
    _, kwargs = mock_app.updater.start_polling.call_args
    assert kwargs.get("drop_pending_updates") is True
    assert "timeout" in kwargs


async def test_disconnect_stops_polling_and_clears_state():
    ch = _make_channel()
    mock_app = _mock_app()

    with patch("omiga.channels.telegram.Application") as MockApp:
        _patch_builder(MockApp, mock_app)
        await ch.connect()
        await ch.disconnect()

    assert ch.is_connected() is False
    assert ch._app is None
    mock_app.updater.stop.assert_called_once()
    mock_app.stop.assert_called_once()
    mock_app.shutdown.assert_called_once()


# ---------------------------------------------------------------------------
# send_message
# ---------------------------------------------------------------------------

async def test_send_message_short_text():
    ch = _make_channel()
    mock_app = _mock_app()

    with patch("omiga.channels.telegram.Application") as MockApp:
        _patch_builder(MockApp, mock_app)
        await ch.connect()
        await ch.send_message("tg:9999", "Hello!")

    mock_app.bot.send_message.assert_called_once_with(
        chat_id=9999, text="Hello!", reply_to_message_id=None
    )


async def test_send_message_long_text_is_split():
    ch = _make_channel()
    mock_app = _mock_app()
    long_text = "x" * 5000

    with patch("omiga.channels.telegram.Application") as MockApp:
        _patch_builder(MockApp, mock_app)
        await ch.connect()
        await ch.send_message("tg:9999", long_text)

    # Should send 2 chunks
    assert mock_app.bot.send_message.call_count == 2


async def test_send_message_when_not_connected_logs_warning(caplog):
    import logging
    ch = _make_channel()
    with caplog.at_level(logging.WARNING, logger="omiga.channels.telegram"):
        await ch.send_message("tg:9999", "Hello")
    assert "not connected" in caplog.text.lower()


# ---------------------------------------------------------------------------
# _handle_text — inbound message routing
# ---------------------------------------------------------------------------

def _make_update(
    chat_id: int,
    chat_type: str,
    text: str,
    user_id: int = 777,
    user_name: str = "Alice",
    message_id: int = 1,
    date: datetime = None,
):
    """Build a minimal fake telegram Update for handler tests."""
    msg = MagicMock()
    msg.text = text
    msg.message_id = message_id
    msg.date = date or datetime(2024, 1, 1, 12, 0, 0, tzinfo=timezone.utc)

    user = MagicMock()
    user.id = user_id
    user.full_name = user_name

    chat = MagicMock()
    chat.id = chat_id
    chat.type = chat_type
    chat.title = "Test Group" if chat_type in ("group", "supergroup") else None

    update = MagicMock()
    update.effective_message = msg
    update.effective_chat = chat
    update.effective_user = user

    return update


async def test_handle_text_calls_on_chat_meta_always():
    on_message = AsyncMock()
    on_chat_meta = AsyncMock()
    ch = _make_channel(on_message=on_message, on_chat_meta=on_chat_meta)
    ch._app = _mock_app()
    ch._bot_id = 999
    ch._connected = True

    update = _make_update(chat_id=-100111, chat_type="supergroup", text="Hi")
    await ch._handle_text(update, MagicMock())
    await asyncio.sleep(0.01)  # let ensure_future tasks run

    on_chat_meta.assert_called_once()
    args = on_chat_meta.call_args[0]
    assert args[0] == "tg:-100111"    # jid
    assert args[3] == "telegram"       # channel name
    assert args[4] is True             # is_group


async def test_handle_text_on_message_only_for_registered_groups():
    on_message = AsyncMock()
    on_chat_meta = AsyncMock()

    registered_jid = "tg:-100111"
    groups = {
        registered_jid: RegisteredGroup(
            name="TestGroup", folder="main", trigger="@Andy",
            added_at="2024-01-01T00:00:00Z",
        )
    }
    ch = _make_channel(
        on_message=on_message,
        on_chat_meta=on_chat_meta,
        registered_groups=lambda: groups,
    )
    ch._app = _mock_app()
    ch._bot_id = 999
    ch._connected = True

    # Registered group → on_message called
    update = _make_update(chat_id=-100111, chat_type="supergroup", text="@Andy hello")
    await ch._handle_text(update, MagicMock())
    await asyncio.sleep(0.01)
    on_message.assert_called_once()

    # Unregistered group → on_message NOT called
    on_message.reset_mock()
    update2 = _make_update(chat_id=-999999, chat_type="supergroup", text="some text")
    await ch._handle_text(update2, MagicMock())
    await asyncio.sleep(0.01)
    on_message.assert_not_called()


async def test_handle_text_bot_own_message_marked_as_bot():
    on_message = AsyncMock()
    ch = _make_channel(
        on_message=on_message,
        registered_groups=lambda: {"tg:100": MagicMock()},
    )
    ch._app = _mock_app()
    ch._bot_id = 777   # matches user_id in update
    ch._connected = True

    update = _make_update(chat_id=100, chat_type="private", user_id=777, text="reply")
    await ch._handle_text(update, MagicMock())
    await asyncio.sleep(0.01)

    call_args = on_message.call_args[0]
    new_msg: NewMessage = call_args[1]
    assert new_msg.is_from_me is True
    assert new_msg.is_bot_message is True


async def test_handle_text_message_id_format():
    on_message = AsyncMock()
    ch = _make_channel(
        on_message=on_message,
        registered_groups=lambda: {"tg:55": MagicMock()},
    )
    ch._app = _mock_app()
    ch._bot_id = 999
    ch._connected = True

    update = _make_update(chat_id=55, chat_type="private", text="hello", message_id=42)
    await ch._handle_text(update, MagicMock())
    await asyncio.sleep(0.01)

    new_msg: NewMessage = on_message.call_args[0][1]
    assert new_msg.id == "42"
    assert new_msg.chat_jid == "tg:55"


async def test_handle_text_dm_is_not_group():
    on_chat_meta = AsyncMock()
    ch = _make_channel(
        on_chat_meta=on_chat_meta,
        registered_groups=lambda: {"tg:42": MagicMock()},
    )
    ch._app = _mock_app()
    ch._bot_id = 999
    ch._connected = True

    update = _make_update(chat_id=42, chat_type="private", text="ping")
    await ch._handle_text(update, MagicMock())
    await asyncio.sleep(0.01)

    args = on_chat_meta.call_args[0]
    assert args[4] is False   # is_group=False for DM
