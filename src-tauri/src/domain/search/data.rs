//! Public biological data-source adapters used by the built-in `search` / `fetch` tools.
//!
//! GEO is backed by official NCBI Entrez E-utilities (`db=gds`). ENA uses the
//! official ENA Portal API for indexed study search and the Browser API XML
//! endpoint as a detail fallback.

use crate::domain::tools::{ToolContext, WebSearchApiKeys};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Map as JsonMap, Value as Json};
use std::time::Duration;

const DEFAULT_EUTILS_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const ENA_PORTAL_SEARCH_URL: &str = "https://www.ebi.ac.uk/ena/portal/api/search";
const ENA_BROWSER_XML_BASE_URL: &str = "https://www.ebi.ac.uk/ena/browser/api/xml";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const DEFAULT_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_TOOL: &str = "omiga";
const GEO_FAVICON: &str = "https://www.ncbi.nlm.nih.gov/favicon.ico";
const ENA_FAVICON: &str = "https://www.ebi.ac.uk/favicon.ico";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicDataSource {
    Geo,
    Ena,
}

impl PublicDataSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "geo" | "gds" | "ncbi_geo" | "ncbi_gds" => Some(Self::Geo),
            "ena" | "european_nucleotide_archive" => Some(Self::Ena),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Geo => "geo",
            Self::Ena => "ena",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Geo => "NCBI GEO DataSets",
            Self::Ena => "European Nucleotide Archive",
        }
    }

    fn favicon(self) -> &'static str {
        match self {
            Self::Geo => GEO_FAVICON,
            Self::Ena => ENA_FAVICON,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DataSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
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
    entrez_base_url: String,
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
            entrez_base_url: DEFAULT_EUTILS_BASE_URL.to_string(),
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
            PublicDataSource::Ena => self.search_ena(args).await,
        }
    }

    pub async fn search_auto(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        let max_results = args.normalized_max_results() as usize;
        let geo_args = args.clone();
        let ena_args = args.clone();
        let (geo, ena) = tokio::join!(self.search_geo(geo_args), self.search_ena(ena_args));

        let mut results = Vec::new();
        let mut total = 0u64;
        let mut saw_total = false;
        let mut notes = vec!["Combined GEO + ENA data search".to_string()];

        for response in [geo, ena] {
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
            PublicDataSource::Ena => self.fetch_ena(identifier).await,
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

    async fn search_ena(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let query = ena_portal_query(args.query.trim());
        let fields = ena_study_fields();
        let response = self
            .http
            .get(ENA_PORTAL_SEARCH_URL)
            .query(&[
                ("result", "read_study".to_string()),
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
        let results = parse_ena_portal_json(&json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "ena".to_string(),
            total: Some(results.len() as u64),
            results,
            notes: vec![
                "ENA Portal API read_study search".to_string(),
                "Simple free-text queries are translated to study_title/study_description wildcards; advanced ENA query syntax is passed through.".to_string(),
            ],
        })
    }

    async fn fetch_ena(&self, identifier: &str) -> Result<DataRecord, String> {
        let accession = normalize_accession(identifier)
            .ok_or_else(|| "ENA fetch requires an accession or ENA Browser URL".to_string())?;
        let query = format!(
            "study_accession=\"{0}\" OR secondary_study_accession=\"{0}\"",
            escape_ena_query_value(&accession)
        );
        let fields = ena_study_fields();
        let response = self
            .http
            .get(ENA_PORTAL_SEARCH_URL)
            .query(&[
                ("result", "read_study".to_string()),
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
            if let Some(record) = parse_ena_portal_json(&json).into_iter().next() {
                return Ok(record);
            }
        }

        let url = format!(
            "{ENA_BROWSER_XML_BASE_URL}/{}",
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
        parse_ena_xml_record(&xml, &accession)
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
        let url = format!("{}/{}.fcgi", self.entrez_base_url, utility);
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

fn parse_ena_portal_json(value: &Json) -> Vec<DataRecord> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(parse_ena_portal_item).collect()
}

fn parse_ena_portal_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let accession = string_field_any(
        map,
        &[
            "study_accession",
            "accession",
            "secondary_study_accession",
            "run_accession",
            "sample_accession",
        ],
    )?;
    let secondary = string_field_any(map, &["secondary_study_accession"]);
    let title = string_field_any(map, &["study_title", "title", "description"])
        .or_else(|| secondary.clone())
        .unwrap_or_else(|| accession.clone());
    let summary = string_field_any(map, &["study_description", "description"]).unwrap_or_default();
    let organism = string_field_any(map, &["scientific_name", "organism", "taxon"]);
    let published_date = string_field_any(map, &["first_public", "first_publication"]);
    let updated_date = string_field_any(map, &["last_updated", "last_update"]);
    let sample_count = json_u64_from_keys(map, &["sample_count", "samples"]);
    let files = string_vec_field_any(map, &["submitted_ftp", "fastq_ftp", "submitted_md5"]);
    let mut extra = JsonMap::new();
    for key in [
        "secondary_study_accession",
        "center_name",
        "tax_id",
        "study_alias",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    Some(DataRecord {
        id: accession.clone(),
        accession: secondary.unwrap_or(accession.clone()),
        source: PublicDataSource::Ena,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary),
        url: ena_record_url(&accession),
        record_type: Some("read_study".to_string()),
        organism,
        published_date,
        updated_date,
        sample_count,
        platform: None,
        files,
        extra,
    })
}

fn parse_ena_xml_record(xml: &str, fallback_accession: &str) -> Option<DataRecord> {
    let accession = extract_xml_attr(xml, "STUDY", "accession")
        .or_else(|| extract_xml_attr(xml, "SAMPLE", "accession"))
        .or_else(|| extract_xml_attr(xml, "RUN", "accession"))
        .or_else(|| extract_xml_attr(xml, "EXPERIMENT", "accession"))
        .or_else(|| extract_xml_attr(xml, "ANALYSIS", "accession"))
        .unwrap_or_else(|| fallback_accession.to_string());
    let title =
        extract_first_xml_tag(xml, &["STUDY_TITLE", "TITLE"]).unwrap_or_else(|| accession.clone());
    let summary = extract_first_xml_tag(xml, &["STUDY_ABSTRACT", "DESCRIPTION", "ABSTRACT"])
        .unwrap_or_default();
    let center = extract_xml_attr(xml, "STUDY", "center_name")
        .or_else(|| extract_first_xml_tag(xml, &["CENTER_NAME"]));
    let alias = extract_xml_attr(xml, "STUDY", "alias");
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
        source: PublicDataSource::Ena,
        title: clean_xml_fragment(&title),
        summary: clean_xml_fragment(&summary),
        url: ena_record_url(&accession),
        record_type: Some("xml_record".to_string()),
        organism: extract_first_xml_tag(xml, &["SCIENTIFIC_NAME", "TAXON"]),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: extract_ena_file_links(xml),
        extra,
    })
}

fn ena_study_fields() -> String {
    [
        "study_accession",
        "secondary_study_accession",
        "study_title",
        "study_description",
        "center_name",
        "tax_id",
        "scientific_name",
        "first_public",
        "last_updated",
    ]
    .join(",")
}

fn ena_portal_query(query: &str) -> String {
    let query = query.trim();
    if looks_like_ena_advanced_query(query) {
        return query.to_string();
    }
    let escaped = escape_ena_query_value(query);
    format!("study_title=\"*{escaped}*\" OR study_description=\"*{escaped}*\"")
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

fn escape_ena_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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
    let upper = accession.to_ascii_uppercase();
    upper.starts_with("PRJ")
        || upper.starts_with("ERP")
        || upper.starts_with("SRP")
        || upper.starts_with("DRP")
        || upper.starts_with("ERX")
        || upper.starts_with("SRX")
        || upper.starts_with("DRX")
        || upper.starts_with("ERR")
        || upper.starts_with("SRR")
        || upper.starts_with("DRR")
        || upper.starts_with("ERS")
        || upper.starts_with("SRS")
        || upper.starts_with("DRS")
        || upper.starts_with("SAM")
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
            "study_description": "Rumen samples",
            "center_name": "EBI",
            "scientific_name": "cow metagenome",
            "first_public": "2024-01-01",
            "last_updated": "2024-02-01"
        }]);
        let records = parse_ena_portal_json(&value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "PRJEB123");
        assert_eq!(records[0].accession, "ERP123");
        assert_eq!(records[0].organism.as_deref(), Some("cow metagenome"));
        assert!(records[0].url.ends_with("/PRJEB123"));
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
        let record = parse_ena_xml_record(xml, "fallback").unwrap();
        assert_eq!(record.accession, "PRJEB999");
        assert_eq!(record.title, "XML Study");
        assert_eq!(record.summary, "XML abstract & details.");
        assert_eq!(record.files, vec!["ftp://example/file.fastq.gz"]);
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
    }
}
