//! Tests for unified `fetch`.

use omiga_lib::domain::tools::fetch::{FetchArgs, FetchTool};
use omiga_lib::domain::tools::{Tool, ToolContext, ToolImpl};

#[test]
fn old_web_fetch_name_is_not_registered() {
    let j = r#"{"url":"https://example.com","prompt":"x"}"#;
    assert!(Tool::from_json_str("web_fetch", j).is_err());
}

#[tokio::test]
async fn fetch_rejects_private_loopback_target() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = FetchArgs {
        category: "web".into(),
        source: None,
        url: Some("http://127.0.0.1:8080/page".to_string()),
        id: None,
        result: None,
        prompt: Some("What is the greeting?".to_string()),
    };
    let err = match FetchTool::execute(&ctx, args).await {
        Ok(_) => panic!("loopback target should be blocked"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(msg.contains("Blocked") || msg.contains("private") || msg.contains("loopback"));
}

#[tokio::test]
async fn fetch_rejects_non_http_scheme() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = FetchArgs {
        category: "web".into(),
        source: None,
        url: Some("ftp://example.com/x".to_string()),
        id: None,
        result: None,
        prompt: Some("x".to_string()),
    };
    assert!(FetchTool::execute(&ctx, args).await.is_err());
}
