//! cBioPortal study discovery/detail adapter.

mod operations;
mod parser;

#[cfg(test)]
mod tests {
    use super::super::common::PublicDataSource;
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_cbioportal_study_json() {
        let value = json!([{
            "studyId": "brca_tcga",
            "name": "Breast Invasive Carcinoma (TCGA, PanCancer Atlas)",
            "description": "TCGA breast cancer study",
            "cancerTypeId": "brca",
            "allSampleCount": 1084,
            "citation": "TCGA, Cell 2018",
            "pmid": "29625048",
            "importDate": "2025-01-01"
        }]);
        let records = parser::parse_cbioportal_studies_json(&value);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "brca_tcga");
        assert_eq!(records[0].source, PublicDataSource::CbioPortal);
        assert_eq!(records[0].sample_count, Some(1084));
        assert_eq!(records[0].organism.as_deref(), Some("brca"));
        assert!(records[0].url.contains("id=brca_tcga"));
        assert_eq!(
            parser::normalize_cbioportal_study_id(
                "https://www.cbioportal.org/study/summary?id=brca_tcga"
            )
            .as_deref(),
            Some("brca_tcga")
        );
    }
}
