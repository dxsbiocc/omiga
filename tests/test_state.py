"""Tests for omiga/state.py — global state CRUD."""
from __future__ import annotations

import json
from unittest.mock import patch

import pytest

import omiga.database as db_mod
import omiga.state as state
from omiga.database import (
    get_registered_group,
    get_router_state,
    init_database,
    set_registered_group,
    set_router_state,
    set_session,
)
from omiga.models import ChatInfo, RegisteredGroup


@pytest.fixture(autouse=True)
async def reset(tmp_path):
    """Isolate DB and reset all state globals between tests."""
    await db_mod.close_database()
    db_mod._DB_PATH = tmp_path / "test.db"
    await init_database()

    state._last_timestamp = ""
    state._sessions = {}
    state._registered_groups = {}
    state._last_agent_timestamp = {}
    state._message_loop_running = False
    state._consecutive_errors = {}
    state._all_chats_cache = []
    state._channels = []
    state._debounce_deadlines = {}
    state._shutdown_event = None

    yield

    await db_mod.close_database()
    db_mod._DB_PATH = None


# ---------------------------------------------------------------------------
# load_state
# ---------------------------------------------------------------------------


async def test_load_state_empty_db():
    await state.load_state()
    assert state._last_timestamp == ""
    assert state._sessions == {}
    assert state._registered_groups == {}
    assert state._last_agent_timestamp == {}


async def test_load_state_reads_persisted_values():
    await set_router_state("last_timestamp", "ts-100")
    await set_router_state("last_agent_timestamp", json.dumps({"jid1": "ts-50"}))
    await set_session("main", "sess-abc")

    await state.load_state()

    assert state._last_timestamp == "ts-100"
    assert state._last_agent_timestamp == {"jid1": "ts-50"}
    assert state._sessions == {"main": "sess-abc"}


async def test_load_state_handles_corrupted_agent_ts(caplog):
    await set_router_state("last_agent_timestamp", "not-valid-json{")

    await state.load_state()

    assert state._last_agent_timestamp == {}
    assert "Corrupted" in caplog.text


# ---------------------------------------------------------------------------
# save_state
# ---------------------------------------------------------------------------


async def test_save_state_persists_to_db():
    state._last_timestamp = "ts-999"
    state._last_agent_timestamp = {"jid2": "ts-888"}

    await state.save_state()

    assert await get_router_state("last_timestamp") == "ts-999"
    raw = await get_router_state("last_agent_timestamp")
    assert json.loads(raw) == {"jid2": "ts-888"}


async def test_save_and_reload_roundtrip():
    state._last_timestamp = "ts-xyz"
    state._last_agent_timestamp = {"jid3": "ts-zzz"}
    await state.save_state()

    state._last_timestamp = ""
    state._last_agent_timestamp = {}
    await state.load_state()

    assert state._last_timestamp == "ts-xyz"
    assert state._last_agent_timestamp == {"jid3": "ts-zzz"}


# ---------------------------------------------------------------------------
# register_group / unregister_group
# ---------------------------------------------------------------------------


async def test_register_group_stores_in_memory_and_db(tmp_path):
    group = RegisteredGroup(
        name="TestGroup", folder="testgroup", trigger="@bot",
        added_at="2024-01-01T00:00:00Z",
    )
    with patch("omiga.state.resolve_group_folder_path", return_value=tmp_path / "testgroup"):
        (tmp_path / "testgroup").mkdir()
        await state.register_group("jid@g.us", group)

    assert "jid@g.us" in state._registered_groups
    assert state._registered_groups["jid@g.us"].name == "TestGroup"

    stored = await get_registered_group("jid@g.us")
    assert stored is not None
    assert stored.folder == "testgroup"


async def test_register_group_creates_logs_dir(tmp_path):
    group = RegisteredGroup(
        name="G", folder="g", trigger="@bot", added_at="2024-01-01T00:00:00Z",
    )
    group_dir = tmp_path / "g"
    group_dir.mkdir()
    with patch("omiga.state.resolve_group_folder_path", return_value=group_dir):
        await state.register_group("jid@g.us", group)

    assert (group_dir / "logs").is_dir()


async def test_register_group_rejects_invalid_folder():
    group = RegisteredGroup(
        name="Bad", folder="../evil", trigger="@bot", added_at="2024-01-01T00:00:00Z",
    )
    with patch("omiga.state.resolve_group_folder_path", side_effect=ValueError("invalid")):
        await state.register_group("jid@g.us", group)

    assert "jid@g.us" not in state._registered_groups


async def test_unregister_group_removes_from_memory_and_db(tmp_path):
    group = RegisteredGroup(
        name="G", folder="g", trigger="@bot", added_at="2024-01-01T00:00:00Z",
    )
    state._registered_groups["jid@g.us"] = group
    await set_registered_group("jid@g.us", group)

    await state.unregister_group("jid@g.us")

    assert "jid@g.us" not in state._registered_groups
    assert await get_registered_group("jid@g.us") is None


async def test_unregister_group_noop_for_unknown_jid():
    # Should not raise
    await state.unregister_group("unknown@g.us")


# ---------------------------------------------------------------------------
# get_available_groups
# ---------------------------------------------------------------------------


async def test_get_available_groups_filters_non_groups():
    state._all_chats_cache = [
        ChatInfo(jid="grp@g.us", name="Group1", is_group=True, last_message_time="ts1", channel="stub"),
        ChatInfo(jid="user@s.us", name="User", is_group=False, last_message_time="ts2", channel="stub"),
        ChatInfo(jid="__group_sync__", name="Sync", is_group=True, last_message_time="ts3", channel="stub"),
    ]
    result = state.get_available_groups()
    jids = {g.jid for g in result}
    assert "grp@g.us" in jids
    assert "user@s.us" not in jids
    assert "__group_sync__" not in jids


async def test_get_available_groups_marks_registered():
    grp = RegisteredGroup(name="G", folder="g", trigger="@bot", added_at="2024-01-01T00:00:00Z")
    state._registered_groups = {"jid1@g.us": grp}
    state._all_chats_cache = [
        ChatInfo(jid="jid1@g.us", name="G1", is_group=True, last_message_time="ts1", channel="stub"),
        ChatInfo(jid="jid2@g.us", name="G2", is_group=True, last_message_time="ts2", channel="stub"),
    ]
    result = state.get_available_groups()
    by_jid = {g.jid: g for g in result}
    assert by_jid["jid1@g.us"].is_registered is True
    assert by_jid["jid2@g.us"].is_registered is False


async def test_get_available_groups_empty_cache():
    state._all_chats_cache = []
    assert state.get_available_groups() == []
