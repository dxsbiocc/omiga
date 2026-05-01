//! Public biological data-source adapters used by the built-in `search` / `fetch` tools.
//!
//! GEO is backed by official NCBI Entrez E-utilities (`db=gds`). ENA uses the
//! official ENA Portal API for indexed record searches and the Browser API XML
//! endpoint as a detail fallback. cBioPortal uses the public REST API for
//! cancer genomics study discovery/detail. GTEx uses the public GTEx Portal API
//! v2 for gene/tissue/expression metadata. NCBI Datasets uses the official v2
//! REST API for genome assembly metadata and metadata-only package links.

mod cbioportal;
mod client;
mod common;
mod ena;
mod geo;
mod gtex;
mod ncbi_datasets;
mod routing;

pub use client::PublicDataClient;
pub use common::{
    detail_to_json, search_response_to_json, DataApiBaseUrls, DataRecord, DataSearchArgs,
    DataSearchResponse, PublicDataSource,
};
pub use ena::{inferred_ena_source_key, looks_like_ena_accession};
pub use geo::looks_like_geo_accession;
pub use gtex::looks_like_gtex_identifier;
pub use ncbi_datasets::looks_like_ncbi_datasets_accession;

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
        assert!(looks_like_ncbi_datasets_accession("GCF_000001405.40"));
        assert!(looks_like_ncbi_datasets_accession(
            "https://www.ncbi.nlm.nih.gov/datasets/genome/GCA_000001405.29/"
        ));
    }
}
