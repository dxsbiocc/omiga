"""Exception types for Omiga.

This module provides fine-grained error classification for better
error handling and automatic recovery strategies.
"""
from __future__ import annotations


class OmigaError(Exception):
    """Base error class for Omiga."""

    pass


class SessionCorruptionError(OmigaError):
    """Session history corrupted (orphaned tool messages)."""

    pass


class TokenLimitExceeded(OmigaError):
    """LLM token limit exceeded."""

    def __init__(self, used: int, limit: int):
        self.used = used
        self.limit = limit
        super().__init__(f"Token limit exceeded: {used}/{limit}")


class ToolExecutionError(OmigaError):
    """Tool execution failed."""

    def __init__(self, tool_name: str, error: str):
        self.tool_name = tool_name
        self.error = error
        super().__init__(f"Tool {tool_name} failed: {error}")


class StuckDetectedError(OmigaError):
    """Agent stuck in a loop (repeated responses)."""

    pass


class RateLimitError(OmigaError):
    """LLM API rate limit exceeded."""

    def __init__(self, retry_after: int | None = None):
        self.retry_after = retry_after
        super().__init__(f"Rate limit exceeded" + (f", retry after {retry_after}s" if retry_after else ""))


class OverloadedError(OmigaError):
    """LLMAPI server overloaded."""

    pass


class AuthenticationError(OmigaError):
    """LLM API authentication failed."""

    pass


class ContextWindowOverflow(OmigaError):
    """Context window overflowed."""

    def __init__(self, used: int, limit: int):
        self.used = used
        self.limit = limit
        super().__init__(f"Context window overflowed: {used}/{limit}")


# Retryable error types for automatic retry logic
RETRYABLE_ERRORS: tuple[type, ...] = (
    OverloadedError,
    RateLimitError,
    ContextWindowOverflow,
)
