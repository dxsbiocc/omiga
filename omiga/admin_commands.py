"""
Inline admin commands for Omiga.

Commands are intercepted from the main group chat (MAIN_GROUP_JID) before
messages reach the container, so the agent is not involved.

Available commands
------------------
/help                          - List all commands
/status                        - Channel connection status + system health
/list                          - List all registered groups
/register <jid> <name>         - Register a group (jid must start with channel prefix)
/unregister <jid>              - Unregister a group (preserves folder on disk)
/tasks                         - List scheduled tasks (id, group, schedule, status)
/ping                          - Quick liveness check

Authorization
-------------
Commands are only processed when the message arrives in the main group
(MAIN_GROUP_JID). Any other origin is ignored and the message flows to the
container as normal.
"""
from __future__ import annotations

import logging
from datetime import datetime, timezone
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
            "/ping                 — liveness check"
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
