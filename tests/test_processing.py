"""Tests for omiga/processing.py — message processing pipeline."""
from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock, patch

import pytest

import omiga.state as state
from omiga.models import NewMessage, RegisteredGroup
from omiga.processing import notify_error, process_admin_command, process_group_messages


@pytest.fixture(autouse=True)
def reset_state():
    state._channels = []
    state._registered_groups = {}
    state._last_agent_timestamp = {}
    state._consecutive_errors = {}
    state._sessions = {}
    yield
    state._channels = []
    state._registered_groups = {}
    state._last_agent_timestamp = {}
    state._consecutive_errors = {}
    state._sessions = {}


def _make_group(folder="g", requires_trigger=True) -> RegisteredGroup:
    return RegisteredGroup(
        name="G", folder=folder, trigger="@bot",
        added_at="2024-01-01T00:00:00Z", requires_trigger=requires_trigger,
    )


def _make_msg(content="hello", ts="2024-01-01T00:00:01Z") -> NewMessage:
    return NewMessage(
        id="1", chat_jid="jid@g.us", sender="s", sender_name="Alice",
        content=content, timestamp=ts,
    )


def _make_channel(connected=True):
    ch = MagicMock()
    ch.is_connected.return_value = connected
    ch.send_message = AsyncMock()
    ch.send_file = AsyncMock()
    ch.set_typing = AsyncMock()
    ch.jids = ["jid@g.us"]
    return ch


# ---------------------------------------------------------------------------
# notify_error
# ---------------------------------------------------------------------------


async def test_notify_error_no_main_jid():
    """No-op when MAIN_GROUP_JID is not configured."""
    with patch("omiga.processing.MAIN_GROUP_JID", ""):
        await notify_error(_make_group(), "jid@g.us")
    # Should not raise or send anything


async def test_notify_error_skip_when_chat_is_main():
    """No-op when the failing group IS the main group (avoid loops)."""
    with patch("omiga.processing.MAIN_GROUP_JID", "main@g.us"):
        await notify_error(_make_group(), "main@g.us")


async def test_notify_error_sends_to_main_group():
    main_ch = _make_channel(connected=True)

    with (
        patch("omiga.processing.MAIN_GROUP_JID", "main@g.us"),
        patch("omiga.processing.find_channel", return_value=main_ch),
    ):
        await notify_error(_make_group(), "other@g.us")

    main_ch.send_message.assert_called_once()
    call_args = main_ch.send_message.call_args[0]
    assert "main@g.us" == call_args[0]
    assert "Container error" in call_args[1]


async def test_notify_error_silent_when_channel_disconnected():
    main_ch = _make_channel(connected=False)

    with (
        patch("omiga.processing.MAIN_GROUP_JID", "main@g.us"),
        patch("omiga.processing.find_channel", return_value=main_ch),
    ):
        await notify_error(_make_group(), "other@g.us")

    main_ch.send_message.assert_not_called()


# ---------------------------------------------------------------------------
# process_admin_command
# ---------------------------------------------------------------------------


async def test_process_admin_command_non_admin_message():
    channel = _make_channel()
    result = await process_admin_command("jid@g.us", channel, [_make_msg("regular message")])
    assert result is False


async def test_process_admin_command_empty_list():
    channel = _make_channel()
    result = await process_admin_command("jid@g.us", channel, [])
    assert result is False


async def test_process_admin_command_task_cmd_any_group():
    """A /task subcommand should be handled from any registered group."""
    channel = _make_channel()
    msg = _make_msg("/task list")

    with (
        patch("omiga.processing.is_admin_command", return_value=True),
        patch("omiga.processing.handle_task_command", AsyncMock(return_value="task reply")),
        patch("omiga.processing.state") as mock_state,
    ):
        mock_state._registered_groups = {}
        mock_state._last_agent_timestamp = {}
        mock_state.save_state = AsyncMock()
        result = await process_admin_command("jid@g.us", channel, [msg])

    assert result is True
    channel.send_message.assert_called_once()


async def test_process_admin_command_non_task_blocked_from_non_main():
    """Non-/task admin commands must come from MAIN_GROUP_JID."""
    channel = _make_channel()
    msg = _make_msg("/register something")

    with (
        patch("omiga.processing.is_admin_command", return_value=True),
        patch("omiga.processing.MAIN_GROUP_JID", "main@g.us"),
    ):
        result = await process_admin_command("other@g.us", channel, [msg])

    assert result is False
    channel.send_message.assert_not_called()


# ---------------------------------------------------------------------------
# process_group_messages
# ---------------------------------------------------------------------------


async def test_process_group_messages_no_group():
    """Unregistered JID → True immediately."""
    state._registered_groups = {}
    result = await process_group_messages("unknown@g.us")
    assert result is True


async def test_process_group_messages_no_channel():
    group = _make_group()
    state._registered_groups = {"jid@g.us": group}

    with patch("omiga.processing.find_channel", return_value=None):
        result = await process_group_messages("jid@g.us")

    assert result is True


async def test_process_group_messages_no_messages():
    group = _make_group()
    state._registered_groups = {"jid@g.us": group}
    channel = _make_channel()

    with (
        patch("omiga.processing.find_channel", return_value=channel),
        patch("omiga.processing.get_messages_since", AsyncMock(return_value=[])),
    ):
        result = await process_group_messages("jid@g.us")

    assert result is True


async def test_process_group_messages_no_trigger_skips():
    """Non-main group without trigger word → return True without running agent."""
    import re
    group = _make_group(requires_trigger=True)
    state._registered_groups = {"jid@g.us": group}
    channel = _make_channel()
    channel.trigger_pattern = re.compile(r"@bot")

    msgs = [_make_msg("just a normal message")]

    with (
        patch("omiga.processing.find_channel", return_value=channel),
        patch("omiga.processing.get_messages_since", AsyncMock(return_value=msgs)),
        patch("omiga.processing.process_admin_command", AsyncMock(return_value=False)),
        patch("omiga.processing.effective_trigger", return_value=re.compile(r"@bot")),
        patch("omiga.processing.MAIN_GROUP_FOLDER", "main"),
    ):
        result = await process_group_messages("jid@g.us")

    assert result is True


async def test_process_group_messages_success():
    """Happy path: messages found, agent succeeds → True."""
    import re
    group = _make_group(folder="main", requires_trigger=False)
    state._registered_groups = {"main@g.us": group}
    state._last_agent_timestamp = {}
    channel = _make_channel()

    msgs = [_make_msg("do something", ts="2024-01-01T00:00:10Z")]

    with (
        patch("omiga.processing.find_channel", return_value=channel),
        patch("omiga.processing.get_messages_since", AsyncMock(return_value=msgs)),
        patch("omiga.processing.process_admin_command", AsyncMock(return_value=False)),
        patch("omiga.processing.MAIN_GROUP_FOLDER", "main"),
        patch("omiga.processing.format_messages", return_value="<messages/>"),
        patch("omiga.processing.state.save_state", AsyncMock()),
        patch("omiga.processing.run_agent", AsyncMock(return_value="success")),
    ):
        result = await process_group_messages("main@g.us")

    assert result is True


async def test_process_group_messages_error_rolls_back_cursor():
    """On agent error with no output sent, cursor is rolled back → False."""
    import re
    group = _make_group(folder="main", requires_trigger=False)
    state._registered_groups = {"main@g.us": group}
    state._last_agent_timestamp = {"main@g.us": "prev-ts"}
    state._consecutive_errors = {}
    channel = _make_channel()

    msgs = [_make_msg("fail me", ts="2024-01-01T00:00:10Z")]

    with (
        patch("omiga.processing.find_channel", return_value=channel),
        patch("omiga.processing.get_messages_since", AsyncMock(return_value=msgs)),
        patch("omiga.processing.process_admin_command", AsyncMock(return_value=False)),
        patch("omiga.processing.MAIN_GROUP_FOLDER", "main"),
        patch("omiga.processing.format_messages", return_value="<messages/>"),
        patch("omiga.processing.state.save_state", AsyncMock()),
        patch("omiga.processing.run_agent", AsyncMock(return_value="error")),
        patch("omiga.processing.notify_error", AsyncMock()),
    ):
        result = await process_group_messages("main@g.us")

    assert result is False
    # Cursor rolled back
    assert state._last_agent_timestamp.get("main@g.us") == "prev-ts"
    assert state._consecutive_errors.get("main@g.us") == 1


async def test_process_group_messages_error_advances_after_max_retries():
    """After MAX_ROLLBACK_RETRIES consecutive errors, cursor advances instead of rolling back."""
    group = _make_group(folder="main", requires_trigger=False)
    state._registered_groups = {"main@g.us": group}
    state._last_agent_timestamp = {"main@g.us": "prev-ts"}
    state._consecutive_errors = {"main@g.us": state.MAX_ROLLBACK_RETRIES}
    channel = _make_channel()

    msgs = [_make_msg("fail again", ts="2024-01-01T00:00:10Z")]

    with (
        patch("omiga.processing.find_channel", return_value=channel),
        patch("omiga.processing.get_messages_since", AsyncMock(return_value=msgs)),
        patch("omiga.processing.process_admin_command", AsyncMock(return_value=False)),
        patch("omiga.processing.MAIN_GROUP_FOLDER", "main"),
        patch("omiga.processing.format_messages", return_value="<messages/>"),
        patch("omiga.processing.state.save_state", AsyncMock()),
        patch("omiga.processing.run_agent", AsyncMock(return_value="error")),
        patch("omiga.processing.notify_error", AsyncMock()),
    ):
        result = await process_group_messages("main@g.us")

    # Cursor NOT rolled back — advancing to avoid infinite loop
    assert result is True
    assert "main@g.us" not in state._consecutive_errors
