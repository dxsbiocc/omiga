//! bioRxiv/medRxiv HTTP operations.

use super::super::{
    normalize_doi, parse_preprint_json, truncate_chars, LiteraturePaper, LiteratureSearchArgs,
    LiteratureSearchResponse, PublicLiteratureClient, PublicLiteratureSource, BIORXIV_API_URL,
    MEDRXIV_API_URL, PREPRINT_MAX_SCAN_PAGES, PREPRINT_SEARCH_WINDOW_DAYS,
};
use serde_json::Value as Json;

impl PublicLiteratureClient {
    pub(in crate::domain::search::literature) async fn search_preprint(
        &self,
        source: PublicLiteratureSource,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let base_url = match source {
            PublicLiteratureSource::Biorxiv => BIORXIV_API_URL,
            PublicLiteratureSource::Medrxiv => MEDRXIV_API_URL,
            _ => return Err(format!("unsupported preprint source {}", source.as_str())),
        };
        let mut results = Vec::new();
        let mut cursor = 0_u32;
        let query = args.query.trim().to_string();
        for _ in 0..PREPRINT_MAX_SCAN_PAGES {
            let url = format!("{base_url}/{PREPRINT_SEARCH_WINDOW_DAYS}d/{cursor}/json");
            let response = self
                .http
                .get(url)
                .send()
                .await
                .map_err(|e| format!("{} search request failed: {e}", source.as_str()))?;
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|e| format!("read {} response: {e}", source.as_str()))?;
            if !status.is_success() {
                return Err(format!(
                    "{} search returned HTTP {}: {}",
                    source.as_str(),
                    status.as_u16(),
                    truncate_chars(&body, 240)
                ));
            }
            let json: Json = serde_json::from_str(&body)
                .map_err(|e| format!("parse {} JSON: {e}", source.as_str()))?;
            let mut page = parse_preprint_json(source, &json, &query);
            results.append(&mut page);
            if results.len() >= max_results as usize {
                break;
            }
            let page_len = json
                .get("collection")
                .and_then(Json::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if page_len < 100 {
                break;
            }
            cursor += 100;
        }
        results.truncate(max_results as usize);
        Ok(LiteratureSearchResponse {
            query,
            source,
            total: None,
            results,
            notes: vec![format!(
                "{} scans recent {} days and filters title/abstract/category locally",
                source.as_str(),
                PREPRINT_SEARCH_WINDOW_DAYS
            )],
        })
    }

    pub(in crate::domain::search::literature) async fn fetch_preprint(
        &self,
        source: PublicLiteratureSource,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
        let doi = normalize_doi(identifier);
        if doi.is_empty() {
            return Err(format!(
                "{} fetch requires a DOI or DOI URL",
                source.as_str()
            ));
        }
        let base_url = match source {
            PublicLiteratureSource::Biorxiv => BIORXIV_API_URL,
            PublicLiteratureSource::Medrxiv => MEDRXIV_API_URL,
            _ => return Err(format!("unsupported preprint source {}", source.as_str())),
        };
        let url = format!("{base_url}/{doi}/na/json");
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("{} fetch request failed: {e}", source.as_str()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read {} fetch response: {e}", source.as_str()))?;
        if !status.is_success() {
            return Err(format!(
                "{} fetch returned HTTP {}: {}",
                source.as_str(),
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        let json: Json = serde_json::from_str(&body)
            .map_err(|e| format!("parse {} JSON: {e}", source.as_str()))?;
        parse_preprint_json(source, &json, "")
            .into_iter()
            .next()
            .ok_or_else(|| {
                format!(
                    "{} did not return a parseable work for `{doi}`",
                    source.as_str()
                )
            })
    }
}
