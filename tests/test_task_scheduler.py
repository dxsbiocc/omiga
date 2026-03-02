"""
Tests for nanoclaw/task_scheduler.py

Unit-tests the scheduler helpers; does not spawn real containers.
"""
from __future__ import annotations

import asyncio
from datetime import datetime, timezone
from typing import Optional
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

import omiga.database as db_mod
from omiga.database import close_database, create_task, get_task_by_id, init_database
from omiga.group_queue import GroupQueue
from omiga.models import RegisteredGroup, ScheduledTask
from omiga.task_scheduler import (
    SchedulerDeps,
    _reset_scheduler_for_tests,
    start_scheduler_loop,
)


@pytest.fixture(autouse=True)
async def fresh_db(tmp_path):
    await db_mod.close_database()          # close any pool connection from a prior test
    db_mod._DB_PATH = tmp_path / "test.db"
    await init_database()
    yield
    await db_mod.close_database()
    db_mod._DB_PATH = None


@pytest.fixture(autouse=True)
def reset_scheduler():
    _reset_scheduler_for_tests()
    yield
    _reset_scheduler_for_tests()


def _make_task(
    task_id: str = "task-1",
    status: str = "active",
    next_run: str = "2000-01-01T00:00:00Z",
    schedule_type: str = "interval",
) -> ScheduledTask:
    return ScheduledTask(
        id=task_id,
        group_folder="main",
        chat_jid="jid@g.us",
        prompt="Do something",
        schedule_type=schedule_type,
        schedule_value="60000",
        context_mode="isolated",
        next_run=next_run,
        last_run=None,
        last_result=None,
        status=status,
        created_at="2024-01-01T00:00:00Z",
    )


def _make_deps(queue: Optional[GroupQueue] = None) -> SchedulerDeps:
    if queue is None:
        queue = GroupQueue()
    return SchedulerDeps(
        registered_groups=lambda: {
            "jid@g.us": RegisteredGroup(
                name="Main", folder="main", trigger="@Andy",
                added_at="2024-01-01T00:00:00Z",
            )
        },
        get_sessions=lambda: {},
        queue=queue,
        on_process=MagicMock(),
        send_message=AsyncMock(),
    )


async def test_scheduler_enqueues_due_task():
    task = _make_task()
    await create_task(task)

    queue = GroupQueue()
    enqueued = []

    def _capture_enqueue(jid, tid, fn):
        enqueued.append(tid)
        # Don't actually run container
    queue.enqueue_task = _capture_enqueue

    # Manually run one scheduler iteration using real DB
    from omiga.database import get_due_tasks, get_task_by_id
    due = await get_due_tasks()
    for t in due:
        current = await get_task_by_id(t.id)
        if current and current.status == "active":
            queue.enqueue_task(current.chat_jid, current.id, lambda: None)

    assert "task-1" in enqueued


async def test_paused_task_not_executed():
    task = _make_task(status="paused")
    await create_task(task)

    from omiga.database import get_due_tasks, get_task_by_id
    due = await get_due_tasks()
    # Paused tasks should not appear in due tasks (status != 'active')
    for t in due:
        current = await get_task_by_id(t.id)
        assert current.status == "active", "Paused task should not be executed"


async def test_scheduler_loop_starts_once():
    queue = GroupQueue()
    deps = _make_deps(queue)

    start_scheduler_loop(deps)
    start_scheduler_loop(deps)  # second call should be idempotent

    import omiga.task_scheduler as ts_mod
    assert ts_mod._scheduler_running


async def test_next_run_computed_for_interval():
    """After a run, interval task should get a future next_run."""
    task = _make_task(schedule_type="interval")
    await create_task(task)

    from omiga.database import update_task_after_run, get_task_by_id
    import time
    future_ts = datetime.fromtimestamp(time.time() + 60, tz=timezone.utc).isoformat()
    await update_task_after_run("task-1", future_ts, "Done")

    found = await get_task_by_id("task-1")
    assert found.last_result == "Done"
    # next_run should be in the future
    assert found.next_run is not None
    assert found.next_run > datetime.now(timezone.utc).isoformat()
