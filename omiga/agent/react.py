"""ReAct Agent implementation for Omiga.

This module implements the ReAct (Reasoning + Acting) agent pattern.
"""
from __future__ import annotations

import logging
from abc import ABC, abstractmethod
from typing import Any, Dict, Optional

from omiga.agent.base import BaseAgent
from omiga.events import SessionState

logger = logging.getLogger("omiga.agent.react")


class ReActAgent(BaseAgent, ABC):
    """ReAct Agent abstract base class.

    This class implements the ReAct pattern:
    1. Think: Process current state and decide next action
    2. Act: Execute the decided action
    3. Observe: Process the result and loop back

    Subclasses must implement:
    - _think_impl(): Core thinking logic
    - _act_impl(): Core action logic
    """

    name: str
    description: Optional[str] = None

    async def think(self) -> bool:
        """Process current state and decide next action.

        Returns:
            True if action should be taken, False otherwise
        """
        self.state = SessionState.THINKING
        try:
            return await self._think_impl()
        finally:
            if self.state == SessionState.THINKING:
                self.state = SessionState.IDLE

    @abstractmethod
    async def _think_impl(self) -> bool:
        """Implement thinking logic.

        Returns:
            True if action should be taken, False otherwise
        """
        pass

    async def act(self) -> str:
        """Execute decided actions.

        Returns:
            Result of action execution
        """
        self.state = SessionState.ACTING
        try:
            return await self._act_impl()
        finally:
            if self.state == SessionState.ACTING:
                self.state = SessionState.IDLE

    @abstractmethod
    async def _act_impl(self) -> str:
        """Implement action logic.

        Returns:
            Result of action execution
        """
        pass

    def get_context_summary(self) -> str:
        """Get a summary of current context for debugging.

        Returns:
            Context summary string
        """
        msg_count = len(self.memory.messages)
        context_keys = list(self.memory.working_context.keys())
        return f"Messages: {msg_count}, Context keys: {context_keys}"
