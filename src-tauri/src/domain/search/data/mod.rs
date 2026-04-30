//! Public biological data-source adapters used by the built-in `search` / `fetch` tools.
//!
//! GEO is backed by official NCBI Entrez E-utilities (`db=gds`). ENA uses the
//! official ENA Portal API for indexed record searches and the Browser API XML
//! endpoint as a detail fallback. cBioPortal uses the public REST API for
//! cancer genomics study discovery/detail. GTEx uses the public GTEx Portal API
//! v2 for gene/tissue/expression metadata.

mod cbioportal;
mod common;
mod ena;
mod geo;
mod gtex;

use crate::domain::tools::ToolContext;
use common::EntrezSettings;
use std::time::Duration;

pub use common::{
    detail_to_json, search_response_to_json, DataApiBaseUrls, DataRecord, DataSearchArgs,
    DataSearchResponse, PublicDataSource,
};
pub use ena::{inferred_ena_source_key, looks_like_ena_accession};
pub use geo::looks_like_geo_accession;
pub use gtex::looks_like_gtex_identifier;

#[derive(Clone)]
pub struct PublicDataClient {
    pub(in crate::domain::search::data) http: reqwest::Client,
    pub(in crate::domain::search::data) base_urls: DataApiBaseUrls,
    pub(in crate::domain::search::data) settings: EntrezSettings,
}

impl PublicDataClient {
    pub fn from_tool_context(ctx: &ToolContext) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(ctx.timeout_secs.clamp(5, 60)))
            .user_agent(format!("Omiga-DataSearch/{}", env!("CARGO_PKG_VERSION")));
        if !ctx.web_use_proxy {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| format!("build data-source HTTP client: {e}"))?;
        Ok(Self {
            http,
            base_urls: ctx.data_api_base_urls.clone(),
            settings: EntrezSettings::from_keys(&ctx.web_search_api_keys),
        })
    }

    pub async fn search(
        &self,
        source: PublicDataSource,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        match source {
            PublicDataSource::Geo => self.search_geo(args).await,
            PublicDataSource::CbioPortal => self.search_cbioportal(args).await,
            PublicDataSource::Gtex => self.search_gtex(args).await,
            source => self.search_ena(source, args).await,
        }
    }

    pub async fn search_auto(&self, args: DataSearchArgs) -> Result<DataSearchResponse, String> {
        if args.query.trim().len() < 2 {
            return Err("data search query must contain at least 2 characters".to_string());
        }
        let max_results = args.normalized_max_results() as usize;
        let geo_args = args.clone();
        let ena_args = args.clone();
        let gtex_args = args.clone();
        let (geo, ena, gtex) = tokio::join!(
            self.search_geo(geo_args),
            self.search_ena(PublicDataSource::EnaStudy, ena_args),
            self.search_gtex(gtex_args)
        );

        let mut results = Vec::new();
        let mut total = 0u64;
        let mut saw_total = false;
        let mut notes = vec!["Combined GEO + ENA + GTEx data search".to_string()];

        for response in [geo, ena, gtex] {
            match response {
                Ok(response) => {
                    if let Some(count) = response.total {
                        total = total.saturating_add(count);
                        saw_total = true;
                    }
                    notes.extend(response.notes);
                    results.extend(response.results);
                }
                Err(err) => notes.push(format!("source failed: {err}")),
            }
        }

        results.truncate(max_results);
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: "auto".to_string(),
            total: saw_total.then_some(total),
            results,
            notes,
        })
    }

    pub async fn fetch(
        &self,
        source: PublicDataSource,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(format!("{} fetch requires a non-empty id", source.as_str()));
        }
        match source {
            PublicDataSource::Geo => self.fetch_geo(identifier).await,
            PublicDataSource::CbioPortal => self.fetch_cbioportal(identifier).await,
            PublicDataSource::Gtex => self.fetch_gtex(identifier).await,
            source => self.fetch_ena(source, identifier).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map as JsonMap;

    #[test]
    fn data_json_uses_serpapi_shape() {
        let record = DataRecord {
            id: "GSE123".to_string(),
            accession: "GSE123".to_string(),
            source: PublicDataSource::Geo,
            title: "Dataset".to_string(),
            summary: "Summary".to_string(),
            url: "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSE123".to_string(),
            record_type: Some("Series".to_string()),
            organism: Some("Homo sapiens".to_string()),
            published_date: None,
            updated_date: None,
            sample_count: Some(3),
            platform: None,
            files: Vec::new(),
            extra: JsonMap::new(),
        };
        let response = DataSearchResponse {
            query: "test".to_string(),
            source: "geo".to_string(),
            total: Some(1),
            results: vec![record.clone()],
            notes: vec![],
        };
        let json = search_response_to_json(&response);
        assert_eq!(json["category"], "data");
        assert_eq!(json["results"][0]["source"], "geo");
        assert_eq!(json["results"][0]["metadata"]["organism"], "Homo sapiens");

        let detail = detail_to_json(&record);
        assert_eq!(detail["category"], "data");
        assert!(detail["content"]
            .as_str()
            .unwrap()
            .contains("Source: NCBI GEO"));
    }

    #[test]
    fn recognizes_data_accessions_and_urls() {
        assert!(looks_like_geo_accession("GSE12345"));
        assert!(looks_like_geo_accession(
            "https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSM575"
        ));
        assert!(looks_like_ena_accession("PRJEB12345"));
        assert!(looks_like_ena_accession(
            "https://www.ebi.ac.uk/ena/browser/view/ERR123"
        ));
        assert_eq!(
            common::normalize_accession("https://www.ebi.ac.uk/ena/browser/view/PRJEB123")
                .as_deref(),
            Some("PRJEB123")
        );
        assert!(looks_like_gtex_identifier(
            "https://gtexportal.org/home/gene/ENSG00000132693.12"
        ));
    }
}
