"""Data models for Omiga memory system."""
from __future__ import annotations

import hashlib
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any, Optional


def _utc_now() -> str:
    """Get current UTC timestamp in ISO format."""
    return datetime.now(timezone.utc).isoformat()


@dataclass
class ToolCallRecord:
    """记录单次工具调用的详细信息。

    Attributes:
        tool_name: 工具名称
        args: 调用参数
        result: 调用结果
        success: 是否成功
        error: 错误信息（如果失败）
        duration_ms: 执行时长（毫秒）
    """
    tool_name: str
    args: dict[str, Any]
    result: Any
    success: bool = True
    error: Optional[str] = None
    duration_ms: Optional[int] = None


@dataclass
class TaskExecution:
    """任务执行记录，用于 SOP 生成。

    Attributes:
        task_id: 任务 ID
        skill_name: 技能名称
        prompt: 用户提示词
        args: 技能参数
        result: 执行结果
        success: 是否成功
        error_message: 错误信息
        duration_ms: 执行时长
        tools_used: 使用的工具列表
        tool_call_records: 详细的工具调用记录
        execution_log: 完整的执行日志
        state_before: 执行前状态快照
        state_after: 执行后状态快照
    """
    task_id: str
    skill_name: str
    prompt: str
    args: dict[str, Any]
    result: Any
    success: bool
    error_message: Optional[str] = None
    duration_ms: Optional[int] = None
    tools_used: Optional[list[str]] = None
    tool_call_records: list[ToolCallRecord] = field(default_factory=list)
    execution_log: str = ""
    state_before: Optional[dict] = None
    state_after: Optional[dict] = None


def _utc_now() -> str:
    """Get current UTC timestamp in ISO format."""
    return datetime.now(timezone.utc).isoformat()


class SOPStatus(str, Enum):
    """SOP lifecycle status."""
    PENDING = "pending"      # Awaiting review
    ACTIVE = "active"        # Approved and in use
    ARCHIVED = "archived"    # Historical record
    REJECTED = "rejected"    # Reviewed and discarded


class SOPType(str, Enum):
    """Types of SOPs."""
    SKILL = "skill"              # Skill execution pattern
    TOOL_USAGE = "tool_usage"    # Tool-specific usage pattern
    TROUBLESHOOTING = "troubleshooting"  # Error diagnosis and fix
    WORKFLOW = "workflow"        # Multi-step workflow
    CONFIGURATION = "configuration"  # Environment configuration


class LessonType(str, Enum):
    """Types of lessons learned."""
    ERROR_PATTERN = "error_pattern"       # Recognizable error pattern
    RECOVERY_STEP = "recovery_step"       # How to recover from failure
    ANTI_PATTERN = "anti_pattern"         # What NOT to do
    PREREQUISITE = "prerequisite"         # Required precondition
    EDGE_CASE = "edge_case"               # Special handling case


@dataclass
class Lesson:
    """A lesson learned from failure or success.

    Attributes:
        lesson_type: Type of lesson
        trigger: Pattern that triggers this lesson (error message, scenario)
        content: The actual lesson content
        source_task_id: Task ID where this was learned
        created_at: When the lesson was recorded
        times_applied: How many times this lesson prevented errors
    """
    lesson_type: LessonType
    trigger: str
    content: str
    source_task_id: str
    created_at: str = field(default_factory=_utc_now)
    times_applied: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "lesson_type": self.lesson_type.value,
            "trigger": self.trigger,
            "content": self.content,
            "source_task_id": self.source_task_id,
            "created_at": self.created_at,
            "times_applied": self.times_applied,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Lesson:
        return cls(
            lesson_type=LessonType(data["lesson_type"]),
            trigger=data["trigger"],
            content=data["content"],
            source_task_id=data["source_task_id"],
            created_at=data.get("created_at", _utc_now()),
            times_applied=data.get("times_applied", 0),
        )


@dataclass
class SOP:
    """Standard Operating Procedure.

    Attributes:
        id: Unique identifier (auto-generated from name + timestamp)
        name: Human-readable name
        sop_type: Category of SOP
        status: Lifecycle status
        task_id: Source task that generated this SOP
        prerequisites: Required conditions before execution
        steps: Ordered execution steps
        pitfalls: Common mistakes and how to avoid them
        lessons: Related lessons learned from failures
        metadata: Additional metadata (author, version, etc.)
        created_at: Creation timestamp
        updated_at: Last modification timestamp
        executed_count: Number of times this SOP was executed
        last_executed_at: Last execution timestamp
        success_count: Number of successful executions
        failure_count: Number of failed executions
        confidence_score: Auto-calculated confidence (0.0-1.0)
    """
    name: str
    sop_type: SOPType
    task_id: str
    status: SOPStatus = SOPStatus.PENDING
    id: str = ""
    prerequisites: list[str] = field(default_factory=list)
    steps: list[str] = field(default_factory=list)
    pitfalls: list[str] = field(default_factory=list)
    lessons: list[Lesson] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)
    created_at: str = field(default_factory=_utc_now)
    updated_at: str = field(default_factory=_utc_now)
    executed_count: int = 0
    last_executed_at: Optional[str] = None
    success_count: int = 0
    failure_count: int = 0
    confidence_score: float = 0.5  # Base confidence

    def __post_init__(self):
        if not self.id:
            # Generate stable ID from name + creation date
            hash_input = f"{self.name}:{self.created_at}"
            self.id = hashlib.sha256(hash_input.encode()).hexdigest()[:12]

        # Calculate initial confidence from metadata if available
        if "confidence" in self.metadata:
            self.confidence_score = self.metadata["confidence"]

    def calculate_confidence(self) -> float:
        """Calculate confidence score based on execution history.

        Returns:
            Confidence score between 0.0 and 1.0
        """
        if self.executed_count == 0:
            return self.confidence_score

        # Success rate component (50% weight)
        success_rate = self.success_count / self.executed_count
        success_component = success_rate * 0.5

        # Volume component (30% weight) - more executions = more confident
        volume_component = min(self.executed_count / 10, 1.0) * 0.3

        # Recency component (20% weight) - recent success = more confident
        recency_component = 0.2
        if self.last_executed_at:
            try:
                from datetime import datetime, timezone
                last_exec = datetime.fromisoformat(self.last_executed_at)
                days_since = (datetime.now(timezone.utc) - last_exec).days
                # Within 7 days = full points, decay after that
                recency_component = 0.2 * max(0, 1 - days_since / 30)
            except Exception:
                pass

        self.confidence_score = success_component + volume_component + recency_component
        return self.confidence_score

    def can_auto_approve(self) -> bool:
        """Check if SOP meets auto-approval criteria.

        Criteria:
        - Confidence score > 0.7
        - At least 3 successful executions
        - Success rate > 80%
        - No failures in last 5 executions

        Returns:
            True if SOP should be auto-approved
        """
        if self.executed_count < 3:
            return False
        if self.success_count < 3:
            return False

        success_rate = self.success_count / self.executed_count
        if success_rate < 0.8:
            return False

        # Check recent executions (from metadata)
        recent_failures = self.metadata.get("recent_failures", 0)
        if recent_failures > 0:
            return False

        return self.calculate_confidence() > 0.7

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "name": self.name,
            "sop_type": self.sop_type.value,
            "status": self.status.value,
            "task_id": self.task_id,
            "prerequisites": self.prerequisites,
            "steps": self.steps,
            "pitfalls": self.pitfalls,
            "lessons": [l.to_dict() for l in self.lessons],
            "metadata": self.metadata,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "executed_count": self.executed_count,
            "last_executed_at": self.last_executed_at,
            "success_count": self.success_count,
            "failure_count": self.failure_count,
            "confidence_score": self.confidence_score,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SOP:
        lessons = [Lesson.from_dict(l) for l in data.get("lessons", [])]
        return cls(
            id=data.get("id", ""),
            name=data["name"],
            sop_type=SOPType(data["sop_type"]),
            status=SOPStatus(data["status"]),
            task_id=data["task_id"],
            prerequisites=data.get("prerequisites", []),
            steps=data.get("steps", []),
            pitfalls=data.get("pitfalls", []),
            lessons=lessons,
            metadata=data.get("metadata", {}),
            created_at=data.get("created_at", _utc_now()),
            updated_at=data.get("updated_at", _utc_now()),
            executed_count=data.get("executed_count", 0),
            last_executed_at=data.get("last_executed_at"),
            success_count=data.get("success_count", 0),
            failure_count=data.get("failure_count", 0),
            confidence_score=data.get("confidence_score", 0.5),
        )

    def to_markdown(self) -> str:
        """Convert SOP to markdown format for file storage."""
        status_icon = {
            SOPStatus.PENDING: "⏳",
            SOPStatus.ACTIVE: "✅",
            SOPStatus.ARCHIVED: "📦",
            SOPStatus.REJECTED: "❌",
        }.get(self.status, "❓")

        # Calculate success rate for display
        success_rate = (self.success_count / self.executed_count * 100) if self.executed_count > 0 else 0

        lines = [
            f"# SOP: {self.name}",
            "",
            f"**状态**: {self.status.value} {status_icon}",
            f"**ID**: `{self.id}`",
            f"**来源任务**: `{self.task_id}`",
            f"**类型**: {self.sop_type.value}",
            f"**创建时间**: {self.created_at}",
            f"**执行次数**: {self.executed_count}",
            f"**成功/失败**: {self.success_count}/{self.failure_count} (成功率 {success_rate:.0f}%)",
            f"**置信度**: {self.confidence_score:.2f}",
            f"**最后执行**: {self.last_executed_at or '从未'}",
            "",
            "---",
            "",
        ]

        if self.prerequisites:
            lines.append("## 前置条件")
            lines.append("")
            for i, prereq in enumerate(self.prerequisites, 1):
                lines.append(f"{i}. {prereq}")
            lines.append("")

        if self.steps:
            lines.append("## 执行步骤")
            lines.append("")
            for i, step in enumerate(self.steps, 1):
                lines.append(f"{i}. {step}")
            lines.append("")

        if self.pitfalls:
            lines.append("## 避坑指南")
            lines.append("")
            for pitfall in self.pitfalls:
                lines.append(f"- ⚠️ {pitfall}")
            lines.append("")

        if self.lessons:
            lines.append("## 经验教训")
            lines.append("")
            for lesson in self.lessons:
                icon = {
                    LessonType.ERROR_PATTERN: "🚨",
                    LessonType.RECOVERY_STEP: "💡",
                    LessonType.ANTI_PATTERN: "⛔",
                    LessonType.PREREQUISITE: "📋",
                    LessonType.EDGE_CASE: "🔍",
                }.get(lesson.lesson_type, "📌")
                lines.append(f"### {icon} {lesson.lesson_type.value}")
                lines.append(f"**触发条件**: `{lesson.trigger}`")
                lines.append(f"**教训**: {lesson.content}")
                lines.append(f"**已应用**: {lesson.times_applied} 次")
                lines.append("")

        if self.metadata:
            lines.append("## 元数据")
            lines.append("")
            for key, value in self.metadata.items():
                lines.append(f"- **{key}**: {value}")
            lines.append("")

        return "\n".join(lines)

    @classmethod
    def from_markdown(cls, path: Path) -> Optional[SOP]:
        """Parse SOP from markdown file."""
        if not path.exists():
            return None

        content = path.read_text(encoding="utf-8")
        # Simple parsing - can be enhanced
        data = {
            "id": "",
            "name": "",
            "sop_type": "skill",
            "status": "pending",
            "task_id": "",
            "prerequisites": [],
            "steps": [],
            "pitfalls": [],
            "lessons": [],
            "metadata": {},
        }

        current_section = None
        current_list = None

        for line in content.splitlines():
            line = line.strip()

            if line.startswith("# SOP:"):
                data["name"] = line.replace("# SOP:", "").strip()
            elif line.startswith("**ID**"):
                # Extract ID from "**ID**: `abc123`"
                parts = line.split("**")
                if len(parts) >= 3:
                    id_val = parts[2].strip()
                    # Remove leading ":" and whitespace
                    if id_val.startswith(":"):
                        id_val = id_val[1:].strip()
                    # Remove backticks
                    id_val = id_val.strip("`")
                else:
                    id_val = ""
                data["id"] = id_val
            elif line.startswith("**来源任务**"):
                data["task_id"] = line.split("**")[2].strip()
            elif line.startswith("**类型**"):
                # Extract value after ": " e.g., "**类型**: workflow" -> "workflow"
                type_val = line.split("**")[2].strip()
                if type_val.startswith(":"):
                    type_val = type_val[1:].strip()
                data["sop_type"] = type_val
            elif line.startswith("**状态**"):
                # Extract status value, e.g., "**状态**: pending ⏳" -> "pending"
                status_text = line.split("**")[2].strip()
                if status_text.startswith(":"):
                    status_text = status_text[1:].strip()
                # Remove emoji and take first word
                status_text = status_text.split()[0] if status_text else ""
                data["status"] = status_text
            elif line.startswith("## 前置条件"):
                current_section = "prerequisites"
                current_list = data["prerequisites"]
            elif line.startswith("## 执行步骤"):
                current_section = "steps"
                current_list = data["steps"]
            elif line.startswith("## 避坑指南"):
                current_section = "pitfalls"
                current_list = data["pitfalls"]
            elif line.startswith("## "):
                current_section = None
                current_list = None
            elif current_list is not None and line.startswith(("1.", "2.", "3.", "4.", "5.", "6.", "7.", "8.", "9.", "-")):
                item = line.lstrip("0123456789.- ").strip()
                if item:
                    current_list.append(item)

        return cls.from_dict(data)


@dataclass
class MemoryIndex:
    """L1 memory index - navigation for L2/L3.

    Attributes:
        topics: High-frequency scenario keywords -> location mapping
        keywords: Low-frequency scenario keywords for discovery
        rules: Red-line rules and common pitfalls
        max_topics: Maximum number of topic entries (default 30)
    """
    topics: dict[str, str] = field(default_factory=dict)  # keyword -> location
    keywords: list[str] = field(default_factory=list)
    rules: list[str] = field(default_factory=list)
    max_topics: int = 30

    def add_topic(self, keyword: str, location: str) -> bool:
        """Add a topic mapping. Returns False if at capacity."""
        if len(self.topics) >= self.max_topics:
            return False
        self.topics[keyword] = location
        return True

    def add_rule(self, rule: str) -> None:
        """Add a red-line rule or common pitfall."""
        if rule not in self.rules:
            self.rules.append(rule)

    def to_markdown(self) -> str:
        """Convert index to markdown format."""
        lines = [
            "# 记忆索引 (Memory Index)",
            "",
            f"> 更新时间：{_utc_now()}",
            f"> 条目数：{len(self.topics)}/{self.max_topics}",
            "",
            "---",
            "",
            "## 高频场景索引",
            "",
        ]

        for keyword, location in sorted(self.topics.items()):
            lines.append(f"- **{keyword}** → `{location}`")

        lines.append("")
        lines.append("## 低频场景关键词")
        lines.append("")
        lines.append(", ".join(self.keywords) if self.keywords else "*(无)*")

        lines.append("")
        lines.append("## RULES - 红线规则与避坑指南")
        lines.append("")
        for rule in self.rules:
            lines.append(f"- {rule}")

        return "\n".join(lines)

    @classmethod
    def from_markdown(cls, content: str) -> MemoryIndex:
        """Parse index from markdown content."""
        index = cls()
        current_section = None

        for line in content.splitlines():
            line = line.strip()

            if line.startswith("## 高频场景索引"):
                current_section = "topics"
            elif line.startswith("## 低频场景关键词"):
                current_section = "keywords"
            elif line.startswith("## RULES"):
                current_section = "rules"
            elif current_section == "topics" and line.startswith("- **"):
                parts = line.replace("- **", "").split("**")
                if len(parts) >= 2:
                    keyword = parts[0]
                    location = parts[1].strip("`").strip() if "`" in line else ""
                    index.topics[keyword] = location
            elif current_section == "keywords" and not line.startswith("#"):
                keywords = [k.strip() for k in line.split(",") if k.strip()]
                index.keywords.extend(keywords)
            elif current_section == "rules" and line.startswith("- "):
                rule = line[2:].strip()
                if rule:
                    index.rules.append(rule)

        return index


@dataclass
class FactEntry:
    """A single fact entry for L2 facts database.

    Attributes:
        section: Section name (e.g., "paths", "credentials", "config")
        key: Fact key within section
        value: Fact value
        verified: Whether this fact was action-verified
        source: Source of verification (task ID or tool result)
    """
    section: str
    key: str
    value: str
    verified: bool = False
    source: str = ""
    created_at: str = field(default_factory=_utc_now)

    def to_dict(self) -> dict[str, Any]:
        return {
            "section": self.section,
            "key": self.key,
            "value": self.value,
            "verified": self.verified,
            "source": self.source,
            "created_at": self.created_at,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> FactEntry:
        return cls(
            section=data["section"],
            key=data["key"],
            value=data["value"],
            verified=data.get("verified", False),
            source=data.get("source", ""),
            created_at=data.get("created_at", _utc_now()),
        )


@dataclass
class FactsDatabase:
    """L2 facts database - global environment facts.

    Attributes:
        entries: All fact entries organized by section
    """
    entries: dict[str, list[FactEntry]] = field(default_factory=dict)

    def add(self, entry: FactEntry) -> None:
        """Add a fact entry."""
        if entry.section not in self.entries:
            self.entries[entry.section] = []
        self.entries[entry.section].append(entry)

    def get(self, section: str, key: str) -> Optional[FactEntry]:
        """Get a fact entry by section and key."""
        entries = self.entries.get(section, [])
        for entry in entries:
            if entry.key == key:
                return entry
        return None

    def get_section(self, section: str) -> list[FactEntry]:
        """Get all entries in a section."""
        return self.entries.get(section, [])

    def to_markdown(self) -> str:
        """Convert facts to markdown format."""
        lines = [
            "# 全局事实库 (Global Facts)",
            "",
            f"> 更新时间：{_utc_now()}",
            f"> 区域数：{len(self.entries)}",
            "",
            "---",
            "",
        ]

        for section, entries in sorted(self.entries.items()):
            lines.append(f"## {section}")
            lines.append("")
            for entry in entries:
                verified_icon = "✅" if entry.verified else "⏳"
                lines.append(f"### {entry.key} {verified_icon}")
                lines.append(f"```")
                lines.append(entry.value)
                lines.append(f"```")
                if entry.source:
                    lines.append(f"*来源*: `{entry.source}`")
                lines.append("")

        return "\n".join(lines)

    @classmethod
    def from_markdown(cls, content: str) -> FactsDatabase:
        """Parse facts from markdown content."""
        db = cls()
        current_section = None
        current_key = None
        current_value_lines = []

        def save_current_entry():
            nonlocal current_key, current_value_lines
            if current_key and current_value_lines:
                entry = FactEntry(
                    section=current_section or "general",
                    key=current_key,
                    value="\n".join(current_value_lines).strip(),
                    verified="✅" in content,
                )
                db.add(entry)
            current_key = None
            current_value_lines = []

        for line in content.splitlines():
            if line.startswith("## "):
                save_current_entry()
                current_section = line[3:].strip()
            elif line.startswith("### "):
                save_current_entry()
                current_key = line[4:].strip().replace("✅", "").replace("⏳", "").strip()
            elif line.startswith("```"):
                pass  # Skip code fence
            elif current_key:
                current_value_lines.append(line)

        save_current_entry()
        return db
