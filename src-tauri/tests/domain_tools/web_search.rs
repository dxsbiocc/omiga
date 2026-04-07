//! Tests for `web_search`.

use omiga_lib::domain::tools::web_search::{WebSearchArgs, WebSearchTool};
use omiga_lib::domain::tools::{Tool, ToolContext, ToolImpl};

#[test]
fn web_search_from_json_accepts_minimal_query() {
    let j = r#"{"query":"hello world"}"#;
    assert!(Tool::from_json_str("web_search", j).is_ok());
}

#[tokio::test]
async fn web_search_execute_rejects_both_domain_filters() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = WebSearchArgs {
        query: "hello".into(),
        allowed_domains: Some(vec!["a.com".into()]),
        blocked_domains: Some(vec!["b.com".into()]),
    };
    assert!(WebSearchTool::execute(&ctx, args).await.is_err());
}
