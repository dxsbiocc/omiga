"""HTTP client utilities for CLI commands."""
from __future__ import annotations

import json
from typing import Any

import click
import httpx


def create_client(base_url: str) -> httpx.Client:
    """Create HTTP client with /api prefix for all requests.

    Args:
        base_url: Base URL of the API server

    Returns:
        Configured HTTP client
    """
    base = base_url.rstrip("/")
    if not base.endswith("/api"):
        base = f"{base}/api"
    return httpx.Client(base_url=base, timeout=30.0)


def print_json(data: Any) -> None:
    """Print data as formatted JSON."""
    print(json.dumps(data, ensure_ascii=False, indent=2))


def get_base_url(host: str, port: int) -> str:
    """Construct base URL from host and port."""
    return f"http://{host}:{port}"


def get_api_url(ctx: click.Context, base_url: str | None) -> str:
    """Resolve API URL from context or explicit base_url.

    Args:
        ctx: Click context with host/port
        base_url: Optional explicit base URL

    Returns:
        Full API URL with /api prefix
    """
    if base_url:
        return base_url.rstrip("/") + "/api"

    host = ctx.obj.get("host", "127.0.0.1")
    port = ctx.obj.get("port", 7891)
    return f"http://{host}:{port}/api"
