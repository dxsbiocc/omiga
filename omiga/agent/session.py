"""Agent Session management for Omiga.

This module provides the core AgentSession class that implements
the think() → act() loop for agent execution.
"""
from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional, Callable, Awaitable

from pydantic import ConfigDict, PrivateAttr

from omiga.agent.exceptions import (
    OmigaError,
    ToolExecutionError,
    StuckDetectedError,
    RETRYABLE_ERRORS,
)
from omiga.tools.base import ToolResult
from omiga.tools.registry import ToolRegistry
from omiga.events import (
    AgentEvent,
    AgentEventType,
    AgentEventBus,
    SessionState,  # Use unified SessionState from events
    get_event_bus,
    agent_start_event,
    agent_end_event,
    agent_error_event,
    message_end_event,
    tool_call_start_event,
    tool_call_end_event,
    state_changed_event,
)
from omiga.memory.models import ToolCallRecord as MemoryToolCallRecord, _utc_now
from omiga.memory.agent_memory import AgentMemory
from omiga.agent.toolcall import ToolCallAgent

logger = logging.getLogger("omiga.agent_session")


# Re-use ToolCallRecord from memory.models with local extension
@dataclass
class ToolCallRecord(MemoryToolCallRecord):
    """Record of a tool call (extends memory ToolCallRecord with timestamp)."""
    result: Optional[ToolResult] = None  # Override with ToolResult type
    success: bool = False  # Different default
    duration_ms: int = 0  # Different default/required
    timestamp: str = field(default_factory=_utc_now)

    @classmethod
    def from_memory_record(cls, record: MemoryToolCallRecord) -> "ToolCallRecord":
        """Create from memory ToolCallRecord."""
        return cls(
            tool_name=record.tool_name,
            args=record.args,
            result=record.result if isinstance(record.result, ToolResult) else None,
            success=record.success,
            error=record.error,
            duration_ms=record.duration_ms or 0,
        )


@dataclass
class Message:
    """A conversation message."""

    role: str  # "user", "assistant", "tool", "system"
    content: str
    tool_call_id: Optional[str] = None
    tool_calls: Optional[List[Dict[str, Any]]] = None
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )

    @classmethod
    def user_message(cls, content: str) -> "Message":
        """Create a user message."""
        return cls(role="user", content=content)

    @classmethod
    def assistant_message(cls, content: str) -> "Message":
        """Create an assistant message."""
        return cls(role="assistant", content=content)

    @classmethod
    def system_message(cls, content: str) -> "Message":
        """Create a system message."""
        return cls(role="system", content=content)

    @classmethod
    def tool_message(cls, result: ToolResult, tool_call_id: str) -> "Message":
        """Create a tool result message."""
        content = result.data if result.success else f"Error: {result.error}"
        return cls(role="tool", content=content, tool_call_id=tool_call_id)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for LLM API."""
        msg: Dict[str, Any] = {"role": self.role, "content": self.content}
        if self.tool_call_id:
            msg["tool_call_id"] = self.tool_call_id
        if self.tool_calls:
            msg["tool_calls"] = self.tool_calls
        return msg


@dataclass
class LLMResponse:
    """Response from LLM."""

    content: str
    tool_calls: List[Dict[str, Any]] = field(default_factory=list)
    stop_reason: Optional[str] = None
    usage: Dict[str, int] = field(default_factory=dict)


class AgentSession(ToolCallAgent):
    """Agent session manager (in-process).

    This class implements the core think() → act() loop for agent execution.
    It extends ToolCallAgent with session management, event bus integration,
    and tool call history tracking.

    Attributes:
        group_folder: Group folder identifier
        tool_calls: History of tool calls (legacy support)
        step_count: Current step counter
        max_steps: Maximum steps before termination
        session_id: Optional session identifier for event tracking
        max_memory_messages: Maximum messages to keep in memory
    """

    model_config = ConfigDict(arbitrary_types_allowed=True)

    # Override defaults from ToolCallAgent
    name: str = "agent_session"
    description: Optional[str] = "Agent session with event bus integration"

    # Session-specific fields (required for Pydantic)
    group_folder: str
    max_steps: int = 20
    session_id: Optional[str] = None
    max_memory_messages: int = 100

    # Tool call history (legacy support for existing code)
    tool_calls: List[ToolCallRecord] = field(default_factory=list)
    step_count: int = 0

    # Private fields (not serialized, using PrivateAttr)
    _event_bus: AgentEventBus = PrivateAttr(default=None)  # type: ignore
    _on_thinking_start: Optional[Callable[[], Awaitable[None]]] = PrivateAttr(default=None)
    _on_thinking_end: Optional[Callable[[LLMResponse], Awaitable[None]]] = PrivateAttr(default=None)
    _on_tool_call_start: Optional[Callable[[ToolCallRecord], Awaitable[None]]] = PrivateAttr(default=None)
    _on_tool_call_end: Optional[Callable[[ToolCallRecord], Awaitable[None]]] = PrivateAttr(default=None)

    @property
    def event_bus(self) -> AgentEventBus:
        """Get event bus instance."""
        if self._event_bus is None:
            self._event_bus = get_event_bus()
        return self._event_bus

    def __init__(
        self,
        group_folder: str,
        tool_registry: Optional[ToolRegistry] = None,
        max_steps: int = 20,
        session_id: Optional[str] = None,
        event_bus: Optional[AgentEventBus] = None,
        max_memory_messages: int = 100,
        **kwargs: Any,
    ):
        """Initialize agent session.

        Args:
            group_folder: Group folder identifier
            tool_registry: Optional tool registry
            max_steps: Maximum steps before termination
            session_id: Optional session identifier for events
            event_bus: Event bus instance (uses global if not provided)
            max_memory_messages: Maximum messages in working memory
            **kwargs: Additional arguments for ToolCallAgent
        """
        # Initialize ToolCallAgent with provided tool_registry
        if tool_registry:
            kwargs["tool_registry"] = tool_registry

        # Set up event bus (using private attribute)
        self._event_bus = event_bus or get_event_bus()

        # Initialize parent class with all required fields
        super().__init__(
            name="agent_session",
            description="Agent session with event bus integration",
            group_folder=group_folder,
            max_steps=max_steps,
            session_id=session_id,
            max_memory_messages=max_memory_messages,
            **kwargs,
        )

        # Configure memory with session_id
        self.memory.session_id = session_id or group_folder

        # Initialize legacy fields
        self.tool_calls = []
        self.step_count = 0

    def _emit(self, event: AgentEvent) -> None:
        """Emit an event to the event bus.

        Args:
            event: Event to emit
        """
        self.event_bus.publish(event)

    def _set_state(self, new_state: SessionState) -> None:
        """Set session state and emit state change event.

        Args:
            new_state: New state to set
        """
        old_state = self.state
        self.state = new_state
        self._emit(state_changed_event(
            session_id=self.session_id or self.group_folder,
            old_state=old_state.value,
            new_state=new_state.value,
        ))

    def record_tool_call(self, record: ToolCallRecord) -> None:
        """Record a tool call (legacy support).

        Args:
            record: Tool call record
        """
        self.tool_calls.append(record)

    # region Event Handlers

    def on_thinking_start(
        self, callback: Callable[[], Awaitable[None]]
    ) -> None:
        """Set callback for thinking start event."""
        self._on_thinking_start = callback

    def on_thinking_end(
        self, callback: Callable[[LLMResponse], Awaitable[None]]
    ) -> None:
        """Set callback for thinking end event."""
        self._on_thinking_end = callback

    def on_tool_call_start(
        self, callback: Callable[[ToolCallRecord], Awaitable[None]]
    ) -> None:
        """Set callback for tool call start event."""
        self._on_tool_call_start = callback

    def on_tool_call_end(
        self, callback: Callable[[ToolCallRecord], Awaitable[None]]
    ) -> None:
        """Set callback for tool call end event."""
        self._on_tool_call_end = callback

    # endregion

    def is_finished(self) -> bool:
        """Check if session is finished."""
        return self.state in (SessionState.FINISHED, SessionState.ERROR)

    def is_stuck(self, threshold: int = 2, lookback: int = 6) -> bool:
        """Detect if agent is stuck in a loop.

        Args:
            threshold: Number of duplicate responses to trigger detection
            lookback: Number of recent messages to check (default: 6)

        Returns:
            True if agent appears to be stuck
        """
        recent_messages = self.memory.get_recent_messages(lookback)
        if len(recent_messages) < 2:
            return False

        # Check only recent assistant messages for efficiency
        assistant_contents = [
            m.content for m in recent_messages if m.role == "assistant" and m.content
        ]
        if len(assistant_contents) < 2:
            return False

        last_content = assistant_contents[-1]
        # Count duplicates (excluding the last one)
        duplicate_count = sum(
            1 for c in assistant_contents[:-1] if c == last_content
        )
        # threshold means: if we see `threshold` or more duplicates, we're stuck
        return duplicate_count >= threshold

    def clear(self) -> None:
        """Clear session state."""
        # Call parent clear to clear memory and reset state
        super().clear()
        # Clear session-specific fields
        self.tool_calls.clear()
        self.step_count = 0
        logger.info(f"Cleared session: {self.group_folder}")

    def add_message(self, message: Message) -> None:
        """Add a message to conversation history.

        Args:
            message: Message to add
        """
        self.memory.add_message(message)
        # Emit message_end event
        self._emit(message_end_event(
            session_id=self.session_id or self.group_folder,
            message=message.to_dict(),
        ))
        logger.debug(
            f"Added {message.role} message: {message.content[:50]}..."
        )

    async def think(self, system_prompt: Optional[str] = None) -> LLMResponse:
        """Think: decide next action using LLM.

        This method overrides ToolCallAgent.think() to integrate with
        the event bus and LLM.

        Args:
            system_prompt: Optional system prompt override

        Returns:
            LLM response with content and/or tool calls

        Raises:
            OmigaError: If thinking fails
        """
        self.state = SessionState.THINKING

        try:
            # Emit thinking start event
            if self._on_thinking_start:
                await self._on_thinking_start()

            # Build messages for LLM using memory abstraction
            messages = self.memory.to_dict_list()

            # TODO: Integrate with actual LLM
            # For now, return a placeholder response
            logger.debug(f"Thinking with {len(messages)} messages...")

            # Placeholder - will be replaced with actual LLM call
            response = LLMResponse(
                content="Thinking...",
                tool_calls=[],
            )

            # Emit thinking end event
            if self._on_thinking_end:
                await self._on_thinking_end(response)

            return response

        except Exception as e:
            self.state = SessionState.ERROR
            logger.error(f"Think failed: {e}")
            raise

        finally:
            if self.state != SessionState.ERROR:
                self.state = SessionState.IDLE

    async def act(
        self, tool_calls: List[Dict[str, Any]]
    ) -> List[ToolResult]:
        """Act: execute tool calls.

        Args:
            tool_calls: List of tool calls from LLM response

        Returns:
            List of tool results

        Raises:
            ToolExecutionError: If tool execution fails
        """
        if not tool_calls:
            return []

        self._set_state(SessionState.ACTING)
        results: List[ToolResult] = []

        for call in tool_calls:
            tool_name = call.get("function", {}).get("name", "unknown")
            args = call.get("function", {}).get("arguments", {})

            # Parse arguments
            try:
                import json

                if isinstance(args, str):
                    args = json.loads(args)
            except json.JSONDecodeError:
                logger.warning(f"Failed to parse tool arguments: {args}")
                args = {}

            record = ToolCallRecord(tool_name=tool_name, args=args)
            session_id = self.session_id or self.group_folder

            try:
                # Emit tool call start event
                self._emit(tool_call_start_event(
                    session_id=session_id,
                    tool_name=tool_name,
                    args=args,
                    tool_call_id=call.get("id"),
                ))
                if self._on_tool_call_start:
                    await self._on_tool_call_start(record)

                # Execute tool
                import time

                start = time.time()
                result = await self.tool_registry.execute_tool(tool_name, **args)
                record.duration_ms = int((time.time() - start) * 1000)

                record.result = result
                record.success = result.success
                if not result.success:
                    record.error = result.error

                results.append(result)
                self.record_tool_call(record)

                # Emit tool call end event
                self._emit(tool_call_end_event(
                    session_id=session_id,
                    tool_name=tool_name,
                    result=result.data if result.success else result.error,
                    success=result.success,
                    tool_call_id=call.get("id"),
                ))
                if self._on_tool_call_end:
                    await self._on_tool_call_end(record)

            except Exception as e:
                record.error = str(e)
                record.success = False
                self.record_tool_call(record)

                # Emit error event
                self._emit(agent_error_event(
                    session_id=session_id,
                    error=str(e),
                    error_type="ToolExecutionError",
                ))

                error_msg = f"Tool {tool_name} execution failed: {e}"
                logger.error(error_msg)
                raise ToolExecutionError(tool_name, str(e))

        self._set_state(SessionState.IDLE)
        return results

    async def run(self, prompt: str, system_prompt: Optional[str] = None) -> str:
        """Run the complete think→act loop.

        Args:
            prompt: User prompt
            system_prompt: Optional system prompt override

        Returns:
            Final response content

        Raises:
            StuckDetectedError: If agent is stuck in a loop
            OmigaError: If execution fails
        """
        session_id = self.session_id or self.group_folder

        # Emit agent start event
        self._emit(agent_start_event(
            session_id=session_id,
            prompt=prompt,
        ))

        try:
            # Add user message
            self.memory.add_message(Message.user_message(prompt))

            while self.step_count < self.max_steps and not self.is_finished():
                # Check for stuck state
                if self.is_stuck():
                    self._set_state(SessionState.ERROR)
                    raise StuckDetectedError("Agent stuck in a loop")

                # Think
                response = await self.think(system_prompt)
                self.memory.add_message(
                    Message(
                        role="assistant",
                        content=response.content,
                        tool_calls=response.tool_calls,
                    )
                )

                # Act (execute tool calls)
                if response.tool_calls:
                    results = await self.act(response.tool_calls)

                    # Add tool results as messages
                    for call, result in zip(response.tool_calls, results):
                        tool_call_id = call.get("id")
                        if tool_call_id:
                            self.memory.add_message(
                                Message.tool_message(result, tool_call_id)
                            )
                else:
                    # No tool calls, return final response
                    self._set_state(SessionState.FINISHED)
                    # Emit agent end event
                    self._emit(agent_end_event(
                        session_id=session_id,
                        result=response.content,
                        steps=self.step_count,
                        tool_calls=len(self.tool_calls),
                    ))
                    return response.content

                self.step_count += 1

            # Max steps reached
            self._set_state(SessionState.FINISHED)
            # Emit agent end event
            self._emit(agent_end_event(
                session_id=session_id,
                result="Max steps reached",
                steps=self.step_count,
                tool_calls=len(self.tool_calls),
            ))
            return "Max steps reached"

        except Exception as e:
            # Emit error event
            self._emit(agent_error_event(
                session_id=session_id,
                error=str(e),
                error_type=type(e).__name__,
            ))
            self._set_state(SessionState.ERROR)
            raise

    def get_summary(self) -> Dict[str, Any]:
        """Get session summary.

        Returns:
            Summary dictionary
        """
        return {
            "group_folder": self.group_folder,
            "state": self.state.value,
            "message_count": len(self.memory.messages),
            "tool_call_count": len(self.tool_calls),
            "step_count": self.step_count,
            "max_steps": self.max_steps,
            "memory_usage": f"{self.memory.get_message_count()}/{self.memory.max_messages}",
        }


# Rebuild model to resolve forward references
AgentSession.model_rebuild()
