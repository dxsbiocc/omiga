//! Computer Use domain primitives.
//!
//! Computer Use carries the explicit task/session gate, model-visible `computer_*`
//! facade schemas, policy checks, audit logging, and the internal MCP bridge to
//! the optional `computer-use` backend plugin.

use crate::domain::tools::ToolSchema;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const MCP_SERVER_NAME: &str = "computer";
pub const SETTINGS_KEY: &str = "omiga.computer_use.settings.v1";
const MAX_ACTIONS_BEFORE_OBSERVE: u32 = 5;
const MAX_TOTAL_ACTIONS: u32 = 15;
const OBSERVATION_TTL_SECS: i64 = 60;
const DEFAULT_LOG_RETENTION_DAYS: u32 = 14;

/// Explicit user-selected scope for exposing Computer Use facade tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerUseMode {
    Off,
    Task,
    Session,
}

impl ComputerUseMode {
    /// Parse the frontend request field. Unknown values are intentionally safe
    /// and behave as `off` so stale or malformed clients cannot enable control.
    pub fn from_request(raw: Option<&str>) -> Self {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            Some(v) if v.eq_ignore_ascii_case("task") => Self::Task,
            Some(v) if v.eq_ignore_ascii_case("session") => Self::Session,
            _ => Self::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Task => "task",
            Self::Session => "session",
        }
    }

    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerFacadeTool {
    Observe,
    SetTarget,
    Click,
    ClickElement,
    Drag,
    TypeText,
    KeyPress,
    Scroll,
    Shortcut,
    Stop,
}

impl ComputerFacadeTool {
    pub fn from_model_name(name: &str) -> Option<Self> {
        match name {
            "computer_observe" => Some(Self::Observe),
            "computer_set_target" => Some(Self::SetTarget),
            "computer_click" => Some(Self::Click),
            "computer_click_element" => Some(Self::ClickElement),
            "computer_drag" => Some(Self::Drag),
            "computer_type" => Some(Self::TypeText),
            "computer_key" => Some(Self::KeyPress),
            "computer_scroll" => Some(Self::Scroll),
            "computer_shortcut" => Some(Self::Shortcut),
            "computer_stop" => Some(Self::Stop),
            _ => None,
        }
    }

    pub fn model_name(self) -> &'static str {
        match self {
            Self::Observe => "computer_observe",
            Self::SetTarget => "computer_set_target",
            Self::Click => "computer_click",
            Self::ClickElement => "computer_click_element",
            Self::Drag => "computer_drag",
            Self::TypeText => "computer_type",
            Self::KeyPress => "computer_key",
            Self::Scroll => "computer_scroll",
            Self::Shortcut => "computer_shortcut",
            Self::Stop => "computer_stop",
        }
    }

    pub fn backend_tool_name(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::SetTarget => "set_target",
            Self::Click => "click",
            Self::ClickElement => "click_element",
            Self::Drag => "drag",
            Self::TypeText => "type_text",
            Self::KeyPress => "key_press",
            Self::Scroll => "scroll",
            Self::Shortcut => "shortcut",
            Self::Stop => "stop",
        }
    }

    pub fn backend_mcp_name(self) -> String {
        format!("mcp__{MCP_SERVER_NAME}__{}", self.backend_tool_name())
    }

    pub fn backend_result_requires_target_identity(self) -> bool {
        matches!(self, Self::Observe | Self::SetTarget)
    }
}

pub fn is_facade_tool_name(name: &str) -> bool {
    ComputerFacadeTool::from_model_name(name).is_some()
}

pub fn facade_tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema::new(
            "computer_observe",
            "Observe the current local computer UI through Omiga's guarded Computer Use facade. Use before any computer_click/computer_type action.",
            json!({
                "type": "object",
                "properties": {
                    "targetHint": {
                        "type": "string",
                        "description": "Optional app/window hint to help identify the desired target."
                    },
                    "extractVisualText": {
                        "type": "boolean",
                        "description": "Optional. When true, request bounded native visual OCR text boxes for the observed target. This may require Screen Recording permission and is off by default."
                    }
                },
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_set_target",
            "Select or switch the target local app/window for Computer Use. Required before cross-app workflows.",
            json!({
                "type": "object",
                "properties": {
                    "appName": { "type": "string" },
                    "bundleId": { "type": "string" },
                    "windowTitle": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_click",
            "Click a coordinate inside the last observed target window. Requires a recent observationId.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "button": {
                        "type": "string",
                        "enum": ["left"],
                        "description": "Only left clicks are supported by the current real backend."
                    }
                },
                "required": ["observationId", "targetWindowId", "x", "y"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_click_element",
            "Click an element returned by computer_observe. Prefer this over raw coordinates when an element id is available.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "elementId": { "type": "string" }
                },
                "required": ["observationId", "targetWindowId", "elementId"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_drag",
            "Drag from one coordinate to another inside the last observed target window after revalidation. Use only for simple left-button drags such as moving a window or selecting visible content.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "startX": { "type": "number" },
                    "startY": { "type": "number" },
                    "endX": { "type": "number" },
                    "endY": { "type": "number" },
                    "durationMs": {
                        "type": "integer",
                        "minimum": 50,
                        "maximum": 2000,
                        "description": "Drag duration in milliseconds. Defaults to 350 and is capped by the backend."
                    },
                    "button": {
                        "type": "string",
                        "enum": ["left"],
                        "description": "Only left-button drags are supported."
                    }
                },
                "required": ["observationId", "targetWindowId", "startX", "startY", "endX", "endY"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_type",
            "Type text into the current target window after revalidation. Do not use for secrets unless the user explicitly asked.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "text": { "type": "string" }
                },
                "required": ["observationId", "targetWindowId", "text"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_key",
            "Press a supported non-text key in the current target window after revalidation. Use for Enter, Escape, Tab, arrows, paging, Home/End, Delete, and Space.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "key": {
                        "type": "string",
                        "enum": [
                            "enter",
                            "return",
                            "tab",
                            "escape",
                            "backspace",
                            "delete",
                            "arrow_up",
                            "arrow_down",
                            "arrow_left",
                            "arrow_right",
                            "page_up",
                            "page_down",
                            "home",
                            "end",
                            "space"
                        ],
                        "description": "Supported fixed key names only; arbitrary shortcuts are intentionally not accepted."
                    },
                    "count": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "description": "Number of key presses. Defaults to 1 and is capped by the backend."
                    }
                },
                "required": ["observationId", "targetWindowId", "key"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_scroll",
            "Scroll inside the current target window after revalidation using a fixed system scroll-wheel event at the target center.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down", "left", "right"],
                        "description": "Scroll direction. Use down/up for vertical content and left/right for horizontal content."
                    },
                    "amount": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 20,
                        "description": "Scroll-wheel line amount. Defaults to 5 and is capped by the backend."
                    }
                },
                "required": ["observationId", "targetWindowId", "direction"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_shortcut",
            "Run a fixed, whitelisted keyboard shortcut in the current target window after revalidation. Arbitrary shortcut strings are not accepted.",
            json!({
                "type": "object",
                "properties": {
                    "observationId": { "type": "string" },
                    "targetWindowId": { "type": ["integer", "string"] },
                    "shortcut": {
                        "type": "string",
                        "enum": ["select_all", "undo", "redo"],
                        "description": "Supported fixed shortcuts only. select_all sends Command+A; undo sends Command+Z; redo sends Command+Shift+Z."
                    }
                },
                "required": ["observationId", "targetWindowId", "shortcut"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "computer_stop",
            "Stop the active Computer Use run and prevent further queued local UI actions.",
            json!({
                "type": "object",
                "properties": {
                    "reason": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
    ]
}

pub fn normalize_facade_arguments(arguments_json: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(arguments_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseSettings {
    #[serde(default = "default_allowed_apps")]
    pub allowed_apps: Vec<String>,
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,
    #[serde(default)]
    pub save_screenshots: bool,
}

impl Default for ComputerUseSettings {
    fn default() -> Self {
        Self {
            allowed_apps: default_allowed_apps(),
            log_retention_days: default_log_retention_days(),
            save_screenshots: false,
        }
    }
}

impl ComputerUseSettings {
    pub fn from_stored_json(raw: Option<&str>) -> Self {
        raw.and_then(|text| serde_json::from_str::<Self>(text).ok())
            .map(|settings| settings.normalized())
            .unwrap_or_default()
    }

    pub fn normalized(mut self) -> Self {
        self.allowed_apps = normalize_allowed_apps(self.allowed_apps);
        if self.allowed_apps.is_empty() {
            self.allowed_apps = default_allowed_apps();
        }
        self.log_retention_days = self.log_retention_days.clamp(1, 365);
        self
    }

    pub fn allows_target(&self, target: &ComputerUseTargetIdentity) -> bool {
        let allowed = normalize_allowed_apps(self.allowed_apps.clone());
        if allowed.iter().any(|entry| entry == "*") {
            return true;
        }
        let app_name = normalize_match_value(target.app_name.as_deref());
        let bundle_id = normalize_match_value(target.bundle_id.as_deref());
        allowed.iter().any(|entry| {
            Some(entry.as_str()) == app_name.as_deref()
                || Some(entry.as_str()) == bundle_id.as_deref()
        })
    }

    pub fn to_backend_value(&self) -> Value {
        json!({
            "allowedApps": self.allowed_apps.clone(),
            "saveScreenshot": self.save_screenshots,
        })
    }
}

fn default_allowed_apps() -> Vec<String> {
    vec!["Omiga".to_string(), "com.omiga.desktop".to_string()]
}

fn default_log_retention_days() -> u32 {
    DEFAULT_LOG_RETENTION_DAYS
}

fn normalize_allowed_apps(apps: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for app in apps {
        let value = normalize_match_value(Some(&app));
        if let Some(value) = value {
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out
}

fn normalize_match_value(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseTargetIdentity {
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseAppPolicyViolation {
    pub target: ComputerUseTargetIdentity,
    pub allowed_apps: Vec<String>,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
struct ComputerUseRun {
    run_id: String,
    started_at: DateTime<Utc>,
    stopped: bool,
    last_observation_id: Option<String>,
    last_observed_at: Option<DateTime<Utc>>,
    target_window_id: Option<String>,
    target_bounds: Option<[f64; 4]>,
    target_app_name: Option<String>,
    target_bundle_id: Option<String>,
    actions_since_observe: u32,
    total_actions: u32,
}

#[derive(Debug, Clone)]
pub struct PreparedComputerUseCall {
    pub run_id: String,
    pub tool: ComputerFacadeTool,
    pub backend_tool_name: String,
    pub backend_arguments_json: String,
    pub redacted_arguments: Value,
}

impl PreparedComputerUseCall {
    pub fn requires_backend_validate(&self) -> bool {
        action_requires_observe(self.tool)
    }

    pub fn validate_backend_tool_name(&self) -> String {
        format!("mcp__{MCP_SERVER_NAME}__validate_target")
    }

    pub fn validate_backend_arguments_json(&self) -> String {
        let mut args = normalize_facade_arguments(&self.backend_arguments_json);
        args.insert("runId".to_string(), json!(self.run_id));
        Value::Object(args).to_string()
    }

    pub fn inject_settings(&mut self, settings: &ComputerUseSettings) {
        let mut args = normalize_facade_arguments(&self.backend_arguments_json);
        args.insert(
            "allowedApps".to_string(),
            json!(settings.allowed_apps.clone()),
        );
        args.insert(
            "saveScreenshot".to_string(),
            json!(settings.save_screenshots),
        );
        if self.tool == ComputerFacadeTool::TypeText {
            let probable_secret = args
                .get("text")
                .map(value_contains_probable_secret)
                .unwrap_or(false);
            args.insert(
                "allowClipboardFallback".to_string(),
                json!(!probable_secret),
            );
            if probable_secret {
                args.insert(
                    "clipboardFallbackPolicy".to_string(),
                    json!("disabled_for_probable_secret"),
                );
            }
        }
        self.backend_arguments_json = Value::Object(args).to_string();
    }
}

#[derive(Debug, Clone)]
pub struct ComputerUsePolicyError {
    pub run_id: Option<String>,
    pub code: &'static str,
    pub message: String,
    pub requires_observe: bool,
    pub requires_confirmation: bool,
}

impl ComputerUsePolicyError {
    pub fn model_output(&self) -> String {
        json!({
            "ok": false,
            "error": self.code,
            "message": self.message,
            "requiresObserve": self.requires_observe,
            "requiresConfirmation": self.requires_confirmation,
            "runId": self.run_id,
        })
        .to_string()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseStopStatus {
    pub stopped: bool,
    pub run_id: Option<String>,
    pub message: String,
    pub backend_stopped: Option<bool>,
    pub backend_message: Option<String>,
    pub backend_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseAuditSummary {
    pub audit_root: String,
    pub runs_root: String,
    pub run_count: usize,
    pub flow_count: usize,
    pub action_count: usize,
    pub result_ok_count: usize,
    pub result_blocked_count: usize,
    pub result_needs_attention_count: usize,
    pub result_stopped_count: usize,
    pub result_unknown_count: usize,
    pub bytes: u64,
    pub retention_days: Option<u32>,
    pub pruned_run_count: usize,
    pub pruned_temp_dir_count: usize,
    pub pruned_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerUseAuditPruneSummary {
    pub pruned_run_count: usize,
    pub pruned_temp_dir_count: usize,
    pub pruned_bytes: u64,
}

static COMPUTER_USE_RUNS: OnceLock<Mutex<HashMap<String, ComputerUseRun>>> = OnceLock::new();

fn runs() -> &'static Mutex<HashMap<String, ComputerUseRun>> {
    COMPUTER_USE_RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn new_run() -> ComputerUseRun {
    ComputerUseRun {
        run_id: format!("curun_{}", uuid::Uuid::new_v4()),
        started_at: Utc::now(),
        stopped: false,
        last_observation_id: None,
        last_observed_at: None,
        target_window_id: None,
        target_bounds: None,
        target_app_name: None,
        target_bundle_id: None,
        actions_since_observe: 0,
        total_actions: 0,
    }
}

fn action_requires_observe(tool: ComputerFacadeTool) -> bool {
    matches!(
        tool,
        ComputerFacadeTool::Click
            | ComputerFacadeTool::ClickElement
            | ComputerFacadeTool::Drag
            | ComputerFacadeTool::TypeText
            | ComputerFacadeTool::KeyPress
            | ComputerFacadeTool::Scroll
            | ComputerFacadeTool::Shortcut
    )
}

pub fn prepare_facade_call(
    session_id: &str,
    tool: ComputerFacadeTool,
    arguments_json: &str,
) -> Result<PreparedComputerUseCall, ComputerUsePolicyError> {
    let mut args = normalize_facade_arguments(arguments_json);
    let redacted_arguments = redact_json_value(&Value::Object(args.clone()));
    let mut guard = runs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if tool == ComputerFacadeTool::Observe {
        let run = guard
            .entry(session_id.to_string())
            .and_modify(|run| {
                if run.stopped {
                    *run = new_run();
                }
                run.actions_since_observe = 0;
            })
            .or_insert_with(new_run);
        args.insert("runId".to_string(), json!(run.run_id));
        return Ok(PreparedComputerUseCall {
            run_id: run.run_id.clone(),
            tool,
            backend_tool_name: tool.backend_mcp_name(),
            backend_arguments_json: Value::Object(args).to_string(),
            redacted_arguments,
        });
    }

    if tool == ComputerFacadeTool::Stop {
        let run = guard.entry(session_id.to_string()).or_insert_with(new_run);
        run.stopped = true;
        args.insert("runId".to_string(), json!(run.run_id));
        return Ok(PreparedComputerUseCall {
            run_id: run.run_id.clone(),
            tool,
            backend_tool_name: tool.backend_mcp_name(),
            backend_arguments_json: Value::Object(args).to_string(),
            redacted_arguments,
        });
    }

    let Some(run) = guard.get_mut(session_id) else {
        return Err(ComputerUsePolicyError {
            run_id: None,
            code: "observe_required",
            message: "Computer Use actions require a fresh computer_observe first.".to_string(),
            requires_observe: true,
            requires_confirmation: false,
        });
    };

    if run.stopped {
        return Err(ComputerUsePolicyError {
            run_id: Some(run.run_id.clone()),
            code: "run_stopped",
            message: "Computer Use run has been stopped; call computer_observe to start a new run."
                .to_string(),
            requires_observe: true,
            requires_confirmation: false,
        });
    }

    if action_requires_observe(tool) {
        let Some(expected_observation_id) = run.last_observation_id.clone() else {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "observe_required",
                message: "Computer Use actions require a fresh computer_observe first.".to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        };
        let Some(observed_at) = run.last_observed_at else {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "observe_required",
                message: "Computer Use actions require a fresh computer_observe first.".to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        };
        if Utc::now().signed_duration_since(observed_at).num_seconds() > OBSERVATION_TTL_SECS {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "observation_expired",
                message: format!(
                    "Computer Use observation is older than {OBSERVATION_TTL_SECS}s; observe again before acting."
                ),
                requires_observe: true,
                requires_confirmation: false,
            });
        }
        let Some(actual_observation_id) = args.get("observationId").and_then(value_to_string)
        else {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "missing_observation_id",
                message: "Computer Use actions must include the observationId returned by the latest computer_observe.".to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        };
        if actual_observation_id != expected_observation_id {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "stale_observation_id",
                message: "Computer Use action referenced an old observationId; observe again before acting.".to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        }
        let Some(expected_window_id) = run.target_window_id.clone() else {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "target_window_missing",
                message:
                    "Computer Use could not identify a target window from the latest observation."
                        .to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        };
        let Some(actual_window_id) = args.get("targetWindowId").and_then(value_to_string) else {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "missing_target_window_id",
                message:
                    "Computer Use actions must include targetWindowId from the latest observation."
                        .to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        };
        if actual_window_id != expected_window_id {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "target_window_changed",
                message:
                    "Computer Use action targetWindowId does not match the locked target window."
                        .to_string(),
                requires_observe: true,
                requires_confirmation: false,
            });
        }
        if tool == ComputerFacadeTool::Click || tool == ComputerFacadeTool::Drag {
            if let Some([bx, by, bw, bh]) = run.target_bounds {
                let points = if tool == ComputerFacadeTool::Click {
                    vec![(
                        args.get("x").and_then(Value::as_f64),
                        args.get("y").and_then(Value::as_f64),
                    )]
                } else {
                    vec![
                        (
                            args.get("startX").and_then(Value::as_f64),
                            args.get("startY").and_then(Value::as_f64),
                        ),
                        (
                            args.get("endX").and_then(Value::as_f64),
                            args.get("endY").and_then(Value::as_f64),
                        ),
                    ]
                };
                for (x, y) in points {
                    if let (Some(x), Some(y)) = (x, y) {
                        if x < bx || y < by || x > bx + bw || y > by + bh {
                            let (code, message) = if tool == ComputerFacadeTool::Click {
                                (
                                    "point_outside_target_window",
                                    "Computer Use click coordinates are outside the locked target window.",
                                )
                            } else {
                                (
                                    "drag_point_outside_target_window",
                                    "Computer Use drag coordinates are outside the locked target window.",
                                )
                            };
                            return Err(ComputerUsePolicyError {
                                run_id: Some(run.run_id.clone()),
                                code,
                                message: message.to_string(),
                                requires_observe: true,
                                requires_confirmation: false,
                            });
                        }
                    }
                }
            }
        }
        if run.actions_since_observe >= MAX_ACTIONS_BEFORE_OBSERVE {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "observe_refresh_required",
                message: format!(
                    "Computer Use requires a new observation after {MAX_ACTIONS_BEFORE_OBSERVE} actions."
                ),
                requires_observe: true,
                requires_confirmation: false,
            });
        }
        if run.total_actions >= MAX_TOTAL_ACTIONS {
            return Err(ComputerUsePolicyError {
                run_id: Some(run.run_id.clone()),
                code: "action_budget_exhausted",
                message: format!(
                    "Computer Use reached the {MAX_TOTAL_ACTIONS}-action budget for this run."
                ),
                requires_observe: false,
                requires_confirmation: true,
            });
        }
    }

    args.insert("runId".to_string(), json!(run.run_id));
    Ok(PreparedComputerUseCall {
        run_id: run.run_id.clone(),
        tool,
        backend_tool_name: tool.backend_mcp_name(),
        backend_arguments_json: Value::Object(args).to_string(),
        redacted_arguments,
    })
}

pub fn record_facade_result(
    project_root: &Path,
    session_id: &str,
    prepared: &PreparedComputerUseCall,
    ok: bool,
    backend_result: &Value,
) {
    let safe_result = redact_json_value(backend_result);
    if ok {
        let mut guard = runs()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(run) = guard.get_mut(session_id) {
            match prepared.tool {
                ComputerFacadeTool::Observe => {
                    run.last_observation_id = extract_observation_id(&safe_result)
                        .or_else(|| Some(format!("obs_{}", uuid::Uuid::new_v4())));
                    run.last_observed_at = Some(Utc::now());
                    run.target_window_id = extract_target_window_id(&safe_result);
                    run.target_bounds = extract_target_bounds(&safe_result);
                    run.target_app_name = extract_target_app_name(&safe_result);
                    run.target_bundle_id = extract_target_bundle_id(&safe_result);
                    run.actions_since_observe = 0;
                }
                ComputerFacadeTool::Click
                | ComputerFacadeTool::ClickElement
                | ComputerFacadeTool::Drag
                | ComputerFacadeTool::TypeText
                | ComputerFacadeTool::KeyPress
                | ComputerFacadeTool::Scroll
                | ComputerFacadeTool::Shortcut => {
                    run.actions_since_observe = run.actions_since_observe.saturating_add(1);
                    run.total_actions = run.total_actions.saturating_add(1);
                }
                ComputerFacadeTool::Stop => {
                    run.stopped = true;
                }
                ComputerFacadeTool::SetTarget => {}
            }
        }
    }

    let entry = json!({
        "ts": Utc::now().to_rfc3339(),
        "sessionId": session_id,
        "runId": prepared.run_id,
        "tool": prepared.tool.model_name(),
        "backendTool": prepared.backend_tool_name,
        "ok": ok,
        "arguments": prepared.redacted_arguments,
        "result": safe_result,
    });
    append_audit_entry(project_root, session_id, &prepared.run_id, &entry);
}

pub fn record_policy_rejection(
    project_root: &Path,
    session_id: &str,
    tool: ComputerFacadeTool,
    arguments_json: &str,
    error: &ComputerUsePolicyError,
) {
    let redacted_arguments =
        redact_json_value(&Value::Object(normalize_facade_arguments(arguments_json)));
    let run_id = error
        .run_id
        .clone()
        .unwrap_or_else(|| "no_active_run".to_string());
    let entry = json!({
        "ts": Utc::now().to_rfc3339(),
        "sessionId": session_id,
        "runId": run_id,
        "tool": tool.model_name(),
        "ok": false,
        "arguments": redacted_arguments,
        "policyError": {
            "code": error.code,
            "message": error.message,
            "requiresObserve": error.requires_observe,
            "requiresConfirmation": error.requires_confirmation,
        },
    });
    append_audit_entry(project_root, session_id, &run_id, &entry);
}

pub fn sanitize_backend_result_for_model(value: &Value) -> Value {
    redact_json_value(value)
}

pub fn app_policy_violation_from_backend_result(
    settings: &ComputerUseSettings,
    value: &Value,
    require_target_identity: bool,
) -> Option<ComputerUseAppPolicyViolation> {
    let Some(target) = extract_target_identity(value) else {
        return require_target_identity.then(|| ComputerUseAppPolicyViolation {
            target: ComputerUseTargetIdentity::default(),
            allowed_apps: settings.allowed_apps.clone(),
            reason: "target_identity_missing",
        });
    };
    if settings.allows_target(&target) {
        return None;
    }
    Some(ComputerUseAppPolicyViolation {
        target,
        allowed_apps: settings.allowed_apps.clone(),
        reason: "target_not_allowed",
    })
}

pub fn app_not_allowed_output(
    run_id: &str,
    facade_tool: ComputerFacadeTool,
    backend_tool_name: &str,
    violation: &ComputerUseAppPolicyViolation,
) -> Value {
    json!({
        "ok": false,
        "error": "app_not_allowed",
        "message": "Computer Use target is not in Settings → Computer Use allowed apps.",
        "requiresObserve": true,
        "requiresConfirmation": true,
        "requiresSettingsChange": true,
        "runId": run_id,
        "facadeTool": facade_tool.model_name(),
        "backendTool": backend_tool_name,
        "target": violation.target,
        "allowedApps": violation.allowed_apps,
        "reason": violation.reason,
    })
}

pub fn backend_validation_allows_action(value: &Value) -> bool {
    let parsed = mcp_text_payload(value).unwrap_or_else(|| value.clone());
    let ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let safe_to_act = parsed
        .get("safeToAct")
        .and_then(Value::as_bool)
        .unwrap_or(ok);
    let target_visible = parsed
        .get("targetVisible")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let occluded = parsed
        .get("occluded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ok && safe_to_act && target_visible && !occluded
}

pub fn stop_active_run(session_id: &str) -> ComputerUseStopStatus {
    let mut guard = runs()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(run) = guard.get_mut(session_id) {
        run.stopped = true;
        return ComputerUseStopStatus {
            stopped: true,
            run_id: Some(run.run_id.clone()),
            message: "Computer Use run stopped in Omiga core.".to_string(),
            backend_stopped: None,
            backend_message: None,
            backend_error: None,
        };
    }

    ComputerUseStopStatus {
        stopped: false,
        run_id: None,
        message: "No active Computer Use run for this session.".to_string(),
        backend_stopped: None,
        backend_message: None,
        backend_error: None,
    }
}

pub fn summarize_audit(project_root: &Path) -> std::io::Result<ComputerUseAuditSummary> {
    let audit_root = audit_root_dir(project_root);
    let runs_root = audit_root.join("runs");
    let mut run_count = 0usize;
    let mut flow_count = 0usize;
    let mut action_count = 0usize;
    let mut result_counts = ComputerUseResultCounts::default();
    let mut bytes = 0u64;

    if runs_root.exists() {
        summarize_audit_dir(
            &runs_root,
            &mut run_count,
            &mut flow_count,
            &mut action_count,
            &mut result_counts,
            &mut bytes,
        )?;
    }

    Ok(ComputerUseAuditSummary {
        audit_root: audit_root.to_string_lossy().into_owned(),
        runs_root: runs_root.to_string_lossy().into_owned(),
        run_count,
        flow_count,
        action_count,
        result_ok_count: result_counts.ok,
        result_blocked_count: result_counts.blocked,
        result_needs_attention_count: result_counts.needs_attention,
        result_stopped_count: result_counts.stopped,
        result_unknown_count: result_counts.unknown,
        bytes,
        retention_days: None,
        pruned_run_count: 0,
        pruned_temp_dir_count: 0,
        pruned_bytes: 0,
    })
}

pub fn summarize_audit_with_retention(
    project_root: &Path,
    retention_days: u32,
) -> std::io::Result<ComputerUseAuditSummary> {
    let retention_days = normalize_retention_days(retention_days);
    let pruned = prune_audit_retention(project_root, retention_days)?;
    let mut summary = summarize_audit(project_root)?;
    summary.retention_days = Some(retention_days);
    summary.pruned_run_count = pruned.pruned_run_count;
    summary.pruned_temp_dir_count = pruned.pruned_temp_dir_count;
    summary.pruned_bytes = pruned.pruned_bytes;
    Ok(summary)
}

pub fn prune_audit_retention(
    project_root: &Path,
    retention_days: u32,
) -> std::io::Result<ComputerUseAuditPruneSummary> {
    let retention_days = normalize_retention_days(retention_days);
    let cutoff = Utc::now() - ChronoDuration::days(retention_days as i64);
    let mut summary = ComputerUseAuditPruneSummary::default();
    let runs_root = audit_root_dir(project_root).join("runs");
    if runs_root.exists() {
        prune_old_run_dirs(&runs_root, cutoff, &mut summary, true)?;
    }
    prune_old_temp_screenshot_dirs(retention_days, &mut summary)?;
    Ok(summary)
}

pub fn clear_audit_runs(project_root: &Path) -> std::io::Result<ComputerUseAuditSummary> {
    let before = summarize_audit(project_root)?;
    let runs_root = PathBuf::from(&before.runs_root);
    if runs_root.exists() {
        std::fs::remove_dir_all(&runs_root)?;
    }
    Ok(before)
}

fn extract_observation_id(value: &Value) -> Option<String> {
    value
        .get("observationId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            mcp_text_payload(value).and_then(|parsed| {
                parsed
                    .get("observationId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
}

fn extract_target_identity(value: &Value) -> Option<ComputerUseTargetIdentity> {
    let parsed = mcp_text_payload(value).unwrap_or_else(|| value.clone());
    let app_name = parsed
        .get("frontmostApp")
        .or_else(|| parsed.pointer("/target/appName"))
        .or_else(|| parsed.pointer("/currentTarget/appName"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let bundle_id = parsed
        .pointer("/target/bundleId")
        .or_else(|| parsed.pointer("/currentTarget/bundleId"))
        .or_else(|| parsed.get("bundleId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let window_title = parsed
        .get("activeWindowTitle")
        .or_else(|| parsed.pointer("/target/windowTitle"))
        .or_else(|| parsed.pointer("/currentTarget/windowTitle"))
        .and_then(Value::as_str)
        .map(str::to_string);

    if app_name.is_none() && bundle_id.is_none() {
        return None;
    }
    Some(ComputerUseTargetIdentity {
        app_name,
        bundle_id,
        window_title,
    })
}

fn extract_target_app_name(value: &Value) -> Option<String> {
    extract_target_identity(value).and_then(|target| target.app_name)
}

fn extract_target_window_id(value: &Value) -> Option<String> {
    let parsed = mcp_text_payload(value).unwrap_or_else(|| value.clone());
    parsed
        .pointer("/target/windowId")
        .or_else(|| parsed.get("targetWindowId"))
        .and_then(value_to_string)
}

fn extract_target_bundle_id(value: &Value) -> Option<String> {
    let parsed = mcp_text_payload(value).unwrap_or_else(|| value.clone());
    parsed
        .pointer("/target/bundleId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn extract_target_bounds(value: &Value) -> Option<[f64; 4]> {
    let parsed = mcp_text_payload(value).unwrap_or_else(|| value.clone());
    let arr = parsed.pointer("/target/bounds")?.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    Some([
        arr[0].as_f64()?,
        arr[1].as_f64()?,
        arr[2].as_f64()?,
        arr[3].as_f64()?,
    ])
}

fn mcp_text_payload(value: &Value) -> Option<Value> {
    value
        .pointer("/content/0/text")
        .or_else(|| value.pointer("/backendResult/content/0/text"))
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn append_audit_entry(project_root: &Path, session_id: &str, run_id: &str, entry: &Value) {
    let dir = audit_run_dir(project_root, session_id, run_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let run_path = dir.join("run.json");
    let mut run = read_run_document(&run_path).unwrap_or_else(|| {
        let started_at = runs()
            .lock()
            .ok()
            .and_then(|guard| guard.get(session_id).map(|r| r.started_at.to_rfc3339()))
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        json!({
            "runId": run_id,
            "sessionId": session_id,
            "startedAt": started_at,
            "auditVersion": 2,
        })
    });
    update_operation_flow(&mut run, entry);
    let _ = std::fs::write(
        &run_path,
        serde_json::to_string_pretty(&run).unwrap_or_else(|_| "{}".to_string()),
    );

    if let Ok(mut line) = serde_json::to_string(entry) {
        line.push('\n');
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("actions.jsonl"))
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(line.as_bytes())
            });
    }
}

fn read_run_document(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

fn update_operation_flow(run: &mut Value, entry: &Value) {
    let Some(run_map) = run.as_object_mut() else {
        return;
    };
    let ts = entry
        .get("ts")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    run_map.insert("auditVersion".to_string(), json!(2));
    run_map.insert("updatedAt".to_string(), json!(ts.clone()));

    let flow = run_map.entry("flow".to_string()).or_insert_with(|| {
        json!({
            "flowVersion": 1,
            "status": "active",
            "actionCount": 0,
            "okActionCount": 0,
            "failedActionCount": 0,
            "policyRejectionCount": 0,
            "toolCounts": {},
        })
    });
    let Some(flow_map) = flow.as_object_mut() else {
        return;
    };
    flow_map.insert("flowVersion".to_string(), json!(1));
    flow_map
        .entry("firstActionAt".to_string())
        .or_insert_with(|| json!(ts.clone()));
    flow_map.insert("lastActionAt".to_string(), json!(ts));

    increment_counter(flow_map, "actionCount");
    if entry.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        increment_counter(flow_map, "okActionCount");
    } else {
        increment_counter(flow_map, "failedActionCount");
    }
    if entry.get("policyError").is_some() {
        increment_counter(flow_map, "policyRejectionCount");
    }

    let tool = entry
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    flow_map.insert("lastTool".to_string(), json!(tool.clone()));
    if let Some(tool_counts) = flow_map
        .entry("toolCounts".to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
    {
        let next = tool_counts
            .get(&tool)
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .saturating_add(1);
        tool_counts.insert(tool, json!(next));
    }

    if let Some(result) = entry.get("result") {
        if let Some(target) = extract_target_identity(result) {
            flow_map.insert("target".to_string(), json!(target));
        }
    }

    if entry.get("policyError").is_some() {
        flow_map.insert("status".to_string(), json!("blocked"));
        flow_map.insert("lastError".to_string(), entry["policyError"].clone());
    } else if entry.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        if entry.get("tool").and_then(Value::as_str) == Some("computer_stop") {
            flow_map.insert("status".to_string(), json!("stopped"));
        } else {
            flow_map.insert("status".to_string(), json!("active"));
        }
        flow_map.remove("lastError");
    } else {
        flow_map.insert("status".to_string(), json!("needs_attention"));
        if let Some(error) = entry.get("result").and_then(|result| result.get("error")) {
            flow_map.insert("lastError".to_string(), json!(error));
        }
    }
}

fn increment_counter(map: &mut Map<String, Value>, key: &str) {
    let next = map
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    map.insert(key.to_string(), json!(next));
}

fn summarize_audit_dir(
    dir: &Path,
    run_count: &mut usize,
    flow_count: &mut usize,
    action_count: &mut usize,
    result_counts: &mut ComputerUseResultCounts,
    bytes: &mut u64,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            summarize_audit_dir(
                &path,
                run_count,
                flow_count,
                action_count,
                result_counts,
                bytes,
            )?;
            continue;
        }

        *bytes = bytes.saturating_add(metadata.len());
        if path.file_name().and_then(|s| s.to_str()) == Some("run.json") {
            *run_count = run_count.saturating_add(1);
            if let Some(flow) =
                read_run_document(&path).and_then(|value| value.get("flow").cloned())
            {
                *flow_count = flow_count.saturating_add(1);
                result_counts.add_flow(&flow);
            }
        } else if path.file_name().and_then(|s| s.to_str()) == Some("actions.jsonl") {
            let content = std::fs::read_to_string(&path)?;
            *action_count = action_count.saturating_add(
                content
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count(),
            );
        }
    }
    Ok(())
}

#[derive(Default)]
struct ComputerUseResultCounts {
    ok: usize,
    blocked: usize,
    needs_attention: usize,
    stopped: usize,
    unknown: usize,
}

impl ComputerUseResultCounts {
    fn add_flow(&mut self, flow: &Value) {
        match classify_flow_result(flow) {
            ComputerUseResultStatus::Ok => self.ok = self.ok.saturating_add(1),
            ComputerUseResultStatus::Blocked => {
                self.blocked = self.blocked.saturating_add(1);
            }
            ComputerUseResultStatus::NeedsAttention => {
                self.needs_attention = self.needs_attention.saturating_add(1);
            }
            ComputerUseResultStatus::Stopped => self.stopped = self.stopped.saturating_add(1),
            ComputerUseResultStatus::Unknown => self.unknown = self.unknown.saturating_add(1),
        }
    }
}

enum ComputerUseResultStatus {
    Ok,
    Blocked,
    NeedsAttention,
    Stopped,
    Unknown,
}

fn classify_flow_result(flow: &Value) -> ComputerUseResultStatus {
    let status = flow
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let action_count = flow.get("actionCount").and_then(Value::as_u64).unwrap_or(0);
    let ok_action_count = flow
        .get("okActionCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let failed_action_count = flow
        .get("failedActionCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let policy_rejection_count = flow
        .get("policyRejectionCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    if policy_rejection_count > 0 || status == "blocked" {
        return ComputerUseResultStatus::Blocked;
    }
    if failed_action_count > 0 || status == "needs_attention" {
        return ComputerUseResultStatus::NeedsAttention;
    }
    if status == "stopped" {
        return ComputerUseResultStatus::Stopped;
    }
    if ok_action_count > 0 || (status == "active" && action_count > 0) {
        return ComputerUseResultStatus::Ok;
    }
    ComputerUseResultStatus::Unknown
}

fn normalize_retention_days(days: u32) -> u32 {
    days.clamp(1, 365)
}

fn prune_old_run_dirs(
    dir: &Path,
    cutoff: DateTime<Utc>,
    summary: &mut ComputerUseAuditPruneSummary,
    keep_root: bool,
) -> std::io::Result<bool> {
    let mut is_empty = true;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            if path.join("run.json").exists() {
                if run_reference_time(&path, &metadata)
                    .map(|reference| reference < cutoff)
                    .unwrap_or(false)
                {
                    summary.pruned_bytes =
                        summary.pruned_bytes.saturating_add(directory_size(&path)?);
                    std::fs::remove_dir_all(&path)?;
                    summary.pruned_run_count = summary.pruned_run_count.saturating_add(1);
                    continue;
                }
                is_empty = false;
                continue;
            }

            if prune_old_run_dirs(&path, cutoff, summary, false)? {
                let _ = std::fs::remove_dir(&path);
            } else {
                is_empty = false;
            }
            continue;
        }
        is_empty = false;
    }
    Ok(!keep_root && is_empty)
}

fn run_reference_time(dir: &Path, metadata: &std::fs::Metadata) -> Option<DateTime<Utc>> {
    let run = read_run_document(&dir.join("run.json"));
    for pointer in ["/updatedAt", "/flow/lastActionAt", "/startedAt"] {
        if let Some(parsed) = run
            .as_ref()
            .and_then(|value| value.pointer(pointer))
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_utc)
        {
            return Some(parsed);
        }
    }
    metadata.modified().ok().map(DateTime::<Utc>::from)
}

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn prune_old_temp_screenshot_dirs(
    retention_days: u32,
    summary: &mut ComputerUseAuditPruneSummary,
) -> std::io::Result<()> {
    let root = std::env::temp_dir().join("omiga-computer-use");
    if !root.exists() {
        return Ok(());
    }
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(
            u64::from(retention_days).saturating_mul(24 * 60 * 60),
        ))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if !metadata.is_dir() {
            continue;
        }
        if metadata
            .modified()
            .map(|modified| modified < cutoff)
            .unwrap_or(false)
        {
            summary.pruned_bytes = summary.pruned_bytes.saturating_add(directory_size(&path)?);
            std::fs::remove_dir_all(&path)?;
            summary.pruned_temp_dir_count = summary.pruned_temp_dir_count.saturating_add(1);
        }
    }
    Ok(())
}

fn directory_size(path: &Path) -> std::io::Result<u64> {
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            bytes = bytes.saturating_add(directory_size(&entry.path())?);
        } else {
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok(bytes)
}

fn audit_root_dir(project_root: &Path) -> PathBuf {
    project_root.join(".omiga").join("computer-use")
}

fn audit_run_dir(project_root: &Path, session_id: &str, run_id: &str) -> PathBuf {
    audit_root_dir(project_root)
        .join("runs")
        .join(sanitize_path_segment(session_id))
        .join(sanitize_path_segment(run_id))
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(redact_secrets_in_text(s)),
        Value::Array(items) => Value::Array(items.iter().map(redact_json_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let redacted_value = if secret_key_name(key) {
                        Value::String("[REDACTED]".to_string())
                    } else {
                        redact_json_value(value)
                    };
                    (key.clone(), redacted_value)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn redact_secrets_in_text(text: &str) -> String {
    let mut out = text.to_string();
    for (pattern, replacement) in [
        (
            r"(?is)-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----",
            "[REDACTED_PRIVATE_KEY]",
        ),
        (r"sk-[A-Za-z0-9_-]{12,}", "sk-[REDACTED]"),
        (r"ghp_[A-Za-z0-9_]{12,}", "ghp_[REDACTED]"),
        (r"AKIA[0-9A-Z]{16}", "AKIA[REDACTED]"),
        (
            r"(?i)\b(password|token|api[_-]?key)\s*[:=]\s*[^,\s;]+",
            "$1=[REDACTED]",
        ),
    ] {
        if let Ok(re) = regex::Regex::new(pattern) {
            out = re.replace_all(&out, replacement).into_owned();
        }
    }
    out
}

pub fn value_contains_probable_secret(value: &Value) -> bool {
    match value {
        Value::String(s) => redact_secrets_in_text(s) != *s,
        Value::Array(items) => items.iter().any(value_contains_probable_secret),
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| secret_key_name(key) || value_contains_probable_secret(value)),
        _ => false,
    }
}

fn secret_key_name(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "password"
        || key == "token"
        || key == "api_key"
        || key == "apikey"
        || key == "secret"
        || key.ends_with("_token")
        || key.ends_with("_key")
}

#[cfg(test)]
mod tests {
    use super::{
        app_policy_violation_from_backend_result, backend_validation_allows_action,
        clear_audit_runs, facade_tool_schemas, prepare_facade_call, record_facade_result,
        redact_json_value, redact_secrets_in_text, stop_active_run, summarize_audit,
        summarize_audit_with_retention, value_contains_probable_secret, ComputerFacadeTool,
        ComputerUseMode, ComputerUseSettings,
    };
    use serde_json::json;
    use std::{collections::HashSet, fs};

    #[test]
    fn computer_use_mode_defaults_to_off_for_missing_or_unknown_values() {
        assert_eq!(ComputerUseMode::from_request(None), ComputerUseMode::Off);
        assert_eq!(
            ComputerUseMode::from_request(Some("")),
            ComputerUseMode::Off
        );
        assert_eq!(
            ComputerUseMode::from_request(Some("always")),
            ComputerUseMode::Off
        );
    }

    #[test]
    fn computer_use_mode_accepts_task_and_session_case_insensitively() {
        assert_eq!(
            ComputerUseMode::from_request(Some("task")),
            ComputerUseMode::Task
        );
        assert_eq!(
            ComputerUseMode::from_request(Some(" TASK ")),
            ComputerUseMode::Task
        );
        assert_eq!(
            ComputerUseMode::from_request(Some("session")),
            ComputerUseMode::Session
        );
        assert_eq!(
            ComputerUseMode::from_request(Some("Session")),
            ComputerUseMode::Session
        );
    }

    #[test]
    fn computer_facade_tools_map_to_reserved_backend_names() {
        let tool = ComputerFacadeTool::from_model_name("computer_type").unwrap();
        assert_eq!(tool.backend_tool_name(), "type_text");
        assert_eq!(tool.backend_mcp_name(), "mcp__computer__type_text");

        let key_tool = ComputerFacadeTool::from_model_name("computer_key").unwrap();
        assert_eq!(key_tool.backend_tool_name(), "key_press");
        assert_eq!(key_tool.backend_mcp_name(), "mcp__computer__key_press");

        let drag_tool = ComputerFacadeTool::from_model_name("computer_drag").unwrap();
        assert_eq!(drag_tool.backend_tool_name(), "drag");
        assert_eq!(drag_tool.backend_mcp_name(), "mcp__computer__drag");

        let scroll_tool = ComputerFacadeTool::from_model_name("computer_scroll").unwrap();
        assert_eq!(scroll_tool.backend_tool_name(), "scroll");
        assert_eq!(scroll_tool.backend_mcp_name(), "mcp__computer__scroll");

        let shortcut_tool = ComputerFacadeTool::from_model_name("computer_shortcut").unwrap();
        assert_eq!(shortcut_tool.backend_tool_name(), "shortcut");
        assert_eq!(shortcut_tool.backend_mcp_name(), "mcp__computer__shortcut");
    }

    #[test]
    fn computer_facade_schema_catalog_contains_model_visible_names_only() {
        let names: HashSet<_> = facade_tool_schemas()
            .into_iter()
            .map(|schema| schema.name)
            .collect();

        assert!(names.contains("computer_observe"));
        assert!(names.contains("computer_click"));
        assert!(names.contains("computer_click_element"));
        assert!(names.contains("computer_drag"));
        assert!(names.contains("computer_type"));
        assert!(names.contains("computer_key"));
        assert!(names.contains("computer_scroll"));
        assert!(names.contains("computer_shortcut"));
        assert!(names.contains("computer_stop"));
        assert!(
            !names.contains("mcp__computer__observe"),
            "raw MCP backend tools must never be model-visible facade schemas"
        );
    }

    #[test]
    fn computer_facade_action_schemas_require_target_lock_fields() {
        let schemas = facade_tool_schemas();
        for tool_name in [
            "computer_click",
            "computer_click_element",
            "computer_drag",
            "computer_type",
            "computer_key",
            "computer_scroll",
            "computer_shortcut",
        ] {
            let schema = schemas
                .iter()
                .find(|schema| schema.name == tool_name)
                .expect("schema");
            let required = schema
                .parameters
                .get("required")
                .and_then(serde_json::Value::as_array)
                .expect("required array")
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<HashSet<_>>();
            assert!(
                required.contains("observationId"),
                "{tool_name} must require observationId"
            );
            assert!(
                required.contains("targetWindowId"),
                "{tool_name} must require targetWindowId"
            );
        }
    }

    #[test]
    fn computer_observe_schema_advertises_optional_visual_text() {
        let schemas = facade_tool_schemas();
        let schema = schemas
            .iter()
            .find(|schema| schema.name == "computer_observe")
            .expect("computer_observe schema");
        assert_eq!(
            schema.parameters["properties"]["extractVisualText"]["type"],
            "boolean"
        );
        let required = schema
            .parameters
            .get("required")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(
            !required
                .iter()
                .any(|value| value.as_str() == Some("extractVisualText")),
            "visual OCR must remain opt-in but not required"
        );
    }

    #[test]
    fn computer_click_schema_does_not_advertise_unsupported_buttons() {
        let schemas = facade_tool_schemas();
        let schema = schemas
            .iter()
            .find(|schema| schema.name == "computer_click")
            .expect("computer_click schema");
        let buttons = schema
            .parameters
            .pointer("/properties/button/enum")
            .and_then(serde_json::Value::as_array)
            .expect("button enum")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(buttons, vec!["left"]);
    }

    #[test]
    fn computer_drag_schema_is_bounded_to_left_button_coordinates() {
        let schemas = facade_tool_schemas();
        let schema = schemas
            .iter()
            .find(|schema| schema.name == "computer_drag")
            .expect("computer_drag schema");
        let required = schema
            .parameters
            .get("required")
            .and_then(serde_json::Value::as_array)
            .expect("required array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            required,
            vec![
                "observationId",
                "targetWindowId",
                "startX",
                "startY",
                "endX",
                "endY"
            ]
        );
        let buttons = schema
            .parameters
            .pointer("/properties/button/enum")
            .and_then(serde_json::Value::as_array)
            .expect("button enum")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(buttons, vec!["left"]);
        assert_eq!(
            schema.parameters["properties"]["durationMs"]["maximum"],
            2000
        );
    }

    #[test]
    fn computer_key_schema_advertises_only_fixed_supported_keys() {
        let schemas = facade_tool_schemas();
        let schema = schemas
            .iter()
            .find(|schema| schema.name == "computer_key")
            .expect("computer_key schema");
        let keys = schema
            .parameters
            .pointer("/properties/key/enum")
            .and_then(serde_json::Value::as_array)
            .expect("key enum")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            vec![
                "enter",
                "return",
                "tab",
                "escape",
                "backspace",
                "delete",
                "arrow_up",
                "arrow_down",
                "arrow_left",
                "arrow_right",
                "page_up",
                "page_down",
                "home",
                "end",
                "space",
            ]
        );
    }

    #[test]
    fn computer_scroll_schema_advertises_only_fixed_directions() {
        let schemas = facade_tool_schemas();
        let schema = schemas
            .iter()
            .find(|schema| schema.name == "computer_scroll")
            .expect("computer_scroll schema");
        let directions = schema
            .parameters
            .pointer("/properties/direction/enum")
            .and_then(serde_json::Value::as_array)
            .expect("direction enum")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(directions, vec!["up", "down", "left", "right"]);
    }

    #[test]
    fn computer_shortcut_schema_advertises_only_fixed_shortcuts() {
        let schemas = facade_tool_schemas();
        let schema = schemas
            .iter()
            .find(|schema| schema.name == "computer_shortcut")
            .expect("computer_shortcut schema");
        let shortcuts = schema
            .parameters
            .pointer("/properties/shortcut/enum")
            .and_then(serde_json::Value::as_array)
            .expect("shortcut enum")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(shortcuts, vec!["select_all", "undo", "redo"]);
    }

    #[test]
    fn computer_use_backend_validation_blocks_unsafe_or_occluded_targets() {
        assert!(backend_validation_allows_action(&json!({
            "ok": true,
            "targetVisible": true,
            "occluded": false,
            "safeToAct": true
        })));
        assert!(!backend_validation_allows_action(&json!({
            "ok": true,
            "targetVisible": true,
            "occluded": true,
            "safeToAct": true
        })));
        assert!(!backend_validation_allows_action(&json!({
            "content": [{
                "type": "text",
                "text": "{\"ok\":true,\"targetVisible\":true,\"occluded\":false,\"safeToAct\":false}"
            }]
        })));
    }

    #[test]
    fn computer_use_settings_enforce_allowed_app_or_bundle() {
        let settings = ComputerUseSettings::from_stored_json(Some(
            r#"{"allowedApps":["Omiga","com.example.allowed"],"logRetentionDays":999,"saveScreenshots":true}"#,
        ));
        assert_eq!(settings.log_retention_days, 365);
        assert!(settings.save_screenshots);

        let allowed_by_app = json!({
            "frontmostApp": "Omiga",
            "target": { "bundleId": "com.other", "windowId": 1 }
        });
        assert!(
            app_policy_violation_from_backend_result(&settings, &allowed_by_app, true).is_none()
        );

        let allowed_by_bundle = json!({
            "frontmostApp": "Other",
            "target": { "bundleId": "com.example.allowed", "windowId": 2 }
        });
        assert!(
            app_policy_violation_from_backend_result(&settings, &allowed_by_bundle, true).is_none()
        );

        let blocked = json!({
            "frontmostApp": "Mail",
            "target": { "bundleId": "com.apple.mail", "windowId": 3 }
        });
        let violation =
            app_policy_violation_from_backend_result(&settings, &blocked, true).unwrap();
        assert_eq!(violation.target.app_name.as_deref(), Some("Mail"));
        assert_eq!(
            violation.target.bundle_id.as_deref(),
            Some("com.apple.mail")
        );
        assert_eq!(violation.reason, "target_not_allowed");

        let missing_identity = json!({ "ok": true, "safeToAct": true });
        assert!(
            app_policy_violation_from_backend_result(&settings, &missing_identity, false).is_none()
        );
        let violation =
            app_policy_violation_from_backend_result(&settings, &missing_identity, true).unwrap();
        assert_eq!(violation.reason, "target_identity_missing");

        let current_target = json!({
            "ok": true,
            "currentTarget": {
                "appName": "Terminal",
                "bundleId": "com.apple.Terminal",
                "windowTitle": "zsh"
            }
        });
        let violation =
            app_policy_violation_from_backend_result(&settings, &current_target, true).unwrap();
        assert_eq!(violation.target.app_name.as_deref(), Some("Terminal"));
        assert_eq!(
            violation.target.bundle_id.as_deref(),
            Some("com.apple.Terminal")
        );
    }

    #[test]
    fn computer_use_policy_requires_observe_and_action_refresh() {
        let session_id = format!("s-{}", uuid::Uuid::new_v4());
        let click =
            prepare_facade_call(&session_id, ComputerFacadeTool::Click, r#"{"x":10,"y":20}"#)
                .unwrap_err();
        assert_eq!(click.code, "observe_required");
        assert!(click.requires_observe);

        let observe = prepare_facade_call(&session_id, ComputerFacadeTool::Observe, "{}").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        record_facade_result(
            tmp.path(),
            &session_id,
            &observe,
            true,
            &json!({
                "observationId": "obs_test",
                "target": {
                    "windowId": 1,
                    "bundleId": "com.omiga.test",
                    "bounds": [0, 0, 100, 100]
                }
            }),
        );

        for _ in 0..5 {
            let prepared = prepare_facade_call(
                &session_id,
                ComputerFacadeTool::Click,
                r#"{"observationId":"obs_test","targetWindowId":1,"x":10,"y":20}"#,
            )
            .unwrap();
            record_facade_result(
                tmp.path(),
                &session_id,
                &prepared,
                true,
                &json!({"ok": true}),
            );
        }

        let refresh = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Click,
            r#"{"observationId":"obs_test","targetWindowId":1,"x":10,"y":20}"#,
        )
        .unwrap_err();
        assert_eq!(refresh.code, "observe_refresh_required");
        assert!(refresh.requires_observe);
    }

    #[test]
    fn computer_use_stop_blocks_followup_actions_until_observe() {
        let session_id = format!("s-{}", uuid::Uuid::new_v4());
        let observe = prepare_facade_call(&session_id, ComputerFacadeTool::Observe, "{}").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        record_facade_result(
            tmp.path(),
            &session_id,
            &observe,
            true,
            &json!({
                "observationId": "obs_stop",
                "target": {
                    "windowId": 1,
                    "bundleId": "com.omiga.test",
                    "bounds": [0, 0, 100, 100]
                }
            }),
        );
        let stop = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Stop,
            r#"{"reason":"test"}"#,
        )
        .unwrap();
        record_facade_result(tmp.path(), &session_id, &stop, true, &json!({"ok": true}));

        let blocked = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::TypeText,
            r#"{"observationId":"obs_stop","targetWindowId":1,"text":"hello"}"#,
        )
        .unwrap_err();
        assert_eq!(blocked.code, "run_stopped");
        assert!(blocked.requires_observe);
    }

    #[test]
    fn computer_type_disables_clipboard_fallback_for_probable_secret() {
        let session_id = format!("s-{}", uuid::Uuid::new_v4());
        let observe = prepare_facade_call(&session_id, ComputerFacadeTool::Observe, "{}").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        record_facade_result(
            tmp.path(),
            &session_id,
            &observe,
            true,
            &json!({
                "observationId": "obs_type",
                "target": {
                    "windowId": 1,
                    "bundleId": "com.omiga.test",
                    "bounds": [0, 0, 100, 100]
                }
            }),
        );

        let settings = ComputerUseSettings::default();
        let mut normal = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::TypeText,
            r#"{"observationId":"obs_type","targetWindowId":1,"text":"hello"}"#,
        )
        .unwrap();
        normal.inject_settings(&settings);
        let normal_args: serde_json::Value =
            serde_json::from_str(&normal.backend_arguments_json).unwrap();
        assert_eq!(normal_args["allowClipboardFallback"], true);

        let mut secret = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::TypeText,
            r#"{"observationId":"obs_type","targetWindowId":1,"text":"token=ghp_1234567890abcdef"}"#,
        )
        .unwrap();
        secret.inject_settings(&settings);
        let secret_args: serde_json::Value =
            serde_json::from_str(&secret.backend_arguments_json).unwrap();
        assert_eq!(secret_args["allowClipboardFallback"], false);
        assert_eq!(
            secret_args["clipboardFallbackPolicy"],
            "disabled_for_probable_secret"
        );
    }

    #[test]
    fn computer_use_ui_stop_and_audit_summary_are_project_scoped() {
        let session_id = format!("s-{}", uuid::Uuid::new_v4());
        let observe = prepare_facade_call(&session_id, ComputerFacadeTool::Observe, "{}").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        record_facade_result(
            tmp.path(),
            &session_id,
            &observe,
            true,
            &json!({
                "observationId": "obs_ui_stop",
                "target": {
                    "windowId": 1,
                    "bundleId": "com.omiga.test",
                    "bounds": [0, 0, 100, 100]
                }
            }),
        );

        let summary = summarize_audit(tmp.path()).unwrap();
        assert_eq!(summary.run_count, 1);
        assert_eq!(summary.flow_count, 1);
        assert_eq!(summary.action_count, 1);
        assert_eq!(summary.result_ok_count, 1);
        assert_eq!(summary.result_blocked_count, 0);
        assert_eq!(summary.result_needs_attention_count, 0);
        assert!(summary.runs_root.contains(".omiga"));
        let run_doc_path = tmp
            .path()
            .join(".omiga")
            .join("computer-use")
            .join("runs")
            .join(&session_id)
            .join(&observe.run_id)
            .join("run.json");
        let run_doc = fs::read_to_string(run_doc_path).unwrap();
        let run_doc: serde_json::Value = serde_json::from_str(&run_doc).unwrap();
        assert_eq!(run_doc["auditVersion"], 2);
        assert_eq!(run_doc["flow"]["actionCount"], 1);
        assert_eq!(run_doc["flow"]["target"]["bundleId"], "com.omiga.test");

        let stopped = stop_active_run(&session_id);
        assert!(stopped.stopped);
        assert_eq!(stopped.run_id.as_deref(), Some(observe.run_id.as_str()));

        let blocked = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Click,
            r#"{"observationId":"obs_ui_stop","targetWindowId":1,"x":10,"y":10}"#,
        )
        .unwrap_err();
        assert_eq!(blocked.code, "run_stopped");

        let cleared = clear_audit_runs(tmp.path()).unwrap();
        assert_eq!(cleared.run_count, 1);
        let empty = summarize_audit(tmp.path()).unwrap();
        assert_eq!(empty.run_count, 0);
        assert_eq!(empty.action_count, 0);
    }

    #[test]
    fn computer_use_retention_prunes_old_operation_flows() {
        let tmp = tempfile::tempdir().unwrap();
        let runs_root = tmp.path().join(".omiga").join("computer-use").join("runs");
        let old_run = runs_root.join("s-old").join("curun_old");
        let fresh_run = runs_root.join("s-fresh").join("curun_fresh");
        fs::create_dir_all(&old_run).unwrap();
        fs::create_dir_all(&fresh_run).unwrap();
        let old_started = (chrono::Utc::now() - chrono::Duration::days(4)).to_rfc3339();
        let fresh_started = chrono::Utc::now().to_rfc3339();
        fs::write(
            old_run.join("run.json"),
            json!({
                "runId": "curun_old",
                "sessionId": "s-old",
                "startedAt": old_started,
                "auditVersion": 2,
                "flow": { "actionCount": 1 }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(old_run.join("actions.jsonl"), "{}\n").unwrap();
        fs::write(
            fresh_run.join("run.json"),
            json!({
                "runId": "curun_fresh",
                "sessionId": "s-fresh",
                "startedAt": fresh_started,
                "auditVersion": 2,
                "flow": { "actionCount": 1 }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(fresh_run.join("actions.jsonl"), "{}\n").unwrap();

        let summary = summarize_audit_with_retention(tmp.path(), 2).unwrap();
        assert_eq!(summary.retention_days, Some(2));
        assert_eq!(summary.pruned_run_count, 1);
        assert_eq!(summary.run_count, 1);
        assert_eq!(summary.flow_count, 1);
        assert!(!old_run.exists());
        assert!(fresh_run.exists());
    }

    #[test]
    fn computer_use_audit_summary_counts_result_statuses() {
        let tmp = tempfile::tempdir().unwrap();
        let runs_root = tmp.path().join(".omiga").join("computer-use").join("runs");
        let cases = [
            (
                "ok",
                json!({
                    "status": "active",
                    "actionCount": 2,
                    "okActionCount": 2,
                    "failedActionCount": 0,
                    "policyRejectionCount": 0
                }),
            ),
            (
                "blocked",
                json!({
                    "status": "blocked",
                    "actionCount": 1,
                    "okActionCount": 0,
                    "failedActionCount": 1,
                    "policyRejectionCount": 1
                }),
            ),
            (
                "needs",
                json!({
                    "status": "needs_attention",
                    "actionCount": 1,
                    "okActionCount": 0,
                    "failedActionCount": 1,
                    "policyRejectionCount": 0
                }),
            ),
            (
                "stopped",
                json!({
                    "status": "stopped",
                    "actionCount": 1,
                    "okActionCount": 1,
                    "failedActionCount": 0,
                    "policyRejectionCount": 0
                }),
            ),
            ("unknown", json!({ "actionCount": 0 })),
        ];
        for (name, flow) in cases {
            let run_dir = runs_root
                .join(format!("s-{name}"))
                .join(format!("curun_{name}"));
            fs::create_dir_all(&run_dir).unwrap();
            fs::write(
                run_dir.join("run.json"),
                json!({
                    "runId": format!("curun_{name}"),
                    "sessionId": format!("s-{name}"),
                    "startedAt": chrono::Utc::now().to_rfc3339(),
                    "auditVersion": 2,
                    "flow": flow
                })
                .to_string(),
            )
            .unwrap();
        }

        let summary = summarize_audit(tmp.path()).unwrap();
        assert_eq!(summary.run_count, 5);
        assert_eq!(summary.flow_count, 5);
        assert_eq!(summary.result_ok_count, 1);
        assert_eq!(summary.result_blocked_count, 1);
        assert_eq!(summary.result_needs_attention_count, 1);
        assert_eq!(summary.result_stopped_count, 1);
        assert_eq!(summary.result_unknown_count, 1);
    }

    #[test]
    fn computer_use_policy_locks_observation_and_target_window() {
        let session_id = format!("s-{}", uuid::Uuid::new_v4());
        let observe = prepare_facade_call(&session_id, ComputerFacadeTool::Observe, "{}").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        record_facade_result(
            tmp.path(),
            &session_id,
            &observe,
            true,
            &json!({
                "observationId": "obs_lock",
                "target": {
                    "windowId": 7,
                    "bundleId": "com.omiga.test",
                    "bounds": [10, 10, 80, 80]
                }
            }),
        );

        let missing_obs = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Click,
            r#"{"targetWindowId":7,"x":20,"y":20}"#,
        )
        .unwrap_err();
        assert_eq!(missing_obs.code, "missing_observation_id");

        let wrong_window = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Click,
            r#"{"observationId":"obs_lock","targetWindowId":8,"x":20,"y":20}"#,
        )
        .unwrap_err();
        assert_eq!(wrong_window.code, "target_window_changed");

        let outside = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Click,
            r#"{"observationId":"obs_lock","targetWindowId":7,"x":500,"y":20}"#,
        )
        .unwrap_err();
        assert_eq!(outside.code, "point_outside_target_window");

        let outside_drag = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Drag,
            r#"{"observationId":"obs_lock","targetWindowId":7,"startX":20,"startY":20,"endX":500,"endY":20}"#,
        )
        .unwrap_err();
        assert_eq!(outside_drag.code, "drag_point_outside_target_window");

        let ok_drag = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Drag,
            r#"{"observationId":"obs_lock","targetWindowId":7,"startX":20,"startY":20,"endX":40,"endY":40}"#,
        )
        .unwrap();
        assert!(ok_drag.requires_backend_validate());

        let ok = prepare_facade_call(
            &session_id,
            ComputerFacadeTool::Click,
            r#"{"observationId":"obs_lock","targetWindowId":7,"x":20,"y":20}"#,
        )
        .unwrap();
        assert!(ok.requires_backend_validate());
    }

    #[test]
    fn computer_use_redacts_secret_values_for_audit() {
        let text = "password=hunter2 token=ghp_1234567890abcdef sk-1234567890abcdef";
        let redacted = redact_secrets_in_text(text);
        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains("ghp_1234567890abcdef"));
        assert!(!redacted.contains("sk-1234567890abcdef"));

        let value = json!({
            "text": "api_key=AKIA1234567890ABCDEF",
            "nested": { "token": "secret-token-value" }
        });
        let redacted_value = redact_json_value(&value);
        assert!(value_contains_probable_secret(&value));
        assert_eq!(redacted_value["nested"]["token"], "[REDACTED]");
        assert!(!redacted_value.to_string().contains("AKIA1234567890ABCDEF"));
    }
}
