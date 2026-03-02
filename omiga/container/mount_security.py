"""
Mount security module for Omiga Python port.

Validates additional mounts against an allowlist stored OUTSIDE the project
root so container agents cannot modify security configuration.

Allowlist: ~/.config/omiga/mount-allowlist.json
Mirrors src/mount-security.ts exactly.
"""
from __future__ import annotations

import json
import logging
import os
from pathlib import Path
from typing import Optional

from omiga.config import MOUNT_ALLOWLIST_PATH
from omiga.models import AdditionalMount, AllowedRoot, MountAllowlist, VolumeMount

logger = logging.getLogger(__name__)

_DEFAULT_BLOCKED_PATTERNS: list[str] = [
    ".ssh", ".gnupg", ".gpg", ".aws", ".azure", ".gcloud", ".kube", ".docker",
    "credentials", ".env", ".netrc", ".npmrc", ".pypirc",
    "id_rsa", "id_ed25519", "private_key", ".secret",
]

_cached_allowlist: Optional[MountAllowlist] = None
_allowlist_load_error: Optional[str] = None


def load_mount_allowlist() -> Optional[MountAllowlist]:
    """Load (and cache) the mount allowlist. Returns None if missing or invalid."""
    global _cached_allowlist, _allowlist_load_error

    if _cached_allowlist is not None:
        return _cached_allowlist
    if _allowlist_load_error is not None:
        return None

    try:
        p = Path(MOUNT_ALLOWLIST_PATH)
        if not p.exists():
            _allowlist_load_error = f"Mount allowlist not found at {p}"
            logger.warning(
                "Mount allowlist not found at %s — additional mounts will be BLOCKED. "
                "Create the file to enable additional mounts.",
                p,
            )
            return None

        raw = json.loads(p.read_text())

        allowed_roots = [
            AllowedRoot(
                path=r["path"],
                allow_read_write=r.get("allowReadWrite", False),
                description=r.get("description"),
            )
            for r in raw["allowedRoots"]
        ]

        blocked = list({*_DEFAULT_BLOCKED_PATTERNS, *raw.get("blockedPatterns", [])})
        non_main_ro = raw.get("nonMainReadOnly", True)

        _cached_allowlist = MountAllowlist(
            allowed_roots=allowed_roots,
            blocked_patterns=blocked,
            non_main_read_only=non_main_ro,
        )
        logger.info(
            "Mount allowlist loaded: %d allowed roots, %d blocked patterns",
            len(allowed_roots),
            len(blocked),
        )
        return _cached_allowlist

    except Exception as err:
        _allowlist_load_error = str(err)
        logger.error("Failed to load mount allowlist: %s — additional mounts BLOCKED", err)
        return None


def _expand_path(p: str) -> Path:
    home = os.environ.get("HOME", str(Path.home()))
    if p.startswith("~/"):
        return Path(home) / p[2:]
    if p == "~":
        return Path(home)
    return Path(p).resolve()


def _real_path(p: Path) -> Optional[Path]:
    try:
        return p.resolve(strict=True)
    except (OSError, RuntimeError):
        return None


def _matches_blocked(real_path: Path, patterns: list[str]) -> Optional[str]:
    parts = real_path.parts
    path_str = str(real_path)
    for pattern in patterns:
        for part in parts:
            if part == pattern or pattern in part:
                return pattern
        if pattern in path_str:
            return pattern
    return None


def _find_allowed_root(real_path: Path, allowed_roots: list[AllowedRoot]) -> Optional[AllowedRoot]:
    for root in allowed_roots:
        expanded = _expand_path(root.path)
        real_root = _real_path(expanded)
        if real_root is None:
            continue
        try:
            real_path.relative_to(real_root)
            return root
        except ValueError:
            pass
    return None


def _is_valid_container_path(cp: str) -> bool:
    if not cp or not cp.strip():
        return False
    if ".." in cp:
        return False
    if cp.startswith("/"):
        return False
    return True


def validate_mount(mount: AdditionalMount, is_main: bool) -> dict:
    """
    Validate a single additional mount.

    Returns dict with keys:
      allowed (bool), reason (str),
      real_host_path (Path|None), resolved_container_path (str|None), effective_readonly (bool|None)
    """
    allowlist = load_mount_allowlist()
    if allowlist is None:
        return {"allowed": False, "reason": f"No mount allowlist configured at {MOUNT_ALLOWLIST_PATH}"}

    import os
    container_path = mount.container_path or os.path.basename(mount.host_path)

    if not _is_valid_container_path(container_path):
        return {
            "allowed": False,
            "reason": f'Invalid container path: "{container_path}" — must be relative, non-empty, no ".."',
        }

    expanded = _expand_path(mount.host_path)
    real = _real_path(expanded)
    if real is None:
        return {"allowed": False, "reason": f'Host path does not exist: "{mount.host_path}"'}

    blocked_match = _matches_blocked(real, allowlist.blocked_patterns)
    if blocked_match is not None:
        return {"allowed": False, "reason": f'Path matches blocked pattern "{blocked_match}": "{real}"'}

    allowed_root = _find_allowed_root(real, allowlist.allowed_roots)
    if allowed_root is None:
        roots_str = ", ".join(str(_expand_path(r.path)) for r in allowlist.allowed_roots)
        return {"allowed": False, "reason": f'Path "{real}" is not under any allowed root. Allowed: {roots_str}'}

    requested_rw = mount.readonly is False
    effective_readonly = True
    if requested_rw:
        if not is_main and allowlist.non_main_read_only:
            logger.info("Mount forced read-only for non-main group: %s", mount.host_path)
        elif not allowed_root.allow_read_write:
            logger.info("Mount forced read-only — root disallows read-write: %s", mount.host_path)
        else:
            effective_readonly = False

    desc = f" ({allowed_root.description})" if allowed_root.description else ""
    return {
        "allowed": True,
        "reason": f'Allowed under root "{allowed_root.path}"{desc}',
        "real_host_path": real,
        "resolved_container_path": container_path,
        "effective_readonly": effective_readonly,
    }


def validate_additional_mounts(
    mounts: list[AdditionalMount],
    group_name: str,
    is_main: bool,
) -> list[VolumeMount]:
    """Validate all additional mounts; return only those that passed."""
    validated: list[VolumeMount] = []
    for mount in mounts:
        result = validate_mount(mount, is_main)
        if result["allowed"]:
            validated.append(
                VolumeMount(
                    host_path=str(result["real_host_path"]),
                    container_path=f"/workspace/extra/{result['resolved_container_path']}",
                    readonly=result["effective_readonly"],
                )
            )
            logger.debug(
                "Mount validated for group %s: %s -> %s (ro=%s)",
                group_name,
                result["real_host_path"],
                result["resolved_container_path"],
                result["effective_readonly"],
            )
        else:
            logger.warning(
                "Additional mount REJECTED for group %s: %s — %s",
                group_name,
                mount.host_path,
                result["reason"],
            )
    return validated
