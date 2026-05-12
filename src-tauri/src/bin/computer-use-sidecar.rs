//! Rust MCP sidecar MVP for Omiga Computer Use.
//!
//! The Python sidecar remains the user-facing default backend, while this
//! opt-in Rust sidecar mirrors the same MCP protocol for internal parity checks.
//! Real macOS mode observes bounded Accessibility elements, validates the
//! frontmost target before actions, and supports left click/click_element,
//! direct-first typing with controlled clipboard fallback, fixed non-text key
//! presses, fixed scroll-wheel events, left-button drag gestures, and a small
//! shortcut allowlist.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::c_void;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROTOCOL_VERSION: &str = "2024-11-05";
const MAX_DIRECT_TYPE_CHARS: usize = 512;
const MAX_AX_ELEMENTS: usize = 80;
const MAX_AX_DEPTH: usize = 4;
const MAX_AX_TEXT_CHARS: usize = 120;
const MAX_VISUAL_TEXT_ITEMS: usize = 40;
const MAX_VISUAL_TEXT_CHARS: usize = 120;
const BACKEND_RUST_MOCK: &str = "computer-use-rust-mock-mcp";
const BACKEND_RUST_MACOS: &str = "computer-use-rust-macos-mcp";
const VISION_OCR_SWIFT: &str = r#"
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
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackendMode {
    Mock,
    Real,
    Auto,
}

impl BackendMode {
    fn from_env() -> Self {
        match env::var("OMIGA_COMPUTER_USE_BACKEND")
            .unwrap_or_else(|_| "auto".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "mock" | "test" => Self::Mock,
            "real" | "macos" | "mac" => Self::Real,
            _ => Self::Auto,
        }
    }

    fn uses_real_backend(self) -> bool {
        match self {
            Self::Mock => false,
            Self::Real => true,
            Self::Auto => cfg!(target_os = "macos"),
        }
    }
}

#[derive(Clone, Debug)]
struct MacWindowInfo {
    app_name: String,
    bundle_id: String,
    pid: i64,
    window_title: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    window_id: String,
}

impl MacWindowInfo {
    fn has_visible_window(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    fn bounds_json(&self) -> Value {
        json!([self.x, self.y, self.width, self.height])
    }

    fn target_json(&self) -> Value {
        json!({
            "appName": self.app_name.clone(),
            "bundleId": self.bundle_id.clone(),
            "windowTitle": self.window_title.clone(),
            "pid": self.pid,
            "windowId": self.window_id.clone(),
            "bounds": self.bounds_json(),
        })
    }
}

fn main() {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut stopped_runs = HashSet::new();
    let mut observations = HashMap::new();

    loop {
        let request = match read_message(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(error) => {
                let _ = write_message(&json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": error.to_string()},
                }));
                return;
            }
        };

        if let Some(response) = handle_request(request, &mut stopped_runs, &mut observations) {
            if write_message(&response).is_err() {
                return;
            }
        }
    }
}

fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    if length == 0 {
        return Ok(None);
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice::<Value>(&body)
        .map(Some)
        .map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid json: {error}"))
        })
}

fn write_message(payload: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    let mut stdout = io::stdout().lock();
    write!(stdout, "Content-Length: {}\r\n\r\n", body.len())?;
    stdout.write_all(&body)?;
    stdout.flush()
}

fn handle_request(
    request: Value,
    stopped_runs: &mut HashSet<String>,
    observations: &mut HashMap<String, Value>,
) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "computer-use", "version": "0.4.0-rust-sidecar"},
            },
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"tools": tools_list()},
        })),
        "tools/call" => {
            let params = request.get("params").and_then(Value::as_object);
            let name = params
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = params
                .and_then(|params| params.get("arguments"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let result = tool_result(name, &args, stopped_runs, observations);
            let is_error = !result.get("ok").and_then(Value::as_bool).unwrap_or(false);
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{"type": "text", "text": result.to_string()}],
                    "isError": is_error,
                },
            }))
        }
        method if method.starts_with("notifications/") => None,
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {method}")},
        })),
    }
}

fn tools_list() -> Value {
    json!([
        tool_schema("observe", "Observe the current local UI target and capture metadata.", json!({}), vec![]),
        tool_schema("set_target", "Activate a target app/window by app name or bundle id.", json!({}), vec![]),
        tool_schema("validate_target", "Validate that the frontmost app/window still matches a prior observation.", json!({}), vec![]),
        tool_schema("click", "Click a coordinate after target validation.", json!({"x": {"type": "number"}, "y": {"type": "number"}}), vec!["x", "y"]),
        tool_schema("click_element", "Click an observed element by id after target validation.", json!({"elementId": {"type": "string"}}), vec!["elementId"]),
        tool_schema("drag", "Drag between two coordinates inside the validated target with a fixed left-button mouse gesture.", json!({"startX": {"type": "number"}, "startY": {"type": "number"}, "endX": {"type": "number"}, "endY": {"type": "number"}, "durationMs": {"type": "integer", "minimum": 50, "maximum": 2000}, "button": {"type": "string", "enum": ["left"]}}), vec!["startX", "startY", "endX", "endY"]),
        tool_schema("type_text", "Type text into the validated target; direct keystrokes first, controlled clipboard fallback only when allowed.", json!({"text": {"type": "string"}}), vec!["text"]),
        tool_schema("key_press", "Press a supported non-text key after target validation.", json!({"key": {"type": "string"}, "count": {"type": "integer", "minimum": 1, "maximum": 20}}), vec!["key"]),
        tool_schema("scroll", "Scroll inside the validated target with a fixed system scroll-wheel event.", json!({"direction": {"type": "string"}, "amount": {"type": "integer", "minimum": 1, "maximum": 20}}), vec!["direction"]),
        tool_schema("shortcut", "Run a fixed whitelisted keyboard shortcut after target validation.", json!({"shortcut": {"type": "string", "enum": ["select_all", "undo", "redo"]}}), vec!["shortcut"]),
        tool_schema("stop", "Stop Computer Use actions for a run.", json!({}), vec![]),
    ])
}

fn tool_schema(name: &str, description: &str, properties: Value, required: Vec<&str>) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": true,
        },
    })
}

fn tool_result(
    name: &str,
    args: &Map<String, Value>,
    stopped_runs: &mut HashSet<String>,
    observations: &mut HashMap<String, Value>,
) -> Value {
    if BackendMode::from_env().uses_real_backend() {
        return real_tool_result(name, args, stopped_runs, observations);
    }
    mock_tool_result(name, args, stopped_runs)
}

fn mock_tool_result(
    name: &str,
    args: &Map<String, Value>,
    stopped_runs: &mut HashSet<String>,
) -> Value {
    let run_id = string_arg(args, "runId")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "rust_mock_run".to_string());
    if name != "stop" && stopped_runs.contains(&run_id) {
        return run_stopped_result(BACKEND_RUST_MOCK);
    }

    if name != "stop" && !target_is_allowed(args, "Omiga", "com.omiga.desktop") {
        return app_not_allowed_for_backend(
            BACKEND_RUST_MOCK,
            args,
            "Omiga",
            "com.omiga.desktop",
            "Mock Window",
            Some(json!(1)),
        );
    }

    match name {
        "observe" => observe_result(args),
        "set_target" => set_target_result(args),
        "validate_target" => validate_target_result(args),
        "click" => json!({
            "ok": true,
            "backend": BACKEND_RUST_MOCK,
            "action": "click",
            "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
            "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
            "x": value_arg(args, "x"),
            "y": value_arg(args, "y"),
            "button": string_arg(args, "button").unwrap_or_else(|| "left".to_string()),
        }),
        "click_element" => json!({
            "ok": true,
            "backend": BACKEND_RUST_MOCK,
            "action": "click_element",
            "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
            "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
            "elementId": value_arg(args, "elementId"),
        }),
        "drag" => drag_result(args),
        "type_text" => type_text_result(args),
        "key_press" => key_press_result(args),
        "scroll" => scroll_result(args),
        "shortcut" => shortcut_result(args),
        "stop" => {
            stopped_runs.insert(run_id);
            json!({
                "ok": true,
                "backend": BACKEND_RUST_MOCK,
                "action": "stop",
                "reason": string_arg(args, "reason").unwrap_or_else(|| "requested".to_string()),
            })
        }
        other => unknown_tool_result(BACKEND_RUST_MOCK, other),
    }
}

fn real_tool_result(
    name: &str,
    args: &Map<String, Value>,
    stopped_runs: &mut HashSet<String>,
    observations: &mut HashMap<String, Value>,
) -> Value {
    if !cfg!(target_os = "macos") {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "unsupported_platform",
            "message": "The Rust real Computer Use backend currently supports macOS only.",
            "safeToAct": false,
            "requiresPermission": false,
        });
    }

    let run_id = string_arg(args, "runId")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "rust_real_run".to_string());
    if name != "stop" && stopped_runs.contains(&run_id) {
        return run_stopped_result(BACKEND_RUST_MACOS);
    }

    match name {
        "observe" => {
            let result = real_observe_result(args);
            if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                if let Some(observation_id) = result.get("observationId").and_then(value_to_string)
                {
                    observations.insert(observation_id, result.clone());
                }
            }
            result
        }
        "set_target" => real_set_target_result(args),
        "validate_target" => real_validate_target_result(args),
        "click" => real_click_result(args),
        "click_element" => real_click_element_result(args, observations),
        "drag" => real_drag_result(args),
        "type_text" => real_type_text_result(args),
        "key_press" => real_key_press_result(args),
        "scroll" => real_scroll_result(args),
        "shortcut" => real_shortcut_result(args),
        "stop" => {
            stopped_runs.insert(run_id);
            json!({
                "ok": true,
                "backend": BACKEND_RUST_MACOS,
                "action": "stop",
                "reason": string_arg(args, "reason").unwrap_or_else(|| "requested".to_string()),
            })
        }
        other => unknown_tool_result(BACKEND_RUST_MACOS, other),
    }
}

fn observe_result(args: &Map<String, Value>) -> Value {
    let mut result = json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "observationId": "obs_rust_mock_1",
        "screenshotPath": Value::Null,
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
        "targetVisible": true,
        "occluded": false,
        "safeToAct": true,
        "elements": [
            {
                "id": "active-window",
                "role": "window",
                "label": "Mock Window",
                "bounds": [0, 0, 1400, 900],
                "source": "mock",
                "depth": 0,
                "kind": "window",
                "interactable": true,
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
                "interactable": true,
                "labelSource": "name",
                "name": "Save",
                "roleDescription": "button",
                "enabled": true,
            },
        ],
        "elementCount": 2,
        "accessibilityElementDepth": MAX_AX_DEPTH,
        "accessibilityTextLimit": MAX_AX_TEXT_CHARS,
        "mockedAt": now_ms(),
    });
    if bool_arg(args, "extractVisualText", false) {
        insert_object_value(&mut result, "visualTextRequested", json!(true));
        insert_object_value(
            &mut result,
            "visualText",
            json!([
                {
                    "text": "Mock Window Save",
                    "confidence": 1.0,
                    "bounds": [90, 90, 140, 48],
                    "source": "mock_ocr",
                }
            ]),
        );
        insert_object_value(&mut result, "visualTextCount", json!(1));
        insert_object_value(&mut result, "visualTextLimit", json!(MAX_VISUAL_TEXT_ITEMS));
        insert_object_value(
            &mut result,
            "visualTextTextLimit",
            json!(MAX_VISUAL_TEXT_CHARS),
        );
        insert_object_value(&mut result, "visualTextSource", json!("mock_ocr"));
    }
    result
}

fn set_target_result(args: &Map<String, Value>) -> Value {
    let app_name = string_arg(args, "appName").unwrap_or_else(|| "Omiga".to_string());
    let requested_bundle_id = string_arg(args, "bundleId").unwrap_or_default();
    let window_title = string_arg(args, "windowTitle").unwrap_or_else(|| "Mock Window".to_string());
    if !activation_target_is_allowed(args, &app_name, &requested_bundle_id) {
        return app_not_allowed_for_backend(
            BACKEND_RUST_MOCK,
            args,
            &app_name,
            &requested_bundle_id,
            &window_title,
            Some(json!(1)),
        );
    }
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "target": {
            "appName": app_name,
            "bundleId": if requested_bundle_id.is_empty() { "com.omiga.desktop" } else { &requested_bundle_id },
            "windowTitle": window_title,
            "windowId": 1,
        },
    })
}

fn validate_target_result(args: &Map<String, Value>) -> Value {
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
        "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        "target": {
            "appName": "Omiga",
            "bundleId": "com.omiga.desktop",
            "windowTitle": "Mock Window",
            "windowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        },
        "targetVisible": true,
        "occluded": false,
        "safeToAct": true,
    })
}

fn type_text_result(args: &Map<String, Value>) -> Value {
    let text = string_arg(args, "text").unwrap_or_default();
    let allow_clipboard = bool_arg(args, "allowClipboardFallback", true);
    let (method, clipboard_used) = if direct_type_supported(&text) {
        ("direct_keystroke", false)
    } else if allow_clipboard && !text_looks_sensitive(&text) {
        ("controlled_clipboard_paste", true)
    } else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MOCK,
            "action": "type_text",
            "method": "direct_keystroke",
            "clipboardFallbackUsed": false,
            "error": "direct_type_unsupported_text",
            "message": "Direct typing is unsupported and clipboard fallback is disabled for this input.",
            "typedChars": text.chars().count(),
            "safeToAct": false,
        });
    };
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "action": "type_text",
        "method": method,
        "clipboardFallbackUsed": clipboard_used,
        "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
        "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        "typedChars": text.chars().count(),
    })
}

fn key_press_result(args: &Map<String, Value>) -> Value {
    let key = normalize_key_name(&string_arg(args, "key").unwrap_or_default());
    let Some(key_code) = key_code_for_name(&key) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MOCK,
            "action": "key_press",
            "error": "unsupported_key",
            "key": key,
            "safeToAct": false,
        });
    };
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "action": "key_press",
        "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
        "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        "key": key,
        "count": bounded_key_count(args),
        "keyCode": key_code,
    })
}

fn scroll_result(args: &Map<String, Value>) -> Value {
    let direction = normalize_scroll_direction(&string_arg(args, "direction").unwrap_or_default());
    let amount = bounded_scroll_amount(args);
    let Some((delta_x, delta_y)) = scroll_deltas(&direction, amount) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MOCK,
            "action": "scroll",
            "error": "unsupported_scroll_direction",
            "direction": direction,
            "safeToAct": false,
        });
    };
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "action": "scroll",
        "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
        "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        "direction": direction,
        "amount": amount,
        "deltaX": delta_x,
        "deltaY": delta_y,
    })
}

fn drag_result(args: &Map<String, Value>) -> Value {
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "action": "drag",
        "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
        "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        "startX": value_arg(args, "startX"),
        "startY": value_arg(args, "startY"),
        "endX": value_arg(args, "endX"),
        "endY": value_arg(args, "endY"),
        "durationMs": bounded_drag_duration_ms(args),
        "button": "left",
    })
}

fn shortcut_result(args: &Map<String, Value>) -> Value {
    let shortcut = normalize_shortcut_name(&string_arg(args, "shortcut").unwrap_or_default());
    if !shortcut_supported(&shortcut) {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MOCK,
            "action": "shortcut",
            "error": "unsupported_shortcut",
            "shortcut": shortcut,
            "safeToAct": false,
        });
    }
    json!({
        "ok": true,
        "backend": BACKEND_RUST_MOCK,
        "action": "shortcut",
        "observationId": value_arg(args, "observationId").unwrap_or_else(|| json!("obs_rust_mock_1")),
        "targetWindowId": value_arg(args, "targetWindowId").unwrap_or_else(|| json!(1)),
        "shortcut": shortcut,
    })
}

fn real_observe_result(args: &Map<String, Value>) -> Value {
    let run_id = string_arg(args, "runId")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "default".to_string());
    let observation_id = format!("obs_mac_{}_{}", now_ms(), uuid::Uuid::new_v4());
    let info = match query_frontmost_window() {
        Ok(info) => info,
        Err(error) => {
            let message = permission_error_message(&error)
                .map(str::to_string)
                .unwrap_or_else(|| error.clone());
            return json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "error": "macos_permission_or_window_query_failed",
                "message": message,
                "requiresPermission": is_permission_error(&error),
                "safeToAct": false,
                "observationId": observation_id,
            });
        }
    };

    if !target_is_allowed(args, &info.app_name, &info.bundle_id) {
        return app_not_allowed_for_backend(
            BACKEND_RUST_MACOS,
            args,
            &info.app_name,
            &info.bundle_id,
            &info.window_title,
            Some(json!(info.window_id)),
        );
    }

    let target_visible = info.has_visible_window();
    let (mut elements, elements_error) = if target_visible {
        match query_accessibility_elements() {
            Ok(elements) => (elements, None),
            Err(error) => {
                let message = permission_error_message(&error)
                    .map(str::to_string)
                    .unwrap_or(error);
                (Vec::new(), Some(message))
            }
        }
    } else {
        (Vec::new(), None)
    };
    if target_visible
        && !elements
            .iter()
            .any(|item| item.get("id").and_then(Value::as_str) == Some("active-window"))
    {
        elements.insert(
            0,
            json!({
                "id": "active-window",
                "role": "window",
                "label": info.window_title.clone(),
                "bounds": info.bounds_json(),
                "source": "frontmost_window",
                "depth": 0,
                "kind": "window",
                "interactable": true,
                "labelSource": if info.window_title.is_empty() { "appName" } else { "windowTitle" },
                "roleDescription": "window",
                "name": if info.window_title.is_empty() { info.app_name.clone() } else { info.window_title.clone() },
            }),
        );
    }
    let element_count = elements.len();
    let extract_visual_text_requested = bool_arg(args, "extractVisualText", false);
    let should_capture_screenshot =
        bool_arg(args, "saveScreenshot", false) || extract_visual_text_requested;
    let (screenshot_path, screenshot_error, screenshot_requires_permission) =
        if should_capture_screenshot {
            let (path, error) = if extract_visual_text_requested {
                capture_screenshot_region(
                    &run_id,
                    &observation_id,
                    info.x,
                    info.y,
                    info.width,
                    info.height,
                )
            } else {
                capture_screenshot(&run_id, &observation_id)
            };
            let requires_permission = error.as_deref().map(is_permission_error).unwrap_or(false);
            (path, error, requires_permission)
        } else {
            (None, None, false)
        };

    let mut result = json!({
        "ok": true,
        "backend": BACKEND_RUST_MACOS,
        "observationId": observation_id,
        "screenshotPath": screenshot_path.as_ref().map(|path| json!(path)).unwrap_or(Value::Null),
        "screenSize": desktop_screen_size().unwrap_or(Value::Null),
        "frontmostApp": info.app_name.clone(),
        "activeWindowTitle": info.window_title.clone(),
        "target": info.target_json(),
        "targetVisible": target_visible,
        "occluded": false,
        "safeToAct": target_visible,
        "elements": elements,
        "elementCount": element_count,
        "accessibilityElementDepth": MAX_AX_DEPTH,
        "accessibilityTextLimit": MAX_AX_TEXT_CHARS,
        "observedAt": now_ms(),
    });

    if let Some(error) = screenshot_error.clone() {
        insert_object_value(&mut result, "screenshotError", json!(error));
        insert_object_value(
            &mut result,
            "screenshotRequiresPermission",
            json!(screenshot_requires_permission),
        );
    }
    if extract_visual_text_requested {
        insert_object_value(&mut result, "visualTextRequested", json!(true));
        insert_object_value(&mut result, "visualTextLimit", json!(MAX_VISUAL_TEXT_ITEMS));
        insert_object_value(
            &mut result,
            "visualTextTextLimit",
            json!(MAX_VISUAL_TEXT_CHARS),
        );
        insert_object_value(&mut result, "visualTextSource", json!("macos_vision_ocr"));
        match screenshot_path.as_deref() {
            Some(path) => match run_visual_text_ocr(path) {
                Ok(items) => {
                    let count = items.len();
                    insert_object_value(&mut result, "visualText", Value::Array(items));
                    insert_object_value(&mut result, "visualTextCount", json!(count));
                }
                Err(error) => {
                    insert_object_value(&mut result, "visualText", json!([]));
                    insert_object_value(&mut result, "visualTextCount", json!(0));
                    insert_object_value(&mut result, "visualTextError", json!(error.clone()));
                    insert_object_value(
                        &mut result,
                        "visualTextRequiresPermission",
                        json!(is_permission_error(&error)),
                    );
                }
            },
            None => {
                let error = screenshot_error
                    .clone()
                    .unwrap_or_else(|| "screenshot_required_for_visual_text".to_string());
                insert_object_value(&mut result, "visualText", json!([]));
                insert_object_value(&mut result, "visualTextCount", json!(0));
                insert_object_value(&mut result, "visualTextError", json!(error.clone()));
                insert_object_value(
                    &mut result,
                    "visualTextRequiresPermission",
                    json!(is_permission_error(&error)),
                );
            }
        }
    }
    if let Some(error) = elements_error {
        insert_object_value(&mut result, "accessibilityElementsError", json!(error));
    }

    result
}

fn real_validate_target_result(args: &Map<String, Value>) -> Value {
    let expected_window_id = string_arg(args, "targetWindowId").unwrap_or_default();
    if expected_window_id.trim().is_empty() {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "missing_target_window_id",
            "message": "validate_target requires targetWindowId from a prior observation.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    let mut observe_args = args.clone();
    observe_args.insert("saveScreenshot".to_string(), json!(false));
    let mut observation = real_observe_result(&observe_args);
    if !observation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        insert_object_value(&mut observation, "requiresObserve", json!(true));
        insert_object_value(&mut observation, "safeToAct", json!(false));
        return observation;
    }

    let current_target = observation
        .get("target")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let current_window_id = current_target
        .get("windowId")
        .and_then(value_to_string)
        .unwrap_or_default();
    let observation_id = value_arg(args, "observationId")
        .or_else(|| observation.get("observationId").cloned())
        .unwrap_or_else(|| json!("obs_mac_unknown"));

    if current_window_id != expected_window_id {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "target_window_changed",
            "reason": "frontmost target no longer matches targetWindowId",
            "message": "Frontmost target no longer matches the observed targetWindowId.",
            "observationId": observation_id,
            "targetWindowId": expected_window_id,
            "expectedTargetWindowId": expected_window_id,
            "actualTargetWindowId": current_window_id,
            "currentTarget": current_target,
            "targetVisible": false,
            "occluded": true,
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    if let (Some(x), Some(y), Some((left, top, width, height))) = (
        number_arg(args, "x"),
        number_arg(args, "y"),
        bounds_from_target(&current_target),
    ) {
        if x < left || y < top || x > left + width || y > top + height {
            return json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "error": "point_outside_target_window",
                "message": "Requested coordinate is outside the currently validated target window.",
                "observationId": observation_id,
                "targetWindowId": expected_window_id,
                "currentTarget": current_target,
                "bounds": [left, top, width, height],
                "x": x,
                "y": y,
                "targetVisible": true,
                "occluded": false,
                "safeToAct": false,
                "requiresObserve": true,
            });
        }
    }

    json!({
        "ok": true,
        "backend": BACKEND_RUST_MACOS,
        "observationId": observation_id,
        "targetWindowId": expected_window_id,
        "target": current_target,
        "targetVisible": observation.get("targetVisible").cloned().unwrap_or_else(|| json!(true)),
        "occluded": false,
        "safeToAct": observation.get("safeToAct").cloned().unwrap_or_else(|| json!(true)),
    })
}

fn real_click_result(args: &Map<String, Value>) -> Value {
    let button = string_arg(args, "button")
        .unwrap_or_else(|| "left".to_string())
        .trim()
        .to_ascii_lowercase();
    if !button.is_empty() && button != "left" {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "unsupported_button",
            "message": "Rust Computer Use supports left click only.",
            "safeToAct": false,
        });
    }

    let Some(x) = number_arg(args, "x") else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "missing_coordinate",
            "message": "click requires numeric x.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    };
    let Some(y) = number_arg(args, "y") else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "missing_coordinate",
            "message": "click requires numeric y.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    };

    let validation = real_validate_target_result(args);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return validation;
    }

    match run_fixed_click(x, y) {
        Ok(()) => json!({
            "ok": true,
            "backend": BACKEND_RUST_MACOS,
            "action": "click",
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "x": x,
            "y": y,
            "button": "left",
        }),
        Err(error) => json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "click",
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "x": x,
            "y": y,
            "button": "left",
            "error": error,
            "message": error,
            "requiresPermission": is_permission_error(&error),
            "safeToAct": false,
            "requiresObserve": true,
        }),
    }
}

fn real_click_element_result(
    args: &Map<String, Value>,
    observations: &HashMap<String, Value>,
) -> Value {
    let observation_id = string_arg(args, "observationId").unwrap_or_default();
    let element_id = string_arg(args, "elementId").unwrap_or_default();
    if observation_id.trim().is_empty() || element_id.trim().is_empty() {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "missing_element_reference",
            "message": "click_element requires observationId and elementId.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    let Some(observation) = observations.get(&observation_id) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "element_not_found",
            "message": "Element id is not available in this sidecar process; observe again.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    };
    let Some((x, y)) = element_center(observation, &element_id) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "element_not_found",
            "message": "Element id is not available in this sidecar process; observe again.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    };

    let mut next_args = args.clone();
    next_args.insert("x".to_string(), json!(x));
    next_args.insert("y".to_string(), json!(y));
    let mut result = real_click_result(&next_args);
    insert_object_value(&mut result, "action", json!("click_element"));
    insert_object_value(&mut result, "elementId", json!(element_id));
    result
}

fn real_type_text_result(args: &Map<String, Value>) -> Value {
    let validation = real_validate_target_result(args);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return validation;
    }

    let text = string_arg(args, "text").unwrap_or_default();
    let (direct_ok, direct_code, direct_error) = run_direct_type(&text);
    if direct_ok {
        return json!({
            "ok": true,
            "backend": BACKEND_RUST_MACOS,
            "action": "type_text",
            "method": "direct_keystroke",
            "clipboardFallbackUsed": false,
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "typedChars": text.chars().count(),
            "error": Value::Null,
        });
    }

    if !clipboard_fallback_allowed(args, &text) {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "type_text",
            "method": "direct_keystroke",
            "clipboardFallbackUsed": false,
            "error": direct_code.unwrap_or("clipboard_fallback_disabled"),
            "message": "Direct typing failed or was unsupported, and clipboard fallback is disabled for this input.",
            "directTypeError": direct_error,
            "requiresConfirmation": true,
            "safeToAct": false,
        });
    }

    let previous_clipboard = clipboard_get();
    let mut restore_error = None;
    if let Err(error) = clipboard_set(&text) {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "type_text",
            "method": "controlled_clipboard_paste",
            "clipboardFallbackUsed": false,
            "directTypeError": direct_error,
            "error": "clipboard_set_failed",
            "message": error,
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    let paste_result =
        run_osascript(r#"tell application "System Events" to keystroke "v" using command down"#);
    if let Err(error) = clipboard_set(&previous_clipboard) {
        restore_error = Some(error);
    }

    match paste_result {
        Ok(_) => {
            let mut result = json!({
                "ok": true,
                "backend": BACKEND_RUST_MACOS,
                "action": "type_text",
                "method": "controlled_clipboard_paste",
                "clipboardFallbackUsed": true,
                "directTypeError": direct_error,
                "observationId": value_arg(args, "observationId"),
                "targetWindowId": value_arg(args, "targetWindowId"),
                "typedChars": text.chars().count(),
                "error": Value::Null,
            });
            if let Some(error) = restore_error {
                insert_object_value(&mut result, "clipboardRestoreError", json!(error));
            }
            result
        }
        Err(error) => {
            let error_message = error.clone();
            let mut result = json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "action": "type_text",
                "method": "controlled_clipboard_paste",
                "clipboardFallbackUsed": true,
                "directTypeError": direct_error,
                "observationId": value_arg(args, "observationId"),
                "targetWindowId": value_arg(args, "targetWindowId"),
                "typedChars": text.chars().count(),
                "error": error,
                "message": error_message,
                "requiresPermission": is_permission_error(&error_message),
                "safeToAct": false,
                "requiresObserve": true,
            });
            if let Some(error) = restore_error {
                insert_object_value(&mut result, "clipboardRestoreError", json!(error));
            }
            result
        }
    }
}

fn real_key_press_result(args: &Map<String, Value>) -> Value {
    let validation = real_validate_target_result(args);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return validation;
    }

    let key = normalize_key_name(&string_arg(args, "key").unwrap_or_default());
    let Some(key_code) = key_code_for_name(&key) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "key_press",
            "error": "unsupported_key",
            "message": "Unsupported key. Use one of enter, tab, escape, arrows, page_up/page_down, home/end, delete, backspace, or space.",
            "key": key,
            "safeToAct": false,
        });
    };
    let count = bounded_key_count(args);

    match run_key_press(key_code, count) {
        Ok(()) => json!({
            "ok": true,
            "backend": BACKEND_RUST_MACOS,
            "action": "key_press",
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "key": key,
            "count": count,
            "keyCode": key_code,
            "error": Value::Null,
        }),
        Err(error) => {
            let error_message = error.clone();
            json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "action": "key_press",
                "observationId": value_arg(args, "observationId"),
                "targetWindowId": value_arg(args, "targetWindowId"),
                "key": key,
                "count": count,
                "keyCode": key_code,
                "error": error,
                "message": error_message,
                "requiresPermission": is_permission_error(&error_message),
                "safeToAct": false,
                "requiresObserve": true,
            })
        }
    }
}

fn real_scroll_result(args: &Map<String, Value>) -> Value {
    let validation = real_validate_target_result(args);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return validation;
    }

    let direction = normalize_scroll_direction(&string_arg(args, "direction").unwrap_or_default());
    let amount = bounded_scroll_amount(args);
    let Some((delta_x, delta_y)) = scroll_deltas(&direction, amount) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "scroll",
            "error": "unsupported_scroll_direction",
            "message": "Unsupported scroll direction. Use up, down, left, or right.",
            "direction": direction,
            "safeToAct": false,
        });
    };
    let Some((x, y)) = target_center_from_validation(&validation) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "scroll",
            "error": "target_bounds_missing",
            "message": "Scroll requires current target bounds from validate_target.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    };

    match run_scroll_event(delta_x, delta_y, x, y) {
        Ok(()) => json!({
            "ok": true,
            "backend": BACKEND_RUST_MACOS,
            "action": "scroll",
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "direction": direction,
            "amount": amount,
            "deltaX": delta_x,
            "deltaY": delta_y,
            "x": x,
            "y": y,
            "error": Value::Null,
        }),
        Err(error) => {
            let error_message = error.clone();
            json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "action": "scroll",
                "observationId": value_arg(args, "observationId"),
                "targetWindowId": value_arg(args, "targetWindowId"),
                "direction": direction,
                "amount": amount,
                "deltaX": delta_x,
                "deltaY": delta_y,
                "x": x,
                "y": y,
                "error": error,
                "message": error_message,
                "requiresPermission": is_permission_error(&error_message),
                "safeToAct": false,
                "requiresObserve": true,
            })
        }
    }
}

fn real_drag_result(args: &Map<String, Value>) -> Value {
    let button = string_arg(args, "button")
        .unwrap_or_else(|| "left".to_string())
        .trim()
        .to_ascii_lowercase();
    if !button.is_empty() && button != "left" {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "drag",
            "error": "unsupported_button",
            "message": "Computer Use drag supports left button only.",
            "safeToAct": false,
        });
    }

    let Some(start_x) = number_arg(args, "startX") else {
        return missing_drag_coordinate_result();
    };
    let Some(start_y) = number_arg(args, "startY") else {
        return missing_drag_coordinate_result();
    };
    let Some(end_x) = number_arg(args, "endX") else {
        return missing_drag_coordinate_result();
    };
    let Some(end_y) = number_arg(args, "endY") else {
        return missing_drag_coordinate_result();
    };

    let validation = real_validate_target_result(args);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return validation;
    }

    let Some(bounds) = target_bounds_from_validation(&validation) else {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "drag",
            "error": "target_bounds_missing",
            "message": "Drag requires current target bounds from validate_target.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    };
    if !point_inside_bounds(start_x, start_y, bounds) || !point_inside_bounds(end_x, end_y, bounds)
    {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "drag",
            "error": "drag_point_outside_target_window",
            "message": "Drag start and end coordinates must stay inside the validated target bounds.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    let duration_ms = bounded_drag_duration_ms(args);
    match run_drag_event(start_x, start_y, end_x, end_y, duration_ms) {
        Ok(()) => json!({
            "ok": true,
            "backend": BACKEND_RUST_MACOS,
            "action": "drag",
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "startX": start_x,
            "startY": start_y,
            "endX": end_x,
            "endY": end_y,
            "durationMs": duration_ms,
            "button": "left",
            "error": Value::Null,
        }),
        Err(error) => {
            let error_message = error.clone();
            json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "action": "drag",
                "observationId": value_arg(args, "observationId"),
                "targetWindowId": value_arg(args, "targetWindowId"),
                "startX": start_x,
                "startY": start_y,
                "endX": end_x,
                "endY": end_y,
                "durationMs": duration_ms,
                "button": "left",
                "error": error,
                "message": error_message,
                "requiresPermission": is_permission_error(&error_message),
                "safeToAct": false,
                "requiresObserve": true,
            })
        }
    }
}

fn missing_drag_coordinate_result() -> Value {
    json!({
        "ok": false,
        "backend": BACKEND_RUST_MACOS,
        "action": "drag",
        "error": "missing_coordinate",
        "message": "drag requires numeric startX, startY, endX, and endY.",
        "safeToAct": false,
        "requiresObserve": true,
    })
}

fn real_shortcut_result(args: &Map<String, Value>) -> Value {
    let validation = real_validate_target_result(args);
    if !validation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return validation;
    }

    let shortcut = normalize_shortcut_name(&string_arg(args, "shortcut").unwrap_or_default());
    if !shortcut_supported(&shortcut) {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "action": "shortcut",
            "error": "unsupported_shortcut",
            "message": "Unsupported shortcut. Use select_all, undo, or redo.",
            "shortcut": shortcut,
            "safeToAct": false,
        });
    }

    match run_shortcut(&shortcut) {
        Ok(()) => json!({
            "ok": true,
            "backend": BACKEND_RUST_MACOS,
            "action": "shortcut",
            "observationId": value_arg(args, "observationId"),
            "targetWindowId": value_arg(args, "targetWindowId"),
            "shortcut": shortcut,
            "error": Value::Null,
        }),
        Err(error) => {
            let error_message = error.clone();
            json!({
                "ok": false,
                "backend": BACKEND_RUST_MACOS,
                "action": "shortcut",
                "observationId": value_arg(args, "observationId"),
                "targetWindowId": value_arg(args, "targetWindowId"),
                "shortcut": shortcut,
                "error": error,
                "message": error_message,
                "requiresPermission": is_permission_error(&error_message),
                "safeToAct": false,
                "requiresObserve": true,
            })
        }
    }
}

fn real_set_target_result(args: &Map<String, Value>) -> Value {
    let requested_app_name = string_arg(args, "appName").unwrap_or_default();
    let requested_bundle_id = string_arg(args, "bundleId").unwrap_or_default();
    let display_app_name = if requested_app_name.trim().is_empty() {
        requested_bundle_id.clone()
    } else {
        requested_app_name.clone()
    };

    if display_app_name.trim().is_empty() && requested_bundle_id.trim().is_empty() {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "missing_target",
            "message": "set_target requires appName or bundleId.",
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    if !activation_target_is_allowed(args, &display_app_name, &requested_bundle_id) {
        return app_not_allowed_for_backend(
            BACKEND_RUST_MACOS,
            args,
            &display_app_name,
            &requested_bundle_id,
            "",
            None,
        );
    }

    let script = if requested_bundle_id.trim().is_empty() {
        format!(
            "tell application {} to activate",
            applescript_quote(&display_app_name)
        )
    } else {
        format!(
            "tell application id {} to activate",
            applescript_quote(&requested_bundle_id)
        )
    };

    if let Err(error) = run_osascript(&script) {
        return json!({
            "ok": false,
            "backend": BACKEND_RUST_MACOS,
            "error": "activate_target_failed",
            "message": error,
            "requiresPermission": is_permission_error(&error),
            "safeToAct": false,
            "requiresObserve": true,
        });
    }

    thread::sleep(Duration::from_millis(250));
    let mut observe_args = args.clone();
    observe_args.insert("saveScreenshot".to_string(), json!(false));
    let mut observation = real_observe_result(&observe_args);
    if !observation
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        insert_object_value(&mut observation, "requiresObserve", json!(true));
        insert_object_value(&mut observation, "safeToAct", json!(false));
        return observation;
    }

    json!({
        "ok": true,
        "backend": BACKEND_RUST_MACOS,
        "target": observation.get("target").cloned().unwrap_or(Value::Null),
        "frontmostApp": observation.get("frontmostApp").cloned().unwrap_or(Value::Null),
        "activeWindowTitle": observation.get("activeWindowTitle").cloned().unwrap_or(Value::Null),
        "targetVisible": observation.get("targetVisible").cloned().unwrap_or_else(|| json!(false)),
        "safeToAct": observation.get("safeToAct").cloned().unwrap_or_else(|| json!(false)),
    })
}

fn app_not_allowed_for_backend(
    backend: &str,
    args: &Map<String, Value>,
    app_name: &str,
    bundle_id: &str,
    window_title: &str,
    window_id: Option<Value>,
) -> Value {
    let mut target = json!({
        "appName": app_name,
        "bundleId": bundle_id,
        "windowTitle": window_title,
    });
    if let (Some(object), Some(window_id)) = (target.as_object_mut(), window_id) {
        object.insert("windowId".to_string(), window_id);
    }
    json!({
        "ok": false,
        "backend": backend,
        "error": "app_not_allowed",
        "message": "Target app is not listed in Settings → Computer Use allowed apps.",
        "target": target,
        "allowedApps": allowed_apps(args),
        "requiresSettingsChange": true,
        "requiresConfirmation": true,
        "targetVisible": false,
        "occluded": true,
        "safeToAct": false,
        "requiresObserve": true,
    })
}

fn target_is_allowed(args: &Map<String, Value>, app_name: &str, bundle_id: &str) -> bool {
    let allowed = allowed_apps(args);
    if allowed.iter().any(|item| item == "*") {
        return true;
    }
    let app = app_name.trim().to_ascii_lowercase();
    let bundle = bundle_id.trim().to_ascii_lowercase();
    allowed.iter().any(|item| item == &app || item == &bundle)
}

fn activation_target_is_allowed(
    args: &Map<String, Value>,
    app_name: &str,
    bundle_id: &str,
) -> bool {
    let allowed = allowed_apps(args);
    if allowed.iter().any(|item| item == "*") {
        return true;
    }
    let bundle = bundle_id.trim().to_ascii_lowercase();
    if !bundle.is_empty() {
        return allowed.iter().any(|item| item == &bundle);
    }
    let app = app_name.trim().to_ascii_lowercase();
    !app.is_empty() && allowed.iter().any(|item| item == &app)
}

fn allowed_apps(args: &Map<String, Value>) -> Vec<String> {
    args.get("allowedApps")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| vec!["omiga".to_string(), "com.omiga.desktop".to_string()])
}

fn value_arg(args: &Map<String, Value>, key: &str) -> Option<Value> {
    args.get(key).cloned()
}

fn string_arg(args: &Map<String, Value>, key: &str) -> Option<String> {
    args.get(key).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    })
}

fn bool_arg(args: &Map<String, Value>, key: &str, default: bool) -> bool {
    match args.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Some(value) => value.as_bool().unwrap_or(default),
        None => default,
    }
}

fn number_arg(args: &Map<String, Value>, key: &str) -> Option<f64> {
    match args.get(key) {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(value)) => value.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn bounds_from_target(target: &Value) -> Option<(f64, f64, f64, f64)> {
    let bounds = target.get("bounds")?.as_array()?;
    if bounds.len() < 4 {
        return None;
    }
    Some((
        bounds.first()?.as_f64()?,
        bounds.get(1)?.as_f64()?,
        bounds.get(2)?.as_f64()?,
        bounds.get(3)?.as_f64()?,
    ))
}

fn element_center(observation: &Value, element_id: &str) -> Option<(f64, f64)> {
    let elements = observation.get("elements")?.as_array()?;
    for item in elements {
        let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();
        if item_id != element_id {
            continue;
        }
        let bounds = item.get("bounds")?.as_array()?;
        if bounds.len() < 4 {
            return None;
        }
        let left = bounds.first()?.as_f64()?;
        let top = bounds.get(1)?.as_f64()?;
        let width = bounds.get(2)?.as_f64()?;
        let height = bounds.get(3)?.as_f64()?;
        return Some((left + width / 2.0, top + height / 2.0));
    }
    None
}

fn insert_object_value(target: &mut Value, key: &str, value: Value) {
    if let Some(object) = target.as_object_mut() {
        object.insert(key.to_string(), value);
    }
}

fn run_stopped_result(backend: &str) -> Value {
    json!({
        "ok": false,
        "backend": backend,
        "error": "run_stopped",
        "safeToAct": false,
        "requiresObserve": true,
    })
}

fn unknown_tool_result(backend: &str, tool: &str) -> Value {
    json!({
        "ok": false,
        "backend": backend,
        "error": format!("unknown tool {tool}"),
        "safeToAct": false,
    })
}

fn query_frontmost_window() -> Result<MacWindowInfo, String> {
    let script = r#"
set AppleScript's text item delimiters to (character id 31)
set appName to ""
set appBundle to ""
set appPid to 0
set windowTitle to ""
set bx to 0
set btop to 0
set bw to 0
set bh to 0
tell application "System Events"
    set frontApps to every application process whose frontmost is true
    if (count of frontApps) is 0 then error "no_frontmost_app"
    set frontApp to item 1 of frontApps
    set appName to name of frontApp as text
    try
        set appBundle to bundle identifier of frontApp as text
    on error
        set appBundle to ""
    end try
    try
        set appPid to unix id of frontApp
    on error
        set appPid to 0
    end try
    if (count of windows of frontApp) > 0 then
        set w to item 1 of windows of frontApp
        try
            set windowTitle to name of w as text
        on error
            set windowTitle to ""
        end try
        try
            set p to position of w
            set s to size of w
            set bx to item 1 of p
            set btop to item 2 of p
            set bw to item 1 of s
            set bh to item 2 of s
        end try
    end if
end tell
if appBundle is "" then
    try
        set appBundle to id of application appName
    on error
        set appBundle to ""
    end try
end if
return appName & (character id 31) & appBundle & (character id 31) & appPid & (character id 31) & windowTitle & (character id 31) & bx & (character id 31) & btop & (character id 31) & bw & (character id 31) & bh
"#;

    let output = run_osascript(script)?;
    let fields = output
        .trim_end_matches(['\r', '\n'])
        .split('\u{1f}')
        .collect::<Vec<_>>();
    if fields.len() < 8 {
        return Err(format!("unexpected frontmost window response: {output}"));
    }

    let app_name = fields[0].trim().to_string();
    if app_name.is_empty() {
        return Err("no_frontmost_app".to_string());
    }
    let bundle_id = fields[1].trim().to_string();
    let pid = fields[2].trim().parse::<i64>().unwrap_or_default();
    let window_title = fields[3].trim().to_string();
    let x = fields[4].trim().parse::<f64>().unwrap_or_default();
    let y = fields[5].trim().parse::<f64>().unwrap_or_default();
    let width = fields[6].trim().parse::<f64>().unwrap_or_default();
    let height = fields[7].trim().parse::<f64>().unwrap_or_default();
    let window_id = stable_window_id(
        &app_name,
        &bundle_id,
        pid,
        &window_title,
        x,
        y,
        width,
        height,
    );

    Ok(MacWindowInfo {
        app_name,
        bundle_id,
        pid,
        window_title,
        x,
        y,
        width,
        height,
        window_id,
    })
}

fn query_accessibility_elements() -> Result<Vec<Value>, String> {
    let script = format!(
        r#"
property rowDelim : character id 30
property colDelim : character id 31
property maxItems : {max_items}
property maxDepth : {max_depth}
property maxTextChars : {max_text_chars}
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
"#,
        max_items = MAX_AX_ELEMENTS,
        max_depth = MAX_AX_DEPTH,
        max_text_chars = MAX_AX_TEXT_CHARS
    );
    parse_ax_element_rows(&run_osascript(&script)?)
}

fn parse_bool_text(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn semantic_kind(role: &str, subrole: &str) -> &'static str {
    let text = format!("{role} {subrole}").to_ascii_lowercase();
    if text.contains("window") {
        "window"
    } else if text.contains("button") {
        if text.contains("check") {
            "checkbox"
        } else if text.contains("radio") {
            "radio"
        } else {
            "button"
        }
    } else if text.contains("text field")
        || text.contains("textfield")
        || text.contains("text area")
        || text.contains("textarea")
    {
        "text_input"
    } else if text.contains("static text") || text.trim() == "axstatictext" {
        "text"
    } else if text.contains("menu") {
        "menu"
    } else if text.contains("scroll") || text.contains("valueindicator") {
        "scroll"
    } else if text.contains("link") {
        "link"
    } else if text.contains("image") {
        "image"
    } else if text.contains("table") || text.contains("outline") || text.contains("list") {
        "collection"
    } else if text.contains("slider") {
        "slider"
    } else {
        "unknown"
    }
}

fn semantic_interactable(kind: &str, enabled: Option<bool>, role: &str) -> bool {
    if enabled == Some(false) {
        return false;
    }
    matches!(
        kind,
        "button" | "checkbox" | "radio" | "text_input" | "menu" | "link" | "slider" | "scroll"
    ) || role.to_ascii_lowercase().contains("button")
        || enabled == Some(true)
}

fn parse_ax_element_rows(raw: &str) -> Result<Vec<Value>, String> {
    let mut elements = Vec::new();
    for row in raw.trim_end_matches(['\r', '\n']).split('\u{1e}') {
        if row.trim().is_empty() {
            continue;
        }
        let fields = row.split('\u{1f}').collect::<Vec<_>>();
        if fields.len() < 7 {
            continue;
        }
        let (
            element_id,
            parent_id,
            depth,
            role,
            subrole,
            role_description,
            name,
            description,
            value_preview,
            help,
            enabled_raw,
            focused_raw,
            selected_raw,
            expanded_raw,
            x_raw,
            y_raw,
            width_raw,
            height_raw,
            label,
            label_source,
        ) = if fields.len() >= 18 {
            let mut label = "";
            let mut label_source = "none";
            for (source, candidate) in [
                ("name", fields[6].trim()),
                ("description", fields[7].trim()),
                ("value", fields[8].trim()),
                ("roleDescription", fields[5].trim()),
            ] {
                if !candidate.is_empty() {
                    label = candidate;
                    label_source = source;
                    break;
                }
            }
            (
                fields[0].trim(),
                fields[1].trim(),
                fields[2].trim().parse::<i64>().unwrap_or_default(),
                fields[3].trim(),
                fields[4].trim(),
                fields[5].trim(),
                fields[6].trim(),
                fields[7].trim(),
                fields[8].trim(),
                fields[9].trim(),
                fields[10].trim(),
                fields[11].trim(),
                fields[12].trim(),
                fields[13].trim(),
                fields[14].trim(),
                fields[15].trim(),
                fields[16].trim(),
                fields[17].trim(),
                label,
                label_source,
            )
        } else {
            (
                fields[0].trim(),
                "",
                0,
                fields[1].trim(),
                "",
                "",
                fields[2].trim(),
                "",
                "",
                "",
                "",
                "",
                "",
                "",
                fields[3].trim(),
                fields[4].trim(),
                fields[5].trim(),
                fields[6].trim(),
                fields[2].trim(),
                "legacy",
            )
        };
        let x = x_raw.parse::<f64>().unwrap_or_default();
        let y = y_raw.parse::<f64>().unwrap_or_default();
        let width = width_raw.parse::<f64>().unwrap_or_default();
        let height = height_raw.parse::<f64>().unwrap_or_default();
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        let role = if role.is_empty() { "unknown" } else { role };
        let kind = semantic_kind(role, subrole);
        let enabled = parse_bool_text(enabled_raw);
        let mut element = json!({
            "id": element_id,
            "role": role,
            "label": label,
            "bounds": [x, y, width, height],
            "source": "macos_accessibility",
            "depth": depth,
            "kind": kind,
            "interactable": semantic_interactable(kind, enabled, role),
            "labelSource": label_source,
        });
        if !parent_id.is_empty() {
            insert_object_value(&mut element, "parentId", json!(parent_id));
        }
        for (key, value) in [
            ("subrole", subrole),
            ("roleDescription", role_description),
            ("name", name),
            ("description", description),
            ("valuePreview", value_preview),
            ("help", help),
        ] {
            if !value.is_empty() {
                insert_object_value(&mut element, key, json!(value));
            }
        }
        for (key, value) in [
            ("enabled", enabled),
            ("focused", parse_bool_text(focused_raw)),
            ("selected", parse_bool_text(selected_raw)),
            ("expanded", parse_bool_text(expanded_raw)),
        ] {
            if let Some(value) = value {
                insert_object_value(&mut element, key, json!(value));
            }
        }
        elements.push(element);
    }
    Ok(elements)
}

fn desktop_screen_size() -> Option<Value> {
    let output =
        run_osascript(r#"tell application "Finder" to get bounds of window of desktop"#).ok()?;
    let values = output
        .split(',')
        .filter_map(|item| item.trim().parse::<f64>().ok())
        .collect::<Vec<_>>();
    if values.len() < 4 {
        return None;
    }
    Some(json!([
        (values[2] - values[0]).max(0.0),
        (values[3] - values[1]).max(0.0),
    ]))
}

fn capture_screenshot(run_id: &str, observation_id: &str) -> (Option<String>, Option<String>) {
    let root = env::temp_dir()
        .join("omiga-computer-use")
        .join(sanitize_path_component(run_id));
    if let Err(error) = fs::create_dir_all(&root) {
        return (
            None,
            Some(format!("failed to create screenshot directory: {error}")),
        );
    }

    let path = root.join(format!("{}.png", sanitize_path_component(observation_id)));
    let output = Command::new("screencapture")
        .arg("-x")
        .arg("-t")
        .arg("png")
        .arg(&path)
        .output();

    match output {
        Ok(output) if output.status.success() && path.exists() => {
            (Some(path_to_string(path)), None)
        }
        Ok(output) => {
            let _ = fs::remove_file(&path);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = permission_error_message(&stderr)
                .map(str::to_string)
                .filter(|message| !message.is_empty())
                .or_else(|| {
                    if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    }
                })
                .unwrap_or_else(|| "screencapture failed".to_string());
            (None, Some(message))
        }
        Err(error) => (None, Some(format!("failed to run screencapture: {error}"))),
    }
}

fn capture_screenshot_region(
    run_id: &str,
    observation_id: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> (Option<String>, Option<String>) {
    if width <= 0.0 || height <= 0.0 {
        return capture_screenshot(run_id, observation_id);
    }
    let root = env::temp_dir()
        .join("omiga-computer-use")
        .join(sanitize_path_component(run_id));
    if let Err(error) = fs::create_dir_all(&root) {
        return (
            None,
            Some(format!("failed to create screenshot directory: {error}")),
        );
    }

    let path = root.join(format!("{}.png", sanitize_path_component(observation_id)));
    let rect = format!(
        "{},{},{},{}",
        x.max(0.0).round() as i64,
        y.max(0.0).round() as i64,
        width.max(1.0).round() as i64,
        height.max(1.0).round() as i64
    );
    let output = Command::new("screencapture")
        .arg("-x")
        .arg("-t")
        .arg("png")
        .arg("-R")
        .arg(rect)
        .arg(&path)
        .output();

    match output {
        Ok(output) if output.status.success() && path.exists() => {
            (Some(path_to_string(path)), None)
        }
        Ok(output) => {
            let _ = fs::remove_file(&path);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = permission_error_message(&stderr)
                .map(str::to_string)
                .filter(|message| !message.is_empty())
                .or_else(|| {
                    if stderr.is_empty() {
                        None
                    } else {
                        Some(stderr)
                    }
                })
                .unwrap_or_else(|| "screencapture region failed".to_string());
            (None, Some(message))
        }
        Err(error) => (
            None,
            Some(format!("failed to run screencapture region: {error}")),
        ),
    }
}

fn vision_ocr_script_path() -> Result<PathBuf, String> {
    let path = env::temp_dir().join("omiga-computer-use-vision-ocr.swift");
    let needs_write = fs::read_to_string(&path)
        .map(|current| current != VISION_OCR_SWIFT)
        .unwrap_or(true);
    if needs_write {
        fs::write(&path, VISION_OCR_SWIFT)
            .map_err(|error| format!("failed to write Vision OCR helper: {error}"))?;
    }
    Ok(path)
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .and_then(|text| text.trim().parse::<f64>().ok())
    })
}

fn truncate_to_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect::<String>() + "…"
}

fn clean_visual_text_item(item: &Value) -> Option<Value> {
    let object = item.as_object()?;
    let text = object
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .replace('\n', " ")
        .trim()
        .to_string();
    if text.is_empty() {
        return None;
    }

    let mut bounds = object
        .get("bounds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(4)
                .map(|value| value_as_f64(value).unwrap_or_default())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    while bounds.len() < 4 {
        bounds.push(0.0);
    }

    let confidence = object
        .get("confidence")
        .and_then(value_as_f64)
        .unwrap_or_default()
        .clamp(0.0, 1.0);
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("macos_vision_ocr");

    Some(json!({
        "text": truncate_to_chars(&text, MAX_VISUAL_TEXT_CHARS),
        "confidence": confidence,
        "bounds": bounds,
        "source": source,
    }))
}

fn run_visual_text_ocr(image_path: &str) -> Result<Vec<Value>, String> {
    if !cfg!(target_os = "macos") {
        return Err("visual_text_ocr_requires_macos".to_string());
    }
    let script = vision_ocr_script_path()?;
    let swift = if PathBuf::from("/usr/bin/swift").exists() {
        PathBuf::from("/usr/bin/swift")
    } else {
        PathBuf::from("swift")
    };
    let output = Command::new(swift)
        .arg(script)
        .arg(image_path)
        .arg(MAX_VISUAL_TEXT_ITEMS.to_string())
        .arg(MAX_VISUAL_TEXT_CHARS.to_string())
        .output()
        .map_err(|error| format!("vision_ocr_unavailable: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = permission_error_message(&stderr)
            .map(str::to_string)
            .or_else(|| {
                if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                }
            })
            .or_else(|| {
                if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                }
            })
            .unwrap_or_else(|| "vision_ocr_failed".to_string());
        return Err(message);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout)
        .map_err(|error| format!("vision_ocr_returned_invalid_json: {error}"))?;
    let items = parsed
        .as_array()
        .ok_or_else(|| "vision_ocr_returned_unexpected_payload".to_string())?;
    Ok(items
        .iter()
        .take(MAX_VISUAL_TEXT_ITEMS)
        .filter_map(clean_visual_text_item)
        .collect())
}

fn run_fixed_click(x: f64, y: f64) -> Result<(), String> {
    let script = format!(
        r#"tell application "System Events" to click at {{{}, {}}}"#,
        x.round() as i64,
        y.round() as i64
    );
    run_osascript(&script).map(|_| ())
}

fn normalize_key_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_")
}

fn key_code_for_name(key: &str) -> Option<i64> {
    match key {
        "enter" | "return" => Some(36),
        "tab" => Some(48),
        "escape" | "esc" => Some(53),
        "backspace" => Some(51),
        "delete" => Some(117),
        "arrow_left" | "left" => Some(123),
        "arrow_right" | "right" => Some(124),
        "arrow_down" | "down" => Some(125),
        "arrow_up" | "up" => Some(126),
        "page_up" => Some(116),
        "page_down" => Some(121),
        "home" => Some(115),
        "end" => Some(119),
        "space" => Some(49),
        _ => None,
    }
}

fn bounded_key_count(args: &Map<String, Value>) -> i64 {
    let count = args.get("count").and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    });
    count.unwrap_or(1).clamp(1, 20)
}

fn run_key_press(key_code: i64, count: i64) -> Result<(), String> {
    let script = format!(
        r#"tell application "System Events"
repeat {count} times
key code {key_code}
delay 0.03
end repeat
end tell"#
    );
    run_osascript(&script).map(|_| ())
}

fn normalize_scroll_direction(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_")
}

fn bounded_scroll_amount(args: &Map<String, Value>) -> i64 {
    let amount = args.get("amount").and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    });
    amount.unwrap_or(5).clamp(1, 20)
}

fn bounded_drag_duration_ms(args: &Map<String, Value>) -> i64 {
    let duration = args.get("durationMs").and_then(|value| {
        value.as_i64().or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
    });
    duration.unwrap_or(350).clamp(50, 2000)
}

fn scroll_deltas(direction: &str, amount: i64) -> Option<(i64, i64)> {
    match direction {
        "down" => Some((0, -amount)),
        "up" => Some((0, amount)),
        "right" => Some((-amount, 0)),
        "left" => Some((amount, 0)),
        _ => None,
    }
}

fn target_bounds_from_validation(validation: &Value) -> Option<(f64, f64, f64, f64)> {
    let bounds = validation
        .pointer("/currentTarget/bounds")
        .or_else(|| validation.pointer("/target/bounds"))?
        .as_array()?;
    if bounds.len() != 4 {
        return None;
    }
    Some((
        bounds.first()?.as_f64()?,
        bounds.get(1)?.as_f64()?,
        bounds.get(2)?.as_f64()?,
        bounds.get(3)?.as_f64()?,
    ))
}

fn point_inside_bounds(x: f64, y: f64, bounds: (f64, f64, f64, f64)) -> bool {
    let (bx, by, width, height) = bounds;
    x >= bx && y >= by && x <= bx + width && y <= by + height
}

fn target_center_from_validation(validation: &Value) -> Option<(f64, f64)> {
    let (x, y, width, height) = target_bounds_from_validation(validation)?;
    Some((x + width / 2.0, y + height / 2.0))
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: *mut c_void) -> *mut c_void;
    fn CGEventCreateScrollWheelEvent(
        source: *mut c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
    ) -> *mut c_void;
    fn CGEventCreateMouseEvent(
        source: *mut c_void,
        mouse_type: u32,
        mouse_cursor_position: CGPoint,
        mouse_button: u32,
    ) -> *mut c_void;
    fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    fn CGEventSetLocation(event: *mut c_void, location: CGPoint);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

fn run_scroll_event(delta_x: i64, delta_y: i64, x: f64, y: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    unsafe {
        let previous_event = CGEventCreate(std::ptr::null_mut());
        let previous_location = if previous_event.is_null() {
            None
        } else {
            let location = CGEventGetLocation(previous_event);
            CFRelease(previous_event.cast_const());
            Some(location)
        };
        let event = CGEventCreateScrollWheelEvent(
            std::ptr::null_mut(),
            1, // kCGScrollEventUnitLine
            2,
            delta_y as i32,
            delta_x as i32,
        );
        if event.is_null() {
            return Err("scroll_event_create_failed".to_string());
        }
        let target_point = CGPoint { x, y };
        CGWarpMouseCursorPosition(target_point);
        thread::sleep(Duration::from_millis(30));
        CGEventSetLocation(event, target_point);
        CGEventPost(0, event); // kCGHIDEventTap
        thread::sleep(Duration::from_millis(150));
        CFRelease(event.cast_const());
        if let Some(location) = previous_location {
            CGWarpMouseCursorPosition(location);
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (delta_x, delta_y, x, y);
        Err("scroll_event_unsupported_platform".to_string())
    }
}

fn run_drag_event(
    start_x: f64,
    start_y: f64,
    end_x: f64,
    end_y: f64,
    duration_ms: i64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    unsafe {
        let previous_event = CGEventCreate(std::ptr::null_mut());
        let previous_location = if previous_event.is_null() {
            None
        } else {
            let location = CGEventGetLocation(previous_event);
            CFRelease(previous_event.cast_const());
            Some(location)
        };

        let post_mouse_event = |event_type: u32, point: CGPoint| -> Result<(), String> {
            let event = CGEventCreateMouseEvent(std::ptr::null_mut(), event_type, point, 0);
            if event.is_null() {
                return Err("mouse_event_create_failed".to_string());
            }
            CGEventPost(0, event); // kCGHIDEventTap
            CFRelease(event.cast_const());
            Ok(())
        };

        let start = CGPoint {
            x: start_x,
            y: start_y,
        };
        let end = CGPoint { x: end_x, y: end_y };
        let steps = (duration_ms / 16).clamp(4, 60);
        let delay = Duration::from_millis((duration_ms / steps).max(1) as u64);

        CGWarpMouseCursorPosition(start);
        thread::sleep(Duration::from_millis(30));
        post_mouse_event(1, start)?; // kCGEventLeftMouseDown
        for step in 1..=steps {
            let ratio = step as f64 / steps as f64;
            let point = CGPoint {
                x: start.x + (end.x - start.x) * ratio,
                y: start.y + (end.y - start.y) * ratio,
            };
            post_mouse_event(6, point)?; // kCGEventLeftMouseDragged
            thread::sleep(delay);
        }
        post_mouse_event(2, end)?; // kCGEventLeftMouseUp
        thread::sleep(Duration::from_millis(150));
        if let Some(location) = previous_location {
            CGWarpMouseCursorPosition(location);
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (start_x, start_y, end_x, end_y, duration_ms);
        Err("drag_event_unsupported_platform".to_string())
    }
}

fn normalize_shortcut_name(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .replace(' ', "_")
}

fn shortcut_supported(shortcut: &str) -> bool {
    matches!(shortcut, "select_all" | "undo" | "redo")
}

fn run_shortcut(shortcut: &str) -> Result<(), String> {
    let script = match shortcut {
        "select_all" => r#"tell application "System Events" to keystroke "a" using command down"#,
        "undo" => r#"tell application "System Events" to keystroke "z" using command down"#,
        "redo" => {
            r#"tell application "System Events" to keystroke "z" using {command down, shift down}"#
        }
        _ => return Err("unsupported_shortcut".to_string()),
    };
    run_osascript(script).map(|_| ())
}

fn direct_type_script(text: &str) -> Option<String> {
    if !direct_type_supported(text) {
        return None;
    }

    let mut lines = vec![r#"tell application "System Events""#.to_string()];
    let mut chunk = String::new();
    let flush_chunk = |lines: &mut Vec<String>, chunk: &mut String| {
        if !chunk.is_empty() {
            lines.push(format!("keystroke {}", applescript_quote(chunk)));
            chunk.clear();
        }
    };

    for ch in text.chars() {
        match ch {
            '\n' => {
                flush_chunk(&mut lines, &mut chunk);
                lines.push("key code 36".to_string());
            }
            '\t' => {
                flush_chunk(&mut lines, &mut chunk);
                lines.push("key code 48".to_string());
            }
            _ => chunk.push(ch),
        }
    }
    flush_chunk(&mut lines, &mut chunk);
    lines.push("end tell".to_string());
    Some(lines.join("\n"))
}

fn run_direct_type(text: &str) -> (bool, Option<&'static str>, Option<String>) {
    let Some(script) = direct_type_script(text) else {
        return (false, Some("direct_type_unsupported_text"), None);
    };
    match run_osascript(&script) {
        Ok(_) => (true, None, None),
        Err(error) => (false, Some("direct_type_failed"), Some(error)),
    }
}

fn clipboard_fallback_allowed(args: &Map<String, Value>, text: &str) -> bool {
    bool_arg(args, "allowClipboardFallback", true) && !text_looks_sensitive(text)
}

fn clipboard_get() -> String {
    let output = Command::new("pbpaste").output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        _ => String::new(),
    }
}

fn clipboard_set(text: &str) -> Result<(), String> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start pbcopy: {error}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("failed to write clipboard: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to finish pbcopy: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("pbcopy exited with status {}", output.status)
        } else {
            stderr
        })
    }
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().to_string()
}

fn stable_window_id(
    app_name: &str,
    bundle_id: &str,
    pid: i64,
    window_title: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> String {
    let stable = json!({
        "appName": app_name,
        "bundleId": bundle_id,
        "pid": pid,
        "windowTitle": window_title,
        "bounds": [x, y, width, height],
    });
    let mut hasher = Sha256::new();
    hasher.update(stable.to_string().as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    format!("mac_{}", &hex[..16])
}

fn run_osascript(script: &str) -> Result<String, String> {
    let mut command = Command::new("osascript");
    for line in script
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
    {
        command.arg("-e").arg(line);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to run osascript: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err(format!("osascript exited with status {}", output.status));
        }
        return Err(stderr);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn is_permission_error(message: &str) -> bool {
    permission_error_message(message).is_some()
}

fn permission_error_message(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    let blocked = lower.contains("not allowed")
        || lower.contains("not authorized")
        || lower.contains("not permitted")
        || lower.contains("accessibility")
        || lower.contains("assistive")
        || lower.contains("privacy")
        || lower.contains("screen recording")
        || lower.contains("could not create image from display")
        || lower.contains("-10827")
        || lower.contains("-25211")
        || message.contains("不允许")
        || message.contains("辅助访问")
        || message.contains("隐私")
        || message.contains("权限");
    if blocked {
        Some(
            "macOS blocked UI automation or screen capture. Grant Accessibility and Screen Recording permissions to Omiga/Terminal, then retry Computer Use.",
        )
    } else {
        None
    }
}

fn direct_type_supported(text: &str) -> bool {
    text.chars().count() <= MAX_DIRECT_TYPE_CHARS
        && text
            .chars()
            .all(|ch| matches!(ch, '\n' | '\t') || !ch.is_control())
}

fn text_looks_sensitive(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    labeled_secret_assignment(&lower, "password")
        || labeled_secret_assignment(&lower, "token")
        || labeled_secret_assignment(&lower, "api_key")
        || labeled_secret_assignment(&lower, "api-key")
        || labeled_secret_assignment(&lower, "apikey")
        || text.contains("sk-")
        || text.contains("ghp_")
        || text.contains("AKIA")
        || text.contains("-----BEGIN ")
}

fn labeled_secret_assignment(lower_text: &str, label: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(relative_index) = lower_text[search_from..].find(label) {
        let start = search_from + relative_index + label.len();
        let rest = &lower_text[start..];
        let next = rest.chars().find(|ch| !ch.is_whitespace());
        if matches!(next, Some('=') | Some(':')) {
            return true;
        }
        search_from = start;
    }
    false
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
