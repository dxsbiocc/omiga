"""
MCP (Model Context Protocol) stdio transport client for the NanoClaw Python agent.

Reads mcp_servers.json, spawns each server as a subprocess, performs the
JSON-RPC handshake, and exposes their tools to the OpenAI tool-use loop.

Protocol flow (MCP 2024-11-05 spec):
  → initialize(protocolVersion, capabilities, clientInfo)
  ← InitializeResult
  → notifications/initialized   (notification, no id)
  → tools/list
  ← {tools: [...]}
  → tools/call(name, arguments)
  ← {content: [...], isError?}

mcp_servers.json format (same as Claude Code / MCP ecosystem):
  {
    "mcpServers": {
      "<name>": {
        "command": "uvx",
        "args": ["mcp-server-sqlite", "--db-path", "/workspace/group/db.sqlite"],
        "env": {}          // optional extra env vars
      }
    }
  }
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any

# Path where container_runner mounts the config (read-only)
MCP_CONFIG_PATH = os.environ.get("NANOCLAW_MCP_CONFIG", "/workspace/mcp_servers.json")

_JSONRPC = "2.0"
_PROTOCOL_VERSION = "2024-11-05"


def _log(msg: str) -> None:
    print(f"[mcp] {msg}", file=sys.stderr, flush=True)


# ── Per-server connection ──────────────────────────────────────────────────

class McpServer:
    """Manages one stdio MCP server subprocess."""

    def __init__(self, name: str, command: str, args: list[str], env: dict[str, str]) -> None:
        self.name = name
        self._proc: subprocess.Popen | None = None
        self._id = 0
        self._lock = threading.Lock()

        merged_env = {**os.environ, **env}
        try:
            self._proc = subprocess.Popen(
                [command, *args],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=merged_env,
                text=True,
                bufsize=1,
            )
        except FileNotFoundError as exc:
            _log(f"  {name}: command not found — {command} ({exc})")
            self._proc = None
        except Exception as exc:
            _log(f"  {name}: failed to start — {exc}")
            self._proc = None

    def _next_id(self) -> int:
        with self._lock:
            self._id += 1
            return self._id

    def _send(self, obj: dict) -> None:
        if self._proc is None or self._proc.stdin is None:
            raise RuntimeError("server not running")
        line = json.dumps(obj) + "\n"
        self._proc.stdin.write(line)
        self._proc.stdin.flush()

    def _recv(self) -> dict:
        if self._proc is None or self._proc.stdout is None:
            raise RuntimeError("server not running")
        while True:
            line = self._proc.stdout.readline()
            if not line:
                raise RuntimeError("server stdout closed")
            line = line.strip()
            if not line:
                continue
            return json.loads(line)

    def _rpc(self, method: str, params: dict | None = None) -> Any:
        rid = self._next_id()
        req: dict = {"jsonrpc": _JSONRPC, "id": rid, "method": method}
        if params is not None:
            req["params"] = params
        self._send(req)
        # Read responses until we see one with our id (skip notifications)
        for _ in range(50):
            resp = self._recv()
            if resp.get("id") == rid:
                if "error" in resp:
                    raise RuntimeError(f"RPC error: {resp['error']}")
                return resp.get("result")
        raise RuntimeError(f"No response for id={rid}")

    def _notify(self, method: str, params: dict | None = None) -> None:
        msg: dict = {"jsonrpc": _JSONRPC, "method": method}
        if params is not None:
            msg["params"] = params
        self._send(msg)

    def initialize(self) -> bool:
        if self._proc is None:
            return False
        try:
            self._rpc("initialize", {
                "protocolVersion": _PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "clientInfo": {"name": "nanoclaw-py-agent", "version": "1.0"},
            })
            self._notify("notifications/initialized")
            return True
        except Exception as exc:
            _log(f"  {self.name}: initialize failed — {exc}")
            return False

    def list_tools(self) -> list[dict]:
        try:
            result = self._rpc("tools/list")
            return result.get("tools", []) if result else []
        except Exception as exc:
            _log(f"  {self.name}: tools/list failed — {exc}")
            return []

    def call_tool(self, tool_name: str, arguments: dict) -> str:
        try:
            result = self._rpc("tools/call", {"name": tool_name, "arguments": arguments})
            if result is None:
                return "(no result)"
            content = result.get("content", [])
            parts: list[str] = []
            for item in content:
                if isinstance(item, dict):
                    if item.get("type") == "text":
                        parts.append(item.get("text", ""))
                    elif item.get("type") == "image":
                        parts.append(f"[image: {item.get('mimeType', 'unknown')}]")
                    elif item.get("type") == "resource":
                        res = item.get("resource", {})
                        parts.append(res.get("text") or f"[resource: {res.get('uri', '?')}]")
                    else:
                        parts.append(json.dumps(item))
            if result.get("isError"):
                return "[MCP tool error]\n" + "\n".join(parts)
            return "\n".join(parts) or "(empty)"
        except Exception as exc:
            return f"[MCP call failed: {exc}]"

    def close(self) -> None:
        if self._proc is not None:
            try:
                self._proc.stdin.close() if self._proc.stdin else None
                self._proc.terminate()
                self._proc.wait(timeout=5)
            except Exception:
                pass
            self._proc = None


# ── MCP registry ──────────────────────────────────────────────────────────

class McpRegistry:
    """Loads mcp_servers.json, starts servers, and exposes a unified tool interface."""

    def __init__(self) -> None:
        self._servers: dict[str, McpServer] = {}
        # Maps "ServerName__tool_name" → (server, original_tool_name)
        self._tool_map: dict[str, tuple[McpServer, str]] = {}
        # OpenAI tool definitions for all MCP tools
        self.tool_definitions: list[dict] = []

    def load(self, config_path: str = MCP_CONFIG_PATH) -> None:
        path = Path(config_path)
        if not path.exists():
            return

        try:
            cfg = json.loads(path.read_text())
        except Exception as exc:
            _log(f"Failed to parse {config_path}: {exc}")
            return

        servers_cfg: dict = cfg.get("mcpServers", {})
        if not servers_cfg:
            return

        _log(f"Loading {len(servers_cfg)} MCP server(s) from {config_path}")

        for name, spec in servers_cfg.items():
            command = spec.get("command", "")
            args = spec.get("args", [])
            env = spec.get("env") or {}

            if not command:
                _log(f"  {name}: missing 'command', skipping")
                continue

            _log(f"  Starting: {name} ({command} {' '.join(args)})")
            srv = McpServer(name, command, args, env)

            if not srv.initialize():
                srv.close()
                continue

            tools = srv.list_tools()
            _log(f"  {name}: {len(tools)} tool(s) available")

            for tool in tools:
                tool_name = tool.get("name", "")
                if not tool_name:
                    continue

                # Namespace tool as "ServerName__tool_name" to avoid collisions
                namespaced = f"{name}__{tool_name}"
                self._tool_map[namespaced] = (srv, tool_name)

                # Convert MCP tool schema → OpenAI function definition
                input_schema = tool.get("inputSchema") or {"type": "object", "properties": {}}
                self.tool_definitions.append({
                    "type": "function",
                    "function": {
                        "name": namespaced,
                        "description": (
                            f"[MCP:{name}] {tool.get('description', '')}"
                        ),
                        "parameters": input_schema,
                    },
                })

            self._servers[name] = srv

        _log(f"MCP ready: {len(self._tool_map)} tool(s) from {len(self._servers)} server(s)")

    def call(self, namespaced_name: str, arguments: dict) -> str:
        entry = self._tool_map.get(namespaced_name)
        if not entry:
            return f"[MCP] Unknown tool: {namespaced_name}"
        srv, original_name = entry
        return srv.call_tool(original_name, arguments)

    def close_all(self) -> None:
        for srv in self._servers.values():
            srv.close()
        self._servers.clear()
        self._tool_map.clear()
        self.tool_definitions.clear()


# Module-level singleton — loaded once per agent process
_registry: McpRegistry | None = None


def get_registry() -> McpRegistry:
    global _registry
    if _registry is None:
        _registry = McpRegistry()
        _registry.load()
    return _registry
