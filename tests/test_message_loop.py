"""Tests for omiga/message_loop.py — typing_safe and recover_pending_messages."""
from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock, patch

import pytest

import omiga.state as state
from omiga.message_loop import recover_pending_messages, typing_safe
from omiga.models import RegisteredGroup


@pytest.fixture(autouse=True)
def reset_state():
    state._registered_groups = {}
    state._last_agent_timestamp = {}
    state._last_timestamp = ""
    state._channels = []
    state._debounce_deadlines = {}
    state._message_loop_running = False
    state._shutdown_event = None
    yield
    state._registered_groups = {}
    state._last_agent_timestamp = {}
    state._last_timestamp = ""
    state._channels = []
    state._debounce_deadlines = {}
    state._message_loop_running = False


def _make_group(folder="g") -> RegisteredGroup:
    return RegisteredGroup(
        name="G", folder=folder, trigger="@bot", added_at="2024-01-01T00:00:00Z",
    )


# ---------------------------------------------------------------------------
# typing_safe
# ---------------------------------------------------------------------------


async def test_typing_safe_calls_channel():
    channel = MagicMock()
    channel.set_typing = AsyncMock()
    await typing_safe(channel, "jid@g.us", True)
    channel.set_typing.assert_awaited_once_with("jid@g.us", True)


async def test_typing_safe_swallows_exception(caplog):
    channel = MagicMock()
    channel.set_typing = AsyncMock(side_effect=RuntimeError("network error"))

    # Should not raise
    await typing_safe(channel, "jid@g.us", True)

    assert "Failed to set typing indicator" in caplog.text


async def test_typing_safe_false_value():
    channel = MagicMock()
    channel.set_typing = AsyncMock()
    await typing_safe(channel, "jid@g.us", False)
    channel.set_typing.assert_awaited_once_with("jid@g.us", False)


# ---------------------------------------------------------------------------
# recover_pending_messages
# ---------------------------------------------------------------------------


def test_recover_pending_enqueues_lagging_groups():
    """Groups whose agent cursor lags behind global cursor should be enqueued."""
    state._last_timestamp = "2024-01-01T00:01:00Z"
    state._registered_groups = {"jid@g.us": _make_group()}
    state._last_agent_timestamp = {"jid@g.us": "2024-01-01T00:00:00Z"}  # behind

    enqueued = []
    state._queue = MagicMock()
    state._queue.enqueue_message_check = lambda jid: enqueued.append(jid)

    recover_pending_messages()

    assert "jid@g.us" in enqueued


def test_recover_pending_skips_current_groups():
    """Groups with cursor >= global cursor need no recovery."""
    state._last_timestamp = "2024-01-01T00:01:00Z"
    state._registered_groups = {"jid@g.us": _make_group()}
    state._last_agent_timestamp = {"jid@g.us": "2024-01-01T00:01:00Z"}  # same

    enqueued = []
    state._queue = MagicMock()
    state._queue.enqueue_message_check = lambda jid: enqueued.append(jid)

    recover_pending_messages()

    assert enqueued == []


def test_recover_pending_multiple_groups_mixed():
    state._last_timestamp = "2024-01-01T00:02:00Z"
    state._registered_groups = {
        "jid1@g.us": _make_group("g1"),
        "jid2@g.us": _make_group("g2"),
        "jid3@g.us": _make_group("g3"),
    }
    state._last_agent_timestamp = {
        "jid1@g.us": "2024-01-01T00:01:00Z",  # lagging — needs recovery
        "jid2@g.us": "2024-01-01T00:02:00Z",  # current — skip
        # jid3 missing — treated as "" which is < global_ts — needs recovery
    }

    enqueued = []
    state._queue = MagicMock()
    state._queue.enqueue_message_check = lambda jid: enqueued.append(jid)

    recover_pending_messages()

    assert "jid1@g.us" in enqueued
    assert "jid3@g.us" in enqueued
    assert "jid2@g.us" not in enqueued


def test_recover_pending_empty_registered():
    state._last_timestamp = "2024-01-01T00:01:00Z"
    state._registered_groups = {}
    state._queue = MagicMock()
    state._queue.enqueue_message_check = MagicMock()

    recover_pending_messages()

    state._queue.enqueue_message_check.assert_not_called()
