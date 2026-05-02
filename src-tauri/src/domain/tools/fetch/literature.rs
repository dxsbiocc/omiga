mod identifiers;
mod public_sources;
mod pubmed;
mod semantic_scholar;

pub(super) use identifiers::resolve_literature_source;
pub(super) use public_sources::fetch_public_literature_json;
pub(super) use pubmed::fetch_pubmed_json;
pub(super) use semantic_scholar::fetch_semantic_scholar_json;

#[cfg(test)]
mod tests {
    use super::super::FetchArgs;
    use super::identifiers::{
        normalize_semantic_scholar_id, resolve_literature_identifier, resolve_literature_source,
        resolve_pubmed_pmid, resolve_semantic_scholar_id,
    };
    use serde_json::json;

    #[test]
    fn resolves_pubmed_pmid_from_url_or_metadata() {
        let from_url = FetchArgs {
            category: "literature".into(),
            source: Some("pubmed".into()),
            subcategory: None,
            url: Some("https://pubmed.ncbi.nlm.nih.gov/12345678/".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_pubmed_pmid(&from_url).as_deref(), Some("12345678"));

        let from_result = FetchArgs {
            category: "literature".into(),
            source: Some("pubmed".into()),
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({"metadata":{"pmid":"42"}})),
            prompt: None,
        };
        assert_eq!(resolve_pubmed_pmid(&from_result).as_deref(), Some("42"));
    }

    #[test]
    fn resolves_literature_source_and_identifier_for_public_sources() {
        let from_arxiv_result = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "source": "arxiv",
                "link": "https://arxiv.org/abs/2401.01234",
                "metadata": {"arxiv_id": "2401.01234"}
            })),
            prompt: None,
        };
        assert_eq!(
            resolve_literature_source(&from_arxiv_result, "auto"),
            "arxiv"
        );
        assert_eq!(
            resolve_literature_identifier(&from_arxiv_result, "arxiv").as_deref(),
            Some("2401.01234")
        );

        let from_doi_url = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: Some("https://doi.org/10.1000/example".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_literature_source(&from_doi_url, "auto"), "crossref");
        assert_eq!(
            resolve_literature_identifier(&from_doi_url, "crossref").as_deref(),
            Some("https://doi.org/10.1000/example")
        );

        let from_openalex_metadata = FetchArgs {
            category: "literature".into(),
            source: Some("openalex".into()),
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "id": "ignored",
                "metadata": {"openalex_id": "W123"}
            })),
            prompt: None,
        };
        assert_eq!(
            resolve_literature_identifier(&from_openalex_metadata, "openalex").as_deref(),
            Some("W123")
        );

        let from_doi_id = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: None,
            id: Some("10.1000/example".into()),
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_literature_source(&from_doi_id, "auto"), "crossref");

        let from_arxiv_id = FetchArgs {
            category: "literature".into(),
            source: None,
            subcategory: None,
            url: None,
            id: Some("2401.01234v2".into()),
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_literature_source(&from_arxiv_id, "auto"), "arxiv");
    }

    #[test]
    fn resolves_semantic_scholar_ids_from_common_inputs() {
        let from_doi = FetchArgs {
            category: "literature".into(),
            source: Some("semantic_scholar".into()),
            subcategory: None,
            url: None,
            id: Some("10.1000/example".into()),
            result: None,
            prompt: None,
        };
        assert_eq!(
            resolve_semantic_scholar_id(&from_doi).as_deref(),
            Some("DOI:10.1000/example")
        );

        let from_slug_url = FetchArgs {
            category: "literature".into(),
            source: Some("semantic_scholar".into()),
            subcategory: None,
            url: Some("https://www.semanticscholar.org/paper/A-title/abcdef123456".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(
            resolve_semantic_scholar_id(&from_slug_url).as_deref(),
            Some("abcdef123456")
        );

        let from_metadata = FetchArgs {
            category: "literature".into(),
            source: Some("semantic_scholar".into()),
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "metadata": {"paper_id": "paper-1", "doi": "10.1000/fallback"}
            })),
            prompt: None,
        };
        assert_eq!(
            resolve_semantic_scholar_id(&from_metadata).as_deref(),
            Some("paper-1")
        );

        assert_eq!(
            normalize_semantic_scholar_id("doi:10.1000/example"),
            "DOI:10.1000/example"
        );
    }
}
