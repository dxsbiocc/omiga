//! OpenAlex HTTP operations.

use super::super::{
    encode_path_segment, normalize_openalex_identifier, parse_openalex_item, parse_openalex_json,
    truncate_chars, LiteraturePaper, LiteratureSearchArgs, LiteratureSearchResponse,
    PublicLiteratureClient, PublicLiteratureSource, OPENALEX_WORKS_URL,
};
use serde_json::Value as Json;

impl PublicLiteratureClient {
    pub(in crate::domain::search::literature) async fn search_openalex(
        &self,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let response = self
            .http
            .get(OPENALEX_WORKS_URL)
            .query(&[
                ("search", args.query.trim().to_string()),
                ("per-page", max_results.to_string()),
                ("mailto", self.mailto.clone()),
            ])
            .send()
            .await
            .map_err(|e| format!("OpenAlex search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read OpenAlex response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "OpenAlex search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse OpenAlex JSON: {e}"))?;
        let total = json.pointer("/meta/count").and_then(Json::as_u64);
        Ok(LiteratureSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicLiteratureSource::OpenAlex,
            total,
            results: parse_openalex_json(&json)
                .into_iter()
                .take(max_results as usize)
                .collect(),
            notes: vec!["OpenAlex Works API metadata search".to_string()],
        })
    }

    pub(in crate::domain::search::literature) async fn fetch_openalex(
        &self,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
        let work_id = normalize_openalex_identifier(identifier)
            .ok_or_else(|| "OpenAlex fetch requires an OpenAlex work id/URL or DOI".to_string())?;
        let url = format!("{OPENALEX_WORKS_URL}/{}", encode_path_segment(&work_id));
        let response = self
            .http
            .get(url)
            .query(&[("mailto", self.mailto.clone())])
            .send()
            .await
            .map_err(|e| format!("OpenAlex fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read OpenAlex fetch response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "OpenAlex fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse OpenAlex JSON: {e}"))?;
        parse_openalex_item(&json)
            .ok_or_else(|| format!("OpenAlex did not return a parseable work for `{work_id}`"))
    }
}
