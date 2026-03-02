"""Skills system for Omiga.

This module provides a lightweight skill framework for extending Omiga's capabilities.
"""
from omiga.skills.base import Skill, SkillContext
from omiga.skills.manager import SkillManager

__all__ = ["Skill", "SkillContext", "SkillManager"]
