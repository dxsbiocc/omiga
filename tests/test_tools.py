"""Tests for Omiga tools system."""
import pytest
from pathlib import Path
from tempfile import TemporaryDirectory

from omiga.tools.base import Tool, ToolContext, ToolResult
from omiga.tools.file_tools import (
    ReadFileTool,
    WriteFileTool,
    ListDirTool,
    FileExistsTool,
)
from omiga.tools.registry import ToolRegistry
from omiga.tools.shell_tools import ExecuteCommandTool


class TestToolBase:
    """Tests for Tool base classes."""

    def test_tool_context(self):
        """Test tool context creation."""
        context = ToolContext(
            working_dir="/tmp",
            data_dir="/tmp/data",
            env_vars={"TEST": "value"},
        )
        assert context.working_dir == "/tmp"
        assert context.env_vars["TEST"] == "value"

    def test_tool_result_ok(self):
        """Test ToolResult.ok factory."""
        result = ToolResult.ok(data={"key": "value"}, extra="info")
        assert result.success is True
        assert result.data == {"key": "value"}
        assert result.error is None
        assert result.metadata["extra"] == "info"

    def test_tool_result_fail(self):
        """Test ToolResult.fail factory."""
        result = ToolResult.fail(error="Something failed", code=500)
        assert result.success is False
        assert result.error == "Something failed"
        assert result.metadata["code"] == 500


class TestFileTools:
    """Tests for file system tools."""

    @pytest.mark.asyncio
    async def test_read_file(self):
        """Test reading a file."""
        with TemporaryDirectory() as tmpdir:
            # Create test file
            test_file = Path(tmpdir) / "test.txt"
            test_file.write_text("Hello, World!")

            context = ToolContext(working_dir=tmpdir, data_dir=tmpdir)
            tool = ReadFileTool(context)
            result = await tool.execute(path=str(test_file))

            assert result.success is True
            assert result.data["content"] == "Hello, World!"
            assert result.data["size"] == 13

    @pytest.mark.asyncio
    async def test_read_file_not_found(self):
        """Test reading nonexistent file."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ReadFileTool(context)
        result = await tool.execute(path="/tmp/nonexistent.txt")

        assert result.success is False
        assert "not found" in result.error.lower()

    @pytest.mark.asyncio
    async def test_write_file(self):
        """Test writing a file."""
        with TemporaryDirectory() as tmpdir:
            test_file = Path(tmpdir) / "output.txt"

            context = ToolContext(working_dir=tmpdir, data_dir=tmpdir)
            tool = WriteFileTool(context)
            result = await tool.execute(
                path=str(test_file),
                content="Test content",
            )

            assert result.success is True
            assert test_file.exists()
            assert test_file.read_text() == "Test content"

    @pytest.mark.asyncio
    async def test_write_file_append(self):
        """Test appending to a file."""
        with TemporaryDirectory() as tmpdir:
            test_file = Path(tmpdir) / "output.txt"
            test_file.write_text("First line\n")

            context = ToolContext(working_dir=tmpdir, data_dir=tmpdir)
            tool = WriteFileTool(context)
            result = await tool.execute(
                path=str(test_file),
                content="Second line\n",
                mode="a",
            )

            assert result.success is True
            content = test_file.read_text()
            assert "First line" in content
            assert "Second line" in content

    @pytest.mark.asyncio
    async def test_list_dir(self):
        """Test listing directory contents."""
        with TemporaryDirectory() as tmpdir:
            # Create test files
            (Path(tmpdir) / "file1.txt").write_text("a")
            (Path(tmpdir) / "file2.txt").write_text("b")
            (Path(tmpdir) / "subdir").mkdir()

            context = ToolContext(working_dir=tmpdir, data_dir=tmpdir)
            tool = ListDirTool(context)
            result = await tool.execute(path=tmpdir)

            assert result.success is True
            assert result.data["count"] == 3
            names = [e["name"] for e in result.data["entries"]]
            assert "file1.txt" in names
            assert "file2.txt" in names
            assert "subdir" in names

    @pytest.mark.asyncio
    async def test_file_exists(self):
        """Test checking file existence."""
        with TemporaryDirectory() as tmpdir:
            test_file = Path(tmpdir) / "exists.txt"
            test_file.write_text("content")

            context = ToolContext(working_dir=tmpdir, data_dir=tmpdir)
            tool = FileExistsTool(context)

            result1 = await tool.execute(path=str(test_file))
            result2 = await tool.execute(path=str(Path(tmpdir) / "nonexistent.txt"))

            assert result1.success is True
            assert result1.data["exists"] is True
            assert result2.success is True
            assert result2.data["exists"] is False

    @pytest.mark.asyncio
    async def test_read_file_schema(self):
        """Test ReadFileTool schema."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ReadFileTool(context)
        schema = tool.schema()

        assert schema["name"] == "read_file"
        assert "path" in schema["parameters"]["properties"]
        assert "path" in schema["parameters"]["required"]


class TestShellTools:
    """Tests for shell command tools."""

    @pytest.mark.asyncio
    async def test_execute_allowed_command(self):
        """Test executing an allowed command."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ExecuteCommandTool(context)
        result = await tool.execute(command="echo hello")

        assert result.success is True
        assert "hello" in result.data["stdout"]

    @pytest.mark.asyncio
    async def test_execute_disallowed_command(self):
        """Test executing a disallowed command."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ExecuteCommandTool(context)
        # Use a command that's not in the whitelist
        result = await tool.execute(command="python3 -c 'print(1)'")

        assert result.success is False
        assert "not allowed" in result.error

    @pytest.mark.asyncio
    async def test_execute_command_with_pipe(self):
        """Test that pipes are blocked."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ExecuteCommandTool(context)
        result = await tool.execute(command="ls | grep test")

        assert result.success is False
        assert "disallowed pattern" in result.error

    @pytest.mark.asyncio
    async def test_execute_command_timeout(self):
        """Test command timeout."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ExecuteCommandTool(context)
        # Use sleep which is now in the whitelist
        result = await tool.execute(command="sleep 10", timeout=1)

        assert result.success is False
        assert "timed out" in result.error

    def test_execute_command_schema(self):
        """Test ExecuteCommandTool schema."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        tool = ExecuteCommandTool(context)
        schema = tool.schema()

        assert schema["name"] == "execute_command"
        assert "command" in schema["parameters"]["properties"]
        assert "timeout" in schema["parameters"]["properties"]


class TestToolRegistry:
    """Tests for ToolRegistry."""

    def test_registry_register_tool(self):
        """Test registering a tool instance."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        registry = ToolRegistry(context)
        tool = ReadFileTool(context)
        registry.register(tool)

        assert registry.get_tool("read_file") is tool
        assert "read_file" in registry.list_tools()

    def test_registry_register_class(self):
        """Test registering a tool class."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        registry = ToolRegistry(context)
        registry.register_class(ReadFileTool)

        tool = registry.get_tool("read_file")
        assert tool is not None
        assert isinstance(tool, ReadFileTool)

    def test_registry_get_nonexistent(self):
        """Test getting a nonexistent tool."""
        registry = ToolRegistry()
        tool = registry.get_tool("nonexistent")
        assert tool is None

    @pytest.mark.asyncio
    async def test_registry_execute(self):
        """Test executing a tool through registry."""
        with TemporaryDirectory() as tmpdir:
            context = ToolContext(working_dir=tmpdir, data_dir=tmpdir)
            registry = ToolRegistry(context)
            registry.register_class(ReadFileTool)

            test_file = Path(tmpdir) / "test.txt"
            test_file.write_text("content")

            result = await registry.execute_tool("read_file", path=str(test_file))
            assert result.success is True

    @pytest.mark.asyncio
    async def test_registry_execute_nonexistent(self):
        """Test executing nonexistent tool."""
        registry = ToolRegistry()
        with pytest.raises(ValueError, match="not found"):
            await registry.execute_tool("nonexistent")

    def test_registry_get_schema(self):
        """Test getting tool schema through registry."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        registry = ToolRegistry(context)
        registry.register_class(ReadFileTool)

        schema = registry.get_schema("read_file")
        assert schema is not None
        assert schema["name"] == "read_file"

    def test_registry_get_all_schemas(self):
        """Test getting all tool schemas."""
        context = ToolContext(working_dir="/tmp", data_dir="/tmp")
        registry = ToolRegistry(context)
        registry.register_class(ReadFileTool)
        registry.register_class(WriteFileTool)

        schemas = registry.get_all_schemas()
        assert len(schemas) == 2
        names = [s["name"] for s in schemas]
        assert "read_file" in names
        assert "write_file" in names
