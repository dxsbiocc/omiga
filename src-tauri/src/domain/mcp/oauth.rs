//! OAuth support for remote HTTP MCP servers.
//!
//! Browser login cookies stay inside the browser.  Omiga must run the MCP OAuth
//! authorization-code + PKCE flow itself, store the returned access token in the
//! existing secret-store abstraction, and inject that token into future MCP HTTP
//! handshakes.

use crate::domain::connectors::secret_store;
use crate::domain::mcp::config::{merged_mcp_servers, McpServerConfig};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rmcp::transport::auth::OAuthState;
use rmcp::transport::{AuthError, AuthorizationManager, CredentialStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use uuid::Uuid;

const MCP_OAUTH_TOKEN_SECRET: &str = "oauth_credentials";
const MCP_OAUTH_DEFAULT_CALLBACK_PORT: u16 = 17656;
const MCP_OAUTH_DEFAULT_CALLBACK_PATH: &str = "/mcp/oauth/callback";
const MCP_OAUTH_LOGIN_EXPIRES_IN: u64 = 10 * 60;
const MCP_OAUTH_LOGIN_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthLoginStartResult {
    pub server_name: String,
    pub login_session_id: String,
    pub authorization_url: String,
    pub expires_in: u64,
    pub interval_secs: u64,
    pub expires_at: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpOAuthLoginPollStatus {
    Pending,
    Complete,
    Expired,
    Denied,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthLoginPollResult {
    pub server_name: String,
    pub status: McpOAuthLoginPollStatus,
    pub message: String,
    pub interval_secs: u64,
}

struct McpOAuthLoginSession {
    server_name: String,
    oauth_state: OAuthState,
    expected_state: String,
    interval_secs: u64,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpOAuthCallbackSuccess {
    code: String,
    state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpOAuthProviderError {
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum McpOAuthCallbackResult {
    Success(McpOAuthCallbackSuccess),
    ProviderError(McpOAuthProviderError),
    ListenerError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum McpOAuthCallbackOutcome {
    Success(McpOAuthCallbackSuccess),
    ProviderError(McpOAuthProviderError),
    Invalid,
}

fn sessions() -> &'static Mutex<HashMap<String, McpOAuthLoginSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, McpOAuthLoginSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn callbacks() -> &'static Mutex<HashMap<String, McpOAuthCallbackResult>> {
    static CALLBACKS: OnceLock<Mutex<HashMap<String, McpOAuthCallbackResult>>> = OnceLock::new();
    CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mcp_oauth_callback_port() -> Result<u16, String> {
    let Ok(value) = std::env::var("OMIGA_MCP_OAUTH_CALLBACK_PORT") else {
        return Ok(MCP_OAUTH_DEFAULT_CALLBACK_PORT);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(MCP_OAUTH_DEFAULT_CALLBACK_PORT);
    }
    let parsed = value
        .parse::<u16>()
        .map_err(|err| format!("invalid MCP OAuth callback port `{value}`: {err}"))?;
    if parsed == 0 {
        return Err(
            "invalid MCP OAuth callback port `0`: choose a fixed local port between 1 and 65535"
                .to_string(),
        );
    }
    Ok(parsed)
}

fn mcp_oauth_callback_path() -> String {
    std::env::var("OMIGA_MCP_OAUTH_CALLBACK_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| value.starts_with('/') && value.len() > 1)
        .unwrap_or_else(|| MCP_OAUTH_DEFAULT_CALLBACK_PATH.to_string())
}

fn mcp_oauth_redirect_uri() -> Result<(String, u16, String), String> {
    let port = mcp_oauth_callback_port()?;
    let path = mcp_oauth_callback_path();
    Ok((format!("http://127.0.0.1:{port}{path}"), port, path))
}

fn endpoint_key(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.trim().as_bytes());
    let digest = hasher.finalize();
    format!("mcp_{}", hex::encode(&digest[..12]))
}

#[derive(Debug, Clone)]
pub(crate) struct McpCredentialStore {
    secret_id: String,
}

impl McpCredentialStore {
    pub(crate) fn for_url(url: &str) -> Self {
        Self {
            secret_id: endpoint_key(url),
        }
    }
}

pub(crate) fn has_stored_credentials_for_url(url: &str) -> bool {
    secret_store::read_connector_secret(&endpoint_key(url), MCP_OAUTH_TOKEN_SECRET)
        .ok()
        .flatten()
        .is_some()
}

pub(crate) fn clear_stored_credentials_for_url(url: &str) -> Result<(), String> {
    secret_store::delete_connector_secret(&endpoint_key(url), MCP_OAUTH_TOKEN_SECRET)
        .map_err(|err| format!("delete MCP OAuth credentials: {err}"))
}

#[async_trait]
impl CredentialStore for McpCredentialStore {
    async fn load(&self) -> Result<Option<rmcp::transport::auth::StoredCredentials>, AuthError> {
        let Some(raw) =
            secret_store::read_connector_secret(&self.secret_id, MCP_OAUTH_TOKEN_SECRET).map_err(
                |err| AuthError::InternalError(format!("read MCP OAuth credentials: {err}")),
            )?
        else {
            return Ok(None);
        };
        let credentials = serde_json::from_str::<rmcp::transport::auth::StoredCredentials>(&raw)
            .map_err(|err| {
                AuthError::InternalError(format!("parse MCP OAuth credentials: {err}"))
            })?;
        Ok(Some(credentials))
    }

    async fn save(
        &self,
        credentials: rmcp::transport::auth::StoredCredentials,
    ) -> Result<(), AuthError> {
        let raw = serde_json::to_string(&credentials).map_err(|err| {
            AuthError::InternalError(format!("serialize MCP OAuth credentials: {err}"))
        })?;
        secret_store::store_connector_secret(&self.secret_id, MCP_OAUTH_TOKEN_SECRET, &raw)
            .map_err(|err| AuthError::InternalError(format!("store MCP OAuth credentials: {err}")))
    }

    async fn clear(&self) -> Result<(), AuthError> {
        secret_store::delete_connector_secret(&self.secret_id, MCP_OAUTH_TOKEN_SECRET)
            .map_err(|err| AuthError::InternalError(format!("delete MCP OAuth credentials: {err}")))
    }
}

pub(crate) async fn stored_auth_manager_for_url(
    url: &str,
    http_client: reqwest::Client,
) -> Result<Option<AuthorizationManager>, String> {
    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|err| format!("initialize MCP OAuth manager: {err}"))?;
    manager
        .with_client(http_client)
        .map_err(|err| format!("configure MCP OAuth HTTP client: {err}"))?;
    manager.set_credential_store(McpCredentialStore::for_url(url));

    match manager.initialize_from_store().await {
        Ok(true) => Ok(Some(manager)),
        Ok(false) => Ok(None),
        Err(err) => {
            tracing::warn!(
                target: "omiga::mcp::oauth",
                url = %url,
                error = %err,
                "Stored MCP OAuth credentials could not be initialized"
            );
            Ok(None)
        }
    }
}

pub(crate) async fn start_mcp_oauth_login(
    project_root: &Path,
    server_name: &str,
) -> Result<McpOAuthLoginStartResult, String> {
    let server_name = server_name.trim().to_string();
    if server_name.is_empty() {
        return Err("MCP server name is required".to_string());
    }
    let cfg = merged_mcp_servers(project_root)
        .remove(&server_name)
        .ok_or_else(|| format!("MCP server `{server_name}` was not found in merged config"))?;
    let McpServerConfig::Url { url, headers } = cfg else {
        return Err(format!(
            "MCP server `{server_name}` is a stdio server; OAuth browser login only applies to remote HTTP MCP servers."
        ));
    };
    if headers
        .keys()
        .any(|key| key.eq_ignore_ascii_case("authorization"))
    {
        return Err(format!(
            "MCP server `{server_name}` already has an Authorization header configured; edit or remove the manual token before starting browser OAuth."
        ));
    }

    let (redirect_uri, callback_port, callback_path) = mcp_oauth_redirect_uri()?;
    let mut oauth_state = OAuthState::new(url.clone(), None)
        .await
        .map_err(|err| format!("create MCP OAuth state: {err}"))?;
    if let OAuthState::Unauthorized(manager) = &mut oauth_state {
        manager.set_credential_store(McpCredentialStore::for_url(&url));
    }
    oauth_state
        .start_authorization(&[], &redirect_uri, Some("Omiga"))
        .await
        .map_err(|err| format!("start MCP OAuth authorization: {err}"))?;
    let authorization_url = oauth_state
        .get_authorization_url()
        .await
        .map_err(|err| format!("read MCP OAuth authorization URL: {err}"))?;
    let expected_state = extract_query_param(&authorization_url, "state").ok_or_else(|| {
        "MCP OAuth authorization URL did not include a CSRF state parameter".to_string()
    })?;

    start_mcp_oauth_callback_listener(
        callback_port,
        callback_path,
        expected_state.clone(),
        MCP_OAUTH_LOGIN_EXPIRES_IN,
    )?;

    let expires_at =
        Utc::now() + Duration::seconds(MCP_OAUTH_LOGIN_EXPIRES_IN.min(i64::MAX as u64) as i64);
    let login_session_id = Uuid::new_v4().to_string();
    sessions()
        .lock()
        .map_err(|_| "MCP OAuth login session lock poisoned".to_string())?
        .insert(
            login_session_id.clone(),
            McpOAuthLoginSession {
                server_name: server_name.clone(),
                oauth_state,
                expected_state,
                interval_secs: MCP_OAUTH_LOGIN_INTERVAL_SECS,
                expires_at,
            },
        );

    Ok(McpOAuthLoginStartResult {
        server_name: server_name.clone(),
        login_session_id,
        authorization_url,
        expires_in: MCP_OAUTH_LOGIN_EXPIRES_IN,
        interval_secs: MCP_OAUTH_LOGIN_INTERVAL_SECS,
        expires_at: expires_at.to_rfc3339(),
        message: format!(
            "已打开 `{server_name}` 的 MCP OAuth 授权页。浏览器登录完成后，Omiga 会接收本地回调、交换 token，并自动重新验证工具列表。"
        ),
    })
}

pub(crate) async fn poll_mcp_oauth_login(
    login_session_id: &str,
) -> Result<McpOAuthLoginPollResult, String> {
    let login_session_id = login_session_id.trim().to_string();
    if login_session_id.is_empty() {
        return Err("MCP OAuth login session id is required".to_string());
    }

    let session = sessions()
        .lock()
        .map_err(|_| "MCP OAuth login session lock poisoned".to_string())?
        .remove(&login_session_id)
        .ok_or_else(|| "MCP OAuth login session expired or is unknown".to_string())?;

    if Utc::now() >= session.expires_at {
        return Ok(McpOAuthLoginPollResult {
            server_name: session.server_name,
            status: McpOAuthLoginPollStatus::Expired,
            message: "MCP OAuth 登录已超时，请重新点击连接。".to_string(),
            interval_secs: session.interval_secs,
        });
    }

    let callback = callbacks()
        .lock()
        .map_err(|_| "MCP OAuth callback lock poisoned".to_string())?
        .remove(&session.expected_state);

    let Some(callback) = callback else {
        let server_name = session.server_name.clone();
        let interval_secs = session.interval_secs;
        sessions()
            .lock()
            .map_err(|_| "MCP OAuth login session lock poisoned".to_string())?
            .insert(login_session_id, session);
        return Ok(McpOAuthLoginPollResult {
            server_name,
            status: McpOAuthLoginPollStatus::Pending,
            message: "等待浏览器授权完成…".to_string(),
            interval_secs,
        });
    };

    match callback {
        McpOAuthCallbackResult::Success(success) => {
            if success.state != session.expected_state {
                return Ok(McpOAuthLoginPollResult {
                    server_name: session.server_name,
                    status: McpOAuthLoginPollStatus::Error,
                    message: "MCP OAuth 回调 state 不匹配，请重新连接。".to_string(),
                    interval_secs: session.interval_secs,
                });
            }
            let mut oauth_state = session.oauth_state;
            match oauth_state
                .handle_callback(&success.code, &success.state)
                .await
            {
                Ok(()) => Ok(McpOAuthLoginPollResult {
                    server_name: session.server_name,
                    status: McpOAuthLoginPollStatus::Complete,
                    message: "MCP OAuth 授权完成，token 已写入系统安全存储。".to_string(),
                    interval_secs: session.interval_secs,
                }),
                Err(err) => Ok(McpOAuthLoginPollResult {
                    server_name: session.server_name,
                    status: McpOAuthLoginPollStatus::Error,
                    message: format!("MCP OAuth token 交换失败：{err}"),
                    interval_secs: session.interval_secs,
                }),
            }
        }
        McpOAuthCallbackResult::ProviderError(error) => {
            let code = error.error.unwrap_or_else(|| "oauth_error".to_string());
            let status = if code == "access_denied" {
                McpOAuthLoginPollStatus::Denied
            } else {
                McpOAuthLoginPollStatus::Error
            };
            Ok(McpOAuthLoginPollResult {
                server_name: session.server_name,
                status,
                message: format!(
                    "MCP OAuth 授权返回 {code}: {}",
                    error
                        .error_description
                        .unwrap_or_else(|| "No additional details provided.".to_string())
                ),
                interval_secs: session.interval_secs,
            })
        }
        McpOAuthCallbackResult::ListenerError(message) => Ok(McpOAuthLoginPollResult {
            server_name: session.server_name,
            status: McpOAuthLoginPollStatus::Error,
            message,
            interval_secs: session.interval_secs,
        }),
    }
}

fn start_mcp_oauth_callback_listener(
    port: u16,
    expected_path: String,
    expected_state: String,
    timeout_secs: u64,
) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|err| {
        format!(
            "start local MCP OAuth callback listener on 127.0.0.1:{port}: {err}. If another process uses this port, set OMIGA_MCP_OAUTH_CALLBACK_PORT and reconnect."
        )
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|err| format!("configure local MCP OAuth callback listener: {err}"))?;

    thread::spawn(move || {
        let deadline = Instant::now() + StdDuration::from_secs(timeout_secs.saturating_add(15));
        loop {
            if Instant::now() >= deadline {
                store_callback(
                    &expected_state,
                    McpOAuthCallbackResult::ListenerError(
                        "Timed out waiting for the local MCP OAuth callback.".to_string(),
                    ),
                );
                break;
            }
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    if handle_callback_stream(&mut stream, &expected_path, &expected_state) {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(StdDuration::from_millis(60));
                }
                Err(err) => {
                    store_callback(
                        &expected_state,
                        McpOAuthCallbackResult::ListenerError(format!(
                            "Local MCP OAuth callback listener failed: {err}"
                        )),
                    );
                    break;
                }
            }
        }
    });
    Ok(())
}

fn handle_callback_stream(
    stream: &mut TcpStream,
    expected_path: &str,
    expected_state: &str,
) -> bool {
    let target = match read_http_request_target(stream) {
        Ok(target) => target,
        Err(message) => {
            let _ = respond_oauth_callback(stream, 400, "Omiga MCP OAuth callback", &message);
            return false;
        }
    };

    match parse_mcp_oauth_callback(&target, expected_path) {
        McpOAuthCallbackOutcome::Success(success) => {
            let _ = respond_oauth_callback(
                stream,
                200,
                "Omiga MCP OAuth complete",
                "Authorization complete. You may close this window and return to Omiga.",
            );
            store_callback(expected_state, McpOAuthCallbackResult::Success(success));
            true
        }
        McpOAuthCallbackOutcome::ProviderError(error) => {
            let _ = respond_oauth_callback(
                stream,
                400,
                "Omiga MCP OAuth denied",
                "The OAuth provider returned an error. Return to Omiga for details.",
            );
            store_callback(expected_state, McpOAuthCallbackResult::ProviderError(error));
            true
        }
        McpOAuthCallbackOutcome::Invalid => {
            if target.starts_with(expected_path) {
                let _ = respond_oauth_callback(
                    stream,
                    400,
                    "Omiga MCP OAuth callback invalid",
                    "The OAuth callback did not include a code or provider error.",
                );
                store_callback(
                    expected_state,
                    McpOAuthCallbackResult::ListenerError(
                        "MCP OAuth callback did not include a code or provider error.".to_string(),
                    ),
                );
                true
            } else {
                let _ = respond_oauth_callback(
                    stream,
                    404,
                    "Omiga MCP OAuth callback",
                    "This local callback server only accepts the active MCP authorization route.",
                );
                false
            }
        }
    }
}

fn read_http_request_target(stream: &mut TcpStream) -> Result<String, String> {
    let _ = stream.set_read_timeout(Some(StdDuration::from_secs(2)));
    let mut buffer = [0_u8; 8192];
    let read = stream
        .read(&mut buffer)
        .map_err(|err| format!("read MCP OAuth callback request: {err}"))?;
    if read == 0 {
        return Err("MCP OAuth callback request was empty".to_string());
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| "MCP OAuth callback request was missing a request line".to_string())?;
    let mut parts = first_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "MCP OAuth callback request was missing method".to_string())?;
    if method != "GET" {
        return Err(format!("MCP OAuth callback must use GET, got {method}"));
    }
    parts
        .next()
        .map(str::to_string)
        .ok_or_else(|| "MCP OAuth callback request was missing target".to_string())
}

fn respond_oauth_callback(
    stream: &mut TcpStream,
    status_code: u16,
    title: &str,
    message: &str,
) -> std::io::Result<()> {
    let status_text = match status_code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body style=\"font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;padding:32px;line-height:1.5\"><h1>{}</h1><p>{}</p></body></html>",
        html_escape(title),
        html_escape(title),
        html_escape(message)
    );
    write!(
        stream,
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn store_callback(expected_state: &str, callback: McpOAuthCallbackResult) {
    if let Ok(mut guard) = callbacks().lock() {
        guard.insert(expected_state.to_string(), callback);
    }
}

fn parse_mcp_oauth_callback(target: &str, expected_path: &str) -> McpOAuthCallbackOutcome {
    let Some((route, query)) = target.split_once('?') else {
        return McpOAuthCallbackOutcome::Invalid;
    };
    if route != expected_path {
        return McpOAuthCallbackOutcome::Invalid;
    }

    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;

    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let Ok(key) = decode_query_component(key) else {
            continue;
        };
        let Ok(value) = decode_query_component(value) else {
            continue;
        };
        match key.as_str() {
            "code" => code = Some(value),
            "state" => state = Some(value),
            "error" => error = Some(value),
            "error_description" => error_description = Some(value),
            _ => {}
        }
    }

    if let (Some(code), Some(state)) = (code, state) {
        return McpOAuthCallbackOutcome::Success(McpOAuthCallbackSuccess { code, state });
    }

    if error.is_some() || error_description.is_some() {
        return McpOAuthCallbackOutcome::ProviderError(McpOAuthProviderError {
            error,
            error_description,
        });
    }

    McpOAuthCallbackOutcome::Invalid
}

fn extract_query_param(url: &str, name: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find_map(|(key, value)| (key == name).then(|| value.into_owned()))
}

fn decode_query_component(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("incomplete percent encoding".to_string());
                }
                let high = hex_value(bytes[index + 1])
                    .ok_or_else(|| "invalid percent encoding".to_string())?;
                let low = hex_value(bytes[index + 2])
                    .ok_or_else(|| "invalid percent encoding".to_string())?;
                out.push((high << 4) | low);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|err| format!("invalid UTF-8 in query component: {err}"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::transport::auth::StoredCredentials;
    use tempfile::tempdir;

    #[test]
    fn endpoint_key_is_stable_and_safe() {
        let key = endpoint_key("https://paperclip.gxl.ai/mcp");
        assert!(key.starts_with("mcp_"));
        assert_eq!(key, endpoint_key("https://paperclip.gxl.ai/mcp"));
        assert!(key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    }

    #[test]
    fn parse_callback_accepts_code_and_state() {
        let parsed = parse_mcp_oauth_callback(
            "/mcp/oauth/callback?code=abc%20123&state=state+value",
            "/mcp/oauth/callback",
        );
        assert_eq!(
            parsed,
            McpOAuthCallbackOutcome::Success(McpOAuthCallbackSuccess {
                code: "abc 123".to_string(),
                state: "state value".to_string(),
            })
        );
    }

    #[test]
    fn parse_callback_returns_provider_error() {
        let parsed = parse_mcp_oauth_callback(
            "/mcp/oauth/callback?error=access_denied&error_description=user+denied",
            "/mcp/oauth/callback",
        );
        assert_eq!(
            parsed,
            McpOAuthCallbackOutcome::ProviderError(McpOAuthProviderError {
                error: Some("access_denied".to_string()),
                error_description: Some("user denied".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn credential_store_round_trips_credentials() {
        let _guard = crate::domain::connectors::CONNECTOR_TEST_ENV_LOCK
            .lock()
            .await;
        let dir = tempdir().expect("tempdir");
        std::env::set_var("OMIGA_CONNECTOR_SECRET_STORE_DIR", dir.path());

        let store = McpCredentialStore::for_url("https://example.com/mcp");
        let credentials: StoredCredentials = serde_json::from_value(serde_json::json!({
            "client_id": "client-1",
            "token_response": null,
            "granted_scopes": ["read"],
            "token_received_at": 123,
        }))
        .expect("credentials json");
        store
            .save(credentials.clone())
            .await
            .expect("save credentials");
        assert!(has_stored_credentials_for_url("https://example.com/mcp"));
        let loaded = store
            .load()
            .await
            .expect("load credentials")
            .expect("stored credentials");
        assert_eq!(loaded.client_id, "client-1");
        assert_eq!(loaded.granted_scopes, vec!["read".to_string()]);
        assert_eq!(loaded.token_received_at, Some(123));
        clear_stored_credentials_for_url("https://example.com/mcp").expect("clear credentials");
        assert!(store.load().await.expect("load after clear").is_none());

        std::env::remove_var("OMIGA_CONNECTOR_SECRET_STORE_DIR");
    }
}
