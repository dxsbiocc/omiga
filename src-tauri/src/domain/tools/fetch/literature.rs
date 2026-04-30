use super::common::{
    clean_nonempty, json_stream, metadata_string_from_result, normalized_source, resolve_url,
    string_from_result,
};
use super::FetchArgs;
use crate::domain::tools::{ToolContext, ToolError};
use serde_json::Value as JsonValue;

const PUBMED_HOST: &str = "pubmed.ncbi.nlm.nih.gov";
pub(super) async fn fetch_public_literature(
    ctx: &ToolContext,
    args: &FetchArgs,
    source: &str,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let source = crate::domain::search::literature::PublicLiteratureSource::parse(source)
        .ok_or_else(|| ToolError::InvalidArguments {
            message: format!("Unsupported public literature source: {source}"),
        })?;
    let identifier = resolve_literature_identifier(args, source.as_str()).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: format!(
                "fetch(category=literature, source={}) requires `id`, `url`, DOI/arXiv/OpenAlex identifier, or a search `result`",
                source.as_str()
            ),
        }
    })?;
    let client = crate::domain::search::literature::PublicLiteratureClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let paper = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(source, &identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(
        crate::domain::search::literature::paper_to_detail_json(&paper),
    ))
}

pub(super) async fn fetch_semantic_scholar(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let paper_id = resolve_semantic_scholar_id(args).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: "Semantic Scholar fetch requires a paper id, DOI/arXiv/PubMed external id, URL, or search result".to_string(),
        }
    })?;
    let client =
        crate::domain::search::semantic_scholar::SemanticScholarClient::from_tool_context(ctx)
            .map_err(|message| ToolError::ExecutionFailed { message })?;
    let paper = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(&paper_id) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(
        crate::domain::search::semantic_scholar::detail_to_json(&paper),
    ))
}

pub(super) async fn fetch_pubmed(
    ctx: &ToolContext,
    args: &FetchArgs,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let pmid = resolve_pubmed_pmid(args).ok_or_else(|| ToolError::InvalidArguments {
        message: "PubMed fetch expects a numeric PMID via `id`, a PubMed `url`, or a PubMed search `result`. DOI-to-PMID resolution is planned for a later version.".to_string(),
    })?;
    let client = crate::domain::search::pubmed::EntrezClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let detail = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch_by_pmid(&pmid) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(crate::domain::search::pubmed::detail_to_json(
        &detail,
    )))
}

fn resolve_pubmed_pmid(args: &FetchArgs) -> Option<String> {
    let raw = args
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| string_from_result(args, &["id", "pmid"]))
        .or_else(|| {
            args.result
                .as_ref()
                .and_then(|v| v.get("metadata"))
                .and_then(|m| m.get("pmid"))
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .or_else(|| resolve_url(args).and_then(|url| pmid_from_pubmed_url(&url)))?;
    let trimmed = raw.trim().trim_start_matches("PMID:").trim();
    trimmed
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| trimmed.to_string())
}

pub(super) fn resolve_literature_source(args: &FetchArgs, requested_source: &str) -> String {
    if requested_source != "auto" {
        return requested_source.to_string();
    }
    if let Some(source) = string_from_result(args, &["source", "effective_source"])
        .map(|s| normalized_source(Some(&s)))
        .filter(|s| s != "auto")
    {
        return source;
    }
    if resolve_pubmed_pmid(args).is_some() {
        return "pubmed".to_string();
    }
    if let Some(id) = args.id.as_deref().and_then(clean_nonempty) {
        if looks_like_arxiv_identifier(&id) {
            return "arxiv".to_string();
        }
        if looks_like_openalex_identifier(&id) {
            return "openalex".to_string();
        }
        if looks_like_doi_identifier(&id) {
            return "crossref".to_string();
        }
    }
    if let Some(url) = resolve_url(args) {
        let lower = url.to_ascii_lowercase();
        if lower.contains("arxiv.org/") {
            return "arxiv".to_string();
        }
        if lower.contains("openalex.org/") {
            return "openalex".to_string();
        }
        if lower.contains("biorxiv.org/") {
            return "biorxiv".to_string();
        }
        if lower.contains("medrxiv.org/") {
            return "medrxiv".to_string();
        }
        if lower.contains("doi.org/") {
            return "crossref".to_string();
        }
    }
    if let Some(arxiv_id) = metadata_string_from_result(args, &["arxiv_id", "arxiv"]) {
        if !arxiv_id.is_empty() {
            return "arxiv".to_string();
        }
    }
    if let Some(doi) = metadata_string_from_result(args, &["doi"]) {
        if !doi.is_empty() {
            return "crossref".to_string();
        }
    }
    "pubmed".to_string()
}

fn resolve_literature_identifier(args: &FetchArgs, source: &str) -> Option<String> {
    let source = normalized_source(Some(source));
    match source.as_str() {
        "pubmed" => resolve_pubmed_pmid(args),
        "arxiv" => args
            .id
            .as_deref()
            .and_then(clean_nonempty)
            .or_else(|| metadata_string_from_result(args, &["arxiv_id", "arxiv"]))
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| resolve_url(args)),
        "openalex" => metadata_string_from_result(args, &["openalex_id", "openalex"])
            .or_else(|| args.id.as_deref().and_then(clean_nonempty))
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| metadata_string_from_result(args, &["doi"]))
            .or_else(|| resolve_url(args)),
        "crossref" | "biorxiv" | "medrxiv" => metadata_string_from_result(args, &["doi"])
            .or_else(|| args.id.as_deref().and_then(clean_nonempty))
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| resolve_url(args)),
        _ => args
            .id
            .as_deref()
            .and_then(clean_nonempty)
            .or_else(|| string_from_result(args, &["id"]))
            .or_else(|| resolve_url(args)),
    }
}

fn resolve_semantic_scholar_id(args: &FetchArgs) -> Option<String> {
    if let Some(id) = args.id.as_deref().and_then(clean_nonempty) {
        return Some(normalize_semantic_scholar_id(&id));
    }
    if let Some(paper_id) = metadata_string_from_result(args, &["paper_id", "paperId"]) {
        return Some(paper_id);
    }
    if let Some(id) = string_from_result(args, &["id"]) {
        if !id.trim().is_empty() {
            return Some(normalize_semantic_scholar_id(&id));
        }
    }
    if let Some(url) = resolve_url(args) {
        if let Some(id) = semantic_scholar_id_from_url(&url) {
            return Some(id);
        }
    }
    if let Some(doi) = metadata_string_from_result(args, &["doi"]) {
        return Some(format!("DOI:{}", strip_doi_prefix(&doi)));
    }
    if let Some(arxiv) = metadata_string_from_result(args, &["arxiv_id", "arxiv"]) {
        return Some(format!("ARXIV:{arxiv}"));
    }
    if let Some(pmid) = metadata_string_from_result(args, &["pubmed_id", "pmid"]) {
        return Some(format!("PMID:{pmid}"));
    }
    None
}

fn normalize_semantic_scholar_id(value: &str) -> String {
    let value = value.trim();
    if let Some(id) = semantic_scholar_id_from_url(value) {
        return id;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("https://doi.org/") || lower.starts_with("http://doi.org/") {
        return format!("DOI:{}", strip_doi_prefix(value));
    }
    if lower.starts_with("doi:") {
        return format!("DOI:{}", strip_doi_prefix(value));
    }
    if lower.starts_with("arxiv:") {
        return format!("ARXIV:{}", value["arxiv:".len()..].trim());
    }
    if lower.starts_with("pmid:") {
        return format!("PMID:{}", value["pmid:".len()..].trim());
    }
    if !value.contains(':') && value.contains('/') && value.starts_with("10.") {
        return format!("DOI:{}", strip_doi_prefix(value));
    }
    value.to_string()
}

fn semantic_scholar_id_from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !host.ends_with("semanticscholar.org") {
        return None;
    }
    let segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    segments
        .iter()
        .position(|segment| *segment == "paper")
        .and_then(|idx| segments.get(idx + 1..))
        .and_then(|remaining| remaining.last().or_else(|| remaining.first()))
        .map(|s| s.to_string())
}

fn strip_doi_prefix(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["https://doi.org/", "http://doi.org/", "doi:"] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn looks_like_doi_identifier(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    lower.starts_with("doi:")
        || lower.starts_with("https://doi.org/")
        || lower.starts_with("http://doi.org/")
        || (value.starts_with("10.") && value.contains('/'))
}

fn looks_like_arxiv_identifier(value: &str) -> bool {
    let value = value.trim().trim_end_matches(".pdf");
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("arxiv:") || lower.contains("arxiv.org/") {
        return true;
    }
    let id = lower.trim_start_matches("arxiv:");
    let id = id
        .rsplit_once('v')
        .filter(|(_, version)| version.chars().all(|c| c.is_ascii_digit()))
        .map(|(base, _)| base)
        .unwrap_or(id);
    let mut parts = id.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(ym), Some(seq), None)
            if ym.len() == 4
                && ym.chars().all(|c| c.is_ascii_digit())
                && seq.len() >= 4
                && seq.chars().all(|c| c.is_ascii_digit())
    )
}

fn looks_like_openalex_identifier(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    lower.contains("openalex.org/")
        || value
            .strip_prefix('W')
            .or_else(|| value.strip_prefix('w'))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn pmid_from_pubmed_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !host.ends_with(PUBMED_HOST) {
        return None;
    }
    parsed
        .path_segments()?
        .find(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
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
