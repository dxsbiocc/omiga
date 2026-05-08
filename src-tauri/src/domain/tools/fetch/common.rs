use super::FetchArgs;
use serde_json::{json, Value as JsonValue};

pub(super) fn normalized_source(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .replace('-', "_")
}

pub(super) fn normalized_subcategory(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase().replace(['-', ' '], "_"))
}

pub(super) fn resolve_url(args: &FetchArgs) -> Option<String> {
    args.url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| string_from_result(args, &["url", "link", "href"]))
}

pub(super) fn title_from_result(args: &FetchArgs) -> Option<String> {
    string_from_result(args, &["title", "name"])
}

pub(super) fn string_from_result(args: &FetchArgs, keys: &[&str]) -> Option<String> {
    let object = args.result.as_ref()?.as_object()?;
    for key in keys {
        let value = object
            .get(*key)
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(value) = value {
            return Some(value.to_string());
        }
    }
    None
}

pub(super) fn metadata_string_from_result(args: &FetchArgs, keys: &[&str]) -> Option<String> {
    let metadata = args.result.as_ref()?.get("metadata")?.as_object()?;
    for key in keys {
        let value = metadata
            .get(*key)
            .and_then(JsonValue::as_str)
            .and_then(clean_nonempty);
        if value.is_some() {
            return value;
        }
    }
    None
}

pub(super) fn clean_nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(super) fn displayed_link_for_url(url: &str) -> String {
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
    out
}

pub(super) fn favicon_for_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(format!(
        "https://www.google.com/s2/favicons?domain={}&sz=64",
        host.trim_start_matches("www.")
    ))
}

pub(super) fn structured_error_json(
    code: &str,
    category: &str,
    source: &str,
    message: impl Into<String>,
) -> JsonValue {
    json!({
        "error": code,
        "category": category,
        "source": source,
        "message": message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_url_from_search_result_link() {
        let args = FetchArgs {
            category: "web".into(),
            source: None,
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({"title":"A","link":"https://example.org/a"})),
            prompt: None,
        };
        assert_eq!(resolve_url(&args).as_deref(), Some("https://example.org/a"));
        assert_eq!(title_from_result(&args).as_deref(), Some("A"));
    }
}
