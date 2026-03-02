"""
Container runtime abstraction for Omiga Python port.

Mirrors src/container-runtime.ts — detects docker binary and manages
orphan container cleanup.
"""
from __future__ import annotations

import logging
import subprocess

logger = logging.getLogger(__name__)

# The container runtime binary
CONTAINER_RUNTIME_BIN: str = "docker"


def readonly_mount_args(host_path: str, container_path: str) -> list[str]:
    """Return CLI args for a readonly bind mount."""
    return ["-v", f"{host_path}:{container_path}:ro"]


def stop_container_cmd(name: str) -> list[str]:
    """Return command list to stop a container by name."""
    return [CONTAINER_RUNTIME_BIN, "stop", name]


def ensure_container_runtime_running() -> None:
    """Assert the container runtime is reachable; raise if not."""
    try:
        subprocess.run(
            [CONTAINER_RUNTIME_BIN, "info"],
            capture_output=True,
            timeout=10,
            check=True,
        )
        logger.debug("Container runtime already running")
    except Exception as err:
        logger.error("Failed to reach container runtime: %s", err)
        print(
            "\n╔════════════════════════════════════════════════════════════════╗\n"
            "║  FATAL: Container runtime failed to start                      ║\n"
            "║                                                                ║\n"
            "║  Agents cannot run without a container runtime. To fix:        ║\n"
            "║  1. Ensure Docker is installed and running                     ║\n"
            "║  2. Run: docker info                                           ║\n"
            "║  3. Restart Omiga                                           ║\n"
            "╚════════════════════════════════════════════════════════════════╝\n"
        )
        raise RuntimeError("Container runtime is required but failed to start") from err


def cleanup_orphans() -> None:
    """Kill orphaned Omiga containers from previous runs."""
    try:
        result = subprocess.run(
            [CONTAINER_RUNTIME_BIN, "ps", "--filter", "name=omiga-", "--format", "{{.Names}}"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        orphans = [n for n in result.stdout.strip().split("\n") if n]
        for name in orphans:
            try:
                subprocess.run(stop_container_cmd(name), capture_output=True, timeout=15)
            except Exception:
                pass  # already stopped
        if orphans:
            logger.info("Stopped %d orphaned container(s): %s", len(orphans), orphans)
    except Exception as err:
        logger.warning("Failed to clean up orphaned containers: %s", err)
