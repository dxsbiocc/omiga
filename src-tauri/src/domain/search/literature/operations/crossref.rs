//! Crossref HTTP operations.

use super::super::{
    encode_path_segment, normalize_doi, parse_crossref_item, parse_crossref_json, truncate_chars,
    LiteraturePaper, LiteratureSearchArgs, LiteratureSearchResponse, PublicLiteratureClient,
    PublicLiteratureSource, CROSSREF_WORKS_URL,
};
use serde_json::Value as Json;

impl PublicLiteratureClient {
    pub(in crate::domain::search::literature) async fn search_crossref(
        &self,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let response = self
            .http
            .get(CROSSREF_WORKS_URL)
            .query(&[
                ("query", args.query.trim().to_string()),
                ("rows", max_results.to_string()),
                ("sort", "relevance".to_string()),
                ("order", "desc".to_string()),
                ("mailto", self.mailto.clone()),
            ])
            .send()
            .await
            .map_err(|e| format!("Crossref search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read Crossref response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "Crossref search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse Crossref JSON: {e}"))?;
        let total = json
            .pointer("/message/total-results")
            .and_then(Json::as_u64);
        Ok(LiteratureSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicLiteratureSource::Crossref,
            total,
            results: parse_crossref_json(&json)
                .into_iter()
                .take(max_results as usize)
                .collect(),
            notes: vec!["Crossref REST API metadata search".to_string()],
        })
    }

    pub(in crate::domain::search::literature) async fn fetch_crossref(
        &self,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
        let doi = normalize_doi(identifier);
        if doi.is_empty() {
            return Err("Crossref fetch requires a DOI or DOI URL".to_string());
        }
        let url = format!("{CROSSREF_WORKS_URL}/{}", encode_path_segment(&doi));
        let response = self
            .http
            .get(url)
            .query(&[("mailto", self.mailto.clone())])
            .send()
            .await
            .map_err(|e| format!("Crossref fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read Crossref fetch response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "Crossref fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse Crossref JSON: {e}"))?;
        parse_crossref_item(json.get("message").unwrap_or(&json))
            .ok_or_else(|| format!("Crossref did not return a parseable work for `{doi}`"))
    }
}
