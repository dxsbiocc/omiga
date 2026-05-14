#!/usr/bin/env python3
"""Aggregate Computer Use release-readiness checks.

Default checks are intentionally low risk: they compile/validate scripts, inspect
install status, run formatting/diff checks, and run mock MCP smoke tests. Checks
that may start the real macOS backend or perform broader project builds are
opt-in flags.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import plistlib
import shlex
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
SMOKE_SCRIPT = REPO_ROOT / "scripts" / "computer-use-smoke.py"
INSTALL_SCRIPT = REPO_ROOT / "scripts" / "install-computer-use-sidecar.sh"
SELF = Path(__file__).resolve()
RUST_SIDECAR = REPO_ROOT / "src-tauri" / "src" / "bin" / "computer-use-sidecar.rs"
PLUGIN_ROOT = REPO_ROOT / "src-tauri" / "bundled_plugins" / "plugins" / "computer-use"
PLUGIN_MANIFEST = PLUGIN_ROOT / ".omiga-plugin" / "plugin.json"
PLUGIN_MCP_CONFIG = PLUGIN_ROOT / ".mcp.json"
MAIN_WRAPPER = PLUGIN_ROOT / "bin" / "computer-use"
ARM_WRAPPER = PLUGIN_ROOT / "bin" / "darwin-arm64" / "computer-use"
X64_WRAPPER = PLUGIN_ROOT / "bin" / "darwin-x64" / "computer-use"
PYTHON_BACKEND = PLUGIN_ROOT / "bin" / "computer-use-macos.py"
TAURI_CONFIG = REPO_ROOT / "src-tauri" / "tauri.conf.json"
TAURI_ENTITLEMENTS = REPO_ROOT / "src-tauri" / "Entitlements.plist"
TAURI_INFO_PLIST = REPO_ROOT / "src-tauri" / "Info.plist"
PLUGIN_MARKETPLACE = REPO_ROOT / "src-tauri" / "bundled_plugins" / "marketplace.json"
CURATED_MARKETPLACE_ROOT = REPO_ROOT / "src-tauri" / "omiga-plugins"
DEFAULT_INSTALLED_SIDECAR = (
    PLUGIN_ROOT / "bin" / "computer-use-sidecar"
)
COMPUTER_USE_PATHS = [
    REPO_ROOT / ".gitignore",
    REPO_ROOT / "docs" / "COMPUTER_USE_EXTENSION.md",
    REPO_ROOT / "docs" / "COMPUTER_USE_EXTENSION_IMPLEMENTATION_PLAN.md",
    REPO_ROOT / "docs" / "COMPUTER_USE_QA_MATRIX.md",
    SELF,
    SMOKE_SCRIPT,
    INSTALL_SCRIPT,
    RUST_SIDECAR,
    PLUGIN_MANIFEST,
    PLUGIN_MCP_CONFIG,
    MAIN_WRAPPER,
    ARM_WRAPPER,
    X64_WRAPPER,
    PYTHON_BACKEND,
    TAURI_CONFIG,
    PLUGIN_MARKETPLACE,
    REPO_ROOT / "src-tauri" / "src" / "commands" / "computer_use.rs",
    REPO_ROOT / "src-tauri" / "src" / "commands" / "chat" / "tool_exec.rs",
    REPO_ROOT / "src-tauri" / "src" / "domain" / "computer_use" / "mod.rs",
    REPO_ROOT / "src-tauri" / "src" / "lib.rs",
    REPO_ROOT / "src" / "components" / "Settings" / "ComputerUseSettingsTab.tsx",
    REPO_ROOT / "src" / "components" / "Chat" / "ChatComposer.tsx",
]


@dataclass
class CheckResult:
    name: str
    ok: bool
    status: str
    durationSeconds: float
    command: list[str] | None = None
    exitCode: int | None = None
    stdout: str = ""
    stderr: str = ""
    note: str = ""


def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).astimezone().isoformat(timespec="seconds")


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def command_label(command: list[str]) -> str:
    return " ".join(shlex.quote(part) for part in command)


def tail(text: str, limit: int = 6000) -> str:
    if len(text) <= limit:
        return text
    return text[-limit:]


def run_command(
    name: str,
    command: list[str],
    *,
    env: dict[str, str] | None = None,
    timeout: int | None = None,
) -> CheckResult:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            command,
            cwd=REPO_ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except FileNotFoundError as error:
        return CheckResult(
            name=name,
            ok=False,
            status="failed",
            durationSeconds=round(time.monotonic() - started, 3),
            command=command,
            exitCode=None,
            stderr=str(error),
            note="command_not_found",
        )
    except subprocess.TimeoutExpired as error:
        return CheckResult(
            name=name,
            ok=False,
            status="failed",
            durationSeconds=round(time.monotonic() - started, 3),
            command=command,
            exitCode=None,
            stdout=tail(error.stdout or ""),
            stderr=tail(error.stderr or ""),
            note=f"timeout_after_{timeout}s",
        )

    ok = completed.returncode == 0
    return CheckResult(
        name=name,
        ok=ok,
        status="passed" if ok else "failed",
        durationSeconds=round(time.monotonic() - started, 3),
        command=command,
        exitCode=completed.returncode,
        stdout=tail(completed.stdout),
        stderr=tail(completed.stderr),
    )


def run_inline_check(name: str, check: Any) -> CheckResult:
    started = time.monotonic()
    try:
        note = check()
    except Exception as error:  # noqa: BLE001 - release report should capture all failures.
        return CheckResult(
            name=name,
            ok=False,
            status="failed",
            durationSeconds=round(time.monotonic() - started, 3),
            stderr=str(error),
        )
    return CheckResult(
        name=name,
        ok=True,
        status="passed",
        durationSeconds=round(time.monotonic() - started, 3),
        note=str(note or ""),
    )


def check_paths_exist() -> str:
    required = [SMOKE_SCRIPT, INSTALL_SCRIPT, RUST_SIDECAR]
    missing = [display_path(path) for path in required if not path.exists()]
    if missing:
        raise RuntimeError("missing required files: " + ", ".join(missing))
    return "required Computer Use scripts and Rust sidecar source are present"


def check_text_hygiene() -> str:
    problems: list[str] = []
    scanned = 0
    for path in COMPUTER_USE_PATHS:
        if not path.is_file():
            continue
        scanned += 1
        data = path.read_bytes()
        if data and not data.endswith(b"\n"):
            problems.append(f"{display_path(path)}: missing final newline")
        for line_no, raw_line in enumerate(data.splitlines(), start=1):
            if raw_line.rstrip(b" \t") != raw_line:
                problems.append(f"{display_path(path)}:{line_no}: trailing whitespace")
    if problems:
        raise RuntimeError("\n".join(problems[:50]))
    return f"scanned {scanned} Computer Use files for trailing whitespace/final newline"


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise RuntimeError(f"missing JSON file: {display_path(path)}") from error
    except json.JSONDecodeError as error:
        raise RuntimeError(f"invalid JSON in {display_path(path)}: {error}") from error
    if not isinstance(value, dict):
        raise RuntimeError(f"expected JSON object in {display_path(path)}")
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def check_shell_syntax(path: Path) -> None:
    completed = subprocess.run(
        ["sh", "-n", display_path(path)],
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"shell syntax check failed for {display_path(path)}: "
            f"{completed.stderr.strip() or completed.stdout.strip()}"
        )


def generated_packaging_artifacts() -> list[str]:
    artifacts: list[Path] = []
    if PLUGIN_ROOT.exists():
        artifacts.extend(path for path in PLUGIN_ROOT.rglob("__pycache__") if path.exists())
        artifacts.extend(path for path in PLUGIN_ROOT.rglob("*.pyc") if path.exists())
    return [display_path(path) for path in sorted(artifacts)]


def check_generated_packaging_artifacts_absent() -> str:
    artifacts = generated_packaging_artifacts()
    if artifacts:
        raise RuntimeError(
            "generated plugin packaging artifacts found; remove them before final packaging: "
            + ", ".join(artifacts)
        )
    return "no generated plugin packaging artifacts found"


def check_plugin_packaging_config() -> str:
    tauri_config = read_json(TAURI_CONFIG)
    resources = (
        tauri_config.get("bundle", {}).get("resources", [])
        if isinstance(tauri_config.get("bundle"), dict)
        else []
    )
    resource_targets = set()
    if isinstance(resources, list):
        resource_targets.update(str(resource).rstrip("/") for resource in resources)
    elif isinstance(resources, dict):
        resource_targets.update(str(target).rstrip("/") for target in resources.values())
    require(
        "bundled_plugins" in resource_targets,
        "tauri bundle.resources must include bundled_plugins",
    )
    require(
        any(
            target == "omiga-plugins" or target.startswith("omiga-plugins/")
            for target in resource_targets
        ),
        "tauri bundle.resources must include omiga-plugins marketplace resources",
    )
    require(
        (CURATED_MARKETPLACE_ROOT / "marketplace.json").is_file(),
        "repo-local omiga-plugins marketplace must be present for packaging",
    )

    marketplace = read_json(PLUGIN_MARKETPLACE)
    plugins = marketplace.get("plugins")
    require(isinstance(plugins, list), "marketplace plugins must be an array")
    computer_entry = next(
        (entry for entry in plugins if isinstance(entry, dict) and entry.get("name") == "computer-use"),
        None,
    )
    require(computer_entry is not None, "computer-use marketplace entry missing")
    source = computer_entry.get("source") if isinstance(computer_entry, dict) else None
    require(isinstance(source, dict), "computer-use marketplace source must be an object")
    require(source.get("source") == "local", "computer-use marketplace source must be local")
    require(
        source.get("path") == "./plugins/computer-use",
        "computer-use marketplace path must be ./plugins/computer-use",
    )

    manifest = read_json(PLUGIN_MANIFEST)
    require(manifest.get("name") == "computer-use", "plugin manifest name must be computer-use")
    require(manifest.get("mcpServers") == "./.mcp.json", "plugin manifest mcpServers must be ./.mcp.json")
    interface = manifest.get("interface")
    require(isinstance(interface, dict), "plugin manifest interface must be present")
    require(interface.get("displayName") == "Computer Use", "plugin displayName must be Computer Use")

    mcp_config = read_json(PLUGIN_MCP_CONFIG)
    servers = mcp_config.get("mcpServers")
    require(isinstance(servers, dict), "plugin .mcp.json must contain mcpServers object")
    computer_server = servers.get("computer") if isinstance(servers, dict) else None
    require(isinstance(computer_server, dict), "plugin .mcp.json must contain computer server")
    require(
        computer_server.get("command") == "./bin/computer-use",
        "computer MCP command must be ./bin/computer-use",
    )
    require(
        computer_server.get("args", []) == [],
        "computer MCP args must be empty unless release checks are updated",
    )

    executable_files = [MAIN_WRAPPER, ARM_WRAPPER, X64_WRAPPER, PYTHON_BACKEND]
    for path in executable_files:
        require(path.is_file(), f"required plugin executable missing: {display_path(path)}")
        require(os.access(path, os.X_OK), f"plugin executable bit missing: {display_path(path)}")
    for path in [MAIN_WRAPPER, ARM_WRAPPER, X64_WRAPPER]:
        check_shell_syntax(path)

    return json.dumps(
        {
            "tauriResources": resources,
            "marketplacePath": source.get("path") if isinstance(source, dict) else None,
            "mcpCommand": computer_server.get("command") if isinstance(computer_server, dict) else None,
            "executables": [display_path(path) for path in executable_files],
            "generatedPackagingArtifacts": generated_packaging_artifacts(),
        },
        ensure_ascii=False,
        sort_keys=True,
    )


def existing_path_args(paths: list[Path]) -> list[str]:
    return [display_path(path) for path in paths if path.exists()]


def resolve_input_path(raw: str | None, default: Path) -> Path:
    if not raw:
        return default
    return Path(raw).expanduser().resolve()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_info(path: Path) -> dict[str, Any]:
    stat = path.stat()
    return {
        "path": str(path),
        "sizeBytes": stat.st_size,
        "executable": os.access(path, os.X_OK),
        "sha256": sha256_file(path),
    }


def manifest_artifact_paths(args: argparse.Namespace) -> list[Path]:
    paths = [
        TAURI_CONFIG,
        PLUGIN_MARKETPLACE,
        PLUGIN_MANIFEST,
        PLUGIN_MCP_CONFIG,
        MAIN_WRAPPER,
        ARM_WRAPPER,
        X64_WRAPPER,
        PYTHON_BACKEND,
    ]
    installed = resolve_input_path(args.installed_sidecar, DEFAULT_INSTALLED_SIDECAR)
    if installed.is_file():
        paths.append(installed)
    return paths


def manifest_entry(path: Path) -> dict[str, Any]:
    stat = path.stat()
    return {
        "path": display_path(path),
        "sizeBytes": stat.st_size,
        "executable": os.access(path, os.X_OK),
        "sha256": sha256_file(path),
    }


def artifact_manifest(args: argparse.Namespace) -> dict[str, Any]:
    entries = [manifest_entry(path) for path in manifest_artifact_paths(args)]
    return {
        "schemaVersion": 1,
        "kind": "omiga.computer-use.release-artifacts",
        "generatedAt": now_iso(),
        "repoRoot": str(REPO_ROOT),
        "artifacts": sorted(entries, key=lambda entry: entry["path"]),
    }


def write_artifact_manifest(args: argparse.Namespace) -> str:
    output = resolve_input_path(args.write_artifact_manifest, Path(args.write_artifact_manifest))
    output.parent.mkdir(parents=True, exist_ok=True)
    manifest = artifact_manifest(args)
    output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return json.dumps(
        {
            "path": str(output),
            "artifactCount": len(manifest["artifacts"]),
            "artifacts": [entry["path"] for entry in manifest["artifacts"]],
        },
        ensure_ascii=False,
        sort_keys=True,
    )


def verify_artifact_manifest(args: argparse.Namespace) -> str:
    manifest_path = resolve_input_path(args.verify_artifact_manifest, Path(args.verify_artifact_manifest))
    expected = read_json(manifest_path)
    expected_entries = expected.get("artifacts")
    if not isinstance(expected_entries, list):
        raise RuntimeError(f"artifact manifest missing artifacts array: {manifest_path}")
    current_entries = artifact_manifest(args)["artifacts"]
    expected_by_path = {
        entry.get("path"): entry for entry in expected_entries if isinstance(entry, dict)
    }
    current_by_path = {entry["path"]: entry for entry in current_entries}
    missing = sorted(set(expected_by_path) - set(current_by_path))
    extra = sorted(set(current_by_path) - set(expected_by_path))
    mismatched: list[dict[str, Any]] = []
    for path in sorted(set(expected_by_path) & set(current_by_path)):
        expected_entry = expected_by_path[path]
        current_entry = current_by_path[path]
        for key in ("sizeBytes", "executable", "sha256"):
            if expected_entry.get(key) != current_entry.get(key):
                mismatched.append(
                    {
                        "path": path,
                        "field": key,
                        "expected": expected_entry.get(key),
                        "current": current_entry.get(key),
                    }
                )
    if missing or extra or mismatched:
        raise RuntimeError(
            json.dumps(
                {
                    "missing": missing,
                    "extra": extra,
                    "mismatched": mismatched,
                },
                ensure_ascii=False,
                sort_keys=True,
            )
        )
    return json.dumps(
        {
            "path": str(manifest_path),
            "artifactCount": len(current_entries),
            "verified": True,
        },
        ensure_ascii=False,
        sort_keys=True,
    )


def plist_file(path: Path) -> dict[str, Any]:
    if not path.is_file():
        raise RuntimeError(f"missing plist: {display_path(path)}")
    try:
        with path.open("rb") as handle:
            value = plistlib.load(handle)
    except Exception as error:  # noqa: BLE001 - release report should capture malformed plists.
        raise RuntimeError(f"invalid plist {display_path(path)}: {error}") from error
    if not isinstance(value, dict):
        raise RuntimeError(f"expected plist dict: {display_path(path)}")
    return value


def run_small_command(command: list[str], timeout: int = 30) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )


def check_signing_preflight(args: argparse.Namespace) -> str:
    if sys.platform != "darwin":
        raise RuntimeError("macOS signing/notarization preflight requires Darwin")

    codesign = shutil.which("codesign")
    xcrun = shutil.which("xcrun")
    security = shutil.which("security")
    require(bool(codesign), "codesign tool not found on PATH")
    require(bool(xcrun), "xcrun tool not found on PATH")

    codesign_help = run_small_command([codesign or "codesign", "-h"], timeout=15)
    require(
        "Usage: codesign" in (codesign_help.stderr + codesign_help.stdout),
        f"codesign help did not return expected usage text: {codesign_help.stderr}",
    )

    notarytool_help = run_small_command([xcrun or "xcrun", "notarytool", "--help"], timeout=30)
    require(notarytool_help.returncode == 0, f"xcrun notarytool --help failed: {notarytool_help.stderr}")

    stapler_help = run_small_command([xcrun or "xcrun", "stapler", "help"], timeout=30)
    require(
        "Usage: stapler" in (stapler_help.stderr + stapler_help.stdout),
        f"xcrun stapler help failed: {stapler_help.stderr}",
    )

    tauri_config = read_json(TAURI_CONFIG)
    macos_config = (
        tauri_config.get("bundle", {}).get("macOS", {})
        if isinstance(tauri_config.get("bundle"), dict)
        else {}
    )
    require(isinstance(macos_config, dict), "tauri bundle.macOS config must be an object")
    entitlements_ref = macos_config.get("entitlements")
    info_plist_ref = macos_config.get("infoPlist")
    require(entitlements_ref == "./Entitlements.plist", "tauri macOS entitlements must be ./Entitlements.plist")
    require(info_plist_ref == "./Info.plist", "tauri macOS infoPlist must be ./Info.plist")

    entitlements = plist_file(TAURI_ENTITLEMENTS)
    info_plist = plist_file(TAURI_INFO_PLIST)

    identity_verified = None
    if args.codesign_identity:
        require(bool(security), "security tool not found on PATH for codesign identity lookup")
        identities = run_small_command([security or "security", "find-identity", "-v", "-p", "codesigning"], timeout=30)
        require(identities.returncode == 0, f"security find-identity failed: {identities.stderr}")
        require(
            args.codesign_identity in identities.stdout,
            f"codesign identity not found in keychain: {args.codesign_identity}",
        )
        identity_verified = True

    notary_profile_verified = None
    if args.verify_notary_profile:
        require(args.notarytool_profile, "--verify-notary-profile requires --notarytool-profile")
        history = run_small_command(
            [xcrun or "xcrun", "notarytool", "history", "--keychain-profile", args.notarytool_profile],
            timeout=60,
        )
        require(history.returncode == 0, f"notarytool profile verification failed: {history.stderr}")
        notary_profile_verified = True

    return json.dumps(
        {
            "codesign": codesign,
            "codesignHelp": True,
            "xcrun": xcrun,
            "notarytoolHelp": True,
            "staplerHelp": True,
            "entitlements": display_path(TAURI_ENTITLEMENTS),
            "entitlementKeys": sorted(entitlements.keys()),
            "infoPlist": display_path(TAURI_INFO_PLIST),
            "infoPlistKeys": sorted(info_plist.keys()),
            "codesignIdentity": args.codesign_identity,
            "codesignIdentityVerified": identity_verified,
            "notarytoolProfile": args.notarytool_profile,
            "notarytoolProfileVerified": notary_profile_verified,
        },
        ensure_ascii=False,
        sort_keys=True,
    )


def check_installed_sidecar(args: argparse.Namespace) -> str:
    installed = resolve_input_path(args.installed_sidecar, DEFAULT_INSTALLED_SIDECAR)
    if not installed.is_file():
        raise RuntimeError(f"installed Rust sidecar not found: {installed}")
    if not os.access(installed, os.X_OK):
        raise RuntimeError(f"installed Rust sidecar is not executable: {installed}")

    installed_info = artifact_info(installed)
    source_info = None
    if args.rust_bin:
        source = resolve_input_path(args.rust_bin, Path(args.rust_bin))
        if not source.is_file():
            raise RuntimeError(f"source Rust sidecar not found: {source}")
        source_info = artifact_info(source)
        if installed_info["sha256"] != source_info["sha256"]:
            raise RuntimeError(
                "installed Rust sidecar hash does not match --rust-bin: "
                f"installed={installed_info['sha256']} source={source_info['sha256']}"
            )

    if args.expected_sidecar_sha256:
        expected = args.expected_sidecar_sha256.lower()
        if installed_info["sha256"].lower() != expected:
            raise RuntimeError(
                "installed Rust sidecar hash does not match --expected-sidecar-sha256: "
                f"installed={installed_info['sha256']} expected={expected}"
            )

    return json.dumps(
        {
            "installed": installed_info,
            "source": source_info,
            "expectedSha256": args.expected_sidecar_sha256,
        },
        ensure_ascii=False,
        sort_keys=True,
    )


def build_checks(args: argparse.Namespace) -> list[tuple[str, list[str], dict[str, str] | None, int | None]]:
    checks: list[tuple[str, list[str], dict[str, str] | None, int | None]] = []

    checks.append(
        (
            "python-syntax-check",
            [
                sys.executable,
                "-c",
                (
                    "import pathlib, sys\n"
                    "for path in sys.argv[1:]:\n"
                    "    compile(pathlib.Path(path).read_text(), path, 'exec')\n"
                ),
                display_path(SMOKE_SCRIPT),
                display_path(SELF),
                display_path(PYTHON_BACKEND),
            ],
            None,
            60,
        )
    )
    checks.append(("install-script-syntax", ["sh", "-n", display_path(INSTALL_SCRIPT)], None, 30))
    checks.append(("install-script-status", [display_path(INSTALL_SCRIPT), "--status"], None, 30))
    checks.append(("rustfmt-sidecar", ["rustfmt", "--edition", "2021", "--check", display_path(RUST_SIDECAR)], None, 60))

    diff_paths = existing_path_args(COMPUTER_USE_PATHS)
    if diff_paths:
        checks.append(("git-diff-check", ["git", "diff", "--check", "--", *diff_paths], None, 60))

    if not args.skip_smoke:
        checks.append(
            (
                "mcp-python-mock-smoke",
                [display_path(SMOKE_SCRIPT), "--suite", "python-mock"],
                None,
                120,
            )
        )
        if args.include_rust_sidecar or args.rust_bin:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", "rust-mock"]
            if args.rust_bin:
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append(("mcp-rust-mock-smoke", smoke, None, 120))

    if args.verify_installed_sidecar and not args.skip_smoke:
        installed = resolve_input_path(args.installed_sidecar, DEFAULT_INSTALLED_SIDECAR)
        checks.append(
            (
                "mcp-installed-rust-mock-smoke",
                [display_path(SMOKE_SCRIPT), "--suite", "rust-mock", "--rust-bin", str(installed)],
                None,
                120,
            )
        )

    if args.include_build:
        checks.append(("frontend-build", ["bun", "run", "build"], None, 300))

    if args.include_cargo_test:
        env = os.environ.copy()
        env.setdefault("CARGO_TARGET_DIR", "/private/tmp/omiga-cu-target")
        checks.append(
            (
                "cargo-computer-use-tests",
                [
                    "cargo",
                    "test",
                    "--manifest-path",
                    "src-tauri/Cargo.toml",
                    "computer_use",
                    "--lib",
                ],
                env,
                300,
            )
        )

    if args.include_real_safe:
        for suite in ("rust-real-safe", "python-real-safe"):
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            if args.require_real_observe:
                smoke.append("--require-real-observe")
            checks.append((f"mcp-{suite}", smoke, None, 120))

    if args.include_real_dialog_e2e:
        suites = ["python-real-dialog-e2e"]
        if args.include_rust_sidecar or args.rust_bin:
            suites.insert(0, "rust-real-dialog-e2e")
        for suite in suites:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append((f"mcp-{suite}", smoke, None, 120))

    if args.include_real_key_e2e:
        suites = ["python-real-key-e2e"]
        if args.include_rust_sidecar or args.rust_bin:
            suites.insert(0, "rust-real-key-e2e")
        for suite in suites:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append((f"mcp-{suite}", smoke, None, 120))

    if args.include_real_drag_e2e:
        suites = ["python-real-drag-e2e"]
        if args.include_rust_sidecar or args.rust_bin:
            suites.insert(0, "rust-real-drag-e2e")
        for suite in suites:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append((f"mcp-{suite}", smoke, None, 120))

    if args.include_real_scroll_e2e:
        suites = ["python-real-scroll-e2e"]
        if args.include_rust_sidecar or args.rust_bin:
            suites.insert(0, "rust-real-scroll-e2e")
        for suite in suites:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append((f"mcp-{suite}", smoke, None, 120))

    if args.include_real_shortcut_e2e:
        suites = ["python-real-shortcut-e2e"]
        if args.include_rust_sidecar or args.rust_bin:
            suites.insert(0, "rust-real-shortcut-e2e")
        for suite in suites:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append((f"mcp-{suite}", smoke, None, 120))

    if args.include_real_visual_text:
        suites = ["python-real-visual-text"]
        if args.include_rust_sidecar or args.rust_bin:
            suites.insert(0, "rust-real-visual-text")
        for suite in suites:
            smoke = [display_path(SMOKE_SCRIPT), "--suite", suite]
            if args.rust_bin and suite.startswith("rust"):
                smoke.extend(["--rust-bin", args.rust_bin])
            checks.append((f"mcp-{suite}", smoke, None, 180))

    return checks


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-bin", help="Path to a built computer-use-sidecar binary for Rust smoke suites.")
    parser.add_argument(
        "--include-rust-sidecar",
        action="store_true",
        help="Also run internal Rust sidecar smoke checks. Default release gate uses Python only.",
    )
    parser.add_argument(
        "--verify-installed-sidecar",
        action="store_true",
        help=(
            "Verify the installed bundled Rust sidecar artifact. If --rust-bin is "
            "also supplied, hashes must match."
        ),
    )
    parser.add_argument(
        "--installed-sidecar",
        help=(
            "Path to installed bundled computer-use-sidecar. Defaults to "
            "src-tauri/bundled_plugins/plugins/computer-use/bin/computer-use-sidecar."
        ),
    )
    parser.add_argument(
        "--expected-sidecar-sha256",
        help="Expected sha256 for the installed Rust sidecar when --verify-installed-sidecar is used.",
    )
    parser.add_argument(
        "--include-build",
        action="store_true",
        help="Also run `bun run build` (not part of the default low-risk script/docs gate).",
    )
    parser.add_argument(
        "--include-cargo-test",
        action="store_true",
        help="Also run cargo computer_use tests; useful before release but may expose unrelated lib breakage.",
    )
    parser.add_argument(
        "--include-real-safe",
        action="store_true",
        help="Also run real macOS safe probes; requires local Accessibility/Screen Recording readiness.",
    )
    parser.add_argument(
        "--require-real-observe",
        action="store_true",
        help=(
            "With --include-real-safe, fail when real observe cannot read a macOS target. "
            "Use on permission-ready QA/packaging machines; omit for fail-closed probes."
        ),
    )
    parser.add_argument(
        "--include-real-dialog-e2e",
        action="store_true",
        help=(
            "Run a side-effectful but controlled macOS dialog E2E: open a temporary "
            "dialog, type text into it, click OK by observed element id, and verify "
            "the returned text. Requires Accessibility permission."
        ),
    )
    parser.add_argument(
        "--include-real-key-e2e",
        action="store_true",
        help=(
            "Run a controlled macOS dialog E2E for key_press: open a temporary "
            "dialog, press Enter through the backend, and verify the dialog is "
            "submitted. Requires Accessibility permission."
        ),
    )
    parser.add_argument(
        "--include-real-drag-e2e",
        action="store_true",
        help=(
            "Run a controlled macOS TextEdit E2E for drag: open a temporary "
            "document, drag the window title area, and verify the window bounds "
            "changed. Requires Accessibility permission."
        ),
    )
    parser.add_argument(
        "--include-real-scroll-e2e",
        action="store_true",
        help=(
            "Run a controlled macOS TextEdit E2E for scroll: open a temporary "
            "long document, scroll down through the backend, and verify the "
            "vertical scroll indicator moves. Requires Accessibility permission."
        ),
    )
    parser.add_argument(
        "--include-real-shortcut-e2e",
        action="store_true",
        help=(
            "Run a controlled macOS TextEdit E2E for shortcut: open a temporary "
            "document, run select_all through the fixed shortcut allowlist, type "
            "replacement text, and verify the document changed. Requires "
            "Accessibility permission."
        ),
    )
    parser.add_argument(
        "--include-real-visual-text",
        action="store_true",
        help=(
            "Run controlled macOS TextEdit OCR E2E: extractVisualText captures a "
            "screenshot, runs native Vision OCR, and verifies visible target text. "
            "Requires Screen Recording and Accessibility permission."
        ),
    )
    parser.add_argument(
        "--skip-smoke",
        action="store_true",
        help="Skip MCP mock smoke when only static/script hygiene is needed.",
    )
    parser.add_argument(
        "--fail-on-generated-artifacts",
        action="store_true",
        help=(
            "Fail if bundled computer-use plugin resources contain generated "
            "__pycache__ or .pyc artifacts. Use for final packaging gates."
        ),
    )
    parser.add_argument(
        "--write-artifact-manifest",
        help=(
            "Write a deterministic checksum manifest for Computer Use release "
            "packaging artifacts to this path."
        ),
    )
    parser.add_argument(
        "--verify-artifact-manifest",
        help=(
            "Verify current Computer Use release packaging artifacts against a "
            "previously written checksum manifest."
        ),
    )
    parser.add_argument(
        "--include-signing-preflight",
        action="store_true",
        help=(
            "Check macOS signing/notarization prerequisites: codesign, xcrun "
            "notarytool/stapler, Tauri macOS plist config, and optional identity/profile."
        ),
    )
    parser.add_argument(
        "--codesign-identity",
        help="Optional codesign identity string to verify with `security find-identity`.",
    )
    parser.add_argument(
        "--notarytool-profile",
        help="Optional notarytool keychain profile name to report or verify.",
    )
    parser.add_argument(
        "--verify-notary-profile",
        action="store_true",
        help=(
            "With --notarytool-profile, run `xcrun notarytool history` to verify "
            "the stored credentials. This may require network/keychain access."
        ),
    )
    parser.add_argument("--format", choices=["json", "markdown"], default="json")
    parser.add_argument("--output", help="Write the report to this path instead of stdout.")
    args = parser.parse_args()
    if args.require_real_observe and not args.include_real_safe:
        parser.error("--require-real-observe requires --include-real-safe")
    if args.verify_notary_profile and not args.notarytool_profile:
        parser.error("--verify-notary-profile requires --notarytool-profile")
    return args


def result_payload(args: argparse.Namespace, checks: list[CheckResult]) -> dict[str, Any]:
    return {
        "ok": all(check.ok for check in checks),
        "generatedAt": now_iso(),
        "repoRoot": str(REPO_ROOT),
        "defaultRiskProfile": "low-risk unless opt-in flags are present",
        "options": {
            "rustBin": args.rust_bin,
            "includeRustSidecar": args.include_rust_sidecar,
            "verifyInstalledSidecar": args.verify_installed_sidecar,
            "installedSidecar": args.installed_sidecar,
            "expectedSidecarSha256": args.expected_sidecar_sha256,
            "includeBuild": args.include_build,
            "includeCargoTest": args.include_cargo_test,
            "includeRealSafe": args.include_real_safe,
            "requireRealObserve": args.require_real_observe,
            "includeRealDialogE2e": args.include_real_dialog_e2e,
            "includeRealKeyE2e": args.include_real_key_e2e,
            "includeRealDragE2e": args.include_real_drag_e2e,
            "includeRealScrollE2e": args.include_real_scroll_e2e,
            "includeRealShortcutE2e": args.include_real_shortcut_e2e,
            "includeRealVisualText": args.include_real_visual_text,
            "skipSmoke": args.skip_smoke,
            "failOnGeneratedArtifacts": args.fail_on_generated_artifacts,
            "writeArtifactManifest": args.write_artifact_manifest,
            "verifyArtifactManifest": args.verify_artifact_manifest,
            "includeSigningPreflight": args.include_signing_preflight,
            "codesignIdentity": args.codesign_identity,
            "notarytoolProfile": args.notarytool_profile,
            "verifyNotaryProfile": args.verify_notary_profile,
        },
        "checks": [asdict(check) for check in checks],
    }


def render_json(payload: dict[str, Any]) -> str:
    return json.dumps(payload, ensure_ascii=False, indent=2) + "\n"


def render_markdown(payload: dict[str, Any]) -> str:
    icon = "✅" if payload["ok"] else "❌"
    lines = [
        "# Computer Use Release Check",
        "",
        f"Overall: {icon} {'PASS' if payload['ok'] else 'FAIL'}",
        f"Generated: `{payload['generatedAt']}`",
        f"Repo: `{payload['repoRoot']}`",
        "",
        "## Options",
        "",
    ]
    for key, value in payload["options"].items():
        lines.append(f"- `{key}`: `{value}`")
    lines.extend(
        [
            "",
            "## Checks",
            "",
            "| Check | Status | Duration | Exit |",
            "| --- | --- | ---: | ---: |",
        ]
    )
    for check in payload["checks"]:
        status_icon = "✅" if check["ok"] else "❌"
        exit_code = "" if check["exitCode"] is None else str(check["exitCode"])
        lines.append(
            f"| `{check['name']}` | {status_icon} {check['status']} | "
            f"{check['durationSeconds']:.3f}s | {exit_code} |"
        )

    for check in payload["checks"]:
        if check["ok"] and not check.get("note"):
            continue
        lines.extend(["", f"### {check['name']}", ""])
        if check.get("command"):
            lines.extend(["Command:", "```sh", command_label(check["command"]), "```"])
        if check.get("note"):
            lines.append(f"Note: {check['note']}")
        if check.get("stdout"):
            lines.extend(["Stdout:", "```", check["stdout"].rstrip(), "```"])
        if check.get("stderr"):
            lines.extend(["Stderr:", "```", check["stderr"].rstrip(), "```"])
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    args = parse_args()
    checks: list[CheckResult] = [
        run_inline_check("required-files", check_paths_exist),
        run_inline_check("computer-use-text-hygiene", check_text_hygiene),
        run_inline_check("computer-use-packaging-config", check_plugin_packaging_config),
    ]
    if args.verify_installed_sidecar:
        checks.append(run_inline_check("installed-sidecar-artifact", lambda: check_installed_sidecar(args)))
    if args.write_artifact_manifest:
        checks.append(run_inline_check("artifact-manifest-write", lambda: write_artifact_manifest(args)))
    if args.verify_artifact_manifest:
        checks.append(run_inline_check("artifact-manifest-verify", lambda: verify_artifact_manifest(args)))
    if args.include_signing_preflight:
        checks.append(run_inline_check("macos-signing-preflight", lambda: check_signing_preflight(args)))
    if args.fail_on_generated_artifacts:
        checks.append(
            run_inline_check(
                "generated-packaging-artifacts-absent",
                check_generated_packaging_artifacts_absent,
            )
        )
    for name, command, env, timeout in build_checks(args):
        checks.append(run_command(name, command, env=env, timeout=timeout))

    payload = result_payload(args, checks)
    rendered = render_markdown(payload) if args.format == "markdown" else render_json(payload)

    if args.output:
        output = Path(args.output).expanduser()
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8")
        print(f"wrote {output} ok={payload['ok']}")
    else:
        print(rendered, end="")

    return 0 if payload["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
