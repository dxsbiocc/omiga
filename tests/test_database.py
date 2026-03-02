"""
Tests for nanoclaw/database.py

Uses aiosqlite in-memory database so no files are created on disk.
"""
from __future__ import annotations

import asyncio
import os
import tempfile
from datetime import datetime, timezone
from pathlib import Path

import pytest

import omiga.database as db_mod
from omiga.database import (
    close_database,
    create_task,
    delete_task,
    get_all_chats,
    get_all_registered_groups,
    get_all_sessions,
    get_all_tasks,
    get_due_tasks,
    get_messages_since,
    get_new_messages,
    get_registered_group,
    get_router_state,
    get_session,
    get_task_by_id,
    get_tasks_for_group,
    init_database,
    log_task_run,
    set_registered_group,
    set_router_state,
    set_session,
    store_chat_metadata,
    store_message,
    update_task,
    update_task_after_run,
)
from omiga.models import NewMessage, RegisteredGroup, ScheduledTask, TaskRunLog


@pytest.fixture(autouse=True)
async def fresh_db(tmp_path):
    """Point the module at a fresh tmp database for each test."""
    await db_mod.close_database()          # close any pool connection from a prior test
    db_path = tmp_path / "test.db"
    db_mod._DB_PATH = db_path
    await init_database()
    yield
    await db_mod.close_database()
    db_mod._DB_PATH = None


# ---------------------------------------------------------------------------
# Router state
# ---------------------------------------------------------------------------

async def test_router_state_roundtrip():
    await set_router_state("last_timestamp", "2024-01-01T00:00:00Z")
    val = await get_router_state("last_timestamp")
    assert val == "2024-01-01T00:00:00Z"


async def test_router_state_missing():
    val = await get_router_state("nonexistent_key")
    assert val is None


# ---------------------------------------------------------------------------
# Sessions
# ---------------------------------------------------------------------------

async def test_session_roundtrip():
    await set_session("main", "sess-abc123")
    val = await get_session("main")
    assert val == "sess-abc123"


async def test_get_all_sessions():
    await set_session("main", "sess-1")
    await set_session("work", "sess-2")
    sessions = await get_all_sessions()
    assert sessions == {"main": "sess-1", "work": "sess-2"}


# ---------------------------------------------------------------------------
# Registered groups
# ---------------------------------------------------------------------------

async def test_registered_group_roundtrip():
    group = RegisteredGroup(
        name="Test Group",
        folder="testgroup",
        trigger="@Andy",
        added_at="2024-01-01T00:00:00Z",
    )
    await set_registered_group("jid-001@g.us", group)
    result = await get_registered_group("jid-001@g.us")
    assert result is not None
    assert result.name == "Test Group"
    assert result.folder == "testgroup"


async def test_get_all_registered_groups():
    g1 = RegisteredGroup(name="G1", folder="g1", trigger="@Andy", added_at="2024-01-01T00:00:00Z")
    g2 = RegisteredGroup(name="G2", folder="g2", trigger="@Andy", added_at="2024-01-01T00:00:00Z")
    await set_registered_group("jid1@g.us", g1)
    await set_registered_group("jid2@g.us", g2)
    groups = await get_all_registered_groups()
    assert "jid1@g.us" in groups
    assert "jid2@g.us" in groups


# ---------------------------------------------------------------------------
# Chat metadata & messages
# ---------------------------------------------------------------------------

async def test_store_chat_metadata():
    await store_chat_metadata("jid@g.us", "2024-01-01T00:00:00Z", "My Chat", "whatsapp", True)
    chats = await get_all_chats()
    assert any(c.jid == "jid@g.us" and c.name == "My Chat" for c in chats)


async def test_store_and_retrieve_messages():
    await store_chat_metadata("jid@g.us", "2024-01-01T00:00:00Z")

    msg = NewMessage(
        id="msg-1",
        chat_jid="jid@g.us",
        sender="user1",
        sender_name="Alice",
        content="Hello",
        timestamp="2024-01-01T00:00:01Z",
    )
    await store_message(msg)

    found, _ = await get_new_messages(["jid@g.us"], "2024-01-01T00:00:00Z", "Andy")
    assert len(found) == 1
    assert found[0].content == "Hello"


async def test_get_messages_since_filters_bot():
    await store_chat_metadata("jid@g.us", "2024-01-01T00:00:00Z")

    user_msg = NewMessage(
        id="m1", chat_jid="jid@g.us", sender="u1", sender_name="Alice",
        content="Hi", timestamp="2024-01-01T00:00:01Z",
    )
    bot_msg = NewMessage(
        id="m2", chat_jid="jid@g.us", sender="bot", sender_name="Andy",
        content="Andy: response", timestamp="2024-01-01T00:00:02Z",
        is_bot_message=True,
    )
    await store_message(user_msg)
    await store_message(bot_msg)

    found = await get_messages_since("jid@g.us", "2024-01-01T00:00:00Z", "Andy")
    assert len(found) == 1
    assert found[0].id == "m1"


# ---------------------------------------------------------------------------
# Scheduled tasks
# ---------------------------------------------------------------------------

def _make_task(task_id: str = "task-1", status: str = "active") -> ScheduledTask:
    return ScheduledTask(
        id=task_id,
        group_folder="main",
        chat_jid="jid@g.us",
        prompt="Do something",
        schedule_type="interval",
        schedule_value="60000",
        context_mode="isolated",
        next_run="2099-01-01T00:00:00Z",
        last_run=None,
        last_result=None,
        status=status,
        created_at="2024-01-01T00:00:00Z",
    )


async def test_create_and_get_task():
    task = _make_task()
    await create_task(task)
    found = await get_task_by_id("task-1")
    assert found is not None
    assert found.prompt == "Do something"


async def test_update_task_status():
    await create_task(_make_task())
    await update_task("task-1", status="paused")
    found = await get_task_by_id("task-1")
    assert found.status == "paused"


async def test_delete_task():
    await create_task(_make_task())
    await delete_task("task-1")
    found = await get_task_by_id("task-1")
    assert found is None


async def test_get_due_tasks():
    past_task = _make_task(task_id="past")
    past_task.next_run = "2000-01-01T00:00:00Z"
    await create_task(past_task)

    future_task = _make_task(task_id="future")
    future_task.next_run = "2099-01-01T00:00:00Z"
    await create_task(future_task)

    due = await get_due_tasks()
    ids = [t.id for t in due]
    assert "past" in ids
    assert "future" not in ids


async def test_update_task_after_run_once_completes():
    task = _make_task()
    task.schedule_type = "once"
    await create_task(task)
    await update_task_after_run("task-1", None, "Done")
    found = await get_task_by_id("task-1")
    assert found.status == "completed"
    assert found.last_result == "Done"


async def test_log_task_run():
    await create_task(_make_task())
    await log_task_run(
        TaskRunLog(
            task_id="task-1",
            run_at="2024-01-01T00:01:00Z",
            duration_ms=500,
            status="success",
            result="ok",
            error=None,
        )
    )
    # No assertion — just ensure no error


async def test_get_tasks_for_group():
    t1 = _make_task("t1"); t1.group_folder = "main"
    t2 = _make_task("t2"); t2.group_folder = "other"
    await create_task(t1)
    await create_task(t2)
    tasks = await get_tasks_for_group("main")
    assert all(t.group_folder == "main" for t in tasks)
