#!/usr/bin/env python3
"""MCP sidecar for Omiga Computer Use on macOS.

The Omiga core owns the model-facing `computer_*` facade, permission prompts,
audit logging, stop handling, and target-window revalidation. This sidecar is a
small backend adapter that exposes the internal `computer` MCP server.

Safety notes:
- The sidecar never executes model-provided scripts.
- macOS automation is limited to fixed, internal `osascript` snippets and
  system tools (`screencapture`, `pbcopy`, `pbpaste`).
- `OMIGA_COMPUTER_USE_BACKEND=mock` keeps deterministic CI/smoke tests.
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = "2024-11-05"
BACKEND_MODE = os.environ.get("OMIGA_COMPUTER_USE_BACKEND", "auto").strip().lower()
IS_DARWIN = platform.system() == "Darwin"
OBSERVATIONS: dict[str, dict[str, Any]] = {}
STOPPED_RUNS: set[str] = set()


def read_message() -> dict[str, Any] | None:
    headers: dict[str, str] = {}
    while True:
        line = sys.stdin.buffer.readline()
        if line == b"":
            return None
        text = line.decode("utf-8", "replace").strip()
        if text == "":
            break
        if ":" in text:
            key, value = text.split(":", 1)
            headers[key.lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    if length <= 0:
        return None
    body = sys.stdin.buffer.read(length)
    if not body:
        return None
    return json.loads(body.decode("utf-8"))


def write_message(payload: dict[str, Any]) -> None:
    body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def tool_schema(
    name: str,
    description: str,
    properties: dict[str, Any] | None = None,
    required: list[str] | None = None,
) -> dict[str, Any]:
    return {
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties or {},
            "required": required or [],
            "additionalProperties": True,
        },
    }


def tools_list() -> list[dict[str, Any]]:
    return [
        tool_schema(
            "observe",
            "Observe the current local macOS UI target and capture metadata.",
        ),
        tool_schema("set_target", "Activate a target app/window by app name or bundle id."),
        tool_schema(
            "validate_target",
            "Validate that the frontmost app/window still matches a prior observation.",
        ),
        tool_schema(
            "click",
            "Click a coordinate after target validation.",
            {"x": {"type": "number"}, "y": {"type": "number"}},
            ["x", "y"],
        ),
        tool_schema(
            "click_element",
            "Click an observed element by id after target validation.",
            {"elementId": {"type": "string"}},
            ["elementId"],
        ),
        tool_schema(
            "type_text",
            "Type text into the validated target via controlled clipboard paste.",
            {"text": {"type": "string"}},
            ["text"],
        ),
        tool_schema("stop", "Stop Computer Use actions for a run."),
    ]


def should_use_mock() -> bool:
    if BACKEND_MODE in {"mock", "test"}:
        return True
    if BACKEND_MODE == "real":
        return False
    return not IS_DARWIN


def now_ms() -> int:
    return int(time.time() * 1000)


def string_arg(args: dict[str, Any], key: str, default: str = "") -> str:
    value = args.get(key, default)
    if value is None:
        return default
    return str(value)


def run_id_arg(args: dict[str, Any]) -> str:
    return string_arg(args, "runId", "default")


def applescript_quote(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def run_command(
    command: list[str],
    *,
    input_text: str | None = None,
    timeout: float = 5.0,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        input=input_text,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )


def run_osascript(script: str, timeout: float = 5.0) -> subprocess.CompletedProcess[str]:
    return run_command(["osascript", "-e", script], timeout=timeout)


def permission_error(stderr: str) -> str | None:
    lowered = stderr.lower()
    if any(
        needle in lowered
        for needle in [
            "assistive access",
            "not allowed",
            "not authorized",
            "not permitted",
            "accessibility",
            "privacy",
        ]
    ):
        return (
            "macOS blocked UI automation. Grant Accessibility and Screen Recording "
            "permissions to Omiga/Terminal, then retry Computer Use."
        )
    return None


def active_window_id(snapshot: dict[str, Any]) -> str | None:
    target = snapshot.get("target") if isinstance(snapshot, dict) else None
    if not isinstance(target, dict):
        return None
    value = target.get("windowId")
    return str(value) if value is not None else None


def stable_window_id(
    app_name: str,
    bundle_id: str,
    pid: int,
    title: str,
    bounds: list[float],
) -> str:
    digest = hashlib.sha256(
        json.dumps(
            {
                "app": app_name,
                "bundle": bundle_id,
                "pid": pid,
                "title": title,
                "bounds": bounds,
            },
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()
    return f"mac_{digest[:16]}"


def parse_float(value: str, default: float = 0.0) -> float:
    try:
        return float(value)
    except Exception:
        return default


def parse_int(value: str, default: int = 0) -> int:
    try:
        return int(float(value))
    except Exception:
        return default


def query_frontmost_window() -> tuple[dict[str, Any] | None, str | None]:
    delimiter = "\x1f"
    script = r'''
tell application "System Events"
  set frontProc to first application process whose frontmost is true
  set appName to name of frontProc as text
  set bundleId to ""
  try
    set bundleId to bundle identifier of frontProc as text
  end try
  set pidValue to unix id of frontProc as integer
  set winTitle to ""
  set x to 0
  set y to 0
  set w to 0
  set h to 0
  if (count of windows of frontProc) > 0 then
    set frontWindow to window 1 of frontProc
    try
      set winTitle to name of frontWindow as text
    end try
    try
      set posValue to position of frontWindow
      set x to item 1 of posValue
      set y to item 2 of posValue
    end try
    try
      set sizeValue to size of frontWindow
      set w to item 1 of sizeValue
      set h to item 2 of sizeValue
    end try
  end if
  set d to ASCII character 31
  return appName & d & bundleId & d & (pidValue as text) & d & winTitle & d & (x as text) & d & (y as text) & d & (w as text) & d & (h as text)
end tell
'''.strip()
    result = run_osascript(script)
    if result.returncode != 0:
        return None, permission_error(result.stderr) or result.stderr.strip() or "osascript failed"
    parts = result.stdout.rstrip("\n").split(delimiter)
    if len(parts) < 8:
        return None, f"unexpected frontmost window response: {result.stdout!r}"
    app_name, bundle_id, pid_raw, title, x_raw, y_raw, w_raw, h_raw = parts[:8]
    if not bundle_id and app_name:
        bundle_result = run_osascript(
            f"try\nid of application {applescript_quote(app_name)}\non error\nreturn \"\"\nend try",
            timeout=3.0,
        )
        if bundle_result.returncode == 0:
            bundle_id = bundle_result.stdout.strip()
    bounds = [
        parse_float(x_raw),
        parse_float(y_raw),
        parse_float(w_raw),
        parse_float(h_raw),
    ]
    pid = parse_int(pid_raw)
    window_id = stable_window_id(app_name, bundle_id, pid, title, bounds)
    return (
        {
            "appName": app_name,
            "bundleId": bundle_id,
            "pid": pid,
            "title": title,
            "bounds": bounds,
            "windowId": window_id,
            "hasWindow": bounds[2] > 0 and bounds[3] > 0,
        },
        None,
    )


def desktop_screen_size() -> list[int] | None:
    result = run_osascript(
        'tell application "Finder" to get bounds of window of desktop',
        timeout=3.0,
    )
    if result.returncode != 0:
        return None
    nums = [parse_int(part.strip()) for part in result.stdout.strip().split(",")]
    if len(nums) != 4:
        return None
    return [max(0, nums[2] - nums[0]), max(0, nums[3] - nums[1])]


def capture_screenshot(run_id: str, observation_id: str) -> tuple[str | None, str | None]:
    root = Path(tempfile.gettempdir()) / "omiga-computer-use" / run_id
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{observation_id}.png"
    result = run_command(["screencapture", "-x", "-t", "png", str(path)], timeout=8.0)
    if result.returncode != 0 or not path.exists():
        if path.exists():
            try:
                path.unlink()
            except OSError:
                pass
        return None, permission_error(result.stderr) or result.stderr.strip() or "screencapture failed"
    return str(path), None


def allowed_apps(args: dict[str, Any]) -> list[str]:
    raw = args.get("allowedApps")
    if isinstance(raw, list):
        values = [str(item).strip().lower() for item in raw if str(item).strip()]
    else:
        values = []
    return values or ["omiga", "com.omiga.desktop"]


def target_is_allowed(args: dict[str, Any], app_name: str, bundle_id: str) -> bool:
    allowed = allowed_apps(args)
    if "*" in allowed:
        return True
    app = app_name.strip().lower()
    bundle = bundle_id.strip().lower()
    return app in allowed or bundle in allowed


def app_not_allowed_result(
    args: dict[str, Any],
    app_name: str,
    bundle_id: str,
    window_title: str,
    window_id: Any | None = None,
) -> dict[str, Any]:
    target: dict[str, Any] = {
        "appName": app_name,
        "bundleId": bundle_id,
        "windowTitle": window_title,
    }
    if window_id is not None:
        target["windowId"] = window_id
    return {
        "ok": False,
        "backend": "computer-use-macos-mcp" if not should_use_mock() else "computer-use-mock-mcp",
        "error": "app_not_allowed",
        "message": "Target app is not listed in Settings → Computer Use allowed apps.",
        "target": target,
        "allowedApps": allowed_apps(args),
        "requiresSettingsChange": True,
        "requiresConfirmation": True,
        "targetVisible": False,
        "occluded": True,
        "safeToAct": False,
        "requiresObserve": True,
    }


def bool_arg(args: dict[str, Any], key: str, default: bool = False) -> bool:
    value = args.get(key, default)
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.strip().lower() in {"1", "true", "yes", "on"}
    return bool(value)


def mac_observe(args: dict[str, Any], *, screenshot: bool | None = None) -> dict[str, Any]:
    run_id = run_id_arg(args)
    observation_id = f"obs_mac_{now_ms()}_{uuid.uuid4().hex[:8]}"
    window, window_error = query_frontmost_window()
    if window_error:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "macos_permission_or_window_query_failed",
            "message": window_error,
            "requiresPermission": True,
            "safeToAct": False,
            "observationId": observation_id,
        }
    assert window is not None
    if not target_is_allowed(args, window["appName"], window["bundleId"]):
        blocked = app_not_allowed_result(
            args,
            window["appName"],
            window["bundleId"],
            window["title"],
            window["windowId"],
        )
        blocked["observationId"] = observation_id
        return blocked
    screenshot_path = None
    screenshot_error = None
    if screenshot is None:
        screenshot = bool_arg(args, "saveScreenshot", False)
    if screenshot:
        screenshot_path, screenshot_error = capture_screenshot(run_id, observation_id)
    screen_size = desktop_screen_size()
    target = {
        "appName": window["appName"],
        "bundleId": window["bundleId"],
        "windowTitle": window["title"],
        "pid": window["pid"],
        "windowId": window["windowId"],
        "bounds": window["bounds"],
    }
    elements = []
    if window["hasWindow"]:
        elements.append(
            {
                "id": "active-window",
                "role": "window",
                "label": window["title"] or window["appName"],
                "bounds": window["bounds"],
            }
        )
    result = {
        "ok": True,
        "backend": "computer-use-macos-mcp",
        "observationId": observation_id,
        "screenshotPath": screenshot_path,
        "screenSize": screen_size,
        "frontmostApp": window["appName"],
        "activeWindowTitle": window["title"],
        "target": target,
        "targetVisible": bool(window["hasWindow"]),
        "occluded": False,
        "safeToAct": bool(window["hasWindow"]),
        "elements": elements,
        "observedAt": now_ms(),
    }
    if screenshot_error:
        result["screenshotError"] = screenshot_error
        result["screenRecordingMayBeRequired"] = True
    OBSERVATIONS[observation_id] = result
    return result


def validate_current_target(args: dict[str, Any]) -> dict[str, Any]:
    run_id = run_id_arg(args)
    if run_id in STOPPED_RUNS:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "run_stopped",
            "safeToAct": False,
            "requiresObserve": True,
        }
    current = mac_observe(args, screenshot=False)
    if not current.get("ok"):
        current["safeToAct"] = False
        current["requiresObserve"] = True
        return current
    expected_window_id = string_arg(args, "targetWindowId")
    actual_window_id = active_window_id(current)
    if expected_window_id and actual_window_id != expected_window_id:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "target_window_changed",
            "reason": "frontmost target no longer matches targetWindowId",
            "expectedTargetWindowId": expected_window_id,
            "actualTargetWindowId": actual_window_id,
            "targetVisible": False,
            "occluded": True,
            "safeToAct": False,
            "requiresObserve": True,
            "currentTarget": current.get("target"),
        }
    target = current.get("target") if isinstance(current.get("target"), dict) else {}
    bounds = target.get("bounds") if isinstance(target, dict) else None
    if isinstance(bounds, list) and len(bounds) == 4 and "x" in args and "y" in args:
        x = float(args.get("x"))
        y = float(args.get("y"))
        bx, by, bw, bh = [float(v) for v in bounds]
        if x < bx or y < by or x > bx + bw or y > by + bh:
            return {
                "ok": False,
                "backend": "computer-use-macos-mcp",
                "error": "point_outside_target_window",
                "targetWindowId": actual_window_id,
                "bounds": bounds,
                "x": x,
                "y": y,
                "targetVisible": True,
                "occluded": False,
                "safeToAct": False,
                "requiresObserve": True,
            }
    return {
        "ok": True,
        "backend": "computer-use-macos-mcp",
        "observationId": args.get("observationId"),
        "targetWindowId": actual_window_id,
        "targetVisible": True,
        "occluded": False,
        "safeToAct": True,
        "currentTarget": current.get("target"),
    }


def mac_set_target(args: dict[str, Any]) -> dict[str, Any]:
    bundle_id = string_arg(args, "bundleId")
    app_name = string_arg(args, "appName")
    if bundle_id:
        script = f"tell application id {applescript_quote(bundle_id)} to activate"
    elif app_name:
        script = f"tell application {applescript_quote(app_name)} to activate"
    else:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "target_required",
            "message": "set_target requires bundleId or appName",
        }
    result = run_osascript(script, timeout=5.0)
    if result.returncode != 0:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "activate_target_failed",
            "message": permission_error(result.stderr) or result.stderr.strip(),
        }
    time.sleep(0.25)
    observed = mac_observe(args, screenshot=False)
    return {
        "ok": bool(observed.get("ok")),
        "backend": "computer-use-macos-mcp",
        "target": observed.get("target"),
        "frontmostApp": observed.get("frontmostApp"),
        "activeWindowTitle": observed.get("activeWindowTitle"),
        "message": observed.get("message"),
    }


def run_fixed_click(x: float, y: float) -> tuple[bool, str | None]:
    script = f'tell application "System Events" to click at {{{int(round(x))}, {int(round(y))}}}'
    result = run_osascript(script, timeout=5.0)
    if result.returncode == 0:
        return True, None
    return False, permission_error(result.stderr) or result.stderr.strip() or "click failed"


def mac_click(args: dict[str, Any]) -> dict[str, Any]:
    if string_arg(args, "button", "left").lower() not in {"", "left"}:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "unsupported_button",
            "message": "Phase 8 MVP supports left click only.",
            "safeToAct": False,
        }
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    x = float(args.get("x"))
    y = float(args.get("y"))
    ok, error = run_fixed_click(x, y)
    return {
        "ok": ok,
        "backend": "computer-use-macos-mcp",
        "action": "click",
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "x": x,
        "y": y,
        "button": "left",
        "error": error,
    }


def element_center(args: dict[str, Any]) -> tuple[float, float] | None:
    observation = OBSERVATIONS.get(string_arg(args, "observationId"))
    if not observation:
        return None
    element_id = string_arg(args, "elementId")
    for item in observation.get("elements") or []:
        if not isinstance(item, dict) or str(item.get("id")) != element_id:
            continue
        bounds = item.get("bounds")
        if isinstance(bounds, list) and len(bounds) == 4:
            return float(bounds[0]) + float(bounds[2]) / 2, float(bounds[1]) + float(bounds[3]) / 2
    return None


def mac_click_element(args: dict[str, Any]) -> dict[str, Any]:
    center = element_center(args)
    if center is None:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "element_not_found",
            "message": "Element id is not available in this sidecar process; observe again.",
            "safeToAct": False,
            "requiresObserve": True,
        }
    next_args = dict(args)
    next_args["x"], next_args["y"] = center
    result = mac_click(next_args)
    result["action"] = "click_element"
    result["elementId"] = args.get("elementId")
    return result


def clipboard_get() -> str:
    result = run_command(["pbpaste"], timeout=3.0)
    return result.stdout if result.returncode == 0 else ""


def clipboard_set(text: str) -> None:
    run_command(["pbcopy"], input_text=text, timeout=3.0)


def mac_type_text(args: dict[str, Any]) -> dict[str, Any]:
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    text = string_arg(args, "text")
    previous_clipboard = clipboard_get()
    try:
        clipboard_set(text)
        result = run_osascript(
            'tell application "System Events" to keystroke "v" using command down',
            timeout=5.0,
        )
        ok = result.returncode == 0
        error = None if ok else permission_error(result.stderr) or result.stderr.strip()
    finally:
        clipboard_set(previous_clipboard)
    return {
        "ok": ok,
        "backend": "computer-use-macos-mcp",
        "action": "type_text",
        "method": "controlled_clipboard_paste",
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "typedChars": len(text),
        "error": error,
    }


def mock_result(tool_name: str, args: dict[str, Any]) -> dict[str, Any]:
    observation_id = args.get("observationId") or "obs_mock_1"
    target_window_id = args.get("targetWindowId") or 1
    if tool_name != "stop" and not target_is_allowed(args, "Omiga", "com.omiga.desktop"):
        return app_not_allowed_result(
            args,
            "Omiga",
            "com.omiga.desktop",
            "Mock Window",
            target_window_id,
        )
    if tool_name == "observe":
        result = {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "observationId": "obs_mock_1",
            "screenshotPath": None,
            "screenSize": [1400, 900],
            "frontmostApp": "Omiga",
            "activeWindowTitle": "Mock Window",
            "target": {
                "appName": "Omiga",
                "bundleId": "com.omiga.desktop",
                "windowTitle": "Mock Window",
                "pid": 1,
                "windowId": 1,
                "bounds": [0, 0, 1400, 900],
            },
            "targetVisible": True,
            "occluded": False,
            "safeToAct": True,
            "elements": [
                {
                    "id": "active-window",
                    "role": "window",
                    "label": "Mock Window",
                    "bounds": [0, 0, 1400, 900],
                },
                {
                    "id": "button-save",
                    "role": "button",
                    "label": "Save",
                    "bounds": [100, 100, 80, 32],
                },
            ],
            "mockedAt": now_ms(),
        }
        OBSERVATIONS["obs_mock_1"] = result
        return result
    if tool_name == "set_target":
        app_name = str(args.get("appName") or "Omiga")
        bundle_id = str(args.get("bundleId") or "com.omiga.desktop")
        window_title = str(args.get("windowTitle") or "Mock Window")
        if not target_is_allowed(args, app_name, bundle_id):
            return app_not_allowed_result(args, app_name, bundle_id, window_title, 1)
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "target": {
                "appName": app_name,
                "bundleId": bundle_id,
                "windowTitle": window_title,
                "windowId": 1,
            },
        }
    if tool_name == "validate_target":
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "target": {
                "appName": "Omiga",
                "bundleId": "com.omiga.desktop",
                "windowTitle": "Mock Window",
                "windowId": target_window_id,
            },
            "targetVisible": True,
            "occluded": False,
            "safeToAct": True,
        }
    if tool_name == "click":
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "click",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "x": args.get("x"),
            "y": args.get("y"),
            "button": args.get("button") or "left",
        }
    if tool_name == "click_element":
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "click_element",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "elementId": args.get("elementId"),
        }
    if tool_name == "type_text":
        text = string_arg(args, "text")
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "type_text",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "typedChars": len(text),
        }
    if tool_name == "stop":
        STOPPED_RUNS.add(run_id_arg(args))
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "stop",
            "reason": args.get("reason") or "requested",
        }
    return {"ok": False, "error": f"unknown tool {tool_name}"}


def real_result(tool_name: str, args: dict[str, Any]) -> dict[str, Any]:
    if not IS_DARWIN:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "error": "unsupported_platform",
            "message": "Phase 8 Computer Use backend supports macOS only.",
            "safeToAct": False,
        }
    if tool_name == "observe":
        return mac_observe(args)
    if tool_name == "set_target":
        return mac_set_target(args)
    if tool_name == "validate_target":
        return validate_current_target(args)
    if tool_name == "click":
        return mac_click(args)
    if tool_name == "click_element":
        return mac_click_element(args)
    if tool_name == "type_text":
        return mac_type_text(args)
    if tool_name == "stop":
        STOPPED_RUNS.add(run_id_arg(args))
        return {
            "ok": True,
            "backend": "computer-use-macos-mcp",
            "action": "stop",
            "reason": args.get("reason") or "requested",
        }
    return {"ok": False, "backend": "computer-use-macos-mcp", "error": f"unknown tool {tool_name}"}


def tool_result(tool_name: str, args: dict[str, Any]) -> dict[str, Any]:
    if should_use_mock():
        return mock_result(tool_name, args)
    return real_result(tool_name, args)


def handle(req: dict[str, Any]) -> dict[str, Any] | None:
    method = req.get("method")
    req_id = req.get("id")
    if method == "initialize":
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": "computer-use",
                    "version": "0.2.0-macos-mvp",
                },
            },
        }
    if method == "tools/list":
        return {"jsonrpc": "2.0", "id": req_id, "result": {"tools": tools_list()}}
    if method == "tools/call":
        params = req.get("params") or {}
        name = params.get("name") or ""
        args = params.get("arguments") or {}
        if not isinstance(args, dict):
            args = {}
        result = tool_result(str(name), args)
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [{"type": "text", "text": json.dumps(result, ensure_ascii=False)}],
                "isError": not bool(result.get("ok", False)),
            },
        }
    if method and str(method).startswith("notifications/"):
        return None
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "error": {"code": -32601, "message": f"Method not found: {method}"},
    }


def main() -> None:
    while True:
        req = read_message()
        if req is None:
            return
        try:
            resp = handle(req)
        except Exception as exc:
            resp = {
                "jsonrpc": "2.0",
                "id": req.get("id") if isinstance(req, dict) else None,
                "error": {"code": -32000, "message": str(exc)},
            }
        if resp is not None and resp.get("id") is not None:
            write_message(resp)


if __name__ == "__main__":
    main()
