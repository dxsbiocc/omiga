//! GTEx Portal API v2 adapter.

mod client;
mod gene;
#[cfg(debug_assertions)]
mod mock;
mod operations;
mod params;
mod parser;

pub fn looks_like_gtex_identifier(value: &str) -> bool {
    params::looks_like_gtex_identifier(value)
}

#[cfg(test)]
mod tests {
    use super::super::common::PublicDataSource;
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_gtex_gene_and_expression_json() {
        assert_eq!(
            PublicDataSource::parse("gtex"),
            Some(PublicDataSource::Gtex)
        );

        let genes = json!({
            "data": [{
                "gencodeId": "ENSG00000012048.21",
                "geneSymbol": "BRCA1",
                "description": "BRCA1 DNA repair associated",
                "geneType": "protein_coding",
                "chromosome": "chr17",
                "start": 43044295,
                "end": 43125482
            }],
            "paging_info": {"totalNumberOfItems": 1}
        });
        let records = parser::parse_gtex_genes_json(&genes);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, PublicDataSource::Gtex);
        assert_eq!(records[0].accession, "ENSG00000012048.21");
        assert!(records[0].title.contains("BRCA1"));
        assert!(records[0].url.ends_with("/ENSG00000012048.21"));

        let expression = json!({
            "data": [{
                "gencodeId": "ENSG00000012048.21",
                "geneSymbol": "BRCA1",
                "tissueSiteDetailId": "Whole_Blood",
                "median": 1.23,
                "unit": "TPM"
            }]
        });
        let expression_records = parser::parse_gtex_median_expression_json(&expression);
        assert_eq!(expression_records.len(), 1);
        assert_eq!(
            expression_records[0].record_type.as_deref(),
            Some("median_gene_expression")
        );
        assert!(expression_records[0].summary.contains("1.23 TPM"));
        assert!(looks_like_gtex_identifier(
            "https://gtexportal.org/home/gene/ENSG00000012048.21"
        ));
    }
}
