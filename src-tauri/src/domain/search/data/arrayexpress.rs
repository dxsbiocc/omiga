//! ArrayExpress adapter backed by the EMBL-EBI BioStudies API.
//!
//! ArrayExpress studies now live in BioStudies. Search uses the public
//! BioStudies search endpoint with `collection=arrayexpress`; detail fetch uses
//! the public BioStudies study JSON endpoint by ArrayExpress accession.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_arrayexpress(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let page_size = args.normalized_max_results().to_string();
        let params = vec![
            ("collection".to_string(), "arrayexpress".to_string()),
            ("query".to_string(), arrayexpress_query(&args)),
            ("page".to_string(), "1".to_string()),
            ("pageSize".to_string(), page_size),
        ];
        let json = self.get_biostudies_json("search", &params).await?;
        Ok(parse_arrayexpress_search_response(
            &json,
            args.query.trim(),
            args.normalized_max_results() as usize,
        ))
    }

    pub(in crate::domain::search::data) async fn fetch_arrayexpress(
        &self,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let accession = normalize_arrayexpress_accession(identifier).ok_or_else(|| {
            format!(
                "ArrayExpress fetch requires an ArrayExpress study accession (for example E-MTAB-1234) or URL, got `{identifier}`"
            )
        })?;
        let endpoint = format!("studies/{}", encode_path_segment(&accession));
        let json = self.get_biostudies_json(&endpoint, &[]).await?;
        parse_arrayexpress_detail(&json)
            .ok_or_else(|| format!("BioStudies returned no ArrayExpress study for `{accession}`"))
    }

    async fn get_biostudies_json(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        #[cfg(debug_assertions)]
        if self.base_urls.biostudies == "mock://biostudies" {
            return mock_biostudies_json(endpoint, params).ok_or_else(|| {
                format!("debug BioStudies mock has no fixture for endpoint `{endpoint}`")
            });
        }

        let response = self
            .http
            .get(format!(
                "{}/{}",
                self.base_urls.biostudies,
                endpoint.trim_start_matches('/')
            ))
            .header(reqwest::header::ACCEPT, "application/json")
            .query(params)
            .send()
            .await
            .map_err(|e| format!("BioStudies API {endpoint} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read BioStudies API {endpoint} response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "BioStudies API {endpoint} returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "parse BioStudies API {endpoint} JSON: {e}; body: {}",
                truncate_for_error(&body)
            )
        })
    }
}

pub fn looks_like_arrayexpress_accession(value: &str) -> bool {
    normalize_arrayexpress_accession(value).is_some()
}

pub(in crate::domain::search::data) fn parse_arrayexpress_search_response(
    value: &Json,
    query: &str,
    max_results: usize,
) -> DataSearchResponse {
    let results = value
        .get("hits")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .take(max_results)
        .filter_map(parse_arrayexpress_search_hit)
        .collect::<Vec<_>>();
    let total = value
        .get("totalHits")
        .and_then(json_u64_from_string_or_number);
    DataSearchResponse {
        query: query.trim().to_string(),
        source: PublicDataSource::ArrayExpress.as_str().to_string(),
        total,
        results,
        notes: vec![
            "BioStudies API search with collection=arrayexpress; fetch returns full study metadata."
                .to_string(),
        ],
    }
}

fn parse_arrayexpress_search_hit(hit: &Json) -> Option<DataRecord> {
    let map = hit.as_object()?;
    let accession = string_field_any(map, &["accession", "accno"])
        .and_then(|value| normalize_arrayexpress_accession(&value))?;
    let title = string_field_any(map, &["title"]).unwrap_or_else(|| accession.clone());
    let content = string_field_any(map, &["content"]).map(|value| clean_html_text(&value));
    let author = string_field_any(map, &["author"]);
    let release_date = string_field_any(map, &["release_date", "releaseDate"]);
    let mut summary = Vec::new();
    push_labeled(&mut summary, "Author", author.as_deref());
    push_plain(&mut summary, content.as_deref());

    let mut extra = JsonMap::new();
    insert_extra(&mut extra, "author", author);
    insert_extra(&mut extra, "views", string_field_any(map, &["views"]));
    insert_extra(&mut extra, "links_count", string_field_any(map, &["links"]));
    insert_extra(&mut extra, "files_count", string_field_any(map, &["files"]));
    if let Some(is_public) = map.get("isPublic").and_then(Json::as_bool) {
        extra.insert("is_public".to_string(), Json::Bool(is_public));
    }

    Some(DataRecord {
        id: accession.clone(),
        accession: accession.clone(),
        source: PublicDataSource::ArrayExpress,
        title,
        summary: summary.join(" | "),
        url: arrayexpress_study_url(&accession),
        record_type: string_field_any(map, &["type"]),
        organism: None,
        published_date: release_date,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: Vec::new(),
        extra,
    })
}

pub(in crate::domain::search::data) fn parse_arrayexpress_detail(
    value: &Json,
) -> Option<DataRecord> {
    let accession = json_path_string(value, &["accno"])
        .and_then(|value| normalize_arrayexpress_accession(&value))?;
    let section = value.get("section").unwrap_or(value);
    let title = attr_first(section, &["Title"])
        .or_else(|| attr_first(value, &["Title"]))
        .unwrap_or_else(|| accession.clone());
    let study_type = attr_first(section, &["Study type", "Study Type"]);
    let organism = attr_first(section, &["Organism"]);
    let description = attr_first(section, &["Description"]).map(|value| clean_html_text(&value));
    let release_date = attr_first(value, &["ReleaseDate", "Release date"])
        .or_else(|| attr_first(section, &["ReleaseDate", "Release date"]));
    let sample_count = find_first_attr(value, &["Sample count"]).and_then(|value| {
        value
            .trim()
            .parse::<u64>()
            .ok()
            .or_else(|| json_u64_from_string_or_number(&Json::String(value)))
    });
    let authors = collect_section_attr_values(value, "Author", "Name", 16);
    let links = collect_links(value, 16);
    let files = collect_file_urls(&accession, value, 24);

    let mut summary = Vec::new();
    push_labeled(&mut summary, "Study type", study_type.as_deref());
    push_labeled(&mut summary, "Organism", organism.as_deref());
    if let Some(samples) = sample_count {
        summary.push(format!("Samples: {samples}"));
    }
    push_plain(&mut summary, description.as_deref());

    let mut extra = JsonMap::new();
    extra.insert("attributes".to_string(), attrs_to_json(value));
    extra.insert("study_attributes".to_string(), attrs_to_json(section));
    if !authors.is_empty() {
        extra.insert(
            "authors".to_string(),
            Json::Array(authors.iter().cloned().map(Json::String).collect()),
        );
    }
    if !links.is_empty() {
        extra.insert(
            "links".to_string(),
            Json::Array(links.iter().cloned().map(Json::String).collect()),
        );
    }
    insert_extra(&mut extra, "study_type", study_type.clone());
    insert_extra(&mut extra, "file_count", Some(files.len().to_string()));

    Some(DataRecord {
        id: accession.clone(),
        accession: accession.clone(),
        source: PublicDataSource::ArrayExpress,
        title,
        summary: summary.join(" | "),
        url: arrayexpress_study_url(&accession),
        record_type: Some("study".to_string()),
        organism,
        published_date: release_date,
        updated_date: None,
        sample_count,
        platform: study_type,
        files,
        extra,
    })
}

fn arrayexpress_query(args: &DataSearchArgs) -> String {
    let mut parts = vec![args.query.trim().to_string()];
    if let Some(organism) = param_string(args.params.as_ref(), &["organism"]) {
        parts.push(organism);
    }
    if let Some(study_type) = param_string(args.params.as_ref(), &["study_type", "type"]) {
        parts.push(study_type);
    }
    parts
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_arrayexpress_accession(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        if let Some(accession) = parsed
            .path_segments()
            .into_iter()
            .flatten()
            .find_map(arrayexpress_accession_from_text)
        {
            return Some(accession);
        }
        for (_, val) in parsed.query_pairs() {
            if let Some(accession) = arrayexpress_accession_from_text(&val) {
                return Some(accession);
            }
        }
    }
    arrayexpress_accession_from_text(value)
}

fn arrayexpress_accession_from_text(value: &str) -> Option<String> {
    lazy_static::lazy_static! {
        static ref RE_ARRAYEXPRESS: regex::Regex =
            regex::Regex::new(r#"(?i)\bE-[A-Z0-9]+-\d+\b"#).unwrap();
    }
    RE_ARRAYEXPRESS
        .find(value)
        .map(|m| m.as_str().to_ascii_uppercase())
}

fn attr_first(value: &Json, names: &[&str]) -> Option<String> {
    value
        .get("attributes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attr| attr.as_object())
        .find_map(|attr| {
            let name = attr.get("name").and_then(json_string)?;
            names
                .iter()
                .any(|wanted| name.eq_ignore_ascii_case(wanted))
                .then(|| attr.get("value").and_then(json_string))
                .flatten()
        })
}

fn find_first_attr(value: &Json, names: &[&str]) -> Option<String> {
    if let Some(value) = attr_first(value, names) {
        return Some(value);
    }
    match value {
        Json::Object(map) => map.values().find_map(|value| find_first_attr(value, names)),
        Json::Array(items) => items.iter().find_map(|value| find_first_attr(value, names)),
        _ => None,
    }
}

fn attrs_to_json(value: &Json) -> Json {
    let mut out = JsonMap::new();
    if let Some(attrs) = value.get("attributes").and_then(Json::as_array) {
        for attr in attrs {
            let Some(name) = json_path_string(attr, &["name"]) else {
                continue;
            };
            let Some(value) = json_path_string(attr, &["value"]) else {
                continue;
            };
            if let Some(existing) = out.get_mut(&name) {
                match existing {
                    Json::Array(items) => items.push(Json::String(value)),
                    other => {
                        let previous = other.take();
                        *other = Json::Array(vec![previous, Json::String(value)]);
                    }
                }
            } else {
                out.insert(name, Json::String(value));
            }
        }
    }
    Json::Object(out)
}

fn collect_section_attr_values(
    value: &Json,
    section_type: &str,
    attr_name: &str,
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    collect_section_attr_values_inner(value, section_type, attr_name, limit, &mut out);
    out
}

fn collect_section_attr_values_inner(
    value: &Json,
    section_type: &str,
    attr_name: &str,
    limit: usize,
    out: &mut Vec<String>,
) {
    if out.len() >= limit {
        return;
    }
    match value {
        Json::Object(map) => {
            let matches_type = map
                .get("type")
                .and_then(json_string)
                .is_some_and(|kind| kind.eq_ignore_ascii_case(section_type));
            if matches_type {
                if let Some(value) = attr_first(value, &[attr_name])
                    .filter(|value| !out.iter().any(|existing| existing == value))
                {
                    out.push(value);
                }
            }
            for value in map.values() {
                collect_section_attr_values_inner(value, section_type, attr_name, limit, out);
            }
        }
        Json::Array(items) => {
            for value in items {
                collect_section_attr_values_inner(value, section_type, attr_name, limit, out);
            }
        }
        _ => {}
    }
}

fn collect_links(value: &Json, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    collect_links_inner(value, limit, &mut out);
    out
}

fn collect_links_inner(value: &Json, limit: usize, out: &mut Vec<String>) {
    if out.len() >= limit {
        return;
    }
    match value {
        Json::Object(map) => {
            if let Some(url) = map.get("url").and_then(json_string) {
                let label = attr_first(value, &["Type", "Description"]);
                let value = label.map(|label| format!("{label}: {url}")).unwrap_or(url);
                if !out.iter().any(|existing| existing == &value) {
                    out.push(value);
                }
            }
            for value in map.values() {
                collect_links_inner(value, limit, out);
            }
        }
        Json::Array(items) => {
            for value in items {
                collect_links_inner(value, limit, out);
            }
        }
        _ => {}
    }
}

fn collect_file_urls(accession: &str, value: &Json, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    collect_file_urls_inner(accession, value, limit, &mut out);
    out
}

fn collect_file_urls_inner(accession: &str, value: &Json, limit: usize, out: &mut Vec<String>) {
    if out.len() >= limit {
        return;
    }
    match value {
        Json::Object(map) => {
            if let Some(path) = map.get("path").and_then(json_string) {
                let url = arrayexpress_file_url(accession, &path);
                if !out.iter().any(|existing| existing == &url) {
                    out.push(url);
                }
            }
            for value in map.values() {
                collect_file_urls_inner(accession, value, limit, out);
            }
        }
        Json::Array(items) => {
            for value in items {
                collect_file_urls_inner(accession, value, limit, out);
            }
        }
        _ => {}
    }
}

fn json_path_string(value: &Json, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    json_string(current)
}

fn param_string(params: Option<&Json>, keys: &[&str]) -> Option<String> {
    let object = params?.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(json_string))
}

fn push_labeled(out: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(format!("{label}: {value}"));
    }
}

fn push_plain(out: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(value.to_string());
    }
}

fn insert_extra(map: &mut JsonMap<String, Json>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
        map.insert(key.to_string(), Json::String(value));
    }
}

fn arrayexpress_study_url(accession: &str) -> String {
    format!("https://www.ebi.ac.uk/biostudies/arrayexpress/studies/{accession}")
}

fn arrayexpress_file_url(accession: &str, path: &str) -> String {
    let encoded_path = path
        .split('/')
        .map(encode_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!("https://www.ebi.ac.uk/biostudies/files/{accession}/{encoded_path}")
}

#[cfg(debug_assertions)]
fn mock_biostudies_json(endpoint: &str, _params: &[(String, String)]) -> Option<Json> {
    match endpoint {
        "search" => Some(json!({
            "page": 1,
            "pageSize": 1,
            "totalHits": 1,
            "hits": [{
                "accession": "E-MTAB-9999",
                "type": "study",
                "title": "Single-cell RNA-seq of mock tissue",
                "author": "Example A Example B",
                "links": 1,
                "files": 2,
                "release_date": "2024-01-02",
                "views": 42,
                "isPublic": true,
                "content": "E-MTAB-9999 single-cell RNA-seq Homo sapiens mock expression dataset"
            }],
            "query": "single cell"
        })),
        "studies/E-MTAB-9999" | "studies/E-GEOD-26319" => Some(json!({
            "accno": "E-MTAB-9999",
            "attributes": [
                {"name": "Title", "value": "Single-cell RNA-seq of mock tissue"},
                {"name": "ReleaseDate", "value": "2024-01-02"},
                {"name": "RootPath", "value": "E-MTAB-9999"},
                {"name": "AttachTo", "value": "ArrayExpress"}
            ],
            "section": {
                "type": "Study",
                "attributes": [
                    {"name": "Title", "value": "Single-cell RNA-seq of mock tissue"},
                    {"name": "Study type", "value": "RNA-seq of coding RNA from single cells"},
                    {"name": "Organism", "value": "Homo sapiens"},
                    {"name": "Description", "value": "Mock ArrayExpress experiment for parser tests."}
                ],
                "links": [[{"url": "GSE999999", "attributes": [{"name": "Type", "value": "GEO"}]}]],
                "subsections": [
                    {"type": "Author", "attributes": [{"name": "Name", "value": "Example A"}]},
                    {"type": "Samples", "attributes": [{"name": "Sample count", "value": "12"}]},
                    {"type": "MAGE-TAB Files", "files": [[
                        {"path": "E-MTAB-9999.idf.txt", "size": 123, "type": "file"},
                        {"path": "E-MTAB-9999.sdrf.txt", "size": 456, "type": "file"}
                    ]]}
                ]
            }
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arrayexpress_search_hits() {
        let json = mock_biostudies_json("search", &[]).unwrap();
        let response = parse_arrayexpress_search_response(&json, "single cell", 10);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].source, PublicDataSource::ArrayExpress);
        assert_eq!(response.results[0].accession, "E-MTAB-9999");
        assert_eq!(
            response.results[0].published_date.as_deref(),
            Some("2024-01-02")
        );
        assert!(response.results[0].summary.contains("Homo sapiens"));
    }

    #[test]
    fn parses_arrayexpress_detail() {
        let json = mock_biostudies_json("studies/E-MTAB-9999", &[]).unwrap();
        let record = parse_arrayexpress_detail(&json).unwrap();
        assert_eq!(record.accession, "E-MTAB-9999");
        assert_eq!(record.organism.as_deref(), Some("Homo sapiens"));
        assert_eq!(record.sample_count, Some(12));
        assert_eq!(
            record.platform.as_deref(),
            Some("RNA-seq of coding RNA from single cells")
        );
        assert_eq!(record.files.len(), 2);
        assert!(record.files[0].contains("/biostudies/files/E-MTAB-9999/"));
        assert_eq!(record.extra["authors"][0].as_str(), Some("Example A"));
    }

    #[test]
    fn recognizes_arrayexpress_accessions_and_urls() {
        assert!(looks_like_arrayexpress_accession("E-MTAB-9999"));
        assert!(looks_like_arrayexpress_accession(
            "https://www.ebi.ac.uk/biostudies/arrayexpress/studies/E-GEOD-26319"
        ));
        assert_eq!(
            normalize_arrayexpress_accession("ArrayExpress:E-MTAB-1234").as_deref(),
            Some("E-MTAB-1234")
        );
    }
}
