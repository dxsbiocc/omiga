"""Omiga memory system - three-layer memory architecture for controlled growth.

This module implements a memory system inspired by pc-agent-loop's SOP philosophy,
but with a review mechanism to ensure stability:

Memory Architecture:
    L1: index.md - Navigation index (≤30 entries) + rules
    L2: facts.md - Global environment facts (paths, configs, credentials)
    L3: SOPs - Standard Operating Procedures with lifecycle management
        - pending/: Awaiting review
        - active/: Approved and in use
        - archived/: Historical records
        - lessons/: Lessons learned from failures

Core Principles:
    1. Action-Verified Only: No Execution, No Memory
    2. Reviewed Growth: New SOPs require review before activation
    3. Stability First: Core skills remain unaffected
    4. Explainability: Every memory has a source task ID
"""

from omiga.memory.manager import MemoryManager
from omiga.memory.models import (
    FactEntry,
    FactsDatabase,
    Lesson,
    LessonType,
    MemoryIndex,
    SOP,
    SOPStatus,
    SOPType,
)
from omiga.memory.sop_generator import SOPGenerator, TaskExecution

__all__ = [
    # Manager
    "MemoryManager",
    # Models
    "SOP",
    "SOPStatus",
    "SOPType",
    "Lesson",
    "LessonType",
    "MemoryIndex",
    "FactEntry",
    "FactsDatabase",
    # Generator
    "SOPGenerator",
    "TaskExecution",
]
