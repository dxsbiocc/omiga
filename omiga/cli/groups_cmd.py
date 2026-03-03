"""Groups command - manage registered groups."""
from __future__ import annotations

import click
import httpx

from .http import create_client, print_json, get_api_url


@click.group("groups")
def groups_cmd() -> None:
    """Manage registered groups."""
    pass


@groups_cmd.command("list")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def list_cmd(ctx: click.Context, base_url: str | None) -> None:
    """List all registered groups."""
    api_url = get_api_url(ctx, base_url)

    try:
        with create_client(api_url) as client:
            response = client.get("/groups")
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            response.raise_for_status()
            print_json(response.json())
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )


@groups_cmd.command("register")
@click.argument("jid")
@click.argument("name")
@click.option(
    "--requires-trigger",
    is_flag=True,
    default=True,
    help="Group requires trigger word to activate",
)
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def register_cmd(
    ctx: click.Context,
    jid: str,
    name: str,
    requires_trigger: bool,
    base_url: str | None,
) -> None:
    """Register a new group.

    JID is the chat identifier (e.g., tg:123456789 for Telegram).
    NAME is the display name for the group.
    """
    api_url = get_api_url(ctx, base_url)

    payload = {
        "jid": jid,
        "name": name,
        "requires_trigger": requires_trigger,
    }

    try:
        with create_client(api_url) as client:
            response = client.post("/groups", json=payload)
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            if response.status_code == 409:
                raise click.ClickException(f"Group already registered: {jid}")
            response.raise_for_status()
            click.echo(f"✓ Group registered: {name} ({jid})")
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )


@groups_cmd.command("unregister")
@click.argument("jid")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def unregister_cmd(ctx: click.Context, jid: str, base_url: str | None) -> None:
    """Unregister a group.

    JID is the chat identifier to unregister.
    """
    api_url = get_api_url(ctx, base_url)

    try:
        with create_client(api_url) as client:
            response = client.delete(f"/groups/{jid}")
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            if response.status_code == 404:
                raise click.ClickException(f"Group not registered: {jid}")
            response.raise_for_status()
            click.echo(f"✓ Group unregistered: {jid}")
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )


@groups_cmd.command("chats")
@click.option(
    "--base-url",
    default=None,
    help="Override the API base URL",
)
@click.pass_context
def chats_cmd(ctx: click.Context, base_url: str | None) -> None:
    """List all known chats (registered and unregistered)."""
    api_url = get_api_url(ctx, base_url)

    try:
        with create_client(api_url) as client:
            response = client.get("/chats")
            if response.status_code == 401:
                raise click.ClickException("Authentication failed. Check HTTP_API_TOKEN.")
            response.raise_for_status()
            print_json(response.json())
    except httpx.ConnectError:
        raise click.ClickException(
            f"Cannot connect to Omiga API at {api_url}.\n"
            "Make sure 'omiga app' is running."
        )
