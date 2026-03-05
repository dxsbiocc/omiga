"""Agent events for Omiga.

This module provides a comprehensive event system for Agent lifecycle,
message streaming, tool calls, and error handling.
"""
from __future__ import annotations

import logging
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from typing import Any, Callable, Dict, List, Optional, Union

logger = logging.getLogger("omiga.events")


class AgentEventType(str, Enum):
    """Agent event types."""

    # Agent lifecycle
    AGENT_START = "agent_start"
    AGENT_END = "agent_end"
    AGENT_ERROR = "agent_error"

    # Message events (streaming)
    MESSAGE_START = "message_start"
    MESSAGE_UPDATE = "message_update"  # Delta update
    MESSAGE_END = "message_end"

    # Tool call events (streaming)
    TOOL_CALL_START = "tool_call_start"
    TOOL_CALL_UPDATE = "tool_call_update"  # Delta update
    TOOL_CALL_END = "tool_call_end"

    # Session events
    SESSION_CREATED = "session_created"
    SESSION_COMPACTED = "session_compacted"

    # State changes
    STATE_CHANGED = "state_changed"


class SessionState(str, Enum):
    """Agent session state."""

    IDLE = "IDLE"
    THINKING = "THINKING"
    ACTING = "ACTING"
    FINISHED = "FINISHED"
    ERROR = "ERROR"


@dataclass
class AgentEvent:
    """An agent event.

    Attributes:
        type: Event type
        timestamp: ISO format UTC timestamp
        session_id: Optional session identifier
        data: Event-specific payload
    """

    type: AgentEventType
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    session_id: Optional[str] = None
    data: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary."""
        return {
            "type": self.type.value,
            "timestamp": self.timestamp,
            "session_id": self.session_id,
            "data": self.data,
        }


# Type aliases for event data
MessageData = Dict[str, Any]  # {role, content, tool_calls?}
ToolCallData = Dict[str, Any]  # {tool_name, args, id?}
ErrorData = Dict[str, Any]  # {error, error_type, traceback?}
StateData = Dict[str, Any]  # {old_state, new_state}


# ---------------------------------------------------------------------------
# Event Builders
# ---------------------------------------------------------------------------


def agent_start_event(
    session_id: str,
    prompt: str,
) -> AgentEvent:
    """Create an agent_start event."""
    return AgentEvent(
        type=AgentEventType.AGENT_START,
        session_id=session_id,
        data={"prompt": prompt},
    )


def agent_end_event(
    session_id: str,
    result: str,
    steps: int,
    tool_calls: int,
) -> AgentEvent:
    """Create an agent_end event."""
    return AgentEvent(
        type=AgentEventType.AGENT_END,
        session_id=session_id,
        data={
            "result": result,
            "steps": steps,
            "tool_calls": tool_calls,
        },
    )


def agent_error_event(
    session_id: str,
    error: str,
    error_type: str = "Unknown",
) -> AgentEvent:
    """Create an agent_error event."""
    return AgentEvent(
        type=AgentEventType.AGENT_ERROR,
        session_id=session_id,
        data={"error": error, "error_type": error_type},
    )


def message_start_event(
    session_id: str,
    message: MessageData,
) -> AgentEvent:
    """Create a message_start event."""
    return AgentEvent(
        type=AgentEventType.MESSAGE_START,
        session_id=session_id,
        data={"message": message},
    )


def message_update_event(
    session_id: str,
    delta: str,
) -> AgentEvent:
    """Create a message_update event (streaming delta)."""
    return AgentEvent(
        type=AgentEventType.MESSAGE_UPDATE,
        session_id=session_id,
        data={"delta": delta},
    )


def message_end_event(
    session_id: str,
    message: MessageData,
) -> AgentEvent:
    """Create a message_end event."""
    return AgentEvent(
        type=AgentEventType.MESSAGE_END,
        session_id=session_id,
        data={"message": message},
    )


def tool_call_start_event(
    session_id: str,
    tool_name: str,
    args: Dict[str, Any],
    tool_call_id: Optional[str] = None,
) -> AgentEvent:
    """Create a tool_call_start event."""
    return AgentEvent(
        type=AgentEventType.TOOL_CALL_START,
        session_id=session_id,
        data={
            "tool_name": tool_name,
            "args": args,
            "tool_call_id": tool_call_id,
        },
    )


def tool_call_update_event(
    session_id: str,
    delta: str,
    tool_call_id: Optional[str] = None,
) -> AgentEvent:
    """Create a tool_call_update event (streaming delta)."""
    return AgentEvent(
        type=AgentEventType.TOOL_CALL_UPDATE,
        session_id=session_id,
        data={"delta": delta, "tool_call_id": tool_call_id},
    )


def tool_call_end_event(
    session_id: str,
    tool_name: str,
    result: Any,
    success: bool,
    tool_call_id: Optional[str] = None,
) -> AgentEvent:
    """Create a tool_call_end event."""
    return AgentEvent(
        type=AgentEventType.TOOL_CALL_END,
        session_id=session_id,
        data={
            "tool_name": tool_name,
            "result": result,
            "success": success,
            "tool_call_id": tool_call_id,
        },
    )


def session_created_event(
    session_id: str,
    chat_jid: str,
) -> AgentEvent:
    """Create a session_created event."""
    return AgentEvent(
        type=AgentEventType.SESSION_CREATED,
        session_id=session_id,
        data={"chat_jid": chat_jid},
    )


def session_compacted_event(
    session_id: str,
    summary: str,
    tokens_before: int,
    tokens_after: int,
) -> AgentEvent:
    """Create a session_compacted event."""
    return AgentEvent(
        type=AgentEventType.SESSION_COMPACTED,
        session_id=session_id,
        data={
            "summary": summary,
            "tokens_before": tokens_before,
            "tokens_after": tokens_after,
        },
    )


def state_changed_event(
    session_id: str,
    old_state: str,
    new_state: str,
) -> AgentEvent:
    """Create a state_changed event."""
    return AgentEvent(
        type=AgentEventType.STATE_CHANGED,
        session_id=session_id,
        data={"old_state": old_state, "new_state": new_state},
    )


# ---------------------------------------------------------------------------
# Event Bus
# ---------------------------------------------------------------------------


class AgentEventBus:
    """Event bus for agent events.

    Features:
    - Subscribe to specific event types
    - Publish events to all subscribers
    - Error isolation (subscriber errors don't affect others)
    - Event history for debugging
    - Async support

    Usage:
        bus = AgentEventBus()

        # Subscribe
        def on_tool_call(event: AgentEvent):
            print(f"Tool called: {event.data['tool_name']}")

        bus.subscribe(AgentEventType.TOOL_CALL_START, on_tool_call)

        # Publish
        bus.publish(tool_call_start_event("session1", "read_file", {"path": "test.txt"}))
    """

    def __init__(self, max_history: int = 1000):
        """Initialize event bus.

        Args:
            max_history: Maximum events to keep in history
        """
        self._subscribers: Dict[
            AgentEventType, List[Callable[[AgentEvent], None]]
        ] = {}
        self._event_history: deque[AgentEvent] = deque(maxlen=max_history)
        self._max_history = max_history

        # Event counters for statistics
        self._event_counts: Dict[AgentEventType, int] = {}

    def subscribe(
        self,
        event_type: AgentEventType,
        callback: Callable[[AgentEvent], None],
    ) -> Callable[[], None]:
        """Subscribe to an event type.

        Args:
            event_type: Type of event to subscribe to
            callback: Function to call when event is published

        Returns:
            Unsubscribe function
        """
        self._subscribers.setdefault(event_type, []).append(callback)
        logger.debug(f"Subscribed to {event_type.value}")

        def unsubscribe() -> None:
            """Remove this subscription."""
            subscribers = self._subscribers.get(event_type)
            if subscribers and callback in subscribers:
                subscribers.remove(callback)
                logger.debug(f"Unsubscribed from {event_type.value}")

        return unsubscribe

    def publish(self, event: AgentEvent) -> None:
        """Publish an event to all subscribers.

        Args:
            event: Event to publish
        """
        subscribers = self._subscribers.get(event.type, [])
        logger.debug(
            f"Publishing {event.type.value} to {len(subscribers)} subscribers"
        )

        for callback in subscribers:
            try:
                callback(event)
            except Exception as e:
                logger.warning(
                    f"Event callback failed for {event.type.value}: {e}",
                    exc_info=True,
                )

        # Store in history (deque with maxlen auto-trims)
        self._event_history.append(event)
        self._event_counts[event.type] = (
            self._event_counts.get(event.type, 0) + 1
        )

    def get_recent_events(
        self,
        event_type: Optional[AgentEventType] = None,
        limit: int = 50,
    ) -> List[AgentEvent]:
        """Get recent events from history.

        Args:
            event_type: Optional filter by event type
            limit: Maximum number of events to return

        Returns:
            List of recent events (most recent first)
        """
        events = list(self._event_history)  # Convert deque to list
        if event_type:
            events = [e for e in events if e.type == event_type]
        return list(reversed(events[-limit:]))

    def get_statistics(self) -> Dict[str, Any]:
        """Get event statistics.

        Returns:
            Statistics dictionary
        """
        return {
            "total_events": sum(self._event_counts.values()),
            "by_type": {
                et.value: count for et, count in self._event_counts.items()
            },
            "history_size": len(self._event_history),
        }

    def clear_history(self) -> None:
        """Clear event history."""
        self._event_history.clear()
        self._event_counts.clear()


# ---------------------------------------------------------------------------
# Global Event Bus
# ---------------------------------------------------------------------------

_event_bus: Optional[AgentEventBus] = None


def get_event_bus() -> AgentEventBus:
    """Get the global event bus instance."""
    global _event_bus
    if _event_bus is None:
        _event_bus = AgentEventBus()
    return _event_bus


def reset_event_bus() -> None:
    """Reset the global event bus (for testing)."""
    global _event_bus
    _event_bus = None
