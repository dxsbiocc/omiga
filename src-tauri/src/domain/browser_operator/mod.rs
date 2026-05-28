//! Native browser operator facade backed by a sidecar JSON-RPC process.
//!
//! MVP scope:
//! - expose model-visible `browser_*` tool schemas
//! - parse `browserUseMode` request gates
//! - execute facade calls through a persistent sidecar transport
//! - return structured, model-safe JSON errors when the Python sidecar is absent
//!   or fails

use crate::domain::tools::ToolSchema;
use async_trait::async_trait;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

const DEFAULT_SIDECAR_RELATIVE_PATH: &str = "browser-operator/browser_operator.py";
const DEFAULT_PYTHON_BIN: &str = "python3";
const MAX_STDERR_TAIL_BYTES: usize = 16 * 1024;
const MAX_RESPONSE_LINE_BYTES: usize = 5 * 1024 * 1024;
const DEFAULT_SIDECAR_CALL_TIMEOUT_SECS: u64 = 90;
const DEFAULT_INSTALL_TIMEOUT_SECS: u64 = 15 * 60;
const PROCESS_KILL_TIMEOUT_SECS: u64 = 2;

/// Explicit user-selected scope for exposing Browser Operator facade tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserUseMode {
    Off,
    Task,
    Session,
}

impl BrowserUseMode {
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
pub enum BrowserFacadeTool {
    Open,
    Snapshot,
    Click,
    Fill,
    Screenshot,
    Close,
}

impl BrowserFacadeTool {
    pub fn from_model_name(name: &str) -> Option<Self> {
        match name {
            "browser_open" => Some(Self::Open),
            "browser_snapshot" => Some(Self::Snapshot),
            "browser_click" => Some(Self::Click),
            "browser_fill" => Some(Self::Fill),
            "browser_screenshot" => Some(Self::Screenshot),
            "browser_close" => Some(Self::Close),
            _ => None,
        }
    }

    pub fn model_name(self) -> &'static str {
        match self {
            Self::Open => "browser_open",
            Self::Snapshot => "browser_snapshot",
            Self::Click => "browser_click",
            Self::Fill => "browser_fill",
            Self::Screenshot => "browser_screenshot",
            Self::Close => "browser_close",
        }
    }

    fn rpc_method(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Snapshot => "snapshot",
            Self::Click => "click",
            Self::Fill => "fill",
            Self::Screenshot => "screenshot",
            Self::Close => "close",
        }
    }
}

pub fn is_facade_tool_name(name: &str) -> bool {
    BrowserFacadeTool::from_model_name(name).is_some()
}

pub fn facade_tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema::new(
            "browser_open",
            "Open a browser page through Omiga's native Browser Operator facade. Use this before snapshot/click/fill when a page is not already open for the current chat session.",
            json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Absolute URL to open."
                    },
                    "sessionId": {
                        "type": "string",
                        "description": "Optional browser session id. Defaults to the current Omiga chat session."
                    }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "browser_snapshot",
            "Capture the current browser page state for the current or specified browser session.",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "Optional browser session id. Defaults to the current Omiga chat session."
                    },
                    "maxElements": { "type": "integer", "minimum": 1, "maximum": 300 },
                    "maxTextChars": { "type": "integer", "minimum": 100, "maximum": 50000 }
                },
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "browser_click",
            "Click a browser target identified by a sidecar-understood target string.",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "Optional browser session id. Defaults to the current Omiga chat session."
                    },
                    "index": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Interactive element index returned by browser_snapshot."
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for the element to click."
                    },
                    "target": {
                        "type": "string",
                        "description": "Human-readable target hint when index/selector is unavailable."
                    }
                },
                "additionalProperties": false,
                "anyOf": [
                    { "required": ["index"] },
                    { "required": ["selector"] },
                    { "required": ["target"] }
                ]
            }),
        ),
        ToolSchema::new(
            "browser_fill",
            "Fill a browser input target with text. Returned/displayed values are redacted by Omiga.",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "Optional browser session id. Defaults to the current Omiga chat session."
                    },
                    "target": {
                        "type": "string",
                        "description": "Target hint for the sidecar."
                    },
                    "index": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Interactive element index returned by browser_snapshot."
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector for the input to fill."
                    },
                    "value": {
                        "type": "string",
                        "description": "Text to enter. Omiga redacts this value in emitted tool results."
                    }
                },
                "required": ["value"],
                "additionalProperties": false,
                "anyOf": [
                    { "required": ["index", "value"] },
                    { "required": ["selector", "value"] },
                    { "required": ["target", "value"] }
                ]
            }),
        ),
        ToolSchema::new(
            "browser_screenshot",
            "Capture a browser screenshot for the current or specified browser session.",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "Optional browser session id. Defaults to the current Omiga chat session."
                    },
                    "fullPage": {
                        "type": "boolean",
                        "description": "Optional. When true, request a full-page screenshot."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["png", "jpeg"]
                    }
                },
                "additionalProperties": false
            }),
        ),
        ToolSchema::new(
            "browser_close",
            "Close the browser page/session managed by Omiga's Browser Operator facade.",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": {
                        "type": "string",
                        "description": "Optional browser session id. Defaults to the current Omiga chat session."
                    }
                },
                "additionalProperties": false
            }),
        ),
    ]
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOperatorError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl BrowserOperatorError {
    fn invalid_arguments(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_arguments".to_string(),
            message: message.into(),
            details: None,
        }
    }

    fn sidecar_unavailable(path: &Path) -> Self {
        Self {
            code: "sidecar_unavailable".to_string(),
            message: format!(
                "Browser Operator Python sidecar is unavailable. Expected file at {}.",
                path.display()
            ),
            details: Some(json!({ "path": path.to_string_lossy() })),
        }
    }

    fn sidecar_spawn_failed(program: &Path, err: impl Into<String>) -> Self {
        Self {
            code: "sidecar_spawn_failed".to_string(),
            message: format!(
                "Failed to start Browser Operator sidecar `{}`: {}",
                program.display(),
                err.into()
            ),
            details: Some(json!({ "program": program.to_string_lossy() })),
        }
    }

    fn sidecar_io(message: impl Into<String>) -> Self {
        Self {
            code: "sidecar_io_error".to_string(),
            message: message.into(),
            details: None,
        }
    }

    fn sidecar_protocol(message: impl Into<String>, details: Option<Value>) -> Self {
        Self {
            code: "sidecar_protocol_error".to_string(),
            message: message.into(),
            details,
        }
    }

    fn sidecar_rpc(message: impl Into<String>, details: Option<Value>) -> Self {
        Self {
            code: "sidecar_rpc_error".to_string(),
            message: message.into(),
            details,
        }
    }

    fn sidecar_timeout(
        method: &str,
        phase: &str,
        duration: Duration,
        stderr: Option<String>,
    ) -> Self {
        let mut details = Map::new();
        details.insert("method".to_string(), json!(method));
        details.insert("phase".to_string(), json!(phase));
        details.insert("timeoutMs".to_string(), json!(duration.as_millis()));
        if let Some(stderr) = stderr.filter(|value| !value.trim().is_empty()) {
            details.insert("stderr".to_string(), json!(stderr));
        }
        Self {
            code: "sidecar_timeout".to_string(),
            message: format!(
                "Browser Operator sidecar timed out while {phase} `{method}` after {}s.",
                duration.as_secs()
            ),
            details: Some(Value::Object(details)),
        }
    }

    fn install_timeout(
        installer: &Path,
        python: &Path,
        duration: Duration,
        skip_browser_install: bool,
    ) -> Self {
        Self {
            code: "install_timeout".to_string(),
            message: format!(
                "Browser Operator backend install timed out after {}s.",
                duration.as_secs()
            ),
            details: Some(json!({
                "installer": installer,
                "python": python,
                "skipBrowserInstall": skip_browser_install,
                "timeoutMs": duration.as_millis(),
            })),
        }
    }
}

impl std::fmt::Display for BrowserOperatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BrowserOperatorError {}

#[derive(Debug)]
pub struct BrowserFacadeExecution {
    pub redacted_arguments: Value,
    pub output: Value,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
struct PreparedBrowserCall {
    session_id: String,
    rpc_method: &'static str,
    rpc_params: Value,
    redacted_arguments: Value,
    fallback_url: Option<String>,
}

#[derive(Debug, Clone)]
struct BrowserSidecarLaunchSpec {
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
}

struct BrowserSidecarProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
}

impl Drop for BrowserSidecarProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl BrowserSidecarProcess {
    async fn send_request(&mut self, request: &Value) -> Result<(), BrowserOperatorError> {
        let raw = serde_json::to_string(request).map_err(|err| {
            BrowserOperatorError::sidecar_protocol(
                format!("Failed to serialize Browser Operator request: {err}"),
                None,
            )
        })?;
        self.stdin.write_all(raw.as_bytes()).await.map_err(|err| {
            BrowserOperatorError::sidecar_io(format!(
                "Failed to write Browser Operator request to sidecar stdin: {err}"
            ))
        })?;
        self.stdin.write_all(b"\n").await.map_err(|err| {
            BrowserOperatorError::sidecar_io(format!(
                "Failed to terminate Browser Operator request line: {err}"
            ))
        })?;
        self.stdin.flush().await.map_err(|err| {
            BrowserOperatorError::sidecar_io(format!(
                "Failed to flush Browser Operator request to sidecar stdin: {err}"
            ))
        })
    }

    async fn read_response(&mut self, request_id: &str) -> Result<Value, BrowserOperatorError> {
        loop {
            let Some(line) = self.read_response_line().await? else {
                let stderr = self.stderr_excerpt().await;
                let message = if stderr.is_empty() {
                    "Browser Operator sidecar exited before returning a response.".to_string()
                } else {
                    format!(
                        "Browser Operator sidecar exited before returning a response. stderr: {stderr}"
                    )
                };
                return Err(BrowserOperatorError::sidecar_protocol(message, None));
            };
            let line = String::from_utf8(line).map_err(|err| {
                BrowserOperatorError::sidecar_protocol(
                    format!("Browser Operator sidecar emitted a non-UTF8 response: {err}"),
                    None,
                )
            })?;
            let Some(response) =
                parse_sidecar_response(line.trim_end_matches(['\r', '\n']), request_id)?
            else {
                continue;
            };
            return Ok(response);
        }
    }

    async fn read_response_line(&mut self) -> Result<Option<Vec<u8>>, BrowserOperatorError> {
        let mut line = Vec::new();
        loop {
            let buffer = self.stdout.fill_buf().await.map_err(|err| {
                BrowserOperatorError::sidecar_io(format!(
                    "Failed to read Browser Operator sidecar response: {err}"
                ))
            })?;
            if buffer.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(line))
                };
            }

            let consumed = buffer
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|index| index + 1)
                .unwrap_or(buffer.len());
            if line.len().saturating_add(consumed) > MAX_RESPONSE_LINE_BYTES {
                return Err(BrowserOperatorError::sidecar_protocol(
                    "Browser Operator sidecar response exceeded the maximum JSONL line size."
                        .to_string(),
                    Some(json!({ "maxBytes": MAX_RESPONSE_LINE_BYTES })),
                ));
            }
            let ends_with_newline = buffer[..consumed].last().is_some_and(|byte| *byte == b'\n');
            line.extend_from_slice(&buffer[..consumed]);
            self.stdout.consume(consumed);
            if ends_with_newline {
                return Ok(Some(line));
            }
        }
    }

    async fn stderr_excerpt(&self) -> String {
        String::from_utf8_lossy(&self.stderr_tail.lock().await).to_string()
    }
}

#[async_trait]
trait BrowserSidecarTransport: Send + Sync {
    async fn call(&self, method: &str, params: Value) -> Result<Value, BrowserOperatorError>;
}

struct ProcessBrowserSidecarTransport {
    launch_spec: Result<BrowserSidecarLaunchSpec, BrowserOperatorError>,
    process: Mutex<Option<BrowserSidecarProcess>>,
}

impl ProcessBrowserSidecarTransport {
    fn new() -> Self {
        Self {
            launch_spec: resolve_default_launch_spec(),
            process: Mutex::new(None),
        }
    }

    async fn start_process(
        launch_spec: &BrowserSidecarLaunchSpec,
    ) -> Result<BrowserSidecarProcess, BrowserOperatorError> {
        let mut command = Command::new(&launch_spec.program);
        command
            .args(&launch_spec.args)
            .current_dir(&launch_spec.cwd)
            .envs(launch_spec.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| {
            BrowserOperatorError::sidecar_spawn_failed(&launch_spec.program, err.to_string())
        })?;
        let stderr_tail = Arc::new(Mutex::new(Vec::new()));
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(drain_stderr(stderr, Arc::clone(&stderr_tail)));
        }
        let stdin = child.stdin.take().ok_or_else(|| {
            BrowserOperatorError::sidecar_protocol(
                "Browser Operator sidecar stdin is unavailable.".to_string(),
                None,
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            BrowserOperatorError::sidecar_protocol(
                "Browser Operator sidecar stdout is unavailable.".to_string(),
                None,
            )
        })?;
        Ok(BrowserSidecarProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_tail,
        })
    }

    async fn reset_process(process: &mut Option<BrowserSidecarProcess>) {
        if let Some(mut child) = process.take() {
            let _ = child.child.start_kill();
            let _ = timeout(
                Duration::from_secs(PROCESS_KILL_TIMEOUT_SECS),
                child.child.wait(),
            )
            .await;
        }
    }
}

#[async_trait]
impl BrowserSidecarTransport for ProcessBrowserSidecarTransport {
    async fn call(&self, method: &str, params: Value) -> Result<Value, BrowserOperatorError> {
        let launch_spec = self.launch_spec.clone()?;
        let mut process_guard = self.process.lock().await;
        if process_guard.is_none() {
            *process_guard = Some(Self::start_process(&launch_spec).await?);
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let request = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });
        let request_timeout = sidecar_call_timeout();

        let send_result = {
            let process = process_guard
                .as_mut()
                .expect("process must exist after lazy sidecar start");
            timeout(request_timeout, process.send_request(&request)).await
        };
        match send_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                Self::reset_process(&mut process_guard).await;
                return Err(err);
            }
            Err(_) => {
                let stderr = match process_guard.as_ref() {
                    Some(process) => Some(process.stderr_excerpt().await),
                    None => None,
                };
                Self::reset_process(&mut process_guard).await;
                return Err(BrowserOperatorError::sidecar_timeout(
                    method,
                    "sending request",
                    request_timeout,
                    stderr,
                ));
            }
        }

        let read_result = {
            let process = process_guard
                .as_mut()
                .expect("process must exist while awaiting sidecar response");
            timeout(request_timeout, process.read_response(&request_id)).await
        };
        match read_result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => {
                Self::reset_process(&mut process_guard).await;
                Err(err)
            }
            Err(_) => {
                let stderr = match process_guard.as_ref() {
                    Some(process) => Some(process.stderr_excerpt().await),
                    None => None,
                };
                Self::reset_process(&mut process_guard).await;
                Err(BrowserOperatorError::sidecar_timeout(
                    method,
                    "waiting for response",
                    request_timeout,
                    stderr,
                ))
            }
        }
    }
}

fn sidecar_call_timeout() -> Duration {
    timeout_from_env(
        "OMIGA_BROWSER_OPERATOR_REQUEST_TIMEOUT_SECS",
        DEFAULT_SIDECAR_CALL_TIMEOUT_SECS,
    )
}

fn install_backend_timeout() -> Duration {
    timeout_from_env(
        "OMIGA_BROWSER_OPERATOR_INSTALL_TIMEOUT_SECS",
        DEFAULT_INSTALL_TIMEOUT_SECS,
    )
}

fn timeout_from_env(env_key: &str, default_secs: u64) -> Duration {
    std::env::var(env_key)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(default_secs))
}

fn browser_operator_sidecar_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OMIGA_BROWSER_OPERATOR_SIDECAR").map(PathBuf::from) {
        return path;
    }

    browser_operator_base_dirs()
        .into_iter()
        .map(|base| base.join(DEFAULT_SIDECAR_RELATIVE_PATH))
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_SIDECAR_RELATIVE_PATH)
        })
}

fn browser_operator_installer_path() -> PathBuf {
    let sidecar = browser_operator_sidecar_path();
    let sidecar_installer = sidecar
        .parent()
        .map(|dir| dir.join("install_backend.py"))
        .unwrap_or_default();
    if sidecar_installer.is_file() {
        return sidecar_installer;
    }

    browser_operator_base_dirs()
        .into_iter()
        .map(|base| base.join("browser-operator").join("install_backend.py"))
        .find(|path| path.is_file())
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("browser-operator")
                .join("install_backend.py")
        })
}

fn browser_operator_base_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    push_unique_path(&mut dirs, PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            push_unique_path(&mut dirs, exe_dir.to_path_buf());
            push_unique_path(&mut dirs, exe_dir.join("resources"));
            push_unique_path(&mut dirs, exe_dir.join("Resources"));
            if let Some(parent) = exe_dir.parent() {
                push_unique_path(&mut dirs, parent.to_path_buf());
                push_unique_path(&mut dirs, parent.join("resources"));
                push_unique_path(&mut dirs, parent.join("Resources"));
            }
        }
    }
    dirs
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
    }
}

/// Process-wide Browser Operator manager stored in [`crate::domain::chat_state::ChatState`].
pub struct BrowserOperatorManager {
    shared_transport: Option<Arc<dyn BrowserSidecarTransport>>,
    transports: Arc<Mutex<HashMap<String, Arc<dyn BrowserSidecarTransport>>>>,
}

impl Default for BrowserOperatorManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserOperatorManager {
    pub fn new() -> Self {
        Self {
            shared_transport: None,
            transports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn with_transport(transport: Arc<dyn BrowserSidecarTransport>) -> Self {
        Self {
            shared_transport: Some(transport),
            transports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn call(
        &self,
        session_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, BrowserOperatorError> {
        if let Some(shared) = &self.shared_transport {
            return shared.call(method, params).await;
        }

        if method == BrowserFacadeTool::Close.rpc_method() {
            let transport = self.transports.lock().await.remove(session_id);
            if let Some(transport) = transport {
                return transport.call(method, params).await;
            }
            return Ok(json!({
                "ok": true,
                "closed": true,
                "alreadyClosed": true,
            }));
        }

        let transport = {
            let mut transports = self.transports.lock().await;
            transports
                .entry(session_id.to_string())
                .or_insert_with(|| Arc::new(ProcessBrowserSidecarTransport::new()))
                .clone()
        };
        let result = transport.call(method, params).await;
        if result.is_err() {
            self.transports.lock().await.remove(session_id);
        }
        result
    }
}

pub async fn execute_facade_tool(
    manager: &BrowserOperatorManager,
    omiga_session_id: &str,
    tool: BrowserFacadeTool,
    arguments: &str,
) -> BrowserFacadeExecution {
    match prepare_browser_call(omiga_session_id, tool, arguments) {
        Ok(prepared) => match manager
            .call(
                &prepared.session_id,
                prepared.rpc_method,
                prepared.rpc_params,
            )
            .await
        {
            Ok(result) => {
                let output = normalize_sidecar_result(
                    tool,
                    &prepared.session_id,
                    prepared.fallback_url.as_deref(),
                    &result,
                );
                let is_error = output
                    .get("ok")
                    .and_then(Value::as_bool)
                    .map(|ok| !ok)
                    .unwrap_or_else(|| output.get("error").is_some());
                BrowserFacadeExecution {
                    redacted_arguments: prepared.redacted_arguments,
                    output,
                    is_error,
                }
            }
            Err(error) => BrowserFacadeExecution {
                redacted_arguments: prepared.redacted_arguments,
                output: structured_error_output(
                    tool,
                    &prepared.session_id,
                    prepared.fallback_url.as_deref(),
                    error,
                ),
                is_error: true,
            },
        },
        Err((redacted_arguments, session_id, error)) => BrowserFacadeExecution {
            redacted_arguments,
            output: structured_error_output(tool, &session_id, None, error),
            is_error: true,
        },
    }
}

pub fn backend_status() -> Value {
    let sidecar = browser_operator_sidecar_path();
    let installer = browser_operator_installer_path();
    let home = managed_backend_home();
    let managed_python = managed_venv_python(&home);
    let managed_browser_use = managed_browser_use_executable(&home);
    let configured_python = std::env::var_os("OMIGA_BROWSER_OPERATOR_PYTHON").map(PathBuf::from);
    json!({
        "sidecarPath": sidecar,
        "sidecarExists": sidecar.is_file(),
        "installerPath": installer,
        "installerExists": installer.is_file(),
        "managedHome": home,
        "managedPython": managed_python,
        "managedPythonExists": managed_python.is_file(),
        "managedBrowserUse": managed_browser_use,
        "managedBrowserUseExists": managed_browser_use.is_file(),
        "configuredPython": configured_python,
        "selectedPython": configured_python
            .clone()
            .unwrap_or_else(|| if managed_python.is_file() { managed_python.clone() } else { PathBuf::from(DEFAULT_PYTHON_BIN) }),
        "playwrightBrowsersPath": managed_backend_home().join("ms-playwright"),
        "requestTimeoutMs": sidecar_call_timeout().as_millis(),
        "installTimeoutMs": install_backend_timeout().as_millis(),
        "installCommand": format!(
            "{} {} --json",
            DEFAULT_PYTHON_BIN,
            installer.to_string_lossy()
        ),
    })
}

pub async fn install_managed_backend(
    skip_browser_install: bool,
) -> Result<Value, BrowserOperatorError> {
    let installer = browser_operator_installer_path();
    if !installer.is_file() {
        return Err(BrowserOperatorError::sidecar_unavailable(&installer));
    }

    let bootstrap_python = std::env::var_os("OMIGA_BROWSER_OPERATOR_BOOTSTRAP_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PYTHON_BIN));
    let mut command = Command::new(&bootstrap_python);
    command.kill_on_drop(true);
    command.arg(&installer).arg("--json");
    if skip_browser_install {
        command.arg("--skip-browser-install");
    }
    let child = command.spawn().map_err(|err| {
        BrowserOperatorError::sidecar_spawn_failed(&bootstrap_python, err.to_string())
    })?;
    let install_timeout = install_backend_timeout();
    let output = timeout(install_timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            BrowserOperatorError::install_timeout(
                &installer,
                &bootstrap_python,
                install_timeout,
                skip_browser_install,
            )
        })?
        .map_err(|err| {
            BrowserOperatorError::sidecar_spawn_failed(&bootstrap_python, err.to_string())
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let parsed = serde_json::from_str::<Value>(&stdout).unwrap_or_else(|_| {
        json!({
            "ok": output.status.success(),
            "stdout": stdout,
            "stderr": stderr,
        })
    });
    if output.status.success() {
        Ok(parsed)
    } else {
        Err(BrowserOperatorError::sidecar_io(format!(
            "Browser Operator backend install failed: {}",
            parsed
        )))
    }
}

fn resolve_default_launch_spec() -> Result<BrowserSidecarLaunchSpec, BrowserOperatorError> {
    let configured_sidecar = browser_operator_sidecar_path();
    if !configured_sidecar.exists() {
        return Err(BrowserOperatorError::sidecar_unavailable(
            &configured_sidecar,
        ));
    }
    if configured_sidecar
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        let managed_home = managed_backend_home();
        let managed_python = managed_venv_python(&managed_home);
        let python = std::env::var_os("OMIGA_BROWSER_OPERATOR_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if managed_python.is_file() {
                    managed_python.clone()
                } else {
                    PathBuf::from(DEFAULT_PYTHON_BIN)
                }
            });
        let cwd = configured_sidecar
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let mut env = Vec::new();
        if std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").is_none()
            && (managed_python.is_file()
                || std::env::var_os("OMIGA_BROWSER_OPERATOR_HOME").is_some())
        {
            env.push((
                "PLAYWRIGHT_BROWSERS_PATH".to_string(),
                managed_home
                    .join("ms-playwright")
                    .to_string_lossy()
                    .to_string(),
            ));
        }
        return Ok(BrowserSidecarLaunchSpec {
            program: python,
            args: vec![configured_sidecar.to_string_lossy().to_string()],
            cwd,
            env,
        });
    }
    Ok(BrowserSidecarLaunchSpec {
        cwd: configured_sidecar
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        program: configured_sidecar,
        args: vec![],
        env: vec![],
    })
}

fn managed_backend_home() -> PathBuf {
    std::env::var_os("OMIGA_BROWSER_OPERATOR_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".omiga").join("browser-operator")))
        .unwrap_or_else(|| std::env::temp_dir().join("omiga-browser-operator"))
}

fn managed_venv_python(home: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        home.join(".venv").join("Scripts").join("python.exe")
    } else {
        home.join(".venv").join("bin").join("python")
    }
}

fn managed_browser_use_executable(home: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        home.join(".venv").join("Scripts").join("browser-use.exe")
    } else {
        home.join(".venv").join("bin").join("browser-use")
    }
}

fn prepare_browser_call(
    omiga_session_id: &str,
    tool: BrowserFacadeTool,
    arguments: &str,
) -> Result<PreparedBrowserCall, (Value, String, BrowserOperatorError)> {
    match tool {
        BrowserFacadeTool::Open => {
            let args: BrowserOpenArgs = serde_json::from_str(arguments).map_err(|err| {
                invalid_argument_error(
                    arguments,
                    omiga_session_id,
                    format!(
                        "browser_open expects {{\"url\": string, \"sessionId\"?: string}}: {err}"
                    ),
                )
            })?;
            let session_id =
                normalize_requested_session_id(args.session_id.as_deref(), omiga_session_id);
            let url = args.url;
            validate_browser_url(&url).map_err(|message| {
                (
                    redact_browser_value(&json!({
                        "url": url.clone(),
                        "sessionId": session_id,
                    })),
                    session_id.to_string(),
                    BrowserOperatorError::invalid_arguments(message),
                )
            })?;
            let redacted_url = redact_url_query(&url);
            let redacted_arguments = json!({
                "url": redacted_url,
                "sessionId": session_id,
            });
            Ok(PreparedBrowserCall {
                session_id: session_id.to_string(),
                rpc_method: tool.rpc_method(),
                rpc_params: json!({
                    "url": url.clone(),
                    "sessionId": session_id,
                }),
                redacted_arguments,
                fallback_url: Some(redact_url_query(&url)),
            })
        }
        BrowserFacadeTool::Snapshot => {
            let args: BrowserSnapshotArgs = serde_json::from_str(arguments).map_err(|err| {
                invalid_argument_error(
                    arguments,
                    omiga_session_id,
                    format!("browser_snapshot expects {{\"sessionId\"?: string, \"maxElements\"?: number, \"maxTextChars\"?: number}}: {err}"),
                )
            })?;
            let session_id =
                normalize_requested_session_id(args.session_id.as_deref(), omiga_session_id);
            let mut rpc_params = Map::new();
            rpc_params.insert("sessionId".to_string(), json!(session_id));
            if let Some(max_elements) = args.max_elements {
                rpc_params.insert("max_elements".to_string(), json!(max_elements));
            }
            if let Some(max_text_chars) = args.max_text_chars {
                rpc_params.insert("max_text_chars".to_string(), json!(max_text_chars));
            }
            Ok(PreparedBrowserCall {
                session_id: session_id.to_string(),
                rpc_method: tool.rpc_method(),
                rpc_params: Value::Object(rpc_params),
                redacted_arguments: json!({ "sessionId": session_id }),
                fallback_url: None,
            })
        }
        BrowserFacadeTool::Click => {
            let args: BrowserTargetArgs = serde_json::from_str(arguments).map_err(|err| {
                invalid_argument_error(arguments, omiga_session_id, format!("browser_click expects {{\"target\": string, \"sessionId\"?: string}}: {err}"))
            })?;
            let session_id =
                normalize_requested_session_id(args.session_id.as_deref(), omiga_session_id);
            let target_params = browser_target_params(&args).map_err(|error| {
                (
                    redact_browser_value(
                        &serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({})),
                    ),
                    session_id.to_string(),
                    error,
                )
            })?;
            let mut rpc_params = target_params.clone();
            rpc_params.insert("sessionId".to_string(), json!(session_id));
            let mut redacted_arguments = target_params;
            redacted_arguments.insert("sessionId".to_string(), json!(session_id));
            Ok(PreparedBrowserCall {
                session_id: session_id.to_string(),
                rpc_method: tool.rpc_method(),
                rpc_params: Value::Object(rpc_params),
                redacted_arguments: Value::Object(redacted_arguments),
                fallback_url: None,
            })
        }
        BrowserFacadeTool::Fill => {
            let args: BrowserFillArgs = serde_json::from_str(arguments).map_err(|err| {
                invalid_argument_error(arguments, omiga_session_id, format!("browser_fill expects {{\"target\": string, \"value\": string, \"sessionId\"?: string}}: {err}"))
            })?;
            let session_id =
                normalize_requested_session_id(args.session_id.as_deref(), omiga_session_id);
            let target_params = browser_fill_target_params(&args).map_err(|error| {
                (
                    redact_browser_value(
                        &serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({})),
                    ),
                    session_id.to_string(),
                    error,
                )
            })?;
            let mut rpc_params = target_params.clone();
            rpc_params.insert("sessionId".to_string(), json!(session_id));
            rpc_params.insert("value".to_string(), json!(args.value));
            let mut redacted_arguments = target_params;
            redacted_arguments.insert("sessionId".to_string(), json!(session_id));
            redacted_arguments.insert("value".to_string(), json!(redact_fill_value(&args.value)));
            Ok(PreparedBrowserCall {
                session_id: session_id.to_string(),
                rpc_method: tool.rpc_method(),
                rpc_params: Value::Object(rpc_params),
                redacted_arguments: Value::Object(redacted_arguments),
                fallback_url: None,
            })
        }
        BrowserFacadeTool::Screenshot => {
            let args: BrowserScreenshotArgs = serde_json::from_str(arguments).map_err(|err| {
                invalid_argument_error(arguments, omiga_session_id, format!("browser_screenshot expects {{\"sessionId\"?: string, \"fullPage\"?: boolean}}: {err}"))
            })?;
            let session_id =
                normalize_requested_session_id(args.session_id.as_deref(), omiga_session_id);
            let mut rpc_params = Map::new();
            rpc_params.insert("sessionId".to_string(), json!(session_id));
            rpc_params.insert(
                "full_page".to_string(),
                json!(args.full_page.unwrap_or(false)),
            );
            if let Some(format) = args
                .format
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                rpc_params.insert("format".to_string(), json!(format));
            }
            Ok(PreparedBrowserCall {
                session_id: session_id.to_string(),
                rpc_method: tool.rpc_method(),
                rpc_params: Value::Object(rpc_params),
                redacted_arguments: json!({
                    "sessionId": session_id,
                    "fullPage": args.full_page.unwrap_or(false),
                    "format": args.format,
                }),
                fallback_url: None,
            })
        }
        BrowserFacadeTool::Close => {
            let args: BrowserSessionArgs = serde_json::from_str(arguments).map_err(|err| {
                invalid_argument_error(
                    arguments,
                    omiga_session_id,
                    format!("browser_close expects {{\"sessionId\"?: string}}: {err}"),
                )
            })?;
            let session_id =
                normalize_requested_session_id(args.session_id.as_deref(), omiga_session_id);
            Ok(PreparedBrowserCall {
                session_id: session_id.to_string(),
                rpc_method: tool.rpc_method(),
                rpc_params: json!({ "sessionId": session_id }),
                redacted_arguments: json!({ "sessionId": session_id }),
                fallback_url: None,
            })
        }
    }
}

fn invalid_argument_error(
    arguments: &str,
    default_session_id: &str,
    message: String,
) -> (Value, String, BrowserOperatorError) {
    let raw_value = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}));
    let session_id = raw_value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_session_id)
        .to_string();
    (
        redact_browser_value(&raw_value),
        session_id,
        BrowserOperatorError::invalid_arguments(message),
    )
}

fn normalize_requested_session_id<'a>(requested: Option<&'a str>, fallback: &'a str) -> &'a str {
    requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
}

fn validate_browser_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = Url::parse(trimmed)
        .map_err(|_| "browser_open requires an absolute http(s) URL.".to_string())?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err("browser_open requires an absolute http(s) URL.".to_string()),
    }
    if !parsed.has_host() {
        return Err("browser_open requires an absolute http(s) URL with a host.".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(
            "browser_open does not allow embedded username/password credentials.".to_string(),
        );
    }
    Ok(())
}

fn browser_target_params(
    args: &BrowserTargetArgs,
) -> Result<Map<String, Value>, BrowserOperatorError> {
    let mut params = Map::new();
    if let Some(index) = args.index {
        params.insert("index".to_string(), json!(index));
        return Ok(params);
    }
    if let Some(selector) = args
        .selector
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        params.insert("selector".to_string(), json!(selector));
        return Ok(params);
    }
    if let Some(target) = args
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        params.insert("target".to_string(), json!(target));
        return Ok(params);
    }
    Err(BrowserOperatorError::invalid_arguments(
        "browser action requires one of `index`, `selector`, or `target`.",
    ))
}

fn browser_fill_target_params(
    args: &BrowserFillArgs,
) -> Result<Map<String, Value>, BrowserOperatorError> {
    let target_args = BrowserTargetArgs {
        session_id: args.session_id.clone(),
        index: args.index,
        selector: args.selector.clone(),
        target: args.target.clone(),
    };
    browser_target_params(&target_args)
}

fn redact_url_query(url: &str) -> String {
    let trimmed = url.trim();
    let (without_fragment, has_fragment) = match trimmed.split_once('#') {
        Some((prefix, _)) => (prefix, true),
        None => (trimmed, false),
    };
    let (prefix, has_query) = match without_fragment.split_once('?') {
        Some((prefix, _)) => (prefix, true),
        None => (without_fragment, false),
    };
    let sanitized_prefix = prefix
        .split_once("://")
        .map(|(scheme, rest)| {
            let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            let (authority, tail) = rest.split_at(authority_end);
            let authority = authority
                .rsplit_once('@')
                .map(|(_, host)| host)
                .unwrap_or(authority);
            format!("{scheme}://{authority}{tail}")
        })
        .unwrap_or_else(|| prefix.to_string());
    let mut redacted = sanitized_prefix;
    if has_query {
        redacted.push_str("?redacted");
    }
    if has_fragment {
        redacted.push_str("#redacted");
    }
    redacted
}

fn normalize_sidecar_result(
    tool: BrowserFacadeTool,
    session_id: &str,
    fallback_url: Option<&str>,
    result: &Value,
) -> Value {
    let sanitized_result = sanitize_sidecar_value(result);
    let mut map = Map::new();
    let object = sanitized_result.as_object();
    let nested_result_object = object
        .and_then(|obj| obj.get("result"))
        .and_then(Value::as_object);
    let info_object = nested_result_object.or(object);
    let ok = object
        .and_then(|obj| obj.get("ok"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| object.and_then(|obj| obj.get("error")).is_none());
    map.insert("ok".to_string(), Value::Bool(ok));
    map.insert(
        "tool".to_string(),
        Value::String(tool.model_name().to_string()),
    );
    map.insert(
        "facadeTool".to_string(),
        Value::String(tool.model_name().to_string()),
    );
    map.insert(
        "backend".to_string(),
        Value::String("browser-use-sidecar".to_string()),
    );
    map.insert(
        "sessionId".to_string(),
        Value::String(session_id.to_string()),
    );
    if let Some(url) = info_object
        .and_then(|obj| find_string_field(obj, &["url", "pageUrl", "currentUrl"]))
        .or(fallback_url)
    {
        map.insert("url".to_string(), Value::String(url.to_string()));
    }
    if let Some(title) = info_object.and_then(|obj| find_string_field(obj, &["title", "pageTitle"]))
    {
        map.insert("title".to_string(), Value::String(title.to_string()));
    }
    if let Some(obj) = object {
        for (key, value) in obj {
            if matches!(
                key.as_str(),
                "ok" | "tool"
                    | "facadeTool"
                    | "backend"
                    | "sessionId"
                    | "url"
                    | "pageUrl"
                    | "currentUrl"
                    | "title"
                    | "pageTitle"
            ) {
                continue;
            }
            if key == "error" {
                map.insert(
                    "error".to_string(),
                    normalize_sidecar_error_value(value).unwrap_or_else(|| {
                        json!({
                            "code": "sidecar_error",
                            "message": "Browser Operator sidecar reported an error."
                        })
                    }),
                );
                continue;
            }
            map.insert(key.clone(), value.clone());
        }
    } else if !sanitized_result.is_null() {
        map.insert("result".to_string(), sanitized_result);
    }
    if !ok && !map.contains_key("error") {
        map.insert(
            "error".to_string(),
            json!({
                "code": "sidecar_call_failed",
                "message": "Browser Operator sidecar reported failure."
            }),
        );
    }
    Value::Object(map)
}

fn structured_error_output(
    tool: BrowserFacadeTool,
    session_id: &str,
    fallback_url: Option<&str>,
    error: BrowserOperatorError,
) -> Value {
    let mut map = Map::new();
    map.insert("ok".to_string(), Value::Bool(false));
    map.insert(
        "tool".to_string(),
        Value::String(tool.model_name().to_string()),
    );
    map.insert(
        "facadeTool".to_string(),
        Value::String(tool.model_name().to_string()),
    );
    map.insert(
        "backend".to_string(),
        Value::String("browser-use-sidecar".to_string()),
    );
    map.insert(
        "sessionId".to_string(),
        Value::String(session_id.to_string()),
    );
    if let Some(url) = fallback_url {
        map.insert("url".to_string(), Value::String(url.to_string()));
    }
    map.insert(
        "error".to_string(),
        sanitize_sidecar_value(&serde_json::to_value(error).unwrap_or_else(|_| {
            json!({
                "code": "browser_operator_error",
                "message": "Browser Operator call failed."
            })
        })),
    );
    Value::Object(map)
}

fn normalize_sidecar_error_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(message) => Some(json!({
            "code": "sidecar_error",
            "message": message,
        })),
        Value::Object(map) => Some(sanitize_sidecar_value(&Value::Object(map.clone()))),
        _ if value.is_null() => None,
        other => Some(json!({
            "code": "sidecar_error",
            "message": other.to_string(),
        })),
    }
}

fn sanitize_sidecar_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(sanitize_sidecar_value).collect()),
        Value::Object(map) => {
            let mut sanitized = Map::new();
            for (key, value) in map {
                if key.eq_ignore_ascii_case("value") {
                    sanitized.insert(key.clone(), Value::String(redact_fill_value_value(value)));
                } else if key.to_ascii_lowercase().contains("url") {
                    sanitized.insert(
                        key.clone(),
                        value
                            .as_str()
                            .map(redact_url_query)
                            .map(Value::String)
                            .unwrap_or_else(|| sanitize_sidecar_value(value)),
                    );
                } else {
                    sanitized.insert(key.clone(), sanitize_sidecar_value(value));
                }
            }
            Value::Object(sanitized)
        }
        _ => value.clone(),
    }
}

fn redact_browser_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(redact_browser_value).collect()),
        Value::Object(map) => {
            let mut redacted = Map::new();
            for (key, value) in map {
                if key.eq_ignore_ascii_case("value") {
                    redacted.insert(key.clone(), Value::String(redact_fill_value_value(value)));
                } else if key.eq_ignore_ascii_case("url") {
                    redacted.insert(
                        key.clone(),
                        value
                            .as_str()
                            .map(redact_url_query)
                            .map(Value::String)
                            .unwrap_or_else(|| redact_browser_value(value)),
                    );
                } else {
                    redacted.insert(key.clone(), redact_browser_value(value));
                }
            }
            Value::Object(redacted)
        }
        _ => value.clone(),
    }
}

pub fn redact_arguments_for_display(value: &Value) -> Value {
    redact_browser_value(value)
}

fn redact_fill_value(value: &str) -> String {
    format!("[REDACTED {} chars]", value.chars().count())
}

fn redact_fill_value_value(value: &Value) -> String {
    match value {
        Value::String(text) => redact_fill_value(text),
        other => format!("[REDACTED {} bytes]", other.to_string().len()),
    }
}

fn find_string_field<'a>(map: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        let value = map
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(value) = value {
            return Some(value);
        }
    }
    None
}

fn parse_sidecar_response(
    line: &str,
    request_id: &str,
) -> Result<Option<Value>, BrowserOperatorError> {
    if line.trim().is_empty() {
        return Ok(None);
    }
    let payload = serde_json::from_str::<Value>(line).map_err(|err| {
        BrowserOperatorError::sidecar_protocol(
            format!("Failed to parse Browser Operator sidecar JSON response: {err}"),
            Some(json!({ "lineBytes": line.len() })),
        )
    })?;
    let Some(object) = payload.as_object() else {
        return Err(BrowserOperatorError::sidecar_protocol(
            "Browser Operator sidecar returned a non-object JSON-RPC payload.".to_string(),
            Some(json!({ "payloadType": json_type_name(&payload) })),
        ));
    };
    let id_matches = object
        .get("id")
        .map(|value| match value {
            Value::String(id) => id == request_id,
            _ => value.to_string() == request_id,
        })
        .unwrap_or(false);
    if !id_matches {
        return Ok(None);
    }
    // The bundled Python sidecar uses a compact JSONL envelope:
    // `{ id, ok, result?, error? }`.  Treat an application-level `ok:false`
    // as a normal facade response so the model sees the original browser-use
    // error code instead of a transport-level JSON-RPC failure.
    if object.contains_key("ok") {
        return Ok(Some(Value::Object(object.clone())));
    }
    if let Some(error) = object.get("error") {
        let details = normalize_sidecar_error_value(error);
        let message = details
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("Browser Operator sidecar returned an RPC error.")
            .to_string();
        return Err(BrowserOperatorError::sidecar_rpc(message, details));
    }
    Ok(Some(
        object
            .get("result")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new())),
    ))
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

async fn drain_stderr(mut stderr: ChildStderr, tail: Arc<Mutex<Vec<u8>>>) {
    let mut buf = [0_u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut tail = tail.lock().await;
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > MAX_STDERR_TAIL_BYTES {
                    let drain = tail.len() - MAX_STDERR_TAIL_BYTES;
                    tail.drain(..drain);
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserOpenArgs {
    url: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSnapshotArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    max_elements: Option<u32>,
    #[serde(default)]
    max_text_chars: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserSessionArgs {
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserTargetArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserFillArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    target: Option<String>,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserScreenshotArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    full_page: Option<bool>,
    #[serde(default)]
    format: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        execute_facade_tool, facade_tool_schemas, BrowserFacadeExecution, BrowserFacadeTool,
        BrowserOperatorError, BrowserOperatorManager, BrowserUseMode,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::Arc;

    struct MockTransport {
        result: Result<serde_json::Value, BrowserOperatorError>,
    }

    #[async_trait]
    impl super::BrowserSidecarTransport for MockTransport {
        async fn call(
            &self,
            _method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, BrowserOperatorError> {
            self.result.clone()
        }
    }

    fn make_manager(
        result: Result<serde_json::Value, BrowserOperatorError>,
    ) -> BrowserOperatorManager {
        BrowserOperatorManager::with_transport(Arc::new(MockTransport { result }))
    }

    #[test]
    fn browser_use_mode_defaults_to_off_for_missing_or_unknown_values() {
        assert_eq!(BrowserUseMode::from_request(None), BrowserUseMode::Off);
        assert_eq!(BrowserUseMode::from_request(Some("")), BrowserUseMode::Off);
        assert_eq!(
            BrowserUseMode::from_request(Some("always")),
            BrowserUseMode::Off
        );
    }

    #[test]
    fn browser_use_mode_accepts_task_and_session_case_insensitively() {
        assert_eq!(
            BrowserUseMode::from_request(Some("task")),
            BrowserUseMode::Task
        );
        assert_eq!(
            BrowserUseMode::from_request(Some(" TASK ")),
            BrowserUseMode::Task
        );
        assert_eq!(
            BrowserUseMode::from_request(Some("session")),
            BrowserUseMode::Session
        );
        assert_eq!(
            BrowserUseMode::from_request(Some(" Session ")),
            BrowserUseMode::Session
        );
    }

    #[test]
    fn browser_facade_schema_catalog_contains_model_visible_names_only() {
        let names: HashSet<_> = facade_tool_schemas()
            .into_iter()
            .map(|schema| schema.name)
            .collect();
        assert_eq!(
            names,
            HashSet::from([
                "browser_open".to_string(),
                "browser_snapshot".to_string(),
                "browser_click".to_string(),
                "browser_fill".to_string(),
                "browser_screenshot".to_string(),
                "browser_close".to_string(),
            ])
        );
    }

    #[tokio::test]
    async fn browser_fill_defaults_session_and_redacts_value() {
        let manager = make_manager(Ok(json!({
            "ok": true,
            "title": "Login",
            "url": "https://example.com/login?token=secret#top",
            "value": "should-not-leak"
        })));
        let execution = execute_facade_tool(
            &manager,
            "omiga-session-1",
            BrowserFacadeTool::Fill,
            r#"{"target":"Email","value":"secret@example.com"}"#,
        )
        .await;

        assert!(!execution.is_error);
        assert_eq!(
            execution.redacted_arguments,
            json!({
                "sessionId": "omiga-session-1",
                "target": "Email",
                "value": "[REDACTED 18 chars]"
            })
        );
        assert_eq!(execution.output["sessionId"], "omiga-session-1");
        assert_eq!(execution.output["tool"], "browser_fill");
        assert_eq!(
            execution.output["url"],
            "https://example.com/login?redacted#redacted"
        );
        assert_eq!(execution.output["title"], "Login");
        assert_eq!(execution.output["value"], "[REDACTED 15 chars]");
    }

    #[tokio::test]
    async fn browser_operator_returns_structured_sidecar_errors() {
        let manager = make_manager(Err(BrowserOperatorError {
            code: "sidecar_unavailable".to_string(),
            message: "missing sidecar".to_string(),
            details: Some(json!({ "path": "/tmp/missing.py" })),
        }));
        let execution = execute_facade_tool(
            &manager,
            "session-42",
            BrowserFacadeTool::Open,
            r#"{"url":"https://example.com"}"#,
        )
        .await;

        assert!(execution.is_error);
        assert_eq!(execution.output["ok"], false);
        assert_eq!(execution.output["tool"], "browser_open");
        assert_eq!(execution.output["sessionId"], "session-42");
        assert_eq!(execution.output["url"], "https://example.com");
        assert_eq!(
            execution.output["error"],
            json!({
                "code": "sidecar_unavailable",
                "message": "missing sidecar",
                "details": { "path": "/tmp/missing.py" }
            })
        );
    }

    #[tokio::test]
    async fn browser_operator_redacts_open_url_in_structured_errors() {
        let manager = make_manager(Err(BrowserOperatorError {
            code: "sidecar_timeout".to_string(),
            message: "timed out".to_string(),
            details: None,
        }));
        let execution = execute_facade_tool(
            &manager,
            "session-redact",
            BrowserFacadeTool::Open,
            r#"{"url":"https://example.com/login?token=secret#frag"}"#,
        )
        .await;

        assert!(execution.is_error);
        assert_eq!(
            execution.redacted_arguments["url"],
            "https://example.com/login?redacted#redacted"
        );
        assert_eq!(
            execution.output["url"],
            "https://example.com/login?redacted#redacted"
        );
    }

    #[tokio::test]
    async fn browser_operator_rejects_invalid_arguments_structured() {
        let manager = make_manager(Ok(json!({ "ok": true })));
        let execution: BrowserFacadeExecution = execute_facade_tool(
            &manager,
            "session-99",
            BrowserFacadeTool::Click,
            r#"{"foo":"bar"}"#,
        )
        .await;

        assert!(execution.is_error);
        assert_eq!(execution.output["ok"], false);
        assert_eq!(execution.output["tool"], "browser_click");
        assert_eq!(execution.output["sessionId"], "session-99");
        assert_eq!(execution.output["error"]["code"], "invalid_arguments");
    }

    #[tokio::test]
    async fn browser_operator_rejects_non_http_open_urls() {
        let manager = make_manager(Ok(json!({ "ok": true })));
        let execution = execute_facade_tool(
            &manager,
            "session-file",
            BrowserFacadeTool::Open,
            r#"{"url":"file:///etc/passwd"}"#,
        )
        .await;

        assert!(execution.is_error);
        assert_eq!(execution.output["ok"], false);
        assert_eq!(execution.output["error"]["code"], "invalid_arguments");
    }

    #[tokio::test]
    async fn browser_operator_rejects_open_urls_with_credentials() {
        let manager = make_manager(Ok(json!({ "ok": true })));
        let execution = execute_facade_tool(
            &manager,
            "session-creds",
            BrowserFacadeTool::Open,
            r#"{"url":"https://user:pass@example.com/private?token=secret#frag"}"#,
        )
        .await;

        assert!(execution.is_error);
        assert_eq!(execution.output["ok"], false);
        assert_eq!(execution.output["error"]["code"], "invalid_arguments");
        assert_eq!(
            execution.redacted_arguments["url"],
            "https://example.com/private?redacted#redacted"
        );
    }
}
