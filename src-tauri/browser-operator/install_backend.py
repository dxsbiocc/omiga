#!/usr/bin/env python3
"""Install Omiga's managed Browser Operator backend.

The Browser Operator facade is bundled with Omiga, but the browser-use Python
runtime and Playwright browser binaries are intentionally installed on demand to
avoid shipping hundreds of MB in the desktop app bundle.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import venv
from pathlib import Path
from typing import Any


def default_home() -> Path:
    configured = os.getenv("OMIGA_BROWSER_OPERATOR_HOME")
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".omiga" / "browser-operator"


def venv_python(venv_dir: Path) -> Path:
    if platform.system().lower().startswith("win"):
        return venv_dir / "Scripts" / "python.exe"
    return venv_dir / "bin" / "python"


def venv_exe(venv_dir: Path, name: str) -> Path:
    suffix = ".exe" if platform.system().lower().startswith("win") else ""
    subdir = "Scripts" if suffix else "bin"
    return venv_dir / subdir / f"{name}{suffix}"


def run_step(
    label: str,
    cmd: list[str],
    *,
    env: dict[str, str],
    dry_run: bool,
    strict: bool = True,
) -> dict[str, Any]:
    started = time.time()
    if dry_run:
        return {
            "label": label,
            "cmd": cmd,
            "dryRun": True,
            "returncode": 0,
            "durationSeconds": 0,
        }

    completed = subprocess.run(
        cmd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    item = {
        "label": label,
        "cmd": cmd,
        "returncode": completed.returncode,
        "durationSeconds": round(time.time() - started, 3),
        "stdoutTail": completed.stdout[-4000:],
        "stderrTail": completed.stderr[-4000:],
    }
    if strict and completed.returncode != 0:
        raise RuntimeError(json.dumps(item, ensure_ascii=False, indent=2))
    return item


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Install Omiga Browser Operator backend")
    parser.add_argument("--home", type=Path, default=None, help="Managed backend root directory")
    parser.add_argument("--venv", type=Path, default=None, help="Virtualenv path")
    parser.add_argument("--package", default="browser-use", help="Python package spec to install")
    parser.add_argument(
        "--skip-browser-install",
        action="store_true",
        help="Only install Python packages; do not download Playwright/Chromium browser binaries",
    )
    parser.add_argument(
        "--run-doctor",
        action="store_true",
        help="Run `browser-use doctor` after install. This may warn/fail when API keys are unset.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print a machine-readable JSON status object",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the steps without creating a venv or installing packages",
    )
    return parser


def main(argv: list[str]) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    home = (args.home or default_home()).expanduser()
    venv_dir = (args.venv or (home / ".venv")).expanduser()
    python = venv_python(venv_dir)
    browser_use = venv_exe(venv_dir, "browser-use")
    browsers_path = home / "ms-playwright"
    env = os.environ.copy()
    env["PLAYWRIGHT_BROWSERS_PATH"] = str(browsers_path)

    steps: list[dict[str, Any]] = []
    try:
        if not args.dry_run:
            home.mkdir(parents=True, exist_ok=True)
            if not python.exists():
                venv.EnvBuilder(with_pip=True, clear=False).create(venv_dir)

        steps.append(
            run_step(
                "upgrade-pip",
                [str(python), "-m", "pip", "install", "--upgrade", "pip"],
                env=env,
                dry_run=args.dry_run,
            )
        )
        steps.append(
            run_step(
                "install-browser-use",
                [str(python), "-m", "pip", "install", "--upgrade", args.package],
                env=env,
                dry_run=args.dry_run,
            )
        )
        steps.append(
            run_step(
                "import-browser-use",
                [str(python), "-c", "import browser_use; print(getattr(browser_use, '__version__', 'unknown'))"],
                env=env,
                dry_run=args.dry_run,
            )
        )
        if not args.skip_browser_install:
            steps.append(
                run_step(
                    "browser-use-install",
                    [str(browser_use), "install"],
                    env=env,
                    dry_run=args.dry_run,
                )
            )
        if args.run_doctor:
            steps.append(
                run_step(
                    "browser-use-doctor",
                    [str(browser_use), "doctor"],
                    env=env,
                    dry_run=args.dry_run,
                    strict=False,
                )
            )

        result = {
            "ok": True,
            "home": str(home),
            "venv": str(venv_dir),
            "python": str(python),
            "browserUse": str(browser_use),
            "playwrightBrowsersPath": str(browsers_path),
            "browserUseOnPath": shutil.which("browser-use"),
            "steps": steps,
        }
    except Exception as exc:
        result = {
            "ok": False,
            "home": str(home),
            "venv": str(venv_dir),
            "python": str(python),
            "browserUse": str(browser_use),
            "playwrightBrowsersPath": str(browsers_path),
            "error": str(exc),
            "steps": steps,
        }

    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        if result["ok"]:
            print("Omiga Browser Operator backend installed.")
            print(f"Python: {result['python']}")
            print(f"Playwright browsers: {result['playwrightBrowsersPath']}")
        else:
            print("Omiga Browser Operator backend install failed.", file=sys.stderr)
            print(result["error"], file=sys.stderr)
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
