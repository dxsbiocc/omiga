"""Session Manager for Omiga.

This module provides session tree management with support for:
- Tree-structured conversation history
- Navigation to specific entries
- Branching from any entry
- JSONL persistence
- Compaction support
"""
from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Literal, Optional

from omiga.agent import Message

logger = logging.getLogger("omiga.session_manager")


# ---------------------------------------------------------------------------
# Session Entry Types
# ---------------------------------------------------------------------------


SessionEntryType = Literal[
    "message",           # LLM message (user/assistant/tool/system)
    "compaction",        # Compression summary
    "branch_summary",    # Branch point summary
    "custom",            # Custom data (not in context)
    "custom_message",    # Custom message (enters context)
    "label",             # User label/tag
]


@dataclass
class SessionEntry:
    """A session entry in the tree.

    Attributes:
        type: Entry type
        id: Unique entry identifier
        parent_id: Parent entry ID (None for root)
        timestamp: ISO format UTC timestamp
        message: Message object (for 'message' type)
        summary: Summary text (for 'compaction', 'branch_summary')
        data: Custom data dict (for 'custom', 'custom_message')
        label: Label text (for 'label' type)
    """

    type: SessionEntryType
    id: str
    parent_id: Optional[str]
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    message: Optional[Message] = None
    summary: Optional[str] = None
    data: Optional[Dict[str, Any]] = None
    label: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        d: Dict[str, Any] = {
            "type": self.type,
            "id": self.id,
            "parent_id": self.parent_id,
            "timestamp": self.timestamp,
        }
        if self.message:
            d["message"] = self.message.to_dict()
        if self.summary:
            d["summary"] = self.summary
        if self.data:
            d["data"] = self.data
        if self.label:
            d["label"] = self.label
        return d

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "SessionEntry":
        """Create from dictionary."""
        message = None
        if "message" in data:
            msg_data = data["message"]
            message = Message(
                role=msg_data.get("role", "user"),
                content=msg_data.get("content", ""),
                tool_call_id=msg_data.get("tool_call_id"),
                tool_calls=msg_data.get("tool_calls"),
                timestamp=msg_data.get("timestamp", data["timestamp"]),
            )

        return cls(
            type=data["type"],
            id=data["id"],
            parent_id=data.get("parent_id"),
            timestamp=data.get("timestamp", ""),
            message=message,
            summary=data.get("summary"),
            data=data.get("data"),
            label=data.get("label"),
        )


# ---------------------------------------------------------------------------
# Session Manager
# ---------------------------------------------------------------------------


def generate_entry_id() -> str:
    """Generate a unique entry ID."""
    import uuid
    return str(uuid.uuid4())[:8]


class SessionManager:
    """Session manager with tree structure.

    Features:
    - Tree-structured conversation history
    - Navigation to specific entries
    - Branching from any entry
    - JSONL persistence
    - Compaction support

    Usage:
        manager = SessionManager(Path("./sessions"))
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Hello"))
        manager.save(session_id)
    """

    def __init__(self, sessions_dir: Path):
        """Initialize session manager.

        Args:
            sessions_dir: Directory to store session files
        """
        self.sessions_dir = sessions_dir
        self.sessions_dir.mkdir(parents=True, exist_ok=True)

        # In-memory session storage: session_id -> list of entries
        self._sessions: Dict[str, List[SessionEntry]] = {}

        # Current position tracking: session_id -> current_entry_id
        self._current_positions: Dict[str, Optional[str]] = {}

        # Session metadata
        self._metadata: Dict[str, Dict[str, Any]] = {}

    def create_session(
        self,
        chat_jid: str,
        session_id: Optional[str] = None,
    ) -> str:
        """Create a new session.

        Args:
            chat_jid: Group/chat identifier
            session_id: Optional session ID (generated if not provided)

        Returns:
            Session ID
        """
        if session_id is None:
            session_id = generate_entry_id()

        self._sessions[session_id] = []
        self._current_positions[session_id] = None
        self._metadata[session_id] = {
            "chat_jid": chat_jid,
            "created_at": datetime.now(timezone.utc).isoformat(),
            "updated_at": datetime.now(timezone.utc).isoformat(),
            "branch_count": 0,
        }

        logger.info(f"Created session: {session_id} for {chat_jid}")
        return session_id

    def get_session(self, session_id: str) -> Optional[List[SessionEntry]]:
        """Get session entries.

        Args:
            session_id: Session identifier

        Returns:
            List of entries or None if not found
        """
        return self._sessions.get(session_id)

    def get_tree(
        self, session_id: str
    ) -> Dict[str, Any]:
        """Get session as a tree structure.

        Args:
            session_id: Session identifier

        Returns:
            Tree dictionary with 'root' and 'children'
        """
        entries = self._sessions.get(session_id, [])
        if not entries:
            return {"root": None, "children": []}

        # Build node map
        node_map: Dict[str, Dict] = {}
        for entry in entries:
            node_map[entry.id] = {
                "entry": entry,
                "children": [],
            }

        # Build tree
        root = None
        for entry in entries:
            if entry.parent_id is None:
                root = node_map[entry.id]
            elif entry.parent_id in node_map:
                node_map[entry.parent_id]["children"].append(node_map[entry.id])

        return {"root": root, "children": node_map[root["entry"].id]["children"] if root else []}

    def get_entries_for_context(
        self,
        session_id: str,
        limit: Optional[int] = None,
    ) -> List[SessionEntry]:
        """Get entries for LLM context (following main branch).

        Args:
            session_id: Session identifier
            limit: Optional limit on number of entries

        Returns:
            List of entries along the main branch
        """
        entries = self._sessions.get(session_id, [])
        if not entries:
            return []

        # Find root and follow main branch
        root = next((e for e in entries if e.parent_id is None), None)
        if not root:
            return list(entries[:limit]) if limit else entries

        # Follow main branch (first child at each level)
        result = [root]
        current_id = root.id
        while True:
            children = [e for e in entries if e.parent_id == current_id]
            if not children:
                break
            # Take the first child (main branch)
            current_id = children[0].id
            result.append(children[0])

        if limit:
            return result[-limit:]
        return result

    def append_message(
        self,
        session_id: str,
        message: Message,
    ) -> str:
        """Append a message to the session.

        Args:
            session_id: Session identifier
            message: Message to append

        Returns:
            Entry ID
        """
        entries = self._sessions.get(session_id)
        if entries is None:
            raise ValueError(f"Session not found: {session_id}")

        # Determine parent (last entry or current position)
        parent_id = self._current_positions.get(session_id)
        if parent_id is None and entries:
            parent_id = entries[-1].id

        entry = SessionEntry(
            type="message",
            id=generate_entry_id(),
            parent_id=parent_id,
            message=message,
        )
        entries.append(entry)
        self._current_positions[session_id] = entry.id
        self._update_metadata(session_id)

        logger.debug(f"Appended message to session {session_id}: {entry.id}")
        return entry.id

    def append_custom(
        self,
        session_id: str,
        custom_type: str,
        data: Dict[str, Any],
        enters_context: bool = False,
    ) -> str:
        """Append a custom entry.

        Args:
            session_id: Session identifier
            custom_type: Custom type identifier
            data: Custom data
            enters_context: If True, uses 'custom_message' type

        Returns:
            Entry ID
        """
        entries = self._sessions.get(session_id)
        if entries is None:
            raise ValueError(f"Session not found: {session_id}")

        parent_id = self._current_positions.get(session_id)
        if parent_id is None and entries:
            parent_id = entries[-1].id

        entry = SessionEntry(
            type="custom_message" if enters_context else "custom",
            id=generate_entry_id(),
            parent_id=parent_id,
            data={"custom_type": custom_type, **data},
        )
        entries.append(entry)
        self._current_positions[session_id] = entry.id
        self._update_metadata(session_id)

        return entry.id

    def navigate_to(self, session_id: str, entry_id: str) -> None:
        """Navigate to a specific entry.

        Future appends will branch from this entry.

        Args:
            session_id: Session identifier
            entry_id: Entry ID to navigate to
        """
        entries = self._sessions.get(session_id)
        if entries is None:
            raise ValueError(f"Session not found: {session_id}")

        entry = next((e for e in entries if e.id == entry_id), None)
        if entry is None:
            raise ValueError(f"Entry not found: {entry_id}")

        self._current_positions[session_id] = entry_id
        logger.debug(f"Navigated to entry {entry_id} in session {session_id}")

    def fork_from(
        self,
        session_id: str,
        entry_id: str,
        new_chat_jid: Optional[str] = None,
    ) -> str:
        """Create a branch from an entry.

        Args:
            session_id: Source session ID
            entry_id: Entry ID to fork from
            new_chat_jid: Optional new chat JID for the branch

        Returns:
            New session ID
        """
        entries = self._sessions.get(session_id)
        if entries is None:
            raise ValueError(f"Session not found: {session_id}")

        entry = next((e for e in entries if e.id == entry_id), None)
        if entry is None:
            raise ValueError(f"Entry not found: {entry_id}")

        # Create new session
        new_session_id = generate_entry_id()
        source_metadata = self._metadata.get(session_id, {})

        # Copy entries up to and including the fork point
        entry_index = next(i for i, e in enumerate(entries) if e.id == entry_id)
        self._sessions[new_session_id] = [
            SessionEntry(
                type=e.type,
                id=e.id,  # Keep same IDs for shared history
                parent_id=e.parent_id,
                timestamp=e.timestamp,
                message=e.message,
                summary=e.summary,
                data=e.data,
                label=e.label,
            )
            for e in entries[: entry_index + 1]
        ]

        # Navigate to the fork point
        self._current_positions[new_session_id] = entry_id

        # Set metadata
        self._metadata[new_session_id] = {
            "chat_jid": new_chat_jid or source_metadata.get("chat_jid"),
            "created_at": datetime.now(timezone.utc).isoformat(),
            "updated_at": datetime.now(timezone.utc).isoformat(),
            "forked_from": session_id,
            "fork_point": entry_id,
            "branch_count": 0,
        }

        # Update source metadata
        self._metadata[session_id]["branch_count"] = \
            self._metadata[session_id].get("branch_count", 0) + 1

        logger.info(
            f"Forked session {session_id}@{entry_id} -> {new_session_id}"
        )
        return new_session_id

    def save_compaction(
        self,
        session_id: str,
        summary: str,
        first_kept_entry_id: str,
        tokens_before: int,
        file_operations: Optional[Dict[str, List[str]]] = None,
    ) -> str:
        """Save a compaction entry.

        Args:
            session_id: Session identifier
            summary: Compression summary
            first_kept_entry_id: First entry ID kept after compaction
            tokens_before: Token count before compaction
            file_operations: File operation tracking

        Returns:
            Entry ID
        """
        entries = self._sessions.get(session_id)
        if entries is None:
            raise ValueError(f"Session not found: {session_id}")

        parent_id = self._current_positions.get(session_id)
        if parent_id is None and entries:
            parent_id = entries[-1].id

        data = {
            "first_kept_entry_id": first_kept_entry_id,
            "tokens_before": tokens_before,
        }
        if file_operations:
            data["file_operations"] = file_operations

        entry = SessionEntry(
            type="compaction",
            id=generate_entry_id(),
            parent_id=parent_id,
            summary=summary,
            data=data,
        )
        entries.append(entry)
        self._current_positions[session_id] = entry.id
        self._update_metadata(session_id)

        logger.info(
            f"Saved compaction for session {session_id}: "
            f"{tokens_before} tokens -> {summary[:50]}..."
        )
        return entry.id

    def save(
        self,
        session_id: str,
        filename: Optional[str] = None,
    ) -> Path:
        """Save session to JSONL file.

        Args:
            session_id: Session identifier
            filename: Optional custom filename

        Returns:
            Path to saved file
        """
        entries = self._sessions.get(session_id)
        if entries is None:
            raise ValueError(f"Session not found: {session_id}")

        if filename is None:
            filename = f"session_{session_id}.jsonl"

        file_path = self.sessions_dir / filename

        with open(file_path, "w", encoding="utf-8") as f:
            # Write header
            header = {
                "type": "header",
                "session_id": session_id,
                "metadata": self._metadata.get(session_id, {}),
            }
            f.write(json.dumps(header, ensure_ascii=False) + "\n")

            # Write entries
            for entry in entries:
                f.write(
                    json.dumps(entry.to_dict(), ensure_ascii=False) + "\n"
                )

        logger.debug(f"Saved session {session_id} to {file_path}")
        return file_path

    def load(
        self,
        session_id: str,
        filename: Optional[str] = None,
    ) -> bool:
        """Load session from JSONL file.

        Args:
            session_id: Session identifier
            filename: Optional custom filename

        Returns:
            True if loaded successfully
        """
        if filename is None:
            filename = f"session_{session_id}.jsonl"

        file_path = self.sessions_dir / filename
        if not file_path.exists():
            logger.warning(f"Session file not found: {file_path}")
            return False

        entries: List[SessionEntry] = []
        metadata: Dict[str, Any] = {}

        with open(file_path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue

                data = json.loads(line)
                if data.get("type") == "header":
                    metadata = data.get("metadata", {})
                else:
                    entries.append(SessionEntry.from_dict(data))

        self._sessions[session_id] = entries
        self._metadata[session_id] = metadata
        if entries:
            self._current_positions[session_id] = entries[-1].id

        logger.info(f"Loaded session {session_id} with {len(entries)} entries")
        return True

    def delete(self, session_id: str) -> bool:
        """Delete a session.

        Args:
            session_id: Session identifier

        Returns:
            True if deleted
        """
        if session_id not in self._sessions:
            return False

        del self._sessions[session_id]
        self._current_positions.pop(session_id, None)
        self._metadata.pop(session_id, None)

        # Delete file if exists
        file_path = self.sessions_dir / f"session_{session_id}.jsonl"
        if file_path.exists():
            file_path.unlink()

        logger.info(f"Deleted session {session_id}")
        return True

    def list_sessions(self) -> List[Dict[str, Any]]:
        """List all sessions.

        Returns:
            List of session metadata
        """
        return [
            {"session_id": sid, **meta}
            for sid, meta in self._metadata.items()
        ]

    def get_statistics(self, session_id: str) -> Dict[str, Any]:
        """Get session statistics.

        Args:
            session_id: Session identifier

        Returns:
            Statistics dictionary
        """
        entries = self._sessions.get(session_id, [])

        message_count = sum(1 for e in entries if e.type == "message")
        compaction_count = sum(1 for e in entries if e.type == "compaction")
        branch_count = self._metadata.get(session_id, {}).get(
            "branch_count", 0
        )

        return {
            "session_id": session_id,
            "total_entries": len(entries),
            "message_count": message_count,
            "compaction_count": compaction_count,
            "branch_count": branch_count,
            "metadata": self._metadata.get(session_id, {}),
        }

    def _update_metadata(self, session_id: str) -> None:
        """Update session metadata timestamp."""
        if session_id in self._metadata:
            self._metadata[session_id]["updated_at"] = datetime.now(
                timezone.utc
            ).isoformat()
