"""Media attachment path management for Omiga.

Provides a single helper that resolves the storage directory and the
workspace-relative path for a media file, given the chat JID and the
registered-groups mapping.
"""
from __future__ import annotations

import re
from datetime import date
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from omiga.models import RegisteredGroup


def get_attachment_path(
    jid: str,
    filename: str,
    registered_groups: dict[str, "RegisteredGroup"],
) -> tuple[Path, str]:
    """Return ``(directory, relative_path)`` for a media attachment.

    Files are organised under a ``YYYY/MM/DD`` date subdirectory so the
    workspace stays tidy over time.

    *relative_path* is relative to the group workspace (e.g.
    ``"attachments/2026/03/02/photo.jpg"``), which maps to
    ``/workspace/group/attachments/2026/03/02/photo.jpg`` inside the
    agent container.
    For unregistered groups *relative_path* is the absolute host path
    string so the agent can still reference the file if needed.

    The directory is created on disk before returning.
    """
    from omiga.config import GROUPS_DIR, DATA_DIR

    today = date.today()
    date_subdir = today.strftime("%Y/%m/%d")

    group = registered_groups.get(jid)
    if group:
        att_dir = GROUPS_DIR / group.folder / "attachments" / date_subdir
        rel = f"attachments/{date_subdir}/{filename}"
    else:
        safe = re.sub(r"[^a-z0-9_-]", "_", jid.lower())
        att_dir = DATA_DIR / "media" / safe / date_subdir
        rel = str(att_dir / filename)

    att_dir.mkdir(parents=True, exist_ok=True)
    return att_dir, rel
