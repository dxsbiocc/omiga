"""Memory event bus for decoupled memory operations.

This module provides a lightweight event system for notifying
interested parties about memory-related events without tight coupling.

Event Types:
    - TOOL_CALL_START / TOOL_CALL_END: Tool execution lifecycle
    - SOP_GENERATED / SOP_STATUS_CHANGED: SOP lifecycle events
    - LESSON_LEARNED: New lesson extracted from failure
    - AGENT_STATE_CHANGED: Agent execution state change

Usage Example:
    ```python
    from omiga.memory.events import MemoryEventBus, MemoryEventType, MemoryEvent

    # Create bus (singleton pattern recommended)
    bus = MemoryEventBus()

    # Subscribe to events
    def on_sop_generated(event: MemoryEvent) -> None:
        print(f"SOP generated: {event.data.get('sop_name')}")

    bus.subscribe(MemoryEventType.SOP_GENERATED, on_sop_generated)

    # Publish event
    bus.publish(MemoryEvent(
        type=MemoryEventType.SOP_GENERATED,
        chat_jid="tg:123456",
        data={"sop_name": "File Reader", "confidence": 0.75}
    ))
    ```
"""
from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from typing import Any, Callable, Dict, List, Optional

logger = logging.getLogger("omiga.memory.events")


class MemoryEventType(str, Enum):
    """Types of memory events."""

    # Tool lifecycle
    TOOL_CALL_START = "tool_call_start"
    TOOL_CALL_END = "tool_call_end"

    # SOP lifecycle
    SOP_GENERATED = "sop_generated"
    SOP_STATUS_CHANGED = "sop_status_changed"
    SOP_AUTO_APPROVED = "sop_auto_approved"
    SOP_REJECTED = "sop_rejected"

    # Lesson events
    LESSON_LEARNED = "lesson_learned"

    # Agent state
    AGENT_STATE_CHANGED = "agent_state_changed"

    # Fact events
    FACT_ADDED = "fact_added"
    FACT_UPDATED = "fact_updated"


@dataclass
class MemoryEvent:
    """A memory system event.

    Attributes:
        type: Event type
        timestamp: ISO format UTC timestamp
        chat_jid: Optional group/chat identifier
        data: Event-specific payload
    """

    type: MemoryEventType
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    chat_jid: Optional[str] = None
    data: Dict[str, Any] = field(default_factory=dict)


class MemoryEventBus:
    """Lightweight event bus for memory events.

    Features:
    - Subscribe/unsubscribe to specific event types
    - Publish events to all subscribers
    - Error isolation (subscriber errors don't affect others)

    Thread Safety:
    - Not thread-safe (designed for asyncio single-threaded use)
    """

    def __init__(self):
        """Initialize the event bus."""
        self._subscribers: Dict[
            MemoryEventType, List[Callable[[MemoryEvent], None]]
        ] = {}
        self._event_history: List[MemoryEvent] = []
        self._max_history: int = 100  # Keep last N events for debugging

    def subscribe(
        self,
        event_type: MemoryEventType,
        callback: Callable[[MemoryEvent], None],
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

    def publish(self, event: MemoryEvent) -> None:
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

        # Store in history for debugging
        self._event_history.append(event)
        if len(self._event_history) > self._max_history:
            self._event_history = self._event_history[-self._max_history:]

    def get_recent_events(
        self, event_type: Optional[MemoryEventType] = None, limit: int = 10
    ) -> List[MemoryEvent]:
        """Get recent events from history.

        Args:
            event_type: Optional filter by event type
            limit: Maximum number of events to return

        Returns:
            List of recent events (most recent first)
        """
        events = self._event_history
        if event_type:
            events = [e for e in events if e.type == event_type]
        return list(reversed(events[-limit:]))

    def clear_history(self) -> None:
        """Clear event history."""
        self._event_history.clear()


# Global singleton instance
_event_bus: Optional[MemoryEventBus] = None


def get_event_bus() -> MemoryEventBus:
    """Get the global event bus instance."""
    global _event_bus
    if _event_bus is None:
        _event_bus = MemoryEventBus()
    return _event_bus


def reset_event_bus() -> None:
    """Reset the global event bus (for testing)."""
    global _event_bus
    _event_bus = None
