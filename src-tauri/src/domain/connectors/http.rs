//! Shared HTTP helpers for native connector calls.
//!
//! The connector layer deliberately keeps external-service HTTP behavior in one place so native
//! connector tools and Settings connection checks get the same timeout, retry, and redaction rules.

use reqwest::{Method, StatusCode};
use serde_json::Value as JsonValue;
use std::fmt;
use std::time::Duration;

const CONNECTOR_USER_AGENT: &str = "omiga-connector/0.2";
const DEFAULT_TIMEOUT_SECS: u64 = 20;
const DEFAULT_MAX_RETRIES: u8 = 1;
const BASE_RETRY_BACKOFF_MS: u64 = 120;

#[derive(Debug, Clone)]
pub(crate) struct ConnectorHttpRequest {
    pub service_name: String,
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub json_body: Option<JsonValue>,
    pub timeout_secs: u64,
    pub max_retries: u8,
}

impl ConnectorHttpRequest {
    pub(crate) fn new(
        service_name: impl Into<String>,
        method: Method,
        url: impl Into<String>,
    ) -> Self {
        Self {
            service_name: service_name.into(),
            method,
            url: url.into(),
            headers: Vec::new(),
            query: Vec::new(),
            json_body: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    pub(crate) fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub(crate) fn bearer_token(self, token: impl AsRef<str>) -> Self {
        self.header("Authorization", format!("Bearer {}", token.as_ref()))
    }

    pub(crate) fn query(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.push((name.into(), value.into()));
        self
    }

    pub(crate) fn json_body(mut self, body: JsonValue) -> Self {
        self.json_body = Some(body);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs.max(1);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn max_retries(mut self, max_retries: u8) -> Self {
        self.max_retries = max_retries;
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConnectorHttpResponse {
    pub status: StatusCode,
    pub body: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ConnectorHttpError {
    pub service_name: String,
    pub status: Option<StatusCode>,
    pub message: String,
    pub retryable: bool,
}

impl ConnectorHttpError {
    pub(crate) fn user_message(&self) -> String {
        match self.status {
            Some(status) => format!(
                "{} request returned {status}: {}",
                self.service_name, self.message
            ),
            None => format!("{} request failed: {}", self.service_name, self.message),
        }
    }
}

impl fmt::Display for ConnectorHttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.user_message())
    }
}

impl std::error::Error for ConnectorHttpError {}

pub(crate) async fn send_connector_request(
    request: ConnectorHttpRequest,
) -> Result<ConnectorHttpResponse, ConnectorHttpError> {
    let client = reqwest::Client::builder()
        .user_agent(CONNECTOR_USER_AGENT)
        .timeout(Duration::from_secs(request.timeout_secs.max(1)))
        .build()
        .map_err(|err| ConnectorHttpError {
            service_name: request.service_name.clone(),
            status: None,
            message: format!("build HTTP client: {err}"),
            retryable: false,
        })?;

    let mut attempt = 0_u8;
    loop {
        let response = send_once(&client, &request).await;
        match response {
            Ok(response) if response.status.is_success() => return Ok(response),
            Ok(response) => {
                let retryable = is_retryable_status(response.status);
                if retryable && attempt < request.max_retries {
                    sleep_for_retry(attempt).await;
                    attempt = attempt.saturating_add(1);
                    continue;
                }
                return Err(ConnectorHttpError {
                    service_name: request.service_name.clone(),
                    status: Some(response.status),
                    message: redact_and_truncate(&response.body, 600),
                    retryable,
                });
            }
            Err(err) => {
                let retryable = err.is_timeout() || err.is_connect();
                if retryable && attempt < request.max_retries {
                    sleep_for_retry(attempt).await;
                    attempt = attempt.saturating_add(1);
                    continue;
                }
                return Err(ConnectorHttpError {
                    service_name: request.service_name.clone(),
                    status: None,
                    message: err.to_string(),
                    retryable,
                });
            }
        }
    }
}

pub(crate) async fn send_connector_json(
    request: ConnectorHttpRequest,
) -> Result<JsonValue, ConnectorHttpError> {
    let service_name = request.service_name.clone();
    let response = send_connector_request(request).await?;
    serde_json::from_str(&response.body).map_err(|err| ConnectorHttpError {
        service_name,
        status: Some(response.status),
        message: format!("response JSON parse failed: {err}"),
        retryable: false,
    })
}

async fn send_once(
    client: &reqwest::Client,
    request: &ConnectorHttpRequest,
) -> Result<ConnectorHttpResponse, reqwest::Error> {
    let mut builder = client
        .request(request.method.clone(), &request.url)
        .header("Accept", "application/json");
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    if !request.query.is_empty() {
        builder = builder.query(&request.query);
    }
    if let Some(body) = &request.json_body {
        builder = builder
            .header("Content-Type", "application/json")
            .json(body);
    }
    let response = builder.send().await?;
    let status = response.status();
    let body = response.text().await?;
    Ok(ConnectorHttpResponse { status, body })
}

async fn sleep_for_retry(attempt: u8) {
    let multiplier = u64::from(attempt).saturating_add(1);
    tokio::time::sleep(Duration::from_millis(BASE_RETRY_BACKOFF_MS * multiplier)).await;
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::BAD_GATEWAY
        || status == StatusCode::SERVICE_UNAVAILABLE
        || status == StatusCode::GATEWAY_TIMEOUT
        || status.is_server_error()
}

pub(crate) fn redact_and_truncate(value: &str, max_chars: usize) -> String {
    let redacted = serde_json::from_str::<JsonValue>(value)
        .map(|mut json| {
            redact_json_secrets(&mut json);
            json.to_string()
        })
        .unwrap_or_else(|_| value.to_string());
    truncate_for_display(&redacted, max_chars)
}

fn redact_json_secrets(value: &mut JsonValue) {
    match value {
        JsonValue::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_secretish_key(key) {
                    *value = JsonValue::String("[redacted]".to_string());
                } else {
                    redact_json_secrets(value);
                }
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                redact_json_secrets(item);
            }
        }
        _ => {}
    }
}

fn is_secretish_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("api_key")
        || key.contains("apikey")
        || key == "authorization"
}

fn truncate_for_display(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secretish_json_keys_before_truncating() {
        let body = serde_json::json!({
            "message": "failed",
            "token": "secret-token",
            "nested": {
                "api_key": "secret-key",
                "safe": "visible"
            }
        })
        .to_string();

        let redacted = redact_and_truncate(&body, 500);
        assert!(redacted.contains("[redacted]"));
        assert!(redacted.contains("visible"));
        assert!(!redacted.contains("secret-token"));
        assert!(!redacted.contains("secret-key"));
    }

    #[test]
    fn retryable_statuses_match_transient_http_failures() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
    }
}
