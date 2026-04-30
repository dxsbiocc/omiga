//! GTEx Portal API v2 adapter.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

impl PublicDataClient {
    pub(super) async fn search_gtex(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let mode = gtex_mode(&args).unwrap_or_else(|| "gene".to_string());
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
        let gene_id = normalize_gtex_identifier(identifier).ok_or_else(|| {
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
            gtex_param_string(&args, &["gencode_id", "gencodeId", "gene_id", "geneId"])
                .or_else(|| {
                    looks_like_gencode_id(args.query.trim()).then(|| args.query.trim().to_string())
                })
                .or_else(|| {
                    args.query
                        .split_whitespace()
                        .find(|part| looks_like_gencode_id(part))
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
                gtex_dataset_id(&args).unwrap_or_else(|| "gtex_v8".to_string()),
            ),
            ("page".to_string(), gtex_page(&args).to_string()),
            ("itemsPerPage".to_string(), limit.to_string()),
        ];
        for tissue in gtex_param_list(
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
        let results = parse_gtex_median_expression_json(&json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "gtex".to_string(),
            total: gtex_total(&json).or(Some(results.len() as u64)),
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
                gtex_dataset_id(&args).unwrap_or_else(|| "gtex_v8".to_string()),
            ),
            ("page".to_string(), gtex_page(&args).to_string()),
            ("itemsPerPage".to_string(), limit.to_string()),
        ];
        let json = self
            .gtex_get_json("dataset/tissueSiteDetail", &params)
            .await?;
        let query = args.query.trim().to_ascii_lowercase();
        let mut results = parse_gtex_tissues_json(&json)
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
            total: gtex_total(&json).or(Some(results.len() as u64)),
            results,
            notes: vec!["GTEx Portal API v2 /dataset/tissueSiteDetail".to_string()],
        })
    }

    pub(super) async fn search_gtex_top_expressed(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let tissue = gtex_param_string(
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
                gtex_dataset_id(&args).unwrap_or_else(|| "gtex_v8".to_string()),
            ),
            ("page".to_string(), gtex_page(&args).to_string()),
            ("itemsPerPage".to_string(), limit.to_string()),
        ];
        if let Some(filter_mt_gene) = gtex_param_bool(&args, &["filterMtGene", "filter_mt_gene"]) {
            params.push(("filterMtGene".to_string(), filter_mt_gene.to_string()));
        }
        let json = self
            .gtex_get_json("expression/topExpressedGene", &params)
            .await?;
        let results = parse_gtex_top_expressed_json(&json, &tissue);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "gtex".to_string(),
            total: gtex_total(&json).or(Some(results.len() as u64)),
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
        let gene_id = normalize_gtex_identifier(gene_id)
            .ok_or_else(|| "GTEx gene search requires a gene symbol or GENCODE ID".to_string())?;
        let mut params = vec![
            ("geneId".to_string(), gene_id.clone()),
            (
                "gencodeVersion".to_string(),
                gtex_param_string_from_value(params_value, &["gencodeVersion", "gencode_version"])
                    .unwrap_or_else(|| "v26".to_string()),
            ),
            (
                "genomeBuild".to_string(),
                gtex_param_string_from_value(params_value, &["genomeBuild", "genome_build"])
                    .unwrap_or_else(|| "GRCh38/hg38".to_string()),
            ),
            (
                "page".to_string(),
                gtex_param_u32_from_value(params_value, &["page"])
                    .unwrap_or(0)
                    .to_string(),
            ),
            (
                "itemsPerPage".to_string(),
                limit.clamp(1, MAX_RESULTS_CAP).to_string(),
            ),
        ];
        if let Some(dataset_id) =
            gtex_param_string_from_value(params_value, &["datasetId", "dataset_id"])
        {
            params.push(("datasetId".to_string(), dataset_id));
        }
        let json = self.gtex_get_json("reference/gene", &params).await?;
        Ok(parse_gtex_genes_json(&json))
    }

    async fn gtex_get_json(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        #[cfg(debug_assertions)]
        if self.base_urls.gtex == "mock://gtex" {
            return mock_gtex_json(endpoint, params).ok_or_else(|| {
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
                truncate_for_error(&body)
            ));
        }
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "parse GTEx Portal API {endpoint} JSON: {e}; body: {}",
                truncate_for_error(&body)
            )
        })
    }
}

fn parse_gtex_genes_json(value: &Json) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(parse_gtex_gene_item)
        .collect()
}

fn parse_gtex_gene_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let gencode_id = string_field_any(map, &["gencodeId", "gencode_id", "geneId", "gene_id"])?;
    let gene_symbol =
        string_field_any(map, &["geneSymbol", "gene_symbol"]).unwrap_or_else(|| gencode_id.clone());
    let description =
        string_field_any(map, &["description", "geneDescription"]).unwrap_or_default();
    let gene_type = string_field_any(map, &["geneType", "gene_type"]);
    let chromosome = string_field_any(map, &["chromosome"]);
    let start = json_u64_from_keys(map, &["start"]);
    let end = json_u64_from_keys(map, &["end"]);
    let title = if gene_symbol == gencode_id {
        gencode_id.clone()
    } else {
        format!("{gene_symbol} ({gencode_id})")
    };
    let mut summary_parts = Vec::new();
    if let Some(gene_type) = gene_type.as_deref().filter(|s| !s.is_empty()) {
        summary_parts.push(gene_type.to_string());
    }
    if let Some(chromosome) = chromosome.as_deref().filter(|s| !s.is_empty()) {
        let location = match (start, end) {
            (Some(start), Some(end)) => format!("{chromosome}:{start}-{end}"),
            _ => chromosome.to_string(),
        };
        summary_parts.push(location);
    }
    if !description.trim().is_empty() {
        summary_parts.push(description.clone());
    }

    Some(DataRecord {
        id: gencode_id.clone(),
        accession: gencode_id.clone(),
        source: PublicDataSource::Gtex,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary_parts.join(" | ")),
        url: gtex_gene_url(&gencode_id),
        record_type: Some("gene".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

fn parse_gtex_median_expression_json(value: &Json) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(parse_gtex_median_expression_item)
        .collect()
}

fn parse_gtex_median_expression_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let gencode_id = string_field_any(map, &["gencodeId", "gencode_id"])?;
    let gene_symbol =
        string_field_any(map, &["geneSymbol", "gene_symbol"]).unwrap_or_else(|| gencode_id.clone());
    let tissue = string_field_any(map, &["tissueSiteDetailId", "tissue_site_detail_id"])
        .unwrap_or_else(|| "all_tissues".to_string());
    let median = json_number_string(map.get("median")?).unwrap_or_else(|| "NA".to_string());
    let unit = string_field_any(map, &["unit"]).unwrap_or_else(|| "TPM".to_string());
    Some(DataRecord {
        id: format!("{gencode_id}:{tissue}"),
        accession: gencode_id.clone(),
        source: PublicDataSource::Gtex,
        title: format!("{gene_symbol} median expression in {tissue}"),
        summary: format!("median {median} {unit}"),
        url: gtex_gene_url(&gencode_id),
        record_type: Some("median_gene_expression".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: Some(unit),
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

fn parse_gtex_tissues_json(value: &Json) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(parse_gtex_tissue_item)
        .collect()
}

fn parse_gtex_tissue_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let tissue_id = string_field_any(map, &["tissueSiteDetailId", "tissue_site_detail_id"])?;
    let tissue_name = string_field_any(map, &["tissueSiteDetail", "tissue_site_detail"])
        .unwrap_or_else(|| tissue_id.clone());
    let tissue_site = string_field_any(map, &["tissueSite", "tissue_site"]);
    let sample_count = json_u64_from_keys(
        map,
        &[
            "rnaSeqSampleCount",
            "rna_seq_sample_count",
            "sampleCount",
            "sample_count",
        ],
    );
    Some(DataRecord {
        id: tissue_id.clone(),
        accession: tissue_id.clone(),
        source: PublicDataSource::Gtex,
        title: tissue_name,
        summary: tissue_site.unwrap_or_else(|| "GTEx tissue".to_string()),
        url: gtex_tissue_url(&tissue_id),
        record_type: Some("tissue".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count,
        platform: None,
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

fn parse_gtex_top_expressed_json(value: &Json, tissue: &str) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(|item| parse_gtex_top_expressed_item(item, tissue))
        .collect()
}

fn parse_gtex_top_expressed_item(item: &Json, fallback_tissue: &str) -> Option<DataRecord> {
    let map = item.as_object()?;
    let gencode_id = string_field_any(map, &["gencodeId", "gencode_id", "geneId", "gene_id"])?;
    let gene_symbol =
        string_field_any(map, &["geneSymbol", "gene_symbol"]).unwrap_or_else(|| gencode_id.clone());
    let tissue = string_field_any(map, &["tissueSiteDetailId", "tissue_site_detail_id"])
        .unwrap_or_else(|| fallback_tissue.to_string());
    let median = map
        .get("median")
        .and_then(json_number_string)
        .unwrap_or_else(|| "NA".to_string());
    let unit = string_field_any(map, &["unit"]).unwrap_or_else(|| "TPM".to_string());
    Some(DataRecord {
        id: format!("{gencode_id}:{tissue}"),
        accession: gencode_id.clone(),
        source: PublicDataSource::Gtex,
        title: format!("{gene_symbol} top expression in {tissue}"),
        summary: format!("median {median} {unit}"),
        url: gtex_gene_url(&gencode_id),
        record_type: Some("top_expressed_gene".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: Some(unit),
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

#[cfg(debug_assertions)]
fn mock_gtex_json(endpoint: &str, params: &[(String, String)]) -> Option<Json> {
    let endpoint = endpoint.trim_start_matches('/');
    let param = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    match endpoint {
        "reference/gene" => {
            let gene = param("geneId").unwrap_or("BRCA1");
            Some(json!({
                "data": [{
                    "gencodeId": if looks_like_gencode_id(gene) { gene } else { "ENSG00000012048.21" },
                    "geneSymbol": if looks_like_gencode_id(gene) { "BRCA1" } else { gene },
                    "description": "BRCA1 DNA repair associated",
                    "geneType": "protein_coding",
                    "chromosome": "chr17",
                    "start": 43044295,
                    "end": 43125482
                }],
                "paging_info": {"totalNumberOfItems": 1}
            }))
        }
        "expression/medianGeneExpression" => {
            let gene = param("gencodeId").unwrap_or("ENSG00000012048.21");
            let tissue = param("tissueSiteDetailId").unwrap_or("Whole_Blood");
            Some(json!({
                "data": [{
                    "gencodeId": gene,
                    "geneSymbol": "BRCA1",
                    "tissueSiteDetailId": tissue,
                    "median": 1.23,
                    "unit": "TPM"
                }],
                "paging_info": {"totalNumberOfItems": 1}
            }))
        }
        _ => None,
    }
}

fn gtex_mode(args: &DataSearchArgs) -> Option<String> {
    gtex_param_string(args, &["endpoint", "mode", "kind", "type"])
        .map(|value| value.trim().to_ascii_lowercase().replace(['-', ' '], "_"))
}

fn gtex_dataset_id(args: &DataSearchArgs) -> Option<String> {
    gtex_param_string(args, &["datasetId", "dataset_id", "dataset"])
}

fn gtex_page(args: &DataSearchArgs) -> u32 {
    gtex_param_u32(args, &["page"]).unwrap_or(0)
}

fn gtex_param_string(args: &DataSearchArgs, keys: &[&str]) -> Option<String> {
    gtex_param_string_from_value(args.params.as_ref(), keys)
}

fn gtex_param_string_from_value(value: Option<&Json>, keys: &[&str]) -> Option<String> {
    let map = value?.as_object()?;
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn gtex_param_u32(args: &DataSearchArgs, keys: &[&str]) -> Option<u32> {
    gtex_param_u32_from_value(args.params.as_ref(), keys)
}

fn gtex_param_u32_from_value(value: Option<&Json>, keys: &[&str]) -> Option<u32> {
    let map = value?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<u32>().ok())
    })
}

fn gtex_param_bool(args: &DataSearchArgs, keys: &[&str]) -> Option<bool> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value.as_bool().or_else(
            || match value.as_str()?.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            },
        )
    })
}

fn gtex_param_list(args: &DataSearchArgs, keys: &[&str]) -> Vec<String> {
    let Some(map) = args.params.as_ref().and_then(Json::as_object) else {
        return Vec::new();
    };
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            return items
                .iter()
                .filter_map(json_string)
                .flat_map(|item| split_csv_like(&item))
                .collect();
        }
        if let Some(value) = json_string(value) {
            return split_csv_like(&value);
        }
    }
    Vec::new()
}

fn split_csv_like(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn gtex_data_items(value: &Json) -> Vec<&Json> {
    if let Some(items) = value.get("data").and_then(Json::as_array) {
        return items.iter().collect();
    }
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    Vec::new()
}

fn gtex_total(value: &Json) -> Option<u64> {
    let paging = value
        .get("paging_info")
        .or_else(|| value.get("pagingInfo"))
        .and_then(Json::as_object)?;
    json_u64_from_keys(
        paging,
        &["totalNumberOfItems", "total_number_of_items", "total"],
    )
}

fn gtex_extra_from_map(map: &JsonMap<String, Json>) -> JsonMap<String, Json> {
    let mut extra = JsonMap::new();
    for key in [
        "chromosome",
        "dataSource",
        "description",
        "end",
        "entrezGeneId",
        "gencodeId",
        "gencodeVersion",
        "geneStatus",
        "geneSymbol",
        "geneSymbolUpper",
        "geneType",
        "genomeBuild",
        "median",
        "ontologyId",
        "start",
        "strand",
        "tissueSite",
        "tissueSiteDetail",
        "tissueSiteDetailId",
        "tss",
        "unit",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    extra
}

fn normalize_gtex_identifier(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("gtexportal.org") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty())
                .map(str::to_string);
        }
    }
    Some(value.to_string())
}

fn looks_like_gencode_id(value: &str) -> bool {
    let value = value.trim_matches(|c: char| c == ',' || c == ';');
    value.to_ascii_uppercase().starts_with("ENSG")
}

fn gtex_gene_url(gencode_id: &str) -> String {
    format!("https://gtexportal.org/home/gene/{gencode_id}")
}

fn gtex_tissue_url(tissue_id: &str) -> String {
    format!("https://gtexportal.org/home/tissue/{tissue_id}")
}

pub fn looks_like_gtex_identifier(value: &str) -> bool {
    let Some(identifier) = normalize_gtex_identifier(value) else {
        return false;
    };
    looks_like_gencode_id(&identifier)
        || value
            .to_ascii_lowercase()
            .contains("gtexportal.org/home/gene/")
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
        let records = parse_gtex_genes_json(&genes);
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
        let expression_records = parse_gtex_median_expression_json(&expression);
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
