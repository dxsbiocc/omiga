"""File system tools for Omiga."""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Dict

from omiga.tools.base import Tool, ToolContext, ToolResult


class ReadFileTool(Tool):
    """Tool for reading files."""

    name = "read_file"
    description = "Read content from a file"

    async def execute(self, path: str, **kwargs: Any) -> ToolResult:  # type: ignore[override]
        """Read a file.

        Args:
            path: Path to the file

        Returns:
            ToolResult with file content
        """
        try:
            file_path = Path(path)
            if not file_path.exists():
                return ToolResult.fail(f"File not found: {path}")
            if not file_path.is_file():
                return ToolResult.fail(f"Not a file: {path}")

            content = file_path.read_text(encoding="utf-8")
            return ToolResult.ok(
                data={"content": content, "size": len(content)},
                path=str(file_path),
            )
        except Exception as e:
            return ToolResult.fail(str(e))

    def schema(self) -> Dict[str, Any]:
        """Return the tool's JSON schema."""
        return {
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read",
                    }
                },
                "required": ["path"],
            },
        }


class WriteFileTool(Tool):
    """Tool for writing files."""

    name = "write_file"
    description = "Write content to a file"

    async def execute(
        self,
        path: str,
        content: str,
        mode: str = "w",
        **kwargs: Any,
    ) -> ToolResult:  # type: ignore[override]
        """Write to a file.

        Args:
            path: Path to the file
            content: Content to write
            mode: Write mode ('w' or 'a')

        Returns:
            ToolResult with write status
        """
        try:
            file_path = Path(path)
            file_path.parent.mkdir(parents=True, exist_ok=True)
            if mode == "a":
                file_path.write_text(file_path.read_text(encoding="utf-8") + content, encoding="utf-8")
            else:
                file_path.write_text(content, encoding="utf-8")
            return ToolResult.ok(
                data={"written": len(content)},
                path=str(file_path),
            )
        except Exception as e:
            return ToolResult.fail(str(e))

    def schema(self) -> Dict[str, Any]:
        """Return the tool's JSON schema."""
        return {
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write",
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write",
                    },
                    "mode": {
                        "type": "string",
                        "description": "Write mode: 'w' (write) or 'a' (append)",
                        "default": "w",
                    },
                },
                "required": ["path", "content"],
            },
        }


class ListDirTool(Tool):
    """Tool for listing directory contents."""

    name = "list_dir"
    description = "List contents of a directory"

    async def execute(self, path: str, **kwargs: Any) -> ToolResult:  # type: ignore[override]
        """List directory contents.

        Args:
            path: Path to the directory

        Returns:
            ToolResult with directory listing
        """
        try:
            dir_path = Path(path)
            if not dir_path.exists():
                return ToolResult.fail(f"Directory not found: {path}")
            if not dir_path.is_dir():
                return ToolResult.fail(f"Not a directory: {path}")

            entries = []
            for entry in dir_path.iterdir():
                entries.append(
                    {
                        "name": entry.name,
                        "is_dir": entry.is_dir(),
                        "size": entry.stat().st_size if entry.is_file() else 0,
                    }
                )

            return ToolResult.ok(
                data={"entries": entries, "count": len(entries)},
                path=str(dir_path),
            )
        except Exception as e:
            return ToolResult.fail(str(e))

    def schema(self) -> Dict[str, Any]:
        """Return the tool's JSON schema."""
        return {
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory to list",
                    }
                },
                "required": ["path"],
            },
        }


class FileExistsTool(Tool):
    """Tool for checking if a file exists."""

    name = "file_exists"
    description = "Check if a file or directory exists"

    async def execute(self, path: str, **kwargs: Any) -> ToolResult:  # type: ignore[override]
        """Check if a path exists.

        Args:
            path: Path to check

        Returns:
            ToolResult with exists boolean
        """
        try:
            exists = Path(path).exists()
            return ToolResult.ok(data={"exists": exists}, path=path)
        except Exception as e:
            return ToolResult.fail(str(e))

    def schema(self) -> Dict[str, Any]:
        """Return the tool's JSON schema."""
        return {
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to check",
                    }
                },
                "required": ["path"],
            },
        }
