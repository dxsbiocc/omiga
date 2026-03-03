"""Tasks command - manage scheduled tasks."""
from __future__ import annotations

import click
import httpx

from .http import create_client, print_json, get_api_url


@click.group("tasks")
def tasks_cmd() -> None:
    """Manage scheduled tasks."""
    pass


@tasks_cmd.command("list")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def list_cmd(ctx: click.Context, base_url: str | None) -> None:
    """List all scheduled tasks."""
    api_url = get_api_url(ctx, base_url)

    try:
        with create_client(api_url) as client:
            response = client.get("/tasks")
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            response.raise_for_status()
            print_json(response.json())
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )


@tasks_cmd.command("run")
@click.argument("task_id")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def run_cmd(ctx: click.Context, task_id: str, base_url: str | None) -> None:
    """Trigger a task to run immediately."""
    api_url = get_api_url(ctx, base_url)

    try:
        with create_client(api_url) as client:
            response = client.post(f"/tasks/{task_id}/run")
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            if response.status_code == 404:
                raise click.ClickException(f"Task not found: {task_id}")
            response.raise_for_status()
            click.echo(f"✓ Task triggered: {task_id}")
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )


@tasks_cmd.command("pause")
@click.argument("task_id")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def pause_cmd(ctx: click.Context, task_id: str, base_url: str | None) -> None:
    """Pause a scheduled task."""
    click.echo("Note: Task pause/resume via IPC requires the scheduler to support it.")
    click.echo("For now, tasks are managed via the IPC mechanism.")


@tasks_cmd.command("status")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def status_cmd(ctx: click.Context, base_url: str | None) -> None:
    """Show system status including uptime and connected channels."""
    api_url = get_api_url(ctx, base_url)

    try:
        with create_client(api_url) as client:
            response = client.get("/status")
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            response.raise_for_status()
            data = response.json()
            click.echo("\n=== Omiga Status ===")
            click.echo(f"Service:  {data.get('service', 'omiga')}")
            click.echo(f"Uptime:   {data.get('uptime', 'N/A')}")
            click.echo(f"Time:     {data.get('time', 'N/A')}")
            click.echo(f"Groups:   {data.get('registered_groups', 0)} registered")
            click.echo("\nChannels:")
            for ch in data.get("channels", []):
                status_icon = "✓" if ch.get("connected") else "✗"
                click.echo(f"  {status_icon} {ch.get('name', 'Unknown')}")
            click.echo("")
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )
