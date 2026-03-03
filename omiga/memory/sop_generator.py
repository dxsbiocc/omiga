"""SOP Generator - Creates SOPs from task execution results.

This module analyzes successful task executions and extracts
Standard Operating Procedures (SOPs) that can be reused later.
It also extracts lessons from failures.
"""
from __future__ import annotations

import logging
import re
from dataclasses import dataclass
from typing import Any, Optional

from omiga.memory.manager import MemoryManager
from omiga.memory.models import LessonType, SOP, SOPType

logger = logging.getLogger("omiga.memory.sop_generator")


@dataclass
class TaskExecution:
    """Represents a task execution for SOP generation.

    Attributes:
        task_id: Unique task identifier
        skill_name: Name of skill that was executed
        prompt: Original user prompt
        args: Arguments passed to the skill
        result: Execution result
        success: Whether execution succeeded
        error_message: Error message if failed
        duration_ms: Execution duration in milliseconds
        tools_used: List of tools that were called
    """
    task_id: str
    skill_name: str
    prompt: str
    args: dict[str, Any]
    result: Any
    success: bool
    error_message: Optional[str] = None
    duration_ms: Optional[int] = None
    tools_used: list[str] | None = None


@dataclass
class SOPDraft:
    """Draft SOP extracted from task execution.

    Attributes:
        name: Proposed SOP name
        sop_type: Type of SOP
        steps: Extracted execution steps
        prerequisites: Required preconditions
        pitfalls: Identified pitfalls
        confidence: Confidence score (0.0 - 1.0)
    """
    name: str
    sop_type: SOPType
    steps: list[str]
    prerequisites: list[str]
    pitfalls: list[str]
    confidence: float


class SOPGenerator:
    """Generates SOPs from task execution results.

    The SOPGenerator analyzes successful executions to extract
    reusable procedures, and failed executions to extract lessons.

    Usage:
        generator = SOPGenerator(memory_manager)
        await generator.generate(execution)
    """

    def __init__(self, memory_manager: MemoryManager):
        """Initialize the SOP generator.

        Args:
            memory_manager: Memory manager for storing SOPs
        """
        self.memory_manager = memory_manager

    async def generate(self, execution: TaskExecution) -> Optional[SOP]:
        """Generate SOP or lesson from task execution.

        Args:
            execution: Task execution to analyze

        Returns:
            Generated SOP if successful and above confidence threshold,
            None otherwise
        """
        if execution.success:
            return await self._generate_success_sop(execution)
        else:
            await self._generate_failure_lesson(execution)
            return None

    async def _generate_success_sop(
        self,
        execution: TaskExecution,
    ) -> Optional[SOP]:
        """Generate SOP from successful execution.

        Args:
            execution: Successful task execution

        Returns:
            Generated SOP or None if confidence too low
        """
        draft = await self._extract_sop_draft(execution)

        if draft.confidence < 0.5:
            logger.debug(
                f"SOP confidence too low ({draft.confidence}), skipping: {draft.name}"
            )
            return None

        # Create SOP with extracted information
        sop = self.memory_manager.create_sop(
            name=draft.name,
            sop_type=draft.sop_type,
            task_id=execution.task_id,
            steps=draft.steps,
            prerequisites=draft.prerequisites,
            pitfalls=draft.pitfalls,
            metadata={
                "skill_name": execution.skill_name,
                "original_prompt": execution.prompt[:200],
                "duration_ms": execution.duration_ms,
                "tools_used": execution.tools_used or [],
            },
        )

        logger.info(
            f"Generated SOP: {sop.id} - {draft.name} "
            f"(confidence: {draft.confidence:.2f})"
        )
        return sop

    async def _extract_sop_draft(self, execution: TaskExecution) -> SOPDraft:
        """Extract SOP draft from successful execution.

        This method analyzes the execution to identify:
        - Key steps that were taken
        - Prerequisites that were needed
        - Common pitfalls to avoid

        Args:
            execution: Successful task execution

        Returns:
            SOP draft with extracted information
        """
        # Determine SOP type based on skill and result
        sop_type = self._determine_sop_type(execution)

        # Generate name from skill and prompt
        name = self._generate_sop_name(execution)

        # Extract steps from execution
        steps = self._extract_steps(execution)

        # Identify prerequisites
        prerequisites = self._extract_prerequisites(execution)

        # Identify pitfalls from result analysis
        pitfalls = self._extract_pitfalls(execution)

        # Calculate confidence based on:
        # - Execution duration (longer = more complex = higher confidence)
        # - Tools used (more tools = more steps = higher confidence)
        # - Result clarity (clear success = higher confidence)
        confidence = self._calculate_confidence(execution)

        return SOPDraft(
            name=name,
            sop_type=sop_type,
            steps=steps,
            prerequisites=prerequisites,
            pitfalls=pitfalls,
            confidence=confidence,
        )

    def _determine_sop_type(self, execution: TaskExecution) -> SOPType:
        """Determine SOP type based on execution characteristics."""
        skill_name = execution.skill_name.lower()

        # Skill-based classification
        if "file" in skill_name:
            return SOPType.WORKFLOW
        elif "search" in skill_name or "query" in skill_name:
            return SOPType.WORKFLOW
        elif "config" in skill_name or "setup" in skill_name:
            return SOPType.CONFIGURATION

        # Result-based classification
        if execution.tools_used:
            if len(execution.tools_used) > 3:
                return SOPType.WORKFLOW
            return SOPType.TOOL_USAGE

        # Default to skill type
        return SOPType.SKILL

    def _generate_sop_name(self, execution: TaskExecution) -> str:
        """Generate human-readable SOP name."""
        # Extract key intent from prompt
        prompt = execution.prompt.strip()

        # Remove common prefixes
        for prefix in ["请", "帮我", "我要", "我想", "please ", "i want to ", "help me "]:
            if prompt.lower().startswith(prefix):
                prompt = prompt[len(prefix):]

        # Truncate to reasonable length
        max_len = 50
        if len(prompt) > max_len:
            prompt = prompt[:max_len].rsplit(" ", 1)[0] + "..."

        # Format: Skill: ShortPrompt
        return f"{execution.skill_name}: {prompt}"

    def _extract_steps(self, execution: TaskExecution) -> list[str]:
        """Extract execution steps from task result.

        This is a simplified extraction - in a full implementation,
        this would analyze the actual execution trace.
        """
        steps = []

        # Step 1: Skill invocation
        steps.append(f"调用技能 `{execution.skill_name}`")

        # Step 2: Tool calls if available
        if execution.tools_used:
            for i, tool in enumerate(execution.tools_used, 1):
                steps.append(f"使用工具 `{tool}`")

        # Step 3: Success confirmation
        steps.append("确认执行成功")

        return steps

    def _extract_prerequisites(self, execution: TaskExecution) -> list[str]:
        """Extract prerequisites from execution context."""
        prerequisites = []

        # Check skill dependencies
        if execution.skill_name:
            prerequisites.append(f"技能 `{execution.skill_name}` 可用")

        # Check if tools were required
        if execution.tools_used:
            for tool in execution.tools_used:
                prerequisites.append(f"工具 `{tool}` 可用")

        return prerequisites

    def _extract_pitfalls(self, execution: TaskExecution) -> list[str]:
        """Extract pitfalls from execution analysis."""
        pitfalls = []

        # Analyze execution duration
        if execution.duration_ms and execution.duration_ms > 10000:
            pitfalls.append("执行时间较长，可能需要优化或后台运行")

        # Analyze tools used for common issues
        if execution.tools_used:
            if "file_write" in execution.tools_used:
                pitfalls.append("写入文件前确认路径权限和目标目录存在")
            if "http_request" in execution.tools_used:
                pitfalls.append("HTTP 请求可能需要处理超时和重试")

        return pitfalls

    def _calculate_confidence(self, execution: TaskExecution) -> float:
        """Calculate SOP generation confidence score."""
        confidence = 0.5  # Base confidence

        # Duration factor (longer = more confident)
        if execution.duration_ms:
            if execution.duration_ms > 5000:
                confidence += 0.15
            elif execution.duration_ms > 1000:
                confidence += 0.1

        # Tools factor (more tools = more confident)
        if execution.tools_used:
            tool_count = len(execution.tools_used)
            if tool_count > 5:
                confidence += 0.2
            elif tool_count > 2:
                confidence += 0.1
            elif tool_count > 0:
                confidence += 0.05

        # Result clarity factor
        if execution.result is not None:
            confidence += 0.05

        return min(confidence, 1.0)

    async def _generate_failure_lesson(self, execution: TaskExecution) -> None:
        """Generate lesson from failed execution.

        This extracts:
        - Error patterns to recognize
        - Recovery steps for next time
        - Anti-patterns to avoid

        Args:
            execution: Failed task execution
        """
        if not execution.error_message:
            return

        # Extract error pattern
        error_pattern = self._extract_error_pattern(execution.error_message)

        # Record as lesson
        self.memory_manager.record_lesson(
            lesson_type=LessonType.ERROR_PATTERN,
            trigger=error_pattern,
            content=self._generate_lesson_content(execution),
            source_task_id=execution.task_id,
        )

        # Try to find related SOP and add lesson there
        related_sop = self.memory_manager.find_sop_by_task_id(execution.task_id)
        if related_sop:
            self.memory_manager.add_lesson_to_sop(
                sop_id=related_sop.id,
                lesson_type=LessonType.ERROR_PATTERN,
                trigger=error_pattern,
                content=self._generate_lesson_content(execution),
                task_id=execution.task_id,
            )

        logger.info(
            f"Recorded lesson from failure: {error_pattern[:50]}..."
        )

    def _extract_error_pattern(self, error_message: str) -> str:
        """Extract recognizable error pattern from error message.

        This normalizes error messages to identify recurring patterns.
        """
        # Remove variable parts (paths, IDs, timestamps)
        pattern = error_message

        # Normalize paths
        pattern = re.sub(r"/[a-zA-Z0-9_/.-]+", "/<path>", pattern)

        # Normalize IDs and numbers
        pattern = re.sub(r"\b[0-9a-f]{8,}\b", "<id>", pattern, flags=re.IGNORECASE)
        pattern = re.sub(r"\b\d+\b", "<num>", pattern)

        # Normalize timestamps
        pattern = re.sub(
            r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}",
            "<timestamp>",
            pattern,
        )

        # Truncate to key part
        if len(pattern) > 100:
            pattern = pattern[:100] + "..."

        return pattern

    def _generate_lesson_content(self, execution: TaskExecution) -> str:
        """Generate lesson content from failed execution.

        This includes:
        - What went wrong
        - How to recover
        - What to try instead
        """
        lines = [
            f"**技能**: {execution.skill_name}",
            f"**错误**: {execution.error_message[:200]}",
            "",
            "**建议恢复步骤**:",
            "1. 检查错误信息和日志",
            "2. 确认前置条件已满足",
            "3. 尝试替代方案或联系用户",
        ]

        # Add specific advice based on error type
        error_lower = (execution.error_message or "").lower()

        if "permission" in error_lower or "access" in error_lower:
            lines.append("")
            lines.append("**权限问题提示**:")
            lines.append("- 检查文件或目录权限")
            lines.append("- 确认以正确权限运行")

        elif "not found" in error_lower or "不存在" in error_lower:
            lines.append("")
            lines.append("**文件/路径问题提示**:")
            lines.append("- 确认路径拼写正确")
            lines.append("- 检查文件/目录是否存在")
            lines.append("- 使用绝对路径而非相对路径")

        elif "timeout" in error_lower or "超时" in error_lower:
            lines.append("")
            lines.append("**超时问题提示**:")
            lines.append("- 增加超时时间")
            lines.append("- 检查网络连接")
            lines.append("- 考虑分批处理")

        return "\n".join(lines)

    async def generate_manual_sop(
        self,
        name: str,
        steps: list[str],
        sop_type: SOPType = SOPType.WORKFLOW,
        prerequisites: Optional[list[str]] = None,
        pitfalls: Optional[list[str]] = None,
    ) -> SOP:
        """Manually create an SOP (bypasses auto-extraction).

        Args:
            name: SOP name
            steps: Execution steps
            sop_type: Type of SOP
            prerequisites: Required preconditions
            pitfalls: Common pitfalls

        Returns:
            Created SOP
        """
        return self.memory_manager.create_sop(
            name=name,
            sop_type=sop_type,
            task_id="manual",
            steps=steps,
            prerequisites=prerequisites or [],
            pitfalls=pitfalls or [],
            metadata={"source": "manual"},
        )
