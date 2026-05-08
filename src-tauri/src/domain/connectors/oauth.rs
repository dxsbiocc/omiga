//! First-class connector OAuth/device-flow support.
//!
//! Connectors should feel like Codex apps: the user clicks a connector, completes a browser or
//! local-software authorization flow, and Omiga stores only non-secret user-level state in
//! `~/.omiga/connectors/config.json`. Provider tokens live in secure storage or in the provider's
//! own local CLI/app credential store.

use super::http::{self, ConnectorHttpRequest};
use super::secret_store;
use super::{
    command_output_trimmed_with_timeout, connect_connector, github_api_base_url, github_cli_binary,
    github_cli_token, gmail_api_base_url, ConnectorConnectRequest, ConnectorInfo,
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use uuid::Uuid;

const GITHUB_OAUTH_TOKEN_SECRET: &str = "oauth_access_token";
const GITHUB_DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const GITHUB_CLI_LOGIN_EXPIRES_IN: u64 = 10 * 60;
const GITHUB_CLI_LOGIN_INTERVAL_SECS: u64 = 3;

const GMAIL_OAUTH_TOKEN_SECRET: &str = "oauth_access_token";
const GMAIL_OAUTH_REFRESH_TOKEN_SECRET: &str = "oauth_refresh_token";
const GMAIL_OAUTH_EXPIRES_AT_SECRET: &str = "oauth_access_token_expires_at";
const GMAIL_OAUTH_PROVIDER: &str = "gmail_oauth";
const GMAIL_BROWSER_LOGIN_EXPIRES_IN: u64 = 10 * 60;
const GMAIL_BROWSER_LOGIN_INTERVAL_SECS: u64 = 2;
#[allow(dead_code)]
const GMAIL_OAUTH_EXPIRY_SKEW_SECS: i64 = 60;
const GMAIL_DEFAULT_CALLBACK_PORT: u16 = 17656;
const GMAIL_DEFAULT_CALLBACK_PATH: &str = "/connectors/gmail/callback";
const GMAIL_DEFAULT_SCOPES: &str =
    "openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send";

const NOTION_OAUTH_TOKEN_SECRET: &str = "oauth_access_token";
const NOTION_OAUTH_PROVIDER: &str = "notion_oauth";
const NOTION_BROWSER_LOGIN_EXPIRES_IN: u64 = 10 * 60;
const NOTION_BROWSER_LOGIN_INTERVAL_SECS: u64 = 2;
const NOTION_DEFAULT_CALLBACK_PORT: u16 = 17654;
const NOTION_DEFAULT_CALLBACK_PATH: &str = "/connectors/notion/callback";
const NOTION_OAUTH_DEFAULT_VERSION: &str = "2026-03-11";

const SLACK_OAUTH_TOKEN_SECRET: &str = "oauth_access_token";
const SLACK_OAUTH_PROVIDER: &str = "slack_oauth";
const SLACK_BROWSER_LOGIN_EXPIRES_IN: u64 = 10 * 60;
const SLACK_BROWSER_LOGIN_INTERVAL_SECS: u64 = 2;
const SLACK_DEFAULT_LOCAL_CALLBACK_PORT: u16 = 17655;
const SLACK_DEFAULT_LOCAL_CALLBACK_PATH: &str = "/connectors/slack/callback";
const SLACK_DEFAULT_BOT_SCOPES: &str = "channels:read,channels:history,chat:write";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorLoginStartResult {
    pub connector_id: String,
    pub connector_name: String,
    pub provider: String,
    pub login_session_id: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub user_code: String,
    pub expires_in: u64,
    pub interval_secs: u64,
    pub expires_at: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorLoginPollStatus {
    Pending,
    SlowDown,
    Complete,
    Expired,
    Denied,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorLoginPollResult {
    pub connector_id: String,
    pub provider: String,
    pub status: ConnectorLoginPollStatus,
    pub message: String,
    pub interval_secs: u64,
    pub connector: Option<ConnectorInfo>,
}

#[derive(Debug, Clone)]
struct LoginSession {
    connector_id: String,
    provider: String,
    client_id: String,
    client_secret: Option<String>,
    code_verifier: Option<String>,
    device_code: String,
    redirect_uri: Option<String>,
    state: Option<String>,
    interval_secs: u64,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserOAuthCallbackSuccess {
    code: String,
    state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserOAuthProviderError {
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PkceCodes {
    code_verifier: String,
    code_challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserOAuthCallbackResult {
    Success(BrowserOAuthCallbackSuccess),
    ProviderError(BrowserOAuthProviderError),
    ListenerError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserOAuthCallbackOutcome {
    Success(BrowserOAuthCallbackSuccess),
    ProviderError(BrowserOAuthProviderError),
    Invalid,
}

#[derive(Debug, Deserialize)]
struct GitHubDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GoogleOAuthTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn gmail_refresh_token_form(
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> Vec<(&'static str, String)> {
    let mut form = vec![
        ("client_id", client_id.to_string()),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if let Some(client_secret) = client_secret.filter(|value| !value.trim().is_empty()) {
        form.push(("client_secret", client_secret.to_string()));
    }
    form
}

#[derive(Debug, Deserialize)]
struct NotionTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    bot_id: Option<String>,
    #[serde(default)]
    workspace_name: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackOAuthAccessResponse {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    bot_user_id: Option<String>,
    #[serde(default)]
    team: Option<SlackOAuthNamedEntity>,
    #[serde(default)]
    enterprise: Option<SlackOAuthNamedEntity>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SlackOAuthNamedEntity {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn sessions() -> &'static Mutex<HashMap<String, LoginSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, LoginSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn browser_callbacks() -> &'static Mutex<HashMap<String, BrowserOAuthCallbackResult>> {
    static CALLBACKS: OnceLock<Mutex<HashMap<String, BrowserOAuthCallbackResult>>> =
        OnceLock::new();
    CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn first_non_empty_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn first_non_empty_bundled(values: &[Option<&'static str>]) -> Option<String> {
    values.iter().find_map(|value| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn generate_pkce_codes() -> PkceCodes {
    let mut bytes = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut bytes);

    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    PkceCodes {
        code_verifier,
        code_challenge,
    }
}

fn github_device_code_url() -> String {
    first_non_empty_env(&["OMIGA_GITHUB_DEVICE_CODE_URL"])
        .unwrap_or_else(|| "https://github.com/login/device/code".to_string())
}

fn github_access_token_url() -> String {
    first_non_empty_env(&["OMIGA_GITHUB_ACCESS_TOKEN_URL"])
        .unwrap_or_else(|| "https://github.com/login/oauth/access_token".to_string())
}

fn github_oauth_client_id() -> Result<String, String> {
    first_non_empty_env(&["OMIGA_GITHUB_OAUTH_CLIENT_ID"]).ok_or_else(|| {
        "GitHub OAuth login requires OMIGA_GITHUB_OAUTH_CLIENT_ID. Create a GitHub OAuth App or GitHub App with Device Flow enabled, then restart Omiga with that client ID.".to_string()
    })
}

fn github_oauth_scope() -> Option<String> {
    first_non_empty_env(&["OMIGA_GITHUB_OAUTH_SCOPE"]).or_else(|| Some("read:user".to_string()))
}

fn google_authorize_url() -> String {
    first_non_empty_env(&["OMIGA_GOOGLE_AUTHORIZE_URL", "OMIGA_GMAIL_AUTHORIZE_URL"])
        .unwrap_or_else(|| "https://accounts.google.com/o/oauth2/v2/auth".to_string())
}

fn google_token_url() -> String {
    first_non_empty_env(&["OMIGA_GOOGLE_TOKEN_URL", "OMIGA_GMAIL_TOKEN_URL"])
        .unwrap_or_else(|| "https://oauth2.googleapis.com/token".to_string())
}

fn gmail_oauth_scope() -> String {
    first_non_empty_env(&["OMIGA_GMAIL_OAUTH_SCOPE", "OMIGA_GOOGLE_OAUTH_SCOPE"])
        .unwrap_or_else(|| GMAIL_DEFAULT_SCOPES.to_string())
}

fn gmail_oauth_client_id() -> Result<String, String> {
    first_non_empty_env(&[
        "OMIGA_GMAIL_OAUTH_CLIENT_ID",
        "OMIGA_GOOGLE_OAUTH_CLIENT_ID",
    ])
    .or_else(|| {
        first_non_empty_bundled(&[
            option_env!("OMIGA_GMAIL_OAUTH_CLIENT_ID"),
            option_env!("OMIGA_GOOGLE_OAUTH_CLIENT_ID"),
        ])
    })
    .ok_or_else(|| {
        format!(
            "Gmail browser login requires Omiga's own Google OAuth client ID: OMIGA_GMAIL_OAUTH_CLIENT_ID or OMIGA_GOOGLE_OAUTH_CLIENT_ID. Omiga implements the local OAuth flow with PKCE; the app build must ship a Google Desktop OAuth client ID registered for the redirect URI {}.",
            default_gmail_redirect_uri()
        )
    })
}

fn gmail_oauth_client_secret() -> Option<String> {
    first_non_empty_env(&[
        "OMIGA_GMAIL_OAUTH_CLIENT_SECRET",
        "OMIGA_GOOGLE_OAUTH_CLIENT_SECRET",
    ])
    .or_else(|| {
        first_non_empty_bundled(&[
            option_env!("OMIGA_GMAIL_OAUTH_CLIENT_SECRET"),
            option_env!("OMIGA_GOOGLE_OAUTH_CLIENT_SECRET"),
        ])
    })
}

fn notion_authorize_url() -> String {
    first_non_empty_env(&["OMIGA_NOTION_AUTHORIZE_URL"])
        .unwrap_or_else(|| "https://api.notion.com/v1/oauth/authorize".to_string())
}

fn notion_token_url() -> String {
    first_non_empty_env(&["OMIGA_NOTION_TOKEN_URL"])
        .unwrap_or_else(|| "https://api.notion.com/v1/oauth/token".to_string())
}

fn notion_oauth_version() -> String {
    first_non_empty_env(&["OMIGA_NOTION_OAUTH_VERSION", "OMIGA_NOTION_VERSION"])
        .unwrap_or_else(|| NOTION_OAUTH_DEFAULT_VERSION.to_string())
}

fn notion_oauth_client_id() -> Result<String, String> {
    first_non_empty_env(&["OMIGA_NOTION_OAUTH_CLIENT_ID"]).ok_or_else(|| {
        format!(
            "Notion browser login requires OMIGA_NOTION_OAUTH_CLIENT_ID and OMIGA_NOTION_OAUTH_CLIENT_SECRET for the Omiga OAuth app. Configure the Notion integration redirect URI as {}. Users should connect by browser authorization, not by pasting workspace tokens.",
            default_notion_redirect_uri()
        )
    })
}

fn notion_oauth_client_secret() -> Result<String, String> {
    first_non_empty_env(&["OMIGA_NOTION_OAUTH_CLIENT_SECRET"]).ok_or_else(|| {
        format!(
            "Notion browser login requires OMIGA_NOTION_OAUTH_CLIENT_SECRET for the Omiga OAuth app. Configure the Notion integration redirect URI as {}. This is product-level OAuth configuration, not a per-user token paste flow.",
            default_notion_redirect_uri()
        )
    })
}

fn slack_authorize_url() -> String {
    first_non_empty_env(&["OMIGA_SLACK_AUTHORIZE_URL"])
        .unwrap_or_else(|| "https://slack.com/oauth/v2/authorize".to_string())
}

fn slack_token_url() -> String {
    first_non_empty_env(&["OMIGA_SLACK_TOKEN_URL"])
        .unwrap_or_else(|| "https://slack.com/api/oauth.v2.access".to_string())
}

fn slack_oauth_client_id() -> Result<String, String> {
    first_non_empty_env(&["OMIGA_SLACK_OAUTH_CLIENT_ID"]).ok_or_else(|| {
        format!(
            "Slack browser login requires OMIGA_SLACK_OAUTH_CLIENT_ID, OMIGA_SLACK_OAUTH_CLIENT_SECRET, and an HTTPS OMIGA_SLACK_OAUTH_REDIRECT_URI registered in Slack. Because Slack redirects only to HTTPS app URLs, configure that URL as an Omiga callback bridge that forwards code/state to {}.",
            default_slack_local_redirect_uri()
        )
    })
}

fn slack_oauth_client_secret() -> Result<String, String> {
    first_non_empty_env(&["OMIGA_SLACK_OAUTH_CLIENT_SECRET"]).ok_or_else(|| {
        format!(
            "Slack browser login requires OMIGA_SLACK_OAUTH_CLIENT_SECRET. Users should install through Slack OAuth in the browser; SLACK_BOT_TOKEN remains an advanced fallback only. The HTTPS callback bridge should forward code/state to {}.",
            default_slack_local_redirect_uri()
        )
    })
}

fn slack_oauth_scope() -> String {
    first_non_empty_env(&["OMIGA_SLACK_OAUTH_SCOPE"])
        .unwrap_or_else(|| SLACK_DEFAULT_BOT_SCOPES.to_string())
}

fn slack_oauth_user_scope() -> Option<String> {
    first_non_empty_env(&["OMIGA_SLACK_OAUTH_USER_SCOPE"])
}

fn encode_form_value(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn form_body(params: &[(&str, String)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{key}={}", encode_form_value(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn default_gmail_redirect_uri() -> String {
    format!("http://127.0.0.1:{GMAIL_DEFAULT_CALLBACK_PORT}{GMAIL_DEFAULT_CALLBACK_PATH}")
}

fn default_notion_redirect_uri() -> String {
    format!("http://127.0.0.1:{NOTION_DEFAULT_CALLBACK_PORT}{NOTION_DEFAULT_CALLBACK_PATH}")
}

fn default_slack_local_redirect_uri() -> String {
    format!(
        "http://127.0.0.1:{SLACK_DEFAULT_LOCAL_CALLBACK_PORT}{SLACK_DEFAULT_LOCAL_CALLBACK_PATH}"
    )
}

fn notion_callback_port() -> Result<u16, String> {
    let Some(port) = first_non_empty_env(&[
        "OMIGA_NOTION_OAUTH_CALLBACK_PORT",
        "OMIGA_CONNECTOR_OAUTH_CALLBACK_PORT",
    ]) else {
        return Ok(NOTION_DEFAULT_CALLBACK_PORT);
    };
    let parsed = port
        .parse::<u16>()
        .map_err(|err| format!("invalid Notion OAuth callback port `{port}`: {err}"))?;
    if parsed == 0 {
        return Err(
            "invalid Notion OAuth callback port `0`: choose a fixed local port between 1 and 65535"
                .to_string(),
        );
    }
    Ok(parsed)
}

fn parse_local_redirect_uri(value: &str) -> Result<(String, u16, String), String> {
    let value = value.trim();
    let rest = value
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| value.strip_prefix("http://localhost:"))
        .ok_or_else(|| {
            format!(
                "OAuth local callback URI must use http://127.0.0.1:<port>/<path> or http://localhost:<port>/<path>. Got `{value}`."
            )
        })?;
    let (port_part, path_part) = rest.split_once('/').ok_or_else(|| {
        format!("OAuth local callback URI must include a callback path. Got `{value}`.")
    })?;
    let port = port_part
        .parse::<u16>()
        .map_err(|err| format!("invalid Notion OAuth redirect URI port `{port_part}`: {err}"))?;
    if port == 0 {
        return Err("OAuth local callback URI port cannot be 0".to_string());
    }
    let path = format!("/{path_part}");
    if path == "/" {
        return Err("OAuth local callback URI must include a callback path".to_string());
    }
    Ok((value.to_string(), port, path))
}

fn gmail_callback_port() -> Result<u16, String> {
    let Some(port) = first_non_empty_env(&[
        "OMIGA_GMAIL_OAUTH_CALLBACK_PORT",
        "OMIGA_GOOGLE_OAUTH_CALLBACK_PORT",
        "OMIGA_CONNECTOR_OAUTH_CALLBACK_PORT",
    ]) else {
        return Ok(GMAIL_DEFAULT_CALLBACK_PORT);
    };
    let parsed = port
        .parse::<u16>()
        .map_err(|err| format!("invalid Gmail OAuth callback port `{port}`: {err}"))?;
    if parsed == 0 {
        return Err(
            "invalid Gmail OAuth callback port `0`: choose a fixed local port between 1 and 65535"
                .to_string(),
        );
    }
    Ok(parsed)
}

fn gmail_redirect_config() -> Result<(String, u16, String), String> {
    if let Some(value) = first_non_empty_env(&[
        "OMIGA_GMAIL_OAUTH_REDIRECT_URI",
        "OMIGA_GOOGLE_OAUTH_REDIRECT_URI",
    ]) {
        return parse_local_redirect_uri(&value);
    }
    let port = gmail_callback_port()?;
    let redirect_uri = format!("http://127.0.0.1:{port}{GMAIL_DEFAULT_CALLBACK_PATH}");
    Ok((redirect_uri, port, GMAIL_DEFAULT_CALLBACK_PATH.to_string()))
}

fn notion_redirect_config() -> Result<(String, u16, String), String> {
    if let Some(value) = first_non_empty_env(&["OMIGA_NOTION_OAUTH_REDIRECT_URI"]) {
        return parse_local_redirect_uri(&value);
    }
    let port = notion_callback_port()?;
    let redirect_uri = format!("http://127.0.0.1:{port}{NOTION_DEFAULT_CALLBACK_PATH}");
    Ok((redirect_uri, port, NOTION_DEFAULT_CALLBACK_PATH.to_string()))
}

fn slack_local_callback_port() -> Result<u16, String> {
    let Some(port) = first_non_empty_env(&[
        "OMIGA_SLACK_OAUTH_LOCAL_CALLBACK_PORT",
        "OMIGA_CONNECTOR_OAUTH_CALLBACK_PORT",
    ]) else {
        return Ok(SLACK_DEFAULT_LOCAL_CALLBACK_PORT);
    };
    let parsed = port
        .parse::<u16>()
        .map_err(|err| format!("invalid Slack OAuth local callback port `{port}`: {err}"))?;
    if parsed == 0 {
        return Err(
            "invalid Slack OAuth local callback port `0`: choose a fixed local port between 1 and 65535"
                .to_string(),
        );
    }
    Ok(parsed)
}

fn slack_local_callback_config() -> Result<(String, u16, String), String> {
    if let Some(value) = first_non_empty_env(&["OMIGA_SLACK_OAUTH_LOCAL_CALLBACK_URI"]) {
        return parse_local_redirect_uri(&value);
    }
    let port = slack_local_callback_port()?;
    let redirect_uri = format!("http://127.0.0.1:{port}{SLACK_DEFAULT_LOCAL_CALLBACK_PATH}");
    Ok((
        redirect_uri,
        port,
        SLACK_DEFAULT_LOCAL_CALLBACK_PATH.to_string(),
    ))
}

fn slack_registered_redirect_uri() -> Result<String, String> {
    let redirect_uri = first_non_empty_env(&["OMIGA_SLACK_OAUTH_REDIRECT_URI"]).ok_or_else(|| {
        format!(
            "Slack OAuth requires an HTTPS redirect URL registered in Slack. Set OMIGA_SLACK_OAUTH_REDIRECT_URI to your Omiga callback bridge URL; that bridge must forward code/state to {}. Slack does not support registering Omiga's localhost callback directly.",
            default_slack_local_redirect_uri()
        )
    })?;
    if !redirect_uri.starts_with("https://") {
        return Err(format!(
            "Slack OAuth redirect URI must be HTTPS and registered in Slack. Got `{redirect_uri}`. Use an HTTPS callback bridge that forwards code/state to {}.",
            default_slack_local_redirect_uri()
        ));
    }
    Ok(redirect_uri)
}

fn gmail_authorization_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> String {
    let base = google_authorize_url();
    let separator = if base.contains('?') { "&" } else { "?" };
    format!(
        "{base}{separator}{}",
        form_body(&[
            ("client_id", client_id.to_string()),
            ("response_type", "code".to_string()),
            ("redirect_uri", redirect_uri.to_string()),
            ("scope", gmail_oauth_scope()),
            ("access_type", "offline".to_string()),
            ("prompt", "consent".to_string()),
            ("code_challenge", code_challenge.to_string()),
            ("code_challenge_method", "S256".to_string()),
            ("state", state.to_string()),
        ])
    )
}

fn notion_authorization_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let base = notion_authorize_url();
    let separator = if base.contains('?') { "&" } else { "?" };
    format!(
        "{base}{separator}{}",
        form_body(&[
            ("client_id", client_id.to_string()),
            ("response_type", "code".to_string()),
            ("owner", "user".to_string()),
            ("redirect_uri", redirect_uri.to_string()),
            ("state", state.to_string()),
        ])
    )
}

fn slack_authorization_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let base = slack_authorize_url();
    let separator = if base.contains('?') { "&" } else { "?" };
    let mut params = vec![
        ("client_id", client_id.to_string()),
        ("scope", slack_oauth_scope()),
        ("redirect_uri", redirect_uri.to_string()),
        ("state", state.to_string()),
    ];
    if let Some(user_scope) = slack_oauth_user_scope() {
        params.push(("user_scope", user_scope));
    }
    format!("{base}{separator}{}", form_body(&params))
}

pub(crate) fn github_oauth_token() -> Option<String> {
    secret_store::read_connector_secret("github", GITHUB_OAUTH_TOKEN_SECRET)
        .ok()
        .flatten()
}

pub(crate) fn delete_github_oauth_token() -> Result<(), String> {
    secret_store::delete_connector_secret("github", GITHUB_OAUTH_TOKEN_SECRET)
}

pub(crate) fn gmail_oauth_token() -> Option<String> {
    secret_store::read_connector_secret("gmail", GMAIL_OAUTH_TOKEN_SECRET)
        .ok()
        .flatten()
}

fn gmail_oauth_refresh_token() -> Option<String> {
    secret_store::read_connector_secret("gmail", GMAIL_OAUTH_REFRESH_TOKEN_SECRET)
        .ok()
        .flatten()
}

fn gmail_oauth_access_token_expires_at() -> Option<DateTime<Utc>> {
    secret_store::read_connector_secret("gmail", GMAIL_OAUTH_EXPIRES_AT_SECRET)
        .ok()
        .flatten()
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn gmail_oauth_access_token_is_fresh() -> bool {
    gmail_oauth_access_token_expires_at()
        .map(|expires_at| Utc::now() + Duration::seconds(GMAIL_OAUTH_EXPIRY_SKEW_SECS) < expires_at)
        .unwrap_or(true)
}

pub(crate) async fn gmail_oauth_token_or_refresh() -> Option<String> {
    let access_token = gmail_oauth_token();
    if access_token.is_some() && gmail_oauth_access_token_is_fresh() {
        return access_token;
    }

    let refresh_token = gmail_oauth_refresh_token()?;
    refresh_gmail_access_token(&refresh_token)
        .await
        .ok()
        .or(access_token)
}

pub(crate) fn delete_gmail_oauth_token() -> Result<(), String> {
    secret_store::delete_connector_secret("gmail", GMAIL_OAUTH_TOKEN_SECRET)?;
    secret_store::delete_connector_secret("gmail", GMAIL_OAUTH_REFRESH_TOKEN_SECRET)?;
    secret_store::delete_connector_secret("gmail", GMAIL_OAUTH_EXPIRES_AT_SECRET)
}

pub(crate) fn notion_oauth_token() -> Option<String> {
    secret_store::read_connector_secret("notion", NOTION_OAUTH_TOKEN_SECRET)
        .ok()
        .flatten()
}

pub(crate) fn delete_notion_oauth_token() -> Result<(), String> {
    secret_store::delete_connector_secret("notion", NOTION_OAUTH_TOKEN_SECRET)
}

pub(crate) fn slack_oauth_token() -> Option<String> {
    secret_store::read_connector_secret("slack", SLACK_OAUTH_TOKEN_SECRET)
        .ok()
        .flatten()
}

pub(crate) fn delete_slack_oauth_token() -> Result<(), String> {
    secret_store::delete_connector_secret("slack", SLACK_OAUTH_TOKEN_SECRET)
}

pub async fn start_connector_login(
    connector_id: &str,
) -> Result<ConnectorLoginStartResult, String> {
    match connector_id.trim().to_ascii_lowercase().as_str() {
        "github" => start_github_login().await,
        "gmail" => start_gmail_login().await,
        "notion" => start_notion_login().await,
        "slack" => start_slack_login().await,
        other => Err(format!(
            "Connector `{other}` does not have a first-class browser/software login flow yet. Add an OAuth/native/MCP provider before it can be connected from the product UI."
        )),
    }
}

async fn start_gmail_login() -> Result<ConnectorLoginStartResult, String> {
    let client_id = gmail_oauth_client_id()?;
    let client_secret = gmail_oauth_client_secret();
    let (redirect_uri, callback_port, callback_path) = gmail_redirect_config()?;
    let pkce = generate_pkce_codes();
    let state = Uuid::new_v4().to_string();
    let expires_at =
        Utc::now() + Duration::seconds(GMAIL_BROWSER_LOGIN_EXPIRES_IN.min(i64::MAX as u64) as i64);

    start_browser_callback_listener(
        "Gmail",
        callback_port,
        callback_path.clone(),
        state.clone(),
        GMAIL_BROWSER_LOGIN_EXPIRES_IN,
    )?;

    let login_session_id = Uuid::new_v4().to_string();
    let session = LoginSession {
        connector_id: "gmail".to_string(),
        provider: GMAIL_OAUTH_PROVIDER.to_string(),
        client_id: client_id.clone(),
        client_secret,
        code_verifier: Some(pkce.code_verifier),
        device_code: String::new(),
        redirect_uri: Some(redirect_uri.clone()),
        state: Some(state.clone()),
        interval_secs: GMAIL_BROWSER_LOGIN_INTERVAL_SECS,
        expires_at,
    };
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .insert(login_session_id.clone(), session);

    Ok(ConnectorLoginStartResult {
        connector_id: "gmail".to_string(),
        connector_name: "Gmail".to_string(),
        provider: GMAIL_OAUTH_PROVIDER.to_string(),
        login_session_id,
        verification_uri: gmail_authorization_url(
            &client_id,
            &redirect_uri,
            &state,
            &pkce.code_challenge,
        ),
        verification_uri_complete: None,
        user_code: String::new(),
        expires_in: GMAIL_BROWSER_LOGIN_EXPIRES_IN,
        interval_secs: GMAIL_BROWSER_LOGIN_INTERVAL_SECS,
        expires_at: expires_at.to_rfc3339(),
        message: format!(
            "A browser authorization page will open for Gmail. After approval, Google redirects to {redirect_uri}; Omiga stores the OAuth token in secure storage, not connectors/config.json."
        ),
    })
}

async fn start_github_login() -> Result<ConnectorLoginStartResult, String> {
    let client_id = match github_oauth_client_id() {
        Ok(client_id) => client_id,
        Err(oauth_error) => return start_github_cli_login(oauth_error),
    };
    start_github_device_login(client_id).await
}

async fn start_github_device_login(client_id: String) -> Result<ConnectorLoginStartResult, String> {
    let mut form = vec![("client_id", client_id.clone())];
    let scope = github_oauth_scope();
    if let Some(scope) = scope.as_deref().filter(|value| !value.trim().is_empty()) {
        form.push(("scope", scope.to_string()));
    }

    let client = reqwest::Client::new();
    let response = client
        .post(github_device_code_url())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body(&form))
        .send()
        .await
        .map_err(|err| format!("request GitHub device code: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("read GitHub device code response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "GitHub device code request returned {status}: {}",
            http::redact_and_truncate(&body, 600)
        ));
    }
    let parsed = serde_json::from_str::<GitHubDeviceCodeResponse>(&body)
        .map_err(|err| format!("parse GitHub device code response: {err}"))?;
    if let Some(error) = parsed.error {
        return Err(format!(
            "GitHub device code request failed: {error}{}",
            parsed
                .error_description
                .map(|description| format!(" - {description}"))
                .unwrap_or_default()
        ));
    }

    let interval_secs = parsed.interval.unwrap_or(5).max(1);
    let expires_at = Utc::now() + Duration::seconds(parsed.expires_in.min(i64::MAX as u64) as i64);
    let login_session_id = Uuid::new_v4().to_string();
    let session = LoginSession {
        connector_id: "github".to_string(),
        provider: "github".to_string(),
        client_id,
        client_secret: None,
        code_verifier: None,
        device_code: parsed.device_code,
        redirect_uri: None,
        state: None,
        interval_secs,
        expires_at,
    };
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .insert(login_session_id.clone(), session);

    Ok(ConnectorLoginStartResult {
        connector_id: "github".to_string(),
        connector_name: "GitHub".to_string(),
        provider: "github".to_string(),
        login_session_id,
        verification_uri: parsed.verification_uri,
        verification_uri_complete: parsed.verification_uri_complete,
        user_code: parsed.user_code,
        expires_in: parsed.expires_in,
        interval_secs,
        expires_at: expires_at.to_rfc3339(),
        message: "Open GitHub, enter the code, and approve Omiga. Omiga will poll without exceeding GitHub's device-flow interval.".to_string(),
    })
}

async fn start_notion_login() -> Result<ConnectorLoginStartResult, String> {
    let client_id = notion_oauth_client_id()?;
    let client_secret = notion_oauth_client_secret()?;
    let (redirect_uri, callback_port, callback_path) = notion_redirect_config()?;
    let state = Uuid::new_v4().to_string();
    let expires_at =
        Utc::now() + Duration::seconds(NOTION_BROWSER_LOGIN_EXPIRES_IN.min(i64::MAX as u64) as i64);

    start_browser_callback_listener(
        "Notion",
        callback_port,
        callback_path.clone(),
        state.clone(),
        NOTION_BROWSER_LOGIN_EXPIRES_IN,
    )?;

    let login_session_id = Uuid::new_v4().to_string();
    let session = LoginSession {
        connector_id: "notion".to_string(),
        provider: NOTION_OAUTH_PROVIDER.to_string(),
        client_id: client_id.clone(),
        client_secret: Some(client_secret),
        code_verifier: None,
        device_code: String::new(),
        redirect_uri: Some(redirect_uri.clone()),
        state: Some(state.clone()),
        interval_secs: NOTION_BROWSER_LOGIN_INTERVAL_SECS,
        expires_at,
    };
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .insert(login_session_id.clone(), session);

    Ok(ConnectorLoginStartResult {
        connector_id: "notion".to_string(),
        connector_name: "Notion".to_string(),
        provider: NOTION_OAUTH_PROVIDER.to_string(),
        login_session_id,
        verification_uri: notion_authorization_url(&client_id, &redirect_uri, &state),
        verification_uri_complete: None,
        user_code: String::new(),
        expires_in: NOTION_BROWSER_LOGIN_EXPIRES_IN,
        interval_secs: NOTION_BROWSER_LOGIN_INTERVAL_SECS,
        expires_at: expires_at.to_rfc3339(),
        message: format!(
            "A browser authorization page will open for Notion. After you approve access, Notion redirects to {redirect_uri}; Omiga stores the access token in secure storage, not connectors/config.json."
        ),
    })
}

async fn start_slack_login() -> Result<ConnectorLoginStartResult, String> {
    let client_id = slack_oauth_client_id()?;
    let client_secret = slack_oauth_client_secret()?;
    let registered_redirect_uri = slack_registered_redirect_uri()?;
    let (local_callback_uri, callback_port, callback_path) = slack_local_callback_config()?;
    let state = Uuid::new_v4().to_string();
    let expires_at =
        Utc::now() + Duration::seconds(SLACK_BROWSER_LOGIN_EXPIRES_IN.min(i64::MAX as u64) as i64);

    start_browser_callback_listener(
        "Slack",
        callback_port,
        callback_path.clone(),
        state.clone(),
        SLACK_BROWSER_LOGIN_EXPIRES_IN,
    )?;

    let login_session_id = Uuid::new_v4().to_string();
    let session = LoginSession {
        connector_id: "slack".to_string(),
        provider: SLACK_OAUTH_PROVIDER.to_string(),
        client_id: client_id.clone(),
        client_secret: Some(client_secret),
        code_verifier: None,
        device_code: String::new(),
        redirect_uri: Some(registered_redirect_uri.clone()),
        state: Some(state.clone()),
        interval_secs: SLACK_BROWSER_LOGIN_INTERVAL_SECS,
        expires_at,
    };
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .insert(login_session_id.clone(), session);

    Ok(ConnectorLoginStartResult {
        connector_id: "slack".to_string(),
        connector_name: "Slack".to_string(),
        provider: SLACK_OAUTH_PROVIDER.to_string(),
        login_session_id,
        verification_uri: slack_authorization_url(&client_id, &registered_redirect_uri, &state),
        verification_uri_complete: None,
        user_code: String::new(),
        expires_in: SLACK_BROWSER_LOGIN_EXPIRES_IN,
        interval_secs: SLACK_BROWSER_LOGIN_INTERVAL_SECS,
        expires_at: expires_at.to_rfc3339(),
        message: format!(
            "A browser authorization page will open for Slack. Slack redirects to your HTTPS Omiga bridge ({registered_redirect_uri}), which must forward code/state to the local callback {local_callback_uri}; Omiga then stores the bot token in secure storage, not connectors/config.json."
        ),
    })
}

fn github_cli_login_args() -> [&'static str; 8] {
    [
        "auth",
        "login",
        "--web",
        "--hostname",
        "github.com",
        "--git-protocol",
        "https",
        "--scopes",
    ]
}

fn github_cli_login_command_line() -> String {
    let mut parts = Vec::from([shell_words::quote(&github_cli_binary()).to_string()]);
    parts.extend(
        github_cli_login_args()
            .into_iter()
            .map(|part| shell_words::quote(part).to_string()),
    );
    parts.push(shell_words::quote("repo,read:org").to_string());
    parts.join(" ")
}

fn github_cli_available() -> bool {
    command_output_trimmed_with_timeout(
        &github_cli_binary(),
        &["--version"],
        StdDuration::from_secs(2),
    )
    .is_some()
}

#[cfg(target_os = "macos")]
fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn launch_github_cli_login() -> Result<(), String> {
    let command_line = github_cli_login_command_line();
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"",
        escape_applescript_string(&command_line)
    );
    Command::new("osascript")
        .args([
            "-e",
            "tell application \"Terminal\" to activate",
            "-e",
            &script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("open GitHub CLI login in Terminal: {err}"))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn launch_github_cli_login() -> Result<(), String> {
    Err(format!(
        "Automatic GitHub CLI login is not implemented on this OS yet. Run `{}` in a terminal, then return to Omiga and test the connector.",
        github_cli_login_command_line()
    ))
}

fn start_github_cli_login(oauth_error: String) -> Result<ConnectorLoginStartResult, String> {
    if !github_cli_available() {
        return Err(format!(
            "{oauth_error} GitHub CLI is not available for local software login. Install GitHub CLI and run `gh auth login`, or configure OMIGA_GITHUB_OAUTH_CLIENT_ID for Omiga's own GitHub OAuth device flow."
        ));
    }

    let already_logged_in = github_cli_token().is_some();
    if !already_logged_in {
        launch_github_cli_login()?;
    }

    let expires_at =
        Utc::now() + Duration::seconds(GITHUB_CLI_LOGIN_EXPIRES_IN.min(i64::MAX as u64) as i64);
    let login_session_id = Uuid::new_v4().to_string();
    let session = LoginSession {
        connector_id: "github".to_string(),
        provider: "github_cli".to_string(),
        client_id: String::new(),
        client_secret: None,
        code_verifier: None,
        device_code: String::new(),
        redirect_uri: None,
        state: None,
        interval_secs: GITHUB_CLI_LOGIN_INTERVAL_SECS,
        expires_at,
    };
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .insert(login_session_id.clone(), session);

    Ok(ConnectorLoginStartResult {
        connector_id: "github".to_string(),
        connector_name: "GitHub".to_string(),
        provider: "github_cli".to_string(),
        login_session_id,
        verification_uri: "https://github.com/login".to_string(),
        verification_uri_complete: None,
        user_code: String::new(),
        expires_in: GITHUB_CLI_LOGIN_EXPIRES_IN,
        interval_secs: GITHUB_CLI_LOGIN_INTERVAL_SECS,
        expires_at: expires_at.to_rfc3339(),
        message: if already_logged_in {
            "GitHub CLI is already logged in. Omiga is verifying the local software login."
                .to_string()
        } else {
            "A Terminal window was opened for `gh auth login --web`. Complete GitHub's browser authorization; Omiga will detect the GitHub CLI login automatically."
                .to_string()
        },
    })
}

pub async fn poll_connector_login(
    login_session_id: &str,
) -> Result<ConnectorLoginPollResult, String> {
    let login_session_id = login_session_id.trim().to_string();
    if login_session_id.is_empty() {
        return Err("login session id is required".to_string());
    }
    let session = sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .get(&login_session_id)
        .cloned()
        .ok_or_else(|| "connector login session expired or is unknown".to_string())?;

    if Utc::now() >= session.expires_at {
        sessions()
            .lock()
            .map_err(|_| "connector login session lock poisoned".to_string())?
            .remove(&login_session_id);
        return Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id.clone(),
            provider: session.provider,
            status: ConnectorLoginPollStatus::Expired,
            message: connector_login_expired_message(&session.connector_id),
            interval_secs: session.interval_secs,
            connector: None,
        });
    }

    match session.provider.as_str() {
        "github" => poll_github_login(login_session_id, session).await,
        "github_cli" => poll_github_cli_login(login_session_id, session).await,
        GMAIL_OAUTH_PROVIDER => poll_gmail_login(login_session_id, session).await,
        NOTION_OAUTH_PROVIDER => poll_notion_login(login_session_id, session).await,
        SLACK_OAUTH_PROVIDER => poll_slack_login(login_session_id, session).await,
        other => Err(format!("unsupported connector login provider `{other}`")),
    }
}

fn connector_login_expired_message(connector_id: &str) -> String {
    match connector_id {
        "gmail" => "Gmail login expired. Start installation again to open a fresh authorization page.".to_string(),
        "notion" => "Notion browser login expired. Start installation again to open a fresh authorization page.".to_string(),
        "slack" => "Slack browser login expired. Start installation again to open a fresh authorization page.".to_string(),
        "github" => "GitHub login code expired. Start installation again to request a new code.".to_string(),
        _ => "Connector login expired. Start installation again.".to_string(),
    }
}

async fn poll_github_cli_login(
    login_session_id: String,
    session: LoginSession,
) -> Result<ConnectorLoginPollResult, String> {
    let Some(token) = github_cli_token() else {
        return Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Pending,
            message: "Waiting for GitHub CLI login to finish. Complete the browser flow opened by `gh auth login --web`.".to_string(),
            interval_secs: session.interval_secs,
            connector: None,
        });
    };

    let account_label = github_user_login(&token).await.ok();
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .remove(&login_session_id);
    let connector = connect_connector(ConnectorConnectRequest {
        connector_id: "github".to_string(),
        account_label,
        auth_source: Some("github_cli".to_string()),
    })?;
    Ok(ConnectorLoginPollResult {
        connector_id: "github".to_string(),
        provider: "github_cli".to_string(),
        status: ConnectorLoginPollStatus::Complete,
        message: "GitHub CLI login complete. Omiga will use the local `gh auth token` provider without storing the token in connectors/config.json.".to_string(),
        interval_secs: session.interval_secs,
        connector: Some(connector),
    })
}

async fn poll_github_login(
    login_session_id: String,
    session: LoginSession,
) -> Result<ConnectorLoginPollResult, String> {
    let client = reqwest::Client::new();
    let response = client
        .post(github_access_token_url())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body(&[
            ("client_id", session.client_id.clone()),
            ("device_code", session.device_code.clone()),
            ("grant_type", GITHUB_DEVICE_GRANT_TYPE.to_string()),
        ]))
        .send()
        .await
        .map_err(|err| format!("poll GitHub OAuth token: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("read GitHub OAuth token response: {err}"))?;
    if !status.is_success() {
        return Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Error,
            message: format!(
                "GitHub OAuth token request returned {status}: {}",
                http::redact_and_truncate(&body, 600)
            ),
            interval_secs: session.interval_secs,
            connector: None,
        });
    }

    let parsed = serde_json::from_str::<GitHubTokenResponse>(&body)
        .map_err(|err| format!("parse GitHub OAuth token response: {err}"))?;
    if let Some(token) = parsed.access_token.filter(|value| !value.trim().is_empty()) {
        let account_label = github_user_login(&token).await.ok();
        secret_store::store_connector_secret("github", GITHUB_OAUTH_TOKEN_SECRET, &token)?;
        sessions()
            .lock()
            .map_err(|_| "connector login session lock poisoned".to_string())?
            .remove(&login_session_id);
        let connector = connect_connector(ConnectorConnectRequest {
            connector_id: "github".to_string(),
            account_label,
            auth_source: Some("oauth_device".to_string()),
        })?;
        return Ok(ConnectorLoginPollResult {
            connector_id: "github".to_string(),
            provider: "github".to_string(),
            status: ConnectorLoginPollStatus::Complete,
            message: format!(
                "GitHub login complete. Token type: {}. Scopes: {}.",
                parsed.token_type.unwrap_or_else(|| "bearer".to_string()),
                parsed.scope.unwrap_or_else(|| "default".to_string())
            ),
            interval_secs: session.interval_secs,
            connector: Some(connector),
        });
    }

    let error = parsed.error.unwrap_or_else(|| "unknown_error".to_string());
    let message = parsed
        .error_description
        .unwrap_or_else(|| github_device_flow_error_message(&error).to_string());
    match error.as_str() {
        "authorization_pending" => Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Pending,
            message,
            interval_secs: session.interval_secs,
            connector: None,
        }),
        "slow_down" => {
            let interval_secs = parsed
                .interval
                .unwrap_or(session.interval_secs + 5)
                .max(session.interval_secs + 5);
            sessions()
                .lock()
                .map_err(|_| "connector login session lock poisoned".to_string())?
                .entry(login_session_id)
                .and_modify(|stored| stored.interval_secs = interval_secs);
            Ok(ConnectorLoginPollResult {
                connector_id: session.connector_id,
                provider: session.provider,
                status: ConnectorLoginPollStatus::SlowDown,
                message,
                interval_secs,
                connector: None,
            })
        }
        "expired_token" | "token_expired" => {
            sessions()
                .lock()
                .map_err(|_| "connector login session lock poisoned".to_string())?
                .remove(&login_session_id);
            Ok(ConnectorLoginPollResult {
                connector_id: session.connector_id,
                provider: session.provider,
                status: ConnectorLoginPollStatus::Expired,
                message,
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
        "access_denied" => {
            sessions()
                .lock()
                .map_err(|_| "connector login session lock poisoned".to_string())?
                .remove(&login_session_id);
            Ok(ConnectorLoginPollResult {
                connector_id: session.connector_id,
                provider: session.provider,
                status: ConnectorLoginPollStatus::Denied,
                message,
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
        _ => Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Error,
            message: format!("GitHub device flow failed: {error} - {message}"),
            interval_secs: session.interval_secs,
            connector: None,
        }),
    }
}

async fn poll_gmail_login(
    login_session_id: String,
    session: LoginSession,
) -> Result<ConnectorLoginPollResult, String> {
    let expected_state = session
        .state
        .clone()
        .ok_or_else(|| "Gmail OAuth session is missing CSRF state".to_string())?;
    let callback = browser_callbacks()
        .lock()
        .map_err(|_| "connector OAuth callback lock poisoned".to_string())?
        .remove(&expected_state);

    let Some(callback) = callback else {
        return Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Pending,
            message:
                "Waiting for Gmail browser authorization to finish. Approve the Google page opened from Omiga."
                    .to_string(),
            interval_secs: session.interval_secs,
            connector: None,
        });
    };

    match callback {
        BrowserOAuthCallbackResult::Success(success) => {
            if success.state != expected_state {
                clear_login_session(&login_session_id)?;
                return Ok(ConnectorLoginPollResult {
                    connector_id: "gmail".to_string(),
                    provider: GMAIL_OAUTH_PROVIDER.to_string(),
                    status: ConnectorLoginPollStatus::Error,
                    message:
                        "Gmail OAuth callback state did not match the active login session. Start connection again."
                            .to_string(),
                    interval_secs: session.interval_secs,
                    connector: None,
                });
            }

            match exchange_gmail_code_for_token(&session, &success.code).await {
                Ok(parsed) => complete_gmail_login(login_session_id, session, parsed).await,
                Err(message) => {
                    clear_login_session(&login_session_id)?;
                    Ok(ConnectorLoginPollResult {
                        connector_id: "gmail".to_string(),
                        provider: GMAIL_OAUTH_PROVIDER.to_string(),
                        status: ConnectorLoginPollStatus::Error,
                        message,
                        interval_secs: session.interval_secs,
                        connector: None,
                    })
                }
            }
        }
        BrowserOAuthCallbackResult::ProviderError(error) => {
            clear_login_session(&login_session_id)?;
            let error_code = error.error.unwrap_or_else(|| "oauth_error".to_string());
            let status = if error_code == "access_denied" {
                ConnectorLoginPollStatus::Denied
            } else {
                ConnectorLoginPollStatus::Error
            };
            Ok(ConnectorLoginPollResult {
                connector_id: "gmail".to_string(),
                provider: GMAIL_OAUTH_PROVIDER.to_string(),
                status,
                message: format!(
                    "Gmail authorization returned {error_code}: {}",
                    error
                        .error_description
                        .unwrap_or_else(|| "No additional details provided.".to_string())
                ),
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
        BrowserOAuthCallbackResult::ListenerError(message) => {
            clear_login_session(&login_session_id)?;
            Ok(ConnectorLoginPollResult {
                connector_id: "gmail".to_string(),
                provider: GMAIL_OAUTH_PROVIDER.to_string(),
                status: ConnectorLoginPollStatus::Error,
                message,
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
    }
}

async fn poll_notion_login(
    login_session_id: String,
    session: LoginSession,
) -> Result<ConnectorLoginPollResult, String> {
    let expected_state = session
        .state
        .clone()
        .ok_or_else(|| "Notion OAuth session is missing CSRF state".to_string())?;
    let callback = browser_callbacks()
        .lock()
        .map_err(|_| "connector OAuth callback lock poisoned".to_string())?
        .remove(&expected_state);

    let Some(callback) = callback else {
        return Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Pending,
            message: "Waiting for Notion browser authorization to finish. Approve the official Notion page opened from Omiga.".to_string(),
            interval_secs: session.interval_secs,
            connector: None,
        });
    };

    match callback {
        BrowserOAuthCallbackResult::Success(success) => {
            if success.state != expected_state {
                clear_login_session(&login_session_id)?;
                return Ok(ConnectorLoginPollResult {
                    connector_id: "notion".to_string(),
                    provider: NOTION_OAUTH_PROVIDER.to_string(),
                    status: ConnectorLoginPollStatus::Error,
                    message: "Notion OAuth callback state did not match the active login session. Start connection again.".to_string(),
                    interval_secs: session.interval_secs,
                    connector: None,
                });
            }

            match exchange_notion_code_for_token(&session, &success.code).await {
                Ok(parsed) => complete_notion_login(login_session_id, session, parsed),
                Err(message) => {
                    clear_login_session(&login_session_id)?;
                    Ok(ConnectorLoginPollResult {
                        connector_id: "notion".to_string(),
                        provider: NOTION_OAUTH_PROVIDER.to_string(),
                        status: ConnectorLoginPollStatus::Error,
                        message,
                        interval_secs: session.interval_secs,
                        connector: None,
                    })
                }
            }
        }
        BrowserOAuthCallbackResult::ProviderError(error) => {
            clear_login_session(&login_session_id)?;
            let error_code = error.error.unwrap_or_else(|| "oauth_error".to_string());
            let status = if error_code == "access_denied" {
                ConnectorLoginPollStatus::Denied
            } else {
                ConnectorLoginPollStatus::Error
            };
            Ok(ConnectorLoginPollResult {
                connector_id: "notion".to_string(),
                provider: NOTION_OAUTH_PROVIDER.to_string(),
                status,
                message: format!(
                    "Notion authorization returned {error_code}: {}",
                    error
                        .error_description
                        .unwrap_or_else(|| "No additional details provided.".to_string())
                ),
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
        BrowserOAuthCallbackResult::ListenerError(message) => {
            clear_login_session(&login_session_id)?;
            Ok(ConnectorLoginPollResult {
                connector_id: "notion".to_string(),
                provider: NOTION_OAUTH_PROVIDER.to_string(),
                status: ConnectorLoginPollStatus::Error,
                message,
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
    }
}

async fn poll_slack_login(
    login_session_id: String,
    session: LoginSession,
) -> Result<ConnectorLoginPollResult, String> {
    let expected_state = session
        .state
        .clone()
        .ok_or_else(|| "Slack OAuth session is missing CSRF state".to_string())?;
    let callback = browser_callbacks()
        .lock()
        .map_err(|_| "connector OAuth callback lock poisoned".to_string())?
        .remove(&expected_state);

    let Some(callback) = callback else {
        return Ok(ConnectorLoginPollResult {
            connector_id: session.connector_id,
            provider: session.provider,
            status: ConnectorLoginPollStatus::Pending,
            message: "Waiting for Slack browser authorization to finish. Approve the official Slack page opened from Omiga.".to_string(),
            interval_secs: session.interval_secs,
            connector: None,
        });
    };

    match callback {
        BrowserOAuthCallbackResult::Success(success) => {
            if success.state != expected_state {
                clear_login_session(&login_session_id)?;
                return Ok(ConnectorLoginPollResult {
                    connector_id: "slack".to_string(),
                    provider: SLACK_OAUTH_PROVIDER.to_string(),
                    status: ConnectorLoginPollStatus::Error,
                    message: "Slack OAuth callback state did not match the active login session. Start connection again.".to_string(),
                    interval_secs: session.interval_secs,
                    connector: None,
                });
            }

            match exchange_slack_code_for_token(&session, &success.code).await {
                Ok(parsed) => complete_slack_login(login_session_id, session, parsed),
                Err(message) => {
                    clear_login_session(&login_session_id)?;
                    Ok(ConnectorLoginPollResult {
                        connector_id: "slack".to_string(),
                        provider: SLACK_OAUTH_PROVIDER.to_string(),
                        status: ConnectorLoginPollStatus::Error,
                        message,
                        interval_secs: session.interval_secs,
                        connector: None,
                    })
                }
            }
        }
        BrowserOAuthCallbackResult::ProviderError(error) => {
            clear_login_session(&login_session_id)?;
            let error_code = error.error.unwrap_or_else(|| "oauth_error".to_string());
            let status = if error_code == "access_denied" {
                ConnectorLoginPollStatus::Denied
            } else {
                ConnectorLoginPollStatus::Error
            };
            Ok(ConnectorLoginPollResult {
                connector_id: "slack".to_string(),
                provider: SLACK_OAUTH_PROVIDER.to_string(),
                status,
                message: format!(
                    "Slack authorization returned {error_code}: {}",
                    error
                        .error_description
                        .unwrap_or_else(|| "No additional details provided.".to_string())
                ),
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
        BrowserOAuthCallbackResult::ListenerError(message) => {
            clear_login_session(&login_session_id)?;
            Ok(ConnectorLoginPollResult {
                connector_id: "slack".to_string(),
                provider: SLACK_OAUTH_PROVIDER.to_string(),
                status: ConnectorLoginPollStatus::Error,
                message,
                interval_secs: session.interval_secs,
                connector: None,
            })
        }
    }
}

fn clear_login_session(login_session_id: &str) -> Result<(), String> {
    sessions()
        .lock()
        .map_err(|_| "connector login session lock poisoned".to_string())?
        .remove(login_session_id);
    Ok(())
}

async fn exchange_gmail_code_for_token(
    session: &LoginSession,
    code: &str,
) -> Result<GoogleOAuthTokenResponse, String> {
    let code_verifier = session
        .code_verifier
        .as_deref()
        .ok_or_else(|| "Gmail OAuth session is missing PKCE code verifier".to_string())?;
    let redirect_uri = session
        .redirect_uri
        .as_deref()
        .ok_or_else(|| "Gmail OAuth session is missing redirect URI".to_string())?;
    let mut form = vec![
        ("client_id", session.client_id.clone()),
        ("code", code.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", code_verifier.to_string()),
    ];
    if let Some(client_secret) = session
        .client_secret
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        form.push(("client_secret", client_secret.to_string()));
    }
    let response = reqwest::Client::new()
        .post(google_token_url())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body(&form))
        .send()
        .await
        .map_err(|err| format!("exchange Gmail OAuth code: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("read Gmail OAuth token response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Gmail OAuth token request returned {status}: {}",
            http::redact_and_truncate(&body, 600)
        ));
    }
    let parsed = serde_json::from_str::<GoogleOAuthTokenResponse>(&body)
        .map_err(|err| format!("parse Gmail OAuth token response: {err}"))?;
    if parsed.error.is_some() || parsed.error_description.is_some() {
        return Err(format!(
            "Gmail OAuth token response returned {}: {}",
            parsed
                .error
                .clone()
                .unwrap_or_else(|| "oauth_error".to_string()),
            parsed
                .error_description
                .clone()
                .unwrap_or_else(|| "No additional details provided.".to_string())
        ));
    }
    Ok(parsed)
}

#[allow(dead_code)]
async fn refresh_gmail_access_token(refresh_token: &str) -> Result<String, String> {
    let client_id = gmail_oauth_client_id()?;
    let client_secret = gmail_oauth_client_secret();
    let response = reqwest::Client::new()
        .post(google_token_url())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body(&gmail_refresh_token_form(
            &client_id,
            client_secret.as_deref(),
            refresh_token,
        )))
        .send()
        .await
        .map_err(|err| format!("refresh Gmail OAuth token: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("read Gmail OAuth refresh response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Gmail OAuth refresh returned {status}: {}",
            http::redact_and_truncate(&body, 600)
        ));
    }
    let parsed = serde_json::from_str::<GoogleOAuthTokenResponse>(&body)
        .map_err(|err| format!("parse Gmail OAuth refresh response: {err}"))?;
    if parsed.error.is_some() || parsed.error_description.is_some() {
        return Err(format!(
            "Gmail OAuth refresh failed: {}{}",
            parsed.error.unwrap_or_else(|| "oauth_error".to_string()),
            parsed
                .error_description
                .map(|description| format!(" - {description}"))
                .unwrap_or_default()
        ));
    }
    let token = parsed
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Gmail OAuth refresh response did not include access_token".to_string())?
        .to_string();
    store_gmail_token_response(&parsed)?;
    Ok(token)
}

fn store_gmail_token_response(parsed: &GoogleOAuthTokenResponse) -> Result<(), String> {
    let token = parsed
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Gmail OAuth token response did not include an access_token".to_string())?;
    secret_store::store_connector_secret("gmail", GMAIL_OAUTH_TOKEN_SECRET, token)?;
    if let Some(refresh_token) = parsed
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        secret_store::store_connector_secret(
            "gmail",
            GMAIL_OAUTH_REFRESH_TOKEN_SECRET,
            refresh_token,
        )?;
    }
    if let Some(expires_in) = parsed.expires_in {
        let expires_at = Utc::now() + Duration::seconds(expires_in.min(i64::MAX as u64) as i64);
        secret_store::store_connector_secret(
            "gmail",
            GMAIL_OAUTH_EXPIRES_AT_SECRET,
            &expires_at.to_rfc3339(),
        )?;
    }
    Ok(())
}

async fn complete_gmail_login(
    login_session_id: String,
    session: LoginSession,
    parsed: GoogleOAuthTokenResponse,
) -> Result<ConnectorLoginPollResult, String> {
    let token = parsed
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Gmail OAuth token response did not include an access_token".to_string())?;
    store_gmail_token_response(&parsed)?;
    clear_login_session(&login_session_id)?;
    let account_label = gmail_profile_email(token).await.ok();
    let connector = connect_connector(ConnectorConnectRequest {
        connector_id: "gmail".to_string(),
        account_label: account_label.clone(),
        auth_source: Some("oauth_browser".to_string()),
    })?;
    let account = account_label.unwrap_or_else(|| "Gmail account".to_string());
    let expiry = parsed
        .expires_in
        .map(|seconds| format!(", expires in {seconds}s"))
        .unwrap_or_default();
    Ok(ConnectorLoginPollResult {
        connector_id: "gmail".to_string(),
        provider: GMAIL_OAUTH_PROVIDER.to_string(),
        status: ConnectorLoginPollStatus::Complete,
        message: format!(
            "Gmail login complete for {account}. Token type: {}{expiry}. Scopes: {}. Secret stored outside connectors/config.json.",
            parsed.token_type.unwrap_or_else(|| "Bearer".to_string()),
            parsed.scope.unwrap_or_else(|| "configured".to_string())
        ),
        interval_secs: session.interval_secs,
        connector: Some(connector),
    })
}

async fn exchange_notion_code_for_token(
    session: &LoginSession,
    code: &str,
) -> Result<NotionTokenResponse, String> {
    let client_secret = session
        .client_secret
        .as_deref()
        .ok_or_else(|| "Notion OAuth session is missing client secret".to_string())?;
    let redirect_uri = session
        .redirect_uri
        .as_deref()
        .ok_or_else(|| "Notion OAuth session is missing redirect URI".to_string())?;
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{client_secret}", session.client_id).as_bytes());
    let response = reqwest::Client::new()
        .post(notion_token_url())
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Basic {basic}"))
        .header("Notion-Version", notion_oauth_version())
        .json(&json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri,
        }))
        .send()
        .await
        .map_err(|err| format!("exchange Notion OAuth code: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("read Notion OAuth token response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Notion OAuth token request returned {status}: {}",
            http::redact_and_truncate(&body, 600)
        ));
    }
    let parsed = serde_json::from_str::<NotionTokenResponse>(&body)
        .map_err(|err| format!("parse Notion OAuth token response: {err}"))?;
    if parsed.error.is_some() || parsed.error_description.is_some() {
        return Err(format!(
            "Notion OAuth token response returned {}: {}",
            parsed
                .error
                .clone()
                .unwrap_or_else(|| "oauth_error".to_string()),
            parsed
                .error_description
                .clone()
                .unwrap_or_else(|| "No additional details provided.".to_string())
        ));
    }
    Ok(parsed)
}

fn complete_notion_login(
    login_session_id: String,
    session: LoginSession,
    parsed: NotionTokenResponse,
) -> Result<ConnectorLoginPollResult, String> {
    let token = parsed
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Notion OAuth token response did not include an access_token".to_string())?;
    secret_store::store_connector_secret("notion", NOTION_OAUTH_TOKEN_SECRET, token)?;
    clear_login_session(&login_session_id)?;
    let account_label = parsed
        .workspace_name
        .clone()
        .or(parsed.workspace_id.clone())
        .or(parsed.bot_id.clone());
    let connector = connect_connector(ConnectorConnectRequest {
        connector_id: "notion".to_string(),
        account_label: account_label.clone(),
        auth_source: Some("oauth_browser".to_string()),
    })?;
    let workspace = account_label.unwrap_or_else(|| "Notion workspace".to_string());
    Ok(ConnectorLoginPollResult {
        connector_id: "notion".to_string(),
        provider: NOTION_OAUTH_PROVIDER.to_string(),
        status: ConnectorLoginPollStatus::Complete,
        message: format!(
            "Notion login complete for {workspace}. Token type: {}. Secret stored outside connectors/config.json.",
            parsed.token_type.unwrap_or_else(|| "bearer".to_string())
        ),
        interval_secs: session.interval_secs,
        connector: Some(connector),
    })
}

async fn exchange_slack_code_for_token(
    session: &LoginSession,
    code: &str,
) -> Result<SlackOAuthAccessResponse, String> {
    let client_secret = session
        .client_secret
        .as_deref()
        .ok_or_else(|| "Slack OAuth session is missing client secret".to_string())?;
    let redirect_uri = session
        .redirect_uri
        .as_deref()
        .ok_or_else(|| "Slack OAuth session is missing redirect URI".to_string())?;
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{client_secret}", session.client_id).as_bytes());
    let response = reqwest::Client::new()
        .post(slack_token_url())
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Authorization", format!("Basic {basic}"))
        .body(form_body(&[
            ("code", code.to_string()),
            ("redirect_uri", redirect_uri.to_string()),
        ]))
        .send()
        .await
        .map_err(|err| format!("exchange Slack OAuth code: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("read Slack OAuth token response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Slack OAuth token request returned {status}: {}",
            http::redact_and_truncate(&body, 600)
        ));
    }
    let parsed = serde_json::from_str::<SlackOAuthAccessResponse>(&body)
        .map_err(|err| format!("parse Slack OAuth token response: {err}"))?;
    if parsed.ok == Some(false) || parsed.error.is_some() {
        return Err(format!(
            "Slack OAuth token response returned {}",
            parsed
                .error
                .clone()
                .unwrap_or_else(|| "oauth_error".to_string())
        ));
    }
    Ok(parsed)
}

fn complete_slack_login(
    login_session_id: String,
    session: LoginSession,
    parsed: SlackOAuthAccessResponse,
) -> Result<ConnectorLoginPollResult, String> {
    let token = parsed
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Slack OAuth token response did not include an access_token".to_string())?;
    secret_store::store_connector_secret("slack", SLACK_OAUTH_TOKEN_SECRET, token)?;
    clear_login_session(&login_session_id)?;
    let account_label = slack_account_label(&parsed);
    let connector = connect_connector(ConnectorConnectRequest {
        connector_id: "slack".to_string(),
        account_label: account_label.clone(),
        auth_source: Some("oauth_browser".to_string()),
    })?;
    let workspace = account_label.unwrap_or_else(|| "Slack workspace".to_string());
    Ok(ConnectorLoginPollResult {
        connector_id: "slack".to_string(),
        provider: SLACK_OAUTH_PROVIDER.to_string(),
        status: ConnectorLoginPollStatus::Complete,
        message: format!(
            "Slack login complete for {workspace}. Token type: {}. Scopes: {}. Secret stored outside connectors/config.json.",
            parsed.token_type.unwrap_or_else(|| "bot".to_string()),
            parsed.scope.unwrap_or_else(|| "configured".to_string())
        ),
        interval_secs: session.interval_secs,
        connector: Some(connector),
    })
}

fn slack_account_label(parsed: &SlackOAuthAccessResponse) -> Option<String> {
    parsed
        .team
        .as_ref()
        .and_then(|team| team.name.clone().or(team.id.clone()))
        .or_else(|| {
            parsed
                .enterprise
                .as_ref()
                .and_then(|enterprise| enterprise.name.clone().or(enterprise.id.clone()))
        })
        .or_else(|| parsed.bot_user_id.clone())
}

fn start_browser_callback_listener(
    service_name: &'static str,
    port: u16,
    expected_path: String,
    expected_state: String,
    timeout_secs: u64,
) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|err| {
        format!(
            "start local OAuth callback listener for {service_name} on 127.0.0.1:{port}: {err}. If another process uses this port, set the connector-specific local callback port and register/bridge the matching redirect URI."
        )
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|err| format!("configure local OAuth callback listener: {err}"))?;

    thread::spawn(move || {
        let deadline = Instant::now() + StdDuration::from_secs(timeout_secs.saturating_add(15));
        loop {
            if Instant::now() >= deadline {
                store_browser_callback(
                    &expected_state,
                    BrowserOAuthCallbackResult::ListenerError(format!(
                        "Timed out waiting for the local {service_name} OAuth callback."
                    )),
                );
                break;
            }
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    let should_stop = handle_browser_callback_stream(
                        &mut stream,
                        &expected_path,
                        &expected_state,
                    );
                    if should_stop {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(StdDuration::from_millis(60));
                }
                Err(err) => {
                    store_browser_callback(
                        &expected_state,
                        BrowserOAuthCallbackResult::ListenerError(format!(
                            "Local OAuth callback listener failed: {err}"
                        )),
                    );
                    break;
                }
            }
        }
    });
    Ok(())
}

fn handle_browser_callback_stream(
    stream: &mut TcpStream,
    expected_path: &str,
    expected_state: &str,
) -> bool {
    let target = match read_http_request_target(stream) {
        Ok(target) => target,
        Err(message) => {
            let _ = respond_oauth_callback(stream, 400, "Omiga OAuth callback", &message);
            return false;
        }
    };

    match parse_browser_oauth_callback(&target, expected_path) {
        BrowserOAuthCallbackOutcome::Success(success) => {
            let _ = respond_oauth_callback(
                stream,
                200,
                "Omiga OAuth complete",
                "Authorization complete. You may close this window and return to Omiga.",
            );
            store_browser_callback(expected_state, BrowserOAuthCallbackResult::Success(success));
            true
        }
        BrowserOAuthCallbackOutcome::ProviderError(error) => {
            let _ = respond_oauth_callback(
                stream,
                400,
                "Omiga OAuth denied",
                "The OAuth provider returned an error. Return to Omiga for details.",
            );
            store_browser_callback(
                expected_state,
                BrowserOAuthCallbackResult::ProviderError(error),
            );
            true
        }
        BrowserOAuthCallbackOutcome::Invalid => {
            if target.starts_with(expected_path) {
                let _ = respond_oauth_callback(
                    stream,
                    400,
                    "Omiga OAuth callback invalid",
                    "The OAuth callback did not include a code or provider error.",
                );
                store_browser_callback(
                    expected_state,
                    BrowserOAuthCallbackResult::ListenerError(
                        "OAuth callback did not include a code or provider error.".to_string(),
                    ),
                );
                true
            } else {
                let _ = respond_oauth_callback(
                    stream,
                    404,
                    "Omiga OAuth callback",
                    "This local callback server only accepts the active connector authorization route.",
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
        .map_err(|err| format!("read OAuth callback request: {err}"))?;
    if read == 0 {
        return Err("OAuth callback request was empty".to_string());
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| "OAuth callback request was missing a request line".to_string())?;
    let mut parts = first_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "OAuth callback request was missing method".to_string())?;
    if method != "GET" {
        return Err(format!("OAuth callback must use GET, got {method}"));
    }
    parts
        .next()
        .map(str::to_string)
        .ok_or_else(|| "OAuth callback request was missing target".to_string())
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

fn store_browser_callback(expected_state: &str, callback: BrowserOAuthCallbackResult) {
    if let Ok(mut callbacks) = browser_callbacks().lock() {
        callbacks.insert(expected_state.to_string(), callback);
    }
}

fn parse_browser_oauth_callback(target: &str, expected_path: &str) -> BrowserOAuthCallbackOutcome {
    let Some((route, query)) = target.split_once('?') else {
        return BrowserOAuthCallbackOutcome::Invalid;
    };
    if route != expected_path {
        return BrowserOAuthCallbackOutcome::Invalid;
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

    if let (Some(code), Some(state)) = (code, state.clone()) {
        return BrowserOAuthCallbackOutcome::Success(BrowserOAuthCallbackSuccess { code, state });
    }

    if error.is_some() || error_description.is_some() {
        return BrowserOAuthCallbackOutcome::ProviderError(BrowserOAuthProviderError {
            state,
            error,
            error_description,
        });
    }

    BrowserOAuthCallbackOutcome::Invalid
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

fn github_device_flow_error_message(error: &str) -> &'static str {
    match error {
        "authorization_pending" => "Waiting for you to approve the GitHub device code.",
        "slow_down" => "GitHub asked Omiga to slow down polling.",
        "expired_token" | "token_expired" => "The GitHub device code expired.",
        "access_denied" => "GitHub authorization was denied.",
        "incorrect_client_credentials" => "GitHub rejected the OAuth client ID.",
        "device_flow_disabled" => "Device Flow is disabled for this GitHub app.",
        _ => "GitHub returned an OAuth device-flow error.",
    }
}

async fn github_user_login(token: &str) -> Result<String, String> {
    let value = http::send_connector_json(
        ConnectorHttpRequest::new(
            "GitHub",
            Method::GET,
            format!("{}/user", github_api_base_url()),
        )
        .bearer_token(token),
    )
    .await
    .map_err(|err| err.user_message())?;
    value
        .get("login")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "GitHub /user response did not include login".to_string())
}

async fn gmail_profile_email(token: &str) -> Result<String, String> {
    let value = http::send_connector_json(
        ConnectorHttpRequest::new(
            "Gmail",
            Method::GET,
            format!("{}/users/me/profile", gmail_api_base_url()),
        )
        .bearer_token(token),
    )
    .await
    .map_err(|err| err.user_message())?;
    value
        .get("emailAddress")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "Gmail profile response did not include emailAddress".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_error_messages_are_user_actionable() {
        assert!(github_device_flow_error_message("authorization_pending").contains("Waiting"));
        assert!(github_device_flow_error_message("device_flow_disabled").contains("disabled"));
    }

    #[test]
    fn notion_authorization_url_contains_required_oauth_params() {
        let url = notion_authorization_url(
            "client id",
            "http://127.0.0.1:17654/connectors/notion/callback",
            "state value",
        );

        assert!(url.starts_with("https://api.notion.com/v1/oauth/authorize?"));
        assert!(url.contains("client_id=client+id"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("owner=user"));
        assert!(url.contains(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A17654%2Fconnectors%2Fnotion%2Fcallback"
        ));
        assert!(url.contains("state=state+value"));
    }

    #[test]
    fn slack_authorization_url_contains_required_oauth_params() {
        let url = slack_authorization_url(
            "client id",
            "https://example.com/omiga/slack/callback",
            "state value",
        );

        assert!(url.starts_with("https://slack.com/oauth/v2/authorize?"));
        assert!(url.contains("client_id=client+id"));
        assert!(url.contains("scope=channels%3Aread%2Cchannels%3Ahistory%2Cchat%3Awrite"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fexample.com%2Fomiga%2Fslack%2Fcallback"));
        assert!(url.contains("state=state+value"));
    }

    #[test]
    fn gmail_authorization_url_contains_omiga_oauth_params() {
        let url = gmail_authorization_url(
            "google client",
            "http://127.0.0.1:17656/connectors/gmail/callback",
            "state value",
            "pkce challenge",
        );

        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=google+client"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A17656%2Fconnectors%2Fgmail%2Fcallback"
        ));
        assert!(url.contains("scope="));
        assert!(url.contains("gmail.readonly"));
        assert!(url.contains("gmail.send"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("code_challenge=pkce+challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state+value"));
        assert!(!url.contains("chatgpt.com"));
    }

    #[test]
    fn generated_pkce_codes_are_url_safe_and_linked() {
        let pkce = generate_pkce_codes();
        let digest = Sha256::digest(pkce.code_verifier.as_bytes());
        let expected_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

        assert!(pkce.code_verifier.len() >= 43);
        assert!(pkce.code_verifier.len() <= 128);
        assert_eq!(pkce.code_challenge, expected_challenge);
        assert!(!pkce.code_verifier.contains('+'));
        assert!(!pkce.code_verifier.contains('/'));
        assert!(!pkce.code_verifier.contains('='));
    }

    #[test]
    fn gmail_refresh_token_form_uses_pkce_public_client_shape() {
        let public_form = gmail_refresh_token_form("google client", None, "refresh-token");
        assert!(public_form.contains(&("client_id", "google client".to_string())));
        assert!(public_form.contains(&("grant_type", "refresh_token".to_string())));
        assert!(public_form.contains(&("refresh_token", "refresh-token".to_string())));
        assert!(!public_form.iter().any(|(key, _)| *key == "client_secret"));

        let confidential_form =
            gmail_refresh_token_form("google client", Some("secret"), "refresh-token");
        assert!(confidential_form.contains(&("client_secret", "secret".to_string())));
    }

    #[test]
    fn parse_local_redirect_uri_rejects_non_local_callbacks() {
        let err = parse_local_redirect_uri("https://example.com/callback")
            .expect_err("non-local callback should be rejected");

        assert!(err.contains("local callback URI"));
    }

    #[test]
    fn parse_browser_oauth_callback_accepts_code_and_state() {
        let parsed = parse_browser_oauth_callback(
            "/connectors/notion/callback?code=abc%20123&state=state+value",
            "/connectors/notion/callback",
        );

        assert_eq!(
            parsed,
            BrowserOAuthCallbackOutcome::Success(BrowserOAuthCallbackSuccess {
                code: "abc 123".to_string(),
                state: "state value".to_string(),
            })
        );
    }

    #[test]
    fn parse_browser_oauth_callback_returns_provider_error() {
        let parsed = parse_browser_oauth_callback(
            "/connectors/notion/callback?error=access_denied&error_description=user+denied&state=s1",
            "/connectors/notion/callback",
        );

        assert_eq!(
            parsed,
            BrowserOAuthCallbackOutcome::ProviderError(BrowserOAuthProviderError {
                state: Some("s1".to_string()),
                error: Some("access_denied".to_string()),
                error_description: Some("user denied".to_string()),
            })
        );
    }

    #[test]
    fn parse_browser_oauth_callback_rejects_wrong_path() {
        let parsed = parse_browser_oauth_callback(
            "/favicon.ico?code=abc&state=s1",
            "/connectors/notion/callback",
        );

        assert_eq!(parsed, BrowserOAuthCallbackOutcome::Invalid);
    }

    #[test]
    fn parse_local_redirect_uri_extracts_port_and_path() {
        let parsed = parse_local_redirect_uri("http://localhost:17654/connectors/notion/callback")
            .expect("redirect should parse");

        assert_eq!(
            parsed,
            (
                "http://localhost:17654/connectors/notion/callback".to_string(),
                17654,
                "/connectors/notion/callback".to_string(),
            )
        );
    }
}
