"""
Tests for nanoclaw/container_runner.py

Focuses on unit-testable helpers; does NOT actually spawn containers.
"""
from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from omiga.container_runner import (
    OUTPUT_END_MARKER,
    OUTPUT_START_MARKER,
    write_groups_snapshot,
    write_tasks_snapshot,
)
from omiga.group_folder import resolve_group_ipc_path
from omiga.models import AvailableGroup


# ---------------------------------------------------------------------------
# Sentinel markers
# ---------------------------------------------------------------------------

def test_sentinel_markers_are_bytes():
    assert isinstance(OUTPUT_START_MARKER, bytes)
    assert isinstance(OUTPUT_END_MARKER, bytes)


def test_sentinel_roundtrip_parsing():
    """Verify the sentinel parsing logic used inside read_stdout."""
    payload = {"status": "success", "result": "hello"}
    json_bytes = json.dumps(payload).encode()
    buffer = OUTPUT_START_MARKER + b"\n" + json_bytes + b"\n" + OUTPUT_END_MARKER

    start = buffer.find(OUTPUT_START_MARKER)
    end = buffer.find(OUTPUT_END_MARKER, start)
    json_str = buffer[start + len(OUTPUT_START_MARKER):end].strip()
    parsed = json.loads(json_str)

    assert parsed["status"] == "success"
    assert parsed["result"] == "hello"


# ---------------------------------------------------------------------------
# write_tasks_snapshot
# ---------------------------------------------------------------------------

@pytest.fixture()
def patched_data_dir(tmp_path, monkeypatch):
    """Redirect DATA_DIR to tmp_path for filesystem tests."""
    import omiga.config as cfg
    monkeypatch.setattr(cfg, "DATA_DIR", tmp_path)
    return tmp_path


def test_write_tasks_snapshot_main_sees_all(patched_data_dir):
    tmp_path = patched_data_dir
    tasks = [
        {"id": "t1", "groupFolder": "main", "prompt": "P1", "schedule_type": "interval",
         "schedule_value": "60000", "status": "active", "next_run": None},
        {"id": "t2", "groupFolder": "work", "prompt": "P2", "schedule_type": "interval",
         "schedule_value": "60000", "status": "active", "next_run": None},
    ]

    write_tasks_snapshot("main", True, tasks)
    snapshot_file = tmp_path / "ipc" / "main" / "current_tasks.json"
    assert snapshot_file.exists()
    data = json.loads(snapshot_file.read_text())
    assert len(data) == 2


def test_write_tasks_snapshot_non_main_filtered(patched_data_dir):
    tmp_path = patched_data_dir
    tasks = [
        {"id": "t1", "groupFolder": "main", "prompt": "P1", "schedule_type": "interval",
         "schedule_value": "60000", "status": "active", "next_run": None},
        {"id": "t2", "groupFolder": "work", "prompt": "P2", "schedule_type": "interval",
         "schedule_value": "60000", "status": "active", "next_run": None},
    ]

    write_tasks_snapshot("work", False, tasks)
    snapshot_file = tmp_path / "ipc" / "work" / "current_tasks.json"
    data = json.loads(snapshot_file.read_text())
    assert len(data) == 1
    assert data[0]["id"] == "t2"


# ---------------------------------------------------------------------------
# write_groups_snapshot
# ---------------------------------------------------------------------------

def test_write_groups_snapshot_main_sees_all(patched_data_dir):
    tmp_path = patched_data_dir
    groups = [
        AvailableGroup("jid1", "G1", "2024-01-01T00:00:00Z", True),
        AvailableGroup("jid2", "G2", "2024-01-02T00:00:00Z", False),
    ]

    write_groups_snapshot("main", True, groups, {"jid1"})
    snapshot_file = tmp_path / "ipc" / "main" / "available_groups.json"
    data = json.loads(snapshot_file.read_text())
    assert len(data["groups"]) == 2


def test_write_groups_snapshot_non_main_empty(patched_data_dir):
    tmp_path = patched_data_dir
    groups = [AvailableGroup("jid1", "G1", "2024-01-01T00:00:00Z", True)]

    write_groups_snapshot("work", False, groups, {"jid1"})
    snapshot_file = tmp_path / "ipc" / "work" / "available_groups.json"
    data = json.loads(snapshot_file.read_text())
    assert data["groups"] == []
