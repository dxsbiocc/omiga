"""Tests for Agent classes.

This module tests the Agent class hierarchy:
- BaseAgent
- ReActAgent
- ToolCallAgent

Note: Message must be imported first to resolve forward references.
"""
import pytest
from unittest.mock import AsyncMock, MagicMock, patch

# Import Message first to resolve forward references
from omiga.agent.session import Message

from omiga.agent.base import BaseAgent
from omiga.agent.react import ReActAgent
from omiga.agent.toolcall import ToolCallAgent
from omiga.tools.registry import ToolRegistry
from omiga.events import SessionState


class TestBaseAgent:
    """Test BaseAgent abstract class."""

    def test_base_agent_is_abstract(self):
        """Test that BaseAgent cannot be instantiated directly."""
        with pytest.raises(TypeError):
            BaseAgent()

    def test_base_agent_subclass_can_instantiate(self):
        """Test that a concrete subclass can be instantiated."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return False

            async def act(self) -> str:
                return "acted"

        agent = ConcreteAgent()
        assert agent.name == "concrete"
        assert agent.state == SessionState.IDLE

    @pytest.mark.asyncio
    async def test_step_no_action(self):
        """Test step when no action is needed."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return False

            async def act(self) -> str:
                return "acted"

        agent = ConcreteAgent()
        result = await agent.step()
        assert result == "Thinking complete - no action needed"
        assert agent.state == SessionState.IDLE

    @pytest.mark.asyncio
    async def test_step_with_action(self):
        """Test step when action is needed."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return True

            async def act(self) -> str:
                self.state = SessionState.ACTING
                return "action result"

        agent = ConcreteAgent()
        result = await agent.step()
        assert result == "action result"

    @pytest.mark.asyncio
    async def test_run(self):
        """Test run method."""
        from typing import ClassVar

        class ConcreteAgent(BaseAgent):
            name: str = "concrete"
            step_count: int = 0

            async def think(self) -> bool:
                self.step_count += 1
                # Return True to trigger act() on first step
                return self.step_count == 1

            async def act(self) -> str:
                return f"step {self.step_count}"

        agent = ConcreteAgent()
        result = await agent.run("test prompt", max_steps=5)
        # After first step, think() returns False, so run() returns the thinking result
        assert "step" in result or "Thinking complete" in result

    def test_add_user_message(self):
        """Test adding user message."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return False

            async def act(self) -> str:
                return "acted"

        agent = ConcreteAgent()
        agent.add_user_message("Hello")
        assert len(agent.memory.messages) == 1
        assert agent.memory.messages[0].role == "user"

    def test_add_assistant_message(self):
        """Test adding assistant message."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return False

            async def act(self) -> str:
                return "acted"

        agent = ConcreteAgent()
        agent.add_assistant_message("Hi there")
        assert len(agent.memory.messages) == 1
        assert agent.memory.messages[0].role == "assistant"

    def test_clear(self):
        """Test clear method."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return False

            async def act(self) -> str:
                return "acted"

        agent = ConcreteAgent()
        agent.add_user_message("Hello")
        agent.state = SessionState.THINKING
        agent.clear()
        assert len(agent.memory.messages) == 0
        assert agent.state == SessionState.IDLE

    def test_is_finished(self):
        """Test is_finished method."""
        class ConcreteAgent(BaseAgent):
            name: str = "concrete"

            async def think(self) -> bool:
                return False

            async def act(self) -> str:
                return "acted"

        agent = ConcreteAgent()
        assert not agent.is_finished()
        agent.state = SessionState.FINISHED
        assert agent.is_finished()
        agent.state = SessionState.ERROR
        assert agent.is_finished()


class TestReActAgent:
    """Test ReActAgent class."""

    def test_react_agent_is_abstract(self):
        """Test that ReActAgent cannot be instantiated directly."""
        with pytest.raises(TypeError):
            ReActAgent()

    @pytest.mark.asyncio
    async def test_think_sets_state(self):
        """Test that think() sets state to THINKING."""
        class ConcreteReActAgent(ReActAgent):
            name: str = "concrete"

            async def _think_impl(self) -> bool:
                return True

            async def _act_impl(self) -> str:
                return "acted"

        agent = ConcreteReActAgent()
        assert agent.state == SessionState.IDLE
        await agent.think()
        assert agent.state == SessionState.IDLE  # Should be reset after think

    @pytest.mark.asyncio
    async def test_act_sets_state(self):
        """Test that act() sets state to ACTING."""
        class ConcreteReActAgent(ReActAgent):
            name: str = "concrete"

            async def _think_impl(self) -> bool:
                return True

            async def _act_impl(self) -> str:
                return "acted"

        agent = ConcreteReActAgent()
        await agent.act()
        assert agent.state == SessionState.IDLE  # Should be reset after act

    @pytest.mark.asyncio
    async def test_think_impl_called(self):
        """Test that _think_impl is called by think()."""
        think_called = False

        class ConcreteReActAgent(ReActAgent):
            name: str = "concrete"

            async def _think_impl(self) -> bool:
                nonlocal think_called
                think_called = True
                return True

            async def _act_impl(self) -> str:
                return "acted"

        agent = ConcreteReActAgent()
        await agent.think()
        assert think_called

    @pytest.mark.asyncio
    async def test_act_impl_called(self):
        """Test that _act_impl is called by act()."""
        act_called = False

        class ConcreteReActAgent(ReActAgent):
            name: str = "concrete"

            async def _think_impl(self) -> bool:
                return True

            async def _act_impl(self) -> str:
                nonlocal act_called
                act_called = True
                return "acted"

        agent = ConcreteReActAgent()
        await agent.act()
        assert act_called

    def test_get_context_summary(self):
        """Test get_context_summary method."""
        class ConcreteReActAgent(ReActAgent):
            name: str = "concrete"

            async def _think_impl(self) -> bool:
                return False

            async def _act_impl(self) -> str:
                return "acted"

        agent = ConcreteReActAgent()
        agent.add_user_message("Hello")
        agent.memory.set_context("key", "value")
        summary = agent.get_context_summary()
        assert "Messages" in summary
        assert "Context keys" in summary


class TestToolCallAgent:
    """Test ToolCallAgent class."""

    @pytest.fixture
    def tool_call_agent(self):
        """Create a ToolCallAgent instance for testing."""
        return ToolCallAgent()

    def test_init(self, tool_call_agent):
        """Test ToolCallAgent initialization."""
        assert tool_call_agent.name == "toolcall_agent"
        assert tool_call_agent.tool_registry is not None
        assert tool_call_agent.pending_tool_calls == []
        assert tool_call_agent.on_tool_update is None

    def test_init_with_custom_registry(self):
        """Test initialization with custom tool registry."""
        registry = ToolRegistry()
        agent = ToolCallAgent(tool_registry=registry)
        assert agent.tool_registry is registry

    @pytest.mark.asyncio
    async def test_execute_tool_call(self, tool_call_agent):
        """Test executing a tool call."""
        # Create a simple tool using function wrapper
        from omiga.tools.base import Tool, ToolResult, ToolContext

        class EchoTool(Tool):
            name = "echo"
            description = "Echo back the input text"

            async def execute(self, text: str = "", **kwargs) -> ToolResult:
                return ToolResult.ok(data=text)

        # Register tool instance
        tool_call_agent.tool_registry.register(EchoTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))

        call = {"name": "echo", "arguments": {"text": "Hello"}}
        result = await tool_call_agent._execute_tool_call(call)
        assert "Success" in result
        assert "Hello" in result

    @pytest.mark.asyncio
    async def test_execute_tool_call_with_callback(self):
        """Test executing a tool call with callback."""
        from omiga.tools.base import Tool, ToolResult, ToolContext

        callback_called = False

        async def on_update(msg: str):
            nonlocal callback_called
            callback_called = True

        class EchoTool(Tool):
            name = "echo"
            description = "Echo back the input text"

            async def execute(self, text: str = "", **kwargs) -> ToolResult:
                return ToolResult.ok(data=text)

        agent = ToolCallAgent(on_tool_update=on_update)
        agent.tool_registry.register(EchoTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))

        call = {"name": "echo", "arguments": {"text": "Hello"}}
        await agent._execute_tool_call(call)
        assert callback_called

    @pytest.mark.asyncio
    async def test_execute_tool_call_error(self, tool_call_agent):
        """Test executing a tool call that raises an error."""
        from omiga.tools.base import Tool, ToolResult, ToolContext

        class FailingTool(Tool):
            name = "failing_tool"
            description = "A tool that always fails"

            async def execute(self, **kwargs) -> ToolResult:
                raise ValueError("Test error")

        tool_call_agent.tool_registry.register(FailingTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))

        call = {"name": "failing_tool", "arguments": {}}
        result = await tool_call_agent._execute_tool_call(call)
        assert "Error" in result
        assert "failing_tool" in result

    def test_format_tool_result_success(self, tool_call_agent):
        """Test formatting successful tool result."""
        from omiga.tools.base import ToolResult
        result = ToolResult(success=True, data="test data")
        formatted = tool_call_agent._format_tool_result("test_tool", result)
        assert "[test_tool]" in formatted
        assert "Success" in formatted
        assert "test data" in formatted

    def test_format_tool_result_error(self, tool_call_agent):
        """Test formatting error tool result."""
        from omiga.tools.base import ToolResult
        result = ToolResult(success=False, error="test error")
        formatted = tool_call_agent._format_tool_result("test_tool", result)
        assert "[test_tool]" in formatted
        assert "Error" in formatted
        assert "test error" in formatted

    @pytest.mark.asyncio
    async def test_execute_tool_directly(self, tool_call_agent):
        """Test executing a tool directly."""
        from omiga.tools.base import Tool, ToolResult, ToolContext

        class EchoTool(Tool):
            name = "echo"
            description = "Echo back the input text"

            async def execute(self, text: str = "", **kwargs) -> ToolResult:
                return ToolResult.ok(data=text)

        tool_call_agent.tool_registry.register(EchoTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))

        result = await tool_call_agent.execute_tool_directly("echo", text="Hello")
        assert result.success
        assert result.data == "Hello"

    def test_add_tool_result_to_memory(self, tool_call_agent):
        """Test adding tool result to memory."""
        from omiga.tools.base import ToolResult
        result = ToolResult(success=True, data="test data")
        tool_call_agent.add_tool_result_to_memory("test_tool", result)
        assert len(tool_call_agent.memory.messages) == 1
        assert tool_call_agent.memory.messages[0].role == "tool"

    def test_get_available_tools(self, tool_call_agent):
        """Test getting available tools."""
        from omiga.tools.base import Tool, ToolResult, ToolContext

        class EchoTool(Tool):
            name = "echo"
            description = "Echo back the input text"

            async def execute(self, text: str = "", **kwargs) -> ToolResult:
                return ToolResult.ok(data=text)

        tool_call_agent.tool_registry.register(EchoTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))
        tools = tool_call_agent.get_available_tools()
        assert len(tools) >= 1

    def test_register_tool(self, tool_call_agent):
        """Test registering a tool."""
        from omiga.tools.base import Tool, ToolResult, ToolContext

        class EchoTool(Tool):
            name = "echo"
            description = "Echo back the input text"

            async def execute(self, text: str = "", **kwargs) -> ToolResult:
                return ToolResult.ok(data=text)

        tool_call_agent.register_tool(EchoTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))
        tools = tool_call_agent.get_available_tools()
        assert len(tools) >= 1

    def test_clear_clears_pending_tool_calls(self, tool_call_agent):
        """Test that clear() clears pending tool calls."""
        tool_call_agent.pending_tool_calls.append({"name": "test"})
        tool_call_agent.add_user_message("Hello")
        tool_call_agent.clear()
        assert tool_call_agent.pending_tool_calls == []
        assert len(tool_call_agent.memory.messages) == 0

    @pytest.mark.asyncio
    async def test_act_no_pending_tool_calls(self, tool_call_agent):
        """Test act() with no pending tool calls."""
        result = await tool_call_agent._act_impl()
        assert result == "No actions to execute"

    @pytest.mark.asyncio
    async def test_act_with_pending_tool_calls(self, tool_call_agent):
        """Test act() with pending tool calls."""
        from omiga.tools.base import Tool, ToolResult, ToolContext

        class EchoTool(Tool):
            name = "echo"
            description = "Echo back the input text"

            async def execute(self, text: str = "", **kwargs) -> ToolResult:
                return ToolResult.ok(data=text)

        tool_call_agent.tool_registry.register(EchoTool(ToolContext(working_dir="/tmp", data_dir="/tmp")))

        tool_call_agent.pending_tool_calls.append(
            {"name": "echo", "arguments": {"text": "Hello"}}
        )
        result = await tool_call_agent._act_impl()
        assert "Success" in result
        assert tool_call_agent.pending_tool_calls == []

    @pytest.mark.asyncio
    async def test_think_impl_placeholder(self, tool_call_agent):
        """Test _think_impl placeholder implementation."""
        result = await tool_call_agent._think_impl()
        assert result is False
        assert tool_call_agent.pending_tool_calls == []


# Test experts module
class TestExperts:
    """Test expert agents."""

    def test_create_expert_browser(self):
        """Test creating browser expert."""
        from omiga.agent.experts import create_expert, BrowserExpert
        expert = create_expert("browser")
        assert isinstance(expert, BrowserExpert)
        assert expert.headless is True

    def test_create_expert_coding(self):
        """Test creating coding expert."""
        from omiga.agent.experts import create_expert, CodingExpert
        expert = create_expert("coding")
        assert isinstance(expert, CodingExpert)
        assert expert.language == "python"

    def test_create_expert_analysis(self):
        """Test creating analysis expert."""
        from omiga.agent.experts import create_expert, AnalysisExpert
        expert = create_expert("analysis")
        assert isinstance(expert, AnalysisExpert)
        assert expert.visualization_backend == "matplotlib"

    def test_create_expert_invalid_type(self):
        """Test creating expert with invalid type."""
        from omiga.agent.experts import create_expert
        with pytest.raises(ValueError, match="Unknown expert type"):
            create_expert("invalid_type")

    def test_browser_expert_capabilities(self):
        """Test browser expert capabilities."""
        from omiga.agent.experts import BrowserExpert
        expert = BrowserExpert()
        capabilities = expert.get_capabilities()
        assert len(capabilities) > 0
        assert "Navigate to URLs" in capabilities

    def test_coding_expert_capabilities(self):
        """Test coding expert capabilities."""
        from omiga.agent.experts import CodingExpert
        expert = CodingExpert()
        capabilities = expert.get_capabilities()
        assert len(capabilities) > 0
        assert "Generate code from specifications" in capabilities

    def test_analysis_expert_capabilities(self):
        """Test analysis expert capabilities."""
        from omiga.agent.experts import AnalysisExpert
        expert = AnalysisExpert()
        capabilities = expert.get_capabilities()
        assert len(capabilities) > 0
        assert "Load and parse data files" in capabilities
