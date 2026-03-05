"""Tests for agent events."""
import pytest
from omiga.events import (
    AgentEvent,
    AgentEventType,
    AgentEventBus,
    get_event_bus,
    reset_event_bus,
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
)


class TestAgentEventType:
    """Test AgentEventType enum."""

    def test_event_types(self):
        """Test all event types exist."""
        assert AgentEventType.AGENT_START == "agent_start"
        assert AgentEventType.AGENT_END == "agent_end"
        assert AgentEventType.AGENT_ERROR == "agent_error"
        assert AgentEventType.MESSAGE_START == "message_start"
        assert AgentEventType.MESSAGE_UPDATE == "message_update"
        assert AgentEventType.MESSAGE_END == "message_end"
        assert AgentEventType.TOOL_CALL_START == "tool_call_start"
        assert AgentEventType.TOOL_CALL_UPDATE == "tool_call_update"
        assert AgentEventType.TOOL_CALL_END == "tool_call_end"
        assert AgentEventType.SESSION_CREATED == "session_created"
        assert AgentEventType.SESSION_COMPACTED == "session_compacted"
        assert AgentEventType.STATE_CHANGED == "state_changed"


class TestAgentEvent:
    """Test AgentEvent dataclass."""

    def test_create_event(self):
        """Test creating an event."""
        event = AgentEvent(
            type=AgentEventType.AGENT_START,
            session_id="test123",
            data={"prompt": "Hello"},
        )
        assert event.type == AgentEventType.AGENT_START
        assert event.session_id == "test123"
        assert event.data["prompt"] == "Hello"

    def test_event_timestamp(self):
        """Test event has timestamp."""
        event = AgentEvent(type=AgentEventType.AGENT_START)
        assert event.timestamp is not None
        assert len(event.timestamp) > 0

    def test_to_dict(self):
        """Test event to_dict."""
        event = AgentEvent(
            type=AgentEventType.AGENT_END,
            session_id="test123",
            data={"result": "Done"},
        )
        d = event.to_dict()
        assert d["type"] == "agent_end"
        assert d["session_id"] == "test123"
        assert d["data"]["result"] == "Done"


class TestEventBuilders:
    """Test event builder functions."""

    def test_agent_start_event(self):
        event = agent_start_event("session1", "Test prompt")
        assert event.type == AgentEventType.AGENT_START
        assert event.session_id == "session1"
        assert event.data["prompt"] == "Test prompt"

    def test_agent_end_event(self):
        event = agent_end_event("session1", "Result", 5, 3)
        assert event.type == AgentEventType.AGENT_END
        assert event.data["result"] == "Result"
        assert event.data["steps"] == 5
        assert event.data["tool_calls"] == 3

    def test_agent_error_event(self):
        event = agent_error_event("session1", "Error message", "TypeError")
        assert event.type == AgentEventType.AGENT_ERROR
        assert event.data["error"] == "Error message"
        assert event.data["error_type"] == "TypeError"

    def test_message_start_event(self):
        event = message_start_event("session1", {"role": "user", "content": "Hi"})
        assert event.type == AgentEventType.MESSAGE_START
        assert event.data["message"]["role"] == "user"

    def test_message_update_event(self):
        event = message_update_event("session1", "Hello ")
        assert event.type == AgentEventType.MESSAGE_UPDATE
        assert event.data["delta"] == "Hello "

    def test_message_end_event(self):
        event = message_end_event("session1", {"role": "assistant", "content": "Done"})
        assert event.type == AgentEventType.MESSAGE_END
        assert event.data["message"]["role"] == "assistant"

    def test_tool_call_start_event(self):
        event = tool_call_start_event(
            "session1", "read_file", {"path": "test.txt"}, "call_123"
        )
        assert event.type == AgentEventType.TOOL_CALL_START
        assert event.data["tool_name"] == "read_file"
        assert event.data["tool_call_id"] == "call_123"

    def test_tool_call_update_event(self):
        event = tool_call_update_event("session1", "partial result", "call_123")
        assert event.type == AgentEventType.TOOL_CALL_UPDATE
        assert event.data["delta"] == "partial result"

    def test_tool_call_end_event(self):
        event = tool_call_end_event(
            "session1", "read_file", {"content": "..."}, True, "call_123"
        )
        assert event.type == AgentEventType.TOOL_CALL_END
        assert event.data["tool_name"] == "read_file"
        assert event.data["success"] is True

    def test_session_created_event(self):
        event = session_created_event("session1", "tg:123456")
        assert event.type == AgentEventType.SESSION_CREATED
        assert event.data["chat_jid"] == "tg:123456"

    def test_session_compacted_event(self):
        event = session_compacted_event("session1", "Summary", 10000, 5000)
        assert event.type == AgentEventType.SESSION_COMPACTED
        assert event.data["summary"] == "Summary"
        assert event.data["tokens_before"] == 10000
        assert event.data["tokens_after"] == 5000

    def test_state_changed_event(self):
        event = state_changed_event("session1", "IDLE", "THINKING")
        assert event.type == AgentEventType.STATE_CHANGED
        assert event.data["old_state"] == "IDLE"
        assert event.data["new_state"] == "THINKING"


class TestAgentEventBus:
    """Test AgentEventBus class."""

    @pytest.fixture
    def bus(self):
        """Create a fresh event bus."""
        reset_event_bus()
        return get_event_bus()

    def test_subscribe_and_publish(self, bus):
        """Test basic subscribe and publish."""
        received = []

        def handler(event):
            received.append(event)

        bus.subscribe(AgentEventType.AGENT_START, handler)
        bus.publish(agent_start_event("session1", "Test"))

        assert len(received) == 1
        assert received[0].type == AgentEventType.AGENT_START

    def test_unsubscribe(self, bus):
        """Test unsubscribe."""
        received = []

        def handler(event):
            received.append(event)

        unsubscribe = bus.subscribe(AgentEventType.AGENT_START, handler)
        bus.publish(agent_start_event("session1", "Test"))
        assert len(received) == 1

        unsubscribe()
        bus.publish(agent_start_event("session2", "Test2"))
        assert len(received) == 1  # Should not receive second event

    def test_multiple_subscribers(self, bus):
        """Test multiple subscribers."""
        received1 = []
        received2 = []

        bus.subscribe(AgentEventType.AGENT_START, lambda e: received1.append(e))
        bus.subscribe(AgentEventType.AGENT_START, lambda e: received2.append(e))

        bus.publish(agent_start_event("session1", "Test"))

        assert len(received1) == 1
        assert len(received2) == 1

    def test_subscriber_error_isolation(self, bus):
        """Test that subscriber errors don't affect others."""
        received = []

        def bad_handler(event):
            raise ValueError("Test error")

        def good_handler(event):
            received.append(event)

        bus.subscribe(AgentEventType.AGENT_START, bad_handler)
        bus.subscribe(AgentEventType.AGENT_START, good_handler)

        bus.publish(agent_start_event("session1", "Test"))

        assert len(received) == 1  # good_handler should still receive

    def test_get_recent_events(self, bus):
        """Test getting recent events."""
        bus.publish(agent_start_event("session1", "Test1"))
        bus.publish(agent_end_event("session1", "Result", 1, 0))
        bus.publish(agent_start_event("session2", "Test2"))

        recent = bus.get_recent_events(limit=2)
        assert len(recent) == 2
        assert recent[0].session_id == "session2"  # Most recent first

    def test_get_recent_events_filtered(self, bus):
        """Test getting recent events with filter."""
        bus.publish(agent_start_event("session1", "Test1"))
        bus.publish(agent_end_event("session1", "Result", 1, 0))
        bus.publish(agent_start_event("session2", "Test2"))

        recent = bus.get_recent_events(event_type=AgentEventType.AGENT_START, limit=10)
        assert len(recent) == 2
        assert all(e.type == AgentEventType.AGENT_START for e in recent)

    def test_get_statistics(self, bus):
        """Test getting event statistics."""
        bus.publish(agent_start_event("session1", "Test1"))
        bus.publish(agent_start_event("session2", "Test2"))
        bus.publish(agent_end_event("session1", "Result", 1, 0))

        stats = bus.get_statistics()
        assert stats["total_events"] == 3
        assert "agent_start" in stats["by_type"]
        assert stats["by_type"]["agent_start"] == 2

    def test_clear_history(self, bus):
        """Test clearing history."""
        bus.publish(agent_start_event("session1", "Test"))
        assert len(bus.get_recent_events()) == 1

        bus.clear_history()
        assert len(bus.get_recent_events()) == 0
        assert bus.get_statistics()["total_events"] == 0

    def test_history_limit(self):
        """Test history limit."""
        bus = AgentEventBus(max_history=5)

        for i in range(10):
            bus.publish(agent_start_event(f"session{i}", f"Test{i}"))

        assert len(bus.get_recent_events()) == 5


class TestGlobalEventBus:
    """Test global event bus singleton."""

    def test_get_event_bus_singleton(self):
        """Test get_event_bus returns singleton."""
        reset_event_bus()
        bus1 = get_event_bus()
        bus2 = get_event_bus()
        assert bus1 is bus2

    def test_reset_event_bus(self):
        """Test reset_event_bus."""
        reset_event_bus()
        bus1 = get_event_bus()
        bus1.publish(agent_start_event("session1", "Test"))

        reset_event_bus()
        bus2 = get_event_bus()
        assert len(bus2.get_recent_events()) == 0
