//! Literature provider HTTP operations.

use super::{
    encode_path_segment, normalize_arxiv_identifier, normalize_doi, normalize_openalex_identifier,
    parse_arxiv_atom, parse_crossref_item, parse_crossref_json, parse_openalex_item,
    parse_openalex_json, parse_preprint_json, truncate_chars, LiteraturePaper,
    LiteratureSearchArgs, LiteratureSearchResponse, PublicLiteratureClient, PublicLiteratureSource,
    ARXIV_API_URL, BIORXIV_API_URL, CROSSREF_WORKS_URL, MEDRXIV_API_URL, OPENALEX_WORKS_URL,
    PREPRINT_MAX_SCAN_PAGES, PREPRINT_SEARCH_WINDOW_DAYS,
};
use serde_json::Value as Json;

impl PublicLiteratureClient {
    pub(super) async fn search_arxiv(
        &self,
        args: LiteratureSearchArgs,
    ) -> Result<LiteratureSearchResponse, String> {
        let max_results = args.normalized_max_results();
        let response = self
            .http
            .get(ARXIV_API_URL)
            .query(&[
                ("search_query", format!("all:{}", args.query.trim())),
                ("start", "0".to_string()),
                ("max_results", max_results.to_string()),
                ("sortBy", "relevance".to_string()),
                ("sortOrder", "descending".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("arXiv search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read arXiv response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "arXiv search returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        Ok(LiteratureSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicLiteratureSource::Arxiv,
            total: None,
            results: parse_arxiv_atom(&body)
                .into_iter()
                .take(max_results as usize)
                .collect(),
            notes: vec!["arXiv official Atom API".to_string()],
        })
    }

    pub(super) async fn fetch_arxiv(&self, identifier: &str) -> Result<LiteraturePaper, String> {
        let arxiv_id = normalize_arxiv_identifier(identifier)
            .ok_or_else(|| "arXiv fetch requires an arXiv id or arxiv.org URL".to_string())?;
        let response = self
            .http
            .get(ARXIV_API_URL)
            .query(&[("id_list", arxiv_id.clone())])
            .send()
            .await
            .map_err(|e| format!("arXiv fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read arXiv fetch response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "arXiv fetch returned HTTP {}: {}",
                status.as_u16(),
                truncate_chars(&body, 240)
            ));
        }
        parse_arxiv_atom(&body)
            .into_iter()
            .next()
            .ok_or_else(|| format!("arXiv did not return a record for `{arxiv_id}`"))
    }

    pub(super) async fn search_crossref(
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

    pub(super) async fn fetch_crossref(&self, identifier: &str) -> Result<LiteraturePaper, String> {
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

    pub(super) async fn search_openalex(
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

    pub(super) async fn fetch_openalex(&self, identifier: &str) -> Result<LiteraturePaper, String> {
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

    pub(super) async fn search_preprint(
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

    pub(super) async fn fetch_preprint(
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
