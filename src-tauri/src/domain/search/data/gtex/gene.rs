//! GTEx reference gene search and fetch operations.

use super::super::common::*;
use super::super::PublicDataClient;
use super::{params, parser};
use serde_json::Value as Json;

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn fetch_gtex(
        &self,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let gene_id = params::normalize_gtex_identifier(identifier).ok_or_else(|| {
            "GTEx fetch requires a gene symbol, GENCODE ID, or GTEx gene URL".to_string()
        })?;
        self.gtex_gene_records(&gene_id, None, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| format!("GTEx did not return a gene record for `{gene_id}`"))
    }

    pub(in crate::domain::search::data::gtex) async fn search_gtex_genes(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let results = self
            .gtex_gene_records(args.query.trim(), args.params.as_ref(), limit)
            .await?;
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "gtex".to_string(),
            total: Some(results.len() as u64),
            results,
            notes: vec![
                "GTEx Portal API v2 /reference/gene".to_string(),
                "Use params.endpoint=median_expression with a versioned GENCODE ID to retrieve median TPM by tissue.".to_string(),
            ],
        })
    }

    pub(in crate::domain::search::data::gtex) async fn gtex_gene_records(
        &self,
        gene_id: &str,
        params_value: Option<&Json>,
        limit: u32,
    ) -> Result<Vec<DataRecord>, String> {
        let gene_id = params::normalize_gtex_identifier(gene_id)
            .ok_or_else(|| "GTEx gene search requires a gene symbol or GENCODE ID".to_string())?;
        let mut params = vec![
            ("geneId".to_string(), gene_id.clone()),
            (
                "gencodeVersion".to_string(),
                params::gtex_param_string_from_value(
                    params_value,
                    &["gencodeVersion", "gencode_version"],
                )
                .unwrap_or_else(|| "v26".to_string()),
            ),
            (
                "genomeBuild".to_string(),
                params::gtex_param_string_from_value(
                    params_value,
                    &["genomeBuild", "genome_build"],
                )
                .unwrap_or_else(|| "GRCh38/hg38".to_string()),
            ),
            (
                "page".to_string(),
                params::gtex_param_u32_from_value(params_value, &["page"])
                    .unwrap_or(0)
                    .to_string(),
            ),
            (
                "itemsPerPage".to_string(),
                limit.clamp(1, MAX_RESULTS_CAP).to_string(),
            ),
        ];
        if let Some(dataset_id) =
            params::gtex_param_string_from_value(params_value, &["datasetId", "dataset_id"])
        {
            params.push(("datasetId".to_string(), dataset_id));
        }
        let json = self.gtex_get_json("reference/gene", &params).await?;
        Ok(parser::parse_gtex_genes_json(&json))
    }
}
