//! Tests for structured `query`.

use futures::StreamExt;
use omiga_lib::domain::search::data::DataApiBaseUrls;
use omiga_lib::domain::tools::query::{QueryArgs, QueryTool};
use omiga_lib::domain::tools::{Tool, ToolContext, ToolImpl, ToolKind, WebSearchApiKeys};
use omiga_lib::infrastructure::streaming::StreamOutputItem;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

#[test]
fn query_from_json_accepts_dataset_search() {
    let j = r#"{
        "category": "dataset",
        "operation": "search",
        "subcategory": "sequencing",
        "source": "auto",
        "query": "rumen metagenome",
        "max_results": 3
    }"#;

    let tool = Tool::from_json_str("query", j).unwrap();

    assert!(matches!(tool, Tool::Query(_)));
    assert_eq!(tool.kind(), ToolKind::Query);
    assert_eq!(tool.name(), "query");
}

#[test]
fn query_from_json_accepts_dataset_fetch() {
    let j = r#"{
        "category": "data",
        "operation": "fetch",
        "source": "ena_run",
        "id": "ERR12345"
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_cbioportal_source() {
    let j = r#"{
        "category": "dataset",
        "operation": "search",
        "source": "cbioportal",
        "query": "breast cancer",
        "max_results": 3
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_gtex_source() {
    let j = r#"{
        "category": "dataset",
        "operation": "search",
        "source": "gtex",
        "query": "BRCA1",
        "params": {
            "endpoint": "median_expression",
            "gencodeId": "ENSG00000012048.21",
            "tissueSiteDetailId": "Whole_Blood",
            "limit": 3
        }
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_ncbi_datasets_source() {
    let j = r#"{
        "category": "dataset",
        "operation": "search",
        "source": "ncbi_datasets",
        "query": "9606",
        "params": {
            "mode": "taxon",
            "reference_only": true,
            "assembly_level": ["chromosome", "complete_genome"]
        },
        "max_results": 2
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_params_aliases() {
    let j = r#"{
        "category": "dataset",
        "params": {
            "op": "search",
            "type": "expression",
            "q": "single cell",
            "limit": "2"
        }
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_ncbi_gene_search() {
    let j = r#"{
        "category": "knowledge",
        "source": "ncbi_gene",
        "operation": "search",
        "query": "TP53",
        "params": {
            "organism": "Homo sapiens",
            "limit": 5
        }
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_ncbi_gene_fetch() {
    let j = r#"{
        "category": "knowledge",
        "source": "ncbi_gene",
        "operation": "fetch",
        "id": "7157"
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_uniprot_search() {
    let j = r#"{
        "category": "knowledge",
        "source": "uniprot",
        "operation": "search",
        "query": "gene_exact:BRCA1",
        "params": {
            "taxon_id": "9606",
            "reviewed": true,
            "limit": 3
        }
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[test]
fn query_from_json_accepts_uniprot_fetch() {
    let j = r#"{
        "category": "knowledge",
        "source": "uniprot",
        "operation": "fetch",
        "id": "P38398"
    }"#;

    assert!(matches!(
        Tool::from_json_str("query", j),
        Ok(Tool::Query(_))
    ));
}

#[tokio::test]
async fn query_tool_executes_gtex_against_mock_api() {
    let mut enabled = HashMap::new();
    enabled.insert("dataset".to_string(), vec!["gtex".to_string()]);
    let keys = WebSearchApiKeys {
        enabled_sources_by_category: Some(enabled),
        ..WebSearchApiKeys::default()
    };
    let ctx = ToolContext::new(std::env::temp_dir())
        .with_web_search_api_keys(keys)
        .with_web_use_proxy(false)
        .with_data_api_base_urls(DataApiBaseUrls {
            gtex: "mock://gtex".to_string(),
            ..DataApiBaseUrls::default()
        });

    let args = QueryArgs {
        category: "dataset".to_string(),
        source: Some("gtex".to_string()),
        operation: Some("search".to_string()),
        subcategory: None,
        query: Some("BRCA1".to_string()),
        id: None,
        url: None,
        result: None,
        params: None,
        max_results: Some(1),
    };
    let json = execute_query_json(&ctx, args).await;

    assert_eq!(json["tool"], "query");
    assert_eq!(json["operation"], "search");
    assert_eq!(json["source"], "gtex");
    assert_eq!(json["results"][0]["title"], "BRCA1 (ENSG00000012048.21)");
    assert_eq!(
        json["results"][0]["metadata"]["source_label"],
        "GTEx tissue expression"
    );
}

#[tokio::test]
async fn query_tool_executes_ncbi_datasets_against_mock_api() {
    let mut enabled = HashMap::new();
    enabled.insert("dataset".to_string(), vec!["ncbi_datasets".to_string()]);
    let keys = WebSearchApiKeys {
        enabled_sources_by_category: Some(enabled),
        ..WebSearchApiKeys::default()
    };
    let ctx = ToolContext::new(std::env::temp_dir())
        .with_web_search_api_keys(keys)
        .with_web_use_proxy(false)
        .with_data_api_base_urls(DataApiBaseUrls {
            ncbi_datasets: "mock://ncbi_datasets".to_string(),
            ..DataApiBaseUrls::default()
        });

    let args = QueryArgs {
        category: "dataset".to_string(),
        source: Some("ncbi_datasets".to_string()),
        operation: Some("search".to_string()),
        subcategory: Some("genomics".to_string()),
        query: Some("9606".to_string()),
        id: None,
        url: None,
        result: None,
        params: Some(serde_json::json!({
            "mode": "taxon",
            "reference_only": true
        })),
        max_results: Some(1),
    };
    let json = execute_query_json(&ctx, args).await;

    assert_eq!(json["source"], "ncbi_datasets");
    assert_eq!(json["results"][0]["accession"], "GCF_000001405.40");
    assert_eq!(
        json["results"][0]["metadata"]["source_label"],
        "NCBI Datasets genome assemblies"
    );
    assert!(
        json["results"][0]["metadata"]["source_specific"]["download_package_url"]
            .as_str()
            .is_some_and(|url| url.contains("DATA_REPORT_ONLY"))
    );
}

#[tokio::test]
async fn query_tool_executes_ncbi_datasets_download_summary_against_mock_api() {
    let mut enabled = HashMap::new();
    enabled.insert("dataset".to_string(), vec!["ncbi_datasets".to_string()]);
    let keys = WebSearchApiKeys {
        enabled_sources_by_category: Some(enabled),
        ..WebSearchApiKeys::default()
    };
    let ctx = ToolContext::new(std::env::temp_dir())
        .with_web_search_api_keys(keys)
        .with_web_use_proxy(false)
        .with_data_api_base_urls(DataApiBaseUrls {
            ncbi_datasets: "mock://ncbi_datasets".to_string(),
            ..DataApiBaseUrls::default()
        });

    let args = QueryArgs {
        category: "dataset".to_string(),
        source: Some("ncbi_datasets".to_string()),
        operation: Some("download_summary".to_string()),
        subcategory: Some("genomics".to_string()),
        query: None,
        id: Some("GCF_000001405.40".to_string()),
        url: None,
        result: None,
        params: Some(serde_json::json!({
            "include": ["genome", "gff3"]
        })),
        max_results: None,
    };
    let json = execute_query_json(&ctx, args).await;

    assert_eq!(json["operation"], "download_summary");
    assert_eq!(json["source"], "ncbi_datasets");
    assert_eq!(json["record_count"], 1);
    assert_eq!(
        json["requested"]["include_annotation_type"][0],
        "GENOME_FASTA"
    );
    assert_eq!(
        json["requested"]["include_annotation_type"][1],
        "GENOME_GFF"
    );
    assert_eq!(
        json["dehydrated"]["cli_download_command_line"],
        "datasets download genome accession GCF_000001405.40 --include gff3,genome --dehydrated"
    );
    assert!(json["content"]
        .as_str()
        .is_some_and(|content| content.contains("Available files")));
}

#[tokio::test]
async fn query_tool_rejects_gtex_when_disabled() {
    let ctx = ToolContext::new(std::env::temp_dir()).with_web_use_proxy(false);
    let args = QueryArgs {
        category: "dataset".to_string(),
        source: Some("gtex".to_string()),
        operation: Some("search".to_string()),
        subcategory: None,
        query: Some("BRCA1".to_string()),
        id: None,
        url: None,
        result: None,
        params: None,
        max_results: Some(1),
    };

    let error = match QueryTool::execute(&ctx, args).await {
        Ok(_) => panic!("GTEx should be disabled by default"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("Dataset source `gtex` is disabled"),
        "{error}"
    );
}

#[tokio::test]
#[ignore = "requires live network access to gtexportal.org"]
async fn query_tool_executes_gtex_against_live_api() {
    let mut enabled = HashMap::new();
    enabled.insert("dataset".to_string(), vec!["gtex".to_string()]);
    let keys = WebSearchApiKeys {
        enabled_sources_by_category: Some(enabled),
        ..WebSearchApiKeys::default()
    };
    let ctx = ToolContext::new(std::env::temp_dir())
        .with_web_search_api_keys(keys)
        .with_web_use_proxy(false);

    let args = QueryArgs {
        category: "dataset".to_string(),
        source: Some("gtex".to_string()),
        operation: Some("search".to_string()),
        subcategory: None,
        query: Some("BRCA1".to_string()),
        id: None,
        url: None,
        result: None,
        params: None,
        max_results: Some(1),
    };
    let json = execute_query_json(&ctx, args).await;

    assert_eq!(json["source"], "gtex");
    assert!(
        json["results"][0]["title"]
            .as_str()
            .is_some_and(|title| title.contains("BRCA1")),
        "{json:#}"
    );
}

async fn execute_query_json(ctx: &ToolContext, args: QueryArgs) -> JsonValue {
    let mut stream = QueryTool::execute(ctx, args)
        .await
        .expect("execute query tool");
    let mut content = String::new();
    while let Some(item) = stream.next().await {
        if let StreamOutputItem::Content(text) = item {
            content.push_str(&text);
        }
    }
    serde_json::from_str(&content).expect("query output should be JSON")
}
