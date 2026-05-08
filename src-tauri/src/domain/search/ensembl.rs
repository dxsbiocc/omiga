//! Ensembl REST adapter for structured `query(category="knowledge")`.
//!
//! Uses the public Ensembl REST API (`rest.ensembl.org`) for gene/transcript
//! symbol lookup, stable-id lookup, and dbSNP/variation retrieval. No API key
//! is required.

use crate::domain::tools::ToolContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as Json};
use std::fmt::Write as _;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://rest.ensembl.org";
const DEFAULT_SPECIES: &str = "homo_sapiens";
const DEFAULT_OBJECT_TYPE: &str = "gene";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const ENSEMBL_FAVICON: &str = "https://www.ensembl.org/favicon.ico";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EnsemblSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default, alias = "organism")]
    pub species: Option<String>,
    #[serde(default, alias = "type")]
    pub object_type: Option<String>,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "size")]
    pub max_results: Option<u32>,
}

impl EnsemblSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }

    fn species(&self) -> String {
        normalize_species(self.species.as_deref())
    }

    fn object_type(&self) -> String {
        normalize_object_type(self.object_type.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnsemblSearchResponse {
    pub query: String,
    pub species: String,
    pub object_type: String,
    pub results: Vec<EnsemblRecord>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnsemblRecord {
    pub id: String,
    pub record_type: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub species: Option<String>,
    pub biotype: Option<String>,
    pub seq_region_name: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub strand: Option<i64>,
    pub assembly_name: Option<String>,
    pub version: Option<u64>,
    pub source: Option<String>,
    pub canonical_transcript: Option<String>,
    pub parent: Option<String>,
    pub synonyms: Vec<String>,
    pub mappings: Vec<EnsemblVariantMapping>,
    pub evidence: Vec<String>,
    pub minor_allele: Option<String>,
    pub maf: Option<f64>,
    pub most_severe_consequence: Option<String>,
    pub raw_entry: Json,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnsemblVariantMapping {
    pub seq_region_name: Option<String>,
    pub location: Option<String>,
    pub allele_string: Option<String>,
    pub assembly_name: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub strand: Option<i64>,
}

#[derive(Clone)]
pub struct EnsemblClient {
    http: reqwest::Client,
    base_url: String,
}

impl EnsemblClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-Ensembl/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build Ensembl HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("Omiga-Ensembl/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("build Ensembl HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        })
    }

    pub async fn search(&self, args: EnsemblSearchArgs) -> Result<EnsemblSearchResponse, String> {
        let query = args.query.trim();
        if query.is_empty() {
            return Err("Ensembl query must not be empty".to_string());
        }
        let species = args.species();
        let object_type = args.object_type();
        let max_results = args.normalized_max_results();

        if let Some(variant_id) = normalize_variant_id(query) {
            let record = self.fetch_variant(&species, &variant_id).await?;
            return Ok(EnsemblSearchResponse {
                query: query.to_string(),
                species,
                object_type: "variant".to_string(),
                results: vec![record],
                notes: Vec::new(),
            });
        }
        if let Some(stable_id) = normalize_stable_id(query) {
            let record = self.fetch_stable_id(&stable_id).await?;
            return Ok(EnsemblSearchResponse {
                query: query.to_string(),
                species,
                object_type,
                results: vec![record],
                notes: Vec::new(),
            });
        }

        let symbol = clean_optional(Some(query)).ok_or_else(|| {
            "Ensembl query must be a gene symbol, Ensembl stable ID, or rsID".to_string()
        })?;
        let path = format!(
            "/xrefs/symbol/{}/{}",
            encode_path_segment(&species),
            encode_path_segment(&symbol)
        );
        let params = vec![("object_type".to_string(), object_type.clone())];
        let json = self.get_json(&path, &params).await?;
        let mut ids = parse_xref_ids(&json, &object_type);
        ids.truncate(max_results as usize);

        let mut notes = Vec::new();
        let mut results = Vec::new();
        if ids.is_empty() {
            let path = format!(
                "/lookup/symbol/{}/{}",
                encode_path_segment(&species),
                encode_path_segment(&symbol)
            );
            let json = self
                .get_json(&path, &[("expand".to_string(), "0".to_string())])
                .await?;
            if let Some(record) = parse_ensembl_record(&json) {
                results.push(record);
            }
        } else {
            for id in ids {
                match self.fetch_stable_id(&id).await {
                    Ok(record) => results.push(record),
                    Err(message) => notes.push(format!("Skipped {id}: {message}")),
                }
            }
        }

        Ok(EnsemblSearchResponse {
            query: query.to_string(),
            species,
            object_type,
            results,
            notes,
        })
    }

    pub async fn fetch(
        &self,
        identifier: &str,
        species: Option<&str>,
    ) -> Result<EnsemblRecord, String> {
        if let Some(variant_id) = normalize_variant_id(identifier) {
            return self
                .fetch_variant(&normalize_species(species), &variant_id)
                .await;
        }
        if let Some(stable_id) = normalize_stable_id(identifier) {
            return self.fetch_stable_id(&stable_id).await;
        }
        let symbol = normalize_symbol(identifier).ok_or_else(|| {
            "Ensembl fetch expects an Ensembl stable ID, rsID, symbol, or Ensembl URL".to_string()
        })?;
        let species = normalize_species(species);
        let path = format!(
            "/lookup/symbol/{}/{}",
            encode_path_segment(&species),
            encode_path_segment(&symbol)
        );
        let json = self
            .get_json(&path, &[("expand".to_string(), "0".to_string())])
            .await?;
        parse_ensembl_record(&json)
            .ok_or_else(|| format!("Ensembl did not return a record for symbol {symbol}"))
    }

    async fn fetch_stable_id(&self, id: &str) -> Result<EnsemblRecord, String> {
        let path = format!("/lookup/id/{}", encode_path_segment(id));
        let json = self
            .get_json(&path, &[("expand".to_string(), "0".to_string())])
            .await?;
        parse_ensembl_record(&json)
            .ok_or_else(|| format!("Ensembl did not return a record for ID {id}"))
    }

    async fn fetch_variant(&self, species: &str, id: &str) -> Result<EnsemblRecord, String> {
        let path = format!(
            "/variation/{}/{}",
            encode_path_segment(species),
            encode_path_segment(id)
        );
        let json = self.get_json(&path, &[]).await?;
        parse_ensembl_record(&json)
            .ok_or_else(|| format!("Ensembl did not return a variation record for {id}"))
    }

    async fn get_json(&self, path: &str, params: &[(String, String)]) -> Result<Json, String> {
        #[cfg(test)]
        if self.base_url == "mock://ensembl" {
            return mock_ensembl_json(path, params);
        }

        let url = format!("{}{}", self.base_url, path);
        let response = self
            .http
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/json")
            .query(params)
            .send()
            .await
            .map_err(|e| format!("Ensembl request failed: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("Ensembl response read failed: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "Ensembl returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }

        serde_json::from_str(&body).map_err(|e| {
            format!(
                "Ensembl returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })
    }
}

pub fn search_response_to_json(response: &EnsemblSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| ensembl_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "species": response.species,
        "object_type": response.object_type,
        "category": "knowledge",
        "source": "ensembl",
        "effective_source": "ensembl",
        "results": results,
        "notes": response.notes,
    })
}

pub fn detail_to_json(record: &EnsemblRecord) -> Json {
    json!({
        "category": "knowledge",
        "source": "ensembl",
        "effective_source": "ensembl",
        "id": record.id,
        "accession": record.id,
        "title": ensembl_title(record),
        "name": record.display_name.as_deref().unwrap_or(&record.id),
        "link": ensembl_url(record),
        "url": ensembl_url(record),
        "displayed_link": displayed_link(&ensembl_url(record)),
        "favicon": ENSEMBL_FAVICON,
        "snippet": ensembl_snippet(record),
        "content": ensembl_content(record),
        "metadata": ensembl_metadata(record),
    })
}

fn ensembl_to_serp_result(record: &EnsemblRecord, position: usize) -> Json {
    json!({
        "position": position,
        "category": "knowledge",
        "source": "ensembl",
        "title": ensembl_title(record),
        "name": record.display_name.as_deref().unwrap_or(&record.id),
        "link": ensembl_url(record),
        "url": ensembl_url(record),
        "displayed_link": displayed_link(&ensembl_url(record)),
        "favicon": ENSEMBL_FAVICON,
        "snippet": ensembl_snippet(record),
        "id": record.id,
        "accession": record.id,
        "record_type": record.record_type,
        "metadata": ensembl_metadata(record),
    })
}

fn ensembl_metadata(record: &EnsemblRecord) -> Json {
    json!({
        "id": record.id,
        "record_type": record.record_type,
        "display_name": record.display_name,
        "description": record.description,
        "species": record.species,
        "biotype": record.biotype,
        "seq_region_name": record.seq_region_name,
        "start": record.start,
        "end": record.end,
        "strand": record.strand,
        "assembly_name": record.assembly_name,
        "version": record.version,
        "source": record.source,
        "canonical_transcript": record.canonical_transcript,
        "parent": record.parent,
        "synonyms": record.synonyms,
        "mappings": record.mappings,
        "evidence": record.evidence,
        "minor_allele": record.minor_allele,
        "maf": record.maf,
        "most_severe_consequence": record.most_severe_consequence,
        "source_specific": record.raw_entry,
    })
}

fn parse_xref_ids(value: &Json, object_type: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(items) = value.as_array() else {
        return out;
    };
    for item in items {
        let Some(id) = item.get("id").and_then(Json::as_str) else {
            continue;
        };
        if let Some(kind) = item.get("type").and_then(Json::as_str) {
            if !kind.eq_ignore_ascii_case(object_type) {
                continue;
            }
        }
        push_unique(&mut out, id);
    }
    out
}

fn parse_ensembl_record(value: &Json) -> Option<EnsemblRecord> {
    let map = value.as_object()?;
    let id = string_field(map, "id").or_else(|| string_field(map, "name"))?;
    let record_type = string_field(map, "object_type")
        .or_else(|| string_field(map, "var_class").map(|_| "Variant".to_string()))
        .unwrap_or_else(|| infer_record_type(&id).to_string());
    let mappings = parse_variant_mappings(map.get("mappings"));
    let primary_mapping = mappings.first();

    Some(EnsemblRecord {
        id,
        record_type,
        display_name: string_field(map, "display_name").or_else(|| string_field(map, "name")),
        description: string_field(map, "description").or_else(|| string_field(map, "source")),
        species: string_field(map, "species"),
        biotype: string_field(map, "biotype").or_else(|| string_field(map, "var_class")),
        seq_region_name: string_field(map, "seq_region_name")
            .or_else(|| primary_mapping.and_then(|mapping| mapping.seq_region_name.clone())),
        start: i64_field(map, "start").or_else(|| primary_mapping.and_then(|m| m.start)),
        end: i64_field(map, "end").or_else(|| primary_mapping.and_then(|m| m.end)),
        strand: i64_field(map, "strand").or_else(|| primary_mapping.and_then(|m| m.strand)),
        assembly_name: string_field(map, "assembly_name")
            .or_else(|| primary_mapping.and_then(|mapping| mapping.assembly_name.clone())),
        version: u64_field(map, "version"),
        source: string_field(map, "source"),
        canonical_transcript: string_field(map, "canonical_transcript"),
        parent: string_field(map, "Parent").or_else(|| string_field(map, "parent")),
        synonyms: json_string_list(map.get("synonyms")),
        mappings,
        evidence: json_string_list(map.get("evidence")),
        minor_allele: string_field(map, "minor_allele"),
        maf: f64_field(map, "MAF").or_else(|| f64_field(map, "maf")),
        most_severe_consequence: string_field(map, "most_severe_consequence"),
        raw_entry: value.clone(),
    })
}

fn parse_variant_mappings(value: Option<&Json>) -> Vec<EnsemblVariantMapping> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let map = item.as_object()?;
                    Some(EnsemblVariantMapping {
                        seq_region_name: string_field(map, "seq_region_name"),
                        location: string_field(map, "location"),
                        allele_string: string_field(map, "allele_string"),
                        assembly_name: string_field(map, "assembly_name"),
                        start: i64_field(map, "start"),
                        end: i64_field(map, "end"),
                        strand: i64_field(map, "strand"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ensembl_title(record: &EnsemblRecord) -> String {
    match record
        .display_name
        .as_deref()
        .filter(|name| *name != record.id)
    {
        Some(name) => format!("{name} ({}) — {}", record.id, record.record_type),
        None => format!("{} — {}", record.id, record.record_type),
    }
}

fn ensembl_snippet(record: &EnsemblRecord) -> String {
    let mut pieces = Vec::new();
    if let Some(species) = record.species.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(species.to_string());
    }
    if let Some(biotype) = record.biotype.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(biotype.to_string());
    }
    if let Some(location) = ensembl_location(record) {
        pieces.push(location);
    }
    if let Some(consequence) = record
        .most_severe_consequence
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(consequence.to_string());
    }
    if let Some(description) = record
        .description
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(truncate_chars(description, 220));
    }
    pieces.join(" | ")
}

fn ensembl_content(record: &EnsemblRecord) -> String {
    let mut out = String::new();
    out.push_str(&ensembl_title(record));
    out.push_str("\n\n");
    out.push_str(&format!("ID: {}\n", record.id));
    out.push_str(&format!("Type: {}\n", record.record_type));
    if let Some(display_name) = record
        .display_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str(&format!("Name: {display_name}\n"));
    }
    if let Some(species) = record.species.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("Species: {species}\n"));
    }
    if let Some(biotype) = record.biotype.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("Biotype/Class: {biotype}\n"));
    }
    if let Some(location) = ensembl_location(record) {
        out.push_str(&format!("Location: {location}\n"));
    }
    if let Some(canonical) = record
        .canonical_transcript
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str(&format!("Canonical transcript: {canonical}\n"));
    }
    if !record.evidence.is_empty() {
        out.push_str(&format!("Evidence: {}\n", record.evidence.join(", ")));
    }
    if let Some(consequence) = record
        .most_severe_consequence
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str(&format!("Most severe consequence: {consequence}\n"));
    }
    out.push_str(&format!("Link: {}\n", ensembl_url(record)));
    if let Some(description) = record
        .description
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("\nDescription:\n");
        out.push_str(description.trim());
    }
    out
}

fn ensembl_location(record: &EnsemblRecord) -> Option<String> {
    let region = record.seq_region_name.as_deref()?.trim();
    if region.is_empty() {
        return None;
    }
    match (record.start, record.end) {
        (Some(start), Some(end)) => Some(format!("{}:{start}-{end}", region)),
        _ => Some(region.to_string()),
    }
}

fn ensembl_url(record: &EnsemblRecord) -> String {
    let species = record
        .species
        .as_deref()
        .map(species_site_path)
        .unwrap_or_else(|| species_site_path(DEFAULT_SPECIES));
    let record_type = record.record_type.to_ascii_lowercase();
    let id = encode_query_component(&record.id);
    if record_type.contains("variant") || record.id.to_ascii_lowercase().starts_with("rs") {
        format!("https://www.ensembl.org/{species}/Variation/Explore?v={id}")
    } else if record_type.contains("transcript") || record.id.to_ascii_uppercase().contains('T') {
        format!("https://www.ensembl.org/{species}/Transcript/Summary?t={id}")
    } else if record_type.contains("translation")
        || record_type.contains("protein")
        || record.id.to_ascii_uppercase().contains('P')
    {
        format!("https://www.ensembl.org/{species}/Transcript/ProteinSummary?p={id}")
    } else {
        format!("https://www.ensembl.org/{species}/Gene/Summary?g={id}")
    }
}

fn displayed_link(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

fn normalize_species(value: Option<&str>) -> String {
    value
        .and_then(|value| clean_optional(Some(value)))
        .map(|value| value.to_ascii_lowercase().replace([' ', '-'], "_"))
        .unwrap_or_else(|| DEFAULT_SPECIES.to_string())
}

fn normalize_object_type(value: Option<&str>) -> String {
    match value
        .and_then(|value| clean_optional(Some(value)))
        .map(|value| value.to_ascii_lowercase().replace([' ', '-'], "_"))
        .as_deref()
    {
        Some("transcript") | Some("transcripts") => "transcript".to_string(),
        Some("translation") | Some("translations") | Some("protein") | Some("proteins") => {
            "translation".to_string()
        }
        Some("variant") | Some("variation") | Some("variants") | Some("rsid") => {
            "variant".to_string()
        }
        _ => DEFAULT_OBJECT_TYPE.to_string(),
    }
}

fn normalize_symbol(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains("://") {
        return None;
    }
    let value = value
        .trim_start_matches("symbol:")
        .trim_start_matches("Symbol:")
        .trim();
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
        && value.chars().any(|c| c.is_ascii_alphabetic())
    {
        Some(value.to_string())
    } else {
        None
    }
}

fn normalize_stable_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    for token in candidate_tokens(value) {
        let upper = token.to_ascii_uppercase();
        if is_ensembl_stable_id(&upper) {
            return Some(upper);
        }
    }
    None
}

fn normalize_variant_id(value: &str) -> Option<String> {
    for token in candidate_tokens(value) {
        let token = token.trim();
        let lower = token.to_ascii_lowercase();
        if lower.starts_with("rs")
            && lower.len() > 2
            && lower[2..].chars().all(|c| c.is_ascii_digit())
        {
            return Some(format!("rs{}", &lower[2..]));
        }
    }
    None
}

fn candidate_tokens(value: &str) -> Vec<String> {
    value
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '/' | '?'
                        | '&'
                        | '='
                        | '#'
                        | ','
                        | ';'
                        | '"'
                        | '\''
                        | '<'
                        | '>'
                        | '['
                        | ']'
                        | '('
                        | ')'
                )
        })
        .map(|token| {
            token
                .trim()
                .trim_start_matches("Ensembl:")
                .trim_start_matches("ensembl:")
                .trim_end_matches(".json")
                .to_string()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn is_ensembl_stable_id(value: &str) -> bool {
    let core = value.split('.').next().unwrap_or(value);
    if !core.starts_with("ENS") {
        return false;
    }
    let Some(idx) = core.find(|c: char| c.is_ascii_digit()) else {
        return false;
    };
    idx >= 4 && core[idx..].chars().all(|c| c.is_ascii_digit())
}

fn infer_record_type(id: &str) -> &'static str {
    let upper = id.to_ascii_uppercase();
    if upper.starts_with("RS") {
        "Variant"
    } else if upper.contains('T') {
        "Transcript"
    } else if upper.contains('P') {
        "Translation"
    } else {
        "Gene"
    }
}

fn species_site_path(species: &str) -> String {
    let mut parts = Vec::new();
    for (idx, part) in species
        .split('_')
        .filter(|part| !part.is_empty())
        .enumerate()
    {
        if idx == 0 {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                parts.push(format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ));
            }
        } else {
            parts.push(part.to_ascii_lowercase());
        }
    }
    if parts.is_empty() {
        "Homo_sapiens".to_string()
    } else {
        parts.join("_")
    }
}

fn encode_path_segment(value: &str) -> String {
    percent_encode(value, false)
}

fn encode_query_component(value: &str) -> String {
    percent_encode(value, true)
}

fn percent_encode(value: &str, space_as_plus: bool) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'~' | b':') {
            out.push(byte as char);
        } else if byte == b' ' && space_as_plus {
            out.push('+');
        } else {
            let _ = write!(&mut out, "%{byte:02X}");
        }
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

fn i64_field(map: &JsonMap<String, Json>, key: &str) -> Option<i64> {
    let value = map.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))
        .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
}

fn u64_field(map: &JsonMap<String, Json>, key: &str) -> Option<u64> {
    let value = map.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

fn f64_field(map: &JsonMap<String, Json>, key: &str) -> Option<f64> {
    let value = map.get(key)?;
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn json_string_list(value: Option<&Json>) -> Vec<String> {
    match value {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        Some(value) => value.as_str().map(str::to_string).into_iter().collect(),
        None => Vec::new(),
    }
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !out.iter().any(|item| item == value) {
        out.push(value.to_string());
    }
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
fn mock_ensembl_json(path: &str, _params: &[(String, String)]) -> Result<Json, String> {
    match path {
        "/xrefs/symbol/homo_sapiens/BRCA2" => Ok(json!([
            {"id": "ENSG00000139618", "type": "gene"}
        ])),
        "/lookup/symbol/homo_sapiens/BRCA2" | "/lookup/id/ENSG00000139618" => Ok(brca2_fixture()),
        "/variation/homo_sapiens/rs56116432" => Ok(variant_fixture()),
        other => Err(format!("unhandled mock Ensembl path: {other}")),
    }
}

#[cfg(test)]
fn brca2_fixture() -> Json {
    json!({
        "id": "ENSG00000139618",
        "end": 32400268,
        "db_type": "core",
        "start": 32315086,
        "species": "homo_sapiens",
        "canonical_transcript": "ENST00000380152.8",
        "object_type": "Gene",
        "version": 19,
        "assembly_name": "GRCh38",
        "strand": 1,
        "display_name": "BRCA2",
        "seq_region_name": "13",
        "source": "ensembl_havana",
        "description": "BRCA2 DNA repair associated [Source:HGNC Symbol;Acc:HGNC:1101]",
        "biotype": "protein_coding"
    })
}

#[cfg(test)]
fn variant_fixture() -> Json {
    json!({
        "synonyms": [],
        "source": "Variants (including SNPs and indels) imported from dbSNP",
        "name": "rs56116432",
        "var_class": "SNP",
        "mappings": [{
            "seq_region_name": "9",
            "strand": 1,
            "start": 133256042,
            "location": "9:133256042-133256042",
            "allele_string": "C/A/T",
            "assembly_name": "GRCh38",
            "end": 133256042
        }],
        "evidence": ["Frequency", "1000Genomes"],
        "most_severe_consequence": "missense_variant",
        "MAF": 0.00274725,
        "minor_allele": "T"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gene_lookup_record() {
        let record = parse_ensembl_record(&brca2_fixture()).unwrap();

        assert_eq!(record.id, "ENSG00000139618");
        assert_eq!(record.display_name.as_deref(), Some("BRCA2"));
        assert_eq!(record.record_type, "Gene");
        assert_eq!(record.biotype.as_deref(), Some("protein_coding"));
        assert_eq!(record.seq_region_name.as_deref(), Some("13"));
        assert_eq!(record.start, Some(32315086));
        assert_eq!(
            record.canonical_transcript.as_deref(),
            Some("ENST00000380152.8")
        );
    }

    #[test]
    fn parses_variant_record() {
        let record = parse_ensembl_record(&variant_fixture()).unwrap();

        assert_eq!(record.id, "rs56116432");
        assert_eq!(record.record_type, "Variant");
        assert_eq!(record.biotype.as_deref(), Some("SNP"));
        assert_eq!(
            record.most_severe_consequence.as_deref(),
            Some("missense_variant")
        );
        assert_eq!(
            record.mappings[0].location.as_deref(),
            Some("9:133256042-133256042")
        );
    }

    #[test]
    fn ensembl_json_uses_serpapi_shape() {
        let record = parse_ensembl_record(&brca2_fixture()).unwrap();
        let response = EnsemblSearchResponse {
            query: "BRCA2".to_string(),
            species: "homo_sapiens".to_string(),
            object_type: "gene".to_string(),
            results: vec![record],
            notes: Vec::new(),
        };
        let json = search_response_to_json(&response);

        assert_eq!(json["category"], "knowledge");
        assert_eq!(json["source"], "ensembl");
        assert_eq!(json["results"][0]["id"], "ENSG00000139618");
        assert_eq!(json["results"][0]["favicon"], ENSEMBL_FAVICON);
    }

    #[test]
    fn normalizes_ensembl_identifiers_from_urls() {
        assert_eq!(
            normalize_stable_id(
                "https://www.ensembl.org/Homo_sapiens/Gene/Summary?g=ENSG00000139618"
            )
            .as_deref(),
            Some("ENSG00000139618")
        );
        assert_eq!(
            normalize_stable_id("Ensembl:enst00000380152.8").as_deref(),
            Some("ENST00000380152.8")
        );
        assert_eq!(
            normalize_variant_id(
                "https://www.ensembl.org/Homo_sapiens/Variation/Explore?v=rs56116432"
            )
            .as_deref(),
            Some("rs56116432")
        );
    }

    #[tokio::test]
    async fn searches_symbol_against_mock_api() {
        let client = EnsemblClient::with_base_url("mock://ensembl").unwrap();
        let response = client
            .search(EnsemblSearchArgs {
                query: "BRCA2".to_string(),
                species: Some("homo sapiens".to_string()),
                object_type: None,
                max_results: Some(2),
            })
            .await
            .unwrap();

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].id, "ENSG00000139618");
        assert_eq!(response.species, "homo_sapiens");
    }

    #[tokio::test]
    async fn fetches_variant_against_mock_api() {
        let client = EnsemblClient::with_base_url("mock://ensembl").unwrap();
        let record = client
            .fetch("rs56116432", Some("homo_sapiens"))
            .await
            .unwrap();

        assert_eq!(record.id, "rs56116432");
        assert_eq!(record.record_type, "Variant");
    }
}
