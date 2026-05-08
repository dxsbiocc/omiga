//! Shared HTTP response handling for literature operations.

use super::super::truncate_chars;

pub(super) async fn read_success_body(
    response: reqwest::Response,
    read_context: &str,
    status_context: &str,
) -> Result<String, String> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("read {read_context}: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "{status_context} returned HTTP {}: {}",
            status.as_u16(),
            truncate_chars(&body, 240)
        ));
    }
    Ok(body)
}
