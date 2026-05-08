use super::params;
use serde_json::{json, Value as Json};

#[cfg(debug_assertions)]
pub(super) fn mock_gtex_json(endpoint: &str, params: &[(String, String)]) -> Option<Json> {
    let endpoint = endpoint.trim_start_matches('/');
    let param = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    match endpoint {
        "reference/gene" => {
            let gene = param("geneId").unwrap_or("BRCA1");
            Some(json!({
                "data": [{
                    "gencodeId": if params::looks_like_gencode_id(gene) { gene } else { "ENSG00000012048.21" },
                    "geneSymbol": if params::looks_like_gencode_id(gene) { "BRCA1" } else { gene },
                    "description": "BRCA1 DNA repair associated",
                    "geneType": "protein_coding",
                    "chromosome": "chr17",
                    "start": 43044295,
                    "end": 43125482
                }],
                "paging_info": {"totalNumberOfItems": 1}
            }))
        }
        "expression/medianGeneExpression" => {
            let gene = param("gencodeId").unwrap_or("ENSG00000012048.21");
            let tissue = param("tissueSiteDetailId").unwrap_or("Whole_Blood");
            Some(json!({
                "data": [{
                    "gencodeId": gene,
                    "geneSymbol": "BRCA1",
                    "tissueSiteDetailId": tissue,
                    "median": 1.23,
                    "unit": "TPM"
                }],
                "paging_info": {"totalNumberOfItems": 1}
            }))
        }
        _ => None,
    }
}
