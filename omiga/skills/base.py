"""Base classes for Omiga skills."""
from __future__ import annotations

import logging
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional, TYPE_CHECKING

logger = logging.getLogger(__name__)

if TYPE_CHECKING:
    from omiga.memory.manager import MemoryManager


@dataclass
class SkillMetadata:
    """Metadata describing a skill."""

    name: str
    description: str
    version: str = "1.0.0"
    author: str = ""
    emoji: str = "🔧"
    tags: list[str] = field(default_factory=list)
    dependencies: list[str] = field(default_factory=list)


@dataclass
class SkillContext:
    """Context provided to skills for interacting with Omiga.

    Attributes:
        groups_dir: Path to the groups directory
        data_dir: Path to the data directory
        send_message: Async callback to send messages to a channel
        get_registered_groups: Get all registered groups
        memory_manager: Optional memory manager for SOP generation
    """

    groups_dir: Path
    data_dir: Path
    send_message: Any = None  # Callable[[str, str], Awaitable[None]]
    get_registered_groups: Any = None  # Callable[[], dict[str, Any]]
    memory_manager: Optional["MemoryManager"] = None


class Skill(ABC):
    """Base class for all Omiga skills.

    Skills are the building blocks that extend Omiga's capabilities.
    Each skill should:
    - Have a unique name
    - Provide a clear description
    - Implement execute() method
    - Handle errors gracefully
    """

    metadata: SkillMetadata

    def __init__(self, context: SkillContext):
        """Initialize the skill with context.

        Args:
            context: The skill context for interacting with Omiga
        """
        self.context = context
        self.logger = logging.getLogger(f"omiga.skills.{self.__class__.__name__}")

    @property
    def name(self) -> str:
        """Return the skill name."""
        return self.metadata.name

    @property
    def description(self) -> str:
        """Return the skill description."""
        return self.metadata.description

    @abstractmethod
    async def execute(self, **kwargs: Any) -> Any:
        """Execute the skill.

        Args:
            **kwargs: Skill-specific arguments

        Returns:
            The result of the skill execution

        Raises:
            SkillError: If the skill fails to execute
        """
        pass

    async def on_load(self) -> None:
        """Called when the skill is loaded.

        Override this method for initialization logic.
        """
        pass

    async def on_unload(self) -> None:
        """Called when the skill is unloaded.

        Override this method for cleanup logic.
        """
        pass

    def get_usage(self) -> str:
        """Return usage instructions for this skill.

        Returns:
            A string with usage instructions
        """
        return f"Skill: {self.name}\nDescription: {self.description}"


class SkillError(Exception):
    """Exception raised by skills."""

    def __init__(self, message: str, skill_name: Optional[str] = None):
        self.message = message
        self.skill_name = skill_name
        super().__init__(self.message)

    def __str__(self) -> str:
        if self.skill_name:
            return f"[{self.skill_name}] {self.message}"
        return self.message
