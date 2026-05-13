use super::ipc::{
    operation_from_plugin, ExecuteRequestEnvelope, InitializeRequest, IpcResponseEnvelope,
    PluginExecuteRequest, ShutdownRequest,
};
use super::lifecycle::PluginLifecyclePolicy;
use super::manifest::{
    PluginRetrievalManifest, PluginRetrievalResource, SUPPORTED_PROTOCOL_VERSION,
};
use crate::domain::retrieval::types::{
    RetrievalError, RetrievalProviderKind, RetrievalRequest, RetrievalResponse,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const MAX_RESPONSE_LINE_BYTES: usize = 5 * 1024 * 1024;
const MAX_STDERR_TAIL_BYTES: usize = 16 * 1024;

#[cfg(test)]
fn plugin_process_start_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub struct PluginProcess {
    plugin_id: String,
    manifest: PluginRetrievalManifest,
    lifecycle: PluginLifecyclePolicy,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<Vec<u8>>>,
}

impl PluginProcess {
    pub async fn start(
        plugin_id: impl Into<String>,
        manifest: PluginRetrievalManifest,
    ) -> Result<Self, RetrievalError> {
        let cancel = CancellationToken::new();
        Self::start_with_cancel(plugin_id, manifest, &cancel).await
    }

    pub async fn start_with_cancel(
        plugin_id: impl Into<String>,
        manifest: PluginRetrievalManifest,
        cancel: &CancellationToken,
    ) -> Result<Self, RetrievalError> {
        if cancel.is_cancelled() {
            return Err(RetrievalError::Cancelled);
        }
        #[cfg(test)]
        let _start_guard = plugin_process_start_lock().lock().await;
        let plugin_id = plugin_id.into();
        let lifecycle = PluginLifecyclePolicy::from_runtime(&manifest.runtime);
        let mut command = Command::new(&manifest.runtime.command);
        command
            .args(&manifest.runtime.args)
            .envs(manifest.runtime.env.iter())
            .current_dir(&manifest.runtime.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| RetrievalError::Protocol {
            plugin: Some(plugin_id.clone()),
            message: format!("spawn plugin process: {err}"),
        })?;
        if cancel.is_cancelled() {
            let _ = child.kill().await;
            return Err(RetrievalError::Cancelled);
        }
        let stderr_tail = Arc::new(Mutex::new(Vec::new()));
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(drain_stderr(stderr, Arc::clone(&stderr_tail)));
        }
        let stdin = child.stdin.take().ok_or_else(|| RetrievalError::Protocol {
            plugin: Some(plugin_id.clone()),
            message: "plugin stdin unavailable".to_string(),
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RetrievalError::Protocol {
                plugin: Some(plugin_id.clone()),
                message: "plugin stdout unavailable".to_string(),
            })?;
        let mut process = Self {
            plugin_id,
            manifest,
            lifecycle,
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_tail,
        };
        process.initialize_with_cancel(cancel).await?;
        Ok(process)
    }

    pub async fn execute(
        &mut self,
        request: &RetrievalRequest,
        credentials: HashMap<String, String>,
    ) -> Result<RetrievalResponse, RetrievalError> {
        let cancel = CancellationToken::new();
        self.execute_with_cancel(request, credentials, &cancel)
            .await
    }

    pub async fn execute_with_cancel(
        &mut self,
        request: &RetrievalRequest,
        credentials: HashMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<RetrievalResponse, RetrievalError> {
        let id = request.request_id.clone();
        let envelope = ExecuteRequestEnvelope {
            id: id.clone(),
            message_type: "execute".to_string(),
            request: PluginExecuteRequest::from_retrieval_request(request, credentials),
        };
        let response = self
            .send_and_read_bounded(&id, &envelope, self.lifecycle.request_timeout, cancel)
            .await?;
        self.execution_response(request, response)
    }

    pub async fn shutdown(&mut self) {
        let id = "shutdown".to_string();
        let envelope = ShutdownRequest {
            id: id.clone(),
            message_type: "shutdown".to_string(),
        };
        let _ = tokio::time::timeout(
            self.lifecycle.cancel_grace,
            self.send_and_read(&id, &envelope),
        )
        .await;
        if tokio::time::timeout(self.lifecycle.kill_grace, self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }

    async fn initialize_with_cancel(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<(), RetrievalError> {
        let id = "initialize".to_string();
        let envelope = InitializeRequest {
            id: id.clone(),
            message_type: "initialize".to_string(),
            protocol_version: SUPPORTED_PROTOCOL_VERSION,
            plugin_id: self.plugin_id.clone(),
        };
        let response = self
            .send_and_read_bounded(
                &id,
                &envelope,
                self.lifecycle.initialization_timeout,
                cancel,
            )
            .await?;
        if response.message_type != "initialized" {
            return Err(RetrievalError::Protocol {
                plugin: Some(self.plugin_id.clone()),
                message: format!(
                    "expected initialized response, got `{}`",
                    response.message_type
                ),
            });
        }
        if response.protocol_version != Some(SUPPORTED_PROTOCOL_VERSION) {
            return Err(RetrievalError::Protocol {
                plugin: Some(self.plugin_id.clone()),
                message: "plugin initialized with unsupported protocol version".to_string(),
            });
        }
        self.validate_initialized_resources(&response)
    }

    fn validate_initialized_resources(
        &self,
        response: &IpcResponseEnvelope,
    ) -> Result<(), RetrievalError> {
        let initialized = response
            .resources
            .iter()
            .map(|source| (source.category.as_str(), source.id.as_str()))
            .collect::<HashSet<_>>();
        for source in &self.manifest.resources {
            if !initialized.contains(&(source.category.as_str(), source.id.as_str())) {
                return Err(RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: format!(
                        "plugin did not initialize declared retrieval resource {}.{}",
                        source.category, source.id
                    ),
                });
            }
            validate_resource_capabilities(&self.plugin_id, source, response)?;
        }
        Ok(())
    }

    async fn send_and_read<T: Serialize + ?Sized>(
        &mut self,
        id: &str,
        message: &T,
    ) -> Result<IpcResponseEnvelope, RetrievalError> {
        self.send_json(message).await?;
        self.read_response(id).await
    }

    async fn send_and_read_bounded<T: Serialize + ?Sized>(
        &mut self,
        id: &str,
        message: &T,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<IpcResponseEnvelope, RetrievalError> {
        let result = tokio::select! {
            result = tokio::time::timeout(timeout, self.send_and_read(id, message)) => result,
            _ = cancel.cancelled() => {
                self.shutdown().await;
                return Err(RetrievalError::Cancelled);
            }
        };
        match result {
            Ok(result) => result,
            Err(_) => {
                let _ = self.child.kill().await;
                Err(RetrievalError::Timeout {
                    seconds: timeout.as_secs().max(1),
                })
            }
        }
    }

    async fn send_json<T: Serialize + ?Sized>(
        &mut self,
        message: &T,
    ) -> Result<(), RetrievalError> {
        let raw = serde_json::to_string(message).map_err(|err| RetrievalError::Protocol {
            plugin: Some(self.plugin_id.clone()),
            message: format!("serialize plugin request: {err}"),
        })?;
        self.stdin
            .write_all(raw.as_bytes())
            .await
            .map_err(|err| RetrievalError::Protocol {
                plugin: Some(self.plugin_id.clone()),
                message: format!("write plugin request: {err}"),
            })?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|err| RetrievalError::Protocol {
                plugin: Some(self.plugin_id.clone()),
                message: format!("write plugin request newline: {err}"),
            })?;
        self.stdin
            .flush()
            .await
            .map_err(|err| RetrievalError::Protocol {
                plugin: Some(self.plugin_id.clone()),
                message: format!("flush plugin request: {err}"),
            })
    }

    async fn read_response(&mut self, id: &str) -> Result<IpcResponseEnvelope, RetrievalError> {
        let mut line = String::new();
        loop {
            line.clear();
            let n =
                self.stdout
                    .read_line(&mut line)
                    .await
                    .map_err(|err| RetrievalError::Protocol {
                        plugin: Some(self.plugin_id.clone()),
                        message: format!("read plugin response: {err}"),
                    })?;
            if n == 0 {
                let stderr = self.stderr_excerpt().await;
                let message = if stderr.is_empty() {
                    "plugin exited before response".to_string()
                } else {
                    format!("plugin exited before response; stderr: {stderr}")
                };
                return Err(RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message,
                });
            }
            if line.len() > MAX_RESPONSE_LINE_BYTES {
                return Err(RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: "plugin response exceeded maximum JSONL line size".to_string(),
                });
            }
            let response: IpcResponseEnvelope =
                serde_json::from_str(line.trim_end()).map_err(|err| RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: format!("parse plugin response JSON: {err}"),
                })?;
            if response.id == id {
                return Ok(response);
            }
        }
    }

    fn execution_response(
        &self,
        request: &RetrievalRequest,
        envelope: IpcResponseEnvelope,
    ) -> Result<RetrievalResponse, RetrievalError> {
        match envelope.message_type.as_str() {
            "result" => {
                let response = envelope.response.ok_or_else(|| RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: "result response missing `response`".to_string(),
                })?;
                if response.ok == Some(false) {
                    return Err(RetrievalError::ExecutionFailed {
                        message: "plugin returned ok=false".to_string(),
                    });
                }
                let operation = operation_from_plugin(&response.operation).ok_or_else(|| {
                    RetrievalError::Protocol {
                        plugin: Some(self.plugin_id.clone()),
                        message: format!(
                            "unsupported plugin response operation `{}`",
                            response.operation
                        ),
                    }
                })?;
                if operation != request.operation {
                    return Err(RetrievalError::Protocol {
                        plugin: Some(self.plugin_id.clone()),
                        message: format!(
                            "plugin response operation `{}` did not match request `{}`",
                            operation.as_str(),
                            request.operation.as_str()
                        ),
                    });
                }
                Ok(RetrievalResponse {
                    operation,
                    category: response.category,
                    source: response.source.clone(),
                    effective_source: response.effective_source.unwrap_or(response.source),
                    provider: RetrievalProviderKind::Plugin,
                    plugin: Some(self.plugin_id.clone()),
                    items: response.items.into_iter().map(Into::into).collect(),
                    detail: response.detail.map(Into::into),
                    total: response.total,
                    notes: response.notes,
                    raw: response.raw,
                })
            }
            "error" => {
                let error = envelope.error.ok_or_else(|| RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: "error response missing `error`".to_string(),
                })?;
                Err(RetrievalError::ExecutionFailed {
                    message: format!("plugin error {}: {}", error.code, error.message),
                })
            }
            other => Err(RetrievalError::Protocol {
                plugin: Some(self.plugin_id.clone()),
                message: format!(
                    "expected result/error response for request {}, got `{other}`",
                    request.request_id
                ),
            }),
        }
    }

    async fn stderr_excerpt(&self) -> String {
        String::from_utf8_lossy(&self.stderr_tail.lock().await)
            .trim()
            .to_string()
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn validate_resource_capabilities(
    plugin_id: &str,
    source: &PluginRetrievalResource,
    response: &IpcResponseEnvelope,
) -> Result<(), RetrievalError> {
    let Some(initialized_resource) = response
        .resources
        .iter()
        .find(|item| item.category == source.category && item.id == source.id)
    else {
        return Ok(());
    };
    let capabilities = initialized_resource
        .capabilities
        .iter()
        .map(|capability| {
            capability
                .trim()
                .to_ascii_lowercase()
                .replace(['-', ' '], "_")
        })
        .collect::<HashSet<_>>();
    for capability in &source.capabilities {
        if !capabilities.contains(capability) {
            return Err(RetrievalError::Protocol {
                plugin: Some(plugin_id.to_string()),
                message: format!(
                    "plugin did not initialize declared capability {} for source {}.{}",
                    capability, source.category, source.id
                ),
            });
        }
    }
    Ok(())
}

async fn drain_stderr(mut stderr: tokio::process::ChildStderr, tail: Arc<Mutex<Vec<u8>>>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin_runtime::retrieval::manifest::load_plugin_retrieval_manifest;
    use crate::domain::retrieval::types::{RetrievalOperation, RetrievalTool};
    use serde_json::json;
    use std::fs;

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn write_mock_plugin(
        script_body: &str,
        timeout_ms: u64,
    ) -> (tempfile::TempDir, PluginRetrievalManifest) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("mock_plugin.py");
        fs::write(&script, script_body).unwrap();
        fs::File::open(&script).unwrap().sync_all().unwrap();
        #[cfg(unix)]
        make_executable(&script);
        let manifest = load_plugin_retrieval_manifest(
            dir.path(),
            json!({
                "protocolVersion": 1,
                "runtime": {
                    "command": "./mock_plugin.py",
                    "requestTimeoutMs": timeout_ms,
                    "cancelGraceMs": 10,
                    "concurrency": 1
                },
                "resources": [{
                    "id": "mock_source",
                    "category": "dataset",
                    "capabilities": ["search", "fetch", "query"]
                }]
            }),
        )
        .unwrap();
        (dir, manifest)
    }

    fn request(query: &str) -> RetrievalRequest {
        RetrievalRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            tool: RetrievalTool::Search,
            operation: RetrievalOperation::Search,
            category: "dataset".to_string(),
            source: "mock_source".to_string(),
            subcategory: None,
            query: Some(query.to_string()),
            id: None,
            url: None,
            result: None,
            params: None,
            max_results: Some(5),
            prompt: None,
            web: None,
        }
    }

    fn documented_basic_fixture_manifest() -> (std::path::PathBuf, PluginRetrievalManifest) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/retrieval-plugins/basic");
        let plugin_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("plugin.json")).unwrap()).unwrap();
        let manifest = load_plugin_retrieval_manifest(
            &root,
            plugin_json
                .get("retrieval")
                .cloned()
                .expect("fixture has retrieval manifest"),
        )
        .unwrap();
        (root, manifest)
    }

    fn fixture_request(operation: RetrievalOperation, marker: &str) -> RetrievalRequest {
        RetrievalRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            tool: match operation {
                RetrievalOperation::Fetch => RetrievalTool::Fetch,
                RetrievalOperation::Query => RetrievalTool::Query,
                _ => RetrievalTool::Search,
            },
            operation,
            category: "dataset".to_string(),
            source: "example_dataset".to_string(),
            subcategory: None,
            query: matches!(
                operation,
                RetrievalOperation::Search | RetrievalOperation::Query
            )
            .then(|| marker.to_string()),
            id: matches!(operation, RetrievalOperation::Fetch).then(|| "example-1".to_string()),
            url: None,
            result: None,
            params: Some(json!({"organism": "human"})),
            max_results: Some(5),
            prompt: matches!(operation, RetrievalOperation::Fetch)
                .then(|| format!("fetch fixture detail for {marker}")),
            web: None,
        }
    }

    const MOCK_PLUGIN: &str = r#"#!/usr/bin/env python3
import json
import sys
import time

for line in sys.stdin:
    msg = json.loads(line)
    msg_type = msg.get("type")
    if msg_type == "initialize":
        print(json.dumps({
            "id": msg["id"],
            "type": "initialized",
            "protocolVersion": 1,
            "resources": [{
                "category": "dataset",
                "id": "mock_source",
                "capabilities": ["search", "fetch", "query"]
            }]
        }), flush=True)
    elif msg_type == "execute":
        req = msg["request"]
        query = req.get("query")
        if query == "sleep":
            time.sleep(5)
            continue
        if query == "bad_json":
            print("{bad json", flush=True)
            continue
        if query == "error":
            print(json.dumps({
                "id": msg["id"],
                "type": "error",
                "error": {"code": "mock_error", "message": "boom"}
            }), flush=True)
            continue
        print(json.dumps({
            "id": msg["id"],
            "type": "result",
            "response": {
                "ok": True,
                "operation": req.get("operation", "search"),
                "category": req.get("category", "dataset"),
                "source": req.get("source", "mock_source"),
                "effectiveSource": req.get("source", "mock_source"),
                "items": [{
                    "id": "mock-1",
                    "title": "Mock Result",
                    "url": "https://example.test/mock-1",
                    "snippet": "Snippet",
                    "metadata": {"credential_keys": sorted(req.get("credentials", {}).keys())}
                }],
                "total": 1,
                "notes": ["mock"]
            }
        }), flush=True)
    elif msg_type == "shutdown":
        print(json.dumps({"id": msg["id"], "type": "shutdown"}), flush=True)
        break
"#;

    #[tokio::test]
    async fn process_executes_mock_search_result() {
        let (_dir, manifest) = write_mock_plugin(MOCK_PLUGIN, 1_000);
        let mut process = PluginProcess::start("mock-plugin", manifest).await.unwrap();
        let response = process
            .execute(
                &request("hello"),
                HashMap::from([("pubmed_email".to_string(), "dev@example.test".to_string())]),
            )
            .await
            .unwrap();

        assert_eq!(response.provider, RetrievalProviderKind::Plugin);
        assert_eq!(response.plugin.as_deref(), Some("mock-plugin"));
        assert_eq!(response.operation, RetrievalOperation::Search);
        assert_eq!(response.category, "dataset");
        assert_eq!(response.effective_source, "mock_source");
        assert_eq!(response.items[0].title.as_deref(), Some("Mock Result"));
        assert_eq!(
            response.items[0].metadata["credential_keys"],
            json!(["pubmed_email"])
        );
        process.shutdown().await;
    }

    #[tokio::test]
    async fn documented_basic_fixture_executes_search_query_and_fetch_protocol() {
        let (_root, manifest) = documented_basic_fixture_manifest();
        #[cfg(unix)]
        make_executable(&manifest.runtime.command);
        let mut process = PluginProcess::start("retrieval-protocol-example", manifest)
            .await
            .unwrap();
        let credentials =
            HashMap::from([("pubmed_email".to_string(), "dev@example.test".to_string())]);

        let search = process
            .execute(
                &fixture_request(RetrievalOperation::Search, "BRCA1"),
                credentials.clone(),
            )
            .await
            .unwrap();
        assert_eq!(search.operation, RetrievalOperation::Search);
        assert_eq!(search.effective_source, "example_dataset");
        assert_eq!(search.items.len(), 1);
        assert_eq!(
            search.items[0].metadata["credentialRefs"],
            json!(["pubmed_email"])
        );

        let query = process
            .execute(
                &fixture_request(RetrievalOperation::Query, "sample metadata"),
                credentials.clone(),
            )
            .await
            .unwrap();
        assert_eq!(query.operation, RetrievalOperation::Query);
        assert_eq!(query.items.len(), 2);
        assert_eq!(query.total, Some(2));

        let fetch = process
            .execute(
                &fixture_request(RetrievalOperation::Fetch, "example-1"),
                credentials,
            )
            .await
            .unwrap();
        assert_eq!(fetch.operation, RetrievalOperation::Fetch);
        assert!(fetch.items.is_empty());
        let detail = fetch.detail.expect("fetch response has detail");
        assert_eq!(detail.id.as_deref(), Some("example-1"));
        assert_eq!(
            detail.metadata["fixture"],
            json!("retrieval-protocol-example")
        );

        process.shutdown().await;
    }

    #[tokio::test]
    async fn process_returns_plugin_error() {
        let (_dir, manifest) = write_mock_plugin(MOCK_PLUGIN, 1_000);
        let mut process = PluginProcess::start("mock-plugin", manifest).await.unwrap();
        let err = process
            .execute(&request("error"), HashMap::new())
            .await
            .unwrap_err();

        assert!(matches!(err, RetrievalError::ExecutionFailed { .. }));
        assert!(err.to_string().contains("mock_error"));
        process.shutdown().await;
    }

    #[tokio::test]
    async fn process_rejects_bad_json_response() {
        let (_dir, manifest) = write_mock_plugin(MOCK_PLUGIN, 1_000);
        let mut process = PluginProcess::start("mock-plugin", manifest).await.unwrap();
        let err = process
            .execute(&request("bad_json"), HashMap::new())
            .await
            .unwrap_err();

        assert!(matches!(err, RetrievalError::Protocol { .. }));
        assert!(err.to_string().contains("parse plugin response JSON"));
        process.shutdown().await;
    }

    #[tokio::test]
    async fn process_timeout_kills_child() {
        let (_dir, manifest) = write_mock_plugin(MOCK_PLUGIN, 100);
        let mut process = PluginProcess::start("mock-plugin", manifest).await.unwrap();
        let err = process
            .execute(&request("sleep"), HashMap::new())
            .await
            .unwrap_err();

        assert!(matches!(err, RetrievalError::Timeout { .. }));
    }

    #[tokio::test]
    async fn process_cancellation_shuts_down_child_and_returns_cancelled() {
        let (_dir, manifest) = write_mock_plugin(MOCK_PLUGIN, 5_000);
        let mut process = PluginProcess::start("mock-plugin", manifest).await.unwrap();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            trigger.cancel();
        });

        let err = process
            .execute_with_cancel(&request("sleep"), HashMap::new(), &cancel)
            .await
            .unwrap_err();

        assert!(matches!(err, RetrievalError::Cancelled));
    }
}
