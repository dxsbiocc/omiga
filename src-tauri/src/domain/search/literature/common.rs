//! Shared parsing and formatting helpers for literature adapters.

use chrono::NaiveDate;
use lazy_static::lazy_static;
use regex::Regex;

pub(in crate::domain::search::literature) fn first_xml_tag(
    block: &str,
    tag: &str,
) -> Option<String> {
    let pattern = format!(
        r#"(?is)<(?:[A-Za-z0-9_-]+:)?{}\b[^>]*>(.*?)</(?:[A-Za-z0-9_-]+:)?{}>"#,
        regex::escape(tag),
        regex::escape(tag)
    );
    Regex::new(&pattern)
        .ok()?
        .captures(block)?
        .get(1)
        .map(|m| m.as_str().to_string())
}

pub(in crate::domain::search::literature) fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?is)\b{}\s*=\s*["']([^"']*)["']"#, regex::escape(name));
    Regex::new(&pattern)
        .ok()?
        .captures(attrs)?
        .get(1)
        .map(|m| decode_html_entities(m.as_str()).trim().to_string())
}

pub(in crate::domain::search::literature) fn clean_xml_text(value: &str) -> String {
    clean_html_text(value)
}

pub(in crate::domain::search::literature) fn clean_html_text(value: &str) -> String {
    lazy_static! {
        static ref RE_TAG: Regex = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).unwrap();
    }
    let without_tags = RE_TAG.replace_all(value, "");
    let decoded = decode_html_entities(without_tags.as_ref());
    RE_WS.replace_all(decoded.trim(), " ").to_string()
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

pub(in crate::domain::search::literature) fn normalize_doi(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["https://doi.org/", "http://doi.org/", "doi:"] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..].trim().to_string();
        }
    }
    trimmed.to_string()
}

pub(in crate::domain::search::literature) fn normalize_arxiv_identifier(
    value: &str,
) -> Option<String> {
    let mut value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(&value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.ends_with("arxiv.org") {
            let path = parsed.path().trim_matches('/');
            value = path
                .strip_prefix("abs/")
                .or_else(|| path.strip_prefix("pdf/"))
                .unwrap_or(path)
                .trim_end_matches(".pdf")
                .to_string();
        }
    }
    value = value.trim_end_matches(".pdf").trim().to_string();
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("arxiv:") {
        value = value["arxiv:".len()..].trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

pub(in crate::domain::search::literature) fn normalize_openalex_identifier(
    value: &str,
) -> Option<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        return None;
    }
    if value.to_ascii_lowercase().starts_with("doi:")
        || value.to_ascii_lowercase().starts_with("pmid:")
        || value.to_ascii_lowercase().starts_with("pmcid:")
        || value.to_ascii_uppercase().starts_with('W')
    {
        return Some(value);
    }
    if let Ok(parsed) = reqwest::Url::parse(&value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.ends_with("openalex.org") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
        if host == "doi.org" || host.ends_with(".doi.org") {
            return Some(format!("doi:{}", normalize_doi(&value)));
        }
    }
    let doi = normalize_doi(&value);
    if doi.contains('/') {
        Some(format!("doi:{doi}"))
    } else {
        Some(value)
    }
}

pub(in crate::domain::search::literature) fn encode_path_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub(in crate::domain::search::literature) fn normalize_date_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.len() >= 10 {
        let candidate = &value[..10];
        if NaiveDate::parse_from_str(candidate, "%Y-%m-%d").is_ok() {
            return Some(candidate.to_string());
        }
    }
    Some(value.to_string())
}

pub(in crate::domain::search::literature) fn arxiv_id_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed
        .rsplit('/')
        .next()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub(in crate::domain::search::literature) fn truncate_chars(
    value: &str,
    max_chars: usize,
) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

pub(in crate::domain::search::literature) fn clean_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}
