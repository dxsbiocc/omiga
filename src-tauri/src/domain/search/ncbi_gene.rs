//! NCBI Gene adapter for structured `query(category="knowledge")`.
//!
//! Uses official NCBI Entrez E-utilities (`db=gene`) for both search and
//! record retrieval.  This is intentionally separate from PubMed parsing:
//! PubMed records are article-shaped, while Gene summaries are identifier and
//! annotation records.

use crate::domain::tools::{ToolContext, WebSearchApiKeys};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as Json};
use std::time::Duration;

const DEFAULT_EUTILS_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const DEFAULT_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_TOOL: &str = "omiga";
const NCBI_FAVICON: &str = "https://www.ncbi.nlm.nih.gov/favicon.ico";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GeneSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default)]
    pub organism: Option<String>,
    #[serde(default, alias = "taxid", alias = "tax_id")]
    pub taxon_id: Option<String>,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
    #[serde(default, alias = "retstart")]
    pub ret_start: Option<u32>,
    #[serde(default)]
    pub sort: Option<String>,
}

impl GeneSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }

    pub fn normalized_ret_start(&self) -> u32 {
        self.ret_start.unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneSearchResponse {
    pub query: String,
    pub effective_query: String,
    pub count: u64,
    pub ret_start: u32,
    pub ret_max: u32,
    pub query_translation: Option<String>,
    pub ids: Vec<String>,
    pub records: Vec<GeneRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeneRecord {
    pub gene_id: String,
    pub symbol: String,
    pub description: String,
    pub organism: Option<String>,
    pub tax_id: Option<String>,
    pub chromosome: Option<String>,
    pub map_location: Option<String>,
    pub aliases: Vec<String>,
    pub other_designations: Vec<String>,
    pub summary: Option<String>,
    pub status: Option<String>,
    pub nomenclature_symbol: Option<String>,
    pub nomenclature_name: Option<String>,
    pub nomenclature_status: Option<String>,
    pub mim: Vec<String>,
    pub genomic_locations: Vec<GeneGenomicLocation>,
    pub raw_summary: Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneGenomicLocation {
    pub accession_version: Option<String>,
    pub start: Option<i64>,
    pub stop: Option<i64>,
    pub orientation: Option<String>,
}

#[derive(Clone)]
pub struct NcbiGeneClient {
    http: reqwest::Client,
    base_url: String,
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
            api_key: clean_optional(keys.pubmed_api_key.as_deref()),
            email: clean_optional(keys.pubmed_email.as_deref())
                .unwrap_or_else(|| DEFAULT_EMAIL.to_string()),
            tool: clean_optional(keys.pubmed_tool_name.as_deref())
                .unwrap_or_else(|| DEFAULT_TOOL.to_string()),
        }
    }
}

impl NcbiGeneClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-NCBI-Gene/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build NCBI Gene HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: DEFAULT_EUTILS_BASE_URL.to_string(),
            settings: EntrezSettings::from_keys(&ctx.web_search_api_keys),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("Omiga-NCBI-Gene/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("build NCBI Gene HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            settings: EntrezSettings {
                api_key: None,
                email: DEFAULT_EMAIL.to_string(),
                tool: DEFAULT_TOOL.to_string(),
            },
        })
    }

    pub async fn search(&self, args: GeneSearchArgs) -> Result<GeneSearchResponse, String> {
        if args.query.trim().is_empty() {
            return Err("NCBI Gene query must not be empty".to_string());
        }
        let ret_max = args.normalized_max_results();
        let ret_start = args.normalized_ret_start();
        let effective_query = build_gene_query(&args);

        let mut params = self.common_entrez_params("json");
        params.push(("term".to_string(), effective_query.clone()));
        params.push(("retmax".to_string(), ret_max.to_string()));
        params.push(("retstart".to_string(), ret_start.to_string()));
        params.push((
            "sort".to_string(),
            clean_optional(args.sort.as_deref()).unwrap_or_else(|| "relevance".to_string()),
        ));

        let search_json = self.get_json("esearch", &params).await?;
        let search = parse_esearch_result(
            &args.query,
            &effective_query,
            ret_start,
            ret_max,
            &search_json,
        )?;

        if search.ids.is_empty() {
            return Ok(search);
        }

        let mut summary_params = self.common_entrez_params("json");
        summary_params.push(("id".to_string(), search.ids.join(",")));
        let summary_json = self.get_json("esummary", &summary_params).await?;
        let records = parse_esummary_result(&summary_json, &search.ids);

        Ok(GeneSearchResponse { records, ..search })
    }

    pub async fn fetch_by_gene_id(&self, gene_id: &str) -> Result<GeneRecord, String> {
        let gene_id = gene_id.trim();
        if gene_id.is_empty() || !gene_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(
                "NCBI Gene fetch expects a numeric Gene ID; use operation=search for symbols"
                    .to_string(),
            );
        }
        let mut params = self.common_entrez_params("json");
        params.push(("id".to_string(), gene_id.to_string()));
        let summary_json = self.get_json("esummary", &params).await?;
        let records = parse_esummary_result(&summary_json, &[gene_id.to_string()]);
        records
            .into_iter()
            .next()
            .ok_or_else(|| format!("NCBI Gene did not return a summary for Gene ID {gene_id}"))
    }

    fn common_entrez_params(&self, retmode: &str) -> Vec<(String, String)> {
        let mut params = vec![
            ("db".to_string(), "gene".to_string()),
            ("retmode".to_string(), retmode.to_string()),
            ("tool".to_string(), self.settings.tool.clone()),
            ("email".to_string(), self.settings.email.clone()),
        ];
        if let Some(api_key) = &self.settings.api_key {
            params.push(("api_key".to_string(), api_key.clone()));
        }
        params
    }

    async fn get_json(&self, utility: &str, params: &[(String, String)]) -> Result<Json, String> {
        let body = self.get_text(utility, params).await?;
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

    async fn get_text(&self, utility: &str, params: &[(String, String)]) -> Result<String, String> {
        let url = format!("{}/{}.fcgi", self.base_url, utility);
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
        Ok(body)
    }
}

pub fn search_response_to_json(response: &GeneSearchResponse) -> Json {
    let results: Vec<Json> = response
        .records
        .iter()
        .enumerate()
        .map(|(idx, item)| gene_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "effective_query": response.effective_query,
        "category": "knowledge",
        "source": "ncbi_gene",
        "effective_source": "ncbi_gene",
        "count": response.count,
        "ret_start": response.ret_start,
        "ret_max": response.ret_max,
        "query_translation": response.query_translation,
        "ids": response.ids,
        "results": results,
    })
}

pub fn detail_to_json(record: &GeneRecord) -> Json {
    json!({
        "category": "knowledge",
        "source": "ncbi_gene",
        "effective_source": "ncbi_gene",
        "id": record.gene_id,
        "gene_id": record.gene_id,
        "title": gene_title(record),
        "name": record.symbol,
        "link": gene_url(&record.gene_id),
        "url": gene_url(&record.gene_id),
        "displayed_link": format!("ncbi.nlm.nih.gov/gene/{}", record.gene_id),
        "favicon": NCBI_FAVICON,
        "snippet": gene_snippet(record),
        "content": gene_content(record),
        "metadata": gene_metadata(record),
    })
}

fn gene_to_serp_result(record: &GeneRecord, position: usize) -> Json {
    json!({
        "position": position,
        "category": "knowledge",
        "source": "ncbi_gene",
        "title": gene_title(record),
        "name": record.symbol,
        "link": gene_url(&record.gene_id),
        "url": gene_url(&record.gene_id),
        "displayed_link": format!("ncbi.nlm.nih.gov/gene/{}", record.gene_id),
        "favicon": NCBI_FAVICON,
        "snippet": gene_snippet(record),
        "id": record.gene_id,
        "gene_id": record.gene_id,
        "metadata": gene_metadata(record),
    })
}

fn gene_metadata(record: &GeneRecord) -> Json {
    json!({
        "gene_id": record.gene_id,
        "symbol": record.symbol,
        "description": record.description,
        "organism": record.organism,
        "tax_id": record.tax_id,
        "chromosome": record.chromosome,
        "map_location": record.map_location,
        "aliases": record.aliases,
        "other_designations": record.other_designations,
        "summary": record.summary,
        "status": record.status,
        "nomenclature_symbol": record.nomenclature_symbol,
        "nomenclature_name": record.nomenclature_name,
        "nomenclature_status": record.nomenclature_status,
        "mim": record.mim,
        "genomic_locations": record.genomic_locations,
        "source_specific": record.raw_summary,
    })
}

fn build_gene_query(args: &GeneSearchArgs) -> String {
    let mut term = args.query.trim().to_string();
    if let Some(taxon_id) = clean_optional(args.taxon_id.as_deref()) {
        if !contains_case_insensitive(&term, "[organism")
            && !contains_case_insensitive(&term, "txid")
        {
            term.push_str(&format!(" AND txid{taxon_id}[Organism:exp]"));
        }
    } else if let Some(organism) = clean_optional(args.organism.as_deref()) {
        if !contains_case_insensitive(&term, "[organism") {
            term.push_str(&format!(" AND {organism}[Organism]"));
        }
    }
    term
}

fn parse_esearch_result(
    query: &str,
    effective_query: &str,
    ret_start: u32,
    ret_max: u32,
    value: &Json,
) -> Result<GeneSearchResponse, String> {
    let root = value
        .get("esearchresult")
        .and_then(Json::as_object)
        .ok_or_else(|| "NCBI Gene ESearch response missing esearchresult".to_string())?;

    if let Some(error) = root.get("error").and_then(Json::as_str) {
        return Err(format!("NCBI Gene ESearch error: {error}"));
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

    Ok(GeneSearchResponse {
        query: query.to_string(),
        effective_query: effective_query.to_string(),
        count,
        ret_start,
        ret_max,
        query_translation,
        ids,
        records: Vec::new(),
    })
}

fn parse_esummary_result(value: &Json, ordered_ids: &[String]) -> Vec<GeneRecord> {
    let Some(result) = value.get("result").and_then(Json::as_object) else {
        return Vec::new();
    };

    let ordered = if ordered_ids.is_empty() {
        result
            .get("uids")
            .and_then(Json::as_array)
            .map(|uids| {
                uids.iter()
                    .filter_map(Json::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        ordered_ids.to_vec()
    };

    ordered
        .iter()
        .filter_map(|gene_id| {
            result
                .get(gene_id)
                .and_then(|doc| parse_gene_summary_doc(gene_id, doc))
        })
        .collect()
}

fn parse_gene_summary_doc(gene_id: &str, doc: &Json) -> Option<GeneRecord> {
    let map = doc.as_object()?;
    let gene_id = string_field(map, "uid").unwrap_or_else(|| gene_id.to_string());
    let symbol = string_field(map, "name")
        .or_else(|| string_field(map, "nomenclaturesymbol"))
        .unwrap_or_else(|| gene_id.clone());
    let description = string_field(map, "description")
        .or_else(|| string_field(map, "nomenclaturename"))
        .unwrap_or_default();
    let organism_value = map.get("organism");
    let organism = organism_value
        .and_then(|v| v.get("scientificname"))
        .and_then(Json::as_str)
        .or_else(|| {
            organism_value
                .and_then(|v| v.get("commonname"))
                .and_then(Json::as_str)
        })
        .map(str::to_string);
    let tax_id = organism_value
        .and_then(|v| v.get("taxid"))
        .and_then(json_string_from_string_or_number);

    Some(GeneRecord {
        gene_id,
        symbol,
        description,
        organism,
        tax_id,
        chromosome: string_field(map, "chromosome"),
        map_location: string_field(map, "maplocation"),
        aliases: split_aliases(map.get("otheraliases")),
        other_designations: split_designations(map.get("otherdesignations")),
        summary: string_field(map, "summary"),
        status: string_field(map, "status"),
        nomenclature_symbol: string_field(map, "nomenclaturesymbol"),
        nomenclature_name: string_field(map, "nomenclaturename"),
        nomenclature_status: string_field(map, "nomenclaturestatus"),
        mim: json_string_list(map.get("mim")),
        genomic_locations: parse_genomic_locations(map.get("genomicinfo")),
        raw_summary: doc.clone(),
    })
}

fn parse_genomic_locations(value: Option<&Json>) -> Vec<GeneGenomicLocation> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let map = item.as_object()?;
                    Some(GeneGenomicLocation {
                        accession_version: string_field(map, "chraccver")
                            .or_else(|| string_field(map, "accessionversion")),
                        start: map.get("chrstart").and_then(json_i64_from_string_or_number),
                        stop: map.get("chrstop").and_then(json_i64_from_string_or_number),
                        orientation: string_field(map, "orientation"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn gene_title(record: &GeneRecord) -> String {
    if record.description.trim().is_empty() {
        record.symbol.clone()
    } else {
        format!("{} — {}", record.symbol, record.description)
    }
}

fn gene_url(gene_id: &str) -> String {
    format!("https://www.ncbi.nlm.nih.gov/gene/{gene_id}")
}

fn gene_snippet(record: &GeneRecord) -> String {
    let mut pieces = Vec::new();
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(organism.to_string());
    }
    if let Some(chromosome) = record
        .chromosome
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(format!("chr {chromosome}"));
    }
    if let Some(location) = record
        .map_location
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(location.to_string());
    }
    if let Some(summary) = record.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(truncate_chars(summary, 280));
    }
    pieces.join(" | ")
}

fn gene_content(record: &GeneRecord) -> String {
    let mut out = String::new();
    out.push_str(&gene_title(record));
    out.push_str("\n\n");
    out.push_str(&format!("Gene ID: {}\n", record.gene_id));
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("Organism: {organism}\n"));
    }
    if let Some(tax_id) = record.tax_id.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("Taxonomy ID: {tax_id}\n"));
    }
    if let Some(chromosome) = record
        .chromosome
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str(&format!("Chromosome: {chromosome}\n"));
    }
    if let Some(location) = record
        .map_location
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str(&format!("Map location: {location}\n"));
    }
    if !record.aliases.is_empty() {
        out.push_str(&format!("Aliases: {}\n", record.aliases.join(", ")));
    }
    out.push_str(&format!("Link: {}\n", gene_url(&record.gene_id)));
    if let Some(summary) = record.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("\nSummary:\n");
        out.push_str(summary.trim());
    }
    out
}

fn string_field(map: &JsonMap<String, Json>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn split_aliases(value: Option<&Json>) -> Vec<String> {
    let Some(value) = value.and_then(Json::as_str) else {
        return Vec::new();
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_designations(value: Option<&Json>) -> Vec<String> {
    let Some(value) = value.and_then(Json::as_str) else {
        return Vec::new();
    };
    value
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn json_string_list(value: Option<&Json>) -> Vec<String> {
    match value {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(json_string_from_string_or_number)
            .collect(),
        Some(value) => json_string_from_string_or_number(value)
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

fn json_string_from_string_or_number(value: &Json) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_u64().map(|v| v.to_string()))
        .or_else(|| value.as_i64().map(|v| v.to_string()))
}

fn json_u64_from_string_or_number(value: &Json) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn json_i64_from_string_or_number(value: &Json) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_organism_filtered_gene_query() {
        let args = GeneSearchArgs {
            query: "TP53".to_string(),
            organism: Some("Homo sapiens".to_string()),
            taxon_id: None,
            max_results: None,
            ret_start: None,
            sort: None,
        };

        assert_eq!(build_gene_query(&args), "TP53 AND Homo sapiens[Organism]");
    }

    #[test]
    fn taxon_filter_takes_precedence_over_organism() {
        let args = GeneSearchArgs {
            query: "Trp53".to_string(),
            organism: Some("Mus musculus".to_string()),
            taxon_id: Some("10090".to_string()),
            max_results: None,
            ret_start: None,
            sort: None,
        };

        assert_eq!(build_gene_query(&args), "Trp53 AND txid10090[Organism:exp]");
    }

    #[test]
    fn parses_gene_esearch_and_esummary_json() {
        let search_json = json!({
            "esearchresult": {
                "count": "1",
                "idlist": ["7157"],
                "querytranslation": "TP53[All Fields]"
            }
        });
        let search = parse_esearch_result("TP53", "TP53", 0, 1, &search_json).unwrap();
        assert_eq!(search.ids, vec!["7157"]);
        assert_eq!(search.count, 1);

        let summary_json = json!({
            "result": {
                "uids": ["7157"],
                "7157": {
                    "uid": "7157",
                    "name": "TP53",
                    "description": "tumor protein p53",
                    "summary": "This gene encodes tumor protein p53.",
                    "chromosome": "17",
                    "maplocation": "17p13.1",
                    "otheraliases": "BCC7,LFS1,P53",
                    "otherdesignations": "cellular tumor antigen p53|antigen NY-CO-13",
                    "organism": {
                        "scientificname": "Homo sapiens",
                        "taxid": 9606
                    },
                    "genomicinfo": [{
                        "chraccver": "NC_000017.11",
                        "chrstart": 7661779,
                        "chrstop": 7687550
                    }]
                }
            }
        });
        let records = parse_esummary_result(&summary_json, &search.ids);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].gene_id, "7157");
        assert_eq!(records[0].symbol, "TP53");
        assert_eq!(records[0].organism.as_deref(), Some("Homo sapiens"));
        assert_eq!(records[0].tax_id.as_deref(), Some("9606"));
        assert_eq!(records[0].aliases, vec!["BCC7", "LFS1", "P53"]);
    }
}
