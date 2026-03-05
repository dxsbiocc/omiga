"""Session context compaction for Omiga.

This module provides automatic context compaction to prevent
token limit overflow in long conversations.
"""
from __future__ import annotations

import re
import logging
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, TYPE_CHECKING

if TYPE_CHECKING:
    from omiga.session.manager import SessionEntry, SessionManager

logger = logging.getLogger("omiga.session.compaction")

# Pre-compiled regex patterns for file operation extraction
READ_FILE_PATTERN = re.compile(
    r'read_file\s*[\(:\s]+["\']([^"\']+)["\']', re.IGNORECASE
)
WRITE_FILE_PATTERN = re.compile(
    r'write_file\s*[\(:\s]+["\']([^"\']+)["\']', re.IGNORECASE
)


@dataclass
class CompactionResult:
    """Result of session compaction.

    Attributes:
        summary: Compressed conversation summary
        first_kept_entry_id: First entry ID kept after compaction
        tokens_before: Token count before compaction
        tokens_after: Token count after compaction (approximate)
        file_operations: File operation tracking
        entries_compacted: Number of entries compacted
    """

    summary: str
    first_kept_entry_id: str
    tokens_before: int
    tokens_after: int = 0
    file_operations: Dict[str, List[str]] = field(default_factory=dict)
    entries_compacted: int = 0


def count_tokens(entries: List[SessionEntry]) -> int:
    """Count tokens in session entries.

    This is a simple estimation. For production, use a proper
    tokenizer like tiktoken.

    Args:
        entries: List of session entries

    Returns:
        Estimated token count
    """
    total = 0
    for entry in entries:
        if entry.message:
            # Rough estimation: 1 token ≈ 4 characters
            total += len(entry.message.content) // 4
        if entry.summary:
            total += len(entry.summary) // 4
        if entry.data:
            import json
            total += len(json.dumps(entry.data)) // 4
    return total


def serialize_entries(entries: List[SessionEntry]) -> str:
    """Serialize entries to text for LLM summarization.

    Args:
        entries: List of session entries

    Returns:
        Serialized text
    """
    lines = []
    for entry in entries:
        if entry.type == "message" and entry.message:
            role = entry.message.role
            content = entry.message.content
            lines.append(f"[{role}]: {content}")
        elif entry.type == "compaction" and entry.summary:
            lines.append(f"[SYSTEM: Compaction] {entry.summary}")
    return "\n".join(lines)


def extract_file_operations(entries: List[SessionEntry]) -> Dict[str, List[str]]:
    """Extract file operations from session entries.

    Args:
        entries: List of session entries

    Returns:
        Dictionary with 'read_files' and 'modified_files' lists
    """
    read_files: set[str] = set()
    modified_files: set[str] = set()

    for entry in entries:
        if entry.message and entry.message.content:
            content = entry.message.content

            # Match read_file calls
            if "read_file" in content.lower():
                paths = READ_FILE_PATTERN.findall(content)
                read_files.update(paths)

            # Match write_file calls
            if "write_file" in content.lower():
                paths = WRITE_FILE_PATTERN.findall(content)
                modified_files.update(paths)

    return {
        "read_files": sorted(list(read_files)),
        "modified_files": sorted(list(modified_files)),
    }


async def compact(
    entries: List[SessionEntry],
    max_tokens: int,
    model_call: Optional[Any] = None,
) -> CompactionResult:
    """Compact session entries.

    This function:
    1. Serializes the conversation
    2. Calls LLM to generate a summary
    3. Extracts file operations
    4. Returns compaction result

    Args:
        entries: List of session entries to compact
        max_tokens: Target token count after compaction
        model_call: Optional model call function for summarization

    Returns:
        CompactionResult
    """
    if not entries:
        return CompactionResult(
            summary="Empty conversation",
            first_kept_entry_id="",
            tokens_before=0,
            tokens_after=0,
        )

    # Count tokens
    tokens_before = count_tokens(entries)

    # Determine compaction threshold (keep ~50% of content)
    target_tokens = max_tokens * 0.5

    # Calculate how many entries to compact
    cumulative_tokens = 0
    compact_until_index = 0
    for i, entry in enumerate(entries):
        if entry.message:
            cumulative_tokens += len(entry.message.content) // 4
        if cumulative_tokens > tokens_before - target_tokens:
            compact_until_index = i
            break

    # Entries to compact
    entries_to_compact = entries[:compact_until_index]
    entries_to_keep = entries[compact_until_index:]

    if not entries_to_compact:
        # Nothing to compact
        return CompactionResult(
            summary="",
            first_kept_entry_id=entries[0].id if entries else "",
            tokens_before=tokens_before,
            tokens_after=tokens_before,
        )

    # Serialize for summarization
    serialized = serialize_entries(entries_to_compact)

    # Generate summary using LLM
    summary = ""
    if model_call:
        try:
            summary = await model_call(
                system_prompt="Summarize this conversation concisely, preserving key information, decisions, and file operations.",
                messages=[{"role": "user", "content": serialized}],
            )
        except Exception as e:
            logger.warning(f"LLM summarization failed, using fallback: {e}")
            summary = f"Conversation summary: {len(entries_to_compact)} messages compacted."
    else:
        # Fallback summary
        summary = (
            f"Previous conversation: {len(entries_to_compact)} messages. "
            f"Key topics discussed and actions taken. "
            f"Token count reduced from {tokens_before} to approximately {target_tokens:.0f}."
        )

    # Extract file operations
    file_operations = extract_file_operations(entries_to_compact)

    # Calculate tokens after (estimate)
    tokens_after = count_tokens(entries_to_keep) + len(summary) // 4

    first_kept_entry = entries_to_keep[0] if entries_to_keep else None

    return CompactionResult(
        summary=summary,
        first_kept_entry_id=first_kept_entry.id if first_kept_entry else "",
        tokens_before=tokens_before,
        tokens_after=tokens_after,
        file_operations=file_operations,
        entries_compacted=len(entries_to_compact),
    )


class CompactionManager:
    """Manager for automatic compaction.

    Monitors token usage and triggers compaction when thresholds are exceeded.

    Usage:
        compaction = CompactionManager(session_manager)
        await compaction.check_and_compact(session_id, max_tokens=100000)
    """

    def __init__(
        self,
        session_manager: SessionManager,
        compaction_threshold: int = 100000,
        target_ratio: float = 0.5,
    ):
        """Initialize compaction manager.

        Args:
            session_manager: Session manager instance
            compaction_threshold: Token count threshold to trigger compaction
            target_ratio: Target token ratio after compaction
        """
        self.session_manager = session_manager
        self.compaction_threshold = compaction_threshold
        self.target_ratio = target_ratio
        self.model_call = None

    def set_model_call(self, model_call: Any) -> None:
        """Set the model call function for summarization.

        Args:
            model_call: Async function for LLM calls
        """
        self.model_call = model_call

    async def check_and_compact(
        self,
        session_id: str,
        override_threshold: Optional[int] = None,
    ) -> Optional[CompactionResult]:
        """Check if compaction is needed and perform it.

        Args:
            session_id: Session identifier
            override_threshold: Optional threshold override

        Returns:
            CompactionResult if compaction was performed, None otherwise
        """
        entries = self.session_manager.get_session(session_id)
        if not entries:
            return None

        threshold = override_threshold or self.compaction_threshold
        current_tokens = count_tokens(entries)

        if current_tokens <= threshold:
            logger.debug(
                f"Session {session_id}: {current_tokens} tokens, "
                f"below threshold {threshold}"
            )
            return None

        logger.info(
            f"Session {session_id}: {current_tokens} tokens exceeds "
            f"threshold {threshold}, triggering compaction"
        )

        # Perform compaction
        target_tokens = int(threshold * self.target_ratio)
        result = await compact(entries, target_tokens, self.model_call)

        # Save compaction to session
        self.session_manager.save_compaction(
            session_id,
            summary=result.summary,
            first_kept_entry_id=result.first_kept_entry_id,
            tokens_before=result.tokens_before,
            file_operations=result.file_operations,
        )

        logger.info(
            f"Compaction complete: {result.tokens_before} -> {result.tokens_after} "
            f"tokens ({len(entries) - result.entries_compacted} entries kept)"
        )

        return result

    def get_token_count(self, session_id: str) -> int:
        """Get current token count for a session.

        Args:
            session_id: Session identifier

        Returns:
            Token count
        """
        entries = self.session_manager.get_session(session_id)
        if not entries:
            return 0
        return count_tokens(entries)
