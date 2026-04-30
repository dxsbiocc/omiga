//! GTEx Portal API v2 adapter.

use super::common;
use super::common::*;
use super::PublicDataClient;
use serde_json::Value as Json;

#[cfg(debug_assertions)]
mod mock;
mod params;
mod parser;

impl PublicDataClient {
    pub(super) async fn search_gtex(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let mode = params::gtex_mode(&args).unwrap_or_else(|| "gene".to_string());
        match mode.as_str() {
            "gene" | "genes" | "reference" | "reference_gene" => {
                self.search_gtex_genes(args).await
            }
            "expression" | "median_expression" | "median_gene_expression" => {
                self.search_gtex_median_expression(args).await
            }
            "tissue" | "tissues" | "tissue_site" | "tissue_site_detail" => {
                self.search_gtex_tissues(args).await
            }
            "top_expressed" | "top_expressed_gene" | "top_genes" => {
                self.search_gtex_top_expressed(args).await
            }
            other => Err(format!(
                "Unsupported GTEx endpoint `{other}`. Use gene, median_expression, tissues, or top_expressed."
            )),
        }
    }

    pub(super) async fn fetch_gtex(&self, identifier: &str) -> Result<DataRecord, String> {
        let gene_id = params::normalize_gtex_identifier(identifier).ok_or_else(|| {
            "GTEx fetch requires a gene symbol, GENCODE ID, or GTEx gene URL".to_string()
        })?;
        self.gtex_gene_records(&gene_id, None, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| format!("GTEx did not return a gene record for `{gene_id}`"))
    }

    pub(super) async fn search_gtex_genes(
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

    pub(super) async fn search_gtex_median_expression(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let gencode_id =
            params::gtex_param_string(&args, &["gencode_id", "gencodeId", "gene_id", "geneId"])
                .or_else(|| {
                    params::looks_like_gencode_id(args.query.trim())
                        .then(|| args.query.trim().to_string())
                })
                .or_else(|| {
                    args.query
                        .split_whitespace()
                        .find(|part| params::looks_like_gencode_id(part))
                        .map(|part| {
                            part.trim_matches(|c: char| c == ',' || c == ';')
                                .to_string()
                        })
                });
        let gencode_id = if let Some(gencode_id) = gencode_id {
            gencode_id
        } else {
            self.gtex_gene_records(args.query.trim(), args.params.as_ref(), 1)
                .await?
                .into_iter()
                .next()
                .map(|record| record.accession)
                .ok_or_else(|| {
                    format!(
                        "GTEx could not resolve `{}` to a versioned GENCODE ID",
                        args.query.trim()
                    )
                })?
        };

        let limit = args.normalized_max_results();
        let mut params = vec![
            ("gencodeId".to_string(), gencode_id.clone()),
            (
                "datasetId".to_string(),
                params::gtex_dataset_id(&args).unwrap_or_else(|| "gtex_v8".to_string()),
            ),
            ("page".to_string(), params::gtex_page(&args).to_string()),
            ("itemsPerPage".to_string(), limit.to_string()),
        ];
        for tissue in params::gtex_param_list(
            &args,
            &[
                "tissueSiteDetailId",
                "tissue_site_detail_id",
                "tissue",
                "tissues",
            ],
        ) {
            params.push(("tissueSiteDetailId".to_string(), tissue));
        }
        let json = self
            .gtex_get_json("expression/medianGeneExpression", &params)
            .await?;
        let results = parser::parse_gtex_median_expression_json(&json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "gtex".to_string(),
            total: parser::gtex_total(&json).or(Some(results.len() as u64)),
            results,
            notes: vec![
                "GTEx Portal API v2 /expression/medianGeneExpression".to_string(),
                "GTEx expression queries work best with versioned GENCODE IDs; symbols are resolved through /reference/gene.".to_string(),
            ],
        })
    }

    pub(super) async fn search_gtex_tissues(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let params = vec![
            (
                "datasetId".to_string(),
                params::gtex_dataset_id(&args).unwrap_or_else(|| "gtex_v8".to_string()),
            ),
            ("page".to_string(), params::gtex_page(&args).to_string()),
            ("itemsPerPage".to_string(), limit.to_string()),
        ];
        let json = self
            .gtex_get_json("dataset/tissueSiteDetail", &params)
            .await?;
        let query = args.query.trim().to_ascii_lowercase();
        let mut results = parser::parse_gtex_tissues_json(&json)
            .into_iter()
            .filter(|record| {
                query.is_empty()
                    || record.id.to_ascii_lowercase().contains(&query)
                    || record.title.to_ascii_lowercase().contains(&query)
                    || record.summary.to_ascii_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        results.truncate(limit as usize);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "gtex".to_string(),
            total: parser::gtex_total(&json).or(Some(results.len() as u64)),
            results,
            notes: vec!["GTEx Portal API v2 /dataset/tissueSiteDetail".to_string()],
        })
    }

    pub(super) async fn search_gtex_top_expressed(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let tissue = params::gtex_param_string(
            &args,
            &["tissueSiteDetailId", "tissue_site_detail_id", "tissue"],
        )
        .unwrap_or_else(|| args.query.trim().to_string());
        if tissue.trim().is_empty() {
            return Err(
                "GTEx top_expressed requires params.tissueSiteDetailId or query=tissue id"
                    .to_string(),
            );
        }
        let limit = args.normalized_max_results();
        let mut params = vec![
            ("tissueSiteDetailId".to_string(), tissue.clone()),
            (
                "datasetId".to_string(),
                params::gtex_dataset_id(&args).unwrap_or_else(|| "gtex_v8".to_string()),
            ),
            ("page".to_string(), params::gtex_page(&args).to_string()),
            ("itemsPerPage".to_string(), limit.to_string()),
        ];
        if let Some(filter_mt_gene) =
            params::gtex_param_bool(&args, &["filterMtGene", "filter_mt_gene"])
        {
            params.push(("filterMtGene".to_string(), filter_mt_gene.to_string()));
        }
        let json = self
            .gtex_get_json("expression/topExpressedGene", &params)
            .await?;
        let results = parser::parse_gtex_top_expressed_json(&json, &tissue);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "gtex".to_string(),
            total: parser::gtex_total(&json).or(Some(results.len() as u64)),
            results,
            notes: vec!["GTEx Portal API v2 /expression/topExpressedGene".to_string()],
        })
    }

    async fn gtex_gene_records(
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

    async fn gtex_get_json(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        #[cfg(debug_assertions)]
        if self.base_urls.gtex == "mock://gtex" {
            return mock::mock_gtex_json(endpoint, params).ok_or_else(|| {
                format!(
                    "debug GTEx mock has no fixture for endpoint `{endpoint}` with params {:?}",
                    params
                )
            });
        }

        let response = self
            .http
            .get(format!(
                "{}/{}",
                self.base_urls.gtex,
                endpoint.trim_start_matches('/')
            ))
            .query(params)
            .send()
            .await
            .map_err(|e| format!("GTEx Portal API {endpoint} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read GTEx Portal API {endpoint} response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "GTEx Portal API {endpoint} returned HTTP {}: {}",
                status.as_u16(),
                common::truncate_for_error(&body)
            ));
        }
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "parse GTEx Portal API {endpoint} JSON: {e}; body: {}",
                common::truncate_for_error(&body)
            )
        })
    }
}

pub fn looks_like_gtex_identifier(value: &str) -> bool {
    params::looks_like_gtex_identifier(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_gtex_gene_and_expression_json() {
        assert_eq!(
            PublicDataSource::parse("gtex"),
            Some(PublicDataSource::Gtex)
        );

        let genes = json!({
            "data": [{
                "gencodeId": "ENSG00000012048.21",
                "geneSymbol": "BRCA1",
                "description": "BRCA1 DNA repair associated",
                "geneType": "protein_coding",
                "chromosome": "chr17",
                "start": 43044295,
                "end": 43125482
            }],
            "paging_info": {"totalNumberOfItems": 1}
        });
        let records = parser::parse_gtex_genes_json(&genes);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, PublicDataSource::Gtex);
        assert_eq!(records[0].accession, "ENSG00000012048.21");
        assert!(records[0].title.contains("BRCA1"));
        assert!(records[0].url.ends_with("/ENSG00000012048.21"));

        let expression = json!({
            "data": [{
                "gencodeId": "ENSG00000012048.21",
                "geneSymbol": "BRCA1",
                "tissueSiteDetailId": "Whole_Blood",
                "median": 1.23,
                "unit": "TPM"
            }]
        });
        let expression_records = parser::parse_gtex_median_expression_json(&expression);
        assert_eq!(expression_records.len(), 1);
        assert_eq!(
            expression_records[0].record_type.as_deref(),
            Some("median_gene_expression")
        );
        assert!(expression_records[0].summary.contains("1.23 TPM"));
        assert!(looks_like_gtex_identifier(
            "https://gtexportal.org/home/gene/ENSG00000012048.21"
        ));
    }
}
