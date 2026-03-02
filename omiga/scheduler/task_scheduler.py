"""
Task scheduler for Omiga Python port.

Polls the database every SCHEDULER_POLL_INTERVAL seconds for due tasks and
enqueues them in GroupQueue.

Mirrors src/task-scheduler.ts.
"""
from __future__ import annotations

import asyncio
import logging
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Optional

from omiga.config import ASSISTANT_NAME, MAIN_GROUP_FOLDER, SCHEDULER_POLL_INTERVAL, TIMEZONE
from omiga.container.runner import run_container_agent, write_tasks_snapshot
from omiga.database import (
    get_all_tasks,
    get_due_tasks,
    get_task_by_id,
    log_task_run,
    update_task,
    update_task_after_run,
)
from omiga.group_folder import resolve_group_folder_path
from omiga.group_queue import GroupQueue
from omiga.models import ContainerInput, ContainerOutput, RegisteredGroup, ScheduledTask, TaskRunLog

logger = logging.getLogger(__name__)

_scheduler_running = False

TASK_CLOSE_DELAY_S = 10.0  # 10s — give container time to finish MCP calls


class SchedulerDeps:
    """Dependency bundle for the scheduler loop."""

    def __init__(
        self,
        *,
        registered_groups: Callable[[], dict[str, RegisteredGroup]],
        get_sessions: Callable[[], dict[str, str]],
        queue: GroupQueue,
        on_process: Callable[
            [str, asyncio.subprocess.Process, str, str], None
        ],
        send_message: Callable[[str, str], "asyncio.coroutine | asyncio.Future"],
    ) -> None:
        self.registered_groups = registered_groups
        self.get_sessions = get_sessions
        self.queue = queue
        self.on_process = on_process
        self.send_message = send_message


async def _run_task(task: ScheduledTask, deps: SchedulerDeps) -> None:
    start_time = time.monotonic()
    start_iso = datetime.now(timezone.utc).isoformat()

    # Validate group folder
    try:
        group_dir = resolve_group_folder_path(task.group_folder)
    except Exception as err:
        error = str(err)
        await update_task(task.id, status="paused")
        logger.error(
            "Task has invalid group folder: id=%s folder=%s error=%s",
            task.id,
            task.group_folder,
            error,
        )
        await log_task_run(
            TaskRunLog(
                task_id=task.id,
                run_at=start_iso,
                duration_ms=int((time.monotonic() - start_time) * 1000),
                status="error",
                result=None,
                error=error,
            )
        )
        return

    group_dir.mkdir(parents=True, exist_ok=True)

    logger.info("Running scheduled task: id=%s group=%s", task.id, task.group_folder)

    groups = deps.registered_groups()
    group = next(
        (g for g in groups.values() if g.folder == task.group_folder),
        None,
    )
    if not group:
        error = (
            f"Group '{task.group_folder}' is not registered. "
            f"Register the group first, then resume with: /task resume {task.id}"
        )
        logger.error("Group not found for task: id=%s folder=%s — task paused", task.id, task.group_folder)
        # Pause the task so it stops re-firing on every poll cycle.
        # Mirrors the "invalid group folder" handling above.
        # Resume with /task resume <id> after registering the group.
        await update_task(task.id, status="paused")
        await log_task_run(
            TaskRunLog(
                task_id=task.id,
                run_at=start_iso,
                duration_ms=int((time.monotonic() - start_time) * 1000),
                status="error",
                result=None,
                error=error,
            )
        )
        return

    is_main = task.group_folder == MAIN_GROUP_FOLDER

    # Write tasks snapshot for the container
    all_tasks = await get_all_tasks()
    write_tasks_snapshot(
        task.group_folder,
        is_main,
        [
            {
                "id": t.id,
                "groupFolder": t.group_folder,
                "prompt": t.prompt,
                "schedule_type": t.schedule_type,
                "schedule_value": t.schedule_value,
                "status": t.status,
                "next_run": t.next_run,
            }
            for t in all_tasks
        ],
    )

    sessions = deps.get_sessions()
    session_id = sessions.get(task.group_folder) if task.context_mode == "group" else None

    result: Optional[str] = None
    error_str: Optional[str] = None

    close_timer_handle: Optional[asyncio.TimerHandle] = None
    loop = asyncio.get_event_loop()

    def schedule_close() -> None:
        nonlocal close_timer_handle
        if close_timer_handle:
            return
        def _close():
            logger.debug("Closing task container after result: id=%s", task.id)
            deps.queue.close_stdin(task.chat_jid)
        close_timer_handle = loop.call_later(TASK_CLOSE_DELAY_S, _close)

    async def on_output(streamed: ContainerOutput) -> None:
        nonlocal result
        if streamed.result:
            result = streamed.result
            coro = deps.send_message(task.chat_jid, streamed.result)
            if asyncio.iscoroutine(coro):
                await coro
            schedule_close()
        if streamed.status == "success":
            deps.queue.notify_idle(task.chat_jid)
        if streamed.status == "error":
            pass  # error_str will be set from final output

    try:
        container_input = ContainerInput(
            prompt=task.prompt,
            session_id=session_id,
            group_folder=task.group_folder,
            chat_jid=task.chat_jid,
            is_main=is_main,
            is_scheduled_task=True,
            assistant_name=ASSISTANT_NAME,
        )

        output = await run_container_agent(
            group,
            container_input,
            lambda proc, name: deps.on_process(task.chat_jid, proc, name, task.group_folder),
            on_output,
        )

        if close_timer_handle:
            close_timer_handle.cancel()

        if output.status == "error":
            error_str = output.error or "Unknown error"
        elif output.result:
            result = output.result

        logger.info(
            "Task completed: id=%s duration_ms=%d",
            task.id,
            int((time.monotonic() - start_time) * 1000),
        )

    except Exception as err:
        if close_timer_handle:
            close_timer_handle.cancel()
        error_str = str(err)
        logger.error("Task failed: id=%s error=%s", task.id, error_str)

    duration_ms = int((time.monotonic() - start_time) * 1000)

    await log_task_run(
        TaskRunLog(
            task_id=task.id,
            run_at=start_iso,
            duration_ms=duration_ms,
            status="error" if error_str else "success",
            result=result,
            error=error_str,
        )
    )

    # Compute next_run
    next_run: Optional[str] = None
    if task.schedule_type == "cron":
        try:
            from croniter import croniter
            it = croniter(task.schedule_value, datetime.now(timezone.utc))
            next_run = it.get_next(datetime).isoformat()
        except Exception:
            pass
    elif task.schedule_type == "interval":
        try:
            ms = int(task.schedule_value)
            next_run = datetime.fromtimestamp(
                time.time() + ms / 1000, tz=timezone.utc
            ).isoformat()
        except Exception:
            pass
    # 'once' → next_run = None → status → 'completed'

    result_summary = (
        f"Error: {error_str}"
        if error_str
        else (result[:200] if result else "Completed")
    )
    await update_task_after_run(task.id, next_run, result_summary)


def start_scheduler_loop(deps: SchedulerDeps) -> None:
    """Start the async scheduler loop (idempotent)."""
    global _scheduler_running
    if _scheduler_running:
        logger.debug("Scheduler loop already running")
        return
    _scheduler_running = True
    logger.info("Scheduler loop started")

    async def _loop() -> None:
        while True:
            try:
                due = await get_due_tasks()
                if due:
                    logger.info("Found %d due task(s)", len(due))

                for task in due:
                    # Re-check status in case it was paused/cancelled
                    current = await get_task_by_id(task.id)
                    if not current or current.status != "active":
                        continue

                    # Capture for closure
                    _task = current
                    deps.queue.enqueue_task(
                        _task.chat_jid,
                        _task.id,
                        lambda t=_task: _run_task(t, deps),
                    )
            except Exception as err:
                logger.error("Error in scheduler loop: %s", err)

            await asyncio.sleep(SCHEDULER_POLL_INTERVAL)

    asyncio.ensure_future(_loop())


def _reset_scheduler_for_tests() -> None:
    global _scheduler_running
    _scheduler_running = False
