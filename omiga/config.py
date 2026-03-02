"""
Configuration module for Omiga Python port.

Reads from .env file (falls back to environment variables).
Secrets are NOT loaded here — they are read only where needed
(container_runner.py) to avoid leaking to child processes.
"""

import os
import re
import time
from pathlib import Path

from dotenv import dotenv_values

# Project root = parent of the omiga package directory
_PROJECT_ROOT = Path(__file__).parent.parent.resolve()
_ENV_FILE = _PROJECT_ROOT / ".env"

# Load non-secret config values from .env
_env = dotenv_values(_ENV_FILE) if _ENV_FILE.exists() else {}


def _get(key: str, default: str = "") -> str:
    return os.environ.get(key) or _env.get(key) or default


# Assistant identity
ASSISTANT_NAME: str = _get("ASSISTANT_NAME", "Omiga")
ASSISTANT_HAS_OWN_NUMBER: bool = (
    _get("ASSISTANT_HAS_OWN_NUMBER", "false").lower() == "true"
)

# Polling intervals (milliseconds in TS → seconds in Python)
POLL_INTERVAL: float = 2.0  # 2s message loop
SCHEDULER_POLL_INTERVAL: float = 60.0  # 60s scheduler
IPC_POLL_INTERVAL: float = 1.0  # 1s IPC

# Message debounce: wait this many seconds after first seeing new messages
# before starting a container, so that rapid follow-up messages get batched.
MESSAGE_DEBOUNCE_SECONDS: float = float(_get("MESSAGE_DEBOUNCE_SECONDS", "2.0"))

# Absolute paths
PROJECT_ROOT: Path = _PROJECT_ROOT
HOME_DIR: Path = Path(os.environ.get("HOME", str(Path.home())))

MOUNT_ALLOWLIST_PATH: Path = HOME_DIR / ".config" / "omiga" / "mount-allowlist.json"
STORE_DIR: Path = (PROJECT_ROOT / "store").resolve()
GROUPS_DIR: Path = (PROJECT_ROOT / "groups").resolve()
DATA_DIR: Path = (PROJECT_ROOT / "data").resolve()
MEDIA_DIR: Path = DATA_DIR / "media"  # fallback for media from unregistered groups
MAIN_GROUP_FOLDER: str = "main"

# Auto-registration of the main group at startup.
# Set MAIN_GROUP_JID to the Telegram/channel JID of your personal chat or
# private group (e.g. "tg:123456789").  When set, omiga will register it
# as the main group on first run — no IPC call needed.
# MAIN_GROUP_NAME defaults to "Main" if not provided.
MAIN_GROUP_JID: str = _get("MAIN_GROUP_JID", "")
MAIN_GROUP_NAME: str = _get("MAIN_GROUP_NAME", "Main")

# Container settings
CONTAINER_IMAGE: str = _get("CONTAINER_IMAGE", "omiga-agent:latest")
CONTAINER_TIMEOUT: int = int(_get("CONTAINER_TIMEOUT", "1800000"))  # ms
CONTAINER_MAX_OUTPUT_SIZE: int = int(
    _get("CONTAINER_MAX_OUTPUT_SIZE", "10485760")
)  # 10MB
IDLE_TIMEOUT: int = int(_get("IDLE_TIMEOUT", "1800000"))  # ms
MAX_CONCURRENT_CONTAINERS: int = max(
    1, int(_get("MAX_CONCURRENT_CONTAINERS", "5") or "5")
)

# Timezone for cron expressions
TIMEZONE: str = os.environ.get("TZ") or _env.get("TZ") or ""
if not TIMEZONE:
    try:
        import datetime

        TIMEZONE = (
            datetime.datetime.now(datetime.timezone.utc).astimezone().tzname() or "UTC"
        )
        # Try to get a proper IANA timezone name
        import zoneinfo

        TIMEZONE = str(zoneinfo.ZoneInfo(time.tzname[0])) if time.tzname[0] else "UTC"
    except Exception:
        TIMEZONE = "UTC"


# HTTP API server settings
HTTP_API_PORT: int = int(_get("HTTP_API_PORT", "7891"))  # 0 = disabled
HTTP_API_HOST: str = _get("HTTP_API_HOST", "127.0.0.1")
HTTP_API_TOKEN: str = _get("HTTP_API_TOKEN", "")  # empty = no auth

# Voice transcription via OpenAI Whisper
# Set WHISPER_ENABLED=true and OPENAI_API_KEY=sk-... to enable.
WHISPER_ENABLED: bool = _get("WHISPER_ENABLED", "false").lower() == "true"
# Optional BCP-47 language hint (e.g. "zh", "en"). Empty = auto-detect.
WHISPER_LANGUAGE: str = _get("WHISPER_LANGUAGE", "")


def get_secret(key: str) -> str:
    """Read a secret value from the environment or .env file.

    Unlike the module-level _get() helper, this is intentionally public so
    that startup code can load secrets (e.g. TELEGRAM_BOT_TOKEN) without
    caching them in module-level constants.
    """
    return os.environ.get(key) or _env.get(key) or ""


def _escape_regex(s: str) -> str:
    return re.escape(s)


# Trigger pattern: ^@ASSISTANT_NAME\b  (case-insensitive)
TRIGGER_PATTERN: re.Pattern = re.compile(
    rf"^@{_escape_regex(ASSISTANT_NAME)}\b", re.IGNORECASE
)
