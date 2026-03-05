"""Events module for Omiga."""
from omiga.events.agent_events import (
    AgentEvent,
    AgentEventType,
    AgentEventBus,
    SessionState,
    # Event builders
    agent_start_event,
    agent_end_event,
    agent_error_event,
    message_start_event,
    message_update_event,
    message_end_event,
    tool_call_start_event,
    tool_call_update_event,
    tool_call_end_event,
    session_created_event,
    session_compacted_event,
    state_changed_event,
    # Global bus
    get_event_bus,
    reset_event_bus,
)

__all__ = [
    "AgentEvent",
    "AgentEventType",
    "AgentEventBus",
    "SessionState",
    "agent_start_event",
    "agent_end_event",
    "agent_error_event",
    "message_start_event",
    "message_update_event",
    "message_end_event",
    "tool_call_start_event",
    "tool_call_update_event",
    "tool_call_end_event",
    "session_created_event",
    "session_compacted_event",
    "state_changed_event",
    "get_event_bus",
    "reset_event_bus",
]
