"""Tests for omiga/bootstrap.py — first-run initialisation."""
from __future__ import annotations

from unittest.mock import AsyncMock, patch

import pytest

import omiga.database as db_mod
import omiga.state as state
from omiga.bootstrap import bootstrap_main_group, bootstrap_profile
from omiga.database import init_database
from omiga.models import RegisteredGroup


@pytest.fixture(autouse=True)
async def reset(tmp_path):
    await db_mod.close_database()
    db_mod._DB_PATH = tmp_path / "test.db"
    await init_database()

    state._registered_groups = {}
    yield

    await db_mod.close_database()
    db_mod._DB_PATH = None
    state._registered_groups = {}


# ---------------------------------------------------------------------------
# bootstrap_profile
# ---------------------------------------------------------------------------


def test_bootstrap_profile_creates_file(tmp_path):
    with patch("omiga.bootstrap.GROUPS_DIR", tmp_path):
        bootstrap_profile()

    profile_path = tmp_path / "global" / "PROFILE.md"
    assert profile_path.exists()
    content = profile_path.read_text()
    assert "# User Profile" in content
    assert "not yet known" in content


def test_bootstrap_profile_skips_if_exists(tmp_path):
    global_dir = tmp_path / "global"
    global_dir.mkdir(parents=True)
    profile_path = global_dir / "PROFILE.md"
    profile_path.write_text("existing content")

    with patch("omiga.bootstrap.GROUPS_DIR", tmp_path):
        bootstrap_profile()

    # File should NOT be overwritten
    assert profile_path.read_text() == "existing content"


def test_bootstrap_profile_creates_global_dir(tmp_path):
    assert not (tmp_path / "global").exists()
    with patch("omiga.bootstrap.GROUPS_DIR", tmp_path):
        bootstrap_profile()
    assert (tmp_path / "global").is_dir()


# ---------------------------------------------------------------------------
# bootstrap_main_group
# ---------------------------------------------------------------------------


async def test_bootstrap_main_group_noop_when_no_jid():
    with patch("omiga.bootstrap.MAIN_GROUP_JID", ""):
        await bootstrap_main_group()
    assert state._registered_groups == {}


async def test_bootstrap_main_group_registers_when_not_present(tmp_path):
    with (
        patch("omiga.bootstrap.MAIN_GROUP_JID", "main@g.us"),
        patch("omiga.bootstrap.MAIN_GROUP_FOLDER", "main"),
        patch("omiga.bootstrap.MAIN_GROUP_NAME", "Main"),
        patch("omiga.state.resolve_group_folder_path", return_value=tmp_path / "main"),
    ):
        (tmp_path / "main").mkdir()
        await bootstrap_main_group()

    assert "main@g.us" in state._registered_groups
    grp = state._registered_groups["main@g.us"]
    assert grp.folder == "main"
    assert grp.requires_trigger is False  # main group never needs trigger


async def test_bootstrap_main_group_skips_if_jid_already_registered(tmp_path):
    existing = RegisteredGroup(
        name="Main", folder="main", trigger="@bot",
        added_at="2024-01-01T00:00:00Z", requires_trigger=False,
    )
    state._registered_groups = {"main@g.us": existing}

    with patch("omiga.bootstrap.MAIN_GROUP_JID", "main@g.us"):
        await bootstrap_main_group()

    # Still just one group, no duplicate
    assert len(state._registered_groups) == 1


async def test_bootstrap_main_group_skips_if_main_folder_exists(tmp_path):
    """Skip registration if a group with folder='main' already exists under a different JID."""
    existing = RegisteredGroup(
        name="Main", folder="main", trigger="@bot",
        added_at="2024-01-01T00:00:00Z", requires_trigger=False,
    )
    state._registered_groups = {"old-jid@g.us": existing}

    with (
        patch("omiga.bootstrap.MAIN_GROUP_JID", "new-jid@g.us"),
        patch("omiga.bootstrap.MAIN_GROUP_FOLDER", "main"),
    ):
        await bootstrap_main_group()

    # Should not have added the new JID
    assert "new-jid@g.us" not in state._registered_groups
