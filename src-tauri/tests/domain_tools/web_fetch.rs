//! Tests for `web_fetch`.

use omiga_lib::domain::tools::web_fetch::{WebFetchArgs, WebFetchTool};
use omiga_lib::domain::tools::{ToolContext, ToolImpl};

#[tokio::test]
async fn web_fetch_rejects_private_loopback_target() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = WebFetchArgs {
        url: "http://127.0.0.1:8080/page".to_string(),
        prompt: "What is the greeting?".to_string(),
    };
    let err = match WebFetchTool::execute(&ctx, args).await {
        Ok(_) => panic!("loopback target should be blocked"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(msg.contains("Blocked") || msg.contains("private") || msg.contains("loopback"));
}

#[tokio::test]
async fn web_fetch_rejects_non_http_scheme() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = WebFetchArgs {
        url: "ftp://example.com/x".to_string(),
        prompt: "x".to_string(),
    };
    assert!(WebFetchTool::execute(&ctx, args).await.is_err());
}
