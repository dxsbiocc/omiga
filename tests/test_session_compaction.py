"""Tests for session compaction."""
import pytest
from omiga.session.compaction import (
    compact,
    count_tokens,
    serialize_entries,
    extract_file_operations,
    CompactionResult,
    CompactionManager,
)
from omiga.session.manager import SessionEntry, SessionManager
from omiga.agent import Message


class TestCountTokens:
    """Test token counting."""

    def test_count_empty_entries(self):
        tokens = count_tokens([])
        assert tokens == 0

    def test_count_message_tokens(self):
        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.user_message("Hello world"),
            )
        ]
        tokens = count_tokens(entries)
        assert tokens > 0

    def test_count_multiple_entries(self):
        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.user_message("Hello " * 10),
            ),
            SessionEntry(
                type="message",
                id="2",
                parent_id="1",
                message=Message.assistant_message("Hi " * 20),
            ),
        ]
        tokens = count_tokens(entries)
        # Should be roughly (6*10 + 3*20) / 4 = 30
        assert tokens > 20


class TestSerializeEntries:
    """Test entry serialization."""

    def test_serialize_empty(self):
        text = serialize_entries([])
        assert text == ""

    def test_serialize_messages(self):
        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.user_message("Hello"),
            ),
            SessionEntry(
                type="message",
                id="2",
                parent_id="1",
                message=Message.assistant_message("Hi there"),
            ),
        ]
        text = serialize_entries(entries)
        assert "[user]: Hello" in text
        assert "[assistant]: Hi there" in text

    def test_serialize_with_compaction(self):
        entries = [
            SessionEntry(
                type="compaction",
                id="1",
                parent_id=None,
                summary="Previous conversation summary",
            )
        ]
        text = serialize_entries(entries)
        assert "[SYSTEM: Compaction]" in text
        assert "Previous conversation summary" in text


class TestExtractFileOperations:
    """Test file operation extraction."""

    def test_extract_empty(self):
        ops = extract_file_operations([])
        assert ops == {"read_files": [], "modified_files": []}

    def test_extract_read_file(self):
        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.assistant_message(
                    'I will read_file("/path/to/file.txt")'
                ),
            )
        ]
        ops = extract_file_operations(entries)
        assert "/path/to/file.txt" in ops["read_files"]

    def test_extract_write_file(self):
        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.assistant_message(
                    'I will write_file "/path/to/output.json"'
                ),
            )
        ]
        ops = extract_file_operations(entries)
        assert "/path/to/output.json" in ops["modified_files"]


class TestCompact:
    """Test compact function."""

    @pytest.mark.asyncio
    async def test_compact_empty_entries(self):
        result = await compact([], max_tokens=1000)
        assert result.summary == "Empty conversation"
        assert result.tokens_before == 0

    @pytest.mark.asyncio
    async def test_compact_small_conversation(self):
        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.user_message("Hello"),
            ),
            SessionEntry(
                type="message",
                id="2",
                parent_id="1",
                message=Message.assistant_message("Hi there"),
            ),
        ]
        result = await compact(entries, max_tokens=1000)
        # Small conversation, may not compact
        assert result.tokens_before > 0

    @pytest.mark.asyncio
    async def test_compact_with_model_call(self):
        async def mock_model_call(system_prompt, messages):
            return "This is a mock summary"

        entries = [
            SessionEntry(
                type="message",
                id="1",
                parent_id=None,
                message=Message.user_message("Hello " * 100),
            ),
            SessionEntry(
                type="message",
                id="2",
                parent_id="1",
                message=Message.assistant_message("Hi " * 100),
            ),
        ]
        result = await compact(entries, max_tokens=100, model_call=mock_model_call)
        assert result.summary == "This is a mock summary"
        assert result.entries_compacted > 0


class TestCompactionManager:
    """Test CompactionManager class."""

    @pytest.fixture
    def session_manager(self, tmp_path):
        """Create a session manager."""
        return SessionManager(tmp_path / "sessions")

    @pytest.fixture
    def compaction_manager(self, session_manager):
        """Create a compaction manager."""
        return CompactionManager(session_manager, compaction_threshold=1000)

    def test_init(self, session_manager):
        manager = CompactionManager(session_manager)
        assert manager.compaction_threshold == 100000
        assert manager.target_ratio == 0.5

    def test_get_token_count_empty(self, session_manager, compaction_manager):
        session_id = session_manager.create_session("tg:123456")
        count = compaction_manager.get_token_count(session_id)
        assert count == 0

    def test_get_token_count_with_messages(
        self, session_manager, compaction_manager
    ):
        session_id = session_manager.create_session("tg:123456")
        session_manager.append_message(
            session_id, Message.user_message("Hello " * 10)
        )
        count = compaction_manager.get_token_count(session_id)
        assert count > 0

    @pytest.mark.asyncio
    async def test_check_and_compact_below_threshold(
        self, session_manager, compaction_manager
    ):
        session_id = session_manager.create_session("tg:123456")
        session_manager.append_message(
            session_id, Message.user_message("Hello")
        )

        result = await compaction_manager.check_and_compact(session_id)
        assert result is None  # Below threshold

    @pytest.mark.asyncio
    async def test_check_and_compact_above_threshold(
        self, session_manager, compaction_manager
    ):
        session_id = session_manager.create_session("tg:123456")
        # Add enough content to exceed threshold
        for i in range(50):
            session_manager.append_message(
                session_id, Message.user_message(f"Message {i} " * 10)
            )
            session_manager.append_message(
                session_id, Message.assistant_message(f"Response {i} " * 10)
            )

        # Use low threshold to trigger compaction
        compaction_manager.compaction_threshold = 100
        result = await compaction_manager.check_and_compact(session_id)

        assert result is not None
        assert result.tokens_before > 0
        assert result.summary != ""

    @pytest.mark.asyncio
    async def test_check_and_compact_saves_to_session(
        self, session_manager, compaction_manager
    ):
        session_id = session_manager.create_session("tg:123456")
        for i in range(30):
            session_manager.append_message(
                session_id, Message.user_message(f"Message {i} " * 10)
            )

        compaction_manager.compaction_threshold = 50
        result = await compaction_manager.check_and_compact(session_id)

        # Verify compaction was saved
        entries = session_manager.get_session(session_id)
        compaction_entries = [e for e in entries if e.type == "compaction"]
        assert len(compaction_entries) >= 1

    def test_set_model_call(self, session_manager):
        manager = CompactionManager(session_manager)
        async def mock_call(system_prompt, messages):
            return "summary"

        manager.set_model_call(mock_call)
        assert manager.model_call is not None
