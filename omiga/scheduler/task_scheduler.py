"""Task scheduler for Omiga using APScheduler.

Uses APScheduler for cron/interval/once scheduling instead of polling.
Mirrors src/task-scheduler.ts functionality with APScheduler backend.
"""
from __future__ import annotations

import asyncio
import logging
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Optional

from apscheduler.schedulers.asyncio import AsyncIOScheduler
from apscheduler.triggers.cron import CronTrigger
from apscheduler.triggers.date import DateTrigger
from apscheduler.triggers.interval import IntervalTrigger

from omiga.config import ASSISTANT_NAME, MAIN_GROUP_FOLDER, TIMEZONE
from omiga.container.runner import write_tasks_snapshot
from omiga.database import (
    get_all_tasks,
    get_task_by_id,
    log_task_run,
    update_task,
    update_task_after_run,
)
from omiga.group_folder import resolve_group_folder_path
from omiga.group_queue import GroupQueue
from omiga.models import ContainerInput, ContainerOutput, RegisteredGroup, ScheduledTask, TaskRunLog

logger = logging.getLogger(__name__)

# Global scheduler instance
_scheduler: Optional[AsyncIOScheduler] = None
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


def _create_trigger(task: ScheduledTask) -> Optional[Any]:
    """Create an APScheduler trigger from a ScheduledTask.

    Args:
        task: The scheduled task

    Returns:
        APScheduler trigger or None if invalid
    """
    if task.schedule_type == "cron":
        try:
            # Parse cron expression: "minute hour day month day_of_week"
            parts = task.schedule_value.split()
            if len(parts) == 5:
                return CronTrigger(
                    minute=parts[0],
                    hour=parts[1],
                    day=parts[2],
                    month=parts[3],
                    day_of_week=parts[4],
                    timezone=TIMEZONE,
                )
        except Exception as e:
            logger.warning("Invalid cron expression '%s': %s", task.schedule_value, e)

    elif task.schedule_type == "interval":
        try:
            ms = int(task.schedule_value)
            if ms > 0:
                # APScheduler 4.x uses 'seconds' as the minimum unit
                # Convert milliseconds to seconds
                seconds = ms / 1000.0
                return IntervalTrigger(seconds=seconds, timezone=TIMEZONE)
        except Exception as e:
            logger.warning("Invalid interval '%s': %s", task.schedule_value, e)

    elif task.schedule_type == "once":
        try:
            scheduled = datetime.fromisoformat(task.schedule_value)
            trigger = DateTrigger(run_date=scheduled, timezone=TIMEZONE)
            return trigger
        except Exception as e:
            logger.warning("Invalid date '%s': %s", task.schedule_value, e)

    return None


async def _run_task(task: ScheduledTask, deps: SchedulerDeps) -> None:
    """Execute a scheduled task.

    Args:
        task: The task to execute
        deps: Scheduler dependencies
    """
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

    # Compute next_run for database update
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


def _on_task_executed(job, retval):
    """Called when a scheduled task completes."""
    logger.debug("Task job completed: job_id=%s retval=%s", job.id, retval)


def _on_task_error(job, exc_type, exc_value, traceback):
    """Called when a scheduled task fails."""
    logger.error("Task job failed: job_id=%s error=%s: %s", job.id, exc_type.__name__, exc_value)


def start_scheduler_loop(deps: SchedulerDeps) -> None:
    """Start the APScheduler-based scheduler loop.

    Args:
        deps: Scheduler dependencies
    """
    global _scheduler, _scheduler_running

    if _scheduler_running:
        logger.debug("Scheduler already running")
        return

    _scheduler = AsyncIOScheduler(timezone=TIMEZONE)
    _scheduler_running = True

    # Load existing tasks and schedule them
    async def _load_and_schedule():
        from omiga.database import get_all_tasks as db_get_all_tasks
        tasks = await db_get_all_tasks()
        scheduled_count = 0

        for task in tasks:
            if task.status != "active":
                continue

            trigger = _create_trigger(task)
            if not trigger:
                logger.warning("Could not create trigger for task: id=%s", task.id)
                continue

            # Create job args
            job_id = f"task_{task.id}"

            # Schedule the job
            _scheduler.add_job(
                _run_task,
                trigger=trigger,
                args=[task, deps],
                id=job_id,
                name=f"Task-{task.id}",
                replace_existing=True,
                on_success=_on_task_executed,
                on_error=_on_task_error,
            )
            scheduled_count += 1
            logger.info(
                "Scheduled task: id=%s type=%s next_run=%s",
                task.id,
                task.schedule_type,
                task.next_run,
            )

        logger.info("Loaded %d active task(s) into APScheduler", scheduled_count)

    # Run the initial load in the event loop
    asyncio.ensure_future(_load_and_schedule())

    _scheduler.start()
    logger.info("APScheduler started with timezone=%s", TIMEZONE)


def stop_scheduler() -> None:
    """Stop the scheduler gracefully."""
    global _scheduler, _scheduler_running

    if _scheduler:
        try:
            _scheduler.shutdown(wait=True)
        except Exception:
            pass  # Ignore if already stopped
        _scheduler = None
        _scheduler_running = False
        logger.info("Scheduler stopped")


def reschedule_task(task: ScheduledTask, deps: SchedulerDeps) -> bool:
    """Reschedule a task (e.g., after create/update).

    Args:
        task: The task to reschedule
        deps: Scheduler dependencies

    Returns:
        True if successfully rescheduled
    """
    global _scheduler

    if not _scheduler or not _scheduler_running:
        return False

    if task.status != "active":
        # Remove from scheduler if paused
        job_id = f"task_{task.id}"
        try:
            _scheduler.remove_job(job_id)
            logger.info("Removed paused task from scheduler: id=%s", task.id)
        except Exception:
            pass
        return True

    trigger = _create_trigger(task)
    if not trigger:
        logger.warning("Could not create trigger for task: id=%s", task.id)
        return False

    job_id = f"task_{task.id}"
    _scheduler.add_job(
        _run_task,
        trigger=trigger,
        args=[task, deps],
        id=job_id,
        name=f"Task-{task.id}",
        replace_existing=True,
        on_success=_on_task_executed,
        on_error=_on_task_error,
    )

    logger.info(
        "Rescheduled task: id=%s type=%s",
        task.id,
        task.schedule_type,
    )
    return True


def remove_task(task_id: str) -> bool:
    """Remove a task from the scheduler.

    Args:
        task_id: The task ID to remove

    Returns:
        True if removed successfully
    """
    global _scheduler

    if not _scheduler:
        return False

    job_id = f"task_{task_id}"
    try:
        _scheduler.remove_job(job_id)
        logger.info("Removed task from scheduler: id=%s", task_id)
        return True
    except Exception as e:
        logger.warning("Failed to remove task %s: %s", task_id, e)
        return False


def get_scheduler_status() -> dict:
    """Get scheduler status information.

    Returns:
        Status dictionary
    """
    global _scheduler

    if not _scheduler:
        return {"running": False, "job_count": 0}

    return {
        "running": _scheduler_running,
        "job_count": len(_scheduler.get_jobs()),
        "timezone": str(_scheduler.timezone),
    }


def _reset_scheduler_for_tests() -> None:
    """Reset scheduler state for tests."""
    global _scheduler, _scheduler_running
    if _scheduler:
        _scheduler.shutdown(wait=False)
        _scheduler = None
    _scheduler_running = False
