//! NCBI GEO DataSets adapter backed by Entrez E-utilities.

mod operations;
mod parser;

pub fn looks_like_geo_accession(value: &str) -> bool {
    parser::looks_like_geo_accession(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_geo_esearch_and_esummary() {
        let search = json!({"esearchresult": {"count": "1", "idlist": ["200000001"], "querytranslation": "cancer"}});
        let (count, ids, translation) = parser::parse_geo_esearch(&search).unwrap();
        assert_eq!(count, 1);
        assert_eq!(ids, vec!["200000001"]);
        assert_eq!(translation.as_deref(), Some("cancer"));

        let summary = json!({"result": {"uids": ["200000001"], "200000001": {"uid": "200000001", "accession": "GSE123", "title": "<b>RNA-seq study</b>", "summary": "A useful dataset", "gdsType": "Expression profiling by high throughput sequencing", "taxon": "Homo sapiens", "n_samples": "42", "GPL": "GPL20301", "PDAT": "2024/01/02"}}});
        let records = parser::parse_geo_esummary(&summary, &ids);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].accession, "GSE123");
        assert_eq!(records[0].title, "RNA-seq study");
        assert_eq!(records[0].sample_count, Some(42));
        assert!(records[0].url.contains("GSE123"));
    }
}
