"""
Tool implementations for the NanoClaw Python agent.

All file operations default to /workspace/group as the root.
"""
from __future__ import annotations

import glob as glob_module
import json
import os
import re
import subprocess
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

WORKSPACE = os.environ.get("NANOCLAW_WORKSPACE", "/workspace/group")


def _resolve(path: str) -> Path:
    """Resolve a path relative to the workspace if not absolute."""
    p = Path(path)
    if not p.is_absolute():
        p = Path(WORKSPACE) / p
    return p.resolve()


def bash(command: str) -> str:
    try:
        r = subprocess.run(
            command, shell=True, capture_output=True, text=True,
            timeout=120, cwd=WORKSPACE,
        )
        out = r.stdout
        if r.stderr:
            out = out + "\n[stderr]\n" + r.stderr if out else r.stderr
        return out.rstrip() or "(no output)"
    except subprocess.TimeoutExpired:
        return "[Error: command timed out after 120s]"
    except Exception as e:
        return f"[Error: {e}]"


def read_file(file_path: str, offset: int = 0, limit: int = 0) -> str:
    path = _resolve(file_path)
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        if offset:
            lines = lines[offset - 1:]
        if limit:
            lines = lines[:limit]
        return "\n".join(f"{offset + i + 1:5d}  {l}" for i, l in enumerate(lines))
    except FileNotFoundError:
        return f"[Error: file not found: {path}]"
    except Exception as e:
        return f"[Error reading {path}: {e}]"


def write_file(file_path: str, content: str) -> str:
    path = _resolve(file_path)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return f"Wrote {len(content)} bytes to {path}"
    except Exception as e:
        return f"[Error writing {path}: {e}]"


def edit_file(file_path: str, old_string: str, new_string: str) -> str:
    path = _resolve(file_path)
    try:
        content = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return f"[Error: file not found: {path}]"
    if old_string not in content:
        return f"[Error: string not found in {path}]"
    path.write_text(content.replace(old_string, new_string, 1), encoding="utf-8")
    return f"Replaced 1 occurrence in {path}"


def glob_search(pattern: str, path: str | None = None) -> str:
    base = str(_resolve(path)) if path else WORKSPACE
    full = os.path.join(base, pattern) if not os.path.isabs(pattern) else pattern
    matches = sorted(glob_module.glob(full, recursive=True))
    if not matches:
        return f"No files matching '{pattern}'"
    result = "\n".join(matches[:200])
    if len(matches) > 200:
        result += f"\n... ({len(matches) - 200} more)"
    return result


def grep_search(pattern: str, path: str | None = None, glob: str | None = None) -> str:
    search_path = str(_resolve(path)) if path else WORKSPACE
    cmd = ["grep", "-rn", pattern, search_path]
    if glob:
        cmd = ["grep", "-rn", "--include", glob, pattern, search_path]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        out = r.stdout.rstrip()
        if not out:
            return f"No matches for '{pattern}'"
        lines = out.splitlines()
        if len(lines) > 100:
            return "\n".join(lines[:100]) + f"\n... ({len(lines) - 100} more lines)"
        return out
    except Exception as e:
        return f"[Error: {e}]"


def web_fetch(url: str, max_chars: int = 8000) -> str:
    """Fetch a URL and return its text content (HTML tags stripped)."""
    try:
        req = urllib.request.Request(
            url,
            headers={"User-Agent": "Mozilla/5.0 (compatible; NanoClaw-Agent/1.0)"},
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            content_type = resp.headers.get("Content-Type", "")
            raw = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        return f"[HTTP {e.code}: {e.reason}]"
    except Exception as e:
        return f"[Fetch error: {e}]"

    # Strip HTML tags for readability
    if "html" in content_type.lower() or raw.lstrip().startswith("<"):
        raw = re.sub(r"<script[^>]*>.*?</script>", "", raw, flags=re.DOTALL | re.IGNORECASE)
        raw = re.sub(r"<style[^>]*>.*?</style>",  "", raw, flags=re.DOTALL | re.IGNORECASE)
        raw = re.sub(r"<[^>]+>", "", raw)
        raw = re.sub(r"\n{3,}", "\n\n", raw)
        raw = "\n".join(line for line in raw.splitlines() if line.strip())

    if len(raw) > max_chars:
        raw = raw[:max_chars] + f"\n... [truncated at {max_chars} chars]"
    return raw.strip() or "(empty response)"


_MEMORY_FILE = Path(os.environ.get("NANOCLAW_WORKSPACE", "/workspace/group")) / ".memory.json"


def _load_memory() -> dict[str, str]:
    try:
        return json.loads(_MEMORY_FILE.read_text()) if _MEMORY_FILE.exists() else {}
    except Exception:
        return {}


def remember(key: str, value: str) -> str:
    """Store a key-value pair in persistent memory (survives across sessions)."""
    mem = _load_memory()
    mem[key] = value
    _MEMORY_FILE.write_text(json.dumps(mem, ensure_ascii=False, indent=2))
    return f"Remembered: {key}"


def recall(key: str = "") -> str:
    """Recall a stored memory value. If key is empty, list all stored keys."""
    mem = _load_memory()
    if not key:
        if not mem:
            return "(no memories stored)"
        return "Stored memories:\n" + "\n".join(f"  {k}: {v[:80]}" for k, v in mem.items())
    return mem.get(key, f"(no memory for key: {key!r})")


# ── OpenAI tool schema ──────────────────────────────────────────────────────

TOOL_DEFINITIONS: list[dict[str, Any]] = [
    {
        "type": "function",
        "function": {
            "name": "Bash",
            "description": "Execute a bash command in /workspace/group.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Bash command to run"},
                },
                "required": ["command"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Read",
            "description": "Read a file's contents with line numbers. Supports optional offset and limit.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Absolute or workspace-relative path"},
                    "offset": {"type": "integer", "description": "Start from this line number (1-indexed)"},
                    "limit":  {"type": "integer", "description": "Maximum lines to return"},
                },
                "required": ["file_path"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Write",
            "description": "Create or completely overwrite a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Absolute or workspace-relative path"},
                    "content":   {"type": "string", "description": "Full file content"},
                },
                "required": ["file_path", "content"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Edit",
            "description": "Replace the first occurrence of a string in a file. Read the file first to get the exact text.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path":  {"type": "string", "description": "File to edit"},
                    "old_string": {"type": "string", "description": "Exact text to replace"},
                    "new_string": {"type": "string", "description": "Replacement text"},
                },
                "required": ["file_path", "old_string", "new_string"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Glob",
            "description": "Find files matching a glob pattern, e.g. '**/*.py'.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern"},
                    "path":    {"type": "string", "description": "Root directory (default: workspace)"},
                },
                "required": ["pattern"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Grep",
            "description": "Search file contents for a regex pattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern"},
                    "path":    {"type": "string", "description": "Directory to search (default: workspace)"},
                    "glob":    {"type": "string", "description": "File filter, e.g. '*.py'"},
                },
                "required": ["pattern"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "WebFetch",
            "description": "Fetch a URL and return its text content (HTML tags stripped). Use for reading web pages, API responses, or documentation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url":       {"type": "string", "description": "Full URL to fetch (https://...)"},
                    "max_chars": {"type": "integer", "description": "Max characters to return (default 8000)"},
                },
                "required": ["url"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Remember",
            "description": "Store a piece of information under a key for later recall (persists across sessions).",
            "parameters": {
                "type": "object",
                "properties": {
                    "key":   {"type": "string", "description": "Short identifier for this memory"},
                    "value": {"type": "string", "description": "Information to store"},
                },
                "required": ["key", "value"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "Recall",
            "description": "Retrieve a stored memory by key, or list all stored memory keys.",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": {"type": "string", "description": "Key to look up (empty = list all keys)"},
                },
                "required": [],
            },
        },
    },
    # ── Browser automation ────────────────────────────────────────────────────
    {
        "type": "function",
        "function": {
            "name": "BrowserNavigate",
            "description": "Open a URL in the headless browser.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "Full URL to open (https://...)"},
                },
                "required": ["url"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserSnapshot",
            "description": (
                "Get a structured text snapshot of the current page: URL, title, "
                "links, inputs, buttons, and page text. Always call this after navigating "
                "to understand the page before clicking or filling."
            ),
            "parameters": {"type": "object", "properties": {}, "required": []},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserClick",
            "description": (
                "Click an element. selector can be: visible text of the element, "
                "'css:.my-class', or 'label:Submit'. "
                "Use BrowserSnapshot first to see what's on the page."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string", "description": "Text, css:..., or label:..."},
                },
                "required": ["selector"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserFill",
            "description": (
                "Type text into an input field or textarea. "
                "selector can be: placeholder text, 'label:Email', or 'css:input[name=q]'."
            ),
            "parameters": {
                "type": "object",
                "properties": {
                    "selector": {"type": "string", "description": "Placeholder, label:..., or css:..."},
                    "value":    {"type": "string", "description": "Text to type"},
                },
                "required": ["selector", "value"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserPressKey",
            "description": "Press a keyboard key (e.g. Enter, Tab, Escape, ArrowDown).",
            "parameters": {
                "type": "object",
                "properties": {
                    "key": {"type": "string", "description": "Key name: Enter, Tab, Escape, ArrowDown, etc."},
                },
                "required": ["key"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserScroll",
            "description": "Scroll the page up, down, left, or right.",
            "parameters": {
                "type": "object",
                "properties": {
                    "direction": {"type": "string", "enum": ["up", "down", "left", "right"], "description": "Scroll direction"},
                    "amount":    {"type": "integer", "description": "Number of scroll steps (default 3)"},
                },
                "required": [],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserBack",
            "description": "Navigate back to the previous page in browser history.",
            "parameters": {"type": "object", "properties": {}, "required": []},
        },
    },
    {
        "type": "function",
        "function": {
            "name": "BrowserClose",
            "description": "Close the browser and free resources. Call when browser tasks are complete.",
            "parameters": {"type": "object", "properties": {}, "required": []},
        },
    },
]


def execute_tool(name: str, args: dict[str, Any]) -> str:
    # Browser tools are imported lazily so playwright is only loaded when used
    import browser as _br

    dispatch = {
        "Bash":              lambda a: bash(a["command"]),
        "Read":              lambda a: read_file(a["file_path"], a.get("offset", 0), a.get("limit", 0)),
        "Write": lambda a: write_file(a["file_path"], a["content"]),
        "Edit":  lambda a: edit_file(a["file_path"], a["old_string"], a["new_string"]),
        "Glob":              lambda a: glob_search(a["pattern"], a.get("path")),
        "Grep":              lambda a: grep_search(a["pattern"], a.get("path"), a.get("glob")),
        "WebFetch":          lambda a: web_fetch(a["url"], a.get("max_chars", 8000)),
        "Remember":          lambda a: remember(a["key"], a["value"]),
        "Recall":            lambda a: recall(a.get("key", "")),
        # Browser
        "BrowserNavigate":   lambda a: _br.navigate(a["url"]),
        "BrowserSnapshot":   lambda a: _br.snapshot(),
        "BrowserClick":      lambda a: _br.click(a["selector"]),
        "BrowserFill":       lambda a: _br.fill(a["selector"], a["value"]),
        "BrowserPressKey":   lambda a: _br.press_key(a["key"]),
        "BrowserScroll":     lambda a: _br.scroll(a.get("direction", "down"), a.get("amount", 3)),
        "BrowserBack":       lambda a: _br.go_back(),
        "BrowserClose":      lambda a: _br.close(),
    }
    fn = dispatch.get(name)
    if fn is None:
        return f"[Unknown tool: {name}]"
    try:
        return fn(args)
    except KeyError as e:
        return f"[Missing required argument: {e}]"
    except Exception as e:
        return f"[Tool error ({name}): {e}]"
