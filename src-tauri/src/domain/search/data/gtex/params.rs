use crate::domain::search::data::common::{self, DataSearchArgs};
use serde_json::Value as Json;

pub(super) fn gtex_mode(args: &DataSearchArgs) -> Option<String> {
    gtex_param_string(args, &["endpoint", "mode", "kind", "type"])
        .map(|value| value.trim().to_ascii_lowercase().replace(['-', ' '], "_"))
}

pub(super) fn gtex_dataset_id(args: &DataSearchArgs) -> Option<String> {
    gtex_param_string(args, &["datasetId", "dataset_id", "dataset"])
}

pub(super) fn gtex_page(args: &DataSearchArgs) -> u32 {
    gtex_param_u32(args, &["page"]).unwrap_or(0)
}

pub(super) fn gtex_param_string(args: &DataSearchArgs, keys: &[&str]) -> Option<String> {
    gtex_param_string_from_value(args.params.as_ref(), keys)
}

pub(super) fn gtex_param_string_from_value(value: Option<&Json>, keys: &[&str]) -> Option<String> {
    let map = value?.as_object()?;
    keys.iter()
        .find_map(|key| map.get(*key).and_then(common::json_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn gtex_param_u32(args: &DataSearchArgs, keys: &[&str]) -> Option<u32> {
    gtex_param_u32_from_value(args.params.as_ref(), keys)
}

pub(super) fn gtex_param_u32_from_value(value: Option<&Json>, keys: &[&str]) -> Option<u32> {
    let map = value?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| value.as_str()?.trim().parse::<u32>().ok())
    })
}

pub(super) fn gtex_param_bool(args: &DataSearchArgs, keys: &[&str]) -> Option<bool> {
    let map = args.params.as_ref()?.as_object()?;
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value.as_bool().or_else(
            || match value.as_str()?.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            },
        )
    })
}

pub(super) fn gtex_param_list(args: &DataSearchArgs, keys: &[&str]) -> Vec<String> {
    let Some(map) = args.params.as_ref().and_then(Json::as_object) else {
        return Vec::new();
    };
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        if let Some(items) = value.as_array() {
            return items
                .iter()
                .filter_map(common::json_string)
                .flat_map(|item| split_csv_like(&item))
                .collect();
        }
        if let Some(value) = common::json_string(value) {
            return split_csv_like(&value);
        }
    }
    Vec::new()
}

fn split_csv_like(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn normalize_gtex_identifier(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("gtexportal.org") {
            return parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty())
                .map(str::to_string);
        }
    }
    Some(value.to_string())
}

pub(super) fn looks_like_gencode_id(value: &str) -> bool {
    let value = value.trim_matches(|c: char| c == ',' || c == ';');
    value.to_ascii_uppercase().starts_with("ENSG")
}

pub(super) fn looks_like_gtex_identifier(value: &str) -> bool {
    let Some(identifier) = normalize_gtex_identifier(value) else {
        return false;
    };
    looks_like_gencode_id(&identifier)
        || value
            .to_ascii_lowercase()
            .contains("gtexportal.org/home/gene/")
}
