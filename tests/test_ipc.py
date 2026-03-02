"""
Tests for omiga/scheduler/ipc.py — IPC task processing and file watcher.
"""
from __future__ import annotations

import asyncio
import json
from typing import Optional
from unittest.mock import AsyncMock, MagicMock

import pytest

import omiga.database as db_mod
import omiga.scheduler.ipc as ipc_mod
from omiga.database import init_database
from omiga.scheduler.ipc import IpcDeps, _process_single_file, process_task_ipc, stop_ipc_watcher
from omiga.models import AvailableGroup, RegisteredGroup, ScheduledTask


@pytest.fixture(autouse=True)
async def fresh_db(tmp_path):
    db_mod._DB_PATH = tmp_path / "test.db"
    await init_database()
    yield
    db_mod._DB_PATH = None


def _make_deps(registered: dict[str, RegisteredGroup]) -> IpcDeps:
    return IpcDeps(
        send_message=AsyncMock(),
        registered_groups=lambda: registered,
        register_group=MagicMock(),
        sync_group_metadata=AsyncMock(),
        get_available_groups=lambda: [],
        write_groups_snapshot=MagicMock(),
    )


async def test_schedule_task_creates_db_entry():
    group = RegisteredGroup(
        name="Main", folder="main", trigger="@Andy",
        added_at="2024-01-01T00:00:00Z",
    )
    registered = {"jid@g.us": group}
    deps = _make_deps(registered)

    data = {
        "type": "schedule_task",
        "prompt": "Say hello",
        "schedule_type": "interval",
        "schedule_value": "60000",
        "targetJid": "jid@g.us",
    }
    await process_task_ipc(data, "main", True, deps)

    from omiga.database import get_all_tasks
    tasks = await get_all_tasks()
    assert len(tasks) == 1
    assert tasks[0].prompt == "Say hello"


async def test_schedule_task_non_main_blocked_for_other_group():
    """Non-main source cannot schedule for another group."""
    g1 = RegisteredGroup(name="G1", folder="g1", trigger="@Andy", added_at="2024-01-01T00:00:00Z")
    g2 = RegisteredGroup(name="G2", folder="g2", trigger="@Andy", added_at="2024-01-01T00:00:00Z")
    registered = {"jid1@g.us": g1, "jid2@g.us": g2}
    deps = _make_deps(registered)

    data = {
        "type": "schedule_task",
        "prompt": "Hacked",
        "schedule_type": "interval",
        "schedule_value": "60000",
        "targetJid": "jid2@g.us",
    }
    # source_group = g1, but targeting g2 — should be blocked
    await process_task_ipc(data, "g1", False, deps)

    from omiga.database import get_all_tasks
    tasks = await get_all_tasks()
    assert len(tasks) == 0


async def test_pause_resume_cancel_task():
    from omiga.database import create_task
    from omiga.models import ScheduledTask

    task = ScheduledTask(
        id="t1", group_folder="main", chat_jid="jid@g.us",
        prompt="Do", schedule_type="interval", schedule_value="60000",
        context_mode="isolated", next_run="2099-01-01T00:00:00Z",
        last_run=None, last_result=None, status="active",
        created_at="2024-01-01T00:00:00Z",
    )
    await create_task(task)

    deps = _make_deps({})

    # Pause
    await process_task_ipc({"type": "pause_task", "taskId": "t1"}, "main", True, deps)
    found = await db_mod.get_task_by_id("t1")
    assert found.status == "paused"

    # Resume
    await process_task_ipc({"type": "resume_task", "taskId": "t1"}, "main", True, deps)
    found = await db_mod.get_task_by_id("t1")
    assert found.status == "active"

    # Cancel
    await process_task_ipc({"type": "cancel_task", "taskId": "t1"}, "main", True, deps)
    found = await db_mod.get_task_by_id("t1")
    assert found is None


async def test_register_group_non_main_blocked():
    deps = _make_deps({})
    data = {
        "type": "register_group",
        "jid": "jid@g.us",
        "name": "Evil",
        "folder": "evil",
        "trigger": "@Andy",
    }
    await process_task_ipc(data, "notmain", False, deps)
    deps.register_group.assert_not_called()


async def test_register_group_main_succeeds():
    deps = _make_deps({})
    data = {
        "type": "register_group",
        "jid": "jid@g.us",
        "name": "Work",
        "folder": "work",
        "trigger": "@Andy",
    }
    await process_task_ipc(data, "main", True, deps)
    deps.register_group.assert_called_once()
    call_args = deps.register_group.call_args
    assert call_args[0][0] == "jid@g.us"


async def test_register_group_invalid_folder_blocked():
    deps = _make_deps({})
    data = {
        "type": "register_group",
        "jid": "jid@g.us",
        "name": "Evil",
        "folder": "../evil",  # path traversal
        "trigger": "@Andy",
    }
    await process_task_ipc(data, "main", True, deps)
    deps.register_group.assert_not_called()


async def test_invalid_cron_expression():
    group = RegisteredGroup(
        name="Main", folder="main", trigger="@Andy",
        added_at="2024-01-01T00:00:00Z",
    )
    registered = {"jid@g.us": group}
    deps = _make_deps(registered)

    data = {
        "type": "schedule_task",
        "prompt": "Bad cron",
        "schedule_type": "cron",
        "schedule_value": "not-a-cron",
        "targetJid": "jid@g.us",
    }
    await process_task_ipc(data, "main", True, deps)

    from omiga.database import get_all_tasks
    tasks = await get_all_tasks()
    assert len(tasks) == 0


# ---------------------------------------------------------------------------
# _process_single_file — new watchfiles-based handler
# ---------------------------------------------------------------------------


async def test_process_single_file_sends_authorized_message(tmp_path):
    """A valid message IPC file from the main group is sent and then deleted."""
    group = RegisteredGroup(
        name="G", folder="main", trigger="@Andy", added_at="2024-01-01T00:00:00Z",
    )
    registered = {"jid@g.us": group}
    deps = _make_deps(registered)

    messages_dir = tmp_path / "main" / "messages"
    messages_dir.mkdir(parents=True)
    fp = messages_dir / "001.json"
    fp.write_text(json.dumps({"type": "message", "chatJid": "jid@g.us", "text": "hello"}))

    await _process_single_file(fp, "main", "messages", deps)

    deps.send_message.assert_called_once()
    call_args = deps.send_message.call_args[0]
    assert call_args[0] == "jid@g.us"
    assert call_args[1] == "hello"
    assert not fp.exists()  # deleted after processing


async def test_process_single_file_blocks_unauthorized_message(tmp_path):
    """Message from non-main source targeting another group's JID is blocked."""
    g1 = RegisteredGroup(name="G1", folder="g1", trigger="@Andy", added_at="2024-01-01T00:00:00Z")
    g2 = RegisteredGroup(name="G2", folder="g2", trigger="@Andy", added_at="2024-01-01T00:00:00Z")
    registered = {"jid1@g.us": g1, "jid2@g.us": g2}
    deps = _make_deps(registered)

    messages_dir = tmp_path / "g1" / "messages"
    messages_dir.mkdir(parents=True)
    fp = messages_dir / "001.json"
    fp.write_text(json.dumps({"type": "message", "chatJid": "jid2@g.us", "text": "hack"}))

    await _process_single_file(fp, "g1", "messages", deps)

    deps.send_message.assert_not_called()
    assert not fp.exists()  # still deleted (consumed but blocked)


async def test_process_single_file_handles_task_file(tmp_path):
    """A tasks/ file is routed to process_task_ipc correctly."""
    group = RegisteredGroup(
        name="Main", folder="main", trigger="@Andy", added_at="2024-01-01T00:00:00Z",
    )
    registered = {"jid@g.us": group}
    deps = _make_deps(registered)

    tasks_dir = tmp_path / "main" / "tasks"
    tasks_dir.mkdir(parents=True)
    fp = tasks_dir / "001.json"
    fp.write_text(json.dumps({
        "type": "schedule_task",
        "prompt": "Run backup",
        "schedule_type": "interval",
        "schedule_value": "3600000",
        "targetJid": "jid@g.us",
    }))

    await _process_single_file(fp, "main", "tasks", deps)

    from omiga.database import get_all_tasks
    tasks = await get_all_tasks()
    assert len(tasks) == 1
    assert tasks[0].prompt == "Run backup"
    assert not fp.exists()


async def test_process_single_file_bad_json_moves_to_errors(tmp_path):
    """A file with invalid JSON is moved to the errors/ directory."""
    deps = _make_deps({})

    # _process_single_file resolves error_dir as DATA_DIR / "ipc" / "errors"
    ipc_base = tmp_path / "ipc"
    messages_dir = ipc_base / "main" / "messages"
    messages_dir.mkdir(parents=True)
    fp = messages_dir / "bad.json"
    fp.write_text("not-valid-json{{")

    from unittest.mock import patch
    with patch("omiga.scheduler.ipc.DATA_DIR", tmp_path):
        await _process_single_file(fp, "main", "messages", deps)

    assert not fp.exists()
    error_dir = ipc_base / "errors"
    moved = list(error_dir.glob("*.json")) if error_dir.exists() else []
    assert len(moved) == 1


# ---------------------------------------------------------------------------
# stop_ipc_watcher
# ---------------------------------------------------------------------------


def test_stop_ipc_watcher_sets_shutdown_event():
    import asyncio

    async def _run():
        ipc_mod._shutdown_event = asyncio.Event()
        ipc_mod._ipc_watcher_running = True
        stop_ipc_watcher()
        assert ipc_mod._shutdown_event.is_set()
        assert ipc_mod._ipc_watcher_running is False

    asyncio.run(_run())


def test_stop_ipc_watcher_safe_when_not_started():
    """stop_ipc_watcher should not raise when called before start."""
    ipc_mod._shutdown_event = None
    ipc_mod._ipc_watcher_running = False
    stop_ipc_watcher()  # should not raise
