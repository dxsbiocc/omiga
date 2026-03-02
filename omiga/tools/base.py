"""Base classes for Omiga tools."""
from __future__ import annotations

import logging
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, Optional, Dict, List


@dataclass
class ToolContext:
    """Context provided to tools."""

    working_dir: str
    data_dir: str
    env_vars: Dict[str, str] = field(default_factory=dict)


@dataclass
class ToolResult:
    """Result of a tool execution."""

    success: bool
    data: Any = None
    error: Optional[str] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def ok(cls, data: Any = None, **metadata: Any) -> "ToolResult":
        """Create a success result."""
        return cls(success=True, data=data, metadata=metadata)

    @classmethod
    def fail(cls, error: str, **metadata: Any) -> "ToolResult":
        """Create a failure result."""
        return cls(success=False, error=error, metadata=metadata)


class Tool(ABC):
    """Base class for all Omiga tools.

    Tools are low-level primitives that skills use.
    Each tool should:
    - Have a unique name
    - Be focused on a single responsibility
    - Handle errors gracefully
    - Be composable
    """

    name: str
    description: str

    def __init__(self, context: ToolContext):
        """Initialize the tool.

        Args:
            context: The tool context
        """
        self.context = context
        self.logger = logging.getLogger(f"omiga.tools.{self.name}")

    @abstractmethod
    async def execute(self, **kwargs: Any) -> ToolResult:
        """Execute the tool.

        Args:
            **kwargs: Tool-specific arguments

        Returns:
            ToolResult indicating success or failure
        """
        pass

    def schema(self) -> Dict[str, Any]:
        """Return the tool's JSON schema.

        This is used for LLM function calling.
        Override in subclasses to provide detailed schema.
        """
        return {
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": {},
            },
        }
