"""
Tests for nanoclaw/mount_security.py

Validates the mount allowlist loading and per-mount validation logic without
touching real filesystem paths.
"""
from __future__ import annotations

import json
from pathlib import Path

import pytest

import omiga.container.mount_security as ms_mod
from omiga.container.mount_security import (
    _is_valid_container_path,
    _matches_blocked,
    load_mount_allowlist,
    validate_mount,
)
from omiga.models import AdditionalMount, AllowedRoot, MountAllowlist


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _reset_cache():
    ms_mod._cached_allowlist = None
    ms_mod._allowlist_load_error = None


@pytest.fixture(autouse=True)
def clear_allowlist_cache():
    """Reset in-memory cache before every test."""
    _reset_cache()
    yield
    _reset_cache()


def _write_allowlist(tmp_path: Path, data: dict) -> Path:
    p = tmp_path / "mount-allowlist.json"
    p.write_text(json.dumps(data))
    return p


# ---------------------------------------------------------------------------
# _is_valid_container_path
# ---------------------------------------------------------------------------

@pytest.mark.parametrize("cp", [
    "data",
    "some/nested/path",
    "file.txt",
])
def test_valid_container_paths(cp: str):
    assert _is_valid_container_path(cp) is True


@pytest.mark.parametrize("cp", [
    "",
    "  ",
    "../escape",
    "a/../b",
    "/absolute/path",
])
def test_invalid_container_paths(cp: str):
    assert _is_valid_container_path(cp) is False


# ---------------------------------------------------------------------------
# _matches_blocked
# ---------------------------------------------------------------------------

def test_matches_blocked_ssh_key():
    blocked = ms_mod._DEFAULT_BLOCKED_PATTERNS
    result = _matches_blocked(Path("/home/user/.ssh/id_rsa"), blocked)
    assert result is not None


def test_matches_blocked_aws_credentials():
    blocked = ms_mod._DEFAULT_BLOCKED_PATTERNS
    result = _matches_blocked(Path("/home/user/.aws/credentials"), blocked)
    assert result is not None


def test_matches_blocked_clean_path():
    blocked = ms_mod._DEFAULT_BLOCKED_PATTERNS
    result = _matches_blocked(Path("/home/user/projects/myapp"), blocked)
    assert result is None


# ---------------------------------------------------------------------------
# load_mount_allowlist — missing file
# ---------------------------------------------------------------------------

def test_load_allowlist_missing_returns_none(tmp_path, monkeypatch):
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(tmp_path / "nonexistent.json"))
    result = load_mount_allowlist()
    assert result is None


# ---------------------------------------------------------------------------
# load_mount_allowlist — valid file
# ---------------------------------------------------------------------------

def test_load_allowlist_valid(tmp_path, monkeypatch):
    p = _write_allowlist(tmp_path, {
        "allowedRoots": [{"path": str(tmp_path), "allowReadWrite": True, "description": "Test"}],
        "blockedPatterns": ["secret"],
        "nonMainReadOnly": False,
    })
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    result = load_mount_allowlist()
    assert result is not None
    assert len(result.allowed_roots) == 1
    assert result.allowed_roots[0].allow_read_write is True
    assert result.non_main_read_only is False
    assert "secret" in result.blocked_patterns


def test_load_allowlist_merges_default_blocked_patterns(tmp_path, monkeypatch):
    p = _write_allowlist(tmp_path, {
        "allowedRoots": [{"path": str(tmp_path)}],
        "blockedPatterns": ["my_custom_secret"],
    })
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    result = load_mount_allowlist()
    assert "my_custom_secret" in result.blocked_patterns
    assert ".ssh" in result.blocked_patterns   # default still present


def test_load_allowlist_cached_on_second_call(tmp_path, monkeypatch):
    p = _write_allowlist(tmp_path, {"allowedRoots": [{"path": str(tmp_path)}]})
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    first = load_mount_allowlist()
    second = load_mount_allowlist()
    assert first is second          # same object → cache hit


# ---------------------------------------------------------------------------
# validate_mount — no allowlist
# ---------------------------------------------------------------------------

def test_validate_mount_blocked_when_no_allowlist(tmp_path, monkeypatch):
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(tmp_path / "nope.json"))
    mount = AdditionalMount(host_path=str(tmp_path), container_path="data")
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is False
    assert "allowlist" in result["reason"].lower() or "No mount allowlist" in result["reason"]


# ---------------------------------------------------------------------------
# validate_mount — path does not exist
# ---------------------------------------------------------------------------

def test_validate_mount_nonexistent_host_path(tmp_path, monkeypatch):
    p = _write_allowlist(tmp_path, {"allowedRoots": [{"path": str(tmp_path)}]})
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(tmp_path / "does_not_exist"), container_path="data")
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is False
    assert "does not exist" in result["reason"]


# ---------------------------------------------------------------------------
# validate_mount — blocked pattern
# ---------------------------------------------------------------------------

def test_validate_mount_blocked_pattern_rejected(tmp_path, monkeypatch):
    ssh_dir = tmp_path / ".ssh"
    ssh_dir.mkdir()
    p = _write_allowlist(tmp_path, {"allowedRoots": [{"path": str(tmp_path)}]})
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(ssh_dir), container_path="keys")
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is False
    assert "blocked pattern" in result["reason"]


# ---------------------------------------------------------------------------
# validate_mount — path not under any allowed root
# ---------------------------------------------------------------------------

def test_validate_mount_outside_allowed_root_rejected(tmp_path, monkeypatch):
    allowed_dir = tmp_path / "allowed"
    allowed_dir.mkdir()
    outside_dir = tmp_path / "outside"
    outside_dir.mkdir()

    p = _write_allowlist(tmp_path, {"allowedRoots": [{"path": str(allowed_dir)}]})
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(outside_dir), container_path="data")
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is False
    assert "not under any allowed root" in result["reason"]


# ---------------------------------------------------------------------------
# validate_mount — happy path
# ---------------------------------------------------------------------------

def test_validate_mount_allowed(tmp_path, monkeypatch):
    data_dir = tmp_path / "mydata"
    data_dir.mkdir()
    p = _write_allowlist(tmp_path, {
        "allowedRoots": [{"path": str(tmp_path), "allowReadWrite": True}],
    })
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(data_dir), container_path="mydata", readonly=False)
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is True
    assert result["effective_readonly"] is False


# ---------------------------------------------------------------------------
# validate_mount — non-main forced read-only
# ---------------------------------------------------------------------------

def test_validate_mount_non_main_forced_readonly(tmp_path, monkeypatch):
    data_dir = tmp_path / "shared"
    data_dir.mkdir()
    p = _write_allowlist(tmp_path, {
        "allowedRoots": [{"path": str(tmp_path), "allowReadWrite": True}],
        "nonMainReadOnly": True,
    })
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(data_dir), container_path="shared", readonly=False)
    result = validate_mount(mount, is_main=False)
    assert result["allowed"] is True
    assert result["effective_readonly"] is True     # forced even though requested rw


# ---------------------------------------------------------------------------
# validate_mount — invalid container path
# ---------------------------------------------------------------------------

def test_validate_mount_absolute_container_path_rejected(tmp_path, monkeypatch):
    data_dir = tmp_path / "d"
    data_dir.mkdir()
    p = _write_allowlist(tmp_path, {"allowedRoots": [{"path": str(tmp_path)}]})
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(data_dir), container_path="/etc/evil")
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is False
    assert "Invalid container path" in result["reason"]


def test_validate_mount_traversal_container_path_rejected(tmp_path, monkeypatch):
    data_dir = tmp_path / "d"
    data_dir.mkdir()
    p = _write_allowlist(tmp_path, {"allowedRoots": [{"path": str(tmp_path)}]})
    monkeypatch.setattr(ms_mod, "MOUNT_ALLOWLIST_PATH", str(p))

    mount = AdditionalMount(host_path=str(data_dir), container_path="../escape")
    result = validate_mount(mount, is_main=True)
    assert result["allowed"] is False
    assert "Invalid container path" in result["reason"]
