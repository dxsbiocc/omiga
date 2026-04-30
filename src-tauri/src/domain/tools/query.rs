//! Structured database query tool.
//!
//! `query` is the structured companion to `search`/`fetch`: it executes
//! source-specific database operations while reusing the same built-in adapters.
//! The first migration target was `dataset`/`data` (GEO + ENA). Additional
//! databases are added one source at a time through this module.

use super::{ToolContext, ToolError, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

mod common;
mod dataset;
mod knowledge;

pub const DESCRIPTION: &str = r#"Run a structured query against a typed database source and return formatted JSON.

Use `query` when the user wants database-native lookup/query semantics rather than broad discovery:
- `category="dataset"` (`data` alias) supports built-in dataset sources: `geo`, `ena`, `ena_run`, `ena_experiment`, `ena_sample`, `ena_analysis`, `ena_assembly`, `ena_sequence`, `cbioportal`, `gtex`.
- `category="knowledge", source="ncbi_gene"` searches/fetches NCBI Gene via official NCBI E-utilities (`db=gene`).
- `category="knowledge", source="uniprot"` searches/fetches UniProtKB protein entries through the public UniProt REST API.
- `operation="search"` searches records by keyword or database query string. `operation="fetch"`/`"get"` retrieves one record by accession, URL, or search result.
- `source="auto"` chooses a source from `subcategory` for search or from the identifier for fetch. Dataset subcategories: `expression` → GEO, `sequencing` → ENA run, `genomics` → ENA assembly, `sample_metadata` → ENA sample, `multi_omics` → cBioPortal.
- `params` may carry database-specific filters; GTEx accepts `endpoint` (`gene`, `median_expression`, `tissues`, `top_expressed`), `datasetId`, `gencodeId`, and `tissueSiteDetailId`; NCBI Gene accepts `organism`, `taxon_id`, `ret_start`, and `sort`; UniProt accepts `organism`, `taxon_id`, and `reviewed`.
- `search`/`fetch` remain compatibility wrappers for discovery/detail flows; new structured dataset/database integrations should be added here one source at a time."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryArgs {
    pub category: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default, alias = "subCategory", alias = "dataset_type", alias = "type")]
    pub subcategory: Option<String>,
    #[serde(default, alias = "q")]
    pub query: Option<String>,
    #[serde(default, alias = "accession")]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub result: Option<JsonValue>,
    #[serde(default)]
    pub params: Option<JsonValue>,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
}

pub struct QueryTool;

#[async_trait]
impl super::ToolImpl for QueryTool {
    type Args = QueryArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let category = common::normalized_category(&args.category);
        match category.as_str() {
            "data" => dataset::query_dataset(ctx, &args).await,
            "knowledge" => knowledge::query_knowledge(ctx, &args).await,
            other => Err(ToolError::InvalidArguments {
                message: format!(
                    "Unsupported query category: {other}. Supported categories: dataset/data, knowledge."
                ),
            }),
        }
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "query",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Database category. Supports dataset (alias: data) and knowledge."
                },
                "source": {
                    "type": "string",
                    "description": "Database source. Dataset supports auto, geo, ena, ena_run, ena_experiment, ena_sample, ena_analysis, ena_assembly, ena_sequence, cbioportal, gtex. Knowledge supports ncbi_gene and uniprot."
                },
                "operation": {
                    "type": "string",
                    "description": "Operation to run: search/query for record search; fetch/get/detail for one accession or URL. Defaults from supplied fields."
                },
                "subcategory": {
                    "type": "string",
                    "description": "Dataset routing hint: expression, sequencing, genomics, sample_metadata, multi_omics."
                },
                "query": {
                    "type": "string",
                    "description": "Keyword or database query string for operation=search/query."
                },
                "id": {
                    "type": "string",
                    "description": "Source-specific identifier/accession for operation=fetch/get/detail."
                },
                "url": {
                    "type": "string",
                    "description": "Source-specific record URL for operation=fetch/get/detail."
                },
                "result": {
                    "type": "object",
                    "description": "A result object returned by search/query; query will read source, id, accession, link/url, and metadata."
                },
                "params": {
                    "type": "object",
                    "description": "Database-specific structured parameters. Dataset sources accept query/q, id/accession/url, source, operation, subcategory, and max_results/limit. GTEx accepts endpoint/mode, datasetId, gencodeId, tissueSiteDetailId, and filterMtGene. NCBI Gene accepts organism, taxon_id/taxid, ret_start/retstart, and sort. UniProt accepts organism, taxon_id/taxid, and reviewed."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 25,
                    "description": "Maximum records for search/query operations."
                }
            },
            "required": ["category"]
        }),
    )
}
