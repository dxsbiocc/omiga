"""
GroupQueue — asyncio-based concurrency control for Omiga Python port.

Mirrors src/group-queue.ts:
  - Global semaphore: MAX_CONCURRENT_CONTAINERS slots
  - Per-group asyncio.Lock: at most one container per group
  - Tasks are prioritised over messages within a group
  - Exponential backoff retry on processGroupMessages failure
"""
from __future__ import annotations

import asyncio
import logging
import os
import time
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Optional

from omiga.config import DATA_DIR, MAX_CONCURRENT_CONTAINERS

logger = logging.getLogger(__name__)

MAX_RETRIES = 5
BASE_RETRY_S = 5.0  # seconds (TS uses 5000ms)


@dataclass
class _QueuedTask:
    id: str
    group_jid: str
    fn: Callable[[], Any]


@dataclass
class _GroupState:
    active: bool = False
    idle_waiting: bool = False
    is_task_container: bool = False
    pending_messages: bool = False
    pending_tasks: deque[_QueuedTask] = field(default_factory=deque)
    process: Optional[asyncio.subprocess.Process] = None
    container_name: Optional[str] = None
    group_folder: Optional[str] = None
    retry_count: int = 0


class GroupQueue:
    """Asyncio-native port of the TypeScript GroupQueue."""

    def __init__(self) -> None:
        self._groups: dict[str, _GroupState] = {}
        self._active_count: int = 0
        self._waiting_groups: deque[str] = deque()
        self._process_messages_fn: Optional[Callable[[str], "asyncio.coroutine[bool]"]] = None
        self._shutting_down: bool = False

    def _get_group(self, jid: str) -> _GroupState:
        if jid not in self._groups:
            self._groups[jid] = _GroupState()
        return self._groups[jid]

    def set_process_messages_fn(
        self, fn: Callable[[str], Any]
    ) -> None:
        self._process_messages_fn = fn

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def enqueue_message_check(self, jid: str) -> None:
        if self._shutting_down:
            return
        state = self._get_group(jid)

        if state.active:
            state.pending_messages = True
            logger.debug("Container active, message queued: jid=%s", jid)
            return

        if self._active_count >= MAX_CONCURRENT_CONTAINERS:
            state.pending_messages = True
            if jid not in self._waiting_groups:
                self._waiting_groups.append(jid)
            logger.debug(
                "At concurrency limit, message queued: jid=%s active=%d",
                jid,
                self._active_count,
            )
            return

        asyncio.ensure_future(self._run_for_group(jid, "messages"))

    def enqueue_task(
        self,
        jid: str,
        task_id: str,
        fn: Callable[[], Any],
    ) -> None:
        if self._shutting_down:
            return
        state = self._get_group(jid)

        if any(t.id == task_id for t in state.pending_tasks):
            logger.debug("Task already queued, skipping: jid=%s task=%s", jid, task_id)
            return

        if state.active:
            state.pending_tasks.append(_QueuedTask(id=task_id, group_jid=jid, fn=fn))
            if state.idle_waiting:
                self.close_stdin(jid)
            logger.debug("Container active, task queued: jid=%s task=%s", jid, task_id)
            return

        if self._active_count >= MAX_CONCURRENT_CONTAINERS:
            state.pending_tasks.append(_QueuedTask(id=task_id, group_jid=jid, fn=fn))
            if jid not in self._waiting_groups:
                self._waiting_groups.append(jid)
            logger.debug(
                "At concurrency limit, task queued: jid=%s task=%s active=%d",
                jid,
                task_id,
                self._active_count,
            )
            return

        asyncio.ensure_future(
            self._run_task(jid, _QueuedTask(id=task_id, group_jid=jid, fn=fn))
        )

    def register_process(
        self,
        jid: str,
        proc: asyncio.subprocess.Process,
        container_name: str,
        group_folder: Optional[str] = None,
    ) -> None:
        state = self._get_group(jid)
        state.process = proc
        state.container_name = container_name
        if group_folder:
            state.group_folder = group_folder

    def notify_idle(self, jid: str) -> None:
        """Mark the container as idle-waiting; preempt if tasks are pending."""
        state = self._get_group(jid)
        state.idle_waiting = True
        if state.pending_tasks:
            self.close_stdin(jid)

    def send_message(self, jid: str, text: str) -> bool:
        """
        Write a follow-up message to the active container via IPC file.
        Returns True if written, False if no active container.
        """
        state = self._get_group(jid)
        if not state.active or not state.group_folder or state.is_task_container:
            return False
        state.idle_waiting = False

        input_dir = DATA_DIR / "ipc" / state.group_folder / "input"
        try:
            input_dir.mkdir(parents=True, exist_ok=True)
            import random, string
            rand = "".join(random.choices(string.ascii_lowercase + string.digits, k=4))
            filename = f"{int(time.time() * 1000)}-{rand}.json"
            filepath = input_dir / filename
            temp_path = input_dir / f"{filename}.tmp"
            temp_path.write_text(
                '{"type":"message","text":' + __import__("json").dumps(text) + "}"
            )
            temp_path.rename(filepath)
            return True
        except Exception:
            return False

    def close_stdin(self, jid: str) -> None:
        """Signal the active container to wind down by writing a close sentinel."""
        state = self._get_group(jid)
        if not state.active or not state.group_folder:
            return
        input_dir = DATA_DIR / "ipc" / state.group_folder / "input"
        try:
            input_dir.mkdir(parents=True, exist_ok=True)
            (input_dir / "_close").write_text("")
        except Exception:
            pass

    async def shutdown(self, grace_period_ms: int = 10000) -> None:
        """Mark as shutting down. Containers are detached (not killed)."""
        self._shutting_down = True
        active_containers = [
            state.container_name
            for state in self._groups.values()
            if state.process and state.container_name
        ]
        logger.info(
            "GroupQueue shutting down: active=%d detached=%s",
            self._active_count,
            active_containers,
        )

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _run_for_group(self, jid: str, reason: str) -> None:
        state = self._get_group(jid)
        state.active = True
        state.idle_waiting = False
        state.is_task_container = False
        state.pending_messages = False
        self._active_count += 1

        logger.debug(
            "Starting container for group: jid=%s reason=%s active=%d",
            jid,
            reason,
            self._active_count,
        )

        try:
            if self._process_messages_fn:
                success = await self._process_messages_fn(jid)
                if success:
                    state.retry_count = 0
                else:
                    self._schedule_retry(jid, state)
        except Exception as err:
            logger.error("Error processing messages for group %s: %s", jid, err)
            self._schedule_retry(jid, state)
        finally:
            state.active = False
            state.process = None
            state.container_name = None
            state.group_folder = None
            self._active_count -= 1
            self._drain_group(jid)

    async def _run_task(self, jid: str, task: _QueuedTask) -> None:
        state = self._get_group(jid)
        state.active = True
        state.idle_waiting = False
        state.is_task_container = True
        self._active_count += 1

        logger.debug(
            "Running queued task: jid=%s task=%s active=%d",
            jid,
            task.id,
            self._active_count,
        )

        try:
            coro = task.fn()
            if asyncio.iscoroutine(coro):
                await coro
        except Exception as err:
            logger.error("Error running task %s for group %s: %s", task.id, jid, err)
        finally:
            state.active = False
            state.is_task_container = False
            state.process = None
            state.container_name = None
            state.group_folder = None
            self._active_count -= 1
            self._drain_group(jid)

    def _schedule_retry(self, jid: str, state: _GroupState) -> None:
        state.retry_count += 1
        if state.retry_count > MAX_RETRIES:
            logger.error(
                "Max retries exceeded for %s — dropping (will retry on next message)", jid
            )
            state.retry_count = 0
            return
        delay_s = BASE_RETRY_S * (2 ** (state.retry_count - 1))
        logger.info(
            "Scheduling retry: jid=%s attempt=%d delay=%.1fs",
            jid,
            state.retry_count,
            delay_s,
        )

        async def _delayed():
            await asyncio.sleep(delay_s)
            if not self._shutting_down:
                self.enqueue_message_check(jid)

        asyncio.ensure_future(_delayed())

    def _drain_group(self, jid: str) -> None:
        if self._shutting_down:
            return
        state = self._get_group(jid)

        if state.pending_tasks:
            task = state.pending_tasks.popleft()
            asyncio.ensure_future(self._run_task(jid, task))
            return

        if state.pending_messages:
            asyncio.ensure_future(self._run_for_group(jid, "drain"))
            return

        self._drain_waiting()

    def _drain_waiting(self) -> None:
        while self._waiting_groups and self._active_count < MAX_CONCURRENT_CONTAINERS:
            next_jid = self._waiting_groups.popleft()
            state = self._get_group(next_jid)

            if state.pending_tasks:
                task = state.pending_tasks.popleft()
                asyncio.ensure_future(self._run_task(next_jid, task))
            elif state.pending_messages:
                asyncio.ensure_future(self._run_for_group(next_jid, "drain"))
