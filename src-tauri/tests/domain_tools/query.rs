//! Tests for structured `query`.

use omiga_lib::domain::tools::{Tool, ToolKind};

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
