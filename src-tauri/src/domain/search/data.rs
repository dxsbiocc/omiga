//! Public biological data-source adapters used by the built-in `search` / `fetch` tools.
//!
//! GEO is backed by official NCBI Entrez E-utilities (`db=gds`). ENA uses the
//! official ENA Portal API for indexed record searches and the Browser API XML
//! endpoint as a detail fallback. cBioPortal uses the public REST API for
//! cancer genomics study discovery/detail. GTEx uses the public GTEx Portal API
//! v2 for gene/tissue/expression metadata.

use crate::domain::tools::{ToolContext, WebSearchApiKeys};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map as JsonMap, Value as Json};
use std::time::Duration;

const DEFAULT_EUTILS_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const ENA_PORTAL_SEARCH_URL: &str = "https://www.ebi.ac.uk/ena/portal/api/search";
const ENA_BROWSER_XML_BASE_URL: &str = "https://www.ebi.ac.uk/ena/browser/api/xml";
const CBIOPORTAL_API_BASE_URL: &str = "https://www.cbioportal.org/api";
const GTEX_API_BASE_URL: &str = "https://gtexportal.org/api/v2";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const DEFAULT_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_TOOL: &str = "omiga";
const GEO_FAVICON: &str = "https://www.ncbi.nlm.nih.gov/favicon.ico";
const ENA_FAVICON: &str = "https://www.ebi.ac.uk/favicon.ico";
const CBIOPORTAL_FAVICON: &str = "https://www.cbioportal.org/favicon.ico";
const GTEX_FAVICON: &str = "https://gtexportal.org/favicon.ico";

#[derive(Clone, Debug)]
pub struct DataApiBaseUrls {
    pub entrez: String,
    pub ena_portal_search: String,
    pub ena_browser_xml: String,
    pub cbioportal: String,
    pub gtex: String,
}

impl Default for DataApiBaseUrls {
    fn default() -> Self {
        Self {
            entrez: DEFAULT_EUTILS_BASE_URL.to_string(),
            ena_portal_search: ENA_PORTAL_SEARCH_URL.to_string(),
            ena_browser_xml: ENA_BROWSER_XML_BASE_URL.to_string(),
            cbioportal: CBIOPORTAL_API_BASE_URL.to_string(),
            gtex: GTEX_API_BASE_URL.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicDataSource {
    Geo,
    EnaStudy,
    EnaRun,
    EnaExperiment,
    EnaSample,
    EnaAnalysis,
    EnaAssembly,
    EnaSequence,
    CbioPortal,
    Gtex,
}

impl PublicDataSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "geo" | "gds" | "ncbi_geo" | "ncbi_gds" => Some(Self::Geo),
            "ena" | "ena_study" | "study" | "read_study" | "european_nucleotide_archive" => {
                Some(Self::EnaStudy)
            }
            "ena_run" | "run" | "read_run" => Some(Self::EnaRun),
            "ena_experiment" | "experiment" | "read_experiment" => Some(Self::EnaExperiment),
            "ena_sample" | "sample" | "read_sample" => Some(Self::EnaSample),
            "ena_analysis" | "analysis" => Some(Self::EnaAnalysis),
            "ena_assembly" | "assembly" => Some(Self::EnaAssembly),
            "ena_sequence" | "sequence" => Some(Self::EnaSequence),
            "cbioportal" | "cbio_portal" | "cbio" | "cancer_genomics" | "multi_omics"
            | "multiomics" | "projects" | "tcga" => Some(Self::CbioPortal),
            "gtex" | "genotype_tissue_expression" | "tissue_expression" => Some(Self::Gtex),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Geo => "geo",
            Self::EnaStudy => "ena",
            Self::EnaRun => "ena_run",
            Self::EnaExperiment => "ena_experiment",
            Self::EnaSample => "ena_sample",
            Self::EnaAnalysis => "ena_analysis",
            Self::EnaAssembly => "ena_assembly",
            Self::EnaSequence => "ena_sequence",
            Self::CbioPortal => "cbioportal",
            Self::Gtex => "gtex",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Geo => "NCBI GEO DataSets",
            Self::EnaStudy => "ENA read studies",
            Self::EnaRun => "ENA raw read runs",
            Self::EnaExperiment => "ENA read experiments",
            Self::EnaSample => "ENA samples",
            Self::EnaAnalysis => "ENA analyses",
            Self::EnaAssembly => "ENA assemblies",
            Self::EnaSequence => "ENA nucleotide sequences",
            Self::CbioPortal => "cBioPortal cancer genomics studies",
            Self::Gtex => "GTEx tissue expression",
        }
    }

    fn favicon(self) -> &'static str {
        match self {
            Self::Geo => GEO_FAVICON,
            Self::EnaStudy
            | Self::EnaRun
            | Self::EnaExperiment
            | Self::EnaSample
            | Self::EnaAnalysis
            | Self::EnaAssembly
            | Self::EnaSequence => ENA_FAVICON,
            Self::CbioPortal => CBIOPORTAL_FAVICON,
            Self::Gtex => GTEX_FAVICON,
        }
    }

    fn ena_result(self) -> Option<&'static str> {
        match self {
            Self::Geo => None,
            Self::EnaStudy => Some("read_study"),
            Self::EnaRun => Some("read_run"),
            Self::EnaExperiment => Some("read_experiment"),
            Self::EnaSample => Some("sample"),
            Self::EnaAnalysis => Some("analysis"),
            Self::EnaAssembly => Some("assembly"),
            Self::EnaSequence => Some("sequence"),
            Self::CbioPortal | Self::Gtex => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DataSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub params: Option<Json>,
}

impl DataSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRecord {
    pub id: String,
    pub accession: String,
    pub source: PublicDataSource,
    pub title: String,
    pub summary: String,
    pub url: String,
    pub record_type: Option<String>,
    pub organism: Option<String>,
    pub published_date: Option<String>,
    pub updated_date: Option<String>,
    pub sample_count: Option<u64>,
    pub platform: Option<String>,
    pub files: Vec<String>,
    pub extra: JsonMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSearchResponse {
    pub query: String,
    pub source: String,
    pub total: Option<u64>,
    pub results: Vec<DataRecord>,
    pub notes: Vec<String>,
}

#[derive(Clone)]
pub struct PublicDataClient {
    http: reqwest::Client,
    base_urls: DataApiBaseUrls,
    settings: EntrezSettings,
}

#[derive(Clone, Debug)]
struct EntrezSettings {
    api_key: Option<String>,
    email: String,
    tool: String,
}

impl EntrezSettings {
    fn from_keys(keys: &WebSearchApiKeys) -> Self {
        Self {
            api_key: clean_optional(&keys.pubmed_api_key),
            email: clean_optional(&keys.pubmed_email).unwrap_or_else(|| DEFAULT_EMAIL.to_string()),
            tool: clean_optional(&keys.pubmed_tool_name)
                .unwrap_or_else(|| DEFAULT_TOOL.to_string()),
        }
    }
}

impl PublicDataClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-DataSearch/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build data-source HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_urls: ctx.data_api_base_urls.clone(),
            settings: EntrezSettings::from_keys(&ctx.web_search_api_keys),
        })
    }

    pub async fn search(
        &self,
        source: PublicDataSource,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        match source {
            PublicDataSource::Geo => self.search_geo(args).await,
            PublicDataSource::CbioPortal => self.search_cbioportal(args).await,
            PublicDataSource::Gtex => self.search_gtex(args).await,
            source => self.search_ena(source, args).await,
        }
    }

    pub async fn search_auto(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        let max_results = args.normalized_max_results() as usize;
        let geo_args = args.clone();
        let ena_args = args.clone();
        let gtex_args = args.clone();
        let (geo, ena, gtex) = tokio::join!(
            self.search_geo(geo_args),
            self.search_ena(PublicDataSource::EnaStudy, ena_args),
            self.search_gtex(gtex_args)
        );

        let mut results = Vec::new();
        let mut total = 0u64;
        let mut saw_total = false;
        let mut notes = vec!["Combined GEO + ENA + GTEx data search".to_string()];

        for response in [geo, ena, gtex] {
            match response {
                Ok(response) => {
                    if let Some(count) = response.total {
                        total = total.saturating_add(count);
                        saw_total = true;
                    }
                    notes.extend(response.notes);
                    results.extend(response.results);
                }
                Err(err) => notes.push(format!("source failed: {err}")),
            }
        }

        results.truncate(max_results);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "auto".to_string(),
            total: saw_total.then_some(total),
            results,
            notes,
        })
    }

    pub async fn fetch(
        &self,
        source: PublicDataSource,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(format!("{} fetch requires a non-empty id", source.as_str()));
        }
        match source {
            PublicDataSource::Geo => self.fetch_geo(identifier).await,
            PublicDataSource::CbioPortal => self.fetch_cbioportal(identifier).await,
            PublicDataSource::Gtex => self.fetch_gtex(identifier).await,
            source => self.fetch_ena(source, identifier).await,
        }
    }

    async fn search_geo(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        let ret_max = args.normalized_max_results();
        let mut params = self.geo_entrez_params("json");
        params.push(("term".to_string(), args.query.trim().to_string()));
        params.push(("retmax".to_string(), ret_max.to_string()));

        let search_json = self.get_entrez_json("esearch", &params).await?;
        let (count, ids, query_translation) = parse_geo_esearch(&search_json)?;
        if ids.is_empty() {
            return Ok(DataSearchResponse {
                query: args.query.trim().to_string(),
                source: "geo".to_string(),
                total: Some(count),
                results: Vec::new(),
                notes: vec![
                    "NCBI GEO DataSets ESearch returned no matching UIDs".to_string(),
                    query_translation
                        .map(|q| format!("NCBI query translation: {q}"))
                        .unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect(),
            });
        }

        let mut summary_params = self.geo_entrez_params("json");
        summary_params.push(("id".to_string(), ids.join(",")));
        let summary_json = self.get_entrez_json("esummary", &summary_params).await?;
        let results = parse_geo_esummary(&summary_json, &ids);
        let mut notes = vec!["NCBI Entrez E-utilities db=gds".to_string()];
        if let Some(q) = query_translation {
            notes.push(format!("NCBI query translation: {q}"));
        }
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "geo".to_string(),
            total: Some(count),
            results,
            notes,
        })
    }

    async fn fetch_geo(&self, identifier: &str) -> Result<DataRecord, String> {
        let uid = if identifier.chars().all(|c| c.is_ascii_digit()) {
            identifier.to_string()
        } else {
            let mut params = self.geo_entrez_params("json");
            params.push(("term".to_string(), format!("{}[ACCN]", identifier.trim())));
            params.push(("retmax".to_string(), "1".to_string()));
            let json = self.get_entrez_json("esearch", &params).await?;
            let (_, ids, _) = parse_geo_esearch(&json)?;
            ids.into_iter()
                .next()
                .ok_or_else(|| format!("GEO did not find accession `{identifier}`"))?
        };
        let mut params = self.geo_entrez_params("json");
        params.push(("id".to_string(), uid.clone()));
        let json = self.get_entrez_json("esummary", &params).await?;
        parse_geo_esummary(&json, std::slice::from_ref(&uid))
            .into_iter()
            .next()
            .ok_or_else(|| format!("GEO did not return a parseable summary for `{uid}`"))
    }

    async fn search_cbioportal(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let response = self
            .http
            .get(format!("{}/studies", self.base_urls.cbioportal))
            .query(&[
                ("projection", "SUMMARY".to_string()),
                ("keyword", args.query.trim().to_string()),
                ("pageNumber", "0".to_string()),
                ("pageSize", limit.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("cBioPortal studies search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read cBioPortal studies response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "cBioPortal studies search returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse cBioPortal JSON: {e}"))?;
        let results = parse_cbioportal_studies_json(&json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "cbioportal".to_string(),
            total: Some(results.len() as u64),
            results,
            notes: vec![
                "cBioPortal REST API /studies search".to_string(),
                "Search is limited to study-level metadata; use fetch(source=cbioportal) for a selected study.".to_string(),
            ],
        })
    }

    async fn fetch_cbioportal(&self, identifier: &str) -> Result<DataRecord, String> {
        let study_id = normalize_cbioportal_study_id(identifier)
            .ok_or_else(|| "cBioPortal fetch requires a study id or study URL".to_string())?;
        let response = self
            .http
            .get(format!(
                "{}/studies/{}",
                self.base_urls.cbioportal,
                encode_path_segment(&study_id)
            ))
            .query(&[("projection", "DETAILED")])
            .send()
            .await
            .map_err(|e| format!("cBioPortal study fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read cBioPortal study response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "cBioPortal study fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse cBioPortal JSON: {e}"))?;
        parse_cbioportal_study(&json)
            .ok_or_else(|| format!("cBioPortal did not return a parseable study for `{study_id}`"))
    }

    async fn search_gtex(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
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

    async fn fetch_gtex(&self, identifier: &str) -> Result<DataRecord, String> {
        let gene_id = normalize_gtex_identifier(identifier).ok_or_else(|| {
            "GTEx fetch requires a gene symbol, GENCODE ID, or GTEx gene URL".to_string()
        })?;
        self.gtex_gene_records(&gene_id, None, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| format!("GTEx did not return a gene record for `{gene_id}`"))
    }

    async fn search_gtex_genes(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
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

    async fn search_gtex_median_expression(
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

    async fn search_gtex_tissues(
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

    async fn search_gtex_top_expressed(
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

    async fn search_ena(
        &self,
        source: PublicDataSource,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let result = source
            .ena_result()
            .ok_or_else(|| "GEO is not an ENA source".to_string())?;
        let query = ena_portal_query(source, args.query.trim());
        let fields = ena_fields(source);
        let response = self
            .http
            .get(&self.base_urls.ena_portal_search)
            .query(&[
                ("result", result.to_string()),
                ("query", query),
                ("fields", fields),
                ("format", "json".to_string()),
                ("limit", limit.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("ENA Portal search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read ENA Portal search response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ENA Portal search returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse ENA Portal JSON: {e}"))?;
        let results = parse_ena_portal_json(source, &json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: source.as_str().to_string(),
            total: Some(results.len() as u64),
            results,
            notes: vec![
                format!("ENA Portal API {result} search"),
                "Simple free-text queries are translated to source-specific wildcard fields; advanced ENA query syntax is passed through.".to_string(),
            ],
        })
    }

    async fn fetch_ena(
        &self,
        source: PublicDataSource,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let accession = normalize_accession(identifier)
            .ok_or_else(|| "ENA fetch requires an accession or ENA Browser URL".to_string())?;
        let source = if matches!(source, PublicDataSource::EnaStudy) {
            infer_ena_source_from_accession(&accession).unwrap_or(source)
        } else {
            source
        };
        let result = source
            .ena_result()
            .ok_or_else(|| "GEO is not an ENA source".to_string())?;
        let query = ena_accession_query(source, &accession);
        let fields = ena_fields(source);
        let response = self
            .http
            .get(&self.base_urls.ena_portal_search)
            .query(&[
                ("result", result.to_string()),
                ("query", query),
                ("fields", fields),
                ("format", "json".to_string()),
                ("limit", "1".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("ENA Portal fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read ENA Portal fetch response: {e}"))?;
        if status.is_success() {
            let json: Json =
                serde_json::from_str(&body).map_err(|e| format!("parse ENA Portal JSON: {e}"))?;
            if let Some(record) = parse_ena_portal_json(source, &json).into_iter().next() {
                return Ok(record);
            }
        }

        let url = format!(
            "{}/{}",
            self.base_urls.ena_browser_xml,
            encode_path_segment(&accession)
        );
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("ENA Browser XML fetch request failed: {e}"))?;
        let status = response.status();
        let xml = response
            .text()
            .await
            .map_err(|e| format!("read ENA Browser XML response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ENA fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&xml)
            ));
        }
        parse_ena_xml_record(source, &xml, &accession)
            .ok_or_else(|| format!("ENA did not return a parseable record for `{accession}`"))
    }

    fn geo_entrez_params(&self, retmode: &str) -> Vec<(String, String)> {
        let mut params = vec![
            ("db".to_string(), "gds".to_string()),
            ("retmode".to_string(), retmode.to_string()),
            ("tool".to_string(), self.settings.tool.clone()),
            ("email".to_string(), self.settings.email.clone()),
        ];
        if let Some(api_key) = &self.settings.api_key {
            params.push(("api_key".to_string(), api_key.clone()));
        }
        params
    }

    async fn get_entrez_json(
        &self,
        utility: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        let url = format!("{}/{}.fcgi", self.base_urls.entrez, utility);
        let response = self
            .http
            .get(&url)
            .query(params)
            .send()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} response read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "NCBI Entrez {utility} returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }
        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "NCBI Entrez {utility} returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(error) = json.get("error").and_then(Json::as_str) {
            return Err(format!("NCBI Entrez {utility} error: {error}"));
        }
        Ok(json)
    }
}

pub fn search_response_to_json(response: &DataSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| record_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "data",
        "source": response.source,
        "effective_source": response.source,
        "total": response.total,
        "route_notes": response.notes,
        "results": results,
    })
}

pub fn detail_to_json(record: &DataRecord) -> Json {
    json!({
        "category": "data",
        "source": record.source.as_str(),
        "effective_source": record.source.as_str(),
        "id": record.id,
        "accession": record.accession,
        "title": record.title,
        "name": record.title,
        "link": record.url,
        "url": record.url,
        "displayed_link": displayed_link_for_url(&record.url),
        "favicon": record.source.favicon(),
        "snippet": data_record_snippet(record),
        "content": data_record_content(record),
        "metadata": record_metadata(record),
    })
}

fn record_to_serp_result(record: &DataRecord, position: usize) -> Json {
    json!({
        "position": position,
        "category": "data",
        "source": record.source.as_str(),
        "title": record.title,
        "name": record.title,
        "link": record.url,
        "url": record.url,
        "displayed_link": displayed_link_for_url(&record.url),
        "favicon": record.source.favicon(),
        "snippet": data_record_snippet(record),
        "id": record.id,
        "accession": record.accession,
        "metadata": record_metadata(record),
    })
}

fn record_metadata(record: &DataRecord) -> Json {
    json!({
        "accession": record.accession,
        "source_label": record.source.label(),
        "record_type": record.record_type,
        "organism": record.organism,
        "published_date": record.published_date,
        "updated_date": record.updated_date,
        "sample_count": record.sample_count,
        "platform": record.platform,
        "files": record.files,
        "source_specific": record.extra,
    })
}

fn data_record_snippet(record: &DataRecord) -> String {
    let mut pieces = Vec::new();
    if let Some(record_type) = record
        .record_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(record_type.to_string());
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(organism.to_string());
    }
    if let Some(samples) = record.sample_count {
        pieces.push(format!("{samples} samples"));
    }
    if !record.summary.trim().is_empty() {
        pieces.push(truncate_chars(&record.summary, 280));
    }
    pieces.join(" | ")
}

fn data_record_content(record: &DataRecord) -> String {
    let mut out = String::new();
    out.push_str(&record.title);
    out.push_str("\n\n");
    out.push_str("Source: ");
    out.push_str(record.source.label());
    out.push('\n');
    if !record.accession.trim().is_empty() {
        out.push_str("Accession: ");
        out.push_str(&record.accession);
        out.push('\n');
    }
    if let Some(record_type) = record
        .record_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Record type: ");
        out.push_str(record_type);
        out.push('\n');
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Organism: ");
        out.push_str(organism);
        out.push('\n');
    }
    if let Some(samples) = record.sample_count {
        out.push_str("Samples: ");
        out.push_str(&samples.to_string());
        out.push('\n');
    }
    if let Some(platform) = record.platform.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Platform: ");
        out.push_str(platform);
        out.push('\n');
    }
    if let Some(date) = record
        .published_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Published: ");
        out.push_str(date);
        out.push('\n');
    }
    out.push_str("Link: ");
    out.push_str(&record.url);
    if !record.files.is_empty() {
        out.push_str("\nFiles:\n");
        for file in &record.files {
            out.push_str("- ");
            out.push_str(file);
            out.push('\n');
        }
    }
    if !record.summary.trim().is_empty() {
        out.push_str("\nSummary\n");
        out.push_str(record.summary.trim());
    }
    out.trim().to_string()
}

fn parse_geo_esearch(value: &Json) -> Result<(u64, Vec<String>, Option<String>), String> {
    let root = value
        .get("esearchresult")
        .and_then(Json::as_object)
        .ok_or_else(|| "NCBI GEO ESearch response missing esearchresult".to_string())?;
    if let Some(error) = root.get("error").and_then(Json::as_str) {
        return Err(format!("NCBI GEO ESearch error: {error}"));
    }
    let count = root
        .get("count")
        .and_then(json_u64_from_string_or_number)
        .unwrap_or(0);
    let ids = root
        .get("idlist")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let query_translation = root
        .get("querytranslation")
        .and_then(Json::as_str)
        .map(str::to_string);
    Ok((count, ids, query_translation))
}

fn parse_geo_esummary(value: &Json, ordered_ids: &[String]) -> Vec<DataRecord> {
    let Some(result) = value.get("result").and_then(Json::as_object) else {
        return Vec::new();
    };
    ordered_ids
        .iter()
        .filter_map(|uid| result.get(uid).and_then(|doc| parse_geo_doc(uid, doc)))
        .collect()
}

fn parse_geo_doc(uid: &str, doc: &Json) -> Option<DataRecord> {
    let map = doc.as_object()?;
    let accession = string_field_any(
        map,
        &["accession", "Accession", "gse", "GSE", "gds", "GDS", "acc"],
    )
    .unwrap_or_else(|| uid.to_string());
    let title = string_field_any(map, &["title", "Title", "gdsTitle", "GDS_Title"])
        .or_else(|| string_field_any(map, &["summary", "Summary"]))
        .unwrap_or_else(|| accession.clone());
    let summary = string_field_any(map, &["summary", "Summary", "description", "Description"])
        .unwrap_or_default();
    let record_type = string_field_any(
        map,
        &["gdsType", "gdstype", "entryType", "entrytype", "type"],
    );
    let organism = string_field_any(map, &["taxon", "taxa", "organism", "Organism"]);
    let sample_count = json_u64_from_keys(map, &["n_samples", "nSamples", "samples", "Samples"]);
    let platform = string_field_any(map, &["GPL", "gpl", "platform", "Platform"]);
    let published_date = string_field_any(map, &["PDAT", "pdat", "pubdate", "pub_date"]);
    let updated_date = string_field_any(map, &["updated", "updated_date", "last_update"]);
    let files = string_vec_field_any(map, &["suppFile", "suppFiles", "ftp", "files"]);
    let url = geo_record_url(&accession, uid);

    let mut extra = JsonMap::new();
    extra.insert("uid".to_string(), json!(uid));
    for key in [
        "gse",
        "GSE",
        "gpl",
        "GPL",
        "gdsType",
        "entryType",
        "FTPLink",
        "ftplink",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }

    Some(DataRecord {
        id: if accession.is_empty() {
            uid.to_string()
        } else {
            accession.clone()
        },
        accession,
        source: PublicDataSource::Geo,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary),
        url,
        record_type,
        organism,
        published_date,
        updated_date,
        sample_count,
        platform,
        files,
        extra,
    })
}

fn parse_cbioportal_studies_json(value: &Json) -> Vec<DataRecord> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(parse_cbioportal_study).collect()
}

fn parse_cbioportal_study(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let study_id = string_field_any(map, &["studyId", "study_id", "id"])?;
    let title =
        string_field_any(map, &["name", "studyName", "title"]).unwrap_or_else(|| study_id.clone());
    let description =
        string_field_any(map, &["description", "shortDescription", "summary"]).unwrap_or_default();
    let cancer_type = string_field_any(map, &["cancerTypeId", "cancer_type_id"])
        .or_else(|| nested_string_field(map, "cancerType", &["name", "cancerTypeId"]));
    let sample_count = json_u64_from_keys(
        map,
        &[
            "allSampleCount",
            "sampleCount",
            "numberOfSamples",
            "samples",
        ],
    );
    let published_date = string_field_any(map, &["importDate", "publishedDate"]);
    let citation = string_field_any(map, &["citation"]);
    let pmid = string_field_any(map, &["pmid", "PMID"]);
    let mut extra = JsonMap::new();
    for key in [
        "studyId",
        "cancerTypeId",
        "cancerType",
        "citation",
        "pmid",
        "groups",
        "referenceGenome",
        "publicStudy",
        "status",
        "readPermission",
        "allSampleCount",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    if let Some(citation) = citation {
        extra.insert("citation_text".to_string(), json!(citation));
    }
    if let Some(pmid) = pmid {
        extra.insert("pmid".to_string(), json!(pmid));
    }

    Some(DataRecord {
        id: study_id.clone(),
        accession: study_id.clone(),
        source: PublicDataSource::CbioPortal,
        title: clean_html_text(&title),
        summary: clean_html_text(&description),
        url: cbioportal_study_url(&study_id),
        record_type: Some("study".to_string()),
        organism: cancer_type,
        published_date,
        updated_date: None,
        sample_count,
        platform: None,
        files: Vec::new(),
        extra,
    })
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

fn parse_ena_portal_json(source: PublicDataSource, value: &Json) -> Vec<DataRecord> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| parse_ena_portal_item(source, item))
        .collect()
}

fn parse_ena_portal_item(source: PublicDataSource, item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let accession = string_field_any(map, ena_accession_fields(source))?;
    let display_accession = string_field_any(map, ena_display_accession_fields(source))
        .unwrap_or_else(|| accession.clone());
    let title = string_field_any(map, ena_title_fields(source))
        .or_else(|| string_field_any(map, &["description"]))
        .unwrap_or_else(|| accession.clone());
    let summary = string_field_any(map, ena_summary_fields(source)).unwrap_or_default();
    let organism = string_field_any(map, &["scientific_name", "organism", "taxon"]);
    let published_date = string_field_any(map, &["first_public", "first_publication"]);
    let updated_date = string_field_any(map, &["last_updated", "last_update"]);
    let sample_count = json_u64_from_keys(map, &["sample_count", "samples"]);
    let platform = string_field_any(
        map,
        &["instrument_platform", "instrument_model", "platform"],
    );
    let files = string_vec_field_any(
        map,
        &[
            "submitted_ftp",
            "fastq_ftp",
            "fasta_ftp",
            "cram_ftp",
            "bam_ftp",
            "sra_ftp",
            "generated_ftp",
        ],
    );
    let mut extra = JsonMap::new();
    for key in [
        "secondary_study_accession",
        "study_accession",
        "experiment_accession",
        "run_accession",
        "sample_accession",
        "analysis_accession",
        "assembly_accession",
        "analysis_title",
        "analysis_alias",
        "analysis_description",
        "assembly_title",
        "center_name",
        "tax_id",
        "study_alias",
        "sample_alias",
        "experiment_alias",
        "library_strategy",
        "library_source",
        "analysis_type",
        "assembly_type",
        "country",
        "collection_date",
        "host",
        "host_tax_id",
        "host_body_site",
        "specimen_voucher",
        "bio_material",
        "fastq_md5",
        "sra_md5",
        "submitted_md5",
        "generated_md5",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    Some(DataRecord {
        id: accession.clone(),
        accession: display_accession,
        source,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary),
        url: ena_record_url(&accession),
        record_type: source.ena_result().map(str::to_string),
        organism,
        published_date,
        updated_date,
        sample_count,
        platform,
        files,
        extra,
    })
}

fn parse_ena_xml_record(
    source: PublicDataSource,
    xml: &str,
    fallback_accession: &str,
) -> Option<DataRecord> {
    let accession = extract_xml_attr(xml, "STUDY", "accession")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "accession"))
        .or_else(|| extract_xml_attr(xml, "RUN", "accession"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "accession"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "accession"))
        .or_else(|| extract_xml_attr(xml, "ASSEMBLY", "accession"))
        .or_else(|| extract_xml_attr(xml, "SEQUENCE", "accession"))
        .unwrap_or_else(|| fallback_accession.to_string());
    let title = extract_first_xml_tag(
        xml,
        &["STUDY_TITLE", "TITLE", "SAMPLE_TITLE", "DESCRIPTION"],
    )
    .unwrap_or_else(|| accession.clone());
    let summary = extract_first_xml_tag(xml, &["STUDY_ABSTRACT", "DESCRIPTION", "ABSTRACT"])
        .unwrap_or_default();
    let center = extract_xml_attr(xml, "STUDY", "center_name")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "center_name"))
        .or_else(|| extract_xml_attr(xml, "RUN", "center_name"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "center_name"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "center_name"))
        .or_else(|| extract_first_xml_tag(xml, &["CENTER_NAME"]));
    let alias = extract_xml_attr(xml, "STUDY", "alias")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "alias"))
        .or_else(|| extract_xml_attr(xml, "RUN", "alias"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "alias"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "alias"));
    let mut extra = JsonMap::new();
    if let Some(center) = center {
        extra.insert("center_name".to_string(), json!(center));
    }
    if let Some(alias) = alias {
        extra.insert("alias".to_string(), json!(alias));
    }
    Some(DataRecord {
        id: accession.clone(),
        accession: accession.clone(),
        source,
        title: clean_xml_fragment(&title),
        summary: clean_xml_fragment(&summary),
        url: ena_record_url(&accession),
        record_type: Some(format!(
            "{} xml_record",
            source.ena_result().unwrap_or("ena")
        )),
        organism: extract_first_xml_tag(xml, &["SCIENTIFIC_NAME", "TAXON"]),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: extract_ena_file_links(xml),
        extra,
    })
}

fn ena_fields(source: PublicDataSource) -> String {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => Vec::new(),
        PublicDataSource::EnaStudy => vec![
            "study_accession",
            "secondary_study_accession",
            "study_title",
            "description",
            "study_alias",
            "center_name",
            "tax_id",
            "scientific_name",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaRun => vec![
            "run_accession",
            "experiment_accession",
            "sample_accession",
            "study_accession",
            "secondary_study_accession",
            "scientific_name",
            "instrument_platform",
            "instrument_model",
            "library_strategy",
            "library_source",
            "first_public",
            "last_updated",
            "fastq_ftp",
            "fastq_md5",
            "submitted_ftp",
            "submitted_md5",
            "sra_ftp",
            "sra_md5",
        ],
        PublicDataSource::EnaExperiment => vec![
            "experiment_accession",
            "study_accession",
            "sample_accession",
            "experiment_title",
            "experiment_alias",
            "scientific_name",
            "instrument_platform",
            "instrument_model",
            "library_strategy",
            "library_source",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaSample => vec![
            "sample_accession",
            "secondary_sample_accession",
            "sample_alias",
            "scientific_name",
            "tax_id",
            "description",
            "country",
            "collection_date",
            "host",
            "host_tax_id",
            "first_public",
            "last_updated",
        ],
        PublicDataSource::EnaAnalysis => vec![
            "analysis_accession",
            "study_accession",
            "sample_accession",
            "analysis_title",
            "analysis_description",
            "analysis_alias",
            "analysis_type",
            "assembly_type",
            "description",
            "scientific_name",
            "first_public",
            "last_updated",
            "submitted_ftp",
            "submitted_md5",
            "generated_ftp",
            "generated_md5",
        ],
        PublicDataSource::EnaAssembly => vec![
            "assembly_accession",
            "scientific_name",
            "tax_id",
            "assembly_name",
            "assembly_title",
            "assembly_level",
            "description",
            "last_updated",
        ],
        PublicDataSource::EnaSequence => vec![
            "accession",
            "description",
            "scientific_name",
            "tax_id",
            "specimen_voucher",
            "bio_material",
            "first_public",
            "last_updated",
        ],
    }
    .join(",")
}

fn ena_portal_query(source: PublicDataSource, query: &str) -> String {
    let query = query.trim();
    if looks_like_ena_advanced_query(query) {
        return query.to_string();
    }
    let escaped = escape_ena_query_value(query);
    ena_simple_search_fields(source)
        .iter()
        .map(|field| format!("{field}=\"*{escaped}*\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn looks_like_ena_advanced_query(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    query.contains('=')
        || lower.contains(" and ")
        || lower.contains(" or ")
        || lower.contains("tax_")
        || lower.contains("country")
        || lower.contains("scientific_name")
}

fn ena_simple_search_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => {
            &["description"]
        }
        PublicDataSource::EnaStudy => &["study_title", "description"],
        PublicDataSource::EnaRun => &["description", "scientific_name", "study_title"],
        PublicDataSource::EnaExperiment => &["experiment_title", "description", "scientific_name"],
        PublicDataSource::EnaSample => &["description", "scientific_name", "sample_alias"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_title",
            "analysis_description",
            "description",
            "analysis_type",
            "scientific_name",
        ],
        PublicDataSource::EnaAssembly => &[
            "assembly_name",
            "assembly_title",
            "description",
            "scientific_name",
        ],
        PublicDataSource::EnaSequence => &["description", "scientific_name"],
    }
}

fn ena_accession_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => {
            &["accession"]
        }
        PublicDataSource::EnaStudy => {
            &["study_accession", "secondary_study_accession", "accession"]
        }
        PublicDataSource::EnaRun => &["run_accession", "accession"],
        PublicDataSource::EnaExperiment => &["experiment_accession", "accession"],
        PublicDataSource::EnaSample => &[
            "sample_accession",
            "secondary_sample_accession",
            "accession",
        ],
        PublicDataSource::EnaAnalysis => &["analysis_accession", "accession"],
        PublicDataSource::EnaAssembly => &["assembly_accession", "accession"],
        PublicDataSource::EnaSequence => &["accession"],
    }
}

fn ena_display_accession_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::EnaStudy => {
            &["secondary_study_accession", "study_accession", "accession"]
        }
        PublicDataSource::EnaSample => &[
            "secondary_sample_accession",
            "sample_accession",
            "accession",
        ],
        _ => ena_accession_fields(source),
    }
}

fn ena_title_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => &["title"],
        PublicDataSource::EnaStudy => &["study_title", "title", "description"],
        PublicDataSource::EnaRun => &["run_alias", "description", "run_accession"],
        PublicDataSource::EnaExperiment => &["experiment_title", "experiment_alias", "description"],
        PublicDataSource::EnaSample => &["sample_alias", "description", "scientific_name"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_title",
            "analysis_alias",
            "analysis_description",
            "description",
            "analysis_accession",
        ],
        PublicDataSource::EnaAssembly => &[
            "assembly_name",
            "assembly_title",
            "description",
            "assembly_accession",
        ],
        PublicDataSource::EnaSequence => &["description", "accession"],
    }
}

fn ena_summary_fields(source: PublicDataSource) -> &'static [&'static str] {
    match source {
        PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex => {
            &["summary"]
        }
        PublicDataSource::EnaStudy => &["study_description", "description"],
        PublicDataSource::EnaRun => &[
            "description",
            "library_strategy",
            "library_source",
            "instrument_model",
        ],
        PublicDataSource::EnaExperiment => &[
            "experiment_title",
            "experiment_alias",
            "library_strategy",
            "library_source",
        ],
        PublicDataSource::EnaSample => &["description", "sample_alias", "country"],
        PublicDataSource::EnaAnalysis => &[
            "analysis_description",
            "analysis_title",
            "description",
            "analysis_type",
            "assembly_type",
        ],
        PublicDataSource::EnaAssembly => &[
            "description",
            "assembly_title",
            "assembly_name",
            "assembly_level",
        ],
        PublicDataSource::EnaSequence => &["description", "scientific_name"],
    }
}

fn ena_accession_query(source: PublicDataSource, accession: &str) -> String {
    let escaped = escape_ena_query_value(accession);
    ena_accession_query_fields(source, infer_ena_source_from_accession(accession))
        .iter()
        .map(|field| format!("{field}=\"{escaped}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn ena_accession_query_fields(
    source: PublicDataSource,
    accession_source: Option<PublicDataSource>,
) -> Vec<&'static str> {
    match (source, accession_source) {
        (PublicDataSource::Geo | PublicDataSource::CbioPortal | PublicDataSource::Gtex, _) => {
            vec!["accession"]
        }
        (PublicDataSource::EnaStudy, _) => vec!["study_accession", "secondary_study_accession"],
        (PublicDataSource::EnaRun, Some(PublicDataSource::EnaStudy)) => {
            vec!["study_accession", "secondary_study_accession"]
        }
        (PublicDataSource::EnaRun, Some(PublicDataSource::EnaExperiment)) => {
            vec!["experiment_accession"]
        }
        (PublicDataSource::EnaRun, Some(PublicDataSource::EnaSample)) => {
            vec!["sample_accession", "secondary_sample_accession"]
        }
        (PublicDataSource::EnaRun, _) => vec!["run_accession"],
        (PublicDataSource::EnaExperiment, Some(PublicDataSource::EnaStudy)) => {
            vec!["study_accession", "secondary_study_accession"]
        }
        (PublicDataSource::EnaExperiment, Some(PublicDataSource::EnaSample)) => {
            vec!["sample_accession", "secondary_sample_accession"]
        }
        (PublicDataSource::EnaExperiment, _) => vec!["experiment_accession"],
        (PublicDataSource::EnaSample, _) => vec!["sample_accession", "secondary_sample_accession"],
        (PublicDataSource::EnaAnalysis, Some(PublicDataSource::EnaStudy)) => {
            vec!["study_accession", "secondary_study_accession"]
        }
        (PublicDataSource::EnaAnalysis, Some(PublicDataSource::EnaSample)) => {
            vec!["sample_accession", "secondary_sample_accession"]
        }
        (PublicDataSource::EnaAnalysis, _) => vec!["analysis_accession"],
        (PublicDataSource::EnaAssembly, _) => vec!["assembly_accession"],
        (PublicDataSource::EnaSequence, _) => vec!["accession"],
    }
}

fn escape_ena_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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

fn normalize_accession(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("ebi.ac.uk") && parsed.path().contains("/ena/browser/view/") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
        if host.contains("ncbi.nlm.nih.gov") {
            for (key, val) in parsed.query_pairs() {
                if key.eq_ignore_ascii_case("acc") && !val.trim().is_empty() {
                    return Some(val.into_owned());
                }
            }
        }
    }
    Some(value.to_string())
}

fn normalize_cbioportal_study_id(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("cbioportal") {
            for (key, val) in parsed.query_pairs() {
                if key.eq_ignore_ascii_case("id")
                    || key.eq_ignore_ascii_case("studyId")
                    || key.eq_ignore_ascii_case("study_id")
                {
                    let val = val.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty() && *segment != "summary")
                .map(str::to_string);
        }
    }
    Some(value.to_string())
}

pub fn looks_like_geo_accession(value: &str) -> bool {
    let Some(accession) = normalize_accession(value) else {
        return false;
    };
    let upper = accession.to_ascii_uppercase();
    ["GSE", "GSM", "GPL", "GDS"].iter().any(|prefix| {
        upper
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    })
}

pub fn looks_like_ena_accession(value: &str) -> bool {
    let Some(accession) = normalize_accession(value) else {
        return false;
    };
    infer_ena_source_from_accession(&accession).is_some()
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

pub fn inferred_ena_source_key(value: &str) -> Option<&'static str> {
    infer_ena_source_from_accession(value).map(PublicDataSource::as_str)
}

fn infer_ena_source_from_accession(value: &str) -> Option<PublicDataSource> {
    let accession = normalize_accession(value)?;
    let upper = accession.to_ascii_uppercase();
    if upper.starts_with("PRJ")
        || upper.starts_with("ERP")
        || upper.starts_with("SRP")
        || upper.starts_with("DRP")
    {
        return Some(PublicDataSource::EnaStudy);
    }
    if upper.starts_with("ERX") || upper.starts_with("SRX") || upper.starts_with("DRX") {
        return Some(PublicDataSource::EnaExperiment);
    }
    if upper.starts_with("ERR") || upper.starts_with("SRR") || upper.starts_with("DRR") {
        return Some(PublicDataSource::EnaRun);
    }
    if upper.starts_with("ERS")
        || upper.starts_with("SRS")
        || upper.starts_with("DRS")
        || upper.starts_with("SAM")
    {
        return Some(PublicDataSource::EnaSample);
    }
    if upper.starts_with("ERZ") || upper.starts_with("SRZ") || upper.starts_with("DRZ") {
        return Some(PublicDataSource::EnaAnalysis);
    }
    if upper.starts_with("GCA_") || upper.starts_with("GCF_") {
        return Some(PublicDataSource::EnaAssembly);
    }
    None
}

fn geo_record_url(accession: &str, uid: &str) -> String {
    if looks_like_geo_accession(accession) {
        format!(
            "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc={}",
            accession
        )
    } else {
        format!("https://www.ncbi.nlm.nih.gov/gds/{uid}")
    }
}

fn ena_record_url(accession: &str) -> String {
    format!("https://www.ebi.ac.uk/ena/browser/view/{accession}")
}

fn cbioportal_study_url(study_id: &str) -> String {
    format!("https://www.cbioportal.org/study/summary?id={study_id}")
}

fn displayed_link_for_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.");
    let mut out = host.to_string();
    let path = parsed.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    if let Some(query) = parsed.query().filter(|q| !q.is_empty()) {
        out.push('?');
        out.push_str(query);
    }
    out
}

fn string_field_any(map: &JsonMap<String, Json>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = map.get(*key).and_then(json_string) {
            return Some(value);
        }
    }
    None
}

fn nested_string_field(
    map: &JsonMap<String, Json>,
    object_key: &str,
    keys: &[&str],
) -> Option<String> {
    let nested = map.get(object_key)?.as_object()?;
    string_field_any(nested, keys)
}

fn string_vec_field_any(map: &JsonMap<String, Json>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            let out = items.iter().filter_map(json_string).collect::<Vec<_>>();
            if !out.is_empty() {
                return out;
            }
        }
        if let Some(value) = json_string(value) {
            let out = value
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !out.is_empty() {
                return out;
            }
        }
    }
    Vec::new()
}

fn json_u64_from_keys(map: &JsonMap<String, Json>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_u64_from_string_or_number))
}

fn json_string(value: &Json) -> Option<String> {
    match value {
        Json::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        Json::Number(n) => Some(n.to_string()),
        Json::Array(items) => {
            let joined = items
                .iter()
                .filter_map(json_string)
                .collect::<Vec<_>>()
                .join(", ");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn json_number_string(value: &Json) -> Option<String> {
    value
        .as_f64()
        .map(|v| {
            if v.fract() == 0.0 {
                format!("{v:.0}")
            } else {
                v.to_string()
            }
        })
        .or_else(|| value.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

fn json_u64_from_string_or_number(value: &Json) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn clean_html_text(value: &str) -> String {
    lazy_static! {
        static ref RE_TAG: Regex = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).unwrap();
    }
    let without_tags = RE_TAG.replace_all(value, " ");
    let decoded = decode_xml_text(without_tags.as_ref());
    RE_WS.replace_all(decoded.trim(), " ").to_string()
}

fn clean_xml_fragment(value: &str) -> String {
    clean_html_text(value)
}

fn decode_xml_text(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn extract_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?is)<{}\b(?P<attrs>[^>]*)>"#,
        regex::escape(tag)
    ))
    .ok()?;
    let attr_re = Regex::new(&format!(
        r#"(?i)\b{}\s*=\s*["'](?P<value>[^"']+)["']"#,
        regex::escape(attr)
    ))
    .ok()?;
    for cap in re.captures_iter(xml) {
        let Some(attrs) = cap.name("attrs").map(|m| m.as_str()) else {
            continue;
        };
        if let Some(value) = attr_re
            .captures(attrs)
            .and_then(|c| c.name("value"))
            .map(|m| decode_xml_text(m.as_str()))
        {
            return Some(value);
        }
    }
    None
}

fn extract_first_xml_tag(xml: &str, tags: &[&str]) -> Option<String> {
    for tag in tags {
        let re = Regex::new(&format!(
            r#"(?is)<{}\b[^>]*>(?P<body>.*?)</{}>"#,
            regex::escape(tag),
            regex::escape(tag)
        ))
        .ok()?;
        if let Some(value) = re
            .captures(xml)
            .and_then(|cap| cap.name("body"))
            .map(|m| clean_xml_fragment(m.as_str()))
            .filter(|s| !s.trim().is_empty())
        {
            return Some(value);
        }
    }
    None
}

fn extract_ena_file_links(xml: &str) -> Vec<String> {
    lazy_static! {
        static ref RE_XREF: Regex =
            Regex::new(r#"(?is)<XREF_LINK\b[^>]*>(?P<body>.*?)</XREF_LINK>"#).unwrap();
    }
    let mut files = Vec::new();
    for cap in RE_XREF.captures_iter(xml) {
        let body = cap.name("body").map(|m| m.as_str()).unwrap_or("");
        if let Some(url) = extract_first_xml_tag(body, &["URL"]).filter(|u| {
            let lower = u.to_ascii_lowercase();
            lower.starts_with("ftp://")
                || lower.starts_with("http://")
                || lower.starts_with("https://")
        }) {
            files.push(url);
        }
    }
    files
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn truncate_for_error(value: &str) -> String {
    truncate_chars(value, 500)
}

fn encode_path_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_geo_esearch_and_esummary() {
        let search = json!({
            "esearchresult": {
                "count": "1",
                "idlist": ["200000001"],
                "querytranslation": "cancer"
            }
        });
        let (count, ids, translation) = parse_geo_esearch(&search).unwrap();
        assert_eq!(count, 1);
        assert_eq!(ids, vec!["200000001"]);
        assert_eq!(translation.as_deref(), Some("cancer"));

        let summary = json!({
            "result": {
                "uids": ["200000001"],
                "200000001": {
                    "uid": "200000001",
                    "accession": "GSE123",
                    "title": "<b>RNA-seq study</b>",
                    "summary": "A useful dataset",
                    "gdsType": "Expression profiling by high throughput sequencing",
                    "taxon": "Homo sapiens",
                    "n_samples": "42",
                    "GPL": "GPL20301",
                    "PDAT": "2024/01/02"
                }
            }
        });
        let records = parse_geo_esummary(&summary, &ids);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].accession, "GSE123");
        assert_eq!(records[0].title, "RNA-seq study");
        assert_eq!(records[0].sample_count, Some(42));
        assert!(records[0].url.contains("GSE123"));
    }

    #[test]
    fn parses_ena_portal_json() {
        let value = json!([{
            "study_accession": "PRJEB123",
            "secondary_study_accession": "ERP123",
            "study_title": "Metagenome study",
            "description": "Rumen samples",
            "center_name": "EBI",
            "scientific_name": "cow metagenome",
            "first_public": "2024-01-01",
            "last_updated": "2024-02-01"
        }]);
        let records = parse_ena_portal_json(PublicDataSource::EnaStudy, &value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "PRJEB123");
        assert_eq!(records[0].accession, "ERP123");
        assert_eq!(records[0].organism.as_deref(), Some("cow metagenome"));
        assert!(records[0].url.ends_with("/PRJEB123"));
    }

    #[test]
    fn parses_ena_run_portal_json_with_file_links() {
        let value = json!([{
            "run_accession": "ERR123",
            "experiment_accession": "ERX123",
            "sample_accession": "ERS123",
            "study_accession": "PRJEB123",
            "scientific_name": "Homo sapiens",
            "instrument_platform": "ILLUMINA",
            "instrument_model": "Illumina NovaSeq 6000",
            "library_strategy": "RNA-Seq",
            "fastq_ftp": "ftp.sra.ebi.ac.uk/vol1/fastq/ERR123/ERR123_1.fastq.gz;ftp.sra.ebi.ac.uk/vol1/fastq/ERR123/ERR123_2.fastq.gz"
        }]);
        let records = parse_ena_portal_json(PublicDataSource::EnaRun, &value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "ERR123");
        assert_eq!(records[0].source, PublicDataSource::EnaRun);
        assert_eq!(records[0].platform.as_deref(), Some("ILLUMINA"));
        assert_eq!(records[0].files.len(), 2);
        assert_eq!(
            records[0].extra["library_strategy"].as_str(),
            Some("RNA-Seq")
        );
    }

    #[test]
    fn parses_cbioportal_study_json() {
        let value = json!([{
            "studyId": "brca_tcga",
            "name": "Breast Invasive Carcinoma (TCGA, PanCancer Atlas)",
            "description": "TCGA breast cancer study",
            "cancerTypeId": "brca",
            "allSampleCount": 1084,
            "citation": "TCGA, Cell 2018",
            "pmid": "29625048",
            "importDate": "2025-01-01"
        }]);
        let records = parse_cbioportal_studies_json(&value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "brca_tcga");
        assert_eq!(records[0].source, PublicDataSource::CbioPortal);
        assert_eq!(records[0].sample_count, Some(1084));
        assert_eq!(records[0].organism.as_deref(), Some("brca"));
        assert!(records[0].url.contains("id=brca_tcga"));
        assert_eq!(
            normalize_cbioportal_study_id("https://www.cbioportal.org/study/summary?id=brca_tcga")
                .as_deref(),
            Some("brca_tcga")
        );
    }

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

    #[test]
    fn parses_ena_xml_record() {
        let xml = r#"
        <STUDY_SET>
          <STUDY accession="PRJEB999" alias="alias-1" center_name="EBI">
            <DESCRIPTOR>
              <STUDY_TITLE>XML Study</STUDY_TITLE>
              <STUDY_ABSTRACT>XML abstract &amp; details.</STUDY_ABSTRACT>
            </DESCRIPTOR>
            <STUDY_LINKS>
              <STUDY_LINK>
                <XREF_LINK><DB>ENA-FASTQ-FILES</DB><URL>ftp://example/file.fastq.gz</URL></XREF_LINK>
              </STUDY_LINK>
            </STUDY_LINKS>
          </STUDY>
        </STUDY_SET>
        "#;
        let record = parse_ena_xml_record(PublicDataSource::EnaStudy, xml, "fallback").unwrap();
        assert_eq!(record.accession, "PRJEB999");
        assert_eq!(record.title, "XML Study");
        assert_eq!(record.summary, "XML abstract & details.");
        assert_eq!(record.files, vec!["ftp://example/file.fastq.gz"]);
    }

    #[test]
    fn builds_ena_queries_and_detects_record_types() {
        assert_eq!(
            PublicDataSource::parse("ena_run"),
            Some(PublicDataSource::EnaRun)
        );
        assert_eq!(
            PublicDataSource::parse("read_experiment"),
            Some(PublicDataSource::EnaExperiment)
        );
        assert_eq!(PublicDataSource::EnaRun.ena_result(), Some("read_run"));
        assert_eq!(
            ena_portal_query(PublicDataSource::EnaStudy, "rumen"),
            "study_title=\"*rumen*\" OR description=\"*rumen*\""
        );
        assert_eq!(
            ena_portal_query(PublicDataSource::EnaRun, "rumen"),
            "description=\"*rumen*\" OR scientific_name=\"*rumen*\" OR study_title=\"*rumen*\""
        );
        assert_eq!(
            ena_portal_query(
                PublicDataSource::EnaRun,
                "country=\"United Kingdom\" AND host_tax_id=9913"
            ),
            "country=\"United Kingdom\" AND host_tax_id=9913"
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaRun, "ERR123"),
            "run_accession=\"ERR123\""
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaRun, "ERX123"),
            "experiment_accession=\"ERX123\""
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaRun, "PRJEB123"),
            "study_accession=\"PRJEB123\" OR secondary_study_accession=\"PRJEB123\""
        );
        assert_eq!(
            ena_accession_query(PublicDataSource::EnaAnalysis, "SAMEA123"),
            "sample_accession=\"SAMEA123\" OR secondary_sample_accession=\"SAMEA123\""
        );
        assert_eq!(
            infer_ena_source_from_accession("ERX123"),
            Some(PublicDataSource::EnaExperiment)
        );
        assert_eq!(
            infer_ena_source_from_accession("ERZ123"),
            Some(PublicDataSource::EnaAnalysis)
        );
        let study_fields = ena_fields(PublicDataSource::EnaStudy);
        assert!(study_fields.contains("description"));
        assert!(!study_fields.contains("study_description"));
        let assembly_fields = ena_fields(PublicDataSource::EnaAssembly);
        assert!(assembly_fields.contains("assembly_title"));
        assert!(!assembly_fields.contains("first_public"));
        assert!(ena_fields(PublicDataSource::EnaAnalysis).contains("generated_ftp"));
    }

    #[test]
    fn data_json_uses_serpapi_shape() {
        let record = DataRecord {
            id: "GSE123".to_string(),
            accession: "GSE123".to_string(),
            source: PublicDataSource::Geo,
            title: "Dataset".to_string(),
            summary: "Summary".to_string(),
            url: "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE123".to_string(),
            record_type: Some("Series".to_string()),
            organism: Some("Homo sapiens".to_string()),
            published_date: None,
            updated_date: None,
            sample_count: Some(3),
            platform: None,
            files: Vec::new(),
            extra: JsonMap::new(),
        };
        let response = DataSearchResponse {
            query: "test".to_string(),
            source: "geo".to_string(),
            total: Some(1),
            results: vec![record.clone()],
            notes: vec![],
        };
        let json = search_response_to_json(&response);
        assert_eq!(json["category"], "data");
        assert_eq!(json["results"][0]["source"], "geo");
        assert_eq!(json["results"][0]["metadata"]["organism"], "Homo sapiens");

        let detail = detail_to_json(&record);
        assert_eq!(detail["category"], "data");
        assert!(detail["content"]
            .as_str()
            .unwrap()
            .contains("Source: NCBI GEO"));
    }

    #[test]
    fn recognizes_data_accessions_and_urls() {
        assert!(looks_like_geo_accession("GSE12345"));
        assert!(looks_like_geo_accession(
            "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSM575"
        ));
        assert!(looks_like_ena_accession("PRJEB12345"));
        assert!(looks_like_ena_accession(
            "https://www.ebi.ac.uk/ena/browser/view/ERR123"
        ));
        assert_eq!(
            normalize_accession("https://www.ebi.ac.uk/ena/browser/view/PRJEB123").as_deref(),
            Some("PRJEB123")
        );
        assert!(looks_like_gtex_identifier(
            "https://gtexportal.org/home/gene/ENSG00000132693.12"
        ));
    }
}
