"""ToolCall Agent implementation for Omiga.

This module implements an agent with tool call support.
"""
from __future__ import annotations

import json
import logging
from typing import Any, Dict, List, Optional, Callable, Awaitable

from pydantic import Field, ConfigDict

from omiga.agent.react import ReActAgent
from omiga.events import SessionState
from omiga.memory.agent_memory import AgentMemory
from omiga.tools.base import ToolResult
from omiga.tools.registry import ToolRegistry


logger = logging.getLogger("omiga.agent.toolcall")


class ToolCallAgent(ReActAgent):
    """Agent with tool call support.

    This agent can:
    - Decide which tools to call based on current state
    - Execute tool calls in parallel or sequence
    - Process tool results and continue reasoning

    Attributes:
        tool_registry: Registry of available tools
        pending_tool_calls: Tool calls decided but not yet executed
        on_tool_update: Optional callback for tool execution updates
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    name: str = "toolcall_agent"
    description: Optional[str] = "Agent with tool call capabilities"

    tool_registry: ToolRegistry = Field(default_factory=ToolRegistry)
    memory: AgentMemory = Field(default_factory=AgentMemory)

    # Pending tool calls from think phase
    pending_tool_calls: List[Dict[str, Any]] = Field(default_factory=list)

    # Optional callback for streaming updates
    on_tool_update: Optional[Callable[[str], Awaitable[None]]] = None

    async def _think_impl(self) -> bool:
        """Process state and decide tool calls.

        This is a placeholder implementation.
        Subclasses should override to implement custom thinking logic.

        Returns:
            True if tool calls were decided, False otherwise
        """
        # Placeholder: no tool calls by default
        self.pending_tool_calls = []
        return False

    async def _act_impl(self) -> str:
        """Execute pending tool calls.

        Returns:
            Combined results from all tool executions
        """
        if not self.pending_tool_calls:
            return "No actions to execute"

        results = []
        for call in self.pending_tool_calls:
            result = await self._execute_tool_call(call)
            results.append(result)

        self.pending_tool_calls.clear()
        return "\n".join(results)

    async def _execute_tool_call(self, call: Dict[str, Any]) -> str:
        """Execute a single tool call.

        Args:
            call: Tool call dict with 'name' and 'arguments' keys

        Returns:
            Tool execution result as string
        """
        tool_name = call.get("name", "")
        arguments = call.get("arguments", {})

        # Parse arguments if string
        if isinstance(arguments, str):
            try:
                arguments = json.loads(arguments)
            except json.JSONDecodeError:
                logger.warning(f"Failed to parse tool arguments: {arguments}")
                arguments = {}

        # Emit update if callback provided
        if self.on_tool_update:
            await self.on_tool_update(f"Executing tool: {tool_name}")

        # Execute tool
        try:
            result = await self.tool_registry.execute_tool(tool_name, **arguments)
            return self._format_tool_result(tool_name, result)
        except Exception as e:
            logger.error(f"Tool execution error: {e}")
            return f"Error executing {tool_name}: {e}"

    def _format_tool_result(self, tool_name: str, result: ToolResult) -> str:
        """Format tool result for output.

        Args:
            tool_name: Name of the tool
            result: Tool execution result

        Returns:
            Formatted result string
        """
        if result.success:
            return f"[{tool_name}] Success: {result.data}"
        else:
            return f"[{tool_name}] Error: {result.error}"

    async def execute_tool_directly(self, tool_name: str, **kwargs: Any) -> ToolResult:
        """Execute a tool directly (bypass think/act cycle).

        Args:
            tool_name: Name of tool to execute
            **kwargs: Tool arguments

        Returns:
            Tool execution result
        """
        if self.on_tool_update:
            await self.on_tool_update(f"Executing tool: {tool_name}")

        return await self.tool_registry.execute_tool(tool_name, **kwargs)

    def add_tool_result_to_memory(self, tool_name: str, result: ToolResult) -> None:
        """Add tool result to memory.

        Args:
            tool_name: Name of the tool
            result: Tool execution result
        """
        from omiga.agent.session import Message

        content = result.data if result.success else f"Error: {result.error}"
        self.memory.add_message(Message(role="tool", content=content))

    def get_available_tools(self) -> List[Dict[str, Any]]:
        """Get list of available tools with their schemas.

        Returns:
            List of tool schemas
        """
        return self.tool_registry.get_all_schemas()

    def register_tool(self, tool: Any) -> None:
        """Register a tool instance.

        Args:
            tool: Tool instance to register
        """
        self.tool_registry.register(tool)
        logger.info(f"Registered tool: {tool.name}")

    def clear(self) -> None:
        """Clear agent state including pending tool calls."""
        super().clear()
        self.pending_tool_calls.clear()
