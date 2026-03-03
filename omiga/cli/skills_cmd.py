"""Skills command - manage skills."""
from __future__ import annotations

import click
from pathlib import Path


@click.group("skills")
def skills_cmd() -> None:
    """Manage skills (list / enable / disable)."""
    pass


@skills_cmd.command("list")
@click.option(
    "--show-all",
    is_flag=True,
    help="Show all skills including disabled ones",
)
def list_cmd(show_all: bool) -> None:
    """List all available skills and their status."""
    from omiga.config import PROJECT_ROOT

    skills_dir = PROJECT_ROOT / "omiga" / "skills"

    if not skills_dir.exists():
        click.echo("No skills directory found.")
        return

    # Find all skill directories
    skill_dirs = [
        d for d in skills_dir.iterdir()
        if d.is_dir() and not d.name.startswith("_")
        and (d / "__init__.py").exists()
    ]

    if not skill_dirs:
        click.echo("No skills found.")
        return

    click.echo(f"\n{'=' * 50}")
    click.echo(f"  {'Skill Name':<20s} {'Description':<28s}")
    click.echo(f"{'=' * 50}")

    for skill_dir in sorted(skill_dirs, key=lambda x: x.name):
        skill_name = skill_dir.name
        description = get_skill_description(skill_dir) or "No description"
        click.echo(f"  {skill_name:<20s} {description:<28s}")

    click.echo(f"{'=' * 50}")
    click.echo(f"  Total: {len(skill_dirs)} skills\n")


@skills_cmd.command("info")
@click.argument("skill_name")
def info_cmd(skill_name: str) -> None:
    """Show detailed information about a skill.

    SKILL_NAME is the name of the skill to inspect.
    """
    from omiga.config import PROJECT_ROOT

    skills_dir = PROJECT_ROOT / "omiga" / "skills"
    skill_dir = skills_dir / skill_name

    if not skill_dir.exists():
        raise click.ClickException(f"Skill '{skill_name}' not found.")

    # Read SKILL.md if it exists
    skill_md = skill_dir / "SKILL.md"
    if skill_md.exists():
        content = skill_md.read_text(encoding="utf-8")
        click.echo("\n=== Skill Information ===\n")
        click.echo(content[:2000])  # Limit output
        if len(content) > 2000:
            click.echo("\n... (truncated)")
    else:
        click.echo("No SKILL.md found for this skill.")

    # Show skill source
    init_file = skill_dir / "__init__.py"
    if init_file.exists():
        click.echo(f"\nSource: {init_file}")


@skills_cmd.command("create")
@click.argument("skill_name")
def create_cmd(skill_name: str) -> None:
    """Create a new skill scaffold.

    SKILL_NAME is the name of the new skill to create.
    """
    from omiga.config import PROJECT_ROOT

    skills_dir = PROJECT_ROOT / "omiga" / "skills"
    skill_dir = skills_dir / skill_name

    if skill_dir.exists():
        raise click.ClickException(f"Skill '{skill_name}' already exists.")

    # Create skill directory
    skill_dir.mkdir(parents=True, exist_ok=True)

    # Create SKILL.md template
    skill_md = skill_dir / "SKILL.md"
    skill_md.write_text(f"""# {skill_name}

## Description

TODO: Add skill description

## Usage

TODO: Add usage examples

## Configuration

TODO: Add configuration options if any
""", encoding="utf-8")

    # Create __init__.py template
    init_file = skill_dir / "__init__.py"
    init_content = f'''"""{skill_name} skill."""
from __future__ import annotations

import logging
from typing import Any, Optional

from omiga.skills.base import Skill, SkillContext, SkillMetadata

logger = logging.getLogger("omiga.skills.{skill_name}")


class {skill_name.capitalize()}Skill(Skill):
    """{skill_name} skill implementation."""

    @property
    def metadata(self) -> SkillMetadata:
        return SkillMetadata(
            name="{skill_name}",
            description="TODO: Add description",
            version="0.1.0",
        )

    async def on_load(self) -> None:
        """Called when the skill is loaded."""
        logger.info(f"Skill '{{self.metadata.name}}' loaded")

    async def on_unload(self) -> None:
        """Called when the skill is unloaded."""
        logger.info(f"Skill '{{self.metadata.name}}' unloaded")

    async def execute(self, **kwargs: Any) -> Any:
        """Execute the skill.

        Args:
            **kwargs: Skill arguments

        Returns:
            The result of the skill execution
        """
        logger.info(f"Executing skill '{{self.metadata.name}}'")
        # TODO: Implement skill logic
        return "Skill executed successfully"


# Factory function for creating skill instance
def create_skill(context: Optional[SkillContext] = None) -> Skill:
    """Create a new instance of the skill.

    Args:
        context: Optional skill context

    Returns:
        A new skill instance
    """
    return {skill_name.capitalize()}Skill(context)
'''
    init_file.write_text(init_content, encoding="utf-8")

    click.echo(f"✓ Skill scaffold created at {skill_dir}")
    click.echo("\nNext steps:")
    click.echo(f"  1. Edit {skill_md.relative_to(PROJECT_ROOT)} to add description")
    click.echo(f"  2. Edit {init_file.relative_to(PROJECT_ROOT)} to implement logic")


def get_skill_description(skill_dir: Path) -> str | None:
    """Extract description from SKILL.md frontmatter."""
    skill_md = skill_dir / "SKILL.md"
    if not skill_md.exists():
        return None

    content = skill_md.read_text(encoding="utf-8")

    # Parse YAML frontmatter for description
    if content.startswith("---"):
        lines = content.splitlines()
        for line in lines[1:]:  # Skip first ---
            if line.strip() == "---":
                break
            if line.startswith("description:"):
                desc = line.split(":", 1)[1].strip().strip('"\'')
                return desc[:50] if desc else None

    return None
