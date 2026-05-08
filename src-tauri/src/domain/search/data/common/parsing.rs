use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{Map as JsonMap, Value as Json};

pub(in crate::domain::search::data) fn normalize_accession(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("ebi.ac.uk") && parsed.path().contains("/ena/browser/view/") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
        }
        if host.contains("ncbi.nlm.nih.gov") {
            for (key, val) in parsed.query_pairs() {
                if key.eq_ignore_ascii_case("acc") && !val.trim().is_empty() {
                    return Some(val.into_owned());
                }
            }
        }
    }
    Some(value.to_string())
}

pub(in crate::domain::search::data) fn displayed_link_for_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return url.to_string();
    };
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.");
    let mut out = host.to_string();
    let path = parsed.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    if let Some(query) = parsed.query().filter(|q| !q.is_empty()) {
        out.push('?');
        out.push_str(query);
    }
    out
}

pub(in crate::domain::search::data) fn string_field_any(
    map: &JsonMap<String, Json>,
    keys: &[&str],
) -> Option<String> {
    for key in keys {
        if let Some(value) = map.get(*key).and_then(json_string) {
            return Some(value);
        }
    }
    None
}

pub(in crate::domain::search::data) fn nested_string_field(
    map: &JsonMap<String, Json>,
    object_key: &str,
    keys: &[&str],
) -> Option<String> {
    let nested = map.get(object_key)?.as_object()?;
    string_field_any(nested, keys)
}

pub(in crate::domain::search::data) fn string_vec_field_any(
    map: &JsonMap<String, Json>,
    keys: &[&str],
) -> Vec<String> {
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            let out = items.iter().filter_map(json_string).collect::<Vec<_>>();
            if !out.is_empty() {
                return out;
            }
        }
        if let Some(value) = json_string(value) {
            let out = value
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !out.is_empty() {
                return out;
            }
        }
    }
    Vec::new()
}

pub(in crate::domain::search::data) fn json_u64_from_keys(
    map: &JsonMap<String, Json>,
    keys: &[&str],
) -> Option<u64> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(json_u64_from_string_or_number))
}

pub(in crate::domain::search::data) fn json_string(value: &Json) -> Option<String> {
    match value {
        Json::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        Json::Number(n) => Some(n.to_string()),
        Json::Array(items) => {
            let joined = items
                .iter()
                .filter_map(json_string)
                .collect::<Vec<_>>()
                .join(", ");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

pub(in crate::domain::search::data) fn json_number_string(value: &Json) -> Option<String> {
    value
        .as_f64()
        .map(|v| {
            if v.fract() == 0.0 {
                format!("{v:.0}")
            } else {
                v.to_string()
            }
        })
        .or_else(|| value.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

pub(in crate::domain::search::data) fn json_u64_from_string_or_number(value: &Json) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
}

pub(in crate::domain::search::data) fn clean_html_text(value: &str) -> String {
    lazy_static! {
        static ref RE_TAG: Regex = Regex::new(r#"(?is)<[^>]+>"#).unwrap();
        static ref RE_WS: Regex = Regex::new(r#"\s+"#).unwrap();
    }
    let without_tags = RE_TAG.replace_all(value, " ");
    let decoded = decode_xml_text(without_tags.as_ref());
    RE_WS.replace_all(decoded.trim(), " ").to_string()
}

pub(in crate::domain::search::data) fn clean_xml_fragment(value: &str) -> String {
    clean_html_text(value)
}

fn decode_xml_text(value: &str) -> String {
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

pub(in crate::domain::search::data) fn extract_xml_attr(
    xml: &str,
    tag: &str,
    attr: &str,
) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?is)<{}\b(?P<attrs>[^>]*)>"#,
        regex::escape(tag)
    ))
    .ok()?;
    let attr_re = Regex::new(&format!(
        r#"(?i)\b{}\s*=\s*["'](?P<value>[^"']+)["']"#,
        regex::escape(attr)
    ))
    .ok()?;
    for cap in re.captures_iter(xml) {
        let Some(attrs) = cap.name("attrs").map(|m| m.as_str()) else {
            continue;
        };
        if let Some(value) = attr_re
            .captures(attrs)
            .and_then(|c| c.name("value"))
            .map(|m| decode_xml_text(m.as_str()))
        {
            return Some(value);
        }
    }
    None
}

pub(in crate::domain::search::data) fn extract_first_xml_tag(
    xml: &str,
    tags: &[&str],
) -> Option<String> {
    for tag in tags {
        let re = Regex::new(&format!(
            r#"(?is)<{}\b[^>]*>(?P<body>.*?)</{}>"#,
            regex::escape(tag),
            regex::escape(tag)
        ))
        .ok()?;
        if let Some(value) = re
            .captures(xml)
            .and_then(|cap| cap.name("body"))
            .map(|m| clean_xml_fragment(m.as_str()))
            .filter(|s| !s.trim().is_empty())
        {
            return Some(value);
        }
    }
    None
}

pub(in crate::domain::search::data) fn extract_ena_file_links(xml: &str) -> Vec<String> {
    lazy_static! {
        static ref RE_XREF: Regex =
            Regex::new(r#"(?is)<XREF_LINK\b[^>]*>(?P<body>.*?)</XREF_LINK>"#).unwrap();
    }
    let mut files = Vec::new();
    for cap in RE_XREF.captures_iter(xml) {
        let body = cap.name("body").map(|m| m.as_str()).unwrap_or("");
        if let Some(url) = extract_first_xml_tag(body, &["URL"]).filter(|u| {
            let lower = u.to_ascii_lowercase();
            lower.starts_with("ftp://")
                || lower.starts_with("http://")
                || lower.starts_with("https://")
        }) {
            files.push(url);
        }
    }
    files
}

pub(in crate::domain::search::data) fn truncate_chars(value: &str, max_chars: usize) -> String {
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

pub(in crate::domain::search::data) fn truncate_for_error(value: &str) -> String {
    truncate_chars(value, 500)
}

pub(in crate::domain::search::data) fn encode_path_segment(value: &str) -> String {
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

pub(in crate::domain::search::data) fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
