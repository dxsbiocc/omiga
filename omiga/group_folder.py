"""
Group folder path safety validation.

Only allows folder names matching [A-Za-z0-9][A-Za-z0-9_-]{0,63}.
Prevents path traversal by rejecting '..' and absolute components.
Mirrors src/group-folder.ts exactly.
"""
import re
from pathlib import Path

import omiga.config as _cfg

_GROUP_FOLDER_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$")
_RESERVED_FOLDERS: frozenset[str] = frozenset({"global"})


def is_valid_group_folder(folder: str) -> bool:
    """Return True if *folder* is a safe, allowed group folder name."""
    if not folder:
        return False
    if folder != folder.strip():
        return False
    if not _GROUP_FOLDER_PATTERN.match(folder):
        return False
    if "/" in folder or "\\" in folder:
        return False
    if ".." in folder:
        return False
    if folder.lower() in _RESERVED_FOLDERS:
        return False
    return True


def assert_valid_group_folder(folder: str) -> None:
    if not is_valid_group_folder(folder):
        raise ValueError(f'Invalid group folder "{folder}"')


def _ensure_within_base(base_dir: Path, resolved: Path) -> None:
    try:
        resolved.relative_to(base_dir)
    except ValueError:
        raise ValueError(f"Path escapes base directory: {resolved}")


def resolve_group_folder_path(folder: str) -> Path:
    """Return the absolute path for a group's workspace directory."""
    assert_valid_group_folder(folder)
    groups_dir = _cfg.GROUPS_DIR
    group_path = (groups_dir / folder).resolve()
    _ensure_within_base(groups_dir, group_path)
    return group_path


def resolve_group_ipc_path(folder: str) -> Path:
    """Return the absolute path for a group's IPC directory."""
    assert_valid_group_folder(folder)
    ipc_base = (_cfg.DATA_DIR / "ipc").resolve()
    ipc_path = (ipc_base / folder).resolve()
    _ensure_within_base(ipc_base, ipc_path)
    return ipc_path
