//! MCP `resources/read` post-processing aligned with `ReadMcpResourceTool` (TypeScript):
//! binary blobs → disk under the session tool-results dir; JSON shape `{ contents: [...] }`.

use crate::errors::ToolError;
use base64::Engine;
use rmcp::model::{ReadResourceResult, ResourceContents};
use serde_json::json;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Map MIME type to a conservative file extension (see `extensionForMimeType` in `mcpOutputStorage.ts`).
#[must_use]
pub fn extension_for_mime_type(mime_type: Option<&str>) -> &'static str {
    let mt = mime_type
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_lowercase())
        .unwrap_or_default();
    match mt.as_str() {
        "application/pdf" => "pdf",
        "application/json" => "json",
        "text/csv" => "csv",
        "text/plain" => "txt",
        "text/html" => "html",
        "text/markdown" => "md",
        "application/zip" => "zip",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "application/msword" => "doc",
        "application/vnd.ms-excel" => "xls",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "" => "bin",
        _ => "bin",
    }
}

#[must_use]
pub fn format_file_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / KB)
    } else {
        format!("{:.1} MB", bytes as f64 / (KB * KB))
    }
}

/// Align with `getBinaryBlobSavedMessage` in `mcpOutputStorage.ts`.
#[must_use]
pub fn binary_blob_saved_message(
    filepath: &Path,
    mime_type: Option<&str>,
    size: usize,
    source_description: &str,
) -> String {
    let mt = mime_type.unwrap_or("unknown type");
    format!(
        "{}Binary content ({}, {}) saved to {}",
        source_description,
        mt,
        format_file_size(size),
        filepath.display()
    )
}

/// Write raw bytes to `dir` with a mime-derived extension (`persistBinaryContent` in TS).
pub async fn persist_binary_content(
    bytes: &[u8],
    mime_type: Option<&str>,
    persist_id: &str,
    dir: &Path,
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| e.to_string())?;
    let ext = extension_for_mime_type(mime_type);
    let path = dir.join(format!("{persist_id}.{ext}"));
    tokio::fs::write(&path, bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Turn `ReadResourceResult` into TS `ReadMcpResourceTool` output: `{ contents: [...] }`.
pub async fn read_resource_result_to_ts_json(
    result: ReadResourceResult,
    server: &str,
    tool_results_dir: &Path,
) -> Result<serde_json::Value, ToolError> {
    let mut contents = Vec::new();

    for (i, c) in result.contents.into_iter().enumerate() {
        let entry = match c {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                ..
            } => {
                let mut o = serde_json::Map::new();
                o.insert("uri".to_string(), json!(uri));
                if let Some(mt) = mime_type {
                    o.insert("mimeType".to_string(), json!(mt));
                }
                o.insert("text".to_string(), json!(text));
                serde_json::Value::Object(o)
            }
            ResourceContents::BlobResourceContents {
                uri,
                mime_type,
                blob,
                ..
            } => {
                let persist_id = format!(
                    "mcp-resource-{}-{}-{:x}",
                    chrono::Utc::now().timestamp_millis(),
                    i,
                    Uuid::new_v4().as_u128()
                );
                let mime_str = mime_type.as_deref();
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(blob.trim())
                    .map_err(|e| ToolError::ExecutionFailed {
                        message: format!("Invalid base64 in MCP blob for {uri}: {e}"),
                    })?;

                match persist_binary_content(&decoded, mime_str, &persist_id, tool_results_dir).await
                {
                    Ok(filepath) => {
                        let source = format!("[Resource from {server} at {uri}] ");
                        let text = binary_blob_saved_message(
                            &filepath,
                            mime_str,
                            decoded.len(),
                            &source,
                        );
                        let mut o = serde_json::Map::new();
                        o.insert("uri".to_string(), json!(uri));
                        if let Some(mt) = mime_type {
                            o.insert("mimeType".to_string(), json!(mt));
                        }
                        o.insert("blobSavedTo".to_string(), json!(filepath.display().to_string()));
                        o.insert("text".to_string(), json!(text));
                        serde_json::Value::Object(o)
                    }
                    Err(e) => {
                        let mut o = serde_json::Map::new();
                        o.insert("uri".to_string(), json!(uri));
                        if let Some(mt) = mime_type {
                            o.insert("mimeType".to_string(), json!(mt));
                        }
                        o.insert(
                            "text".to_string(),
                            json!(format!("Binary content could not be saved to disk: {e}")),
                        );
                        serde_json::Value::Object(o)
                    }
                }
            }
        };
        contents.push(entry);
    }

    Ok(json!({ "contents": contents }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_for_mime_type_matches_ts_table() {
        assert_eq!(extension_for_mime_type(Some("application/pdf")), "pdf");
        assert_eq!(extension_for_mime_type(Some("text/plain; charset=utf-8")), "txt");
        assert_eq!(extension_for_mime_type(Some("IMAGE/PNG")), "png");
        assert_eq!(extension_for_mime_type(None), "bin");
        assert_eq!(extension_for_mime_type(Some("application/octet-stream")), "bin");
    }
}
