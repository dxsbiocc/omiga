"""
Container runner for Omiga Python port.

Spawns agent containers via asyncio.subprocess, handles sentinel-wrapped
JSON streaming output, and constructs volume mounts.

Mirrors src/container-runner.ts.
"""
from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Optional

from omiga.config import (
    ASSISTANT_NAME,
    CONTAINER_IMAGE,
    CONTAINER_MAX_OUTPUT_SIZE,
    CONTAINER_TIMEOUT,
    DATA_DIR,
    GROUPS_DIR,
    IDLE_TIMEOUT,
    TIMEZONE,
)
from omiga.container.runtime import CONTAINER_RUNTIME_BIN, readonly_mount_args, stop_container_cmd
from omiga.group_folder import resolve_group_folder_path, resolve_group_ipc_path
from omiga.models import (
    AvailableGroup,
    ContainerInput,
    ContainerOutput,
    RegisteredGroup,
    VolumeMount,
)
from omiga.config import PROJECT_ROOT
from omiga.container.mount_security import validate_additional_mounts

logger = logging.getLogger(__name__)

OUTPUT_START_MARKER = b"---NANOCLAW_OUTPUT_START---"
OUTPUT_END_MARKER = b"---NANOCLAW_OUTPUT_END---"

_ERROR_DUMPS_DIR = DATA_DIR / "logs" / "errors"


def write_error_dump(
    group: "RegisteredGroup",
    input_data: "ContainerInput",
    error: str,
    stderr: str,
    duration_ms: int,
    exit_code: Optional[int],
) -> Path:
    """Write a structured JSON error dump to data/logs/errors/.

    Returns the path of the written file so callers can log it.
    """
    _ERROR_DUMPS_DIR.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc)
    ts_file = ts.strftime("%Y%m%dT%H%M%S")
    dump_file = _ERROR_DUMPS_DIR / f"{group.folder.replace('/', '-')}-{ts_file}.json"

    payload = {
        "timestamp": ts.isoformat(),
        "group_folder": group.folder,
        "group_name": group.name,
        "session_id": input_data.session_id,
        "is_main": input_data.is_main,
        "duration_ms": duration_ms,
        "exit_code": exit_code,
        "error": error,
        "prompt_preview": (input_data.prompt or "")[:500],
        "stderr_tail": stderr[-2000:] if stderr else "",
    }
    try:
        dump_file.write_text(json.dumps(payload, ensure_ascii=False, indent=2))
    except Exception as exc:
        logger.warning("Failed to write error dump: %s", exc)
    return dump_file


async def ensure_image() -> None:
    """Auto-build the container image if it is not present locally.

    For ``omiga-py-agent:latest``: if the agent script exists locally the
    image is not needed (direct-run mode), so this is a no-op.
    For other images: checks with ``docker image inspect`` and builds if missing.
    """
    if _use_direct_mode():
        logger.info(
            "Direct-run mode: agent.py executed in-process, no Docker image needed."
        )
        return
    # Map image name → build directory (relative to project root)
    _build_dirs: dict[str, str] = {
        "omiga-py-agent:latest": "container/py-runner",
        "omiga-agent:latest":    "container",
    }

    # Check whether the image already exists (fast path)
    check = await asyncio.create_subprocess_exec(
        CONTAINER_RUNTIME_BIN, "image", "inspect", CONTAINER_IMAGE,
        stdout=asyncio.subprocess.DEVNULL,
        stderr=asyncio.subprocess.DEVNULL,
    )
    await check.wait()
    if check.returncode == 0:
        return  # image present — nothing to do

    build_dir_rel = _build_dirs.get(CONTAINER_IMAGE)
    if not build_dir_rel:
        logger.warning(
            "Image '%s' not found and no auto-build rule defined. "
            "Build it manually before starting.",
            CONTAINER_IMAGE,
        )
        return

    build_dir = PROJECT_ROOT / build_dir_rel
    if not build_dir.exists():
        logger.error("Build directory not found: %s", build_dir)
        return

    logger.info(
        "Image '%s' not found — building from %s (first-run, may take a minute)…",
        CONTAINER_IMAGE, build_dir_rel,
    )
    proc = await asyncio.create_subprocess_exec(
        CONTAINER_RUNTIME_BIN, "build", "-t", CONTAINER_IMAGE, str(build_dir),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
    )
    stdout, _ = await proc.communicate()
    if proc.returncode == 0:
        logger.info("Image '%s' built successfully.", CONTAINER_IMAGE)
    else:
        logger.error(
            "Failed to build image '%s':\n%s",
            CONTAINER_IMAGE, stdout.decode(errors="replace"),
        )

# Python runner script path (used in direct mode, no Docker required)
_PY_RUNNER_SCRIPT = PROJECT_ROOT / "container" / "py-runner" / "agent.py"

# Images that support direct (no-Docker) execution via _PY_RUNNER_SCRIPT.
# The Python runner image now uses Docker for proper isolation; direct mode
# is kept only as a development fallback activated by NANOCLAW_DIRECT_MODE=1.
_DIRECT_RUN_IMAGES: set[str] = set()


def _use_direct_mode() -> bool:
    """Return True only when explicitly opted-in via env var (dev/testing only)."""
    return os.environ.get("NANOCLAW_DIRECT_MODE", "").strip() == "1" and _PY_RUNNER_SCRIPT.exists()


OnOutputCallback = Callable[[ContainerOutput], Any]


def _read_secrets() -> dict[str, str]:
    """Read auth secrets from .env (never from env vars populated by parent).

    Supports two container runtimes:

    omiga-agent (TypeScript / Claude Code):
      ANTHROPIC_API_KEY   — Anthropic API key, OR
      CLAUDE_CODE_OAUTH_TOKEN — OAuth token for Claude Code

    omiga-py-agent (Python / OpenAI-compatible):
      AI_API_KEY   — provider API key (DeepSeek / Qwen / Minimax / …)
      AI_BASE_URL  — provider base URL (e.g. https://api.deepseek.com/v1)
      AI_MODEL     — model name        (e.g. deepseek-chat, qwen-plus)
    """
    from dotenv import dotenv_values
    # .env file is at project root (parent of omiga package directory)
    env_file = Path(__file__).parent.parent.parent / ".env"
    env = dotenv_values(str(env_file)) if env_file.exists() else {}
    secrets: dict[str, str] = {}
    for key in (
        # TypeScript runner (Anthropic)
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        # Python runner (OpenAI-compatible providers)
        "AI_API_KEY",
        "AI_BASE_URL",
        "AI_MODEL",
    ):
        val = env.get(key) or os.environ.get(key)
        if val:
            secrets[key] = val
    return secrets


def _build_volume_mounts(group: RegisteredGroup, is_main: bool) -> list[VolumeMount]:
    mounts: list[VolumeMount] = []
    project_root = Path.cwd()
    group_dir = resolve_group_folder_path(group.folder)

    if is_main:
        mounts.append(VolumeMount(
            host_path=str(project_root),
            container_path="/workspace/project",
            readonly=True,
        ))
        mounts.append(VolumeMount(
            host_path=str(group_dir),
            container_path="/workspace/group",
            readonly=False,
        ))
    else:
        mounts.append(VolumeMount(
            host_path=str(group_dir),
            container_path="/workspace/group",
            readonly=False,
        ))

    # Global dir: always mounted read-write so the agent can update PROFILE.md.
    # For the main group it serves as the global workspace; for other groups it
    # carries the shared user profile and global instructions.
    global_dir = GROUPS_DIR / "global"
    global_dir.mkdir(parents=True, exist_ok=True)
    if not is_main:
        mounts.append(VolumeMount(
            host_path=str(global_dir),
            container_path="/workspace/global",
            readonly=False,
        ))

    # Persistent Python/Node env directory — survives container restarts.
    # Agent can install packages here with:
    #   pip install --prefix /workspace/env <pkg>
    #   (PYTHONPATH is pre-configured by the entrypoint)
    group_env_dir = DATA_DIR / "envs" / group.folder
    group_env_dir.mkdir(parents=True, exist_ok=True)
    mounts.append(VolumeMount(
        host_path=str(group_env_dir),
        container_path="/workspace/env",
        readonly=False,
    ))

    # Per-group Claude sessions directory
    group_sessions_dir = DATA_DIR / "sessions" / group.folder / ".claude"
    group_sessions_dir.mkdir(parents=True, exist_ok=True)
    settings_file = group_sessions_dir / "settings.json"
    if not settings_file.exists():
        settings_file.write_text(
            json.dumps(
                {
                    "env": {
                        "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1",
                        "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD": "1",
                        "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "0",
                    }
                },
                indent=2,
            )
            + "\n"
        )

    # Sync container skills into per-group .claude/skills/
    skills_src = project_root / "container" / "skills"
    skills_dst = group_sessions_dir / "skills"
    if skills_src.exists():
        for skill_dir in skills_src.iterdir():
            if skill_dir.is_dir():
                dst = skills_dst / skill_dir.name
                shutil.copytree(str(skill_dir), str(dst), dirs_exist_ok=True)

    mounts.append(VolumeMount(
        host_path=str(group_sessions_dir),
        container_path="/home/node/.claude",
        readonly=False,
    ))

    # Per-group IPC directory
    group_ipc_dir = resolve_group_ipc_path(group.folder)
    for subdir in ("messages", "tasks", "input"):
        (group_ipc_dir / subdir).mkdir(parents=True, exist_ok=True)
    mounts.append(VolumeMount(
        host_path=str(group_ipc_dir),
        container_path="/workspace/ipc",
        readonly=False,
    ))

    # Copy agent-runner source into a per-group writable location
    agent_runner_src = project_root / "container" / "agent-runner" / "src"
    group_agent_runner_dir = DATA_DIR / "sessions" / group.folder / "agent-runner-src"
    if not group_agent_runner_dir.exists() and agent_runner_src.exists():
        shutil.copytree(str(agent_runner_src), str(group_agent_runner_dir))
    mounts.append(VolumeMount(
        host_path=str(group_agent_runner_dir),
        container_path="/app/src",
        readonly=False,
    ))

    # MCP servers config — user-editable, no image rebuild needed.
    # Located at data/mcp/{group.folder}/mcp_servers.json.
    # Create an empty config on first run so the agent can explain how to use it.
    mcp_config_path = DATA_DIR / "mcp" / group.folder / "mcp_servers.json"
    mcp_config_path.parent.mkdir(parents=True, exist_ok=True)
    if not mcp_config_path.exists():
        mcp_config_path.write_text(
            json.dumps(
                {
                    "_comment": (
                        "Add MCP servers here. Changes take effect on the next "
                        "conversation — no container rebuild needed. "
                        "See: https://modelcontextprotocol.io/docs/concepts/servers"
                    ),
                    "mcpServers": {},
                },
                indent=2,
                ensure_ascii=False,
            )
            + "\n"
        )
    mounts.append(VolumeMount(
        host_path=str(mcp_config_path),
        container_path="/workspace/mcp_servers.json",
        readonly=True,
    ))

    # Additional mounts (validated against allowlist)
    if group.container_config and group.container_config.additional_mounts:
        extra = validate_additional_mounts(
            group.container_config.additional_mounts,
            group.name,
            is_main,
        )
        mounts.extend(extra)

    return mounts


def _build_container_args(mounts: list[VolumeMount], container_name: str) -> list[str]:
    args = [CONTAINER_RUNTIME_BIN, "run", "-i", "--rm", "--name", container_name]

    args += ["-e", f"TZ={TIMEZONE}"]

    # Pass host UID/GID so the entrypoint creates a matching user inside the
    # container.  Files written to mounted volumes are then owned by the host
    # user, not root.  The entrypoint handles sudo setup and privilege drop.
    uid = os.getuid() if hasattr(os, "getuid") else 1000
    gid = os.getgid() if hasattr(os, "getgid") else 1000
    args += ["-e", f"HOST_UID={uid}", "-e", f"HOST_GID={gid}"]

    for m in mounts:
        if m.readonly:
            args += readonly_mount_args(m.host_path, m.container_path)
        else:
            args += ["-v", f"{m.host_path}:{m.container_path}"]

    args.append(CONTAINER_IMAGE)
    return args


def _build_input_json(input_data: ContainerInput) -> bytes:
    """Serialize ContainerInput to JSON bytes (injects secrets, then clears them)."""
    input_data.secrets = _read_secrets()
    payload = json.dumps({
        "prompt": input_data.prompt,
        "sessionId": input_data.session_id,
        "groupFolder": input_data.group_folder,
        "chatJid": input_data.chat_jid,
        "isMain": input_data.is_main,
        "isScheduledTask": input_data.is_scheduled_task,
        "assistantName": input_data.assistant_name or ASSISTANT_NAME,
        "secrets": input_data.secrets,
    }).encode()
    input_data.secrets = None
    return payload


async def _spawn_direct(
    group: RegisteredGroup,
    input_data: ContainerInput,
    on_process: Callable[[asyncio.subprocess.Process, str], None],
) -> tuple[asyncio.subprocess.Process, str]:
    """Run agent.py directly as a subprocess (no Docker required)."""
    import sys

    group_dir = resolve_group_folder_path(group.folder)
    group_dir.mkdir(parents=True, exist_ok=True)
    global_dir = GROUPS_DIR / "global"
    ipc_dir = resolve_group_ipc_path(group.folder)
    for sub in ("messages", "tasks", "input"):
        (ipc_dir / sub).mkdir(parents=True, exist_ok=True)

    runner_dir = PROJECT_ROOT / "container" / "py-runner"
    env = {
        **os.environ,
        "NANOCLAW_WORKSPACE": str(group_dir),
        "NANOCLAW_GLOBAL_DIR": str(global_dir),
        "NANOCLAW_IPC_DIR": str(ipc_dir / "input"),
        "PYTHONPATH": str(runner_dir),
    }

    label = f"direct-{group.folder}-{int(time.time() * 1000)}"
    logger.info(
        "Spawning agent directly (no Docker): group=%s label=%s", group.name, label
    )

    proc = await asyncio.create_subprocess_exec(
        sys.executable, str(_PY_RUNNER_SCRIPT),
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env=env,
    )
    on_process(proc, label)
    return proc, label


async def run_container_agent(
    group: RegisteredGroup,
    input_data: ContainerInput,
    on_process: Callable[[asyncio.subprocess.Process, str], None],
    on_output: Optional[Callable[[ContainerOutput], Any]] = None,
) -> ContainerOutput:
    """
    Spawn an agent, feed it JSON via stdin, stream sentinel-wrapped
    JSON output, and return the final ContainerOutput.

    In direct mode (omiga-py-agent): runs agent.py as a subprocess.
    In container mode (omiga-agent): runs inside Docker.
    """
    start_time = time.monotonic()

    group_dir = resolve_group_folder_path(group.folder)
    group_dir.mkdir(parents=True, exist_ok=True)

    input_json = _build_input_json(input_data)

    container_name: str
    if _use_direct_mode():
        proc, container_name = await _spawn_direct(group, input_data, on_process)
    else:
        mounts = _build_volume_mounts(group, input_data.is_main)
        safe_name = group.folder.replace("/", "-").replace("\\", "-")
        container_name = f"omiga-{safe_name}-{int(time.time() * 1000)}"
        container_args = _build_container_args(mounts, container_name)

        logger.info(
            "Spawning container agent: group=%s container=%s mounts=%d is_main=%s",
            group.name, container_name, len(mounts), input_data.is_main,
        )

        proc = await asyncio.create_subprocess_exec(
            *container_args,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        on_process(proc, container_name)

    logs_dir = group_dir / "logs"
    logs_dir.mkdir(parents=True, exist_ok=True)

    # Write input and close stdin
    proc.stdin.write(input_json)
    await proc.stdin.drain()
    proc.stdin.close()

    stdout_chunks: list[bytes] = []
    stderr_chunks: list[bytes] = []
    stdout_total = 0
    stderr_total = 0
    stdout_truncated = False
    stderr_truncated = False

    parse_buffer = b""
    new_session_id: Optional[str] = None
    had_streaming_output = False
    output_chain: asyncio.Future = asyncio.get_event_loop().create_future()
    output_chain.set_result(None)

    timed_out = False
    config_timeout = (
        group.container_config.timeout if group.container_config and group.container_config.timeout
        else CONTAINER_TIMEOUT
    )
    # Grace period: hard timeout >= IDLE_TIMEOUT + 30s
    timeout_ms = max(config_timeout, IDLE_TIMEOUT + 30_000)
    timeout_s = timeout_ms / 1000

    loop = asyncio.get_event_loop()
    timeout_handle: Optional[asyncio.TimerHandle] = None

    def kill_on_timeout() -> None:
        nonlocal timed_out
        timed_out = True
        logger.error("Container timeout: group=%s container=%s", group.name, container_name)
        try:
            import subprocess
            subprocess.run(stop_container_cmd(container_name), capture_output=True, timeout=15)
        except Exception:
            if proc.returncode is None:
                proc.kill()

    def reset_timeout() -> None:
        nonlocal timeout_handle
        if timeout_handle:
            timeout_handle.cancel()
        timeout_handle = loop.call_later(timeout_s, kill_on_timeout)

    reset_timeout()

    async def read_stderr() -> None:
        nonlocal stderr_total, stderr_truncated
        assert proc.stderr
        while True:
            chunk = await proc.stderr.read(8192)
            if not chunk:
                break
            for line in chunk.decode(errors="replace").splitlines():
                if line.strip():
                    logger.debug("[container:%s] %s", group.folder, line)
            if not stderr_truncated:
                remaining = CONTAINER_MAX_OUTPUT_SIZE - stderr_total
                if len(chunk) > remaining:
                    stderr_chunks.append(chunk[:remaining])
                    stderr_truncated = True
                    logger.warning("Container stderr truncated: group=%s", group.name)
                else:
                    stderr_chunks.append(chunk)
                    stderr_total += len(chunk)

    async def read_stdout() -> None:
        nonlocal stdout_total, stdout_truncated, parse_buffer, new_session_id, had_streaming_output, output_chain
        assert proc.stdout
        while True:
            chunk = await proc.stdout.read(8192)
            if not chunk:
                break

            if not stdout_truncated:
                remaining = CONTAINER_MAX_OUTPUT_SIZE - stdout_total
                if len(chunk) > remaining:
                    stdout_chunks.append(chunk[:remaining])
                    stdout_truncated = True
                    logger.warning("Container stdout truncated: group=%s", group.name)
                else:
                    stdout_chunks.append(chunk)
                    stdout_total += len(chunk)

            if on_output:
                parse_buffer += chunk
                while True:
                    start_idx = parse_buffer.find(OUTPUT_START_MARKER)
                    if start_idx == -1:
                        break
                    end_idx = parse_buffer.find(OUTPUT_END_MARKER, start_idx)
                    if end_idx == -1:
                        break  # incomplete pair

                    json_bytes = parse_buffer[start_idx + len(OUTPUT_START_MARKER):end_idx].strip()
                    parse_buffer = parse_buffer[end_idx + len(OUTPUT_END_MARKER):]

                    try:
                        parsed_raw = json.loads(json_bytes)
                        parsed = ContainerOutput(
                            status=parsed_raw.get("status", "error"),
                            result=parsed_raw.get("result"),
                            new_session_id=parsed_raw.get("newSessionId"),
                            error=parsed_raw.get("error"),
                        )
                        if parsed.new_session_id:
                            new_session_id = parsed.new_session_id
                        had_streaming_output = True
                        reset_timeout()
                        # Chain output callbacks sequentially
                        prev_chain = output_chain
                        new_future: asyncio.Future = loop.create_future()

                        async def _call_output(p=parsed, pf=prev_chain, nf=new_future):
                            await pf
                            try:
                                coro = on_output(p)
                                if asyncio.iscoroutine(coro):
                                    await coro
                            finally:
                                nf.set_result(None)

                        asyncio.ensure_future(_call_output())
                        output_chain = new_future

                    except Exception as e:
                        logger.warning("Failed to parse streamed output chunk: %s", e)

    # Read stdout and stderr concurrently
    await asyncio.gather(read_stdout(), read_stderr())
    await proc.wait()

    if timeout_handle:
        timeout_handle.cancel()

    # Wait for output chain to settle
    await output_chain

    duration_ms = int((time.monotonic() - start_time) * 1000)
    stdout_bytes = b"".join(stdout_chunks)
    stderr_str = b"".join(stderr_chunks).decode(errors="replace")
    code = proc.returncode

    # Write container log
    ts = datetime.now(timezone.utc).isoformat().replace(":", "-").replace(".", "-")
    log_file = logs_dir / f"container-{ts}.log"
    is_verbose = os.environ.get("LOG_LEVEL") in ("debug", "trace")
    is_error = code != 0

    log_lines = [
        "=== Container Run Log ===",
        f"Timestamp: {datetime.now(timezone.utc).isoformat()}",
        f"Group: {group.name}",
        f"IsMain: {input_data.is_main}",
        f"Duration: {duration_ms}ms",
        f"Exit Code: {code}",
        f"Stdout Truncated: {stdout_truncated}",
        f"Stderr Truncated: {stderr_truncated}",
        "",
    ]
    if timed_out:
        log_lines.insert(1, "=== TIMEOUT ===")

    if is_verbose or is_error:
        log_lines += [
            "=== Container Args ===",
            " ".join(container_args),
            "",
            "=== Mounts ===",
            "\n".join(f"{m.host_path} -> {m.container_path}{' (ro)' if m.readonly else ''}" for m in mounts),
            "",
            f"=== Stderr{'(TRUNCATED)' if stderr_truncated else ''} ===",
            stderr_str,
            "",
            f"=== Stdout{'(TRUNCATED)' if stdout_truncated else ''} ===",
            stdout_bytes.decode(errors="replace"),
        ]

    try:
        log_file.write_text("\n".join(log_lines))
        logger.debug("Container log written: %s", log_file)
    except Exception:
        pass

    if timed_out:
        if had_streaming_output:
            logger.info(
                "Container timed out after output (idle cleanup): group=%s container=%s duration=%dms",
                group.name, container_name, duration_ms,
            )
            return ContainerOutput(
                status="success",
                result=None,
                new_session_id=new_session_id,
                execution_log=stdout_bytes.decode(errors="replace"),
            )

        error_msg = f"Container timed out after {config_timeout}ms"
        dump = write_error_dump(group, input_data, error_msg, stderr_str, duration_ms, None)
        logger.error(
            "Container timed out with no output: group=%s container=%s dump=%s",
            group.name, container_name, dump.name,
        )
        return ContainerOutput(
            status="error",
            result=None,
            error=error_msg,
            execution_log=stdout_bytes.decode(errors="replace"),
        )

    if code != 0:
        error_msg = f"Container exited with code {code}: {stderr_str[-200:]}"
        dump = write_error_dump(group, input_data, error_msg, stderr_str, duration_ms, code)

        # Check if shutting down (exit code 130 = SIGINT, 137 = SIGKILL, 143 = SIGTERM)
        # These are expected during graceful shutdown
        import omiga.state as state
        is_shutdown = state._shutdown_event is not None and state._shutdown_event.is_set()
        is_signal_exit = code in (130, 137, 143)

        if is_shutdown and is_signal_exit:
            logger.info(
                "Container stopped (shutdown): group=%s code=%d duration=%dms",
                group.name, code, duration_ms,
            )
        else:
            logger.error(
                "Container exited with error: group=%s code=%d duration=%dms dump=%s",
                group.name, code, duration_ms, dump.name,
            )

        return ContainerOutput(
            status="error",
            result=None,
            error=error_msg,
            execution_log=stdout_bytes.decode(errors="replace"),
        )

    if on_output:
        logger.info(
            "Container completed (streaming mode): group=%s duration=%dms new_session=%s",
            group.name, duration_ms, new_session_id,
        )
        return ContainerOutput(
            status="success",
            result=None,
            new_session_id=new_session_id,
            execution_log=stdout_bytes.decode(errors="replace"),
        )

    # Legacy mode: parse last sentinel pair from stdout
    stdout_str = stdout_bytes.decode(errors="replace")
    try:
        start_idx = stdout_str.find(OUTPUT_START_MARKER.decode())
        end_idx = stdout_str.find(OUTPUT_END_MARKER.decode())
        if start_idx != -1 and end_idx != -1 and end_idx > start_idx:
            json_str = stdout_str[start_idx + len(OUTPUT_START_MARKER):end_idx].strip()
        else:
            lines = stdout_str.strip().splitlines()
            json_str = lines[-1] if lines else "{}"
        raw = json.loads(json_str)

        # Parse tool_calls if available (for SOP generation)
        tool_calls = raw.get("tool_calls")

        result = ContainerOutput(
            status=raw.get("status", "error"),
            result=raw.get("result"),
            new_session_id=raw.get("newSessionId"),
            error=raw.get("error"),
            execution_log=stdout_str,  # 完整执行日志
            tool_calls=tool_calls,     # 工具调用记录
        )
        logger.info(
            "Container completed: group=%s duration=%dms status=%s",
            group.name, duration_ms, result.status,
        )
        return result
    except Exception as e:
        logger.error("Failed to parse container output: group=%s error=%s", group.name, e)
        return ContainerOutput(
            status="error",
            result=None,
            error=f"Failed to parse container output: {e}",
            execution_log=stdout_str,
        )


def write_tasks_snapshot(
    group_folder: str,
    is_main: bool,
    tasks: list[dict],
) -> None:
    group_ipc_dir = resolve_group_ipc_path(group_folder)
    group_ipc_dir.mkdir(parents=True, exist_ok=True)

    filtered = tasks if is_main else [t for t in tasks if t.get("groupFolder") == group_folder]

    tasks_file = group_ipc_dir / "current_tasks.json"
    tasks_file.write_text(json.dumps(filtered, indent=2))


def write_groups_snapshot(
    group_folder: str,
    is_main: bool,
    groups: list[AvailableGroup],
    registered_jids: set[str],
) -> None:
    group_ipc_dir = resolve_group_ipc_path(group_folder)
    group_ipc_dir.mkdir(parents=True, exist_ok=True)

    visible = (
        [
            {
                "jid": g.jid,
                "name": g.name,
                "lastActivity": g.last_activity,
                "isRegistered": g.is_registered,
            }
            for g in groups
        ]
        if is_main
        else []
    )

    groups_file = group_ipc_dir / "available_groups.json"
    groups_file.write_text(
        json.dumps(
            {"groups": visible, "lastSync": datetime.now(timezone.utc).isoformat()},
            indent=2,
        )
    )
