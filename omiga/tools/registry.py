"""Tool registry for registering and retrieving tools."""
from __future__ import annotations

import logging
from typing import Any, Dict, List, Optional, Type

from omiga.tools.base import Tool, ToolContext, ToolResult

logger = logging.getLogger("omiga.tools.registry")


class ToolRegistry:
    """Registry for tools.

    The ToolRegistry manages tool registration and lookup.
    Tools are registered by name and can be retrieved for execution.
    """

    def __init__(self, context: Optional[ToolContext] = None):
        """Initialize the registry.

        Args:
            context: Optional tool context
        """
        self.context = context
        self._tools: Dict[str, Tool] = {}
        self._tool_classes: Dict[str, Type[Tool]] = {}

    def set_context(self, context: ToolContext) -> None:
        """Set the tool context.

        Args:
            context: The tool context
        """
        self.context = context

    def register(self, tool: Tool) -> None:
        """Register a tool instance.

        Args:
            tool: The tool instance to register
        """
        self._tools[tool.name] = tool
        logger.debug(f"Registered tool: {tool.name}")

    def register_class(self, tool_class: Type[Tool]) -> None:
        """Register a tool class for lazy instantiation.

        Args:
            tool_class: The tool class to register
        """
        if not hasattr(tool_class, "name"):
            raise ValueError("Tool class must have 'name' attribute")
        self._tool_classes[tool_class.name] = tool_class
        logger.debug(f"Registered tool class: {tool_class.name}")

    def get_tool(self, name: str) -> Optional[Tool]:
        """Get a tool by name.

        Args:
            name: The tool name

        Returns:
            The tool instance or None
        """
        if name in self._tools:
            return self._tools[name]

        # Lazy instantiation
        if name in self._tool_classes:
            tool_class = self._tool_classes[name]
            if self.context:
                tool = tool_class(self.context)
                self._tools[name] = tool
                return tool
            logger.warning(f"Cannot instantiate tool '{name}' without context")

        return None

    async def execute_tool(self, name: str, **kwargs: Any) -> ToolResult:
        """Execute a tool by name.

        Args:
            name: The tool name
            **kwargs: Arguments to pass to the tool

        Returns:
            ToolResult from the tool execution

        Raises:
            ValueError: If the tool is not found
        """
        tool = self.get_tool(name)
        if not tool:
            raise ValueError(f"Tool not found: {name}")

        try:
            return await tool.execute(**kwargs)
        except Exception as e:
            return ToolResult.fail(f"Tool execution failed: {e}")

    def list_tools(self) -> List[str]:
        """List all registered tool names.

        Returns:
            List of tool names
        """
        return list(self._tools.keys()) + list(self._tool_classes.keys())

    def get_schema(self, name: str) -> Optional[Dict[str, Any]]:
        """Get the JSON schema for a tool.

        Args:
            name: The tool name

        Returns:
            The tool's JSON schema or None
        """
        tool = self.get_tool(name)
        if tool:
            return tool.schema()
        return None

    def get_all_schemas(self) -> List[Dict[str, Any]]:
        """Get schemas for all registered tools.

        Returns:
            List of tool schemas
        """
        schemas = []
        for name in self.list_tools():
            schema = self.get_schema(name)
            if schema:
                schemas.append(schema)
        return schemas
