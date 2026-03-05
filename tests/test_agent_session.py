"""Tests for AgentSession."""
import pytest
from omiga.agent import (
    AgentSession,
    SessionState,
    Message,
    ToolCallRecord,
    LLMResponse,
)
from omiga.tools.base import ToolResult, ToolContext, Tool
from omiga.tools.registry import ToolRegistry


class MockTool(Tool):
    """Mock tool for testing."""

    name = "mock_tool"
    description = "A mock tool"

    def __init__(self, context: ToolContext):
        super().__init__(context)
        self.call_count = 0

    async def execute(self, **kwargs) -> ToolResult:
        self.call_count += 1
        return ToolResult.ok(data=f"Mock executed with {kwargs}")


class TestMessage:
    """Test Message class."""

    def test_user_message(self):
        msg = Message.user_message("Hello")
        assert msg.role == "user"
        assert msg.content == "Hello"

    def test_assistant_message(self):
        msg = Message.assistant_message("Hi there")
        assert msg.role == "assistant"
        assert msg.content == "Hi there"

    def test_system_message(self):
        msg = Message.system_message("System instruction")
        assert msg.role == "system"
        assert msg.content == "System instruction"

    def test_tool_message(self):
        result = ToolResult.ok(data="Tool result")
        msg = Message.tool_message(result, "call_123")
        assert msg.role == "tool"
        assert msg.content == "Tool result"
        assert msg.tool_call_id == "call_123"

    def test_to_dict(self):
        msg = Message.user_message("Test")
        d = msg.to_dict()
        assert d == {"role": "user", "content": "Test"}

    def test_to_dict_with_tool_calls(self):
        msg = Message.assistant_message("Test")
        msg.tool_calls = [{"id": "call_1", "function": {"name": "test"}}]
        d = msg.to_dict()
        assert d["role"] == "assistant"
        assert d["content"] == "Test"
        assert d["tool_calls"] == msg.tool_calls


class TestToolCallRecord:
    """Test ToolCallRecord class."""

    def test_record_creation(self):
        record = ToolCallRecord(tool_name="test", args={"key": "value"})
        assert record.tool_name == "test"
        assert record.args == {"key": "value"}
        assert record.success is False
        assert record.result is None


class TestAgentSession:
    """Test AgentSession class."""

    @pytest.fixture
    def session(self):
        """Create a test session."""
        return AgentSession(group_folder="test_group")

    def test_init(self, session):
        """Test session initialization."""
        assert session.group_folder == "test_group"
        assert session.state == SessionState.IDLE
        assert session.step_count == 0
        assert session.max_steps == 20

    def test_add_message(self, session):
        """Test adding messages."""
        msg = Message.user_message("Hello")
        session.add_message(msg)
        assert len(session.memory.messages) == 1
        assert session.memory.messages[0].content == "Hello"

    def test_record_tool_call(self, session):
        """Test recording tool calls."""
        record = ToolCallRecord(tool_name="test", args={})
        session.record_tool_call(record)
        assert len(session.tool_calls) == 1

    def test_is_finished(self, session):
        """Test finished state check."""
        assert session.is_finished() is False

        session.state = SessionState.FINISHED
        assert session.is_finished() is True

        session.state = SessionState.ERROR
        assert session.is_finished() is True

    def test_is_stuck_no_duplicates(self, session):
        """Test stuck detection with no duplicates."""
        session.add_message(Message.assistant_message("Response 1"))
        session.add_message(Message.assistant_message("Response 2"))
        assert session.is_stuck() is False

    def test_is_stuck_with_duplicates(self, session):
        """Test stuck detection with duplicate responses."""
        session.add_message(Message.assistant_message("Same response"))
        session.add_message(Message.assistant_message("Same response"))
        session.add_message(Message.assistant_message("Same response"))
        assert session.is_stuck() is True

    def test_is_stuck_threshold(self, session):
        """Test stuck detection with custom threshold."""
        # Need 3 messages: 2 duplicates + 1 last one as baseline
        session.add_message(Message.assistant_message("Same"))
        session.add_message(Message.assistant_message("Same"))
        session.add_message(Message.assistant_message("Same"))
        assert session.is_stuck(threshold=2) is True

    def test_clear(self, session):
        """Test session clear."""
        session.add_message(Message.user_message("Test"))
        session.add_message(Message.assistant_message("Response"))
        session.record_tool_call(ToolCallRecord(tool_name="test", args={}))
        session.step_count = 5

        session.clear()

        assert session.state == SessionState.IDLE
        assert len(session.memory.messages) == 0
        assert len(session.tool_calls) == 0
        assert session.step_count == 0

    def test_get_summary(self, session):
        """Test session summary."""
        session.add_message(Message.user_message("Test"))
        session.record_tool_call(ToolCallRecord(tool_name="test", args={}))
        session.step_count = 3

        summary = session.get_summary()
        assert summary["group_folder"] == "test_group"
        assert summary["message_count"] == 1
        assert summary["tool_call_count"] == 1
        assert summary["step_count"] == 3


class TestAgentSessionWithRegistry:
    """Test AgentSession with tool registry."""

    @pytest.fixture
    def registry(self):
        """Create a mock tool registry."""
        ctx = ToolContext(working_dir="/tmp", data_dir="/tmp/data")
        registry = ToolRegistry(ctx)
        registry.register(MockTool(ctx))
        return registry

    @pytest.fixture
    def session(self, registry):
        """Create session with tool registry."""
        return AgentSession(
            group_folder="test_group", tool_registry=registry
        )

    @pytest.mark.asyncio
    async def test_think_placeholder(self, session):
        """Test think method (placeholder)."""
        response = await session.think()
        assert isinstance(response, LLMResponse)
        assert session.state == SessionState.IDLE

    @pytest.mark.asyncio
    async def test_act_no_tool_calls(self, session):
        """Test act with no tool calls."""
        results = await session.act([])
        assert results == []

    @pytest.mark.asyncio
    async def test_act_with_tool_call(self, session):
        """Test act with tool calls."""
        tool_calls = [
            {"function": {"name": "mock_tool", "arguments": '{"key": "value"}'}}
        ]

        results = await session.act(tool_calls)

        assert len(results) == 1
        assert results[0].success is True
        assert len(session.tool_calls) == 1
        assert session.tool_calls[0].tool_name == "mock_tool"

    @pytest.mark.asyncio
    async def test_run_max_steps(self, session):
        """Test run with max steps (think returns empty tool calls)."""
        # Since think returns placeholder with no tool_calls,
        # run should return immediately with final response
        result = await session.run("Test prompt")
        assert result == "Thinking..."
        assert session.state == SessionState.FINISHED
