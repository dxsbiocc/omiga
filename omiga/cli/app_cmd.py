"""App command - start the Omiga server."""
from __future__ import annotations

import logging
import os

import click

from omiga.config import HTTP_API_HOST, HTTP_API_PORT

logger = logging.getLogger("omiga.cli")


@click.command("app")
@click.option(
    "--host",
    default=None,
    help="Bind host (overrides global --host)",
)
@click.option(
    "--port",
    default=None,
    type=int,
    help="Bind port (overrides global --port)",
)
@click.option(
    "--reload",
    is_flag=True,
    help="Enable auto-reload (dev only)",
)
@click.option(
    "--workers",
    default=1,
    type=int,
    show_default=True,
    help="Number of worker processes",
)
@click.option(
    "--log-level",
    default="info",
    type=click.Choice(
        ["critical", "error", "warning", "info", "debug"],
        case_sensitive=False,
    ),
    show_default=True,
    help="Log level",
)
def app_cmd(
    host: str | None,
    port: int | None,
    reload: bool,
    workers: int,
    log_level: str,
) -> None:
    """Run Omiga server.

    Starts the main Omiga application with all subsystems:
    - Message processing
    - Task scheduler
    - IPC watcher
    - HTTP API server (with Web Console)

    The server runs until interrupted (Ctrl+C).

    Open http://localhost:<port>/console in your browser to access the Web Console.
    """
    from omiga.main import main as run_omiga

    # Set log level
    os.environ["LOG_LEVEL"] = log_level

    # Override host/port if provided
    if host:
        os.environ["HTTP_API_HOST"] = host
    if port:
        os.environ["HTTP_API_PORT"] = str(port)

    api_host = host or HTTP_API_HOST
    api_port = port or HTTP_API_PORT

    click.echo(f"Starting Omiga...")
    click.echo(f"Log level: {log_level}")
    click.echo(f"Web Console: http://{api_host}:{api_port}/console")
    click.echo(f"HTTP API: http://{api_host}:{api_port}/api")
    click.echo("")
    click.echo("Press Ctrl+C to stop")
    click.echo("")

    try:
        run_omiga()
    except KeyboardInterrupt:
        click.echo("\nShutting down...")
