//! Tests for unified `search`.

use omiga_lib::domain::tools::search::{SearchArgs, SearchTool};
use omiga_lib::domain::tools::{Tool, ToolContext, ToolImpl};

#[test]
fn search_from_json_accepts_minimal_web_query() {
    let j = r#"{"category":"web","query":"hello world"}"#;
    assert!(Tool::from_json_str("search", j).is_ok());
}

#[test]
fn search_from_json_accepts_semantic_scholar_source() {
    let j =
        r#"{"category":"literature","source":"semantic_scholar","query":"large language models"}"#;
    assert!(Tool::from_json_str("search", j).is_ok());
}

#[test]
fn search_from_json_accepts_public_literature_sources() {
    for source in ["arxiv", "crossref", "openalex", "biorxiv", "medrxiv"] {
        let j = format!(r#"{{"category":"literature","source":"{source}","query":"crispr"}}"#);
        assert!(Tool::from_json_str("search", &j).is_ok(), "{source}");
    }
}

#[test]
fn search_from_json_accepts_data_sources() {
    for source in [
        "geo",
        "ena",
        "ena_run",
        "ena_experiment",
        "ena_sample",
        "ena_analysis",
        "ena_assembly",
        "ena_sequence",
    ] {
        let j = format!(r#"{{"category":"data","source":"{source}","query":"single cell"}}"#);
        assert!(Tool::from_json_str("search", &j).is_ok(), "{source}");
    }
}

#[test]
fn search_from_json_accepts_dataset_alias_and_subcategory() {
    let j = r#"{"category":"dataset","subcategory":"sequencing","query":"rumen metagenome"}"#;
    assert!(Tool::from_json_str("search", j).is_ok());
}

#[test]
fn search_from_json_accepts_knowledge_category() {
    let j = r#"{"category":"knowledge","source":"wiki","query":"prior decisions"}"#;
    assert!(Tool::from_json_str("search", j).is_ok());
}

#[test]
fn search_from_json_accepts_wechat_source() {
    let j = r#"{"category":"social","source":"wechat","query":"人工智能"}"#;
    assert!(Tool::from_json_str("search", j).is_ok());
}

#[test]
fn old_web_search_name_is_not_registered() {
    let j = r#"{"query":"hello world"}"#;
    assert!(Tool::from_json_str("web_search", j).is_err());
}

#[tokio::test]
async fn search_execute_rejects_both_domain_filters() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = SearchArgs {
        category: "web".into(),
        source: None,
        subcategory: None,
        query: "hello".into(),
        allowed_domains: Some(vec!["a.com".into()]),
        blocked_domains: Some(vec!["b.com".into()]),
        max_results: None,
        search_url: None,
    };
    assert!(SearchTool::execute(&ctx, args).await.is_err());
}
