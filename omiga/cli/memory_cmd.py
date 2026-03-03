"""Memory command - manage Omiga's memory system (SOPs, lessons, facts)."""
from __future__ import annotations

import click
from datetime import datetime
from pathlib import Path

from omiga.config import DATA_DIR


@click.group("memory")
def memory_cmd() -> None:
    """Manage Omiga's memory system (SOPs, lessons, facts)."""
    pass


@memory_cmd.command("status")
def status_cmd() -> None:
    """Show memory system status."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    # Initialize synchronously for CLI
    import asyncio
    asyncio.run(manager.initialize())

    stats = manager.get_memory_stats()

    click.echo("\n=== Omiga Memory Status ===\n")
    click.echo(f"L1 Index:")
    click.echo(f"  Topics: {stats['l1_topics']}/30")
    click.echo(f"  Rules:  {stats['l1_rules']}")
    click.echo(f"\nL2 Facts:")
    click.echo(f"  Sections: {stats['l2_sections']}")
    click.echo(f"\nL3 SOPs:")
    click.echo(f"  Pending:  {stats['l3_pending']} (awaiting review)")
    click.echo(f"  Active:   {stats['l3_active']} (in use)")
    click.echo(f"  Archived: {stats['l3_archived']} (historical)")
    click.echo(f"\nLessons:")
    click.echo(f"  Recorded: {stats['lessons']}")
    click.echo(f"\nScripts:")
    click.echo(f"  Generated: {stats['scripts']}")
    click.echo("")


@memory_cmd.command("list")
@click.option(
    "--pending",
    "show_pending",
    is_flag=True,
    help="Show pending SOPs awaiting review",
)
@click.option(
    "--active",
    "show_active",
    is_flag=True,
    help="Show active SOPs",
)
@click.option(
    "--all",
    "show_all",
    is_flag=True,
    help="Show all SOPs (pending + active)",
)
def list_cmd(show_pending: bool, show_active: bool, show_all: bool) -> None:
    """List SOPs by status."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    if show_all or (not show_pending and not show_active):
        show_pending = True
        show_active = True

    if show_pending:
        pending = manager.list_pending_sops()
        if pending:
            click.echo(f"\n=== Pending SOPs ({len(pending)} awaiting review) ===\n")
            click.echo(f"{'ID':<14} {'Name':<40} {'Type':<15} {'Created':<20}")
            click.echo("-" * 90)
            for sop in sorted(pending, key=lambda s: s.created_at, reverse=True):
                click.echo(
                    f"{sop.id:<14} {sop.name[:40]:<40} {sop.sop_type.value:<15} "
                    f"{sop.created_at[:19]:<20}"
                )
            click.echo("")
        else:
            click.echo("\nNo pending SOPs.\n")

    if show_active:
        active = manager.list_active_sops()
        if active:
            click.echo(f"\n=== Active SOPs ({len(active)}) ===\n")
            click.echo(f"{'ID':<14} {'Name':<40} {'Executions':<12} {'Last Run':<20}")
            click.echo("-" * 90)
            for sop in sorted(active, key=lambda s: s.executed_count, reverse=True):
                last_run = sop.last_executed_at[:19] if sop.last_executed_at else "Never"
                click.echo(
                    f"{sop.id:<14} {sop.name[:40]:<40} {sop.executed_count:<12} {last_run:<20}"
                )
            click.echo("")
        else:
            click.echo("\nNo active SOPs.\n")


@memory_cmd.command("show")
@click.argument("sop_id")
def show_cmd(sop_id: str) -> None:
    """Show details of a specific SOP."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    sop = manager.get_sop(sop_id)
    if not sop:
        raise click.ClickException(f"SOP not found: {sop_id}")

    click.echo("\n" + "=" * 70)
    click.echo(f"SOP: {sop.name}")
    click.echo("=" * 70)
    click.echo(f"\n**ID**: `{sop.id}`")
    click.echo(f"**状态**: {sop.status.value}")
    click.echo(f"**类型**: {sop.sop_type.value}")
    click.echo(f"**来源任务**: `{sop.task_id}`")
    click.echo(f"**创建时间**: {sop.created_at}")
    click.echo(f"**执行次数**: {sop.executed_count}")
    click.echo(f"**最后执行**: {sop.last_executed_at or '从未'}")

    if sop.prerequisites:
        click.echo("\n## 前置条件")
        for i, prereq in enumerate(sop.prerequisites, 1):
            click.echo(f"  {i}. {prereq}")

    if sop.steps:
        click.echo("\n## 执行步骤")
        for i, step in enumerate(sop.steps, 1):
            click.echo(f"  {i}. {step}")

    if sop.pitfalls:
        click.echo("\n## 避坑指南")
        for pitfall in sop.pitfalls:
            click.echo(f"  ⚠️ {pitfall}")

    if sop.lessons:
        click.echo("\n## 经验教训")
        for lesson in sop.lessons:
            click.echo(f"  - [{lesson.lesson_type.value}] {lesson.content[:60]}...")

    click.echo("")


@memory_cmd.command("approve")
@click.argument("sop_id")
def approve_cmd(sop_id: str) -> None:
    """Approve a pending SOP and move it to active."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    if manager.approve_sop(sop_id):
        click.echo(f"✓ SOP approved: {sop_id}")
    else:
        raise click.ClickException(
            f"SOP not found or not pending: {sop_id}"
        )


@memory_cmd.command("reject")
@click.argument("sop_id")
@click.option(
    "--reason",
    "-r",
    default="",
    help="Rejection reason",
)
def reject_cmd(sop_id: str, reason: str) -> None:
    """Reject a pending SOP."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    if manager.reject_sop(sop_id, reason):
        click.echo(f"✓ SOP rejected: {sop_id}")
        if reason:
            click.echo(f"  Reason: {reason}")
    else:
        raise click.ClickException(
            f"SOP not found or not pending: {sop_id}"
        )


@memory_cmd.command("archive")
@click.argument("sop_id")
def archive_cmd(sop_id: str) -> None:
    """Archive an active SOP."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    if manager.archive_sop(sop_id):
        click.echo(f"✓ SOP archived: {sop_id}")
    else:
        raise click.ClickException(
            f"SOP not found or not active: {sop_id}"
        )


@memory_cmd.command("cleanup")
@click.option(
    "--older-than",
    "days",
    default=90,
    help="Archive SOPs older than N days (default: 90)",
)
def cleanup_cmd(days: int) -> None:
    """Clean up old archived SOPs."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    cleaned = manager.cleanup_old_archived(days)
    click.echo(f"✓ Cleaned up {cleaned} old archived SOPs (>{days} days)")


@memory_cmd.command("index")
def index_cmd() -> None:
    """Show L1 memory index."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    index = manager.get_index()

    click.echo("\n=== L1 Memory Index ===\n")
    click.echo(f"Topics ({len(index.topics)}/30):")
    for keyword, location in sorted(index.topics.items()):
        click.echo(f"  **{keyword}** → `{location}`")

    click.echo(f"\nRules ({len(index.rules)}):")
    for rule in index.rules:
        click.echo(f"  - {rule}")

    click.echo("")


@memory_cmd.command("facts")
@click.argument("section", required=False)
def facts_cmd(section: str | None) -> None:
    """Show L2 facts database."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    facts = manager.get_facts()

    if section:
        entries = facts.get_section(section)
        if entries:
            click.echo(f"\n=== L2 Facts: {section} ({len(entries)} entries) ===\n")
            for entry in entries:
                icon = "✅" if entry.verified else "⏳"
                click.echo(f"### {entry.key} {icon}")
                click.echo(f"```")
                click.echo(entry.value[:500])  # Truncate long values
                click.echo(f"```")
                if entry.source:
                    click.echo(f"*来源*: `{entry.source}`")
                click.echo("")
        else:
            click.echo(f"\nNo facts found in section: {section}\n")
    else:
        click.echo(f"\n=== L2 Facts ({len(facts.entries)} sections) ===\n")
        for section_name in sorted(facts.entries.keys()):
            count = len(facts.entries[section_name])
            click.echo(f"  {section_name}: {count} entries")
        click.echo("")


@memory_cmd.command("lessons")
def lessons_cmd() -> None:
    """Show recorded lessons from failures."""
    from omiga.memory.manager import MemoryManager

    memory_dir = DATA_DIR / "memory"
    if not memory_dir.exists():
        click.echo("Memory system not initialized yet.")
        return

    manager = MemoryManager(memory_dir)
    import asyncio
    asyncio.run(manager.initialize())

    lessons_dir = manager.lessons_dir
    if not lessons_dir.exists() or not any(lessons_dir.glob("*.md")):
        click.echo("\nNo recorded lessons.\n")
        return

    lesson_files = list(lessons_dir.glob("*.md"))
    click.echo(f"\n=== Recorded Lessons ({len(lesson_files)}) ===\n")

    for lesson_file in sorted(lesson_files, key=lambda f: f.stat().st_mtime, reverse=True)[:20]:
        content = lesson_file.read_text(encoding="utf-8")
        # Extract title
        title_line = next((l for l in content.splitlines() if l.startswith("#")), "")
        trigger_line = next((l for l in content.splitlines() if l.startswith("**触发条件**")), "")

        click.echo(f"📌 {lesson_file.stem}")
        click.echo(f"   {title_line.replace('#', '').strip()}")
        click.echo(f"   {trigger_line}")
        click.echo("")
