"""Base Agent classes for Omiga.

This module implements the Agent 分层 architecture inspired by OpenManus:

BaseAgent (基础)
    └── ReActAgent (think/act 抽象)
        └── ToolCallAgent (工具调用支持)
            └── ContainerAgent (Omiga 容器隔离特色)
"""
from __future__ import annotations

import logging
from abc import ABC, abstractmethod
from typing import Any, Dict, List, Optional

from pydantic import BaseModel, Field, ConfigDict

from omiga.events import SessionState
from omiga.memory.agent_memory import AgentMemory
from omiga.tools.base import ToolResult
from omiga.tools.registry import ToolRegistry


logger = logging.getLogger("omiga.agent.base")


class BaseAgent(BaseModel, ABC):
    """Base Agent class.

    This is the abstract base class for all Agent types in Omiga.
    It defines the core interface and shared functionality.

    Attributes:
        name: Agent name
        description: Optional agent description
        state: Current agent state
        memory: Agent working memory
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    name: str
    description: Optional[str] = None
    state: SessionState = Field(default=SessionState.IDLE)
    memory: AgentMemory = Field(default_factory=AgentMemory)

    @abstractmethod
    async def think(self) -> bool:
        """Process current state and decide next action.

        Returns:
            True if action should be taken, False otherwise
        """
        pass

    @abstractmethod
    async def act(self) -> str:
        """Execute decided actions.

        Returns:
            Result of action execution
        """
        pass

    async def step(self) -> str:
        """Execute a single think→act step.

        Returns:
            Step result
        """
        should_act = await self.think()
        if not should_act:
            self.state = SessionState.IDLE
            return "Thinking complete - no action needed"

        self.state = SessionState.ACTING
        return await self.act()

    def is_finished(self) -> bool:
        """Check if agent has finished execution."""
        return self.state in (SessionState.FINISHED, SessionState.ERROR)

    def add_user_message(self, content: str) -> None:
        """Add a user message to memory.

        Args:
            content: Message content
        """
        # Import Message class here to avoid circular import
        from omiga.agent.session import Message
        self.memory.add_message(Message.user_message(content))

    def add_assistant_message(self, content: str) -> None:
        """Add an assistant message to memory.

        Args:
            content: Message content
        """
        from omiga.agent.session import Message
        self.memory.add_message(Message.assistant_message(content))

    async def run(self, prompt: str, max_steps: int = 20) -> str:
        """Run the agent loop.

        Args:
            prompt: User prompt to process
            max_steps: Maximum steps before termination

        Returns:
            Final result
        """
        # Add prompt to memory
        self.add_user_message(prompt)

        results = []
        for step_num in range(max_steps):
            if self.is_finished():
                break

            result = await self.step()
            results.append(result)

            # If no action was taken, return thinking result
            if result == "Thinking complete - no action needed":
                self.state = SessionState.FINISHED
                return result

        self.state = SessionState.FINISHED
        return "\n".join(results) if results else "No output"

    def clear(self) -> None:
        """Clear agent state."""
        self.memory.clear()
        self.state = SessionState.IDLE
