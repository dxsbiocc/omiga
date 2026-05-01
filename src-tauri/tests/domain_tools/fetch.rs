//! Tests for unified `fetch`.

use omiga_lib::domain::tools::fetch::{FetchArgs, FetchTool};
use omiga_lib::domain::tools::{Tool, ToolContext, ToolImpl};

#[test]
fn old_web_fetch_name_is_not_registered() {
    let j = r#"{"url":"https://example.com","prompt":"x"}"#;
    assert!(Tool::from_json_str("web_fetch", j).is_err());
}

#[test]
fn fetch_from_json_accepts_public_literature_sources() {
    for source in ["arxiv", "crossref", "openalex", "biorxiv", "medrxiv"] {
        let j =
            format!(r#"{{"category":"literature","source":"{source}","id":"10.1000/example"}}"#);
        assert!(Tool::from_json_str("fetch", &j).is_ok(), "{source}");
    }
}

#[test]
fn fetch_from_json_accepts_semantic_scholar_source() {
    let j = r#"{"category":"literature","source":"semantic_scholar","id":"DOI:10.1000/example"}"#;
    assert!(Tool::from_json_str("fetch", j).is_ok());
}

#[test]
fn fetch_from_json_accepts_data_sources() {
    for (source, id) in [
        ("geo", "GSE12345"),
        ("ena", "PRJEB12345"),
        ("ena_run", "ERR12345"),
        ("ena_experiment", "ERX12345"),
        ("ena_sample", "ERS12345"),
        ("ena_analysis", "ERZ12345"),
        ("ena_assembly", "GCA_000001405.29"),
        ("ena_sequence", "DQ285577"),
        ("cbioportal", "brca_tcga"),
        ("gtex", "ENSG00000012048.21"),
        ("ncbi_datasets", "GCF_000001405.40"),
        ("arrayexpress", "E-MTAB-9999"),
        ("biosample", "SAMN15960293"),
    ] {
        let j = format!(r#"{{"category":"data","source":"{source}","id":"{id}"}}"#);
        assert!(Tool::from_json_str("fetch", &j).is_ok(), "{source}");
    }
}

#[test]
fn fetch_from_json_accepts_dataset_alias_and_subcategory() {
    let j = r#"{"category":"dataset","subcategory":"sample_metadata","id":"SAMEA123"}"#;
    assert!(Tool::from_json_str("fetch", j).is_ok());
}

#[tokio::test]
async fn fetch_rejects_private_loopback_target() {
    let ctx = ToolContext::new(std::env::temp_dir());
    let args = FetchArgs {
        category: "web".into(),
        source: None,
        subcategory: None,
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
        subcategory: None,
        url: Some("ftp://example.com/x".to_string()),
        id: None,
        result: None,
        prompt: Some("x".to_string()),
    };
    assert!(FetchTool::execute(&ctx, args).await.is_err());
}
