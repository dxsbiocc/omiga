"""Agent module for Omiga.

This module provides the core agent functionality:
- AgentSession: Session management with think() → act() loop
- Message: Conversation message dataclass
- ToolCallRecord: Tool call tracking
- Agent runner: Container agent execution
- Exceptions: Agent-related error types
- SessionState: Agent session state enum
- BaseAgent: Abstract base class for all agents
- ReActAgent: ReAct pattern agent
- ToolCallAgent: Agent with tool call support
- Experts: Domain-specific expert agents (BrowserExpert, CodingExpert, AnalysisExpert)
"""

from omiga.events import SessionState

from omiga.agent.session import (
    AgentSession,
    Message,
    ToolCallRecord,
    LLMResponse,
)

from omiga.agent.runner import (
    run_agent,
    effective_trigger,
    is_session_corruption_error,
)

from omiga.agent.exceptions import (
    OmigaError,
    SessionCorruptionError,
    TokenLimitExceeded,
    ToolExecutionError,
    StuckDetectedError,
    RateLimitError,
    OverloadedError,
    AuthenticationError,
    ContextWindowOverflow,
    RETRYABLE_ERRORS,
)

from omiga.agent.base import BaseAgent
from omiga.agent.react import ReActAgent
from omiga.agent.toolcall import ToolCallAgent
from omiga.agent.experts import (
    BrowserExpert,
    CodingExpert,
    AnalysisExpert,
    create_expert,
)

__all__ = [
    # Session
    "AgentSession",
    "Message",
    "ToolCallRecord",
    "LLMResponse",
    "SessionState",
    # Base Agents
    "BaseAgent",
    "ReActAgent",
    "ToolCallAgent",
    # Experts
    "BrowserExpert",
    "CodingExpert",
    "AnalysisExpert",
    "create_expert",
    # Runner
    "run_agent",
    "effective_trigger",
    "is_session_corruption_error",
    # Exceptions
    "OmigaError",
    "SessionCorruptionError",
    "TokenLimitExceeded",
    "ToolExecutionError",
    "StuckDetectedError",
    "RateLimitError",
    "OverloadedError",
    "AuthenticationError",
    "ContextWindowOverflow",
    "RETRYABLE_ERRORS",
]
