//! UniProtKB adapter for structured `query(category="knowledge")`.
//!
//! Uses the public UniProt REST API (`rest.uniprot.org`) for protein search and
//! single-entry retrieval. No API key is required.

use crate::domain::tools::ToolContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as Json};
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://rest.uniprot.org";
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 25;
const UNIPROT_FAVICON: &str = "https://www.uniprot.org/favicon.ico";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct UniProtSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default)]
    pub organism: Option<String>,
    #[serde(default, alias = "taxid", alias = "tax_id", alias = "taxonomy_id")]
    pub taxon_id: Option<String>,
    #[serde(default)]
    pub reviewed: Option<bool>,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "size")]
    pub max_results: Option<u32>,
}

impl UniProtSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UniProtSearchResponse {
    pub query: String,
    pub effective_query: String,
    pub total: Option<u64>,
    pub results: Vec<UniProtRecord>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UniProtRecord {
    pub accession: String,
    pub secondary_accessions: Vec<String>,
    pub entry_name: String,
    pub entry_type: Option<String>,
    pub reviewed: bool,
    pub protein_name: String,
    pub gene_names: Vec<String>,
    pub organism: Option<String>,
    pub common_organism: Option<String>,
    pub taxon_id: Option<u64>,
    pub length: Option<u64>,
    pub mass: Option<u64>,
    pub sequence: Option<String>,
    pub function: Option<String>,
    pub subcellular_location: Vec<String>,
    pub disease: Vec<String>,
    pub keywords: Vec<String>,
    pub go_terms: Vec<UniProtCrossReference>,
    pub pdb_ids: Vec<String>,
    pub ensembl_ids: Vec<String>,
    pub gene_ids: Vec<String>,
    pub raw_entry: Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UniProtCrossReference {
    pub database: String,
    pub id: String,
    pub term: Option<String>,
    pub evidence: Option<String>,
}

#[derive(Clone)]
pub struct UniProtClient {
    http: reqwest::Client,
    base_url: String,
}

impl UniProtClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-UniProt/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build UniProt HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("Omiga-UniProt/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| format!("build UniProt HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        })
    }

    pub async fn search(&self, args: UniProtSearchArgs) -> Result<UniProtSearchResponse, String> {
        if args.query.trim().is_empty() {
            return Err("UniProt query must not be empty".to_string());
        }
        let effective_query = build_uniprot_query(&args);
        let size = args.normalized_max_results();
        let params = vec![
            ("query".to_string(), effective_query.clone()),
            ("format".to_string(), "json".to_string()),
            ("size".to_string(), size.to_string()),
        ];

        let (json, total) = self.get_json("/uniprotkb/search", &params).await?;
        let records = parse_uniprot_search_json(&json);
        let mut notes = Vec::new();
        if records.len() == size as usize && total.is_some_and(|value| value > size as u64) {
            notes.push(format!(
                "UniProt returned the first {size} records; increase max_results to retrieve more."
            ));
        }

        Ok(UniProtSearchResponse {
            query: args.query.trim().to_string(),
            effective_query,
            total,
            results: records,
            notes,
        })
    }

    pub async fn fetch(&self, identifier: &str) -> Result<UniProtRecord, String> {
        let accession = normalize_accession(identifier).ok_or_else(|| {
            "UniProt fetch expects an accession, entry URL, or search result id".to_string()
        })?;
        let path = format!("/uniprotkb/{accession}.json");
        let (json, _) = self.get_json(&path, &[]).await?;
        parse_uniprot_entry(&json)
            .ok_or_else(|| format!("UniProt did not return an entry for accession {accession}"))
    }

    async fn get_json(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<(Json, Option<u64>), String> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .http
            .get(&url)
            .query(params)
            .send()
            .await
            .map_err(|e| format!("UniProt request failed: {e}"))?;

        let status = response.status();
        let total = response
            .headers()
            .get("x-total-results")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let body = response
            .text()
            .await
            .map_err(|e| format!("UniProt response read failed: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "UniProt returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }

        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "UniProt returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(messages) = json.get("messages").and_then(Json::as_array) {
            if !messages.is_empty() {
                let message = messages
                    .iter()
                    .filter_map(Json::as_str)
                    .collect::<Vec<_>>()
                    .join("; ");
                if !message.is_empty() {
                    return Err(format!("UniProt error: {message}"));
                }
            }
        }
        Ok((json, total))
    }
}

pub fn search_response_to_json(response: &UniProtSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| uniprot_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "effective_query": response.effective_query,
        "category": "knowledge",
        "source": "uniprot",
        "effective_source": "uniprot",
        "total": response.total,
        "results": results,
        "notes": response.notes,
    })
}

pub fn detail_to_json(record: &UniProtRecord) -> Json {
    json!({
        "category": "knowledge",
        "source": "uniprot",
        "effective_source": "uniprot",
        "id": record.accession,
        "accession": record.accession,
        "title": uniprot_title(record),
        "name": record.entry_name,
        "link": uniprot_url(&record.accession),
        "url": uniprot_url(&record.accession),
        "displayed_link": format!("uniprot.org/uniprotkb/{}", record.accession),
        "favicon": UNIPROT_FAVICON,
        "snippet": uniprot_snippet(record),
        "content": uniprot_content(record),
        "metadata": uniprot_metadata(record),
    })
}

fn uniprot_to_serp_result(record: &UniProtRecord, position: usize) -> Json {
    json!({
        "position": position,
        "category": "knowledge",
        "source": "uniprot",
        "title": uniprot_title(record),
        "name": record.entry_name,
        "link": uniprot_url(&record.accession),
        "url": uniprot_url(&record.accession),
        "displayed_link": format!("uniprot.org/uniprotkb/{}", record.accession),
        "favicon": UNIPROT_FAVICON,
        "snippet": uniprot_snippet(record),
        "id": record.accession,
        "accession": record.accession,
        "metadata": uniprot_metadata(record),
    })
}

fn uniprot_metadata(record: &UniProtRecord) -> Json {
    json!({
        "accession": record.accession,
        "secondary_accessions": record.secondary_accessions,
        "entry_name": record.entry_name,
        "entry_type": record.entry_type,
        "reviewed": record.reviewed,
        "protein_name": record.protein_name,
        "gene_names": record.gene_names,
        "organism": record.organism,
        "common_organism": record.common_organism,
        "taxon_id": record.taxon_id,
        "length": record.length,
        "mass": record.mass,
        "function": record.function,
        "subcellular_location": record.subcellular_location,
        "disease": record.disease,
        "keywords": record.keywords,
        "go_terms": record.go_terms,
        "pdb_ids": record.pdb_ids,
        "ensembl_ids": record.ensembl_ids,
        "gene_ids": record.gene_ids,
        "source_specific": record.raw_entry,
    })
}

fn build_uniprot_query(args: &UniProtSearchArgs) -> String {
    let mut query = args.query.trim().to_string();
    if let Some(taxon_id) = clean_optional(args.taxon_id.as_deref()) {
        if !contains_case_insensitive(&query, "organism_id:")
            && !contains_case_insensitive(&query, "taxonomy_id:")
        {
            query = format!("({query}) AND (organism_id:{taxon_id})");
        }
    } else if let Some(organism) = clean_optional(args.organism.as_deref()) {
        if !contains_case_insensitive(&query, "organism_name:")
            && !contains_case_insensitive(&query, "organism_id:")
        {
            query = format!(
                "({query}) AND (organism_name:\"{}\")",
                escape_query_phrase(&organism)
            );
        }
    }
    if let Some(reviewed) = args.reviewed {
        if !contains_case_insensitive(&query, "reviewed:") {
            query = format!("({query}) AND (reviewed:{reviewed})");
        }
    }
    query
}

fn parse_uniprot_search_json(value: &Json) -> Vec<UniProtRecord> {
    value
        .get("results")
        .and_then(Json::as_array)
        .map(|items| items.iter().filter_map(parse_uniprot_entry).collect())
        .unwrap_or_default()
}

fn parse_uniprot_entry(value: &Json) -> Option<UniProtRecord> {
    let map = value.as_object()?;
    let accession = string_field(map, "primaryAccession")?;
    let entry_name = string_field(map, "uniProtkbId").unwrap_or_else(|| accession.clone());
    let entry_type = string_field(map, "entryType");
    let reviewed = entry_type
        .as_deref()
        .map(is_reviewed_entry_type)
        .unwrap_or(false);
    let protein_name =
        protein_name(map.get("proteinDescription")).unwrap_or_else(|| entry_name.clone());
    let organism = map.get("organism");
    let sequence = map.get("sequence");
    let cross_refs = parse_cross_references(map.get("uniProtKBCrossReferences"));

    Some(UniProtRecord {
        accession,
        secondary_accessions: json_string_list(map.get("secondaryAccessions")),
        entry_name,
        entry_type,
        reviewed,
        protein_name,
        gene_names: gene_names(map.get("genes")),
        organism: organism
            .and_then(|v| v.get("scientificName"))
            .and_then(Json::as_str)
            .map(str::to_string),
        common_organism: organism
            .and_then(|v| v.get("commonName"))
            .and_then(Json::as_str)
            .map(str::to_string),
        taxon_id: organism
            .and_then(|v| v.get("taxonId"))
            .and_then(json_u64_from_string_or_number),
        length: sequence
            .and_then(|v| v.get("length"))
            .and_then(json_u64_from_string_or_number),
        mass: sequence
            .and_then(|v| v.get("molWeight"))
            .and_then(json_u64_from_string_or_number),
        sequence: sequence
            .and_then(|v| v.get("value"))
            .and_then(Json::as_str)
            .map(str::to_string),
        function: comment_text(map.get("comments"), "FUNCTION").map(|items| items.join("\n\n")),
        subcellular_location: subcellular_locations(map.get("comments")),
        disease: comment_text(map.get("comments"), "DISEASE").unwrap_or_default(),
        keywords: keywords(map.get("keywords")),
        go_terms: cross_refs
            .iter()
            .filter(|xref| xref.database == "GO")
            .cloned()
            .collect(),
        pdb_ids: cross_ref_ids(&cross_refs, "PDB"),
        ensembl_ids: cross_ref_ids(&cross_refs, "Ensembl"),
        gene_ids: cross_ref_ids(&cross_refs, "GeneID"),
        raw_entry: value.clone(),
    })
}

fn protein_name(value: Option<&Json>) -> Option<String> {
    let root = value?.as_object()?;
    root.get("recommendedName")
        .and_then(full_name_from_name_object)
        .or_else(|| {
            root.get("submissionNames")
                .and_then(Json::as_array)?
                .iter()
                .find_map(full_name_from_name_object)
        })
        .or_else(|| {
            root.get("alternativeNames")
                .and_then(Json::as_array)?
                .iter()
                .find_map(full_name_from_name_object)
        })
}

fn full_name_from_name_object(value: &Json) -> Option<String> {
    value
        .get("fullName")
        .and_then(|v| v.get("value"))
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn gene_names(value: Option<&Json>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(items) = value.and_then(Json::as_array) else {
        return out;
    };
    for item in items {
        for path in ["geneName", "orderedLocusNames", "orfNames", "synonyms"] {
            match item.get(path) {
                Some(Json::Array(names)) => {
                    for name in names {
                        push_unique(
                            &mut out,
                            name.get("value").and_then(Json::as_str).unwrap_or_default(),
                        );
                    }
                }
                Some(name) => push_unique(
                    &mut out,
                    name.get("value").and_then(Json::as_str).unwrap_or_default(),
                ),
                None => {}
            }
        }
    }
    out
}

fn comment_text(value: Option<&Json>, comment_type: &str) -> Option<Vec<String>> {
    let items = value.and_then(Json::as_array)?;
    let mut out = Vec::new();
    for item in items {
        if item
            .get("commentType")
            .and_then(Json::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case(comment_type))
        {
            if let Some(texts) = item.get("texts").and_then(Json::as_array) {
                for text in texts {
                    push_unique(
                        &mut out,
                        text.get("value").and_then(Json::as_str).unwrap_or_default(),
                    );
                }
            }
            if let Some(note) = item.get("note").and_then(Json::as_str) {
                push_unique(&mut out, note);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn subcellular_locations(value: Option<&Json>) -> Vec<String> {
    let Some(items) = value.and_then(Json::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        if !item
            .get("commentType")
            .and_then(Json::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("SUBCELLULAR LOCATION"))
        {
            continue;
        }
        if let Some(locations) = item.get("subcellularLocations").and_then(Json::as_array) {
            for location in locations {
                if let Some(value) = location
                    .get("location")
                    .and_then(|v| v.get("value"))
                    .and_then(Json::as_str)
                {
                    push_unique(&mut out, value);
                }
            }
        }
        if let Some(texts) = comment_text(
            Some(&Json::Array(vec![item.clone()])),
            "SUBCELLULAR LOCATION",
        ) {
            for text in texts {
                push_unique(&mut out, &text);
            }
        }
    }
    out
}

fn keywords(value: Option<&Json>) -> Vec<String> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            let mut out = Vec::new();
            for item in items {
                push_unique(
                    &mut out,
                    item.get("name").and_then(Json::as_str).unwrap_or_default(),
                );
            }
            out
        })
        .unwrap_or_default()
}

fn parse_cross_references(value: Option<&Json>) -> Vec<UniProtCrossReference> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let database = item.get("database").and_then(Json::as_str)?.to_string();
                    let id = item.get("id").and_then(Json::as_str)?.to_string();
                    let (term, evidence) = xref_term_and_evidence(item.get("properties"));
                    Some(UniProtCrossReference {
                        database,
                        id,
                        term,
                        evidence,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn xref_term_and_evidence(value: Option<&Json>) -> (Option<String>, Option<String>) {
    let Some(items) = value.and_then(Json::as_array) else {
        return (None, None);
    };
    let mut term = None;
    let mut evidence = None;
    for item in items {
        let key = item.get("key").and_then(Json::as_str).unwrap_or_default();
        let value = item.get("value").and_then(Json::as_str).map(str::to_string);
        if key.eq_ignore_ascii_case("GoTerm") || key.eq_ignore_ascii_case("Term") {
            term = value;
        } else if key.eq_ignore_ascii_case("GoEvidenceType") || key.eq_ignore_ascii_case("Evidence")
        {
            evidence = value;
        }
    }
    (term, evidence)
}

fn cross_ref_ids(refs: &[UniProtCrossReference], database: &str) -> Vec<String> {
    let mut out = Vec::new();
    for xref in refs {
        if xref.database.eq_ignore_ascii_case(database) {
            push_unique(&mut out, &xref.id);
        }
    }
    out
}

fn uniprot_title(record: &UniProtRecord) -> String {
    if record.gene_names.is_empty() {
        format!("{} — {}", record.accession, record.protein_name)
    } else {
        format!(
            "{} ({}) — {}",
            record.gene_names[0], record.accession, record.protein_name
        )
    }
}

fn uniprot_url(accession: &str) -> String {
    format!("https://www.uniprot.org/uniprotkb/{accession}/entry")
}

fn uniprot_snippet(record: &UniProtRecord) -> String {
    let mut pieces = Vec::new();
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(organism.to_string());
    }
    if record.reviewed {
        pieces.push("reviewed".to_string());
    }
    if let Some(length) = record.length {
        pieces.push(format!("{length} aa"));
    }
    if let Some(function) = record.function.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(truncate_chars(function, 280));
    }
    pieces.join(" | ")
}

fn uniprot_content(record: &UniProtRecord) -> String {
    let mut out = String::new();
    out.push_str(&uniprot_title(record));
    out.push_str("\n\n");
    out.push_str(&format!("Accession: {}\n", record.accession));
    out.push_str(&format!("Entry: {}\n", record.entry_name));
    if !record.gene_names.is_empty() {
        out.push_str(&format!("Genes: {}\n", record.gene_names.join(", ")));
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("Organism: {organism}\n"));
    }
    if let Some(taxon_id) = record.taxon_id {
        out.push_str(&format!("Taxonomy ID: {taxon_id}\n"));
    }
    if let Some(length) = record.length {
        out.push_str(&format!("Length: {length} aa\n"));
    }
    if !record.go_terms.is_empty() {
        let terms = record
            .go_terms
            .iter()
            .take(12)
            .map(|item| match item.term.as_deref() {
                Some(term) => format!("{} ({term})", item.id),
                None => item.id.clone(),
            })
            .collect::<Vec<_>>();
        out.push_str(&format!("GO: {}\n", terms.join("; ")));
    }
    out.push_str(&format!("Link: {}\n", uniprot_url(&record.accession)));
    if let Some(function) = record.function.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("\nFunction:\n");
        out.push_str(function.trim());
    }
    if !record.subcellular_location.is_empty() {
        out.push_str("\n\nSubcellular location:\n");
        out.push_str(&record.subcellular_location.join("; "));
    }
    if !record.disease.is_empty() {
        out.push_str("\n\nDisease:\n");
        out.push_str(&record.disease.join("\n\n"));
    }
    out
}

fn normalize_accession(value: &str) -> Option<String> {
    let mut value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(idx) = value.find("/uniprotkb/") {
        value = &value[idx + "/uniprotkb/".len()..];
    } else if let Some(idx) = value.find("/uniprot/") {
        value = &value[idx + "/uniprot/".len()..];
    }
    value = value
        .trim_start_matches("UniProtKB:")
        .trim_start_matches("uniprotkb:")
        .trim_start_matches("UniProt:")
        .trim_start_matches("uniprot:");
    let value = value
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".json")
        .trim_end_matches(".fasta")
        .trim()
        .to_ascii_uppercase();
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && value.chars().any(|c| c.is_ascii_alphabetic())
    {
        Some(value)
    } else {
        None
    }
}

fn is_reviewed_entry_type(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    if value.contains("unreviewed") || value.contains("trembl") {
        return false;
    }
    value.contains("reviewed") || value.contains("swiss-prot")
}

fn string_field(map: &JsonMap<String, Json>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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

fn json_u64_from_string_or_number(value: &Json) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
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

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn escape_query_phrase(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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

    fn brca1_fixture() -> Json {
        json!({
            "entryType": "UniProtKB reviewed (Swiss-Prot)",
            "primaryAccession": "P38398",
            "secondaryAccessions": ["E9PFZ0"],
            "uniProtkbId": "BRCA1_HUMAN",
            "organism": {
                "scientificName": "Homo sapiens",
                "commonName": "Human",
                "taxonId": 9606
            },
            "proteinDescription": {
                "recommendedName": {
                    "fullName": { "value": "Breast cancer type 1 susceptibility protein" }
                }
            },
            "genes": [{
                "geneName": { "value": "BRCA1" },
                "synonyms": [{ "value": "RNF53" }]
            }],
            "comments": [{
                "commentType": "FUNCTION",
                "texts": [{ "value": "E3 ubiquitin-protein ligase involved in DNA repair." }]
            }, {
                "commentType": "SUBCELLULAR LOCATION",
                "subcellularLocations": [{
                    "location": { "value": "Nucleus" }
                }]
            }],
            "keywords": [{ "name": "DNA damage" }],
            "sequence": {
                "value": "M",
                "length": 1863,
                "molWeight": 207721
            },
            "uniProtKBCrossReferences": [{
                "database": "GO",
                "id": "GO:0005515",
                "properties": [{
                    "key": "GoTerm",
                    "value": "F:protein binding"
                }]
            }, {
                "database": "PDB",
                "id": "1JM7"
            }, {
                "database": "GeneID",
                "id": "672"
            }]
        })
    }

    #[test]
    fn builds_filtered_uniprot_query() {
        let args = UniProtSearchArgs {
            query: "gene_exact:BRCA1".to_string(),
            organism: None,
            taxon_id: Some("9606".to_string()),
            reviewed: Some(true),
            max_results: None,
        };

        assert_eq!(
            build_uniprot_query(&args),
            "((gene_exact:BRCA1) AND (organism_id:9606)) AND (reviewed:true)"
        );
    }

    #[test]
    fn parses_uniprot_search_json() {
        let root = json!({ "results": [brca1_fixture()] });
        let records = parse_uniprot_search_json(&root);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].accession, "P38398");
        assert_eq!(records[0].entry_name, "BRCA1_HUMAN");
        assert_eq!(records[0].gene_names, vec!["BRCA1", "RNF53"]);
        assert_eq!(records[0].organism.as_deref(), Some("Homo sapiens"));
        assert_eq!(records[0].taxon_id, Some(9606));
        assert_eq!(records[0].length, Some(1863));
        assert_eq!(records[0].go_terms[0].id, "GO:0005515");
        assert_eq!(records[0].pdb_ids, vec!["1JM7"]);
        assert_eq!(records[0].gene_ids, vec!["672"]);
    }

    #[test]
    fn uniprot_json_uses_serpapi_shape() {
        let record = parse_uniprot_entry(&brca1_fixture()).unwrap();
        let response = UniProtSearchResponse {
            query: "BRCA1".to_string(),
            effective_query: "BRCA1".to_string(),
            total: Some(1),
            results: vec![record],
            notes: Vec::new(),
        };
        let json = search_response_to_json(&response);

        assert_eq!(json["category"], "knowledge");
        assert_eq!(json["source"], "uniprot");
        assert_eq!(json["results"][0]["accession"], "P38398");
        assert_eq!(json["results"][0]["favicon"], UNIPROT_FAVICON);
    }

    #[test]
    fn normalizes_uniprot_accessions_from_urls() {
        assert_eq!(
            normalize_accession("https://www.uniprot.org/uniprotkb/P38398/entry").as_deref(),
            Some("P38398")
        );
        assert_eq!(
            normalize_accession("UniProtKB:a0a022ywf9.json").as_deref(),
            Some("A0A022YWF9")
        );
    }

    #[test]
    fn distinguishes_reviewed_from_unreviewed_entry_types() {
        assert!(is_reviewed_entry_type("UniProtKB reviewed (Swiss-Prot)"));
        assert!(!is_reviewed_entry_type("UniProtKB unreviewed (TrEMBL)"));
    }
}
