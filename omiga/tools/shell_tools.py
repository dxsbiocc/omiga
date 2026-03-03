"""Shell command execution tools for Omiga."""
from __future__ import annotations

import asyncio
import shlex
from typing import Any, Dict, List, Optional

from omiga.tools.base import Tool, ToolContext, ToolResult


class ExecuteCommandTool(Tool):
    """Tool for executing shell commands."""

    name = "execute_command"
    description = "Execute a shell command and return output"

    # Allowed commands whitelist (can be extended)
    ALLOWED_COMMANDS = {
        "ls",
        "cat",
        "head",
        "tail",
        "wc",
        "grep",
        "find",
        "pwd",
        "echo",
        "mkdir",
        "cp",
        "mv",
        "rm",
        "touch",
        "date",
        "whoami",
        "uname",
        "file",
        "sort",
        "uniq",
        "curl",
        "wget",
        "sleep",
    }

    async def execute(
        self,
        command: str,
        timeout: int = 60,
        **kwargs: Any,
    ) -> ToolResult:  # type: ignore[override]
        """Execute a shell command.

        Args:
            command: The command to execute
            timeout: Timeout in seconds

        Returns:
            ToolResult with command output
        """
        # Security: validate command
        validation = self._validate_command(command)
        if not validation[0]:
            return ToolResult.fail(validation[1])

        try:
            process = await asyncio.create_subprocess_shell(
                command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            try:
                stdout, stderr = await asyncio.wait_for(
                    process.communicate(),
                    timeout=timeout,
                )
            except asyncio.TimeoutError:
                process.kill()
                return ToolResult.fail(f"Command timed out after {timeout}s")

            return ToolResult.ok(
                data={
                    "stdout": stdout.decode("utf-8", errors="replace"),
                    "stderr": stderr.decode("utf-8", errors="replace"),
                    "returncode": process.returncode,
                }
            )

        except Exception as e:
            return ToolResult.fail(str(e))

    def _validate_command(self, command: str) -> tuple[bool, str]:
        """Validate that a command is safe to execute.

        Returns:
            (is_valid, error_message)
        """
        # Parse the command
        try:
            parts = shlex.split(command)
        except ValueError as e:
            return False, f"Invalid command syntax: {e}"

        if not parts:
            return False, "Empty command"

        # Check base command
        base_cmd = parts[0]

        # Allow paths like /usr/bin/git
        if "/" in base_cmd:
            base_cmd = base_cmd.split("/")[-1]

        # Check against whitelist
        if base_cmd not in self.ALLOWED_COMMANDS:
            return False, f"Command not allowed: {base_cmd}. Allowed: {', '.join(sorted(self.ALLOWED_COMMANDS))}"

        # Block dangerous patterns
        dangerous_patterns = ["|", ";", "&&", "||", "`", "$("]
        for pattern in dangerous_patterns:
            if pattern in command:
                return False, f"Command contains disallowed pattern: {pattern}"

        return True, ""

    def schema(self) -> Dict[str, Any]:
        """Return the tool's JSON schema."""
        return {
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute",
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds",
                        "default": 60,
                    },
                },
                "required": ["command"],
            },
        }
