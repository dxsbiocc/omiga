#!/usr/bin/env python3
"""
NanoClaw Python Agent Runner

Receives ContainerInput JSON via stdin, runs an agentic tool loop using
any OpenAI-compatible provider, outputs results via sentinel-wrapped JSON.

Provider config comes from the `secrets` field in the input JSON:
  AI_API_KEY   - API key for the provider
  AI_BASE_URL  - Provider base URL (e.g. https://api.deepseek.com/v1)
  AI_MODEL     - Model name (e.g. deepseek-chat, qwen-plus, abab6.5s-chat)

Stdout protocol (same as TypeScript runner):
  ---NANOCLAW_OUTPUT_START---
  {"status": "success", "result": "...", "newSessionId": "..."}
  ---NANOCLAW_OUTPUT_END---
"""
from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path
from typing import Any

from openai import OpenAI
from tools import TOOL_DEFINITIONS, execute_tool
from mcp_client import get_registry

OUTPUT_START = "---NANOCLAW_OUTPUT_START---"
OUTPUT_END   = "---NANOCLAW_OUTPUT_END---"

IPC_INPUT_DIR      = os.environ.get("NANOCLAW_IPC_DIR", "/workspace/ipc/input")
IPC_CLOSE_SENTINEL = os.path.join(IPC_INPUT_DIR, "_close")
IPC_POLL_INTERVAL  = 0.5  # seconds

BASE_SYSTEM_PROMPT = """\
You are a personal AI assistant running inside an isolated Docker container.
Your working directory is /workspace/group.

## Directory layout
- /workspace/group          — your workspace (read-write, persists across sessions)
- /workspace/env            — persistent package environment (read-write)
- /workspace/global         — global user profile & notes (read-write, shared)
- /workspace/extra/*        — additional host paths (read-only, allowlist-controlled)

## Memory system

You maintain three layers of persistent memory. Read them at the start of each
conversation, update them proactively as you learn new things.

### 1. Global user profile  →  /workspace/global/PROFILE.md
Shared across all groups. Contains the user's name, timezone, language, preferences,
and ongoing projects. Update it whenever the user reveals personal information.

**Bootstrap** — If PROFILE.md still shows "(not yet known)" for most fields,
this is likely the first conversation. Greet the user warmly and *naturally* ask
for their name, preferred language, timezone, and a little context about what they
want to use you for. Fill in PROFILE.md as they answer. Do this conversationally,
not as a questionnaire.

### 2. Group-specific memory  →  /workspace/group/MEMORY.md
Contains facts, decisions, tool configs, and lessons learned that are specific to
this group. Create or update it when you learn something worth remembering.

Suggested structure:
```
# Memory

## Projects & Context
## Tool Setup
## Lessons Learned
## Preferences (group-specific)
```

### 3. Daily notes  →  /workspace/group/notes/YYYY-MM-DD.md
(where YYYY-MM-DD is today's date, e.g. 2026-03-01)
Append a brief log of key events, decisions, or facts discovered during each
conversation. Create the notes/ directory if it doesn't exist.

### Memory rules

**Reading (always do this):**
- At the start of each conversation, Read /workspace/global/PROFILE.md and
  /workspace/group/MEMORY.md. Use what you learn to personalize your responses.

**Writing (proactive, without being asked):**
- User reveals name, timezone, language, or preferences → Update PROFILE.md immediately.
- You discover project context, tool configs, or decisions for this group → Update MEMORY.md.
- End of a substantive conversation → Append a 2–5 line summary to today's notes file.

**Context compaction:**
- When the conversation is very long and you're reaching your context limit, write a
  "Session summary" entry to today's notes file, then continue. This prevents losing
  context across long sessions.

**Format:**
- Use Markdown. Keep entries concise (bullets preferred).
- Always Read a file before Edit-ing it to get the exact text.
- Use Write for new files, Edit for targeted updates.

## Available tools
- Bash        : run shell commands (git, python3, node, curl, etc.)
- Read        : read a file with line numbers
- Write       : create or overwrite a file
- Edit        : replace a specific string in a file (read the file first)
- Glob        : list files matching a pattern (e.g. **/*.py)
- Grep        : search file contents by regex
- WebFetch    : fetch a URL and return its text content
- Browser*    : headless Chromium automation (Navigate, Snapshot, Click, Fill, …)

## MCP tools
If MCP servers are configured in /workspace/mcp_servers.json, their tools appear
as "ServerName__tool_name" in your available tools list. Use them like any other tool.
To add or change MCP servers, edit /workspace/mcp_servers.json — changes take effect
on the next conversation (no container rebuild needed).

## Installing packages
- Python (persistent): pip install --prefix /workspace/env <package>
  Packages are auto-discoverable (PYTHONPATH covers both site-packages layouts).
- Python (session-only): pip install <package>
- Node.js: npm/pnpm install in /workspace/group (node_modules persists)
- System tools (session): sudo apt-get install -y <package>
- Use -i https://pypi.tuna.tsinghua.edu.cn/simple/ for faster pip downloads in China

## General rules
- Always Read a file before Edit-ing it to get the exact text.
- Prefer Edit for targeted changes, Write for new files or full rewrites.
- When a task is complete, respond with a concise summary of what was done.
"""


# ── Output helpers ─────────────────────────────────────────────────────────

def write_output(status: str, result: str | None,
                 session_id: str | None = None, error: str | None = None) -> None:
    print(OUTPUT_START, flush=True)
    print(json.dumps({
        "status": status,
        "result": result,
        "newSessionId": session_id,
        "error": error,
    }), flush=True)
    print(OUTPUT_END, flush=True)


def log(msg: str) -> None:
    print(f"[py-agent] {msg}", file=sys.stderr, flush=True)


# ── System prompt ──────────────────────────────────────────────────────────

def build_system_prompt(is_main: bool, assistant_name: str) -> str:
    parts = [BASE_SYSTEM_PROMPT.replace(
        "personal AI assistant", f"{assistant_name} (personal AI assistant)"
    )]

    workspace = os.environ.get("NANOCLAW_WORKSPACE", "/workspace/group")
    global_dir = os.environ.get("NANOCLAW_GLOBAL_DIR", "/workspace/global")

    # Global CLAUDE.md (shared across all groups)
    global_md = Path(global_dir) / "CLAUDE.md"
    if global_md.exists():
        parts.append(f"\n\n# Global Instructions\n{global_md.read_text()}")

    # Group-specific CLAUDE.md
    group_md = Path(workspace) / "CLAUDE.md"
    if group_md.exists():
        parts.append(f"\n\n# Group Instructions\n{group_md.read_text()}")

    return "\n".join(parts)


# ── IPC helpers ────────────────────────────────────────────────────────────

def drain_ipc() -> list[str]:
    """Consume all pending IPC message files, return their texts."""
    try:
        ipc = Path(IPC_INPUT_DIR)
        ipc.mkdir(parents=True, exist_ok=True)
        messages: list[str] = []
        for f in sorted(ipc.glob("*.json")):
            try:
                data = json.loads(f.read_text())
                f.unlink()
                if data.get("type") == "message" and data.get("text"):
                    messages.append(data["text"])
            except Exception as e:
                log(f"IPC read error {f.name}: {e}")
                try:
                    f.unlink()
                except Exception:
                    pass
        return messages
    except Exception as e:
        log(f"IPC drain failed: {e}")
        return []


def check_close() -> bool:
    sentinel = Path(IPC_CLOSE_SENTINEL)
    if sentinel.exists():
        try:
            sentinel.unlink()
        except Exception:
            pass
        return True
    return False


def wait_for_ipc() -> str | None:
    """Block until a new IPC message arrives or _close sentinel appears."""
    while True:
        if check_close():
            return None
        msgs = drain_ipc()
        if msgs:
            return "\n".join(msgs)
        time.sleep(IPC_POLL_INTERVAL)


# ── Agent loop ─────────────────────────────────────────────────────────────

_CHAR_LIMIT = 120_000  # ~30k tokens; trim history if exceeded


def _trim_messages(messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Drop oldest non-system messages when total size exceeds the limit."""
    total = sum(len(json.dumps(m)) for m in messages)
    while total > _CHAR_LIMIT and len(messages) > 2:
        dropped = messages.pop(0)
        total -= len(json.dumps(dropped))
    return messages


def run_once(client: OpenAI, model: str, system: str,
             prompt: str, session_id: str) -> str:
    """Run one turn: call LLM, execute tools until final answer, return text."""
    mcp = get_registry()
    all_tools = TOOL_DEFINITIONS + mcp.tool_definitions

    messages: list[dict[str, Any]] = [{"role": "user", "content": prompt}]

    for iteration in range(50):
        log(f"LLM call #{iteration + 1}")
        _trim_messages(messages)
        response = client.chat.completions.create(
            model=model,
            messages=[{"role": "system", "content": system}] + messages,
            tools=all_tools,
            tool_choice="auto",
            max_tokens=8192,
        )
        choice = response.choices[0]
        msg = choice.message
        messages.append(msg.model_dump(exclude_none=True))

        if choice.finish_reason == "tool_calls" and msg.tool_calls:
            for tc in msg.tool_calls:
                try:
                    args = json.loads(tc.function.arguments)
                except json.JSONDecodeError:
                    args = {}
                name = tc.function.name
                log(f"  Tool: {name}({list(args.keys())})")
                # Route to MCP server if the tool is namespaced (ServerName__tool)
                if "__" in name and name in {t["function"]["name"] for t in mcp.tool_definitions}:
                    result = mcp.call(name, args)
                else:
                    result = execute_tool(name, args)
                messages.append({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": result,
                })
        else:
            return msg.content or ""

    return "(reached iteration limit)"


# ── Entry point ────────────────────────────────────────────────────────────

def main() -> None:
    try:
        container_input: dict[str, Any] = json.loads(sys.stdin.read())
    except Exception as e:
        write_output("error", None, error=f"Failed to parse input: {e}")
        sys.exit(1)

    # Secrets passed in via the input JSON (never from process env directly)
    secrets: dict[str, str] = container_input.get("secrets") or {}

    api_key  = secrets.get("AI_API_KEY")  or os.environ.get("AI_API_KEY", "")
    base_url = secrets.get("AI_BASE_URL") or os.environ.get("AI_BASE_URL", "")
    model    = secrets.get("AI_MODEL")    or os.environ.get("AI_MODEL", "deepseek-chat")

    if not api_key:
        write_output("error", None, error=(
            "AI_API_KEY is not set. "
            "Add it to .env: AI_API_KEY=sk-... AI_BASE_URL=https://api.deepseek.com/v1 AI_MODEL=deepseek-chat"
        ))
        sys.exit(1)

    group_folder   = container_input.get("groupFolder", "main")
    is_main        = bool(container_input.get("isMain", False))
    is_scheduled   = bool(container_input.get("isScheduledTask", False))
    assistant_name = container_input.get("assistantName") or "Andy"
    session_id     = container_input.get("sessionId") or f"py-{int(time.time())}"
    prompt         = container_input.get("prompt", "")

    if is_scheduled:
        prompt = f"[SCHEDULED TASK — not from a user directly]\n\n{prompt}"

    client = OpenAI(api_key=api_key, base_url=base_url or None)
    system = build_system_prompt(is_main, assistant_name)

    log(f"model={model} base_url={base_url or '(default)'} group={group_folder}")

    # Merge any pending IPC messages into the initial prompt
    pending = drain_ipc()
    if pending:
        prompt += "\n" + "\n".join(pending)

    try:
        while True:
            result = run_once(client, model, system, prompt, session_id)
            write_output("success", result, session_id=session_id)
            # Session-update heartbeat so host can track the session id
            write_output("success", None, session_id=session_id)

            log("Waiting for next IPC message...")
            next_msg = wait_for_ipc()
            if next_msg is None:
                log("Close sentinel — exiting")
                break
            prompt = next_msg
            log(f"New IPC message ({len(next_msg)} chars)")
    except Exception as e:
        log(f"Fatal: {e}")
        write_output("error", None, session_id=session_id, error=str(e))
        sys.exit(1)


if __name__ == "__main__":
    main()
