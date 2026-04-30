//! cBioPortal REST request execution.

use super::super::common::*;
use super::super::PublicDataClient;
use super::parser;
use serde_json::Value as Json;

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_cbioportal(
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
        let results = parser::parse_cbioportal_studies_json(&json);
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

    pub(in crate::domain::search::data) async fn fetch_cbioportal(
        &self,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let study_id = parser::normalize_cbioportal_study_id(identifier)
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
        parser::parse_cbioportal_study(&json)
            .ok_or_else(|| format!("cBioPortal did not return a parseable study for `{study_id}`"))
    }
}
