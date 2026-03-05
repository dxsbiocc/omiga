"""Tests for AgentMemory."""
import pytest
from pathlib import Path

from omiga.memory.agent_memory import AgentMemory
from omiga.memory.manager import MemoryManager
from omiga.agent.session import Message


class TestAgentMemory:
    """Test AgentMemory class."""

    def test_init_default(self):
        """Test default initialization."""
        memory = AgentMemory()
        assert memory.max_messages == 100
        assert memory.messages == []
        assert memory.working_context == {}
        assert memory.session_id is None

    def test_init_custom(self):
        """Test custom initialization."""
        memory = AgentMemory(max_messages=50, session_id="test-123")
        assert memory.max_messages == 50
        assert memory.session_id == "test-123"

    def test_add_message(self):
        """Test adding messages."""
        memory = AgentMemory()
        memory.add_message(Message.user_message("Hello"))
        assert len(memory.messages) == 1
        assert memory.messages[0].content == "Hello"
        assert memory.messages[0].role == "user"

    def test_add_message_auto_prune(self):
        """Test automatic pruning when exceeding max_messages."""
        memory = AgentMemory(max_messages=5)

        # Add more messages than max
        for i in range(10):
            memory.add_message(Message.user_message(f"Message {i}"))

        # Should only keep last 5
        assert len(memory.messages) == 5
        assert memory.messages[0].content == "Message 5"
        assert memory.messages[-1].content == "Message 9"

    def test_get_recent_messages_all(self):
        """Test getting all recent messages."""
        memory = AgentMemory()
        memory.add_message(Message.user_message("First"))
        memory.add_message(Message.assistant_message("Second"))
        memory.add_message(Message.user_message("Third"))

        messages = memory.get_recent_messages()
        assert len(messages) == 3
        assert messages[0].content == "First"
        assert messages[-1].content == "Third"

    def test_get_recent_messages_limited(self):
        """Test getting limited recent messages."""
        memory = AgentMemory()
        for i in range(10):
            memory.add_message(Message.user_message(f"Message {i}"))

        recent = memory.get_recent_messages(3)
        assert len(recent) == 3
        assert recent[0].content == "Message 7"
        assert recent[-1].content == "Message 9"

    def test_get_recent_messages_zero(self):
        """Test getting zero messages."""
        memory = AgentMemory()
        memory.add_message(Message.user_message("Hello"))
        assert memory.get_recent_messages(0) == []

    def test_working_context(self):
        """Test working context operations."""
        memory = AgentMemory()

        # Set context
        memory.set_context("key1", "value1")
        memory.set_context("key2", {"nested": "data"})

        # Get context
        assert memory.get_context("key1") == "value1"
        assert memory.get_context("key2") == {"nested": "data"}
        assert memory.get_context("nonexistent", "default") == "default"

    def test_clear_context_specific(self):
        """Test clearing specific context key."""
        memory = AgentMemory()
        memory.set_context("key1", "value1")
        memory.set_context("key2", "value2")

        memory.clear_context("key1")
        assert memory.get_context("key1") is None
        assert memory.get_context("key2") == "value2"

    def test_clear_context_all(self):
        """Test clearing all context."""
        memory = AgentMemory()
        memory.set_context("key1", "value1")
        memory.set_context("key2", "value2")

        memory.clear_context()
        assert memory.get_context("key1") is None
        assert memory.get_context("key2") is None

    def test_clear_messages(self):
        """Test clearing messages only."""
        memory = AgentMemory()
        memory.add_message(Message.user_message("Hello"))
        memory.set_context("key", "value")

        memory.clear_messages()
        assert len(memory.messages) == 0
        assert memory.get_context("key") == "value"

    def test_clear_all(self):
        """Test clearing all memory."""
        memory = AgentMemory()
        memory.add_message(Message.user_message("Hello"))
        memory.set_context("key", "value")

        memory.clear()
        assert len(memory.messages) == 0
        assert memory.get_context("key") is None

    def test_message_count(self):
        """Test message count."""
        memory = AgentMemory()
        assert memory.get_message_count() == 0

        memory.add_message(Message.user_message("Hello"))
        assert memory.get_message_count() == 1

        memory.add_message(Message.assistant_message("Hi"))
        assert memory.get_message_count() == 2

    def test_is_near_capacity(self):
        """Test capacity check."""
        memory = AgentMemory(max_messages=10)

        # Not near capacity (default 80% threshold)
        for i in range(5):
            memory.add_message(Message.user_message(f"Msg {i}"))
        assert not memory.is_near_capacity()

        # Near capacity
        for i in range(3):
            memory.add_message(Message.user_message(f"Msg {i+5}"))
        assert memory.is_near_capacity()

        # At capacity
        for i in range(2):
            memory.add_message(Message.user_message(f"Msg {i+8}"))
        assert memory.is_near_capacity()

    def test_is_near_capacity_custom_threshold(self):
        """Test capacity check with custom threshold."""
        memory = AgentMemory(max_messages=10)

        for i in range(5):
            memory.add_message(Message.user_message(f"Msg {i}"))

        # 50% threshold should trigger
        assert memory.is_near_capacity(threshold=0.5)
        # 60% threshold should not trigger
        assert not memory.is_near_capacity(threshold=0.6)

    def test_to_dict_list(self):
        """Test converting messages to dict list."""
        memory = AgentMemory()
        memory.add_message(Message.user_message("Hello"))
        memory.add_message(Message.assistant_message("Hi there"))

        dict_list = memory.to_dict_list()
        assert len(dict_list) == 2
        assert dict_list[0]["role"] == "user"
        assert dict_list[0]["content"] == "Hello"
        assert dict_list[1]["role"] == "assistant"
        assert dict_list[1]["content"] == "Hi there"

    def test_store_fact(self):
        """Test storing facts for later sync."""
        memory = AgentMemory()

        memory.store_fact("paths", "config_dir", "/path/to/config")
        memory.store_fact("paths", "data_dir", "/path/to/data")
        memory.store_fact("config", "timeout", "30s")

        facts = memory.working_context.get("facts_to_store", {})
        assert "paths" in facts
        assert "config" in facts
        assert facts["paths"]["config_dir"] == "/path/to/config"
        assert facts["paths"]["data_dir"] == "/path/to/data"
        assert facts["config"]["timeout"] == "30s"

    def test_sync_to_long_term(self, tmp_path):
        """Test syncing facts to long-term memory."""
        memory_manager = MemoryManager(tmp_path / "memory")

        # Initialize async
        import asyncio
        asyncio.run(memory_manager.initialize())

        memory = AgentMemory(session_id="test-session")
        memory.store_fact("paths", "config_dir", "/path/to/config")
        memory.store_fact("config", "timeout", "30s")

        # Sync to long-term
        memory.sync_to_long_term(memory_manager)

        # Verify facts were stored
        facts_db = memory_manager.get_facts()
        paths_entry = facts_db.get("paths", "config_dir")
        config_entry = facts_db.get("config", "timeout")

        assert paths_entry is not None
        assert paths_entry.value == "/path/to/config"
        assert paths_entry.verified is True
        assert paths_entry.source == "test-session"

        assert config_entry is not None
        assert config_entry.value == "30s"

        # Verify facts were cleared from working context
        assert "facts_to_store" not in memory.working_context


class TestAgentMemoryIntegration:
    """Test AgentMemory integration with MemoryManager."""

    @pytest.fixture
    def memory_manager(self, tmp_path):
        """Create a memory manager."""
        import asyncio
        manager = MemoryManager(tmp_path / "memory")
        asyncio.run(manager.initialize())
        return manager

    def test_get_all_facts(self, memory_manager):
        """Test retrieving all facts from long-term memory."""
        # Add facts
        memory_manager.add_fact("paths", "config", "/path/config", "source-1")
        memory_manager.add_fact("paths", "data", "/path/data", "source-1")
        memory_manager.add_fact("credentials", "api_key", "secret", "source-2")

        # Create agent memory and retrieve facts
        memory = AgentMemory()
        facts = memory.get_all_facts(memory_manager)

        assert "paths" in facts
        assert "credentials" in facts
        assert facts["paths"]["config"] == "/path/config"
        assert facts["paths"]["data"] == "/path/data"
        assert facts["credentials"]["api_key"] == "secret"

    def test_get_active_sops(self, memory_manager):
        """Test retrieving active SOPs."""
        # Should return empty list initially
        memory = AgentMemory()
        sops = memory.get_active_sops(memory_manager)
        assert sops == []

    def test_find_sop_by_keyword(self, memory_manager):
        """Test finding SOP by keyword."""
        memory = AgentMemory()

        # Should return None when no SOPs exist
        result = memory.find_sop(memory_manager, "nonexistent")
        assert result is None
