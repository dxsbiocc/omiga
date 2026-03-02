"""IPC watcher for Omiga.

Watches per-group IPC directories for JSON files written by container agents
using ``watchfiles.awatch`` instead of a polling loop.

Handles messages, task operations, and group registration.

Mirrors src/ipc.ts exactly (authorization logic, per-group namespaces).
"""
from __future__ import annotations

import asyncio
import json
import logging
import random
import string
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import TYPE_CHECKING, Callable, Optional

from watchfiles import Change, awatch

from omiga.config import DATA_DIR, MAIN_GROUP_FOLDER, TIMEZONE
from omiga.database import (
    create_task,
    delete_task,
    get_task_by_id,
    update_task,
)
from omiga.group_folder import is_valid_group_folder
from omiga.models import AvailableGroup, RegisteredGroup, ScheduledTask

if TYPE_CHECKING:
    pass

logger = logging.getLogger(__name__)

_ipc_watcher_running = False
_shutdown_event: Optional[asyncio.Event] = None


class IpcDeps:
    """Dependency bundle passed to the IPC watcher."""

    def __init__(
        self,
        *,
        send_message: Callable[[str, str], "asyncio.coroutine | asyncio.Future"],
        registered_groups: Callable[[], dict[str, RegisteredGroup]],
        register_group: Callable[[str, RegisteredGroup], None],
        sync_group_metadata: Callable[[bool], "asyncio.coroutine | asyncio.Future"],
        get_available_groups: Callable[[], list[AvailableGroup]],
        write_groups_snapshot: Callable[[str, bool, list[AvailableGroup], set[str]], None],
    ) -> None:
        self.send_message = send_message
        self.registered_groups = registered_groups
        self.register_group = register_group
        self.sync_group_metadata = sync_group_metadata
        self.get_available_groups = get_available_groups
        self.write_groups_snapshot = write_groups_snapshot


async def process_task_ipc(
    data: dict,
    source_group: str,
    is_main: bool,
    deps: IpcDeps,
) -> None:
    """Process a single IPC task command from *source_group*."""
    registered = deps.registered_groups()
    task_type = data.get("type", "")

    if task_type == "schedule_task":
        prompt = data.get("prompt")
        schedule_type = data.get("schedule_type") or data.get("scheduleType")
        schedule_value = data.get("schedule_value") or data.get("scheduleValue")
        target_jid = data.get("targetJid") or data.get("target_jid")

        if not (prompt and schedule_type and schedule_value and target_jid):
            return

        target_group = registered.get(target_jid)
        if not target_group:
            logger.warning("Cannot schedule task: target JID not registered: %s", target_jid)
            return

        target_folder = target_group.folder

        if not is_main and target_folder != source_group:
            logger.warning(
                "Unauthorized schedule_task blocked: source=%s target=%s",
                source_group,
                target_folder,
            )
            return

        # Compute next_run
        next_run: Optional[str] = None
        if schedule_type == "cron":
            try:
                from croniter import croniter
                it = croniter(schedule_value, datetime.now(timezone.utc))
                next_run = it.get_next(datetime).isoformat()
            except Exception:
                logger.warning("Invalid cron expression: %s", schedule_value)
                return
        elif schedule_type == "interval":
            try:
                ms = int(schedule_value)
                if ms <= 0:
                    raise ValueError
                next_run = (
                    datetime.fromtimestamp(time.time() + ms / 1000, tz=timezone.utc).isoformat()
                )
            except Exception:
                logger.warning("Invalid interval: %s", schedule_value)
                return
        elif schedule_type == "once":
            try:
                scheduled = datetime.fromisoformat(schedule_value)
                next_run = scheduled.isoformat()
            except Exception:
                logger.warning("Invalid timestamp: %s", schedule_value)
                return

        rand = "".join(random.choices(string.ascii_lowercase + string.digits, k=6))
        task_id = f"task-{int(time.time() * 1000)}-{rand}"

        context_mode_raw = data.get("context_mode") or data.get("contextMode") or "isolated"
        context_mode = context_mode_raw if context_mode_raw in ("group", "isolated") else "isolated"

        task = ScheduledTask(
            id=task_id,
            group_folder=target_folder,
            chat_jid=target_jid,
            prompt=prompt,
            schedule_type=schedule_type,
            schedule_value=schedule_value,
            context_mode=context_mode,
            next_run=next_run,
            last_run=None,
            last_result=None,
            status="active",
            created_at=datetime.now(timezone.utc).isoformat(),
        )
        await create_task(task)
        logger.info("Task created via IPC: id=%s source=%s target=%s", task_id, source_group, target_folder)

    elif task_type == "pause_task":
        task_id = data.get("taskId") or data.get("task_id")
        if task_id:
            t = await get_task_by_id(task_id)
            if t and (is_main or t.group_folder == source_group):
                await update_task(task_id, status="paused")
                logger.info("Task paused via IPC: id=%s source=%s", task_id, source_group)
            else:
                logger.warning("Unauthorized task pause attempt: id=%s source=%s", task_id, source_group)

    elif task_type == "resume_task":
        task_id = data.get("taskId") or data.get("task_id")
        if task_id:
            t = await get_task_by_id(task_id)
            if t and (is_main or t.group_folder == source_group):
                await update_task(task_id, status="active")
                logger.info("Task resumed via IPC: id=%s source=%s", task_id, source_group)
            else:
                logger.warning("Unauthorized task resume attempt: id=%s source=%s", task_id, source_group)

    elif task_type == "cancel_task":
        task_id = data.get("taskId") or data.get("task_id")
        if task_id:
            t = await get_task_by_id(task_id)
            if t and (is_main or t.group_folder == source_group):
                await delete_task(task_id)
                logger.info("Task cancelled via IPC: id=%s source=%s", task_id, source_group)
            else:
                logger.warning("Unauthorized task cancel attempt: id=%s source=%s", task_id, source_group)

    elif task_type == "refresh_groups":
        if is_main:
            logger.info("Group metadata refresh requested via IPC: source=%s", source_group)
            coro = deps.sync_group_metadata(True)
            if asyncio.iscoroutine(coro):
                await coro
            available = deps.get_available_groups()
            deps.write_groups_snapshot(
                source_group,
                True,
                available,
                set(registered.keys()),
            )
        else:
            logger.warning("Unauthorized refresh_groups attempt: source=%s", source_group)

    elif task_type == "register_group":
        if not is_main:
            logger.warning("Unauthorized register_group attempt: source=%s", source_group)
            return

        jid = data.get("jid")
        name = data.get("name")
        folder = data.get("folder")
        trigger = data.get("trigger")

        if not (jid and name and folder and trigger):
            logger.warning("Invalid register_group request — missing fields: %s", data)
            return

        if not is_valid_group_folder(folder):
            logger.warning(
                "Invalid register_group request — unsafe folder: source=%s folder=%s",
                source_group,
                folder,
            )
            return

        requires_trigger = data.get("requiresTrigger")
        group = RegisteredGroup(
            name=name,
            folder=folder,
            trigger=trigger,
            added_at=datetime.now(timezone.utc).isoformat(),
            requires_trigger=requires_trigger,
        )
        deps.register_group(jid, group)

    else:
        logger.warning("Unknown IPC task type: %s", task_type)


async def _process_single_file(
    path: Path,
    group_name: str,
    subdir: str,
    deps: IpcDeps,
) -> None:
    """Process one IPC JSON file from a known group directory."""
    ipc_base = DATA_DIR / "ipc"
    error_dir = ipc_base / "errors"
    registered = deps.registered_groups()
    is_main = group_name == MAIN_GROUP_FOLDER

    try:
        data = json.loads(path.read_text())
    except Exception as err:
        logger.error("Error reading IPC file %s: %s", path.name, err)
        error_dir.mkdir(parents=True, exist_ok=True)
        try:
            path.rename(error_dir / f"{group_name}-{path.name}")
        except Exception:
            pass
        return

    try:
        if subdir == "messages":
            if data.get("type") == "message" and data.get("chatJid") and data.get("text"):
                chat_jid = data["chatJid"]
                target_group = registered.get(chat_jid)
                if is_main or (target_group and target_group.folder == group_name):
                    coro = deps.send_message(chat_jid, data["text"])
                    if asyncio.iscoroutine(coro):
                        await coro
                    logger.info(
                        "IPC message sent: chatJid=%s source=%s",
                        chat_jid,
                        group_name,
                    )
                else:
                    logger.warning(
                        "Unauthorized IPC message blocked: chatJid=%s source=%s",
                        chat_jid,
                        group_name,
                    )
            path.unlink(missing_ok=True)

        elif subdir == "tasks":
            await process_task_ipc(data, group_name, is_main, deps)
            path.unlink(missing_ok=True)

    except Exception as err:
        logger.error("Error processing IPC file %s: %s", path.name, err)
        error_dir.mkdir(parents=True, exist_ok=True)
        try:
            path.rename(error_dir / f"{group_name}-{path.name}")
        except Exception:
            pass


async def _watch_loop(deps: IpcDeps) -> None:
    """Event-driven IPC loop using watchfiles.awatch."""
    ipc_base = DATA_DIR / "ipc"
    ipc_base.mkdir(parents=True, exist_ok=True)
    logger.info("IPC watcher started (watchfiles)")

    async for changes in awatch(str(ipc_base), stop_event=_shutdown_event):
        # Process all changes in this batch, sorted by path for deterministic order
        for change_type, path_str in sorted(changes, key=lambda x: x[1]):
            if change_type == Change.deleted:
                continue

            path = Path(path_str)
            if path.suffix != ".json":
                continue

            try:
                parts = path.relative_to(ipc_base).parts
            except ValueError:
                continue

            if len(parts) != 3:  # group_name / {messages|tasks} / file.json
                continue

            group_name, subdir, _ = parts
            if group_name == "errors":
                continue
            if subdir not in ("messages", "tasks"):
                continue

            await _process_single_file(path, group_name, subdir, deps)


def start_ipc_watcher(deps: IpcDeps) -> None:
    """Start the async IPC file watcher (idempotent)."""
    global _ipc_watcher_running, _shutdown_event
    if _ipc_watcher_running:
        logger.debug("IPC watcher already running")
        return
    _ipc_watcher_running = True

    # Create a fresh stop event for this run
    _shutdown_event = asyncio.Event()

    ipc_base = DATA_DIR / "ipc"
    ipc_base.mkdir(parents=True, exist_ok=True)

    asyncio.ensure_future(_watch_loop(deps))


def stop_ipc_watcher() -> None:
    """Signal the IPC watcher to stop (called during graceful shutdown)."""
    global _ipc_watcher_running
    if _shutdown_event is not None:
        _shutdown_event.set()
    _ipc_watcher_running = False
    logger.debug("IPC watcher stop requested")
