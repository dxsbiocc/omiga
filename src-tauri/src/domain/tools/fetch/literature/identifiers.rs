use super::super::common::{
    clean_nonempty, metadata_string_from_result, normalized_source, resolve_url, string_from_result,
};
use super::super::FetchArgs;
use serde_json::Value as JsonValue;

const PUBMED_HOST: &str = "pubmed.ncbi.nlm.nih.gov";

pub(in crate::domain::tools::fetch::literature) fn resolve_pubmed_pmid(
    args: &FetchArgs,
) -> Option<String> {
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

pub(in crate::domain::tools::fetch) fn resolve_literature_source(
    args: &FetchArgs,
    requested_source: &str,
) -> String {
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

pub(in crate::domain::tools::fetch::literature) fn resolve_literature_identifier(
    args: &FetchArgs,
    source: &str,
) -> Option<String> {
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

pub(in crate::domain::tools::fetch::literature) fn resolve_semantic_scholar_id(
    args: &FetchArgs,
) -> Option<String> {
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

pub(in crate::domain::tools::fetch::literature) fn normalize_semantic_scholar_id(
    value: &str,
) -> String {
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
