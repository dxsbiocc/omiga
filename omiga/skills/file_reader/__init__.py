"""File reader skill for reading and summarizing text files."""
from __future__ import annotations

import mimetypes
from pathlib import Path
from typing import Any, Optional

from omiga.skills.base import Skill, SkillContext, SkillMetadata, SkillError


class FileReaderSkill(Skill):
    """Skill for reading and summarizing text files."""

    metadata = SkillMetadata(
        name="file_reader",
        description="读取和总结本地文本文件",
        emoji="📄",
        tags=["file", "read", "text"],
    )

    # Safe text-based extensions
    SAFE_EXTENSIONS = {
        ".txt",
        ".md",
        ".json",
        ".yaml",
        ".yml",
        ".csv",
        ".tsv",
        ".log",
        ".ini",
        ".toml",
        ".cfg",
        ".py",
        ".js",
        ".ts",
        ".go",
        ".rs",
        ".java",
        ".c",
        ".cpp",
        ".h",
        ".hpp",
        ".rb",
        ".php",
        ".html",
        ".xml",
        ".sql",
        ".sh",
        ".bash",
        ".zsh",
    }

    # Max file size to read (10MB)
    MAX_FILE_SIZE = 10 * 1024 * 1024

    # Max content length to return
    MAX_CONTENT_LENGTH = 50000

    def __init__(self, context: SkillContext):
        super().__init__(context)

    async def execute(
        self,
        file_path: str,
        action: str = "read",
        lines: Optional[int] = None,
        **kwargs: Any,
    ) -> Any:  # type: ignore[override]
        """Execute the file reader skill.

        Args:
            file_path: Path to the file to read
            action: Action to perform (read, summarize, tail, head)
            lines: Number of lines to read (for tail/head)
            **kwargs: Additional arguments

        Returns:
            File content or summary
        """
        path = Path(file_path)

        # Security checks
        await self._validate_file_path(path)

        if action == "read":
            return await self._read_file(path)
        elif action == "summarize":
            return await self._summarize_file(path)
        elif action == "tail":
            return await self._tail_file(path, lines or 20)
        elif action == "head":
            return await self._head_file(path, lines or 50)
        else:
            raise SkillError(f"Unknown action: {action}", self.name)

    async def _validate_file_path(self, path: Path) -> None:
        """Validate the file path for security."""
        # Resolve to absolute path
        resolved = path.resolve()

        # Check if file exists
        if not resolved.exists():
            raise SkillError(f"File not found: {path}", self.name)

        if not resolved.is_file():
            raise SkillError(f"Not a file: {path}", self.name)

        # Check extension
        ext = resolved.suffix.lower()
        if ext not in self.SAFE_EXTENSIONS:
            # Try mime type check as fallback
            mime_type, _ = mimetypes.guess_type(str(resolved))
            if mime_type and not mime_type.startswith("text/"):
                raise SkillError(
                    f"Unsupported file type: {ext} ({mime_type})", self.name
                )

        # Check file size
        file_size = resolved.stat().st_size
        if file_size > self.MAX_FILE_SIZE:
            raise SkillError(
                f"File too large: {file_size / 1024 / 1024:.1f}MB (max: {self.MAX_FILE_SIZE / 1024 / 1024:.0f}MB)",
                self.name,
            )

    async def _read_file(self, path: Path) -> dict:
        """Read file content."""
        try:
            content = path.read_text(encoding="utf-8")

            # Truncate if too long
            truncated = False
            if len(content) > self.MAX_CONTENT_LENGTH:
                content = content[: self.MAX_CONTENT_LENGTH] + "\n... (truncated)"
                truncated = True

            return {
                "file": str(path),
                "size": path.stat().st_size,
                "content": content,
                "truncated": truncated,
            }
        except UnicodeDecodeError as e:
            raise SkillError(f"Cannot read file (encoding error): {e}", self.name)
        except Exception as e:
            raise SkillError(f"Failed to read file: {e}", self.name)

    async def _summarize_file(self, path: Path) -> dict:
        """Summarize file content."""
        try:
            content = path.read_text(encoding="utf-8")
            lines = content.splitlines()

            summary = {
                "file": str(path),
                "size": path.stat().st_size,
                "total_lines": len(lines),
                "type": path.suffix.lower(),
            }

            # Type-specific summaries
            if path.suffix.lower() == ".json":
                summary["format"] = "JSON"
                summary["note"] = "JSON structure - use parse_json for detailed analysis"
            elif path.suffix.lower() in {".yaml", ".yml"}:
                summary["format"] = "YAML"
            elif path.suffix.lower() == ".csv":
                if lines:
                    summary["headers"] = lines[0].split(",")[:10]
                    summary["data_rows"] = len(lines) - 1
            elif path.suffix.lower() in {".py", ".js", ".ts", ".go", ".rs"}:
                # Count functions and classes
                functions = sum(1 for line in lines if "def " in line or "function " in line)
                classes = sum(1 for line in lines if "class " in line)
                summary["functions"] = functions
                summary["classes"] = classes
            else:
                # Show first few lines as preview
                preview = "\n".join(lines[:5])
                if len(lines) > 5:
                    preview += "\n..."
                summary["preview"] = preview

            return summary
        except Exception as e:
            raise SkillError(f"Failed to summarize file: {e}", self.name)

    async def _tail_file(self, path: Path, lines: int) -> dict:
        """Read last N lines of file."""
        try:
            content = path.read_text(encoding="utf-8")
            all_lines = content.splitlines()
            tail_lines = all_lines[-lines:] if len(all_lines) > lines else all_lines

            return {
                "file": str(path),
                "action": f"tail -{lines}",
                "content": "\n".join(tail_lines),
                "total_lines": len(all_lines),
            }
        except Exception as e:
            raise SkillError(f"Failed to tail file: {e}", self.name)

    async def _head_file(self, path: Path, lines: int) -> dict:
        """Read first N lines of file."""
        try:
            content = path.read_text(encoding="utf-8")
            all_lines = content.splitlines()
            head_lines = all_lines[:lines]

            return {
                "file": str(path),
                "action": f"head -{lines}",
                "content": "\n".join(head_lines),
                "total_lines": len(all_lines),
            }
        except Exception as e:
            raise SkillError(f"Failed to head file: {e}", self.name)

    def get_usage(self) -> str:
        """Return usage instructions."""
        return """
File Reader Skill - 文件读取

可用操作:
- read <file_path>: 读取文件内容
- summarize <file_path>: 总结文件
- tail <file_path> [lines]: 读取文件末尾 (默认 20 行)
- head <file_path> [lines]: 读取文件开头 (默认 50 行)

支持的文件类型:
- 文本：.txt, .md, .json, .yaml, .csv
- 日志：.log
- 代码：.py, .js, .ts, .go, .rs, 等

示例:
- 读取文件：execute(action="read", file_path="/path/to/file.txt")
- 查看日志末尾：execute(action="tail", file_path="app.log", lines=50)
"""
