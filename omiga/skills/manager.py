"""Skill manager for loading and managing skills."""
from __future__ import annotations

import importlib
import inspect
import logging
from pathlib import Path
from typing import Any, Optional, Type

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError

logger = logging.getLogger("omiga.skills.manager")


class SkillManager:
    """Manages skill loading, registration, and execution.

    The SkillManager is responsible for:
    - Discovering skills in the skills directory
    - Loading skill modules dynamically
    - Managing skill lifecycle (load/unload)
    - Executing skills on demand
    """

    def __init__(
        self,
        skills_dir: Path,
        context: Optional[SkillContext] = None,
    ):
        """Initialize the skill manager.

        Args:
            skills_dir: Path to the skills directory
            context: Optional skill context (created at runtime)
        """
        self.skills_dir = skills_dir
        self.context = context
        self._skills: dict[str, Skill] = {}
        self._skill_classes: dict[str, Type[Skill]] = {}

    def set_context(self, context: SkillContext) -> None:
        """Set the skill context.

        Args:
            context: The skill context to use
        """
        self.context = context
        # Re-instantiate skills with new context
        for name, skill_class in self._skill_classes.items():
            if name not in self._skills:
                self._skills[name] = skill_class(context)

    async def load_skill(self, skill_name: str) -> bool:
        """Load a skill by name.

        Args:
            skill_name: Name of the skill to load

        Returns:
            True if loaded successfully, False otherwise
        """
        if skill_name in self._skills:
            logger.debug(f"Skill '{skill_name}' already loaded")
            return True

        if skill_name not in self._skill_classes:
            # Try to discover and load the skill class
            await self._discover_skill(skill_name)

        if skill_name not in self._skill_classes:
            logger.error(f"Skill '{skill_name}' not found")
            return False

        try:
            skill_class = self._skill_classes[skill_name]
            if self.context is None:
                logger.error(f"Skill context not set for skill '{skill_name}'")
                return False
            skill = skill_class(self.context)
            await skill.on_load()
            self._skills[skill_name] = skill
            logger.info(f"Loaded skill '{skill_name}'")
            return True
        except Exception as e:
            logger.error(f"Failed to load skill '{skill_name}': {e}")
            return False

    async def load_all_skills(self) -> int:
        """Load all available skills.

        Returns:
            Number of skills loaded successfully
        """
        count = 0
        skill_names = await self.discover_available_skills()
        for name in skill_names:
            if await self.load_skill(name):
                count += 1
        return count

    async def unload_skill(self, skill_name: str) -> bool:
        """Unload a skill.

        Args:
            skill_name: Name of the skill to unload

        Returns:
            True if unloaded successfully, False otherwise
        """
        if skill_name not in self._skills:
            return False

        try:
            skill = self._skills[skill_name]
            await skill.on_unload()
            del self._skills[skill_name]
            logger.info(f"Unloaded skill '{skill_name}'")
            return True
        except Exception as e:
            logger.error(f"Failed to unload skill '{skill_name}': {e}")
            return False

    async def execute_skill(
        self,
        skill_name: str,
        **kwargs: Any,
    ) -> Any:
        """Execute a skill with trace recording.

        Uses execute_with_trace() to automatically record tool calls
        and execution logs for SOP generation.

        Args:
            skill_name: Name of the skill to execute
            **kwargs: Arguments to pass to the skill

        Returns:
            The result of the skill execution

        Raises:
            SkillError: If the skill is not loaded or execution fails
        """
        if skill_name not in self._skills:
            loaded = await self.load_skill(skill_name)
            if not loaded:
                raise SkillError(f"Skill '{skill_name}' is not available", skill_name)

        skill = self._skills[skill_name]
        try:
            # Use execute_with_trace to enable automatic trace recording
            return await skill.execute_with_trace(**kwargs)
        except Exception as e:
            raise SkillError(str(e), skill_name)

    def get_skill(self, skill_name: str) -> Optional[Skill]:
        """Get a skill instance by name.

        Args:
            skill_name: Name of the skill

        Returns:
            The skill instance or None if not found
        """
        return self._skills.get(skill_name)

    def list_loaded_skills(self) -> list[str]:
        """List all loaded skill names.

        Returns:
            List of loaded skill names
        """
        return list(self._skills.keys())

    async def discover_available_skills(self) -> list[str]:
        """Discover all available skills in the skills directory.

        Returns:
            List of available skill names
        """
        skill_names: list[str] = []

        if not self.skills_dir.exists():
            logger.warning(f"Skills directory does not exist: {self.skills_dir}")
            return skill_names

        # Look for skill directories containing SKILL.md or __init__.py
        for item in self.skills_dir.iterdir():
            if not item.is_dir():
                continue
            if item.name.startswith("_"):
                continue

            # Check for skill module
            skill_init = item / "__init__.py"
            if skill_init.exists():
                skill_names.append(item.name)

        return skill_names

    async def _discover_skill(self, skill_name: str) -> None:
        """Discover and register a skill class.

        Args:
            skill_name: Name of the skill to discover
        """
        skill_dir = self.skills_dir / skill_name
        if not skill_dir.exists():
            return

        try:
            # Import the skill module
            module_name = f"omiga.skills.{skill_name}"
            module = importlib.import_module(module_name)

            # Find the Skill subclass
            for name, obj in inspect.getmembers(module, inspect.isclass):
                if issubclass(obj, Skill) and obj is not Skill:
                    self._skill_classes[skill_name] = obj
                    logger.debug(f"Discovered skill class: {skill_name}")
                    break

        except ImportError as e:
            logger.error(f"Failed to import skill '{skill_name}': {e}")
        except Exception as e:
            logger.error(f"Failed to discover skill '{skill_name}': {e}")
