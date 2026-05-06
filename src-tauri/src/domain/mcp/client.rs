//! Connect to MCP servers via **rmcp** (stdio or streamable HTTP) and run `resources/*` calls.

use crate::domain::mcp::config::{merged_mcp_servers, McpServerConfig};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::model::Tool as McpTool;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ReadResourceResult,
};
use rmcp::service::{Peer, RoleClient, RunningService};
use rmcp::transport::{
    streamable_http_client::{StreamableHttpClientTransportConfig, StreamableHttpClientWorker},
    AuthClient, ConfigureCommandExt, TokioChildProcess,
};
use rmcp::ServiceExt;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

use std::time::Instant;

/// Connection type for lifecycle management
#[derive(Debug, Clone, PartialEq)]
pub enum McpConnectionType {
    /// Local stdio process (needs special lifecycle management)
    Stdio,
    /// Remote HTTP/SSE connection
    Remote,
}

/// A live, reusable MCP connection.
/// Keep this value alive (e.g. in a HashMap) to hold the background I/O pump open.
/// Drop it to cancel the pump and (for stdio servers) kill the child process.
pub struct McpLiveConnection {
    /// Cloneable handle used to send requests; cheap to clone.
    pub peer: Peer<RoleClient>,
    /// Background service that drives the transport; must stay alive.
    _running: RunningService<RoleClient, ()>,
    /// Normalized-tool-name → original-server-tool-name map, built once on connect.
    /// Avoids calling `tools/list` on every tool invocation.
    pub tool_name_map: std::collections::HashMap<String, String>,
    /// Connection type for lifecycle decisions
    pub connection_type: McpConnectionType,
    /// Last time this connection was used for a tool call
    pub last_used: Instant,
    /// Session ID that created this connection (for session boundary detection)
    pub created_in_session: String,
    /// For stdio connections: store the child process ID for health checks
    #[cfg(unix)]
    pub pid: Option<i32>,
    #[cfg(windows)]
    pub pid: Option<u32>,
}

impl McpLiveConnection {
    /// Returns true if the underlying transport has been cancelled or closed.
    pub fn is_closed(&self) -> bool {
        self._running.is_closed()
    }

    /// Update last_used timestamp
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    /// Check if this connection was created in a different session
    pub fn is_from_different_session(&self, current_session: &str) -> bool {
        self.created_in_session != current_session
    }

    /// Check if this stdio process is still alive (platform-specific).
    /// Uses `kill(pid, 0)` which checks process existence without sending any signal.
    #[cfg(unix)]
    pub fn is_process_alive(&self) -> bool {
        match self.pid {
            None => {
                // PID not available (rmcp doesn't expose child PID after handshake).
                // Assume alive and let the is_closed() check catch dead transports.
                true
            }
            Some(pid) => {
                // Signal 0 checks process existence without sending a real signal.
                matches!(
                    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
                    Ok(())
                )
            }
        }
    }

    /// Check if this stdio process is still alive (Windows implementation)
    #[cfg(windows)]
    pub fn is_process_alive(&self) -> bool {
        match self.pid {
            None => false,
            Some(pid) => {
                use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
                use windows_sys::Win32::System::Threading::OpenProcess;
                use windows_sys::Win32::System::Threading::PROCESS_QUERY_INFORMATION;

                unsafe {
                    let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
                    if handle == INVALID_HANDLE_VALUE {
                        return false;
                    }
                    CloseHandle(handle);
                    true
                }
            }
        }
    }
}

/// Connection metadata returned when establishing a connection
pub struct ConnectionMeta {
    pub connection_type: McpConnectionType,
    #[cfg(unix)]
    pub pid: Option<i32>,
    #[cfg(windows)]
    pub pid: Option<u32>,
}

/// Establish a persistent connection to a named MCP server and eagerly fetch the tool list.
/// Returns an [`McpLiveConnection`] whose lifetime controls the underlying process / HTTP session.
/// The embedded `tool_name_map` lets callers resolve normalized → original tool names without
/// a second round-trip.
///
/// # Arguments
/// * `project_root` - Project root path for config resolution and working directory
/// * `server_name` - Name of the MCP server to connect to
/// * `timeout` - Connection and handshake timeout
/// * `session_id` - Current session ID for session boundary tracking
pub async fn connect_mcp_server(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
    session_id: &str,
) -> Result<McpLiveConnection, String> {
    use crate::domain::mcp::config::McpServerConfig;
    use crate::domain::mcp::names::normalize_name_for_mcp;

    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    // Determine connection type before consuming cfg
    let connection_type = match &cfg {
        McpServerConfig::Stdio { .. } => McpConnectionType::Stdio,
        McpServerConfig::Url { .. } => McpConnectionType::Remote,
    };

    let running = create_running_service(project_root, cfg, timeout).await?;
    let peer = running.peer().clone();

    // Eagerly fetch tool list so every subsequent call can resolve names without a round-trip.
    let tools = list_tools_via_peer(&peer, timeout)
        .await
        .unwrap_or_default();
    let tool_name_map: std::collections::HashMap<String, String> = tools
        .into_iter()
        .map(|t| (normalize_name_for_mcp(t.name.as_ref()), t.name.to_string()))
        .collect();

    // rmcp 1.x does not expose the child process after `serve()` completes the handshake,
    // so the PID is unavailable. `is_process_alive()` falls back to `is_closed()` when
    // `pid` is None, which is sufficient for health-check purposes.
    let pid = None;

    Ok(McpLiveConnection {
        peer,
        _running: running,
        tool_name_map,
        connection_type,
        last_used: Instant::now(),
        created_in_session: session_id.to_string(),
        pid,
    })
}

/// Legacy entry point without session tracking (for backwards compatibility)
/// Prefer the new `connect_mcp_server` with session_id parameter
pub async fn connect_mcp_server_legacy(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
) -> Result<McpLiveConnection, String> {
    // Use a sentinel session ID for legacy calls
    connect_mcp_server(project_root, server_name, timeout, "legacy").await
}

/// Call a tool using a pre-established peer (no new connection overhead).
pub async fn call_tool_via_peer(
    peer: &Peer<RoleClient>,
    tool_name: &str,
    arguments: Option<serde_json::Map<String, serde_json::Value>>,
    timeout: Duration,
) -> Result<CallToolResult, String> {
    let peer = peer.clone();
    let tool_name = tool_name.to_string();
    tokio::time::timeout(timeout, async move {
        let mut params = CallToolRequestParams::new(tool_name);
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        peer.call_tool(params)
            .await
            .map_err(|e| format!("tools/call: {e}"))
    })
    .await
    .map_err(|_| "MCP operation timed out".to_string())?
}

/// List tools using a pre-established peer.
pub async fn list_tools_via_peer(
    peer: &Peer<RoleClient>,
    timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let peer = peer.clone();
    tokio::time::timeout(timeout, async move {
        peer.list_all_tools()
            .await
            .map_err(|e| format!("tools/list: {e}"))
    })
    .await
    .map_err(|_| "MCP operation timed out".to_string())?
}

/// List resources from one named server (MCP `resources/list` with pagination via rmcp).
pub async fn list_resources_for_server(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
) -> Result<Vec<serde_json::Value>, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        let resources = peer
            .list_all_resources()
            .await
            .map_err(|e| format!("resources/list: {e}"))?;
        let mut out = Vec::with_capacity(resources.len());
        for r in resources {
            let mut v = serde_json::to_value(&r)
                .unwrap_or_else(|_| json!({ "error": "serialize Resource" }));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("server".to_string(), json!(server_name));
            }
            out.push(v);
        }
        Ok(out)
    })
    .await
}

/// Read one resource (MCP `resources/read`).
pub async fn read_resource_for_server(
    project_root: &Path,
    server_name: &str,
    uri: &str,
    timeout: Duration,
) -> Result<ReadResourceResult, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        peer.read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|e| format!("resources/read: {e}"))
    })
    .await
}

/// List tools from one named server (MCP `tools/list`).
pub async fn list_tools_for_server(
    project_root: &Path,
    server_name: &str,
    timeout: Duration,
) -> Result<Vec<McpTool>, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        peer.list_all_tools()
            .await
            .map_err(|e| format!("tools/list: {e}"))
    })
    .await
}

/// Call a tool on a named server (MCP `tools/call`).
/// Prefer [`call_tool_via_peer`] with a pooled connection to avoid per-call spawn overhead.
pub async fn call_tool_on_server(
    project_root: &Path,
    server_name: &str,
    tool_name: &str,
    arguments: Option<serde_json::Map<String, serde_json::Value>>,
    timeout: Duration,
) -> Result<CallToolResult, String> {
    let cfg = merged_mcp_servers(project_root)
        .remove(server_name)
        .ok_or_else(|| {
            format!(
                "MCP server \"{server_name}\" not found in merged Omiga MCP config (bundled defaults, ~/.omiga/mcp.json, <project>/.omiga/mcp.json)"
            )
        })?;

    with_mcp_peer(project_root, cfg, timeout, |peer| async move {
        let mut params = CallToolRequestParams::new(tool_name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        peer.call_tool(params)
            .await
            .map_err(|e| format!("tools/call: {e}"))
    })
    .await
}

/// Create a `RunningService` from a config (used by both the pool and one-shot helpers).
///
/// For remote HTTP connections, this function implements retry logic with exponential backoff
/// to handle transient network failures (e.g., "Connection reset by peer").
async fn create_running_service(
    project_root: &Path,
    config: McpServerConfig,
    timeout: Duration,
) -> Result<RunningService<RoleClient, ()>, String> {
    match config {
        McpServerConfig::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let resolved_command = resolve_stdio_command(&command)?;
            let current_dir = resolve_stdio_cwd(project_root, cwd.as_deref());
            let transport =
                TokioChildProcess::new(Command::new(&resolved_command).configure(|cmd| {
                    cmd.args(&args);
                    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                    cmd.current_dir(&current_dir);
                    cmd.stdin(std::process::Stdio::piped());
                    cmd.stdout(std::process::Stdio::piped());
                    cmd.stderr(std::process::Stdio::piped());
                }))
                .map_err(|e| format!("MCP stdio spawn: {e}"))?;

            tokio::time::timeout(timeout, ().serve(transport))
                .await
                .map_err(|_| "MCP stdio handshake timed out".to_string())?
                .map_err(|e| format!("MCP stdio handshake: {e}"))
        }
        McpServerConfig::Url { url, headers } => {
            // Retry configuration for HTTP connections
            const MAX_RETRIES: u32 = 3;
            const INITIAL_BACKOFF_MS: u64 = 500;

            let mut last_error = None;
            let local_proxy = local_env_proxy_for_http();
            let mut bypass_env_proxy = false;
            let default_headers = build_http_header_map(&headers)?;
            let has_explicit_authorization = headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case("authorization"));

            for attempt in 0..MAX_RETRIES {
                if attempt > 0 {
                    let backoff = INITIAL_BACKOFF_MS * (1 << (attempt - 1)); // Exponential backoff
                    tracing::info!(
                        target: "omiga::mcp",
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = MAX_RETRIES,
                        backoff_ms = backoff,
                        "Retrying MCP HTTP connection after failure"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(backoff)).await;
                }

                // Create a fresh client for each attempt to avoid connection reuse issues
                let mut http_builder = reqwest::Client::builder()
                    .timeout(timeout)
                    .pool_max_idle_per_host(0); // Disable connection pooling to avoid stale connections
                if !default_headers.is_empty() {
                    http_builder = http_builder.default_headers(default_headers.clone());
                }
                if bypass_env_proxy {
                    http_builder = http_builder.no_proxy();
                }
                let http = http_builder
                    .build()
                    .map_err(|e| format!("reqwest client build: {e}"))?;

                let transport_config = StreamableHttpClientTransportConfig::with_uri(url.clone());
                let service_result = if !has_explicit_authorization {
                    match crate::domain::mcp::oauth::stored_auth_manager_for_url(&url, http.clone())
                        .await
                    {
                        Ok(Some(auth_manager)) => {
                            serve_http_transport(
                                AuthClient::new(http.clone(), auth_manager),
                                transport_config,
                                timeout,
                            )
                            .await
                        }
                        Ok(None) => serve_http_transport(http, transport_config, timeout).await,
                        Err(err) => Err(err),
                    }
                } else {
                    serve_http_transport(http, transport_config, timeout).await
                };

                match service_result {
                    Ok(service) => {
                        if attempt > 0 {
                            tracing::info!(
                                target: "omiga::mcp",
                                url = %url,
                                attempt = attempt + 1,
                                "MCP HTTP connection succeeded after retry"
                            );
                        }
                        return Ok(service);
                    }
                    Err(error_msg) => {
                        // Check if this is a retriable error
                        let error_lower = error_msg.to_ascii_lowercase();
                        let is_retriable = error_lower.contains("connection reset")
                            || error_lower.contains("connection refused")
                            || error_lower.contains("connect")
                            || error_lower.contains("timed out")
                            || error_lower.contains("timeout");

                        tracing::warn!(
                            target: "omiga::mcp",
                            url = %url,
                            attempt = attempt + 1,
                            error = %error_msg,
                            is_retriable = is_retriable,
                            bypass_env_proxy = bypass_env_proxy,
                            "MCP HTTP handshake failed"
                        );

                        if is_retriable
                            && !bypass_env_proxy
                            && local_proxy.is_some()
                            && attempt < MAX_RETRIES - 1
                        {
                            let proxy = local_proxy.as_deref().unwrap_or("local proxy");
                            bypass_env_proxy = true;
                            last_error = Some(format!(
                                "{error_msg}; retrying without environment proxy {proxy}"
                            ));
                            tracing::warn!(
                                target: "omiga::mcp",
                                url = %url,
                                proxy = %proxy,
                                "MCP HTTP handshake failed while a localhost proxy is configured; retrying direct"
                            );
                            continue;
                        }

                        if !is_retriable || attempt == MAX_RETRIES - 1 {
                            return Err(with_http_proxy_context(
                                error_msg,
                                local_proxy.as_deref(),
                                bypass_env_proxy,
                            ));
                        }
                        last_error = Some(error_msg);
                    }
                }
            }

            // Should not reach here, but handle just in case
            Err(with_http_proxy_context(
                format!(
                    "MCP HTTP connection failed after {} retries: {}",
                    MAX_RETRIES,
                    last_error.unwrap_or_else(|| "Unknown error".to_string())
                ),
                local_proxy.as_deref(),
                bypass_env_proxy,
            ))
        }
    }
}

async fn serve_http_transport<C>(
    client: C,
    config: StreamableHttpClientTransportConfig,
    timeout: Duration,
) -> Result<RunningService<RoleClient, ()>, String>
where
    C: rmcp::transport::streamable_http_client::StreamableHttpClient,
{
    let worker = StreamableHttpClientWorker::new(client, config);
    tokio::time::timeout(timeout, ().serve(worker))
        .await
        .map_err(|_| "MCP HTTP handshake timed out".to_string())?
        .map_err(|err| format!("MCP HTTP handshake: {err}"))
}

fn local_env_proxy_for_http() -> Option<String> {
    local_env_proxy_for_http_from_vars(
        [
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "HTTP_PROXY",
            "http_proxy",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key, value))),
    )
}

fn local_env_proxy_for_http_from_vars<I, K, V>(vars: I) -> Option<String>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    vars.into_iter().find_map(|(key, value)| {
        let key = key.as_ref();
        let value = value.as_ref();
        let lower = value.to_ascii_lowercase();
        if lower.contains("127.0.0.1") || lower.contains("localhost") || lower.contains("[::1]") {
            Some(format!("{key}=<local proxy>"))
        } else {
            None
        }
    })
}

fn with_http_proxy_context(
    message: String,
    local_proxy: Option<&str>,
    retried_direct: bool,
) -> String {
    match (local_proxy, retried_direct) {
        (Some(proxy), true) => {
            format!(
                "{message}; detected environment proxy {proxy} and retried direct without proxy"
            )
        }
        (Some(proxy), false) => format!("{message}; detected environment proxy {proxy}"),
        _ => message,
    }
}

fn build_http_header_map(headers: &HashMap<String, String>) -> Result<HeaderMap, String> {
    let mut out = HeaderMap::new();
    for (name, raw_value) in headers {
        let header_name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|e| format!("Invalid MCP HTTP header name \"{name}\": {e}"))?;
        let value = expand_header_env_placeholders(raw_value)?;
        let header_value = HeaderValue::from_str(&value)
            .map_err(|e| format!("Invalid MCP HTTP header value for \"{name}\": {e}"))?;
        out.insert(header_name, header_value);
    }
    Ok(out)
}

fn expand_header_env_placeholders(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let name = &after_start[..end];
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        {
            return Err(format!(
                "Invalid environment placeholder \"${{{name}}}\" in MCP HTTP header"
            ));
        }
        let value = std::env::var(name).map_err(|_| {
            format!("Environment variable \"{name}\" referenced by MCP HTTP header is not set")
        })?;
        out.push_str(&value);
        rest = &after_start[end + 1..];
    }

    out.push_str(rest);
    Ok(out)
}

fn resolve_stdio_cwd(project_root: &Path, cwd: Option<&str>) -> PathBuf {
    let Some(raw) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return project_root.to_path_buf();
    };

    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    }
}

fn resolve_stdio_command(command: &str) -> Result<PathBuf, String> {
    if command == "__omiga_self__" {
        std::env::current_exe()
            .map_err(|e| format!("resolve __omiga_self__ to current executable: {e}"))
    } else {
        Ok(PathBuf::from(command))
    }
}

async fn with_mcp_peer<F, Fut, T>(
    project_root: &Path,
    config: McpServerConfig,
    timeout: Duration,
    work: F,
) -> Result<T, String>
where
    F: FnOnce(Peer<RoleClient>) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut running = create_running_service(project_root, config, timeout).await?;
    let peer = running.peer().clone();
    let out = tokio::time::timeout(timeout, work(peer))
        .await
        .map_err(|_| "MCP operation timed out".to_string())??;
    let _ = running.close().await;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        build_http_header_map, expand_header_env_placeholders, local_env_proxy_for_http_from_vars,
    };
    use std::collections::HashMap;

    #[test]
    fn detects_local_http_proxy_env_values() {
        assert_eq!(
            local_env_proxy_for_http_from_vars([
                ("HTTPS_PROXY", "https://proxy.example.com:443"),
                ("all_proxy", "socks5://127.0.0.1:9981"),
            ]),
            Some("all_proxy=<local proxy>".to_string())
        );
    }

    #[test]
    fn ignores_non_local_proxy_env_values() {
        assert_eq!(
            local_env_proxy_for_http_from_vars([
                ("HTTPS_PROXY", "https://proxy.example.com:443"),
                ("HTTP_PROXY", "http://10.0.0.2:3128"),
            ]),
            None
        );
    }

    #[test]
    fn builds_http_auth_headers() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());
        headers.insert("X-Api-Key".to_string(), "abc".to_string());

        let map = build_http_header_map(&headers).expect("headers");

        assert_eq!(map["authorization"], "Bearer token");
        assert_eq!(map["x-api-key"], "abc");
    }

    #[test]
    fn expands_http_header_env_placeholders() {
        unsafe {
            std::env::set_var("OMIGA_MCP_HEADER_TEST_TOKEN", "secret-token");
        }
        assert_eq!(
            expand_header_env_placeholders("Bearer ${OMIGA_MCP_HEADER_TEST_TOKEN}")
                .expect("expanded"),
            "Bearer secret-token"
        );
        unsafe {
            std::env::remove_var("OMIGA_MCP_HEADER_TEST_TOKEN");
        }
    }
}
