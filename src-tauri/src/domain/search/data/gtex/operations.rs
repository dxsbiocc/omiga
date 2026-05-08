//! GTEx endpoint execution for the PublicDataClient adapter.

use super::super::common::*;
use super::super::PublicDataClient;
use super::{params, parser};

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_gtex(
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
}
