//! GEO Entrez request execution.

use super::super::common::*;
use super::super::PublicDataClient;
use super::parser;
use serde_json::Value as Json;

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_geo(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let ret_max = args.normalized_max_results();
        let mut params = self.geo_entrez_params("json");
        params.push(("term".to_string(), args.query.trim().to_string()));
        params.push(("retmax".to_string(), ret_max.to_string()));

        let search_json = self.get_entrez_json("esearch", &params).await?;
        let (count, ids, query_translation) = parser::parse_geo_esearch(&search_json)?;
        if ids.is_empty() {
            return Ok(DataSearchResponse {
                query: args.query.trim().to_string(),
                source: "geo".to_string(),
                total: Some(count),
                results: Vec::new(),
                notes: vec![
                    "NCBI GEO DataSets ESearch returned no matching UIDs".to_string(),
                    query_translation
                        .map(|q| format!("NCBI query translation: {q}"))
                        .unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect(),
            });
        }

        let mut summary_params = self.geo_entrez_params("json");
        summary_params.push(("id".to_string(), ids.join(",")));
        let summary_json = self.get_entrez_json("esummary", &summary_params).await?;
        let results = parser::parse_geo_esummary(&summary_json, &ids);
        let mut notes = vec!["NCBI Entrez E-utilities db=gds".to_string()];
        if let Some(q) = query_translation {
            notes.push(format!("NCBI query translation: {q}"));
        }
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "geo".to_string(),
            total: Some(count),
            results,
            notes,
        })
    }

    pub(in crate::domain::search::data) async fn fetch_geo(
        &self,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let uid = if identifier.chars().all(|c| c.is_ascii_digit()) {
            identifier.to_string()
        } else {
            let mut params = self.geo_entrez_params("json");
            params.push(("term".to_string(), format!("{}[ACCN]", identifier.trim())));
            params.push(("retmax".to_string(), "1".to_string()));
            let json = self.get_entrez_json("esearch", &params).await?;
            let (_, ids, _) = parser::parse_geo_esearch(&json)?;
            ids.into_iter()
                .next()
                .ok_or_else(|| format!("GEO did not find accession `{identifier}`"))?
        };
        let mut params = self.geo_entrez_params("json");
        params.push(("id".to_string(), uid.clone()));
        let json = self.get_entrez_json("esummary", &params).await?;
        parser::parse_geo_esummary(&json, std::slice::from_ref(&uid))
            .into_iter()
            .next()
            .ok_or_else(|| format!("GEO did not return a parseable summary for `{uid}`"))
    }

    fn geo_entrez_params(&self, retmode: &str) -> Vec<(String, String)> {
        let mut params = vec![
            ("db".to_string(), "gds".to_string()),
            ("retmode".to_string(), retmode.to_string()),
            ("tool".to_string(), self.settings.tool.clone()),
            ("email".to_string(), self.settings.email.clone()),
        ];
        if let Some(api_key) = &self.settings.api_key {
            params.push(("api_key".to_string(), api_key.clone()));
        }
        params
    }

    async fn get_entrez_json(
        &self,
        utility: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        let url = format!("{}/{}.fcgi", self.base_urls.entrez, utility);
        let response = self
            .http
            .get(&url)
            .query(params)
            .send()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("NCBI Entrez {utility} response read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "NCBI Entrez {utility} returned HTTP {status}: {}",
                truncate_for_error(&body)
            ));
        }
        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "NCBI Entrez {utility} returned non-JSON response: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(error) = json.get("error").and_then(Json::as_str) {
            return Err(format!("NCBI Entrez {utility} error: {error}"));
        }
        Ok(json)
    }
}
