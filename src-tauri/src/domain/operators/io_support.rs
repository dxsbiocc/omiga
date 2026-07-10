use serde::Serialize;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::Path;

use super::OperatorToolError;

pub(crate) fn current_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub(crate) fn safe_relative_string(path: &Path) -> Result<String, OperatorToolError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(OperatorToolError::new(
                    "execution_infra_error",
                    true,
                    format!("unsafe plugin relative path {}", path.display()),
                ))
            }
        }
    }
    Ok(parts.join("/"))
}

pub(crate) fn write_json_file(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, format!("{raw}\n")).map_err(|err| err.to_string())
}

pub(crate) fn read_json_value(path: &Path) -> Result<JsonValue, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&raw).map_err(|err| err.to_string())
}

pub(crate) fn read_tail(path: impl AsRef<Path>) -> Option<String> {
    read_tail_limited(path, 4000)
}

pub(crate) fn read_tail_limited(path: impl AsRef<Path>, limit_chars: usize) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let chars = raw.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(limit_chars);
    Some(chars[start..].iter().collect())
}
