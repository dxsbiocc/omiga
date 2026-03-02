"""
Colored, structured logging for Omiga.

Color-codes log records by level using ANSI escape codes.  Falls back to
plain text automatically on non-TTY streams (e.g. systemd journal, Docker
log drivers).

Usage
-----
    from omiga.logging_setup import configure_logging
    configure_logging()  # call once at process start
"""
from __future__ import annotations

import logging
import os
import sys


# ---------------------------------------------------------------------------
# ANSI color codes
# ---------------------------------------------------------------------------
_RESET = "\033[0m"
_BOLD = "\033[1m"

_LEVEL_COLORS: dict[int, str] = {
    logging.DEBUG:    "\033[36m",   # cyan
    logging.INFO:     "\033[32m",   # green
    logging.WARNING:  "\033[33m",   # yellow
    logging.ERROR:    "\033[31m",   # red
    logging.CRITICAL: "\033[1;31m", # bold red
}

_LEVEL_LABELS: dict[int, str] = {
    logging.DEBUG:    "DEBUG",
    logging.INFO:     "INFO ",
    logging.WARNING:  "WARN ",
    logging.ERROR:    "ERROR",
    logging.CRITICAL: "CRIT ",
}


class ColorFormatter(logging.Formatter):
    """Formatter that adds ANSI color codes when writing to a TTY."""

    def __init__(self, use_color: bool = True) -> None:
        super().__init__()
        self._use_color = use_color

    def format(self, record: logging.LogRecord) -> str:  # noqa: A003
        ts = self.formatTime(record, "%Y-%m-%dT%H:%M:%S")
        level_label = _LEVEL_LABELS.get(record.levelno, record.levelname[:5].ljust(5))

        # Short logger name: keep last two components (e.g. nanoclaw.main → main)
        parts = record.name.split(".")
        short_name = ".".join(parts[-2:]) if len(parts) >= 2 else record.name
        short_name = short_name[:20]

        msg = record.getMessage()
        if record.exc_info:
            msg = msg + "\n" + self.formatException(record.exc_info)

        if self._use_color:
            color = _LEVEL_COLORS.get(record.levelno, "")
            return (
                f"{color}{_BOLD}{ts}{_RESET} "
                f"{color}{level_label}{_RESET} "
                f"\033[90m[{short_name}]{_RESET} "
                f"{msg}"
            )
        return f"{ts} {level_label} [{short_name}] {msg}"


def _is_tty(stream=None) -> bool:
    """Return True if *stream* (default: stderr) is an interactive terminal."""
    if stream is None:
        stream = sys.stderr
    try:
        return stream.isatty()
    except Exception:
        return False


def configure_logging(level: str | None = None) -> None:
    """Configure root logger with ColorFormatter.

    Parameters
    ----------
    level:
        Log level string (e.g. ``"DEBUG"``, ``"INFO"``).  Defaults to the
        ``LOG_LEVEL`` environment variable, or ``INFO`` if unset.
    """
    raw_level = level or os.environ.get("LOG_LEVEL", "INFO")
    numeric = getattr(logging, raw_level.upper(), logging.INFO)

    handler = logging.StreamHandler(sys.stderr)
    handler.setLevel(numeric)

    use_color = _is_tty(sys.stderr) and os.environ.get("NO_COLOR", "") == ""
    handler.setFormatter(ColorFormatter(use_color=use_color))

    root = logging.getLogger()
    root.setLevel(numeric)

    # Replace any existing handlers (e.g. from a previous basicConfig call)
    root.handlers.clear()
    root.addHandler(handler)

    # Suppress noisy third-party loggers unless DEBUG is enabled
    if numeric > logging.DEBUG:
        for noisy in ("discord", "aiohttp", "websockets", "urllib3", "httpx", "httpcore"):
            logging.getLogger(noisy).setLevel(logging.WARNING)
