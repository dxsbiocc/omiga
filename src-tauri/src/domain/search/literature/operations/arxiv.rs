//! arXiv HTTP operations.

use super::super::{
    normalize_arxiv_identifier, parse_arxiv_atom, truncate_chars, LiteraturePaper,
    LiteratureSearchArgs, LiteratureSearchResponse, PublicLiteratureClient, PublicLiteratureSource,
    ARXIV_API_URL,
};

impl PublicLiteratureClient {
    pub(in crate::domain::search::literature) async fn search_arxiv(
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

    pub(in crate::domain::search::literature) async fn fetch_arxiv(
        &self,
        identifier: &str,
    ) -> Result<LiteraturePaper, String> {
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
}
