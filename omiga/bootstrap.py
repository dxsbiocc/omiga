"""One-time startup bootstrap tasks for Omiga.

Handles auto-registration of the main group and creation of the user profile
template on first run.
"""
from __future__ import annotations

import logging

import omiga.state as state
from omiga.config import ASSISTANT_NAME, GROUPS_DIR, MAIN_GROUP_FOLDER, MAIN_GROUP_JID, MAIN_GROUP_NAME
from omiga.models import RegisteredGroup

logger = logging.getLogger("omiga.bootstrap")


async def bootstrap_main_group() -> None:
    """Auto-register the main group at startup if MAIN_GROUP_JID is configured.

    The main group (folder="main") never requires a trigger word — every
    message is forwarded directly to the agent.  This is the right setting
    for a personal private chat or a dedicated bot channel.

    Registration is skipped when:
      - MAIN_GROUP_JID is not set in .env
      - The JID is already registered (any folder)
      - A group with folder="main" already exists
    """
    if not MAIN_GROUP_JID:
        return

    # Already registered?
    if MAIN_GROUP_JID in state._registered_groups:
        return
    if any(g.folder == MAIN_GROUP_FOLDER for g in state._registered_groups.values()):
        return

    from datetime import datetime, timezone
    group = RegisteredGroup(
        name=MAIN_GROUP_NAME,
        folder=MAIN_GROUP_FOLDER,
        trigger=f"@{ASSISTANT_NAME}",
        added_at=datetime.now(timezone.utc).isoformat(),
        requires_trigger=False,  # main group never needs a trigger word
    )
    await state.register_group(MAIN_GROUP_JID, group)
    logger.info(
        "Main group auto-registered: jid=%s name=%s (no trigger word required)",
        MAIN_GROUP_JID,
        MAIN_GROUP_NAME,
    )


def bootstrap_profile() -> None:
    """Create a PROFILE.md template in groups/global/ on first run.

    This file is mounted read-write into every container so the agent can
    update it as it learns about the user.  On first run it contains only
    placeholder values; the agent will fill them in during the first
    conversation (bootstrap flow).
    """
    global_dir = GROUPS_DIR / "global"
    global_dir.mkdir(parents=True, exist_ok=True)
    profile_path = global_dir / "PROFILE.md"
    if profile_path.exists():
        return  # already bootstrapped

    profile_path.write_text(
        """\
# User Profile

> This file is maintained by the AI assistant. It is updated automatically
> as the assistant learns more about you. You can also edit it directly.

## Basic Info

- **Name**: (not yet known)
- **Timezone**: (not yet known)
- **Language preference**: (not yet known)
- **Preferred reply language**: (not yet known)

## Preferences

- **Reply style**: (not yet known — concise / detailed / casual / formal)
- **Programming languages**: (not yet known)
- **Favorite tools / stack**: (not yet known)

## Ongoing Projects

(none recorded yet)

## Interests & Background

(none recorded yet)

## Notes

(none recorded yet)
""",
        encoding="utf-8",
    )
    logger.info(
        "First run: created PROFILE.md template at %s — "
        "the assistant will fill it in during the first conversation.",
        profile_path,
    )
