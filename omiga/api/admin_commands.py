"""
Inline admin commands for Omiga.

Commands are intercepted from the main group chat (MAIN_GROUP_JID) before
messages reach the container, so the agent is not involved.

Available commands (main group only)
-------------------------------------
/help                               - List all commands
/status                             - Channel connection status + system health
/list                               - List all registered groups
/register <jid> <name>              - Register a group
/unregister <jid>                   - Unregister a group (preserves folder on disk)
/tasks                              - List scheduled tasks
/ping                               - Quick liveness check

Task management (any registered group)
----------------------------------------
/task list                          - List tasks for this group
/task add "<prompt>" <type> <value> - Create a new task
/task pause <id>                    - Pause a task
/task resume <id>                   - Resume a task
/task delete <id>                   - Delete a task
/task run <id>                      - Trigger a task immediately (next poll ≤60 s)
/task info <id>                     - Show full task details

Task type / value examples:
  cron     "0 8 * * *"      → every day at 08:00 UTC
  interval 3600              → every hour
  once     "2026-03-15T09:00"→ one-time at that UTC time
"""
from __future__ import annotations

import logging
import shlex
import uuid
from datetime import datetime, timedelta, timezone
from typing import TYPE_CHECKING, Callable, Optional

if TYPE_CHECKING:
    from omiga.channels.base import Channel
    from omiga.models import RegisteredGroup, ScheduledTask

logger = logging.getLogger(__name__)

# Prefix that marks an admin command
COMMAND_PREFIX = "/"


def is_admin_command(text: str) -> bool:
    """Return True if *text* looks like an admin command (starts with /)."""
    stripped = text.strip()
    return stripped.startswith(COMMAND_PREFIX) and len(stripped) > 1 and " " != stripped[1:2]


def _now_utc() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


def _fmt_bool(v: Optional[bool]) -> str:
    if v is True:
        return "yes"
    if v is False:
        return "no"
    return "default"


async def handle_admin_command(
    text: str,
    channels: "list[Channel]",
    registered_groups: "dict[str, RegisteredGroup]",
    get_tasks: "Callable[[], list[ScheduledTask]]",
    register_group_fn: "Callable[[str, str], None]",
    unregister_group_fn: "Callable[[str], None]",
) -> Optional[str]:
    """Parse and execute an admin command.

    Returns the reply text, or None if the command was not recognised.

    Parameters
    ----------
    text:
        Raw message text (already stripped of leading whitespace).
    channels:
        Active channel list.
    registered_groups:
        Current ``{jid: RegisteredGroup}`` mapping.
    get_tasks:
        Callable returning the current task list.
    register_group_fn:
        Called as ``register_group_fn(jid, name)`` to register a new group.
    unregister_group_fn:
        Called as ``unregister_group_fn(jid)`` to remove a group.
    """
    parts = text.strip().split()
    if not parts:
        return None

    cmd = parts[0].lower()

    # ------------------------------------------------------------------
    # /help
    # ------------------------------------------------------------------
    if cmd == "/help":
        return (
            "Omiga admin commands (main group only):\n"
            "\n"
            "/help                 — this message\n"
            "/status               — channel & system status\n"
            "/list                 — list registered groups\n"
            "/register <jid> <name>— register a group\n"
            "/unregister <jid>     — unregister a group\n"
            "/tasks                — list scheduled tasks\n"
            "/ping                 — liveness check\n"
            "\n"
            "Task commands (any registered group):\n"
            "/task list\n"
            '/task add "<prompt>" cron "0 8 * * *"\n'
            "/task add \"<prompt>\" interval 3600\n"
            '/task add "<prompt>" once "2026-03-15T09:00"\n'
            "/task pause <id>\n"
            "/task resume <id>\n"
            "/task delete <id>\n"
            "/task run <id>\n"
            "/task info <id>"
        )

    # ------------------------------------------------------------------
    # /ping
    # ------------------------------------------------------------------
    if cmd == "/ping":
        return f"pong — {_now_utc()}"

    # ------------------------------------------------------------------
    # /status
    # ------------------------------------------------------------------
    if cmd == "/status":
        lines = ["[Omiga status]", f"Time: {_now_utc()}"]
        lines.append(f"Registered groups: {len(registered_groups)}")
        lines.append("")
        lines.append("Channels:")
        for ch in channels:
            state = "connected" if ch.is_connected() else "DISCONNECTED"
            lines.append(f"  {ch.name}: {state}")
        return "\n".join(lines)

    # ------------------------------------------------------------------
    # /list
    # ------------------------------------------------------------------
    if cmd == "/list":
        if not registered_groups:
            return "No groups registered."
        lines = [f"[{len(registered_groups)} registered group(s)]"]
        for jid, grp in registered_groups.items():
            trigger = _fmt_bool(not grp.requires_trigger if grp.requires_trigger is not None else None)
            lines.append(f"  {grp.name} | {jid} | folder={grp.folder} | no_trigger={trigger}")
        return "\n".join(lines)

    # ------------------------------------------------------------------
    # /register <jid> <name ...>
    # ------------------------------------------------------------------
    if cmd == "/register":
        if len(parts) < 3:
            return "Usage: /register <jid> <name>\nExample: /register tg:123456789 My Group"
        jid = parts[1]
        name = " ".join(parts[2:])
        if jid in registered_groups:
            return f"Already registered: {jid} ('{registered_groups[jid].name}')"
        try:
            register_group_fn(jid, name)
            return f"Registered: {name} ({jid})"
        except Exception as exc:
            logger.error("Admin /register failed: %s", exc)
            return f"Error: {exc}"

    # ------------------------------------------------------------------
    # /unregister <jid>
    # ------------------------------------------------------------------
    if cmd == "/unregister":
        if len(parts) < 2:
            return "Usage: /unregister <jid>"
        jid = parts[1]
        if jid not in registered_groups:
            return f"Not registered: {jid}"
        grp_name = registered_groups[jid].name
        try:
            unregister_group_fn(jid)
            return f"Unregistered: {grp_name} ({jid}). Folder preserved on disk."
        except Exception as exc:
            logger.error("Admin /unregister failed: %s", exc)
            return f"Error: {exc}"

    # ------------------------------------------------------------------
    # /tasks
    # ------------------------------------------------------------------
    if cmd == "/tasks":
        try:
            tasks = get_tasks()
        except Exception as exc:
            return f"Error fetching tasks: {exc}"
        if not tasks:
            return "No scheduled tasks."
        lines = [f"[{len(tasks)} task(s)]"]
        for t in tasks:
            next_run = t.next_run or "—"
            lines.append(
                f"  [{t.id[:8]}] {t.group_folder} | {t.schedule_type}:{t.schedule_value} "
                f"| {t.status} | next={next_run}"
            )
        return "\n".join(lines)

    return None  # unknown command — let the container handle it


# ---------------------------------------------------------------------------
# /task subcommands (available from any registered group)
# ---------------------------------------------------------------------------

def _short_id() -> str:
    return uuid.uuid4().hex[:8]


def _compute_next_run(schedule_type: str, schedule_value: str) -> Optional[str]:
    """Calculate the first run time for a new task."""
    now = datetime.now(timezone.utc)
    if schedule_type == "cron":
        try:
            from croniter import croniter
            return croniter(schedule_value, now).get_next(datetime).isoformat()
        except Exception as exc:
            logger.debug("croniter error: %s", exc)
            return None
    elif schedule_type == "interval":
        try:
            secs = int(float(schedule_value))
            return (now + timedelta(seconds=secs)).isoformat()
        except Exception:
            return None
    elif schedule_type == "once":
        try:
            dt = datetime.fromisoformat(schedule_value)
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=timezone.utc)
            return dt.isoformat()
        except Exception:
            return None
    return None


def _parse_task_add(args: str):
    """Parse: "prompt" type value  (shlex-split, handles quoted strings).

    Returns ``(prompt, schedule_type, schedule_value)`` or ``(None, None, None)``.
    """
    try:
        tokens = shlex.split(args)
    except ValueError:
        return None, None, None
    if len(tokens) < 3:
        return None, None, None
    return tokens[0], tokens[1].lower(), tokens[2]


def _fmt_task(t: "ScheduledTask") -> str:
    next_run = (t.next_run or "—")[:19]
    return (
        f"[{t.id[:8]}] {t.schedule_type}:{t.schedule_value} "
        f"| {t.status} | next={next_run}\n"
        f"  {t.prompt[:80]}"
    )


async def handle_task_command(
    text: str,
    jid: str,
    registered_groups: "dict[str, RegisteredGroup]",
) -> Optional[str]:
    """Handle ``/task <sub> [args]`` from any registered group.

    Returns the reply string, or None if the command is unrecognised.
    """
    from omiga.database import (
        create_task,
        delete_task,
        get_all_tasks,
        get_task_by_id,
        get_tasks_for_group,
        update_task,
    )
    from omiga.models import ScheduledTask

    parts = text.strip().split(None, 2)  # ["/task", sub, rest?]
    if len(parts) < 2:
        return _task_usage()

    sub = parts[1].lower()
    rest = parts[2].strip() if len(parts) > 2 else ""

    # Determine folder for the calling group
    group = registered_groups.get(jid)
    folder = group.folder if group else "main"

    # ------------------------------------------------------------------
    # list
    # ------------------------------------------------------------------
    if sub == "list":
        tasks = await get_tasks_for_group(folder)
        if not tasks:
            return f"No tasks for group '{folder}'."
        lines = [f"[{len(tasks)} task(s) in '{folder}']"]
        for t in tasks:
            lines.append("  " + _fmt_task(t))
        return "\n".join(lines)

    # ------------------------------------------------------------------
    # add "prompt" type value
    # ------------------------------------------------------------------
    if sub == "add":
        if not rest:
            return _task_usage()
        prompt, stype, svalue = _parse_task_add(rest)
        if not prompt or not stype or not svalue:
            return (
                "Usage: /task add \"<prompt>\" <type> <value>\n"
                "Types: cron | interval | once\n"
                "Examples:\n"
                '  /task add "Daily report" cron "0 8 * * *"\n'
                "  /task add \"Hourly ping\" interval 3600"
            )
        if stype not in ("cron", "interval", "once"):
            return f"Unknown schedule type '{stype}'. Use: cron | interval | once"

        next_run = _compute_next_run(stype, svalue)
        if next_run is None:
            return f"Invalid schedule value '{svalue}' for type '{stype}'."

        task = ScheduledTask(
            id=_short_id(),
            group_folder=folder,
            chat_jid=jid,
            prompt=prompt,
            schedule_type=stype,
            schedule_value=svalue,
            context_mode="group",
            next_run=next_run,
            last_run=None,
            last_result=None,
            status="active",
            created_at=datetime.now(timezone.utc).isoformat(),
        )
        await create_task(task)
        return (
            f"Task created: [{task.id}]\n"
            f"  Prompt: {prompt[:80]}\n"
            f"  Schedule: {stype} {svalue}\n"
            f"  First run: {next_run[:19]} UTC"
        )

    # ------------------------------------------------------------------
    # pause / resume / delete / run / info — require a task id
    # ------------------------------------------------------------------
    if sub in ("pause", "resume", "delete", "run", "info"):
        if not rest:
            return f"Usage: /task {sub} <id>"

        task_id = rest.split()[0]
        task = await get_task_by_id(task_id)

        # Also try prefix match (short id)
        if not task:
            all_tasks = await get_tasks_for_group(folder)
            matches = [t for t in all_tasks if t.id.startswith(task_id)]
            if len(matches) == 1:
                task = matches[0]
            elif len(matches) > 1:
                return f"Ambiguous ID '{task_id}' matches {len(matches)} tasks. Use more characters."

        if not task:
            return f"Task not found: {task_id}"

        # Restrict non-main groups to their own tasks
        if task.group_folder != folder and folder != "main":
            return f"Task [{task.id}] belongs to group '{task.group_folder}', not '{folder}'."

        if sub == "info":
            return (
                f"Task [{task.id}]\n"
                f"  Group:    {task.group_folder}\n"
                f"  Status:   {task.status}\n"
                f"  Schedule: {task.schedule_type} {task.schedule_value}\n"
                f"  Context:  {task.context_mode}\n"
                f"  Next run: {task.next_run or '—'}\n"
                f"  Last run: {task.last_run or '—'}\n"
                f"  Prompt:   {task.prompt}"
            )

        if sub == "pause":
            if task.status == "paused":
                return f"Task [{task.id}] is already paused."
            await update_task(task.id, status="paused")
            return f"Task [{task.id}] paused."

        if sub == "resume":
            if task.status == "active":
                return f"Task [{task.id}] is already active."
            # Recalculate next_run from now when resuming
            next_run = _compute_next_run(task.schedule_type, task.schedule_value)
            await update_task(task.id, status="active", next_run=next_run)
            return f"Task [{task.id}] resumed. Next run: {(next_run or '—')[:19]} UTC"

        if sub == "delete":
            await delete_task(task.id)
            return f"Task [{task.id}] deleted."

        if sub == "run":
            # Set next_run to now — scheduler will pick it up within 60 s
            now = datetime.now(timezone.utc).isoformat()
            await update_task(task.id, status="active", next_run=now)
            return f"Task [{task.id}] triggered. Will run within the next scheduler cycle (≤60 s)."

    return _task_usage()


def _task_usage() -> str:
    return (
        "Task commands:\n"
        "/task list\n"
        '/task add "<prompt>" cron "0 8 * * *"\n'
        "/task add \"<prompt>\" interval 3600\n"
        '/task add "<prompt>" once "2026-03-15T09:00"\n'
        "/task pause <id>\n"
        "/task resume <id>\n"
        "/task delete <id>\n"
        "/task run <id>\n"
        "/task info <id>"
    )
