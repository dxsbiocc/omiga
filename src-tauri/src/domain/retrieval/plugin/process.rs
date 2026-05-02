use super::ipc::{
    operation_from_plugin, ExecuteRequestEnvelope, InitializeRequest, IpcResponseEnvelope,
    PluginExecuteRequest, ShutdownRequest,
};
use super::manifest::{PluginRetrievalManifest, PluginRetrievalSource, SUPPORTED_PROTOCOL_VERSION};
use crate::domain::retrieval::types::{
    RetrievalError, RetrievalProviderKind, RetrievalRequest, RetrievalResponse,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const MAX_RESPONSE_LINE_BYTES: usize = 5 * 1024 * 1024;
const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_INITIALIZATION_TIMEOUT_MS: u64 = 15_000;

pub struct PluginProcess {
    plugin_id: String,
    manifest: PluginRetrievalManifest,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl PluginProcess {
    pub async fn start(
        plugin_id: impl Into<String>,
        manifest: PluginRetrievalManifest,
    ) -> Result<Self, RetrievalError> {
        let plugin_id = plugin_id.into();
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
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(drain_stderr(stderr));
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
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        process.initialize().await?;
        Ok(process)
    }

    pub async fn execute(
        &mut self,
        request: &RetrievalRequest,
        credentials: HashMap<String, String>,
    ) -> Result<RetrievalResponse, RetrievalError> {
        let id = request.request_id.clone();
        let envelope = ExecuteRequestEnvelope {
            id: id.clone(),
            message_type: "execute".to_string(),
            request: PluginExecuteRequest::from_retrieval_request(request, credentials),
        };
        let timeout = self.request_timeout();
        let response = match tokio::time::timeout(timeout, self.send_and_read(&id, &envelope)).await
        {
            Ok(result) => result?,
            Err(_) => {
                let _ = self.child.kill().await;
                return Err(RetrievalError::Timeout {
                    seconds: timeout.as_secs().max(1),
                });
            }
        };
        self.execution_response(request, response)
    }

    pub async fn shutdown(&mut self) {
        let id = "shutdown".to_string();
        let envelope = ShutdownRequest {
            id: id.clone(),
            message_type: "shutdown".to_string(),
        };
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            self.send_and_read(&id, &envelope),
        )
        .await;
        if tokio::time::timeout(Duration::from_millis(200), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }

    async fn initialize(&mut self) -> Result<(), RetrievalError> {
        let id = "initialize".to_string();
        let envelope = InitializeRequest {
            id: id.clone(),
            message_type: "initialize".to_string(),
            protocol_version: SUPPORTED_PROTOCOL_VERSION,
            plugin_id: self.plugin_id.clone(),
        };
        let timeout = self.initialization_timeout();
        let response = match tokio::time::timeout(timeout, self.send_and_read(&id, &envelope)).await
        {
            Ok(result) => result?,
            Err(_) => {
                let _ = self.child.kill().await;
                return Err(RetrievalError::Timeout {
                    seconds: timeout.as_secs().max(1),
                });
            }
        };
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
        self.validate_initialized_sources(&response)
    }

    fn validate_initialized_sources(
        &self,
        response: &IpcResponseEnvelope,
    ) -> Result<(), RetrievalError> {
        let initialized = response
            .sources
            .iter()
            .map(|source| (source.category.as_str(), source.id.as_str()))
            .collect::<HashSet<_>>();
        for source in &self.manifest.sources {
            if !initialized.contains(&(source.category.as_str(), source.id.as_str())) {
                return Err(RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: format!(
                        "plugin did not initialize declared retrieval source {}.{}",
                        source.category, source.id
                    ),
                });
            }
            validate_source_capabilities(&self.plugin_id, source, response)?;
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
                return Err(RetrievalError::Protocol {
                    plugin: Some(self.plugin_id.clone()),
                    message: "plugin exited before response".to_string(),
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

    fn request_timeout(&self) -> Duration {
        Duration::from_millis(
            self.manifest
                .runtime
                .request_timeout_ms
                .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS)
                .max(1),
        )
    }

    fn initialization_timeout(&self) -> Duration {
        self.request_timeout()
            .max(Duration::from_millis(DEFAULT_INITIALIZATION_TIMEOUT_MS))
    }
}

fn validate_source_capabilities(
    plugin_id: &str,
    source: &PluginRetrievalSource,
    response: &IpcResponseEnvelope,
) -> Result<(), RetrievalError> {
    let Some(initialized_source) = response
        .sources
        .iter()
        .find(|item| item.category == source.category && item.id == source.id)
    else {
        return Ok(());
    };
    let capabilities = initialized_source
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

async fn drain_stderr(mut stderr: tokio::process::ChildStderr) {
    let mut buf = [0_u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::retrieval::plugin::manifest::load_plugin_retrieval_manifest;
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
                    "concurrency": 1
                },
                "sources": [{
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
            "sources": [{
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
}
