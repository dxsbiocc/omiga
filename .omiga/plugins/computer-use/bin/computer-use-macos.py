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

import ctypes
import hashlib
import json
import os
import platform
import re
import shutil
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
MAX_DIRECT_TYPE_CHARS = 512
MAX_AX_ELEMENTS = 80
MAX_AX_DEPTH = 4
MAX_AX_TEXT_CHARS = 120
MAX_VISUAL_TEXT_ITEMS = 40
MAX_VISUAL_TEXT_CHARS = 120
VISION_OCR_TIMEOUT_SECONDS = 25.0
VISION_OCR_SWIFT = r"""
import Foundation
import Vision
import CoreGraphics
import ImageIO

struct VisualText: Encodable {
    let text: String
    let confidence: Float
    let bounds: [Double]
    let source: String
}

func emit(_ rows: [VisualText]) {
    do {
        let data = try JSONEncoder().encode(rows)
        FileHandle.standardOutput.write(data)
    } catch {
        fputs("failed_to_encode_vision_ocr_json: \(error)\n", stderr)
        exit(3)
    }
}

guard CommandLine.arguments.count >= 4 else {
    fputs("usage: vision-ocr.swift IMAGE_PATH MAX_ITEMS MAX_TEXT_CHARS\n", stderr)
    exit(2)
}

let imagePath = CommandLine.arguments[1]
let maxItems = max(0, Int(CommandLine.arguments[2]) ?? 40)
let maxTextChars = max(1, Int(CommandLine.arguments[3]) ?? 120)
let imageUrl = URL(fileURLWithPath: imagePath)

guard
    let imageSource = CGImageSourceCreateWithURL(imageUrl as CFURL, nil),
    let cgImage = CGImageSourceCreateImageAtIndex(imageSource, 0, nil)
else {
    fputs("failed_to_load_image_for_vision_ocr\n", stderr)
    exit(4)
}

let imageWidth = Double(cgImage.width)
let imageHeight = Double(cgImage.height)
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = true

do {
    try VNImageRequestHandler(cgImage: cgImage, options: [:]).perform([request])
} catch {
    fputs("vision_ocr_failed: \(error)\n", stderr)
    exit(5)
}

let observations = (request.results ?? []).prefix(maxItems)
var rows: [VisualText] = []
for observation in observations {
    guard let candidate = observation.topCandidates(1).first else {
        continue
    }
    var text = candidate.string
        .replacingOccurrences(of: "\n", with: " ")
        .trimmingCharacters(in: .whitespacesAndNewlines)
    if text.isEmpty {
        continue
    }
    if text.count > maxTextChars {
        let end = text.index(text.startIndex, offsetBy: maxTextChars)
        text = String(text[..<end]) + "…"
    }
    let box = observation.boundingBox
    let x = Double(box.origin.x) * imageWidth
    let y = (1.0 - Double(box.origin.y) - Double(box.size.height)) * imageHeight
    let width = Double(box.size.width) * imageWidth
    let height = Double(box.size.height) * imageHeight
    rows.append(
        VisualText(
            text: text,
            confidence: candidate.confidence,
            bounds: [x, y, width, height],
            source: "macos_vision_ocr"
        )
    )
}

emit(rows)
"""
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
            "drag",
            "Drag between two coordinates inside the validated target with a fixed left-button mouse gesture.",
            {
                "startX": {"type": "number"},
                "startY": {"type": "number"},
                "endX": {"type": "number"},
                "endY": {"type": "number"},
                "durationMs": {"type": "integer", "minimum": 50, "maximum": 2000},
                "button": {"type": "string", "enum": ["left"]},
            },
            ["startX", "startY", "endX", "endY"],
        ),
        tool_schema(
            "type_text",
            "Type text into the validated target; direct keystrokes first, controlled clipboard fallback only when allowed.",
            {"text": {"type": "string"}},
            ["text"],
        ),
        tool_schema(
            "key_press",
            "Press a supported non-text key after target validation.",
            {
                "key": {"type": "string"},
                "count": {"type": "integer", "minimum": 1, "maximum": 20},
            },
            ["key"],
        ),
        tool_schema(
            "scroll",
            "Scroll inside the validated target with a fixed system scroll-wheel event.",
            {
                "direction": {"type": "string"},
                "amount": {"type": "integer", "minimum": 1, "maximum": 20},
            },
            ["direction"],
        ),
        tool_schema(
            "shortcut",
            "Run a fixed whitelisted keyboard shortcut after target validation.",
            {"shortcut": {"type": "string", "enum": ["select_all", "undo", "redo"]}},
            ["shortcut"],
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
            "-10827",
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


def parse_bool_text(value: str) -> bool | None:
    normalized = value.strip().lower()
    if normalized == "true":
        return True
    if normalized == "false":
        return False
    return None


def semantic_kind(role: str, subrole: str = "") -> str:
    text = f"{role} {subrole}".lower()
    if "window" in text:
        return "window"
    if "button" in text:
        if "check" in text:
            return "checkbox"
        if "radio" in text:
            return "radio"
        return "button"
    if "text field" in text or "textfield" in text or "text area" in text or "textarea" in text:
        return "text_input"
    if "static text" in text or text.strip() == "axstatictext":
        return "text"
    if "menu" in text:
        return "menu"
    if "scroll" in text or "valueindicator" in text:
        return "scroll"
    if "link" in text:
        return "link"
    if "image" in text:
        return "image"
    if "table" in text or "outline" in text or "list" in text:
        return "collection"
    if "slider" in text:
        return "slider"
    return "unknown"


def semantic_interactable(kind: str, enabled: bool | None, role: str) -> bool:
    if enabled is False:
        return False
    if kind in {"button", "checkbox", "radio", "text_input", "menu", "link", "slider", "scroll"}:
        return True
    return "button" in role.lower() or enabled is True


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
  set d to character id 31
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


def parse_ax_element_rows(raw: str) -> list[dict[str, Any]]:
    elements: list[dict[str, Any]] = []
    if not raw.strip():
        return elements
    for row in raw.rstrip("\n").split("\x1e"):
        parts = row.split("\x1f")
        if len(parts) < 7:
            continue
        if len(parts) >= 18:
            (
                element_id,
                parent_id,
                depth_raw,
                role,
                subrole,
                role_description,
                name,
                description,
                value_preview,
                help_text,
                enabled_raw,
                focused_raw,
                selected_raw,
                expanded_raw,
                x_raw,
                y_raw,
                w_raw,
                h_raw,
            ) = parts[:18]
            label_source = "none"
            label = ""
            for candidate_source, candidate in [
                ("name", name),
                ("description", description),
                ("value", value_preview),
                ("roleDescription", role_description),
            ]:
                if candidate.strip():
                    label_source = candidate_source
                    label = candidate.strip()
                    break
            depth = parse_int(depth_raw)
        else:
            element_id, role, label, x_raw, y_raw, w_raw, h_raw = parts[:7]
            parent_id = ""
            depth = 0
            subrole = ""
            role_description = ""
            name = label
            description = ""
            value_preview = ""
            help_text = ""
            enabled_raw = focused_raw = selected_raw = expanded_raw = ""
            label_source = "legacy"
        bounds = [
            parse_float(x_raw),
            parse_float(y_raw),
            parse_float(w_raw),
            parse_float(h_raw),
        ]
        if bounds[2] <= 0 or bounds[3] <= 0:
            continue
        role = role or "unknown"
        kind = semantic_kind(role, subrole)
        enabled = parse_bool_text(enabled_raw)
        element = {
            "id": element_id,
            "role": role,
            "label": label,
            "bounds": bounds,
            "source": "macos_accessibility",
            "depth": depth,
            "kind": kind,
            "interactable": semantic_interactable(kind, enabled, role),
            "labelSource": label_source,
        }
        if parent_id:
            element["parentId"] = parent_id
        for key, value in [
            ("subrole", subrole),
            ("roleDescription", role_description),
            ("name", name),
            ("description", description),
            ("valuePreview", value_preview),
            ("help", help_text),
        ]:
            if value.strip():
                element[key] = value.strip()
        for key, value in [
            ("enabled", enabled),
            ("focused", parse_bool_text(focused_raw)),
            ("selected", parse_bool_text(selected_raw)),
            ("expanded", parse_bool_text(expanded_raw)),
        ]:
            if value is not None:
                element[key] = value
        elements.append(element)
    return elements


def query_accessibility_elements() -> tuple[list[dict[str, Any]], str | None]:
    script = f'''
property rowDelim : character id 30
property colDelim : character id 31
property maxItems : {MAX_AX_ELEMENTS}
property maxDepth : {MAX_AX_DEPTH}
property maxTextChars : {MAX_AX_TEXT_CHARS}
property rows : {{}}

on cleanedText(rawValue)
  try
    set textValue to rawValue as text
  on error
    return ""
  end try
  set oldDelimiters to AppleScript's text item delimiters
  set AppleScript's text item delimiters to rowDelim
  set textValue to text items of textValue as text
  set AppleScript's text item delimiters to colDelim
  set textValue to text items of textValue as text
  set AppleScript's text item delimiters to oldDelimiters
  return textValue
end cleanedText

on boundedText(rawValue)
  global maxTextChars
  set textValue to my cleanedText(rawValue)
  if (length of textValue) > maxTextChars then
    return (text 1 thru maxTextChars of textValue) & "…"
  end if
  return textValue
end boundedText

on elementRole(axElement)
  try
    tell application "System Events" to set roleValue to role of axElement
    return my boundedText(roleValue)
  on error
    return "unknown"
  end try
end elementRole

on elementSubrole(axElement)
  try
    tell application "System Events" to set subroleValue to subrole of axElement
    return my boundedText(subroleValue)
  on error
    return ""
  end try
end elementSubrole

on elementRoleDescription(axElement)
  try
    tell application "System Events" to set roleDescriptionValue to role description of axElement
    return my boundedText(roleDescriptionValue)
  on error
    return ""
  end try
end elementRoleDescription

on elementName(axElement)
  try
    tell application "System Events" to set nameValue to name of axElement
    return my boundedText(nameValue)
  on error
    return ""
  end try
end elementName

on elementDescription(axElement)
  try
    tell application "System Events" to set descriptionValue to description of axElement
    return my boundedText(descriptionValue)
  on error
    return ""
  end try
end elementDescription

on elementValuePreview(axElement)
  try
    tell application "System Events" to set rawValue to value of axElement
    return my boundedText(rawValue)
  on error
    return ""
  end try
end elementValuePreview

on elementHelp(axElement)
  try
    tell application "System Events" to set helpValue to help of axElement
    return my boundedText(helpValue)
  on error
    return ""
  end try
end elementHelp

on boolText(rawValue)
  try
    if rawValue then return "true"
    return "false"
  on error
    return ""
  end try
end boolText

on elementEnabled(axElement)
  try
    tell application "System Events" to set enabledValue to enabled of axElement
    return my boolText(enabledValue)
  on error
    return ""
  end try
end elementEnabled

on elementFocused(axElement)
  try
    tell application "System Events" to set focusedValue to focused of axElement
    return my boolText(focusedValue)
  on error
    return ""
  end try
end elementFocused

on elementSelected(axElement)
  try
    tell application "System Events" to set selectedValue to selected of axElement
    return my boolText(selectedValue)
  on error
    return ""
  end try
end elementSelected

on elementExpanded(axElement)
  return ""
end elementExpanded

on appendElement(axElement, elementId, parentId, depth)
  global rows, rowDelim, colDelim, maxItems, maxDepth
  if (count of rows) is greater than or equal to maxItems then return
  set hasBounds to false
  set x to 0
  set y to 0
  set w to 0
  set h to 0
  try
    tell application "System Events"
      set posValue to position of axElement
      set sizeValue to size of axElement
    end tell
    set x to item 1 of posValue
    set y to item 2 of posValue
    set w to item 1 of sizeValue
    set h to item 2 of sizeValue
    set hasBounds to true
  end try
  if hasBounds and w > 0 and h > 0 then
    set rowText to elementId & colDelim & parentId & colDelim & (depth as text) & colDelim & (my elementRole(axElement)) & colDelim & (my elementSubrole(axElement)) & colDelim & (my elementRoleDescription(axElement)) & colDelim & (my elementName(axElement)) & colDelim & (my elementDescription(axElement)) & colDelim & (my elementValuePreview(axElement)) & colDelim & (my elementHelp(axElement)) & colDelim & (my elementEnabled(axElement)) & colDelim & (my elementFocused(axElement)) & colDelim & (my elementSelected(axElement)) & colDelim & (my elementExpanded(axElement)) & colDelim & (x as text) & colDelim & (y as text) & colDelim & (w as text) & colDelim & (h as text)
    set end of rows to rowText
  end if
  if depth is greater than or equal to maxDepth then return
  try
    tell application "System Events" to set childElements to UI elements of axElement
  on error
    return
  end try
  set childIndex to 0
  repeat with childElement in childElements
    if (count of rows) is greater than or equal to maxItems then exit repeat
    set childIndex to childIndex + 1
    my appendElement(childElement, elementId & "." & childIndex, elementId, depth + 1)
  end repeat
end appendElement

tell application "System Events"
  set frontProc to first application process whose frontmost is true
  if (count of windows of frontProc) > 0 then
    set frontWindow to window 1 of frontProc
    my appendElement(frontWindow, "active-window", "", 0)
  end if
end tell

set oldDelimiters to AppleScript's text item delimiters
set AppleScript's text item delimiters to rowDelim
set outputText to rows as text
set AppleScript's text item delimiters to oldDelimiters
return outputText
'''.strip()
    try:
        result = run_osascript(script, timeout=8.0)
    except subprocess.TimeoutExpired:
        return [], "accessibility element query timed out"
    if result.returncode != 0:
        return [], permission_error(result.stderr) or result.stderr.strip() or "accessibility element query failed"
    return parse_ax_element_rows(result.stdout), None


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


def capture_screenshot_region(
    run_id: str,
    observation_id: str,
    bounds: list[Any],
) -> tuple[str | None, str | None]:
    if len(bounds) != 4:
        return capture_screenshot(run_id, observation_id)
    try:
        x = max(0, round(float(bounds[0])))
        y = max(0, round(float(bounds[1])))
        width = max(1, round(float(bounds[2])))
        height = max(1, round(float(bounds[3])))
    except (TypeError, ValueError):
        return capture_screenshot(run_id, observation_id)
    root = Path(tempfile.gettempdir()) / "omiga-computer-use" / run_id
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{observation_id}.png"
    rect = f"{x},{y},{width},{height}"
    result = run_command(["screencapture", "-x", "-t", "png", "-R", rect, str(path)], timeout=8.0)
    if result.returncode != 0 or not path.exists():
        if path.exists():
            try:
                path.unlink()
            except OSError:
                pass
        return None, permission_error(result.stderr) or result.stderr.strip() or "screencapture region failed"
    return str(path), None


def vision_ocr_script_path() -> Path:
    path = Path(tempfile.gettempdir()) / "omiga-computer-use-vision-ocr.swift"
    try:
        if not path.exists() or path.read_text(encoding="utf-8") != VISION_OCR_SWIFT:
            path.write_text(VISION_OCR_SWIFT, encoding="utf-8")
    except OSError:
        # Let the caller surface the Swift execution/write failure as a
        # structured visualTextError; observe must remain fail-soft.
        pass
    return path


def clean_visual_text_item(item: Any) -> dict[str, Any] | None:
    if not isinstance(item, dict):
        return None
    text = str(item.get("text") or "").replace("\n", " ").strip()
    if not text:
        return None
    if len(text) > MAX_VISUAL_TEXT_CHARS:
        text = text[:MAX_VISUAL_TEXT_CHARS] + "…"
    bounds_raw = item.get("bounds")
    bounds: list[float] = []
    if isinstance(bounds_raw, list):
        for value in bounds_raw[:4]:
            try:
                bounds.append(float(value))
            except (TypeError, ValueError):
                bounds.append(0.0)
    while len(bounds) < 4:
        bounds.append(0.0)
    try:
        confidence = float(item.get("confidence", 0.0))
    except (TypeError, ValueError):
        confidence = 0.0
    return {
        "text": text,
        "confidence": max(0.0, min(1.0, confidence)),
        "bounds": bounds,
        "source": str(item.get("source") or "macos_vision_ocr"),
    }


def extract_visual_text(image_path: str) -> tuple[list[dict[str, Any]], str | None]:
    if not IS_DARWIN:
        return [], "visual_text_ocr_requires_macos"
    swift = shutil.which("swift") or "/usr/bin/swift"
    if not Path(swift).exists() and shutil.which(swift) is None:
        return [], "swift_runtime_unavailable_for_visual_text_ocr"
    script_path = vision_ocr_script_path()
    try:
        result = run_command(
            [
                swift,
                str(script_path),
                image_path,
                str(MAX_VISUAL_TEXT_ITEMS),
                str(MAX_VISUAL_TEXT_CHARS),
            ],
            timeout=VISION_OCR_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired:
        return [], "vision_ocr_timed_out"
    except OSError as error:
        return [], f"vision_ocr_unavailable: {error}"
    if result.returncode != 0:
        message = permission_error(result.stderr) or result.stderr.strip() or result.stdout.strip()
        return [], message or "vision_ocr_failed"
    try:
        raw_items = json.loads(result.stdout or "[]")
    except json.JSONDecodeError as error:
        return [], f"vision_ocr_returned_invalid_json: {error}"
    if not isinstance(raw_items, list):
        return [], "vision_ocr_returned_unexpected_payload"
    items: list[dict[str, Any]] = []
    for item in raw_items[:MAX_VISUAL_TEXT_ITEMS]:
        cleaned = clean_visual_text_item(item)
        if cleaned:
            items.append(cleaned)
    return items, None


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


def activation_target_is_allowed(args: dict[str, Any], app_name: str, bundle_id: str) -> bool:
    """Conservatively allow only the exact identifier used for activation.

    `mac_set_target` prefers bundleId when present, so a model-provided
    allowlisted appName must not be able to smuggle a different bundleId into
    the activation call.
    """
    allowed = allowed_apps(args)
    if "*" in allowed:
        return True
    if bundle_id.strip():
        return bundle_id.strip().lower() in allowed
    if app_name.strip():
        return app_name.strip().lower() in allowed
    return False


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
    extract_visual_text_requested = bool_arg(args, "extractVisualText", False)
    if screenshot or extract_visual_text_requested:
        if extract_visual_text_requested:
            screenshot_path, screenshot_error = capture_screenshot_region(
                run_id,
                observation_id,
                window["bounds"],
            )
        else:
            screenshot_path, screenshot_error = capture_screenshot(run_id, observation_id)
    visual_text: list[dict[str, Any]] = []
    visual_text_error = None
    if extract_visual_text_requested:
        if screenshot_path:
            visual_text, visual_text_error = extract_visual_text(screenshot_path)
        else:
            visual_text_error = screenshot_error or "screenshot_required_for_visual_text"
    screen_size = desktop_screen_size()
    target = {
        "appName": window["appName"],
        "bundleId": window["bundleId"],
        "windowTitle": window["title"],
        "pid": window["pid"],
        "windowId": window["windowId"],
        "bounds": window["bounds"],
    }
    elements, elements_error = query_accessibility_elements() if window["hasWindow"] else ([], None)
    if window["hasWindow"] and not any(item.get("id") == "active-window" for item in elements):
        elements.insert(
            0,
            {
                "id": "active-window",
                "role": "window",
                "label": window["title"] or window["appName"],
                "bounds": window["bounds"],
                "source": "frontmost_window",
                "depth": 0,
                "kind": "window",
                "interactable": True,
                "labelSource": "windowTitle" if window["title"] else "appName",
                "roleDescription": "window",
                "name": window["title"] or window["appName"],
            },
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
        "elementCount": len(elements),
        "accessibilityElementDepth": MAX_AX_DEPTH,
        "accessibilityTextLimit": MAX_AX_TEXT_CHARS,
        "observedAt": now_ms(),
    }
    if screenshot_error:
        result["screenshotError"] = screenshot_error
        result["screenRecordingMayBeRequired"] = True
    if extract_visual_text_requested:
        result["visualTextRequested"] = True
        result["visualText"] = visual_text
        result["visualTextCount"] = len(visual_text)
        result["visualTextLimit"] = MAX_VISUAL_TEXT_ITEMS
        result["visualTextTextLimit"] = MAX_VISUAL_TEXT_CHARS
        result["visualTextSource"] = "macos_vision_ocr"
        if visual_text_error:
            result["visualTextError"] = visual_text_error
            result["visualTextRequiresPermission"] = bool(permission_error(visual_text_error))
    if elements_error:
        result["accessibilityElementsError"] = elements_error
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
    window_title = string_arg(args, "windowTitle")
    if bundle_id:
        if not activation_target_is_allowed(args, app_name, bundle_id):
            return app_not_allowed_result(args, app_name, bundle_id, window_title)
        script = f"tell application id {applescript_quote(bundle_id)} to activate"
    elif app_name:
        if not activation_target_is_allowed(args, app_name, bundle_id):
            return app_not_allowed_result(args, app_name, bundle_id, window_title)
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


def target_bounds_from_validation(validation: dict[str, Any]) -> tuple[float, float, float, float] | None:
    target = validation.get("currentTarget")
    if not isinstance(target, dict):
        target = validation.get("target")
    if not isinstance(target, dict):
        return None
    bounds = target.get("bounds")
    if not (isinstance(bounds, list) and len(bounds) == 4):
        return None
    try:
        x, y, width, height = [float(value) for value in bounds]
    except (TypeError, ValueError):
        return None
    return x, y, width, height


def point_inside_bounds(x: float, y: float, bounds: tuple[float, float, float, float]) -> bool:
    bx, by, width, height = bounds
    return bx <= x <= bx + width and by <= y <= by + height


def bounded_drag_duration_ms(args: dict[str, Any]) -> int:
    value = args.get("durationMs", 350)
    try:
        duration = int(value)
    except (TypeError, ValueError):
        duration = 350
    return max(50, min(2000, duration))


def run_drag_event(
    start_x: float,
    start_y: float,
    end_x: float,
    end_y: float,
    duration_ms: int,
) -> tuple[bool, str | None]:
    try:
        class CGPoint(ctypes.Structure):
            _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]

        app_services = ctypes.CDLL(
            "/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices"
        )
        app_services.CGEventCreateMouseEvent.restype = ctypes.c_void_p
        app_services.CGEventCreateMouseEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint32,
            CGPoint,
            ctypes.c_uint32,
        ]
        app_services.CGEventCreate.restype = ctypes.c_void_p
        app_services.CGEventCreate.argtypes = [ctypes.c_void_p]
        app_services.CGEventGetLocation.restype = CGPoint
        app_services.CGEventGetLocation.argtypes = [ctypes.c_void_p]
        app_services.CGWarpMouseCursorPosition.argtypes = [CGPoint]
        app_services.CGWarpMouseCursorPosition.restype = ctypes.c_int32
        app_services.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
        app_services.CFRelease.argtypes = [ctypes.c_void_p]

        previous_event = app_services.CGEventCreate(None)
        previous_location = None
        if previous_event:
            previous_location = app_services.CGEventGetLocation(previous_event)
            app_services.CFRelease(previous_event)

        def post_mouse_event(event_type: int, point: CGPoint) -> str | None:
            event = app_services.CGEventCreateMouseEvent(None, event_type, point, 0)
            if not event:
                return "mouse_event_create_failed"
            try:
                app_services.CGEventPost(0, event)  # kCGHIDEventTap
            finally:
                app_services.CFRelease(event)
            return None

        start = CGPoint(float(start_x), float(start_y))
        end = CGPoint(float(end_x), float(end_y))
        steps = max(4, min(60, int(duration_ms / 16)))
        delay = duration_ms / steps / 1000.0
        try:
            app_services.CGWarpMouseCursorPosition(start)
            time.sleep(0.03)
            error = post_mouse_event(1, start)  # kCGEventLeftMouseDown
            if error:
                return False, error
            for step in range(1, steps + 1):
                ratio = step / steps
                point = CGPoint(
                    start.x + (end.x - start.x) * ratio,
                    start.y + (end.y - start.y) * ratio,
                )
                error = post_mouse_event(6, point)  # kCGEventLeftMouseDragged
                if error:
                    return False, error
                time.sleep(delay)
            error = post_mouse_event(2, end)  # kCGEventLeftMouseUp
            if error:
                return False, error
            time.sleep(0.15)
        finally:
            if previous_location is not None:
                app_services.CGWarpMouseCursorPosition(previous_location)
        return True, None
    except Exception as error:  # noqa: BLE001 - backend should return structured failure.
        return False, str(error)


def mac_drag(args: dict[str, Any]) -> dict[str, Any]:
    if string_arg(args, "button", "left").lower() not in {"", "left"}:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "drag",
            "error": "unsupported_button",
            "message": "Computer Use drag supports left button only.",
            "safeToAct": False,
        }
    try:
        start_x = float(args.get("startX"))
        start_y = float(args.get("startY"))
        end_x = float(args.get("endX"))
        end_y = float(args.get("endY"))
    except (TypeError, ValueError):
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "drag",
            "error": "missing_coordinate",
            "message": "drag requires numeric startX, startY, endX, and endY.",
            "safeToAct": False,
            "requiresObserve": True,
        }
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    bounds = target_bounds_from_validation(validation)
    if bounds is None:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "drag",
            "error": "target_bounds_missing",
            "message": "Drag requires current target bounds from validate_target.",
            "safeToAct": False,
            "requiresObserve": True,
        }
    if not (point_inside_bounds(start_x, start_y, bounds) and point_inside_bounds(end_x, end_y, bounds)):
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "drag",
            "error": "drag_point_outside_target_window",
            "message": "Drag start and end coordinates must stay inside the validated target bounds.",
            "safeToAct": False,
            "requiresObserve": True,
        }
    duration_ms = bounded_drag_duration_ms(args)
    ok, error = run_drag_event(start_x, start_y, end_x, end_y, duration_ms)
    return {
        "ok": ok,
        "backend": "computer-use-macos-mcp",
        "action": "drag",
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "startX": start_x,
        "startY": start_y,
        "endX": end_x,
        "endY": end_y,
        "durationMs": duration_ms,
        "button": "left",
        "error": error,
    }


def clipboard_get() -> str:
    result = run_command(["pbpaste"], timeout=3.0)
    return result.stdout if result.returncode == 0 else ""


def clipboard_set(text: str) -> None:
    run_command(["pbcopy"], input_text=text, timeout=3.0)


def text_looks_sensitive(text: str) -> bool:
    patterns = [
        r"(?is)-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----",
        r"sk-[A-Za-z0-9_-]{12,}",
        r"ghp_[A-Za-z0-9_]{12,}",
        r"AKIA[0-9A-Z]{16}",
        r"(?i)\b(password|token|api[_-]?key)\s*[:=]\s*[^,\s;]+",
    ]
    return any(re.search(pattern, text) for pattern in patterns)


def direct_type_supported(text: str) -> bool:
    if len(text) > MAX_DIRECT_TYPE_CHARS:
        return False
    return all(ch in {"\n", "\t"} or ord(ch) >= 32 for ch in text)


def direct_type_script(text: str) -> str | None:
    if not direct_type_supported(text):
        return None
    lines = ['tell application "System Events"']
    chunk: list[str] = []

    def flush_chunk() -> None:
        if chunk:
            lines.append(f"keystroke {applescript_quote(''.join(chunk))}")
            chunk.clear()

    for ch in text:
        if ch == "\n":
            flush_chunk()
            lines.append("key code 36")
        elif ch == "\t":
            flush_chunk()
            lines.append("key code 48")
        else:
            chunk.append(ch)
    flush_chunk()
    lines.append("end tell")
    return "\n".join(lines)


def run_direct_type(text: str) -> tuple[bool, str | None, str | None]:
    script = direct_type_script(text)
    if script is None:
        return False, "direct_type_unsupported_text", None
    result = run_osascript(script, timeout=8.0)
    if result.returncode == 0:
        return True, None, None
    return False, "direct_type_failed", permission_error(result.stderr) or "direct type failed"


def clipboard_fallback_allowed(args: dict[str, Any], text: str) -> bool:
    return bool_arg(args, "allowClipboardFallback", True) and not text_looks_sensitive(text)


def mac_type_text(args: dict[str, Any]) -> dict[str, Any]:
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    text = string_arg(args, "text")
    direct_ok, direct_code, direct_error = run_direct_type(text)
    if direct_ok:
        return {
            "ok": True,
            "backend": "computer-use-macos-mcp",
            "action": "type_text",
            "method": "direct_keystroke",
            "clipboardFallbackUsed": False,
            "observationId": args.get("observationId"),
            "targetWindowId": args.get("targetWindowId"),
            "typedChars": len(text),
            "error": None,
        }

    if not clipboard_fallback_allowed(args, text):
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "type_text",
            "method": "direct_keystroke",
            "clipboardFallbackUsed": False,
            "error": direct_code or "clipboard_fallback_disabled",
            "message": "Direct typing failed or was unsupported, and clipboard fallback is disabled for this input.",
            "directTypeError": direct_error,
            "requiresConfirmation": True,
            "safeToAct": False,
        }

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
        "clipboardFallbackUsed": True,
        "directTypeError": direct_error,
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "typedChars": len(text),
        "error": error,
    }


KEY_CODES: dict[str, int] = {
    "enter": 36,
    "return": 36,
    "tab": 48,
    "escape": 53,
    "esc": 53,
    "backspace": 51,
    "delete": 117,
    "arrow_left": 123,
    "left": 123,
    "arrow_right": 124,
    "right": 124,
    "arrow_down": 125,
    "down": 125,
    "arrow_up": 126,
    "up": 126,
    "page_up": 116,
    "page_down": 121,
    "home": 115,
    "end": 119,
    "space": 49,
}


def normalized_key_name(raw: str) -> str:
    return raw.strip().lower().replace("-", "_").replace(" ", "_")


def bounded_key_count(args: dict[str, Any]) -> int:
    value = args.get("count", 1)
    try:
        count = int(value)
    except (TypeError, ValueError):
        count = 1
    return max(1, min(20, count))


def run_key_press(key_code: int, count: int) -> tuple[bool, str | None]:
    script = (
        'tell application "System Events"\n'
        f"repeat {count} times\n"
        f"key code {key_code}\n"
        "delay 0.03\n"
        "end repeat\n"
        "end tell"
    )
    result = run_osascript(script, timeout=5.0)
    if result.returncode == 0:
        return True, None
    return False, permission_error(result.stderr) or result.stderr.strip() or "key press failed"


def mac_key_press(args: dict[str, Any]) -> dict[str, Any]:
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    key = normalized_key_name(string_arg(args, "key"))
    key_code = KEY_CODES.get(key)
    if key_code is None:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "key_press",
            "error": "unsupported_key",
            "message": "Unsupported key. Use one of enter, tab, escape, arrows, page_up/page_down, home/end, delete, backspace, or space.",
            "key": key,
            "safeToAct": False,
        }
    count = bounded_key_count(args)
    ok, error = run_key_press(key_code, count)
    return {
        "ok": ok,
        "backend": "computer-use-macos-mcp",
        "action": "key_press",
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "key": key,
        "count": count,
        "keyCode": key_code,
        "error": error,
    }


def normalized_scroll_direction(raw: str) -> str:
    return raw.strip().lower().replace("-", "_").replace(" ", "_")


def bounded_scroll_amount(args: dict[str, Any]) -> int:
    value = args.get("amount", 5)
    try:
        amount = int(value)
    except (TypeError, ValueError):
        amount = 5
    return max(1, min(20, amount))


def scroll_deltas(direction: str, amount: int) -> tuple[int, int] | None:
    if direction == "down":
        return 0, -amount
    if direction == "up":
        return 0, amount
    if direction == "right":
        return -amount, 0
    if direction == "left":
        return amount, 0
    return None


def target_center_from_validation(validation: dict[str, Any]) -> tuple[float, float] | None:
    target = validation.get("currentTarget")
    if not isinstance(target, dict):
        return None
    bounds = target.get("bounds")
    if not (isinstance(bounds, list) and len(bounds) == 4):
        return None
    try:
        x, y, width, height = [float(value) for value in bounds]
    except (TypeError, ValueError):
        return None
    return x + width / 2.0, y + height / 2.0


def run_scroll_event(delta_x: int, delta_y: int, x: float, y: float) -> tuple[bool, str | None]:
    try:
        class CGPoint(ctypes.Structure):
            _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]

        app_services = ctypes.CDLL(
            "/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices"
        )
        app_services.CGEventCreateScrollWheelEvent.restype = ctypes.c_void_p
        app_services.CGEventCreateScrollWheelEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.c_uint32,
            ctypes.c_int32,
            ctypes.c_int32,
        ]
        app_services.CGEventCreate.restype = ctypes.c_void_p
        app_services.CGEventCreate.argtypes = [ctypes.c_void_p]
        app_services.CGEventGetLocation.restype = CGPoint
        app_services.CGEventGetLocation.argtypes = [ctypes.c_void_p]
        app_services.CGEventSetLocation.argtypes = [ctypes.c_void_p, CGPoint]
        app_services.CGWarpMouseCursorPosition.argtypes = [CGPoint]
        app_services.CGWarpMouseCursorPosition.restype = ctypes.c_int32
        app_services.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
        app_services.CFRelease.argtypes = [ctypes.c_void_p]
        previous_event = app_services.CGEventCreate(None)
        previous_location = None
        if previous_event:
            previous_location = app_services.CGEventGetLocation(previous_event)
            app_services.CFRelease(previous_event)
        event = app_services.CGEventCreateScrollWheelEvent(
            None,
            1,  # kCGScrollEventUnitLine
            2,
            int(delta_y),
            int(delta_x),
        )
        if not event:
            return False, "scroll_event_create_failed"
        target_point = CGPoint(float(x), float(y))
        try:
            app_services.CGWarpMouseCursorPosition(target_point)
            time.sleep(0.03)
            app_services.CGEventSetLocation(event, target_point)
            app_services.CGEventPost(0, event)  # kCGHIDEventTap
            time.sleep(0.15)
        finally:
            app_services.CFRelease(event)
            if previous_location is not None:
                app_services.CGWarpMouseCursorPosition(previous_location)
        return True, None
    except Exception as error:  # noqa: BLE001 - backend should return structured failure.
        return False, str(error)


def mac_scroll(args: dict[str, Any]) -> dict[str, Any]:
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    direction = normalized_scroll_direction(string_arg(args, "direction"))
    amount = bounded_scroll_amount(args)
    deltas = scroll_deltas(direction, amount)
    if deltas is None:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "scroll",
            "error": "unsupported_scroll_direction",
            "message": "Unsupported scroll direction. Use up, down, left, or right.",
            "direction": direction,
            "safeToAct": False,
        }
    center = target_center_from_validation(validation)
    if center is None:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "scroll",
            "error": "target_bounds_missing",
            "message": "Scroll requires current target bounds from validate_target.",
            "safeToAct": False,
            "requiresObserve": True,
        }
    delta_x, delta_y = deltas
    x, y = center
    ok, error = run_scroll_event(delta_x, delta_y, x, y)
    return {
        "ok": ok,
        "backend": "computer-use-macos-mcp",
        "action": "scroll",
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "direction": direction,
        "amount": amount,
        "deltaX": delta_x,
        "deltaY": delta_y,
        "x": x,
        "y": y,
        "error": error,
    }


SHORTCUT_SCRIPTS: dict[str, str] = {
    "select_all": 'tell application "System Events" to keystroke "a" using command down',
    "undo": 'tell application "System Events" to keystroke "z" using command down',
    "redo": 'tell application "System Events" to keystroke "z" using {command down, shift down}',
}


def normalized_shortcut_name(raw: str) -> str:
    return raw.strip().lower().replace("-", "_").replace(" ", "_")


def run_shortcut(shortcut: str) -> tuple[bool, str | None]:
    script = SHORTCUT_SCRIPTS.get(shortcut)
    if script is None:
        return False, "unsupported_shortcut"
    result = run_osascript(script, timeout=5.0)
    if result.returncode == 0:
        return True, None
    return False, permission_error(result.stderr) or result.stderr.strip() or "shortcut failed"


def mac_shortcut(args: dict[str, Any]) -> dict[str, Any]:
    validation = validate_current_target(args)
    if not validation.get("ok"):
        return validation
    shortcut = normalized_shortcut_name(string_arg(args, "shortcut"))
    if shortcut not in SHORTCUT_SCRIPTS:
        return {
            "ok": False,
            "backend": "computer-use-macos-mcp",
            "action": "shortcut",
            "error": "unsupported_shortcut",
            "message": "Unsupported shortcut. Use select_all, undo, or redo.",
            "shortcut": shortcut,
            "safeToAct": False,
        }
    ok, error = run_shortcut(shortcut)
    return {
        "ok": ok,
        "backend": "computer-use-macos-mcp",
        "action": "shortcut",
        "observationId": args.get("observationId"),
        "targetWindowId": args.get("targetWindowId"),
        "shortcut": shortcut,
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
                    "source": "mock",
                    "depth": 0,
                    "kind": "window",
                    "interactable": True,
                    "labelSource": "name",
                    "name": "Mock Window",
                    "roleDescription": "window",
                },
                {
                    "id": "button-save",
                    "role": "button",
                    "label": "Save",
                    "bounds": [100, 100, 80, 32],
                    "source": "mock",
                    "parentId": "active-window",
                    "depth": 1,
                    "kind": "button",
                    "interactable": True,
                    "labelSource": "name",
                    "name": "Save",
                    "roleDescription": "button",
                    "enabled": True,
                },
            ],
            "elementCount": 2,
            "accessibilityElementDepth": MAX_AX_DEPTH,
            "accessibilityTextLimit": MAX_AX_TEXT_CHARS,
            "mockedAt": now_ms(),
        }
        if bool_arg(args, "extractVisualText", False):
            result.update(
                {
                    "visualTextRequested": True,
                    "visualText": [
                        {
                            "text": "Mock Window Save",
                            "confidence": 1.0,
                            "bounds": [90, 90, 140, 48],
                            "source": "mock_ocr",
                        }
                    ],
                    "visualTextCount": 1,
                    "visualTextLimit": MAX_VISUAL_TEXT_ITEMS,
                    "visualTextTextLimit": MAX_VISUAL_TEXT_CHARS,
                    "visualTextSource": "mock_ocr",
                }
            )
        OBSERVATIONS["obs_mock_1"] = result
        return result
    if tool_name == "set_target":
        app_name = str(args.get("appName") or "Omiga")
        requested_bundle_id = str(args.get("bundleId") or "")
        window_title = str(args.get("windowTitle") or "Mock Window")
        if not activation_target_is_allowed(args, app_name, requested_bundle_id):
            return app_not_allowed_result(args, app_name, requested_bundle_id, window_title, 1)
        bundle_id = requested_bundle_id or "com.omiga.desktop"
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
    if tool_name == "drag":
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "drag",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "startX": args.get("startX"),
            "startY": args.get("startY"),
            "endX": args.get("endX"),
            "endY": args.get("endY"),
            "durationMs": bounded_drag_duration_ms(args),
            "button": "left",
        }
    if tool_name == "type_text":
        text = string_arg(args, "text")
        if direct_type_supported(text):
            method = "direct_keystroke"
            clipboard_used = False
        elif clipboard_fallback_allowed(args, text):
            method = "controlled_clipboard_paste"
            clipboard_used = True
        else:
            return {
                "ok": False,
                "backend": "computer-use-mock-mcp",
                "action": "type_text",
                "method": "direct_keystroke",
                "clipboardFallbackUsed": False,
                "error": "direct_type_unsupported_text",
                "message": "Direct typing is unsupported and clipboard fallback is disabled for this input.",
                "typedChars": len(text),
            }
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "type_text",
            "method": method,
            "clipboardFallbackUsed": clipboard_used,
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "typedChars": len(text),
        }
    if tool_name == "key_press":
        key = normalized_key_name(string_arg(args, "key"))
        key_code = KEY_CODES.get(key)
        if key_code is None:
            return {
                "ok": False,
                "backend": "computer-use-mock-mcp",
                "action": "key_press",
                "error": "unsupported_key",
                "key": key,
                "safeToAct": False,
            }
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "key_press",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "key": key,
            "count": bounded_key_count(args),
            "keyCode": key_code,
        }
    if tool_name == "scroll":
        direction = normalized_scroll_direction(string_arg(args, "direction"))
        amount = bounded_scroll_amount(args)
        deltas = scroll_deltas(direction, amount)
        if deltas is None:
            return {
                "ok": False,
                "backend": "computer-use-mock-mcp",
                "action": "scroll",
                "error": "unsupported_scroll_direction",
                "direction": direction,
                "safeToAct": False,
            }
        delta_x, delta_y = deltas
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "scroll",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "direction": direction,
            "amount": amount,
            "deltaX": delta_x,
            "deltaY": delta_y,
        }
    if tool_name == "shortcut":
        shortcut = normalized_shortcut_name(string_arg(args, "shortcut"))
        if shortcut not in SHORTCUT_SCRIPTS:
            return {
                "ok": False,
                "backend": "computer-use-mock-mcp",
                "action": "shortcut",
                "error": "unsupported_shortcut",
                "shortcut": shortcut,
                "safeToAct": False,
            }
        return {
            "ok": True,
            "backend": "computer-use-mock-mcp",
            "action": "shortcut",
            "observationId": observation_id,
            "targetWindowId": target_window_id,
            "shortcut": shortcut,
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
    if tool_name == "drag":
        return mac_drag(args)
    if tool_name == "type_text":
        return mac_type_text(args)
    if tool_name == "key_press":
        return mac_key_press(args)
    if tool_name == "scroll":
        return mac_scroll(args)
    if tool_name == "shortcut":
        return mac_shortcut(args)
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
