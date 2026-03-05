"""Tests for SessionManager."""
import json
import pytest
from pathlib import Path
from datetime import datetime, timezone

from omiga.session.manager import (
    SessionManager,
    SessionEntry,
    generate_entry_id,
)
from omiga.agent import Message


class TestGenerateEntryId:
    """Test ID generation."""

    def test_generates_unique_ids(self):
        ids = [generate_entry_id() for _ in range(100)]
        assert len(set(ids)) == 100

    def test_id_length(self):
        entry_id = generate_entry_id()
        assert len(entry_id) == 8


class TestSessionEntry:
    """Test SessionEntry class."""

    def test_create_message_entry(self):
        msg = Message.user_message("Hello")
        entry = SessionEntry(
            type="message",
            id="test1",
            parent_id=None,
            message=msg,
        )
        assert entry.type == "message"
        assert entry.message.content == "Hello"

    def test_create_compaction_entry(self):
        entry = SessionEntry(
            type="compaction",
            id="test2",
            parent_id="test1",
            summary="Conversation summary",
        )
        assert entry.type == "compaction"
        assert entry.summary == "Conversation summary"

    def test_create_custom_entry(self):
        entry = SessionEntry(
            type="custom",
            id="test3",
            parent_id="test2",
            data={"key": "value"},
        )
        assert entry.type == "custom"
        assert entry.data["key"] == "value"

    def test_to_dict(self):
        msg = Message.user_message("Test")
        entry = SessionEntry(
            type="message",
            id="test4",
            parent_id=None,
            message=msg,
        )
        d = entry.to_dict()
        assert d["type"] == "message"
        assert d["id"] == "test4"
        assert d["message"]["content"] == "Test"

    def test_from_dict(self):
        data = {
            "type": "message",
            "id": "test5",
            "parent_id": None,
            "timestamp": "2026-03-04T00:00:00Z",
            "message": {"role": "user", "content": "Hello"},
        }
        entry = SessionEntry.from_dict(data)
        assert entry.id == "test5"
        assert entry.message is not None
        assert entry.message.content == "Hello"


class TestSessionManager:
    """Test SessionManager class."""

    @pytest.fixture
    def tmp_sessions_dir(self, tmp_path):
        """Create a temporary sessions directory."""
        return tmp_path / "sessions"

    @pytest.fixture
    def manager(self, tmp_sessions_dir):
        """Create a session manager."""
        return SessionManager(tmp_sessions_dir)

    def test_init_creates_directory(self, tmp_sessions_dir):
        assert not tmp_sessions_dir.exists()
        SessionManager(tmp_sessions_dir)
        assert tmp_sessions_dir.exists()

    def test_create_session(self, manager):
        session_id = manager.create_session("tg:123456")
        assert session_id is not None
        assert len(session_id) == 8
        assert manager.get_session(session_id) == []

    def test_create_session_with_custom_id(self, manager):
        session_id = manager.create_session("tg:123456", session_id="custom1")
        assert session_id == "custom1"

    def test_get_nonexistent_session(self, manager):
        assert manager.get_session("nonexistent") is None

    def test_append_message(self, manager):
        session_id = manager.create_session("tg:123456")
        entry_id = manager.append_message(
            session_id, Message.user_message("Hello")
        )

        entries = manager.get_session(session_id)
        assert len(entries) == 1
        assert entries[0].id == entry_id
        assert entries[0].message.content == "Hello"

    def test_append_multiple_messages(self, manager):
        session_id = manager.create_session("tg:123456")

        manager.append_message(session_id, Message.user_message("Hello"))
        manager.append_message(
            session_id, Message.assistant_message("Hi there")
        )
        manager.append_message(session_id, Message.user_message("How are you?"))

        entries = manager.get_session(session_id)
        assert len(entries) == 3
        # Verify parent chain
        assert entries[0].parent_id is None
        assert entries[1].parent_id == entries[0].id
        assert entries[2].parent_id == entries[1].id

    def test_append_custom(self, manager):
        session_id = manager.create_session("tg:123456")
        entry_id = manager.append_custom(
            session_id, "system_note", {"note": "Important"}
        )

        entries = manager.get_session(session_id)
        assert len(entries) == 1
        assert entries[0].type == "custom"
        assert entries[0].data["custom_type"] == "system_note"
        assert entries[0].data["note"] == "Important"

    def test_append_custom_enters_context(self, manager):
        session_id = manager.create_session("tg:123456")
        manager.append_custom(
            session_id, "user_note", {"note": "Visible"}, enters_context=True
        )

        entries = manager.get_session(session_id)
        assert entries[0].type == "custom_message"

    def test_navigate_to(self, manager):
        session_id = manager.create_session("tg:123456")
        entry1 = manager.append_message(
            session_id, Message.user_message("First")
        )
        entry2 = manager.append_message(
            session_id, Message.assistant_message("Second")
        )

        manager.navigate_to(session_id, entry1)
        # Next append should branch from entry1
        entry3 = manager.append_message(
            session_id, Message.user_message("Branch")
        )

        entries = manager.get_session(session_id)
        assert len(entries) == 3
        assert entries[2].parent_id == entry1

    def test_navigate_to_nonexistent_entry(self, manager):
        session_id = manager.create_session("tg:123456")
        with pytest.raises(ValueError, match="Entry not found"):
            manager.navigate_to(session_id, "nonexistent")

    def test_fork_from(self, manager):
        session_id = manager.create_session("tg:123456")
        entry1 = manager.append_message(
            session_id, Message.user_message("Hello")
        )
        entry2 = manager.append_message(
            session_id, Message.assistant_message("Hi")
        )

        new_session_id = manager.fork_from(session_id, entry1)

        # Verify new session
        new_entries = manager.get_session(new_session_id)
        assert len(new_entries) == 1
        assert new_entries[0].id == entry1

        # Verify current position
        assert manager._current_positions[new_session_id] == entry1

    def test_fork_from_with_new_chat_jid(self, manager):
        session_id = manager.create_session("tg:123456")
        entry1 = manager.append_message(
            session_id, Message.user_message("Hello")
        )

        new_session_id = manager.fork_from(
            session_id, entry1, new_chat_jid="tg:999999"
        )

        metadata = manager._metadata[new_session_id]
        assert metadata["chat_jid"] == "tg:999999"

    def test_save_compaction(self, manager):
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Hello"))

        entry_id = manager.save_compaction(
            session_id,
            summary="Conversation summary",
            first_kept_entry_id="entry1",
            tokens_before=1000,
        )

        entries = manager.get_session(session_id)
        assert len(entries) == 2
        compaction = entries[1]
        assert compaction.type == "compaction"
        assert compaction.summary == "Conversation summary"
        assert compaction.data["tokens_before"] == 1000

    def test_save_and_load(self, manager, tmp_sessions_dir):
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Hello"))
        manager.append_message(
            session_id, Message.assistant_message("Hi there")
        )

        # Save
        file_path = manager.save(session_id)
        assert file_path.exists()

        # Clear in-memory
        manager._sessions.clear()

        # Load
        loaded = manager.load(session_id)
        assert loaded is True

        entries = manager.get_session(session_id)
        assert len(entries) == 2
        assert entries[0].message.content == "Hello"
        assert entries[1].message.content == "Hi there"

    def test_load_nonexistent_file(self, manager):
        loaded = manager.load("nonexistent")
        assert loaded is False

    def test_delete(self, manager, tmp_sessions_dir):
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Hello"))
        manager.save(session_id)

        deleted = manager.delete(session_id)
        assert deleted is True
        assert manager.get_session(session_id) is None

        # Verify file deleted
        file_path = tmp_sessions_dir / f"session_{session_id}.jsonl"
        assert not file_path.exists()

    def test_delete_nonexistent(self, manager):
        deleted = manager.delete("nonexistent")
        assert deleted is False

    def test_list_sessions(self, manager):
        session1 = manager.create_session("tg:111111")
        session2 = manager.create_session("tg:222222")

        sessions = manager.list_sessions()
        assert len(sessions) == 2
        session_ids = {s["session_id"] for s in sessions}
        assert session1 in session_ids
        assert session2 in session_ids

    def test_get_statistics(self, manager):
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Hello"))
        manager.append_message(
            session_id, Message.assistant_message("Hi")
        )
        manager.save_compaction(
            session_id,
            summary="Summary",
            first_kept_entry_id="entry1",
            tokens_before=100,
        )

        stats = manager.get_statistics(session_id)
        assert stats["total_entries"] == 3
        assert stats["message_count"] == 2
        assert stats["compaction_count"] == 1

    def test_get_tree(self, manager):
        session_id = manager.create_session("tg:123456")
        entry1 = manager.append_message(
            session_id, Message.user_message("Hello")
        )
        entry2 = manager.append_message(
            session_id, Message.assistant_message("Hi")
        )

        tree = manager.get_tree(session_id)
        assert tree["root"] is not None
        assert tree["root"]["entry"].id == entry1
        assert len(tree["children"]) == 1
        assert tree["children"][0]["entry"].id == entry2

    def test_get_entries_for_context(self, manager):
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Hello"))
        manager.append_message(
            session_id, Message.assistant_message("Hi")
        )
        manager.append_message(session_id, Message.user_message("How are you?"))

        entries = manager.get_entries_for_context(session_id)
        assert len(entries) == 3

        # Test limit
        entries_limited = manager.get_entries_for_context(session_id, limit=2)
        assert len(entries_limited) == 2

    def test_jsonl_format(self, manager, tmp_sessions_dir):
        """Verify JSONL file format."""
        session_id = manager.create_session("tg:123456")
        manager.append_message(session_id, Message.user_message("Test"))
        manager.save(session_id)

        file_path = tmp_sessions_dir / f"session_{session_id}.jsonl"
        with open(file_path, "r") as f:
            lines = f.readlines()

        assert len(lines) == 2  # header + 1 entry

        # Verify header
        header = json.loads(lines[0])
        assert header["type"] == "header"
        assert header["session_id"] == session_id

        # Verify entry
        entry_data = json.loads(lines[1])
        assert entry_data["type"] == "message"
        assert entry_data["message"]["content"] == "Test"
