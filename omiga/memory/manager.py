"""Memory manager for Omiga's three-layer memory architecture.

This module implements the memory management system that enables
controlled growth of SOPs and lessons learned from both success and failure.

Memory Architecture:
    L1: index.md - Navigation index (≤30 entries) + rules
    L2: facts.md - Global environment facts (paths, configs, credentials)
    L3: SOPs - Standard Operating Procedures with lifecycle management
        - pending/: Awaiting review
        - active/: Approved and in use
        - archived/: Historical records
        - lessons/: Lessons learned from failures
"""
from __future__ import annotations

import asyncio
import hashlib
import logging
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

from omiga.memory.models import (
    FactEntry,
    FactsDatabase,
    Lesson,
    LessonType,
    MemoryIndex,
    SOP,
    SOPStatus,
    SOPType,
)

logger = logging.getLogger("omiga.memory.manager")

# Filename patterns
SOP_FILENAME_PATTERN = "SOP_{id}_{name}.md"
LESSON_FILENAME_PATTERN = "{type}_{trigger_hash}.md"

# Confidence thresholds for SOP generation
SOP_MIN_CONFIDENCE = 0.5
CONFIDENCE_BASE = 0.5
CONFIDENCE_LONG_DURATION = 5000  # ms
CONFIDENCE_MEDIUM_DURATION = 1000  # ms
LONG_EXECUTION_THRESHOLD = 10000  # ms


def _utc_now() -> str:
    """Get current UTC timestamp in ISO format."""
    return datetime.now(timezone.utc).isoformat()


def _utc_now_timestamp() -> float:
    """Get current UTC timestamp as float for comparisons."""
    return datetime.now(timezone.utc).timestamp()


def _make_lesson_filename(lesson_type: LessonType, trigger: str) -> str:
    """Generate a stable filename for a lesson using SHA256 hash."""
    trigger_hash = hashlib.sha256(trigger.encode()).hexdigest()[:16]
    return f"{lesson_type.value}_{trigger_hash}.md"


def _make_sop_filename(sop: SOP) -> str:
    """Generate SOP filename from SOP data."""
    safe_name = sop.name.replace(" ", "_")
    return SOP_FILENAME_PATTERN.format(id=sop.id, name=safe_name)


class MemoryManager:
    """Manages Omiga's three-layer memory system.

    The MemoryManager handles:
    - L1 index: Navigation for discovering memories
    - L2 facts: Action-verified environment facts
    - L3 SOPs: Standard Operating Procedures with lifecycle
    - Lessons: Learned from both success and failure

    All writes follow the "Action-Verified Only" principle:
    - No Execution, No Memory
    - Only successful tool results can be written as facts
    - Lessons are extracted from failures
    """

    def __init__(self, memory_dir: Path):
        """Initialize the memory manager.

        Args:
            memory_dir: Base directory for memory storage
        """
        self.memory_dir = memory_dir
        # Internal directories (private by convention)
        self._l1_dir = memory_dir / "L1"
        self._l2_dir = memory_dir / "L2"
        self._l3_dir = memory_dir / "L3"

        # L3 subdirectories
        self._pending_dir = self._l3_dir / "pending"
        self._active_dir = self._l3_dir / "active"
        self._archived_dir = self._l3_dir / "archived"
        self._lessons_dir = self._l3_dir / "lessons"
        self._scripts_dir = self._l3_dir / "scripts"

        # Cached data
        self._index: Optional[MemoryIndex] = None
        self._facts: Optional[FactsDatabase] = None
        self._sops: dict[str, SOP] = {}
        self._lessons: dict[str, dict[str, Any]] = {}  # Lessons cache

        # Async lock for state modifications
        self._lock = asyncio.Lock()

    async def initialize(self) -> None:
        """Initialize memory directories and load existing data."""
        # Create directory structure (async-safe)
        directories = [
            self._l1_dir, self._l2_dir, self._pending_dir,
            self._active_dir, self._archived_dir,
            self._lessons_dir, self._scripts_dir,
        ]
        for directory in directories:
            directory.mkdir(parents=True, exist_ok=True)

        # Create default files if missing
        index_file = self._l1_dir / "index.md"
        if not index_file.exists():
            self._index = MemoryIndex()
            self._index.add_rule("无执行，不记忆 (No Execution, No Memory)")
            self._index.add_rule("严禁在 L1/L2 存储密码、API Key 等敏感信息")
            self._index.add_rule("SOP 默认进入 pending/，需审查后激活")
            index_file.write_text(self._index.to_markdown(), encoding="utf-8")
        else:
            self._index = MemoryIndex.from_markdown(index_file.read_text(encoding="utf-8"))

        facts_file = self._l2_dir / "facts.md"
        if not facts_file.exists():
            self._facts = FactsDatabase()
            facts_file.write_text(self._facts.to_markdown(), encoding="utf-8")
        else:
            self._facts = FactsDatabase.from_markdown(facts_file.read_text(encoding="utf-8"))

        # Load active SOPs and lessons in parallel
        await asyncio.gather(
            self._load_active_sops(),
            self._load_lessons(),
        )

        logger.info(
            "Memory initialized: %d topics, %d sections, %d active SOPs, %d lessons",
            len(self._index.topics) if self._index else 0,
            len(self._facts.entries) if self._facts else 0,
            len(self._sops),
            len(self._lessons),
        )

    async def _load_active_sops(self) -> None:
        """Load all active SOPs into memory in parallel."""
        self._sops.clear()
        sop_files = list(self._active_dir.glob("*.md"))

        async def load_single(sop_file: Path) -> tuple[str, SOP] | None:
            try:
                sop = await asyncio.to_thread(SOP.from_markdown, sop_file)
                if sop:
                    return (sop.id, sop)
            except Exception as e:
                logger.warning(f"Failed to load SOP {sop_file.name}: {e}")
            return None

        results = await asyncio.gather(*[load_single(f) for f in sop_files])
        for result in results:
            if result:
                self._sops[result[0]] = result[1]

    async def _load_lessons(self) -> None:
        """Load all lessons into memory cache for fast lookup."""
        self._lessons.clear()
        lesson_files = list(self._lessons_dir.glob("*.md"))

        async def load_single(lesson_file: Path) -> dict[str, Any] | None:
            try:
                content = await asyncio.to_thread(lesson_file.read_text, encoding="utf-8")
                # Extract trigger for indexing
                import re
                trigger_match = re.search(r"\*\*触发条件\*\*: `([^`]+)`", content)
                if trigger_match:
                    return {
                        "file": lesson_file.name,
                        "trigger": trigger_match.group(1),
                        "content": content,
                    }
            except Exception as e:
                logger.warning(f"Failed to load lesson {lesson_file.name}: {e}")
            return None

        results = await asyncio.gather(*[load_single(f) for f in lesson_files])
        for result in results:
            if result:
                self._lessons[result["file"]] = result

    # -------------------------------------------------------------------------
    # L1 Index Operations
    # -------------------------------------------------------------------------

    def get_index(self) -> MemoryIndex:
        """Get the current L1 index."""
        if self._index is None:
            raise RuntimeError("Memory not initialized")
        return self._index

    def add_index_topic(self, keyword: str, location: str) -> bool:
        """Add a topic to the L1 index.

        Args:
            keyword: High-frequency scenario keyword
            location: L2 section or L3 SOP id

        Returns:
            False if index is at capacity (≤30 entries)
        """
        if self._index is None:
            raise RuntimeError("Memory not initialized")

        if not self._index.add_topic(keyword, location):
            logger.warning(f"Index at capacity, cannot add topic: {keyword}")
            return False

        self._save_index()
        return True

    def add_index_rule(self, rule: str) -> None:
        """Add a red-line rule to L1 index."""
        if self._index is None:
            raise RuntimeError("Memory not initialized")

        self._index.add_rule(rule)
        self._save_index()

    def _save_index(self) -> None:
        """Persist L1 index to disk."""
        index_file = self._l1_dir / "index.md"
        if self._index:
            index_file.write_text(self._index.to_markdown(), encoding="utf-8")

    def _save_facts(self) -> None:
        """Persist L2 facts to disk."""
        facts_file = self._l2_dir / "facts.md"
        if self._facts:
            facts_file.write_text(self._facts.to_markdown(), encoding="utf-8")

    # -------------------------------------------------------------------------
    # L2 Facts Operations
    # -------------------------------------------------------------------------

    def get_facts(self) -> FactsDatabase:
        """Get the L2 facts database."""
        if self._facts is None:
            raise RuntimeError("Memory not initialized")
        return self._facts

    def add_fact(
        self,
        section: str,
        key: str,
        value: str,
        source: str,
        verified: bool = True,
    ) -> None:
        """Add an action-verified fact to L2.

        Args:
            section: Section name (e.g., "paths", "config", "credentials")
            key: Fact key within section
            value: Fact value
            source: Source task ID or tool result reference
            verified: Must be True - only action-verified facts allowed
        """
        if self._facts is None:
            raise RuntimeError("Memory not initialized")

        if not verified:
            logger.warning("Attempted to add unverified fact - rejecting")
            return

        entry = FactEntry(
            section=section,
            key=key,
            value=value,
            verified=verified,
            source=source,
        )
        self._facts.add(entry)
        self._save_facts()

        # Auto-add to L1 index if section is new
        if section not in self._facts.entries or len(self._facts.entries[section]) == 1:
            self.add_index_topic(section, f"L2/{section}")

    # -------------------------------------------------------------------------
    # L3 SOP Operations
    # -------------------------------------------------------------------------

    def create_sop(
        self,
        name: str,
        sop_type: SOPType,
        task_id: str,
        steps: list[str],
        prerequisites: Optional[list[str]] = None,
        pitfalls: Optional[list[str]] = None,
        metadata: Optional[dict[str, Any]] = None,
    ) -> SOP:
        """Create a new SOP (initially in pending status).

        Args:
            name: Human-readable SOP name
            sop_type: Category of SOP
            task_id: Source task that generated this SOP
            steps: Ordered execution steps
            prerequisites: Required conditions before execution
            pitfalls: Common mistakes and how to avoid them
            metadata: Additional metadata

        Returns:
            The created SOP (in pending status)
        """
        sop = SOP(
            name=name,
            sop_type=sop_type,
            task_id=task_id,
            status=SOPStatus.PENDING,
            steps=steps,
            prerequisites=prerequisites or [],
            pitfalls=pitfalls or [],
            metadata=metadata or {},
        )

        # Save to pending directory
        sop_file = self._pending_dir / f"SOP_{sop.id}_{name.replace(' ', '_')}.md"
        sop_file.write_text(sop.to_markdown(), encoding="utf-8")

        logger.info(f"Created pending SOP: {sop.id} - {name}")
        return sop

    def add_lesson_to_sop(
        self,
        sop_id: str,
        lesson_type: LessonType,
        trigger: str,
        content: str,
        task_id: str,
    ) -> bool:
        """Add a lesson to an existing SOP.

        Args:
            sop_id: Target SOP ID
            lesson_type: Type of lesson
            trigger: Pattern that triggers this lesson
            content: The lesson content
            task_id: Source task where this was learned

        Returns:
            False if SOP not found
        """
        if sop_id not in self._sops:
            # Try to find in pending
            sop = self._find_sop_by_id(sop_id)
            if not sop:
                logger.warning(f"SOP not found: {sop_id}")
                return False
        else:
            sop = self._sops[sop_id]

        lesson = Lesson(
            lesson_type=lesson_type,
            trigger=trigger,
            content=content,
            source_task_id=task_id,
        )
        sop.lessons.append(lesson)
        sop.updated_at = _utc_now()

        # Save to appropriate directory
        if sop.status == SOPStatus.PENDING:
            sop_file = self._pending_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"
        elif sop.status == SOPStatus.ACTIVE:
            sop_file = self._active_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"
            self._sops[sop_id] = sop
        else:
            sop_file = self._archived_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"

        sop_file.write_text(sop.to_markdown(), encoding="utf-8")

        # Add to lessons directory for quick lookup
        lesson_file = self._lessons_dir / f"lesson_{sop_id}_{lesson_type.value}.md"
        lesson_file.write_text(
            f"# Lesson: {lesson_type.value}\n\n"
            f"**SOP**: `{sop_id}`\n\n"
            f"**触发条件**: `{trigger}`\n\n"
            f"**教训**: {content}\n",
            encoding="utf-8",
        )

        logger.info(f"Added lesson to SOP {sop_id}: {lesson_type.value}")
        return True

    def _find_sop_by_id(self, sop_id: str) -> Optional[SOP]:
        """Find SOP by ID in pending/active directories."""
        for sop_file in list(self._pending_dir.glob("*.md")) + list(self._active_dir.glob("*.md")):
            try:
                sop = SOP.from_markdown(sop_file)
                if sop and sop.id == sop_id:
                    return sop
            except Exception:
                pass
        return None

    def list_pending_sops(self) -> list[SOP]:
        """List all pending SOPs awaiting review."""
        sops = []
        for sop_file in self._pending_dir.glob("*.md"):
            try:
                sop = SOP.from_markdown(sop_file)
                if sop:
                    sops.append(sop)
            except Exception as e:
                logger.warning(f"Failed to parse SOP {sop_file.name}: {e}")
        return sops

    def list_active_sops(self) -> list[SOP]:
        """List all active SOPs."""
        return list(self._sops.values())

    def approve_sop(self, sop_id: str) -> bool:
        """Approve a pending SOP and move it to active.

        Args:
            sop_id: SOP ID to approve

        Returns:
            False if SOP not found or not pending
        """
        sop = self._find_sop_by_id(sop_id)
        if not sop or sop.status != SOPStatus.PENDING:
            return False

        # Update status
        sop.status = SOPStatus.ACTIVE
        sop.updated_at = _utc_now()

        # Move file from pending to active
        old_file = self._pending_dir / f"SOP_{sop_id}_{sop.name.replace(' ', '_')}.md"
        new_file = self._active_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"

        if old_file.exists():
            shutil.move(str(old_file), str(new_file))

        # Update in-memory cache
        self._sops[sop_id] = sop

        # Add to L1 index
        self.add_index_topic(sop.name, f"L3/{sop_id}")

        logger.info(f"Approved SOP: {sop_id} - {sop.name}")
        return True

    def reject_sop(self, sop_id: str, reason: str = "") -> bool:
        """Reject a pending SOP.

        Args:
            sop_id: SOP ID to reject
            reason: Rejection reason

        Returns:
            False if SOP not found or not pending
        """
        sop = self._find_sop_by_id(sop_id)
        if not sop or sop.status != SOPStatus.PENDING:
            return False

        # Update status
        sop.status = SOPStatus.REJECTED
        sop.metadata["rejection_reason"] = reason
        sop.updated_at = _utc_now()

        # Move file from pending to archived (keep for reference)
        old_file = self._pending_dir / f"SOP_{sop_id}_{sop.name.replace(' ', '_')}.md"
        new_file = self._archived_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}_rejected.md"

        if old_file.exists():
            shutil.move(str(old_file), str(new_file))

        logger.info(f"Rejected SOP: {sop_id} - {sop.name}. Reason: {reason}")
        return True

    def archive_sop(self, sop_id: str) -> bool:
        """Archive an active SOP.

        Args:
            sop_id: SOP ID to archive

        Returns:
            False if SOP not found or not active
        """
        if sop_id not in self._sops:
            return False

        sop = self._sops[sop_id]
        if sop.status != SOPStatus.ACTIVE:
            return False

        # Update status
        sop.status = SOPStatus.ARCHIVED
        sop.updated_at = _utc_now()

        # Move file from active to archived
        old_file = self._active_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"
        new_file = self._archived_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}_archived.md"

        if old_file.exists():
            shutil.move(str(old_file), str(new_file))

        # Remove from in-memory cache
        del self._sops[sop_id]

        logger.info(f"Archived SOP: {sop_id} - {sop.name}")
        return True

    def get_sop(self, sop_id: str) -> Optional[SOP]:
        """Get an SOP by ID."""
        return self._sops.get(sop_id) or self._find_sop_by_id(sop_id)

    def find_sop_by_task_id(self, task_id: str) -> Optional[SOP]:
        """Find SOP by source task ID."""
        for sop in list(self._sops.values()) + self.list_pending_sops():
            if sop.task_id == task_id:
                return sop
        return None

    def record_sop_execution(self, sop_id: str, success: bool) -> None:
        """Record SOP execution for statistics and auto-approval.

        Args:
            sop_id: SOP that was executed
            success: Whether execution succeeded
        """
        if sop_id not in self._sops:
            # Try to find in pending
            sop = self._find_sop_by_id(sop_id)
            if not sop:
                return
        else:
            sop = self._sops[sop_id]

        sop.executed_count += 1
        sop.last_executed_at = _utc_now()
        sop.updated_at = _utc_now()

        if success:
            sop.success_count += 1
        else:
            sop.failure_count += 1
            # Track recent failures for auto-approval
            sop.metadata["recent_failures"] = sop.metadata.get("recent_failures", 0) + 1
            # Reset recent failures after 5 successful executions
            if sop.success_count > 5:
                sop.metadata["recent_failures"] = 0

        # Recalculate confidence
        sop.calculate_confidence()

        # Save updated SOP
        if sop.status == SOPStatus.ACTIVE:
            sop_file = self._active_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"
        elif sop.status == SOPStatus.PENDING:
            sop_file = self._pending_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"
        else:
            sop_file = self._archived_dir / f"SOP_{sop.id}_{sop.name.replace(' ', '_')}.md"

        sop_file.write_text(sop.to_markdown(), encoding="utf-8")

        # Auto-approve if criteria met (only for pending SOPs)
        if sop.status == SOPStatus.PENDING and sop.can_auto_approve():
            logger.info(
                f"SOP {sop_id} meets auto-approval criteria: "
                f"executed={sop.executed_count}, success={sop.success_count}, "
                f"confidence={sop.confidence_score:.2f}"
            )
            self.approve_sop(sop_id)

    def check_and_auto_approve_sops(self) -> list[str]:
        """Check all pending SOPs for auto-approval criteria.

        Returns:
            List of SOP IDs that were auto-approved
        """
        approved = []
        for sop in self.list_pending_sops():
            if sop.can_auto_approve():
                logger.info(
                    f"Auto-approving SOP {sop.id}: "
                    f"executed={sop.executed_count}, success_rate={sop.success_count/sop.executed_count:.0%}, "
                    f"confidence={sop.confidence_score:.2f}"
                )
                if self.approve_sop(sop.id):
                    approved.append(sop.id)
        return approved

    # -------------------------------------------------------------------------
    # Lesson Management (Failure Learning)
    # -------------------------------------------------------------------------

    def record_lesson(
        self,
        lesson_type: LessonType,
        trigger: str,
        content: str,
        source_task_id: str,
        related_sop_id: Optional[str] = None,
    ) -> Lesson:
        """Record a lesson learned from failure or success.

        Args:
            lesson_type: Type of lesson (error pattern, recovery step, etc.)
            trigger: Pattern that triggers this lesson (error message, scenario)
            content: The lesson content
            source_task_id: Task where this was learned
            related_sop_id: Optional related SOP

        Returns:
            The recorded lesson
        """
        lesson = Lesson(
            lesson_type=lesson_type,
            trigger=trigger,
            content=content,
            source_task_id=source_task_id,
        )

        # Save to lessons directory using stable hash
        lesson_filename = _make_lesson_filename(lesson_type, trigger)
        lesson_file = self._lessons_dir / lesson_filename
        lesson_file.write_text(
            f"# Lesson: {lesson_type.value}\\n\\n"
            f"**来源任务**: `{source_task_id}`\\n\\n"
            f"**触发条件**: `{trigger}`\\n\\n"
            f"**教训**: {content}\\n\\n"
            f"**关联 SOP**: `{related_sop_id or '无'}`\\n",
            encoding="utf-8",
        )

        # Update in-memory cache
        self._lessons[lesson_filename] = {
            "file": lesson_filename,
            "trigger": trigger,
            "content": content,
        }

        # Add related SOP if provided
        if related_sop_id:
            self.add_lesson_to_sop(
                sop_id=related_sop_id,
                lesson_type=lesson_type,
                trigger=trigger,
                content=content,
                task_id=source_task_id,
            )

        # Add to L1 index if it's a common pattern
        if lesson_type in [LessonType.ANTI_PATTERN, LessonType.ERROR_PATTERN]:
            self.add_index_rule(f"⚠️ {trigger[:50]}: {content[:50]}")

        logger.info(f"Recorded lesson: {lesson_type.value} from task {source_task_id}")
        return lesson

    def find_lessons_for_error(self, error_message: str) -> list[Lesson]:
        """Find lessons that match an error pattern using cached data.

        Args:
            error_message: Current error message

        Returns:
            Matching lessons that can help resolve this error
        """
        matching_lessons = []
        error_lower = error_message.lower()

        # Use in-memory cache for fast lookup
        for lesson_data in self._lessons.values():
            trigger = lesson_data.get("trigger", "")
            if trigger.lower() in error_lower or trigger.lower() in error_lower:
                lesson = Lesson(
                    lesson_type=LessonType.ERROR_PATTERN,
                    trigger=trigger,
                    content=lesson_data.get("content", "")[:500],
                    source_task_id="cached",
                )
                matching_lessons.append(lesson)

        return matching_lessons

    # -------------------------------------------------------------------------
    # Utility Methods
    # -------------------------------------------------------------------------

    def get_memory_stats(self) -> dict[str, Any]:
        """Get memory system statistics."""
        pending_count = len(list(self._pending_dir.glob("*.md")))
        active_count = len(self._sops)
        archived_count = len(list(self._archived_dir.glob("*.md")))
        lessons_count = len(list(self._lessons_dir.glob("*.md")))
        scripts_count = len(list(self._scripts_dir.glob("*.py")))

        return {
            "l1_topics": len(self._index.topics) if self._index else 0,
            "l1_rules": len(self._index.rules) if self._index else 0,
            "l2_sections": len(self._facts.entries) if self._facts else 0,
            "l3_pending": pending_count,
            "l3_active": active_count,
            "l3_archived": archived_count,
            "lessons": lessons_count,
            "scripts": scripts_count,
        }

    def cleanup_old_archived(self, days: int = 90) -> int:
        """Clean up old archived SOPs.

        Args:
            days: Archive SOPs older than this many days

        Returns:
            Number of SOPs cleaned up
        """
        cleaned = 0
        cutoff = _utc_now_timestamp() - (days * 86400)

        for sop_file in self._archived_dir.glob("*.md"):
            try:
                sop = SOP.from_markdown(sop_file)
                if sop and sop.created_at:
                    created_ts = datetime.fromisoformat(sop.created_at).timestamp()
                    if created_ts < cutoff:
                        sop_file.unlink()
                        cleaned += 1
            except Exception:
                pass

        logger.info(f"Cleaned up {cleaned} old archived SOPs (>{days} days)")
        return cleaned
