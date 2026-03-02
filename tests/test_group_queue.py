"""
Tests for nanoclaw/group_queue.py
"""
from __future__ import annotations

import asyncio

import pytest

from omiga.group_queue import GroupQueue


async def test_enqueue_message_check_runs_fn():
    queue = GroupQueue()
    called = []

    async def _process(jid: str) -> bool:
        called.append(jid)
        return True

    queue.set_process_messages_fn(_process)
    queue.enqueue_message_check("jid1")
    await asyncio.sleep(0.05)  # let event loop flush
    assert "jid1" in called


async def test_enqueue_task_runs_fn():
    queue = GroupQueue()
    ran = []

    async def _task():
        ran.append("task-1")

    queue.enqueue_task("jid1", "task-1", _task)
    await asyncio.sleep(0.05)
    assert "task-1" in ran


async def test_active_container_queues_message():
    """While a container is active, a second message check should be queued."""
    queue = GroupQueue()
    barrier = asyncio.Event()
    started = asyncio.Event()

    async def _slow_process(jid: str) -> bool:
        started.set()
        await barrier.wait()
        return True

    queue.set_process_messages_fn(_slow_process)
    queue.enqueue_message_check("jid1")
    await started.wait()

    # Second enqueue while active — should set pendingMessages
    queue.enqueue_message_check("jid1")
    state = queue._get_group("jid1")
    assert state.pending_messages

    barrier.set()
    await asyncio.sleep(0.05)


async def test_concurrent_limit():
    """Active count is incremented and decremented correctly."""
    queue = GroupQueue()
    barrier = asyncio.Event()
    max_seen = []

    async def _process(jid: str) -> bool:
        max_seen.append(queue._active_count)
        await barrier.wait()
        return True

    queue.set_process_messages_fn(_process)
    queue.enqueue_message_check("jid1")
    await asyncio.sleep(0.05)  # let first task start

    # active_count should be 1 now
    assert queue._active_count == 1

    barrier.set()
    await asyncio.sleep(0.05)

    # after completion, active count returns to 0
    assert queue._active_count == 0


async def test_notify_idle_preempts_task():
    """notifyIdle should close stdin if tasks are pending."""
    queue = GroupQueue()
    closed = []
    state = queue._get_group("jid1")
    state.active = True
    state.group_folder = "main"

    import omiga.group_queue as gq_mod
    original_close = queue.close_stdin
    queue.close_stdin = lambda jid: closed.append(jid)  # type: ignore

    # Add a pending task
    async def _dummy():
        pass
    from omiga.group_queue import _QueuedTask
    state.pending_tasks.append(_QueuedTask(id="t1", group_jid="jid1", fn=_dummy))

    queue.notify_idle("jid1")
    assert "jid1" in closed

    queue.close_stdin = original_close  # restore


async def test_send_message_no_active_container():
    queue = GroupQueue()
    result = queue.send_message("jid1", "hello")
    assert result is False


async def test_shutdown_sets_flag():
    queue = GroupQueue()
    await queue.shutdown(1000)
    assert queue._shutting_down is True
