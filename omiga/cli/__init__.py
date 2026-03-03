"""CLI entry point for Omiga."""
from __future__ import annotations

import click

from .app_cmd import app_cmd
from .config_cmd import config_cmd
from .groups_cmd import groups_cmd
from .tasks_cmd import tasks_cmd
from .skills_cmd import skills_cmd
from .memory_cmd import memory_cmd


@click.group(context_settings={"help_option_names": ["-h", "--help"]})
@click.option(
    "--host",
    default="127.0.0.1",
    help="API host (default: 127.0.0.1)",
)
@click.option(
    "--port",
    default=7891,
    type=int,
    help="API port (default: 7891)",
)
@click.pass_context
def cli(ctx: click.Context, host: str, port: int) -> None:
    """Omiga - Your Personal AI Assistant.

    A containerized AI assistant that integrates with multiple chat platforms.
    """
    ctx.ensure_object(dict)
    ctx.obj["host"] = host
    ctx.obj["port"] = port


# Register commands
cli.add_command(app_cmd)
cli.add_command(config_cmd)
cli.add_command(groups_cmd)
cli.add_command(tasks_cmd)
cli.add_command(skills_cmd)
cli.add_command(memory_cmd)


def main() -> None:
    """CLI entry point."""
    cli()


if __name__ == "__main__":
    main()
