//! cBioPortal study discovery/detail adapter.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

impl PublicDataClient {
    pub(super) async fn search_cbioportal(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
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

    pub(super) async fn fetch_cbioportal(&self, identifier: &str) -> Result<DataRecord, String> {
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

fn cbioportal_study_url(study_id: &str) -> String {
    format!("https://www.cbioportal.org/study/summary?id={study_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
