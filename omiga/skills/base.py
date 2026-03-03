"""Base classes for Omiga skills."""
from __future__ import annotations

import logging
import os
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional, TYPE_CHECKING

logger = logging.getLogger(__name__)

if TYPE_CHECKING:
    from omiga.memory.manager import MemoryManager


@dataclass
class ToolCallRecord:
    """记录单次工具调用的详细信息。"""
    tool_name: str
    args: dict[str, Any]
    result: Any
    success: bool = True
    error: Optional[str] = None
    duration_ms: Optional[int] = None


@dataclass
class SkillMetadata:
    """Metadata describing a skill."""

    name: str
    description: str
    version: str = "1.0.0"
    author: str = ""
    emoji: str = "🔧"
    tags: list[str] = field(default_factory=list)
    dependencies: list[str] = field(default_factory=list)


@dataclass
class SkillContext:
    """Context provided to skills for interacting with Omiga.

    Attributes:
        groups_dir: Path to the groups directory
        data_dir: Path to the data directory
        send_message: Async callback to send messages to a channel
        get_registered_groups: Get all registered groups
        memory_manager: Optional memory manager for SOP generation
        tool_calls: List of tool call records for SOP generation
        execution_log: List of log entries for SOP generation
    """

    groups_dir: Path
    data_dir: Path
    send_message: Any = None  # Callable[[str, str], Awaitable[None]]
    get_registered_groups: Any = None  # Callable[[], dict[str, Any]]
    memory_manager: Optional["MemoryManager"] = None
    # 新增：执行 trace 记录
    tool_calls: list[ToolCallRecord] = field(default_factory=list)
    execution_log: list[str] = field(default_factory=list)

    def record_tool_call(
        self,
        tool_name: str,
        args: dict[str, Any],
        result: Any,
        success: bool = True,
        error: Optional[str] = None,
        duration_ms: Optional[int] = None,
    ) -> None:
        """记录工具调用（用于 SOP 生成）。"""
        self.tool_calls.append(ToolCallRecord(
            tool_name=tool_name,
            args=args,
            result=result,
            success=success,
            error=error,
            duration_ms=duration_ms,
        ))

    def log(self, message: str) -> None:
        """记录执行日志（用于 SOP 生成）。"""
        self.execution_log.append(message)

    def get_state_before(self) -> dict:
        """捕获执行前状态。"""
        return {
            "cwd": os.getcwd(),
            "env_keys": list(os.environ.keys()),
        }

    def get_state_after(self, state_before: dict) -> dict:
        """捕获执行后状态。"""
        before_keys = set(state_before.get("env_keys", []))
        after_keys = set(os.environ.keys())
        return {
            "cwd": os.getcwd(),
            "env_keys": list(os.environ.keys()),
            "new_env_vars": list(after_keys - before_keys),
        }


class Skill(ABC):
    """Base class for all Omiga skills.

    Skills are the building blocks that extend Omiga's capabilities.
    Each skill should:
    - Have a unique name
    - Provide a clear description
    - Implement execute() method
    - Handle errors gracefully

    The execute_with_trace() method wraps execute() to automatically
    record tool calls and execution logs for SOP generation.
    """

    metadata: SkillMetadata

    def __init__(self, context: SkillContext):
        """Initialize the skill with context.

        Args:
            context: The skill context for interacting with Omiga
        """
        self.context = context
        self.logger = logging.getLogger(f"omiga.skills.{self.__class__.__name__}")
        self._state_before: Optional[dict] = None

    @property
    def name(self) -> str:
        """Return the skill name."""
        return self.metadata.name

    @property
    def description(self) -> str:
        """Return the skill description."""
        return self.metadata.description

    async def execute_with_trace(self, **kwargs: Any) -> Any:
        """Execute the skill with automatic trace recording.

        This method wraps execute() to:
        1. Call before_execute() hook
        2. Capture state before execution
        3. Execute the skill
        4. Record tool call (success or failure)
        5. Capture state after execution
        6. Call after_execute() hook

        Args:
            **kwargs: Skill-specific arguments

        Returns:
            The result of the skill execution
        """
        import time
        start_time = time.time()

        # 执行前钩子
        await self.before_execute(**kwargs)

        # 捕获执行前状态
        self._state_before = self.context.get_state_before()
        self.context.log(f"Starting skill: {self.name}")

        try:
            # 执行技能
            result = await self.execute(**kwargs)

            # 计算执行时长
            duration_ms = int((time.time() - start_time) * 1000)

            # 记录成功的工具调用
            self.context.record_tool_call(
                tool_name=self.name,
                args=kwargs,
                result=result,
                success=True,
                duration_ms=duration_ms,
            )
            self.context.log(f"Skill completed successfully in {duration_ms}ms")

            # 捕获执行后状态
            state_after = self.context.get_state_after(self._state_before)
            if state_after.get("new_env_vars"):
                self.context.log(f"New env vars: {state_after['new_env_vars']}")

            return result

        except Exception as e:
            # 计算执行时长
            duration_ms = int((time.time() - start_time) * 1000)

            # 记录失败的工具调用
            self.context.record_tool_call(
                tool_name=self.name,
                args=kwargs,
                result=None,
                success=False,
                error=str(e),
                duration_ms=duration_ms,
            )
            self.context.log(f"Skill failed: {e}")

            # 重新抛出异常
            raise

        finally:
            # 执行后钩子
            await self.after_execute()

    @abstractmethod
    async def execute(self, **kwargs: Any) -> Any:
        """Execute the skill.

        Args:
            **kwargs: Skill-specific arguments

        Returns:
            The result of the skill execution

        Raises:
            SkillError: If the skill fails to execute
        """
        pass

    async def before_execute(self, **kwargs: Any) -> None:
        """Hook called before skill execution.

        Override this method for pre-execution logic.

        Args:
            **kwargs: Skill-specific arguments
        """
        pass

    async def after_execute(self) -> None:
        """Hook called after skill execution.

        Override this method for post-execution logic.
        """
        pass

    async def on_load(self) -> None:
        """Called when the skill is loaded.

        Override this method for initialization logic.
        """
        pass

    async def on_unload(self) -> None:
        """Called when the skill is unloaded.

        Override this method for cleanup logic.
        """
        pass

    def get_usage(self) -> str:
        """Return usage instructions for this skill.

        Returns:
            A string with usage instructions
        """
        return f"Skill: {self.name}\nDescription: {self.description}"


class SkillError(Exception):
    """Exception raised by skills."""

    def __init__(self, message: str, skill_name: Optional[str] = None):
        self.message = message
        self.skill_name = skill_name
        super().__init__(self.message)

    def __str__(self) -> str:
        if self.skill_name:
            return f"[{self.skill_name}] {self.message}"
        return self.message
