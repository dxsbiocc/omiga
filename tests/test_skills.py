"""Tests for Omiga skills system."""
import asyncio
import pytest
from pathlib import Path
from tempfile import TemporaryDirectory

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError
from omiga.skills.manager import SkillManager


class MockSkill(Skill):
    """Mock skill for testing."""

    metadata = SkillMetadata(
        name="mock",
        description="A mock skill for testing",
        emoji="🧪",
    )

    async def execute(self, value: str = "default", **kwargs) -> str:
        """Execute the mock skill."""
        return f"executed: {value}"


class TestSkillBase:
    """Tests for Skill base class."""

    def test_skill_metadata(self):
        """Test skill metadata initialization."""
        metadata = SkillMetadata(
            name="test",
            description="Test skill",
            version="2.0.0",
            author="Tester",
            emoji="📦",
            tags=["test", "mock"],
        )
        assert metadata.name == "test"
        assert metadata.version == "2.0.0"
        assert metadata.author == "Tester"
        assert metadata.emoji == "📦"
        assert metadata.tags == ["test", "mock"]

    def test_skill_context(self):
        """Test skill context creation."""
        with TemporaryDirectory() as tmpdir:
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir) / "data",
            )
            assert isinstance(context.groups_dir, Path)
            assert context.send_message is None

    @pytest.mark.asyncio
    async def test_skill_execute(self):
        """Test skill execution."""
        with TemporaryDirectory() as tmpdir:
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            skill = MockSkill(context)
            result = await skill.execute(value="test")
            assert result == "executed: test"

    @pytest.mark.asyncio
    async def test_skill_on_load_unload(self):
        """Test skill lifecycle hooks."""
        loaded = False
        unloaded = False

        class LifecycleSkill(Skill):
            metadata = SkillMetadata(
                name="lifecycle",
                description="Test lifecycle hooks",
            )

            async def execute(self, **kwargs):
                return "ok"

            async def on_load(self):
                nonlocal loaded
                loaded = True

            async def on_unload(self):
                nonlocal unloaded
                unloaded = True

        with TemporaryDirectory() as tmpdir:
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            skill = LifecycleSkill(context)
            await skill.on_load()
            assert loaded is True
            await skill.on_unload()
            assert unloaded is True

    def test_skill_error(self):
        """Test SkillError exception."""
        err = SkillError("Test error", "mock")
        assert str(err) == "[mock] Test error"
        assert err.message == "Test error"
        assert err.skill_name == "mock"

        err2 = SkillError("No skill name")
        assert str(err2) == "No skill name"


class TestSkillManager:
    """Tests for SkillManager."""

    @pytest.mark.asyncio
    async def test_manager_init(self):
        """Test skill manager initialization."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            assert manager.skills_dir == Path(tmpdir)
            assert manager._skills == {}
            assert manager.list_loaded_skills() == []

    @pytest.mark.asyncio
    async def test_manager_set_context(self):
        """Test setting context on manager."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir) / "data",
            )
            manager.set_context(context)
            assert manager.context == context

    @pytest.mark.asyncio
    async def test_manager_load_skill(self):
        """Test loading a skill."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            manager.set_context(context)

            # Register mock skill class directly (simulating discovered skill)
            manager._skill_classes["mock"] = MockSkill

            # Load the skill
            loaded = await manager.load_skill("mock")
            assert loaded is True
            assert "mock" in manager._skills

            result = await manager.execute_skill("mock")
            assert result == "executed: default"

    @pytest.mark.asyncio
    async def test_manager_execute_skill(self):
        """Test executing a skill through manager."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            manager.set_context(context)

            # Register mock skill directly
            manager._skill_classes["mock"] = MockSkill

            result = await manager.execute_skill("mock", value="hello")
            assert result == "executed: hello"

    @pytest.mark.asyncio
    async def test_manager_list_loaded(self):
        """Test listing loaded skills."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            manager.set_context(context)
            manager._skill_classes["mock"] = MockSkill

            await manager.load_skill("mock")
            skills = manager.list_loaded_skills()
            assert "mock" in skills

    @pytest.mark.asyncio
    async def test_manager_unload_skill(self):
        """Test unloading a skill."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            manager.set_context(context)
            manager._skill_classes["mock"] = MockSkill

            await manager.load_skill("mock")
            assert "mock" in manager._skills

            result = await manager.unload_skill("mock")
            assert result is True
            assert "mock" not in manager._skills

    @pytest.mark.asyncio
    async def test_manager_discover_empty_dir(self):
        """Test discovering skills in empty directory."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            skills = await manager.discover_available_skills()
            assert skills == []

    @pytest.mark.asyncio
    async def test_manager_load_nonexistent_skill(self):
        """Test loading a nonexistent skill."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            result = await manager.load_skill("nonexistent")
            assert result is False

    @pytest.mark.asyncio
    async def test_manager_execute_error(self):
        """Test executing skill that raises error."""
        with TemporaryDirectory() as tmpdir:
            manager = SkillManager(skills_dir=Path(tmpdir))
            context = SkillContext(
                groups_dir=Path(tmpdir),
                data_dir=Path(tmpdir),
            )
            manager.set_context(context)

            class ErrorSkill(Skill):
                metadata = SkillMetadata(
                    name="error",
                    description="Always fails",
                )

                async def execute(self, **kwargs):
                    raise ValueError("Intentional error")

            manager._skill_classes["error"] = ErrorSkill

            with pytest.raises(SkillError):
                await manager.execute_skill("error")
