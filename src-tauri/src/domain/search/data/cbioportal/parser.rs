//! cBioPortal study response parsing and identifier helpers.

use super::super::common::*;
use serde_json::{json, Map as JsonMap, Value as Json};

pub(super) fn parse_cbioportal_studies_json(value: &Json) -> Vec<DataRecord> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(parse_cbioportal_study).collect()
}

pub(super) fn parse_cbioportal_study(item: &Json) -> Option<DataRecord> {
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

pub(super) fn normalize_cbioportal_study_id(value: &str) -> Option<String> {
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

fn cbioportal_study_url(study_id: &str) -> String {
    format!("https://www.cbioportal.org/study/summary?id={study_id}")
}
