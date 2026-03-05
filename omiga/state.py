"""Global application state for Omiga.

All mutable module-level variables are centralised here.  Other modules
access and mutate them via::

    import omiga.state as state
    state._last_timestamp
    state._sessions[key] = value
    state._last_timestamp = new_ts   # attribute assignment on module object
"""
from __future__ import annotations

import asyncio
import json
import logging
from enum import Enum
from typing import TYPE_CHECKING, Optional

from omiga.channels.manager import ChannelManager
from omiga.memory.manager import MemoryManager

from omiga.database import (
    delete_registered_group,
    get_all_chats,
    get_all_registered_groups,
    get_all_sessions,
    get_router_state,
    set_registered_group,
    set_router_state,
)
from omiga.group_folder import resolve_group_folder_path
from omiga.group_queue import GroupQueue
from omiga.models import AvailableGroup, ChatInfo, RegisteredGroup
from omiga.agent import AgentSession
from omiga.tools.registry import ToolRegistry

if TYPE_CHECKING:
    from omiga.channels.base import Channel

logger = logging.getLogger("omiga.state")


# ---------------------------------------------------------------------------
# Agent state enums (tracking per-group agent execution state)
# ---------------------------------------------------------------------------


class AgentState(str, Enum):
    """Agent execution state machine."""

    IDLE = "IDLE"  # Idle, ready to accept new messages
    RUNNING = "RUNNING"  # Currently executing
    FINISHED = "FINISHED"  # Execution completed
    ERROR = "ERROR"  # Error state


# ---------------------------------------------------------------------------
# Global state (mirrors index.ts module-level vars)
# ---------------------------------------------------------------------------

_last_timestamp: str = ""
_sessions: dict[str, str] = {}
_registered_groups: dict[str, RegisteredGroup] = {}
_last_agent_timestamp: dict[str, str] = {}
_message_loop_running: bool = False

# Agent state tracking per group (chat_jid -> AgentState)
_agent_states: dict[str, AgentState] = {}

# Agent retry count per group (chat_jid -> retry count)
_agent_retry_counts: dict[str, int] = {}

# Consecutive error counter per group (chat_jid → count).
# Prevents infinite cursor-rollback loops on persistent agent failures.
_consecutive_errors: dict[str, int] = {}
MAX_ROLLBACK_RETRIES: int = 3

# Cache of all known chats, refreshed at startup.  Used by IPC's refresh_groups.
_all_chats_cache: list[ChatInfo] = []

_channels: list[Channel] = []
_queue: GroupQueue = GroupQueue()

# Debounce: maps chat_jid → monotonic deadline before which we don't start a container.
# Cleared once the container is enqueued.
_debounce_deadlines: dict[str, float] = {}

# Shutdown flag: set by signal handler to stop the message loop cleanly.
_shutdown_event: Optional[asyncio.Event] = None

# Channel manager for unified channel management
_channel_manager: Optional[ChannelManager] = None

# Memory manager for three-layer memory system
_memory_manager: Optional[MemoryManager] = None

# Agent session manager (chat_jid -> AgentSession)
_agent_sessions: dict[str, AgentSession] = {}

# Global tool registry
_tool_registry: Optional[ToolRegistry] = None


# ---------------------------------------------------------------------------
# State helpers
# ---------------------------------------------------------------------------


def get_agent_state(jid: str) -> AgentState:
    """Get agent execution state for a group."""
    return _agent_states.get(jid, AgentState.IDLE)


def set_agent_state(jid: str, state: AgentState) -> None:
    """Set agent execution state for a group."""
    _agent_states[jid] = state


def get_agent_retry_count(jid: str) -> int:
    """Get retry count for a group."""
    return _agent_retry_counts.get(jid, 0)


def increment_agent_retry(jid: str) -> int:
    """Increment retry count for a group, returns new count."""
    _agent_retry_counts[jid] = _agent_retry_counts.get(jid, 0) + 1
    return _agent_retry_counts[jid]


def reset_agent_retry(jid: str) -> None:
    """Reset retry count for a group."""
    _agent_retry_counts.pop(jid, None)


async def load_state() -> None:
    global _last_timestamp, _last_agent_timestamp, _sessions, _registered_groups, _all_chats_cache

    _last_timestamp = (await get_router_state("last_timestamp")) or ""
    raw_agent_ts = await get_router_state("last_agent_timestamp")
    try:
        _last_agent_timestamp = json.loads(raw_agent_ts) if raw_agent_ts else {}
    except Exception:
        logger.warning("Corrupted last_agent_timestamp in DB, resetting")
        _last_agent_timestamp = {}

    _sessions = await get_all_sessions()
    _registered_groups = await get_all_registered_groups()
    _all_chats_cache = await get_all_chats()

    # Initialize memory system
    await init_memory()

    logger.info(
        "State loaded: %d registered groups, %d known chats",
        len(_registered_groups),
        len(_all_chats_cache),
    )


async def save_state() -> None:
    await set_router_state("last_timestamp", _last_timestamp)
    await set_router_state("last_agent_timestamp", json.dumps(_last_agent_timestamp))


async def init_memory() -> None:
    """Initialize the memory manager."""
    global _memory_manager
    from omiga.config import DATA_DIR
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    _memory_manager = MemoryManager(memory_dir)
    await _memory_manager.initialize()
    logger.info(
        "Memory initialized: L1=%d topics, L2=%d sections, L3=%d active SOPs",
        len(_memory_manager.get_index().topics),
        len(_memory_manager.get_facts().entries),
        len(_memory_manager.list_active_sops()),
    )


def get_memory_manager() -> Optional[MemoryManager]:
    """Get the memory manager instance."""
    return _memory_manager


# ---------------------------------------------------------------------------
# Agent Session Management
# ---------------------------------------------------------------------------


def get_agent_session(jid: str) -> Optional[AgentSession]:
    """Get agent session for a group."""
    return _agent_sessions.get(jid)


def create_agent_session(
    jid: str, group_folder: str
) -> AgentSession:
    """Create a new agent session for a group.

    Args:
        jid: Group chat JID
        group_folder: Group folder identifier

    Returns:
        New AgentSession instance
    """
    session = AgentSession(group_folder=group_folder)
    _agent_sessions[jid] = session
    logger.info(f"Created agent session: jid={jid} folder={group_folder}")
    return session


def remove_agent_session(jid: str) -> None:
    """Remove agent session for a group."""
    _agent_sessions.pop(jid, None)
    logger.debug(f"Removed agent session: jid={jid}")


def get_tool_registry() -> ToolRegistry:
    """Get the global tool registry."""
    global _tool_registry
    if _tool_registry is None:
        _tool_registry = ToolRegistry()
    return _tool_registry


def set_tool_registry(registry: ToolRegistry) -> None:
    """Set the global tool registry."""
    global _tool_registry
    _tool_registry = registry


async def unregister_group(jid: str) -> None:
    """Remove a group from the in-memory map and the DB (folder is kept)."""
    _registered_groups.pop(jid, None)
    await delete_registered_group(jid)
    logger.info("Group unregistered: jid=%s", jid)


async def register_group(jid: str, group: RegisteredGroup) -> None:
    try:
        group_dir = resolve_group_folder_path(group.folder)
    except ValueError as err:
        logger.warning(
            "Rejecting group registration with invalid folder: jid=%s folder=%s err=%s",
            jid,
            group.folder,
            err,
        )
        return

    _registered_groups[jid] = group
    await set_registered_group(jid, group)

    (group_dir / "logs").mkdir(parents=True, exist_ok=True)
    logger.info("Group registered: jid=%s name=%s folder=%s", jid, group.name, group.folder)


def get_available_groups() -> list[AvailableGroup]:
    """Return all known group chats from the startup cache.

    The cache is populated by load_state() and reflects the chats table at
    startup.  Channels may update it via store_chat_metadata; the IPC watcher
    calls this function synchronously so we use the in-memory snapshot.
    """
    registered_jids = set(_registered_groups.keys())
    return [
        AvailableGroup(
            jid=c.jid,
            name=c.name,
            last_activity=c.last_message_time,
            is_registered=c.jid in registered_jids,
        )
        for c in _all_chats_cache
        if c.jid != "__group_sync__" and c.is_group
    ]
