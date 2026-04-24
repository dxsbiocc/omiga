#!/usr/bin/env python3
"""Ralph error dedup helper.

Usage:
    python3 .omiga/scripts/ralph_record_error.py <state_file> <error_message>

Reads the Ralph state JSON, fingerprints the error, compares with last_error,
increments or resets consecutive_errors, writes the updated state, then prints:

    CONTINUE   — errors < 3, keep going
    STOP       — same error 3+ times; Ralph must report and halt

Exit code 0 always (so the caller can read stdout safely regardless of state).
"""

import json
import hashlib
import sys
import os
from datetime import datetime, timezone

MAX_CONSECUTIVE = 3


def fingerprint(msg: str) -> str:
    """Normalize and fingerprint an error message for dedup comparison.

    Strips line numbers, memory addresses, and timestamps so that the same
    logical error is treated as identical even if details change.
    """
    import re
    # Remove line numbers like ":42:", "line 42", "L42"
    msg = re.sub(r"\bline\s+\d+\b", "line N", msg, flags=re.IGNORECASE)
    msg = re.sub(r":\d+:", ":N:", msg)
    # Remove hex addresses like 0x7f3a...
    msg = re.sub(r"0x[0-9a-fA-F]+", "0xADDR", msg)
    # Remove timestamps in common formats
    msg = re.sub(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}", "TIMESTAMP", msg)
    # Collapse whitespace
    msg = " ".join(msg.split())
    # Take first 200 chars to avoid hash differences from long stack traces
    return hashlib.sha1(msg[:200].encode()).hexdigest()[:16]


def main():
    if len(sys.argv) < 3:
        print("Usage: ralph_record_error.py <state_file> <error_message>", file=sys.stderr)
        print("CONTINUE")
        return

    state_file = sys.argv[1]
    error_msg = " ".join(sys.argv[2:])

    # Load state
    try:
        with open(state_file, "r") as f:
            state = json.load(f)
    except Exception as e:
        print(f"[ralph_record_error] Could not read state: {e}", file=sys.stderr)
        print("CONTINUE")
        return

    fp = fingerprint(error_msg)
    last_fp = fingerprint(state.get("last_error", "") or "")

    if fp == last_fp and state.get("last_error"):
        state["consecutive_errors"] = state.get("consecutive_errors", 0) + 1
    else:
        state["consecutive_errors"] = 1

    state["last_error"] = error_msg[:500]  # cap stored size
    state["updated_at"] = datetime.now(timezone.utc).isoformat()

    # Write updated state
    try:
        with open(state_file, "w") as f:
            json.dump(state, f, indent=2)
    except Exception as e:
        print(f"[ralph_record_error] Could not write state: {e}", file=sys.stderr)

    if state["consecutive_errors"] >= MAX_CONSECUTIVE:
        print(f"STOP  # same error {state['consecutive_errors']}x: {error_msg[:120]}")
    else:
        print(f"CONTINUE  # error {state['consecutive_errors']}/{MAX_CONSECUTIVE}: {error_msg[:80]}")


if __name__ == "__main__":
    main()
