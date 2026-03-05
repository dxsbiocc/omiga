"""Agent working memory for Omiga.

This module provides a unified Memory abstraction for Agent sessions,
separating working memory (short-term) from long-term memory (SOPs, facts).

The Memory class manages:
- Conversation messages with automatic pruning
- Working context for temporary state
- Sync to long-term memory (MemoryManager)
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, TYPE_CHECKING

# Delay import to avoid circular import
if TYPE_CHECKING:
    from omiga.agent.session import Message

from omiga.memory.manager import MemoryManager


@dataclass
class AgentMemory:
    """Agent working memory.

    This class manages the Agent's short-term working memory, including
    conversation messages and temporary context. It interfaces with the
    long-term MemoryManager for persistence and SOP retrieval.

    Attributes:
        messages: Conversation history
        max_messages: Maximum messages to keep in memory
        working_context: Temporary context for current session
        session_id: Optional session identifier
    """

    messages: List[Any] = field(default_factory=list)  # type: ignore[assignment]
    max_messages: int = 100
    working_context: Dict[str, Any] = field(default_factory=dict)
    session_id: Optional[str] = None

    def add_message(self, message: Any) -> None:  # type: ignore[override]
        """Add a message to working memory.

        Automatically prunes old messages if exceeding max_messages.

        Args:
            message: Message to add
        """
        self.messages.append(message)

        # Prune old messages if exceeding limit
        if len(self.messages) > self.max_messages:
            # Keep the most recent messages
            self.messages = self.messages[-self.max_messages:]

    def get_recent_messages(self, n: Optional[int] = None) -> List[Any]:  # type: ignore[override]
        """Get recent messages from working memory.

        Args:
            n: Number of messages to retrieve (None = all)

        Returns:
            List of recent messages
        """
        if n is None:
            return self.messages.copy()

        return self.messages[-n:] if n > 0 else []

    def get_context(self, key: str, default: Any = None) -> Any:
        """Get a value from working context.

        Args:
            key: Context key
            default: Default value if key not found

        Returns:
            Context value or default
        """
        return self.working_context.get(key, default)

    def set_context(self, key: str, value: Any) -> None:
        """Set a value in working context.

        Args:
            key: Context key
            value: Context value
        """
        self.working_context[key] = value

    def clear_context(self, key: Optional[str] = None) -> None:
        """Clear working context.

        Args:
            key: Specific key to clear (None = clear all)
        """
        if key is None:
            self.working_context.clear()
        else:
            self.working_context.pop(key, None)

    def clear_messages(self) -> None:
        """Clear all messages from working memory."""
        self.messages.clear()

    def clear(self) -> None:
        """Clear all working memory (messages + context)."""
        self.clear_messages()
        self.clear_context()

    def get_message_count(self) -> int:
        """Get current message count."""
        return len(self.messages)

    def is_near_capacity(self, threshold: float = 0.8) -> bool:
        """Check if memory is near capacity.

        Args:
            threshold: Capacity threshold (0.0-1.0)

        Returns:
            True if near capacity
        """
        return len(self.messages) >= (self.max_messages * threshold)

    def to_dict_list(self) -> List[Dict[str, Any]]:
        """Convert messages to dictionary list for LLM API.

        Returns:
            List of message dictionaries
        """
        return [msg.to_dict() for msg in self.messages]

    def sync_to_long_term(
        self,
        memory_manager: MemoryManager,
        auto_compact: bool = True,
    ) -> None:
        """Sync working memory to long-term memory.

        This method extracts action-verified facts and stores them
        in the long-term memory system.

        Args:
            memory_manager: Long-term memory manager
            auto_compact: Whether to compact if near capacity
        """
        if auto_compact and self.is_near_capacity():
            # Trigger compaction hint
            # Actual compaction is handled by SessionManager
            pass

        # Extract and store facts from working context
        facts = self.working_context.get("facts_to_store", {})
        for section, entries in facts.items():
            for key, value in entries.items():
                memory_manager.add_fact(
                    section=section,
                    key=key,
                    value=str(value),
                    source=self.session_id or "unknown",
                    verified=True,
                )

        # Clear stored facts after syncing
        if "facts_to_store" in self.working_context:
            del self.working_context["facts_to_store"]

    def store_fact(self, section: str, key: str, value: str) -> None:
        """Queue a fact for storage to long-term memory.

        Facts are stored in working context and synced later
        via sync_to_long_term().

        Args:
            section: Fact section (e.g., "paths", "config")
            key: Fact key
            value: Fact value
        """
        if "facts_to_store" not in self.working_context:
            self.working_context["facts_to_store"] = {}

        if section not in self.working_context["facts_to_store"]:
            self.working_context["facts_to_store"][section] = {}

        self.working_context["facts_to_store"][section][key] = value

    def get_all_facts(self, memory_manager: MemoryManager) -> Dict[str, Dict[str, str]]:
        """Get all facts from long-term memory.

        Args:
            memory_manager: Long-term memory manager

        Returns:
            Dictionary of section -> key -> value
        """
        facts_db = memory_manager.get_facts()
        result = {}

        for section, entries in facts_db.entries.items():
            result[section] = {}
            for entry in entries:
                result[section][entry.key] = entry.value

        return result

    def find_sop(
        self,
        memory_manager: MemoryManager,
        keyword: str,
    ) -> Optional[Any]:
        """Find SOP by keyword from long-term memory.

        Args:
            memory_manager: Long-term memory manager
            keyword: Search keyword

        Returns:
            Matching SOP or None
        """
        # First check index for quick lookup
        index = memory_manager.get_index()
        location = index.topics.get(keyword)

        if location and location.startswith("L3/"):
            sop_id = location.split("/", 1)[1]
            return memory_manager.get_sop(sop_id)

        # Fallback: search all active SOPs
        for sop in memory_manager.list_active_sops():
            if keyword.lower() in sop.name.lower():
                return sop

        return None

    def get_active_sops(self, memory_manager: MemoryManager) -> List[Any]:
        """Get all active SOPs from long-term memory.

        Args:
            memory_manager: Long-term memory manager

        Returns:
            List of active SOPs
        """
        return memory_manager.list_active_sops()

    def get_lessons_for_error(
        self,
        memory_manager: MemoryManager,
        error_message: str,
    ) -> List[Any]:
        """Find lessons matching an error pattern.

        Args:
            memory_manager: Long-term memory manager
            error_message: Error message to match

        Returns:
            List of matching lessons
        """
        return memory_manager.find_lessons_for_error(error_message)
