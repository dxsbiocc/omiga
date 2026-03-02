"""Tests for omiga/agent.py — container agent runner."""
from __future__ import annotations

import re
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

import omiga.state as state
from omiga.agent import effective_trigger, is_session_corruption_error, run_agent
from omiga.models import RegisteredGroup


@pytest.fixture(autouse=True)
def reset_state():
    state._sessions = {}
    state._registered_groups = {}
    yield
    state._sessions = {}
    state._registered_groups = {}


# ---------------------------------------------------------------------------
# is_session_corruption_error
# ---------------------------------------------------------------------------


def test_corruption_openai_double_quote():
    err = 'messages with role "tool" must be a response to a preceeding message with "tool_calls"'
    assert is_session_corruption_error(err) is True


def test_corruption_openai_single_quote():
    err = "messages with role 'tool' must be a response to a preceeding message with 'tool_calls'"
    assert is_session_corruption_error(err) is True


def test_corruption_anthropic_marker():
    err = "tool_result block(s) provided when previous message does not have tool_calls"
    assert is_session_corruption_error(err) is True


def test_corruption_case_insensitive():
    err = "TOOL_RESULT BLOCK(S) PROVIDED WHEN PREVIOUS MESSAGE DOES NOT HAVE TOOL_CALLS"
    assert is_session_corruption_error(err) is True


def test_corruption_unrelated_error():
    assert is_session_corruption_error("connection timeout after 30s") is False


def test_corruption_empty_string():
    assert is_session_corruption_error("") is False


# ---------------------------------------------------------------------------
# effective_trigger
# ---------------------------------------------------------------------------


def test_effective_trigger_returns_channel_pattern():
    channel = MagicMock()
    channel.trigger_pattern = re.compile(r"@mybot")
    assert effective_trigger(channel) is channel.trigger_pattern


def test_effective_trigger_falls_back_to_config():
    channel = MagicMock()
    channel.trigger_pattern = None
    sentinel = re.compile(r"@fallback")
    # TRIGGER_PATTERN is imported inside the function body, so patch at the source
    with patch("omiga.config.TRIGGER_PATTERN", sentinel):
        result = effective_trigger(channel)
    assert result is sentinel


# ---------------------------------------------------------------------------
# run_agent — success / error / session corruption retry
# ---------------------------------------------------------------------------


def _make_group(folder="g") -> RegisteredGroup:
    return RegisteredGroup(
        name="G", folder=folder, trigger="@bot", added_at="2024-01-01T00:00:00Z",
    )


def _mock_output(status="success", error=None, new_session_id=None):
    out = MagicMock()
    out.status = status
    out.error = error
    out.new_session_id = new_session_id
    return out


async def test_run_agent_success():
    group = _make_group()
    state._sessions = {}

    with (
        patch("omiga.agent.get_all_tasks", AsyncMock(return_value=[])),
        patch("omiga.agent.get_all_chats", AsyncMock(return_value=[])),
        patch("omiga.agent.write_tasks_snapshot"),
        patch("omiga.agent.write_groups_snapshot"),
        patch("omiga.agent.run_container_agent", AsyncMock(return_value=_mock_output("success"))),
    ):
        result = await run_agent(group, "hello", "jid@g.us")

    assert result == "success"


async def test_run_agent_error():
    group = _make_group()
    state._sessions = {}

    with (
        patch("omiga.agent.get_all_tasks", AsyncMock(return_value=[])),
        patch("omiga.agent.get_all_chats", AsyncMock(return_value=[])),
        patch("omiga.agent.write_tasks_snapshot"),
        patch("omiga.agent.write_groups_snapshot"),
        patch("omiga.agent.run_container_agent", AsyncMock(return_value=_mock_output("error", "boom"))),
    ):
        result = await run_agent(group, "hello", "jid@g.us")

    assert result == "error"


async def test_run_agent_saves_new_session_id():
    group = _make_group()
    state._sessions = {}

    with (
        patch("omiga.agent.get_all_tasks", AsyncMock(return_value=[])),
        patch("omiga.agent.get_all_chats", AsyncMock(return_value=[])),
        patch("omiga.agent.write_tasks_snapshot"),
        patch("omiga.agent.write_groups_snapshot"),
        patch("omiga.agent.run_container_agent",
              AsyncMock(return_value=_mock_output("success", new_session_id="new-sess"))),
        patch("omiga.agent.set_session", AsyncMock()) as mock_set,
    ):
        await run_agent(group, "hello", "jid@g.us")

    assert state._sessions.get("g") == "new-sess"
    mock_set.assert_called()


async def test_run_agent_session_corruption_retries_with_fresh_session():
    """Corruption error on attempt 0 → clear session → retry attempt 1 (success)."""
    group = _make_group()
    state._sessions = {"g": "old-session"}

    corruption_err = 'messages with role "tool" must be a response to a preceeding message with "tool_calls"'
    outputs = [_mock_output("error", corruption_err), _mock_output("success")]
    call_idx = 0

    async def _mock_run(*args, **kwargs):
        nonlocal call_idx
        out = outputs[call_idx]
        call_idx += 1
        return out

    with (
        patch("omiga.agent.get_all_tasks", AsyncMock(return_value=[])),
        patch("omiga.agent.get_all_chats", AsyncMock(return_value=[])),
        patch("omiga.agent.write_tasks_snapshot"),
        patch("omiga.agent.write_groups_snapshot"),
        patch("omiga.agent.run_container_agent", side_effect=_mock_run),
        patch("omiga.agent.set_session", AsyncMock()),
    ):
        result = await run_agent(group, "hello", "jid@g.us")

    assert call_idx == 2  # retried once
    assert result == "success"
    # Session cleared after corruption detection
    assert "g" not in state._sessions


async def test_run_agent_exception_returns_error():
    group = _make_group()
    state._sessions = {}

    with (
        patch("omiga.agent.get_all_tasks", AsyncMock(return_value=[])),
        patch("omiga.agent.get_all_chats", AsyncMock(return_value=[])),
        patch("omiga.agent.write_tasks_snapshot"),
        patch("omiga.agent.write_groups_snapshot"),
        patch("omiga.agent.run_container_agent", AsyncMock(side_effect=RuntimeError("crash"))),
    ):
        result = await run_agent(group, "hello", "jid@g.us")

    assert result == "error"


async def test_run_agent_on_output_callback_called():
    group = _make_group()
    state._sessions = {}
    received = []

    async def _on_output(out):
        received.append(out)

    mock_out = _mock_output("success")

    async def _mock_run(grp, inp, register_fn, on_output_fn):
        if on_output_fn:
            await on_output_fn(mock_out)
        return mock_out

    with (
        patch("omiga.agent.get_all_tasks", AsyncMock(return_value=[])),
        patch("omiga.agent.get_all_chats", AsyncMock(return_value=[])),
        patch("omiga.agent.write_tasks_snapshot"),
        patch("omiga.agent.write_groups_snapshot"),
        patch("omiga.agent.run_container_agent", side_effect=_mock_run),
    ):
        await run_agent(group, "hello", "jid@g.us", on_output=_on_output)

    assert len(received) == 1
