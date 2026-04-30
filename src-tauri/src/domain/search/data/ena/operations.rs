//! ENA Portal and Browser request execution.

use super::super::common;
use super::super::common::*;
use super::super::PublicDataClient;
use super::{fields, parser, query};
use serde_json::Value as Json;

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_ena(
        &self,
        source: PublicDataSource,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let limit = args.normalized_max_results();
        let result = source
            .ena_result()
            .ok_or_else(|| "GEO is not an ENA source".to_string())?;
        let query = query::ena_portal_query(source, args.query.trim());
        let fields = fields::ena_fields(source);
        let response = self
            .http
            .get(&self.base_urls.ena_portal_search)
            .query(&[
                ("result", result.to_string()),
                ("query", query),
                ("fields", fields),
                ("format", "json".to_string()),
                ("limit", limit.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("ENA Portal search request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read ENA Portal search response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ENA Portal search returned HTTP {}: {}",
                status.as_u16(),
                common::truncate_for_error(&body)
            ));
        }
        let json: Json =
            serde_json::from_str(&body).map_err(|e| format!("parse ENA Portal JSON: {e}"))?;
        let results = parser::parse_ena_portal_json(source, &json);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: source.as_str().to_string(),
            total: Some(results.len() as u64),
            results,
            notes: vec![
                format!("ENA Portal API {result} search"),
                "Simple free-text queries are translated to source-specific wildcard fields; advanced ENA query syntax is passed through.".to_string(),
            ],
        })
    }

    pub(in crate::domain::search::data) async fn fetch_ena(
        &self,
        source: PublicDataSource,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let accession = common::normalize_accession(identifier)
            .ok_or_else(|| "ENA fetch requires an accession or ENA Browser URL".to_string())?;
        let source = if matches!(source, PublicDataSource::EnaStudy) {
            query::infer_ena_source_from_accession(&accession).unwrap_or(source)
        } else {
            source
        };
        let result = source
            .ena_result()
            .ok_or_else(|| "GEO is not an ENA source".to_string())?;
        let query = query::ena_accession_query(source, &accession);
        let fields = fields::ena_fields(source);
        let response = self
            .http
            .get(&self.base_urls.ena_portal_search)
            .query(&[
                ("result", result.to_string()),
                ("query", query),
                ("fields", fields),
                ("format", "json".to_string()),
                ("limit", "1".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("ENA Portal fetch request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read ENA Portal fetch response: {e}"))?;
        if status.is_success() {
            let json: Json =
                serde_json::from_str(&body).map_err(|e| format!("parse ENA Portal JSON: {e}"))?;
            if let Some(record) = parser::parse_ena_portal_json(source, &json)
                .into_iter()
                .next()
            {
                return Ok(record);
            }
        }

        let url = format!(
            "{}/{}",
            self.base_urls.ena_browser_xml,
            common::encode_path_segment(&accession)
        );
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("ENA Browser XML fetch request failed: {e}"))?;
        let status = response.status();
        let xml = response
            .text()
            .await
            .map_err(|e| format!("read ENA Browser XML response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "ENA fetch returned HTTP {}: {}",
                status.as_u16(),
                common::truncate_for_error(&xml)
            ));
        }
        parser::parse_ena_xml_record(source, &xml, &accession)
            .ok_or_else(|| format!("ENA did not return a parseable record for `{accession}`"))
    }
}
