"""
Security tests for nanoclaw/group_folder.py

Verifies that path traversal, absolute paths, reserved names, and
other invalid inputs are rejected before any filesystem access.
"""
from __future__ import annotations

import pytest

from omiga.group_folder import (
    assert_valid_group_folder,
    is_valid_group_folder,
    resolve_group_folder_path,
    resolve_group_ipc_path,
)


# ---------------------------------------------------------------------------
# is_valid_group_folder — valid inputs
# ---------------------------------------------------------------------------

@pytest.mark.parametrize("name", [
    "main",
    "work",
    "my-group",
    "group_1",
    "A",
    "Group123",
    "a" * 64,           # max length (1 leading + 63 body)
])
def test_valid_folder_names(name: str):
    assert is_valid_group_folder(name) is True


# ---------------------------------------------------------------------------
# is_valid_group_folder — invalid inputs
# ---------------------------------------------------------------------------

@pytest.mark.parametrize("name", [
    "",                     # empty
    " ",                    # whitespace only
    " main",                # leading space
    "main ",                # trailing space
    "global",               # reserved (lowercase)
    "GLOBAL",               # reserved (case-insensitive)
    ".hidden",              # starts with dot
    "_underscore",          # starts with underscore
    "-dash",                # starts with dash
    "main/sub",             # path separator
    "main\\sub",            # Windows path separator
    "../etc/passwd",        # classic path traversal
    "a/../b",               # embedded path traversal
    "..",                   # double-dot
    "a" * 65,               # too long (> 64 chars)
    "hello world",          # space in middle
    "café",                 # non-ASCII
    "",                     # empty after strip (duplicate for clarity)
])
def test_invalid_folder_names(name: str):
    assert is_valid_group_folder(name) is False


# ---------------------------------------------------------------------------
# assert_valid_group_folder — raises on invalid
# ---------------------------------------------------------------------------

def test_assert_raises_on_traversal():
    with pytest.raises(ValueError):
        assert_valid_group_folder("../evil")


def test_assert_raises_on_reserved():
    with pytest.raises(ValueError):
        assert_valid_group_folder("global")


def test_assert_raises_on_empty():
    with pytest.raises(ValueError):
        assert_valid_group_folder("")


# ---------------------------------------------------------------------------
# resolve_group_folder_path — path confinement
# ---------------------------------------------------------------------------

def test_resolve_group_folder_path_stays_within_base(tmp_path, monkeypatch):
    import omiga.config as cfg
    monkeypatch.setattr(cfg, "GROUPS_DIR", tmp_path)

    result = resolve_group_folder_path("main")
    assert str(result).startswith(str(tmp_path))


def test_resolve_group_folder_path_rejects_traversal(tmp_path, monkeypatch):
    import omiga.config as cfg
    monkeypatch.setattr(cfg, "GROUPS_DIR", tmp_path)

    with pytest.raises(ValueError):
        resolve_group_folder_path("../evil")


# ---------------------------------------------------------------------------
# resolve_group_ipc_path — path confinement
# ---------------------------------------------------------------------------

def test_resolve_group_ipc_path_stays_within_ipc_base(tmp_path, monkeypatch):
    import omiga.config as cfg
    monkeypatch.setattr(cfg, "DATA_DIR", tmp_path)

    result = resolve_group_ipc_path("main")
    expected_base = str(tmp_path / "ipc")
    assert str(result).startswith(expected_base)


def test_resolve_group_ipc_path_rejects_traversal(tmp_path, monkeypatch):
    import omiga.config as cfg
    monkeypatch.setattr(cfg, "DATA_DIR", tmp_path)

    with pytest.raises(ValueError):
        resolve_group_ipc_path("../etc")


def test_resolve_group_ipc_path_rejects_reserved(tmp_path, monkeypatch):
    import omiga.config as cfg
    monkeypatch.setattr(cfg, "DATA_DIR", tmp_path)

    with pytest.raises(ValueError):
        resolve_group_ipc_path("global")
