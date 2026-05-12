#!/usr/bin/env python3
"""Smoke tests for Omiga Computer Use MCP sidecars.

The default suite is deterministic and side-effect free. Real backend suites are
"safe" probes: they observe the current target and exercise validation failures
that should not click or type.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
PYTHON_BACKEND = (
    REPO_ROOT
    / "src-tauri"
    / "bundled_plugins"
    / "plugins"
    / "computer-use"
    / "bin"
    / "computer-use-macos.py"
)
RUST_BIN_CANDIDATES = [
    REPO_ROOT / "src-tauri" / "target" / "debug" / "computer-use-sidecar",
    REPO_ROOT / "src-tauri" / "target" / "release" / "computer-use-sidecar",
    REPO_ROOT
    / "src-tauri"
    / "bundled_plugins"
    / "plugins"
    / "computer-use"
    / "bin"
    / "computer-use-sidecar",
]


class SmokeFailure(RuntimeError):
    pass


class McpClient:
    def __init__(self, command: list[str], env: dict[str, str]) -> None:
        self.command = command
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
        )
        self.next_id = 1

    def close(self) -> None:
        if self.process.stdin:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
        self.process.terminate()
        try:
            self.process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=2)

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        payload: dict[str, Any] = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if params is not None:
            payload["params"] = params
        self._write(payload)
        return self._read()

    def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        response = self.request(
            "tools/call",
            {"name": name, "arguments": arguments},
        )
        try:
            content = response["result"]["content"][0]["text"]
            return json.loads(content)
        except (KeyError, IndexError, TypeError, json.JSONDecodeError) as error:
            raise SmokeFailure(f"invalid tools/call response for {name}: {response}") from error

    def _write(self, payload: dict[str, Any]) -> None:
        if not self.process.stdin:
            raise SmokeFailure("process stdin unavailable")
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.process.stdin.write(
            b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body
        )
        self.process.stdin.flush()

    def _read(self) -> dict[str, Any]:
        if not self.process.stdout:
            raise SmokeFailure("process stdout unavailable")
        headers: list[str] = []
        while True:
            line = self.process.stdout.readline()
            if line == b"":
                stderr = self.process.stderr.read().decode("utf-8", "replace") if self.process.stderr else ""
                raise SmokeFailure(f"sidecar exited before response; stderr={stderr!r}")
            if line in (b"\r\n", b"\n"):
                break
            headers.append(line.decode("utf-8", "replace").strip())
        length = 0
        for header in headers:
            key, _, value = header.partition(":")
            if key.lower() == "content-length":
                length = int(value.strip())
                break
        if length <= 0:
            raise SmokeFailure(f"missing Content-Length in headers: {headers}")
        body = self.process.stdout.read(length)
        return json.loads(body.decode("utf-8"))


def assert_true(condition: bool, message: str) -> None:
    if not condition:
        raise SmokeFailure(message)


def resolve_rust_bin(path: str | None) -> Path:
    if path:
        candidate = Path(path).expanduser()
        if candidate.is_file():
            return candidate
        raise SmokeFailure(f"Rust sidecar binary not found: {candidate}")
    for candidate in RUST_BIN_CANDIDATES:
        if candidate.is_file():
            return candidate
    raise SmokeFailure(
        "Rust sidecar binary not found. Run "
        "`cargo build --manifest-path src-tauri/Cargo.toml --bin computer-use-sidecar` "
        "or pass --rust-bin PATH."
    )


def run_initialize(client: McpClient) -> str:
    response = client.request("initialize", {})
    version = response["result"]["serverInfo"]["version"]
    tools = client.request("tools/list", {})["result"]["tools"]
    assert_true(any(tool.get("name") == "observe" for tool in tools), "observe tool missing")
    assert_true(any(tool.get("name") == "key_press" for tool in tools), "key_press tool missing")
    assert_true(any(tool.get("name") == "drag" for tool in tools), "drag tool missing")
    assert_true(any(tool.get("name") == "scroll" for tool in tools), "scroll tool missing")
    assert_true(any(tool.get("name") == "shortcut" for tool in tools), "shortcut tool missing")
    return str(version)


def run_mock_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    observe = client.call_tool("observe", {"allowedApps": ["Omiga"], "runId": "smoke_mock"})
    assert_true(observe.get("ok") is True, f"{backend_label} mock observe failed: {observe}")
    elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
    save_element = next(
        (element for element in elements if isinstance(element, dict) and element.get("id") == "button-save"),
        None,
    )
    assert_true(
        isinstance(save_element, dict)
        and save_element.get("kind") == "button"
        and save_element.get("interactable") is True
        and save_element.get("parentId") == "active-window"
        and save_element.get("depth") == 1
        and save_element.get("labelSource") == "name",
        f"{backend_label} mock observe missing semantic element metadata: {observe}",
    )
    visual_observe = client.call_tool(
        "observe",
        {"allowedApps": ["Omiga"], "runId": "smoke_mock_visual_text", "extractVisualText": True},
    )
    assert_true(
        visual_observe.get("visualTextRequested") is True
        and isinstance(visual_observe.get("visualText"), list)
        and visual_observe.get("visualTextCount", 0) >= 1,
        f"{backend_label} mock observe missing visual text metadata: {visual_observe}",
    )

    smuggle = client.call_tool(
        "set_target",
        {
            "appName": "Calculator",
            "bundleId": "com.apple.calculator",
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        smuggle.get("error") == "app_not_allowed",
        f"{backend_label} mock set_target smuggle was not blocked: {smuggle}",
    )

    observation_id = str(observe.get("observationId") or "obs_mock_1")
    target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
    target_window_id = target.get("windowId") or 1

    direct = client.call_tool(
        "type_text",
        {
            "text": "hello",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "allowClipboardFallback": False,
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(direct.get("ok") is True, f"{backend_label} mock direct type failed: {direct}")
    assert_true(
        direct.get("method") == "direct_keystroke"
        and direct.get("clipboardFallbackUsed") is False,
        f"{backend_label} mock direct type did not use direct/no-clipboard path: {direct}",
    )

    click_element = client.call_tool(
        "click_element",
        {
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "elementId": "button-save",
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        click_element.get("ok") is True
        and click_element.get("action") == "click_element"
        and click_element.get("elementId") == "button-save",
        f"{backend_label} mock click_element did not use observed element id: {click_element}",
    )

    key_press = client.call_tool(
        "key_press",
        {
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "key": "page_down",
            "count": 2,
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        key_press.get("ok") is True
        and key_press.get("action") == "key_press"
        and key_press.get("key") == "page_down"
        and key_press.get("count") == 2,
        f"{backend_label} mock key_press failed: {key_press}",
    )

    drag = client.call_tool(
        "drag",
        {
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "startX": 20,
            "startY": 20,
            "endX": 80,
            "endY": 50,
            "durationMs": 120,
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        drag.get("ok") is True
        and drag.get("action") == "drag"
        and drag.get("durationMs") == 120
        and drag.get("button") == "left",
        f"{backend_label} mock drag failed: {drag}",
    )

    scroll = client.call_tool(
        "scroll",
        {
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "direction": "down",
            "amount": 3,
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        scroll.get("ok") is True
        and scroll.get("action") == "scroll"
        and scroll.get("direction") == "down"
        and scroll.get("amount") == 3,
        f"{backend_label} mock scroll failed: {scroll}",
    )

    shortcut = client.call_tool(
        "shortcut",
        {
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "shortcut": "select_all",
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        shortcut.get("ok") is True
        and shortcut.get("action") == "shortcut"
        and shortcut.get("shortcut") == "select_all",
        f"{backend_label} mock shortcut failed: {shortcut}",
    )
    unsupported_shortcut = client.call_tool(
        "shortcut",
        {
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "shortcut": "command_q",
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(
        unsupported_shortcut.get("ok") is False
        and unsupported_shortcut.get("error") == "unsupported_shortcut",
        f"{backend_label} mock unsupported shortcut was not blocked: {unsupported_shortcut}",
    )

    secret_text = "password = " + ("x" * 600)
    secret = client.call_tool(
        "type_text",
        {
            "text": secret_text,
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "allowClipboardFallback": True,
            "allowedApps": ["Omiga"],
            "runId": "smoke_mock",
        },
    )
    assert_true(secret.get("ok") is False, f"{backend_label} mock secret unexpectedly typed")
    assert_true(
        secret.get("clipboardFallbackUsed") is False,
        f"{backend_label} mock secret used clipboard fallback: {secret}",
    )
    assert_true(secret_text not in json.dumps(secret), "typed secret leaked in mock response")

    return {
        "suite": f"{backend_label}-mock",
        "version": version,
        "observeBackend": observe.get("backend"),
        "semanticElementKind": save_element.get("kind") if isinstance(save_element, dict) else None,
        "visualTextCount": visual_observe.get("visualTextCount"),
        "clickElementAction": click_element.get("action"),
        "keyPressAction": key_press.get("action"),
        "dragAction": drag.get("action"),
        "scrollAction": scroll.get("action"),
        "shortcutAction": shortcut.get("action"),
        "unsupportedShortcutError": unsupported_shortcut.get("error"),
        "secretClipboardFallbackUsed": secret.get("clipboardFallbackUsed"),
    }


def run_real_safe_suite(
    client: McpClient,
    backend_label: str,
    *,
    require_real_observe: bool = False,
) -> dict[str, Any]:
    version = run_initialize(client)
    run_id = f"smoke_{backend_label}_real_safe"
    observe = client.call_tool(
        "observe",
        {"allowedApps": ["*"], "saveScreenshot": False, "runId": run_id},
    )
    if observe.get("ok") is not True:
        assert_true(observe.get("safeToAct") is False, f"unsafe observe failure shape: {observe}")
        if require_real_observe:
            raise SmokeFailure(
                f"{backend_label} real-safe observe did not succeed. "
                "Grant macOS Accessibility/Screen Recording permissions and retry, "
                f"or omit --require-real-observe for fail-closed safe probes. observe={observe}"
            )
        return {
            "suite": f"{backend_label}-real-safe",
            "version": version,
            "observeOk": False,
            "observeError": observe.get("error"),
            "safeFailureOnly": True,
        }
    elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
    active_window = next(
        (element for element in elements if isinstance(element, dict) and element.get("id") == "active-window"),
        None,
    )
    assert_true(
        isinstance(active_window, dict)
        and active_window.get("kind") == "window"
        and active_window.get("depth") == 0,
        f"{backend_label} real-safe observe missing semantic active-window metadata: {observe}",
    )

    target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
    bounds = target.get("bounds") if isinstance(target.get("bounds"), list) else [0, 0, 0, 0]
    x = float(bounds[0]) - 10.0
    y = float(bounds[1]) - 10.0
    observation_id = str(observe.get("observationId"))
    target_window_id = str(target.get("windowId"))

    outside = client.call_tool(
        "click",
        {
            "allowedApps": ["*"],
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "x": x,
            "y": y,
            "runId": run_id,
        },
    )
    assert_true(
        outside.get("error") in {"point_outside_target_window", "target_window_changed", "target_changed"},
        f"{backend_label} real-safe outside click was not blocked: {outside}",
    )

    wrong_target_type = client.call_tool(
        "type_text",
        {
            "allowedApps": ["*"],
            "observationId": observation_id,
            "targetWindowId": "definitely_wrong",
            "text": "hello",
            "runId": run_id,
        },
    )
    assert_true(
        wrong_target_type.get("error") in {"target_window_changed", "target_changed"},
        f"{backend_label} real-safe wrong target type was not blocked: {wrong_target_type}",
    )

    stop = client.call_tool("stop", {"runId": run_id, "reason": "smoke_done"})
    assert_true(stop.get("ok") is True, f"{backend_label} real-safe stop failed: {stop}")

    after_stop = client.call_tool(
        "validate_target",
        {
            "allowedApps": ["*"],
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "runId": run_id,
        },
    )
    assert_true(
        after_stop.get("error") == "run_stopped",
        f"{backend_label} real-safe run_stopped guard failed: {after_stop}",
    )

    return {
        "suite": f"{backend_label}-real-safe",
        "version": version,
        "observeOk": True,
        "safeFailureOnly": False,
        "frontmostApp": observe.get("frontmostApp"),
        "semanticActiveWindowKind": active_window.get("kind") if isinstance(active_window, dict) else None,
        "outsideClickError": outside.get("error"),
        "wrongTargetTypeError": wrong_target_type.get("error"),
        "afterStopError": after_stop.get("error"),
    }


def element_label(element: dict[str, Any]) -> str:
    return str(element.get("label") or element.get("title") or element.get("name") or "")


def element_role(element: dict[str, Any]) -> str:
    return str(element.get("role") or "").lower()


def find_element_id(elements: list[Any], *, label: str, role_contains: str | None = None) -> str | None:
    wanted_label = label.strip().lower()
    wanted_role = role_contains.strip().lower() if role_contains else None
    for element in elements:
        if not isinstance(element, dict):
            continue
        current_label = element_label(element).strip().lower()
        current_role = element_role(element)
        if current_label != wanted_label:
            continue
        if wanted_role and wanted_role not in current_role:
            continue
        element_id = element.get("id")
        if element_id is not None:
            return str(element_id)
    return None


def launch_dialog_target(title: str) -> subprocess.Popen[str]:
    script = (
        f'tell application "Finder"\n'
        f'activate\n'
        f'display dialog "Omiga Computer Use positive E2E" '
        f'default answer "" buttons {{"Cancel", "OK"}} '
        f'default button "OK" cancel button "Cancel" '
        f'with title "{title}"\n'
        f'end tell'
    )
    return subprocess.Popen(
        ["osascript", "-e", script],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def launch_visual_text_dialog_target(title: str) -> subprocess.Popen[str]:
    script = (
        f'tell application "Finder"\n'
        f'activate\n'
        f'display dialog "OMIGA VISUAL TEXT TARGET\\ncomputer use native OCR verification" '
        f'default answer "" buttons {{"Cancel", "OK"}} '
        f'default button "OK" cancel button "Cancel" '
        f'with title "{title}"\n'
        f'end tell'
    )
    return subprocess.Popen(
        ["osascript", "-e", script],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def activate_dialog_target() -> None:
    subprocess.run(
        ["osascript", "-e", 'tell application "Finder" to activate'],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def close_dialog_target(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=2)


def observe_dialog(client: McpClient, run_id: str, title: str) -> dict[str, Any]:
    last_observe: dict[str, Any] | None = None
    deadline = time.monotonic() + 8.0
    while time.monotonic() < deadline:
        activate_dialog_target()
        observe = client.call_tool(
            "observe",
            {"allowedApps": ["*"], "saveScreenshot": False, "runId": run_id},
        )
        last_observe = observe
        if observe.get("ok") is True:
            title_text = str(observe.get("activeWindowTitle") or "")
            elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
            labels = " ".join(element_label(element) for element in elements if isinstance(element, dict))
            if title in title_text or title in labels or find_element_id(elements, label="OK", role_contains="button"):
                return observe
        time.sleep(0.25)
    raise SmokeFailure(f"dialog target was not observed; last_observe={last_observe}")


def observe_visual_text_dialog(
    client: McpClient,
    run_id: str,
    title: str,
    process: subprocess.Popen[str],
) -> dict[str, Any]:
    last_observe: dict[str, Any] | None = None
    deadline = time.monotonic() + 8.0
    while time.monotonic() < deadline:
        if process.poll() is not None:
            stderr = process.stderr.read() if process.stderr else ""
            raise SmokeFailure(f"visual text dialog exited before observe; stderr={stderr!r}")
        activate_dialog_target()
        observe = client.call_tool(
            "observe",
            {"allowedApps": ["*"], "saveScreenshot": False, "runId": run_id},
        )
        last_observe = observe
        if observe.get("ok") is True:
            title_text = str(observe.get("activeWindowTitle") or "")
            elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
            labels = " ".join(element_label(element) for element in elements if isinstance(element, dict))
            if title in title_text or title in labels:
                return observe
        time.sleep(0.25)
    raise SmokeFailure(f"visual text dialog target was not observed; last_observe={last_observe}")


def run_real_dialog_e2e_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    token = f"omiga-positive-{uuid.uuid4().hex[:10]}"
    title = f"OmigaCU-{token}"
    run_id = f"smoke_{backend_label}_dialog_e2e_{token}"
    process = launch_dialog_target(title)
    try:
        observe = observe_dialog(client, run_id, title)
        target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
        observation_id = str(observe.get("observationId"))
        target_window_id = target.get("windowId")
        assert_true(bool(target_window_id), f"{backend_label} dialog observe missing targetWindowId: {observe}")
        elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
        ok_button_id = find_element_id(elements, label="OK", role_contains="button") or find_element_id(elements, label="OK")
        assert_true(
            ok_button_id is not None,
            f"{backend_label} dialog observe did not return an OK button element: {observe}",
        )

        typed = client.call_tool(
            "type_text",
            {
                "allowedApps": ["*"],
                "observationId": observation_id,
                "targetWindowId": target_window_id,
                "text": token,
                "allowClipboardFallback": False,
                "runId": run_id,
            },
        )
        assert_true(typed.get("ok") is True, f"{backend_label} dialog type_text failed: {typed}")
        assert_true(
            typed.get("method") == "direct_keystroke",
            f"{backend_label} dialog type_text did not use direct keystroke: {typed}",
        )

        activate_dialog_target()
        clicked = client.call_tool(
            "click_element",
            {
                "allowedApps": ["*"],
                "observationId": observation_id,
                "targetWindowId": target_window_id,
                "elementId": ok_button_id,
                "runId": run_id,
            },
        )
        assert_true(clicked.get("ok") is True, f"{backend_label} dialog click_element failed: {clicked}")

        stdout, stderr = process.communicate(timeout=5)
        assert_true(
            token in stdout and "button returned:OK" in stdout,
            f"{backend_label} dialog did not return typed text after click; stdout={stdout!r} stderr={stderr!r}",
        )
        return {
            "suite": f"{backend_label}-real-dialog-e2e",
            "version": version,
            "frontmostApp": observe.get("frontmostApp"),
            "elementCount": observe.get("elementCount", len(elements)),
            "okButtonElementId": ok_button_id,
            "typeMethod": typed.get("method"),
            "clickAction": clicked.get("action"),
            "typedTextVerified": True,
        }
    except subprocess.TimeoutExpired as error:
        raise SmokeFailure(f"{backend_label} dialog target did not close after click") from error
    finally:
        close_dialog_target(process)


def run_real_key_e2e_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    token = f"omiga-key-{uuid.uuid4().hex[:10]}"
    title = f"OmigaCUKey-{token}"
    run_id = f"smoke_{backend_label}_key_e2e_{token}"
    process = launch_dialog_target(title)
    try:
        observe: dict[str, Any] | None = None
        pressed: dict[str, Any] | None = None
        stdout = ""
        stderr = ""
        for _attempt in range(2):
            observe = observe_dialog(client, run_id, title)
            target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
            observation_id = str(observe.get("observationId"))
            target_window_id = target.get("windowId")
            assert_true(bool(target_window_id), f"{backend_label} key dialog observe missing targetWindowId: {observe}")

            activate_dialog_target()
            time.sleep(0.15)
            pressed = client.call_tool(
                "key_press",
                {
                    "allowedApps": ["*"],
                    "observationId": observation_id,
                    "targetWindowId": target_window_id,
                    "key": "enter",
                    "runId": run_id,
                },
            )
            assert_true(pressed.get("ok") is True, f"{backend_label} dialog key_press failed: {pressed}")

            try:
                stdout, stderr = process.communicate(timeout=5)
                break
            except subprocess.TimeoutExpired:
                activate_dialog_target()
                time.sleep(0.2)
        assert_true(
            process.returncode == 0 and "button returned:OK" in stdout,
            f"{backend_label} dialog did not submit after Enter key press; pressed={pressed} stdout={stdout!r} stderr={stderr!r}",
        )
        return {
            "suite": f"{backend_label}-real-key-e2e",
            "version": version,
            "frontmostApp": observe.get("frontmostApp") if observe else None,
            "elementCount": observe.get("elementCount") if observe else None,
            "keyAction": pressed.get("action") if pressed else None,
            "key": pressed.get("key") if pressed else None,
            "dialogSubmittedByKey": True,
        }
    except subprocess.TimeoutExpired as error:
        raise SmokeFailure(f"{backend_label} dialog target did not close after key press") from error
    finally:
        close_dialog_target(process)


def launch_textedit_scroll_target(title: str) -> Path:
    path = Path(tempfile.gettempdir()) / title
    path.write_text("\n".join(f"line {index:03d}" for index in range(1, 500)), encoding="utf-8")
    subprocess.run(["open", "-a", "TextEdit", str(path)], check=False)
    activate_textedit_scroll_target()
    return path


def activate_textedit_scroll_target() -> None:
    subprocess.run(
        ["osascript", "-e", 'tell application "TextEdit" to activate'],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def close_textedit_scroll_target(title: str, path: Path) -> None:
    script = f'tell application "TextEdit" to close (first window whose name is "{title}") saving no'
    subprocess.run(["osascript", "-e", script], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    try:
        path.unlink()
    except FileNotFoundError:
        pass


def set_textedit_front_window_bounds(left: int, top: int, right: int, bottom: int) -> None:
    script = f'tell application "TextEdit" to set bounds of front window to {{{left}, {top}, {right}, {bottom}}}'
    result = subprocess.run(
        ["osascript", "-e", script],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise SmokeFailure(f"failed to set TextEdit window bounds: {result.stderr!r}")


def read_textedit_front_document() -> str:
    result = subprocess.run(
        ["osascript", "-e", 'tell application "TextEdit" to text of front document'],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise SmokeFailure(f"failed to read TextEdit front document: {result.stderr!r}")
    return result.stdout


def vertical_scroll_indicator_y(observe: dict[str, Any]) -> float | None:
    elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
    for element in elements:
        if not isinstance(element, dict) or element.get("role") != "AXValueIndicator":
            continue
        bounds = element.get("bounds")
        if isinstance(bounds, list) and len(bounds) == 4:
            try:
                return float(bounds[1])
            except (TypeError, ValueError):
                return None
    return None


def observe_textedit_target(
    client: McpClient,
    run_id: str,
    title: str,
    *,
    require_scroll_indicator: bool,
) -> dict[str, Any]:
    last_observe: dict[str, Any] | None = None
    deadline = time.monotonic() + 8.0
    while time.monotonic() < deadline:
        activate_textedit_scroll_target()
        observe = client.call_tool(
            "observe",
            {"allowedApps": ["*"], "saveScreenshot": False, "runId": run_id},
        )
        last_observe = observe
        if (
            observe.get("ok") is True
            and observe.get("frontmostApp") == "TextEdit"
            and title in str(observe.get("activeWindowTitle") or "")
            and (
                not require_scroll_indicator
                or vertical_scroll_indicator_y(observe) is not None
            )
        ):
            return observe
        time.sleep(0.25)
    expected = "TextEdit scroll target" if require_scroll_indicator else "TextEdit target"
    raise SmokeFailure(f"{expected} was not observed; last_observe={last_observe}")


def observe_textedit_scroll_target(client: McpClient, run_id: str, title: str) -> dict[str, Any]:
    return observe_textedit_target(client, run_id, title, require_scroll_indicator=True)


def normalized_ocr_text(items: list[Any]) -> str:
    combined = " ".join(str(item.get("text") or "") for item in items if isinstance(item, dict))
    return re.sub(r"[^a-z0-9]+", "", combined.lower())


def visual_text_has_target_words(items: list[Any]) -> bool:
    normalized = normalized_ocr_text(items)
    return "omiga" in normalized and "visual" in normalized and "text" in normalized


def run_real_visual_text_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    title = f"OmigaCUOCR-{uuid.uuid4().hex[:10]}"
    run_id = f"smoke_{backend_label}_visual_text_{uuid.uuid4().hex[:10]}"
    process = launch_visual_text_dialog_target(title)
    try:
        observe_visual_text_dialog(client, run_id, title, process)
        observe: dict[str, Any] | None = None
        visual_text: list[Any] = []
        deadline = time.monotonic() + 6.0
        while time.monotonic() < deadline:
            observe = client.call_tool(
                "observe",
                {"allowedApps": ["*"], "saveScreenshot": False, "extractVisualText": True, "runId": run_id},
            )
            visual_text = observe.get("visualText") if isinstance(observe.get("visualText"), list) else []
            if observe.get("visualTextError") or visual_text_has_target_words(visual_text):
                break
            time.sleep(0.4)
        assert_true(observe is not None, f"{backend_label} visual text observe did not run")
        assert_true(observe.get("ok") is True, f"{backend_label} visual text observe failed: {observe}")
        assert_true(
            observe.get("visualTextRequested") is True,
            f"{backend_label} visual text observe did not acknowledge OCR request: {observe}",
        )
        assert_true(
            bool(observe.get("screenshotPath")),
            f"{backend_label} visual text observe did not capture a screenshot for OCR: {observe}",
        )
        assert_true(
            not observe.get("visualTextError"),
            f"{backend_label} visual text OCR failed: {observe}",
        )
        assert_true(
            len(visual_text) >= 1,
            f"{backend_label} visual text OCR returned no text boxes: {observe}",
        )
        normalized = normalized_ocr_text(visual_text)
        assert_true(
            visual_text_has_target_words(visual_text),
            f"{backend_label} visual text OCR did not recognize target text; normalized={normalized!r} observe={observe}",
        )
        return {
            "suite": f"{backend_label}-real-visual-text",
            "version": version,
            "frontmostApp": observe.get("frontmostApp"),
            "visualTextCount": observe.get("visualTextCount", len(visual_text)),
            "visualTextSource": observe.get("visualTextSource"),
            "targetTextRecognized": True,
        }
    finally:
        close_dialog_target(process)


def target_bounds(observe: dict[str, Any]) -> tuple[float, float, float, float]:
    target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
    bounds = target.get("bounds")
    if not (isinstance(bounds, list) and len(bounds) == 4):
        raise SmokeFailure(f"observe missing target bounds: {observe}")
    try:
        return tuple(float(value) for value in bounds)  # type: ignore[return-value]
    except (TypeError, ValueError) as error:
        raise SmokeFailure(f"observe has invalid target bounds: {observe}") from error


def element_bounds_by_role(observe: dict[str, Any], role: str) -> tuple[float, float, float, float] | None:
    elements = observe.get("elements") if isinstance(observe.get("elements"), list) else []
    for element in elements:
        if not isinstance(element, dict) or element.get("role") != role:
            continue
        bounds = element.get("bounds")
        if not (isinstance(bounds, list) and len(bounds) == 4):
            continue
        try:
            return tuple(float(value) for value in bounds)  # type: ignore[return-value]
        except (TypeError, ValueError):
            continue
    return None


def run_real_scroll_e2e_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    title = f"omiga-cu-scroll-{uuid.uuid4().hex[:10]}.txt"
    run_id = f"smoke_{backend_label}_scroll_e2e_{title}"
    path = launch_textedit_scroll_target(title)
    try:
        activate_textedit_scroll_target()
        time.sleep(0.2)
        observe = observe_textedit_scroll_target(client, run_id, title)
        before_y = vertical_scroll_indicator_y(observe)
        assert_true(before_y is not None, f"{backend_label} scroll observe missing indicator: {observe}")
        target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
        observation_id = str(observe.get("observationId"))
        target_window_id = target.get("windowId")
        assert_true(bool(target_window_id), f"{backend_label} scroll observe missing targetWindowId: {observe}")

        scrolled: dict[str, Any] | None = None
        after: dict[str, Any] | None = None
        after_y: float | None = None
        for attempt in range(3):
            activate_textedit_scroll_target()
            time.sleep(0.1)
            if attempt > 0:
                retry_observe = observe_textedit_scroll_target(client, run_id, title)
                observation_id = str(retry_observe.get("observationId"))
                retry_target = retry_observe.get("target") if isinstance(retry_observe.get("target"), dict) else {}
                target_window_id = retry_target.get("windowId")
            scrolled = client.call_tool(
                "scroll",
                {
                    "allowedApps": ["*"],
                    "observationId": observation_id,
                    "targetWindowId": target_window_id,
                    "direction": "down",
                    "amount": 20,
                    "runId": run_id,
                },
            )
            assert_true(scrolled.get("ok") is True, f"{backend_label} scroll failed: {scrolled}")

            deadline = time.monotonic() + 3.0
            while time.monotonic() < deadline:
                activate_textedit_scroll_target()
                after = observe_textedit_scroll_target(client, run_id, title)
                after_y = vertical_scroll_indicator_y(after)
                if after_y is not None and after_y > before_y + 1.0:
                    break
                time.sleep(0.25)
            if after_y is not None and after_y > before_y + 1.0:
                break
        assert_true(
            after_y is not None and after_y > before_y + 1.0,
            f"{backend_label} scroll did not move vertical indicator; before={before_y} after={after_y} scrolled={scrolled} last={after}",
        )
        return {
            "suite": f"{backend_label}-real-scroll-e2e",
            "version": version,
            "frontmostApp": observe.get("frontmostApp"),
            "beforeIndicatorY": before_y,
            "afterIndicatorY": after_y,
            "scrollAction": scrolled.get("action"),
            "direction": scrolled.get("direction"),
            "amount": scrolled.get("amount"),
        }
    finally:
        close_textedit_scroll_target(title, path)


def run_real_drag_e2e_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    title = f"omiga-cu-drag-{uuid.uuid4().hex[:10]}.txt"
    run_id = f"smoke_{backend_label}_drag_e2e_{title}"
    path = Path(tempfile.gettempdir()) / title
    path.write_text("drag-window-move-target\n", encoding="utf-8")
    subprocess.run(["open", "-a", "TextEdit", str(path)], check=False)
    activate_textedit_scroll_target()
    try:
        time.sleep(0.3)
        set_textedit_front_window_bounds(160, 180, 760, 640)
        time.sleep(0.2)
        observe = observe_textedit_target(client, run_id, title, require_scroll_indicator=False)
        before_x, before_y, width, height = target_bounds(observe)
        target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
        observation_id = str(observe.get("observationId"))
        target_window_id = target.get("windowId")
        assert_true(bool(target_window_id), f"{backend_label} drag observe missing targetWindowId: {observe}")
        start_x = before_x + width - 120
        start_y = before_y + 14
        end_x = min(start_x + 90, before_x + width - 20)
        end_y = min(start_y + 32, before_y + height - 30)

        activate_textedit_scroll_target()
        dragged = client.call_tool(
            "drag",
            {
                "allowedApps": ["*"],
                "observationId": observation_id,
                "targetWindowId": target_window_id,
                "startX": start_x,
                "startY": start_y,
                "endX": end_x,
                "endY": end_y,
                "durationMs": 350,
                "runId": run_id,
            },
        )
        assert_true(dragged.get("ok") is True, f"{backend_label} drag failed: {dragged}")

        after: dict[str, Any] | None = None
        after_x = before_x
        after_y = before_y
        deadline = time.monotonic() + 4.0
        while time.monotonic() < deadline:
            activate_textedit_scroll_target()
            after = observe_textedit_target(client, run_id, title, require_scroll_indicator=False)
            after_x, after_y, _, _ = target_bounds(after)
            if abs(after_x - before_x) >= 20 or abs(after_y - before_y) >= 12:
                break
            time.sleep(0.25)
        assert_true(
            after is not None and (abs(after_x - before_x) >= 20 or abs(after_y - before_y) >= 12),
            f"{backend_label} drag did not move TextEdit window; before=({before_x},{before_y}) after=({after_x},{after_y}) dragged={dragged} last={after}",
        )
        return {
            "suite": f"{backend_label}-real-drag-e2e",
            "version": version,
            "frontmostApp": observe.get("frontmostApp"),
            "dragAction": dragged.get("action"),
            "start": [start_x, start_y],
            "end": [end_x, end_y],
            "beforeBounds": [before_x, before_y, width, height],
            "afterBounds": [after_x, after_y],
            "windowMoved": True,
        }
    finally:
        close_textedit_scroll_target(title, path)


def run_real_shortcut_e2e_suite(client: McpClient, backend_label: str) -> dict[str, Any]:
    version = run_initialize(client)
    title = f"omiga-cu-shortcut-{uuid.uuid4().hex[:10]}.txt"
    run_id = f"smoke_{backend_label}_shortcut_e2e_{title}"
    path = Path(tempfile.gettempdir()) / title
    original_text = "original-shortcut-content\n"
    replacement = f"42-shortcut-replaced-{uuid.uuid4().hex[:10]}"
    path.write_text(original_text, encoding="utf-8")
    subprocess.run(["open", "-a", "TextEdit", str(path)], check=False)
    activate_textedit_scroll_target()
    try:
        observe = observe_textedit_target(client, run_id, title, require_scroll_indicator=False)
        target = observe.get("target") if isinstance(observe.get("target"), dict) else {}
        observation_id = str(observe.get("observationId"))
        target_window_id = target.get("windowId")
        assert_true(bool(target_window_id), f"{backend_label} shortcut observe missing targetWindowId: {observe}")

        text_area = element_bounds_by_role(observe, "AXTextArea")
        if text_area is not None:
            tx, ty, tw, _th = text_area
            activate_textedit_scroll_target()
            focused = client.call_tool(
                "click",
                {
                    "allowedApps": ["*"],
                    "observationId": observation_id,
                    "targetWindowId": target_window_id,
                    "x": tx + min(40, max(10, tw / 4)),
                    "y": ty + 20,
                    "runId": run_id,
                },
            )
            assert_true(focused.get("ok") is True, f"{backend_label} shortcut text focus click failed: {focused}")
            time.sleep(0.15)

        activate_textedit_scroll_target()
        shortcut = client.call_tool(
            "shortcut",
            {
                "allowedApps": ["*"],
                "observationId": observation_id,
                "targetWindowId": target_window_id,
                "shortcut": "select_all",
                "runId": run_id,
            },
        )
        assert_true(shortcut.get("ok") is True, f"{backend_label} select_all shortcut failed: {shortcut}")
        time.sleep(0.15)
        activate_textedit_scroll_target()

        typed = client.call_tool(
            "type_text",
            {
                "allowedApps": ["*"],
                "observationId": observation_id,
                "targetWindowId": target_window_id,
                "text": replacement,
                "allowClipboardFallback": False,
                "runId": run_id,
            },
        )
        assert_true(typed.get("ok") is True, f"{backend_label} shortcut replacement type failed: {typed}")
        time.sleep(0.3)
        document_text = read_textedit_front_document()
        assert_true(
            replacement in document_text and "original-shortcut-content" not in document_text,
            f"{backend_label} select_all did not cause replacement; document_text={document_text!r}",
        )
        return {
            "suite": f"{backend_label}-real-shortcut-e2e",
            "version": version,
            "frontmostApp": observe.get("frontmostApp"),
            "shortcutAction": shortcut.get("action"),
            "shortcut": shortcut.get("shortcut"),
            "typeMethod": typed.get("method"),
            "replacementVerified": True,
        }
    finally:
        close_textedit_scroll_target(title, path)


def run_suite(name: str, args: argparse.Namespace) -> dict[str, Any]:
    env = os.environ.copy()
    env.setdefault("PYTHONDONTWRITEBYTECODE", "1")
    if name == "rust-mock":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "mock"
        client = McpClient([str(bin_path)], env)
        try:
            return run_mock_suite(client, "rust")
        finally:
            client.close()
    if name == "rust-real-safe":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_safe_suite(
                client,
                "rust",
                require_real_observe=args.require_real_observe,
            )
        finally:
            client.close()
    if name == "rust-real-dialog-e2e":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_dialog_e2e_suite(client, "rust")
        finally:
            client.close()
    if name == "rust-real-key-e2e":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_key_e2e_suite(client, "rust")
        finally:
            client.close()
    if name == "rust-real-drag-e2e":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_drag_e2e_suite(client, "rust")
        finally:
            client.close()
    if name == "rust-real-scroll-e2e":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_scroll_e2e_suite(client, "rust")
        finally:
            client.close()
    if name == "rust-real-shortcut-e2e":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_shortcut_e2e_suite(client, "rust")
        finally:
            client.close()
    if name == "rust-real-visual-text":
        bin_path = resolve_rust_bin(args.rust_bin)
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(bin_path)], env)
        try:
            return run_real_visual_text_suite(client, "rust")
        finally:
            client.close()
    if name == "python-mock":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "mock"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_mock_suite(client, "python")
        finally:
            client.close()
    if name == "python-real-safe":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_safe_suite(
                client,
                "python",
                require_real_observe=args.require_real_observe,
            )
        finally:
            client.close()
    if name == "python-real-dialog-e2e":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_dialog_e2e_suite(client, "python")
        finally:
            client.close()
    if name == "python-real-key-e2e":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_key_e2e_suite(client, "python")
        finally:
            client.close()
    if name == "python-real-drag-e2e":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_drag_e2e_suite(client, "python")
        finally:
            client.close()
    if name == "python-real-scroll-e2e":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_scroll_e2e_suite(client, "python")
        finally:
            client.close()
    if name == "python-real-shortcut-e2e":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_shortcut_e2e_suite(client, "python")
        finally:
            client.close()
    if name == "python-real-visual-text":
        env["OMIGA_COMPUTER_USE_BACKEND"] = "real"
        client = McpClient([str(PYTHON_BACKEND)], env)
        try:
            return run_real_visual_text_suite(client, "python")
        finally:
            client.close()
    raise SmokeFailure(f"unknown suite: {name}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite",
        choices=[
            "mock",
            "rust-mock",
            "python-mock",
            "rust-real-safe",
            "python-real-safe",
            "rust-real-dialog-e2e",
            "python-real-dialog-e2e",
            "rust-real-key-e2e",
            "python-real-key-e2e",
            "rust-real-drag-e2e",
            "python-real-drag-e2e",
            "rust-real-scroll-e2e",
            "python-real-scroll-e2e",
            "rust-real-shortcut-e2e",
            "python-real-shortcut-e2e",
            "rust-real-visual-text",
            "python-real-visual-text",
            "all-safe",
            "dialog-e2e",
            "key-e2e",
            "drag-e2e",
            "scroll-e2e",
            "shortcut-e2e",
            "visual-text",
        ],
        default="mock",
        help="Smoke suite to run. Default: mock (rust-mock + python-mock).",
    )
    parser.add_argument("--rust-bin", help="Path to computer-use-sidecar binary.")
    parser.add_argument(
        "--require-real-observe",
        action="store_true",
        help=(
            "For real-safe suites, fail if observe cannot read a real macOS target. "
            "Use this on a permission-ready packaging/QA machine; omit it when "
            "testing only fail-closed behavior."
        ),
    )
    return parser.parse_args()


def suites_for(selected: str) -> list[str]:
    if selected == "mock":
        return ["rust-mock", "python-mock"]
    if selected == "all-safe":
        return ["rust-mock", "python-mock", "rust-real-safe", "python-real-safe"]
    if selected == "dialog-e2e":
        return ["rust-real-dialog-e2e", "python-real-dialog-e2e"]
    if selected == "key-e2e":
        return ["rust-real-key-e2e", "python-real-key-e2e"]
    if selected == "drag-e2e":
        return ["rust-real-drag-e2e", "python-real-drag-e2e"]
    if selected == "scroll-e2e":
        return ["rust-real-scroll-e2e", "python-real-scroll-e2e"]
    if selected == "shortcut-e2e":
        return ["rust-real-shortcut-e2e", "python-real-shortcut-e2e"]
    if selected == "visual-text":
        return ["rust-real-visual-text", "python-real-visual-text"]
    return [selected]


def main() -> int:
    args = parse_args()
    results: list[dict[str, Any]] = []
    for suite in suites_for(args.suite):
        results.append(run_suite(suite, args))
    print(json.dumps({"ok": True, "results": results}, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeFailure as error:
        print(json.dumps({"ok": False, "error": str(error)}, ensure_ascii=False, indent=2), file=sys.stderr)
        raise SystemExit(1)
