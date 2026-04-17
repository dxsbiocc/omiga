//! Jupyter notebook (.ipynb) cell edits — aligns with `NotebookEditTool` (TypeScript).
//!
//! LLM tool name: `notebook_edit` (snake_case). Supports `replace`, `insert`, and `delete`
//! on `cells[]`, targeting by Jupyter `cell.id` or `cell-N` index (0-based).

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::FsError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::sync::OnceLock;

/// Same cap as `file_read` / `file_edit`
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

pub const DESCRIPTION: &str = r#"Edit a Jupyter notebook (.ipynb) cell in place.

- `notebook_path`: path to the `.ipynb` file (project-relative or absolute).
- `cell_id`: optional. Match a cell's `id` field, or use `cell-0`, `cell-1`, … for 0-based index.
- `edit_mode`: `replace` (default), `insert`, or `delete`. Insert inserts **after** the referenced cell; without `cell_id`, inserts at the top.
- `cell_type`: `code` or `markdown` — required for `insert` (unless appending via replace→insert at end, which defaults to `code`).
- `new_source`: new cell source for replace/insert; ignored for delete.

Prefer this over `file_edit` for `.ipynb` files (they are JSON, not plain text)."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookEditArgs {
    pub notebook_path: String,
    #[serde(default)]
    pub cell_id: Option<String>,
    pub new_source: String,
    #[serde(default)]
    pub cell_type: Option<String>,
    #[serde(default)]
    pub edit_mode: Option<String>,
}

pub struct NotebookEditTool;

#[async_trait]
impl super::ToolImpl for NotebookEditTool {
    type Args = NotebookEditArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let path = resolve_path(&ctx.project_root, &args.notebook_path)?;

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("ipynb") {
            return Err(ToolError::InvalidArguments {
                message: "File must be a Jupyter notebook (.ipynb). For other files use file_edit or file_write."
                    .to_string(),
            });
        }

        let mode_in = args.edit_mode.as_deref().unwrap_or("replace");
        if !matches!(mode_in, "replace" | "insert" | "delete") {
            return Err(ToolError::InvalidArguments {
                message: "edit_mode must be replace, insert, or delete.".to_string(),
            });
        }

        let meta = tokio::fs::metadata(&path).await.map_err(FsError::from)?;
        if !meta.is_file() {
            return Err(FsError::InvalidPath {
                path: args.notebook_path.clone(),
            }
            .into());
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(FsError::FileTooLarge {
                path: args.notebook_path.clone(),
                size: meta.len(),
                max: MAX_FILE_BYTES,
            }
            .into());
        }

        let raw = tokio::fs::read(&path).await.map_err(FsError::from)?;
        let content = String::from_utf8(raw).map_err(|_| FsError::BinaryFile {
            path: args.notebook_path.clone(),
        })?;

        let mut notebook: Value = serde_json::from_str(&content).map_err(|_| FsError::IoError {
            message: "Notebook is not valid JSON.".to_string(),
        })?;

        let cells_ref = notebook
            .get("cells")
            .and_then(|c| c.as_array())
            .ok_or_else(|| FsError::IoError {
                message: "Notebook JSON has no cells array.".to_string(),
            })?;

        let mut edit_mode = mode_in.to_string();
        let mut cell_type_opt = args.cell_type.clone();

        let cell_index = compute_cell_index(cells_ref, &args, mode_in)?;

        if edit_mode == "replace" && cell_index == cells_ref.len() {
            edit_mode = "insert".to_string();
            if cell_type_opt.is_none() {
                cell_type_opt = Some("code".to_string());
            }
        }

        if edit_mode == "insert" && cell_type_opt.is_none() {
            return Err(ToolError::InvalidArguments {
                message: "cell_type is required when using edit_mode=insert.".to_string(),
            });
        }

        if edit_mode == "insert" {
            validate_cell_type(cell_type_opt.as_deref().unwrap())?;
        } else if edit_mode == "replace" {
            if let Some(ref ct) = cell_type_opt {
                validate_cell_type(ct)?;
            }
        }

        let language = notebook
            .get("metadata")
            .and_then(|m| m.get("language_info"))
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("python")
            .to_string();

        let generate_cell_ids = should_generate_cell_ids(&notebook);

        let cells = notebook
            .get_mut("cells")
            .and_then(|c| c.as_array_mut())
            .ok_or_else(|| FsError::IoError {
                message: "Notebook JSON has no cells array.".to_string(),
            })?;

        let mut new_cell_id_report: Option<String> = None;

        match edit_mode.as_str() {
            "delete" => {
                if cell_index >= cells.len() {
                    return Err(ToolError::InvalidArguments {
                        message: format!(
                            "Cell index {} is out of range (notebook has {} cells).",
                            cell_index,
                            cells.len()
                        ),
                    });
                }
                cells.remove(cell_index);
            }
            "insert" => {
                let id = if generate_cell_ids {
                    let id = random_cell_id();
                    new_cell_id_report = Some(id.clone());
                    Some(id)
                } else {
                    None
                };
                let new_cell =
                    build_new_cell(cell_type_opt.as_deref().unwrap(), &args.new_source, id)?;
                if cell_index > cells.len() {
                    return Err(ToolError::InvalidArguments {
                        message: format!(
                            "Insert position {} is past end ({} cells).",
                            cell_index,
                            cells.len()
                        ),
                    });
                }
                cells.insert(cell_index, new_cell);
            }
            "replace" => {
                if cell_index >= cells.len() {
                    return Err(ToolError::InvalidArguments {
                        message: format!(
                            "Cell index {} is out of range (notebook has {} cells).",
                            cell_index,
                            cells.len()
                        ),
                    });
                }
                let target = &mut cells[cell_index];
                if let Some(ct) = &cell_type_opt {
                    if let Some(cur) = target.get("cell_type").and_then(|v| v.as_str()) {
                        if ct != cur {
                            if ct == "markdown" {
                                if let Some(obj) = target.as_object_mut() {
                                    obj.insert(
                                        "cell_type".to_string(),
                                        Value::String("markdown".to_string()),
                                    );
                                    obj.remove("execution_count");
                                    obj.remove("outputs");
                                }
                            } else {
                                target["cell_type"] = Value::String("code".to_string());
                                target["execution_count"] = Value::Null;
                                target["outputs"] = Value::Array(vec![]);
                            }
                        }
                    }
                }
                target["source"] = Value::String(args.new_source.clone());
                if target.get("cell_type").and_then(|v| v.as_str()) == Some("code") {
                    target["execution_count"] = Value::Null;
                    target["outputs"] = Value::Array(vec![]);
                }
                if let Some(cid) = args.cell_id.as_ref() {
                    new_cell_id_report = Some(cid.clone());
                }
            }
            _ => unreachable!(),
        }

        let updated = serde_json::to_string_pretty(&notebook).map_err(|e| FsError::IoError {
            message: format!("Failed to serialize notebook: {}", e),
        })?;

        let temp_path = path.with_extension("tmp");
        tokio::fs::write(&temp_path, updated.as_bytes())
            .await
            .map_err(|e| FsError::IoError {
                message: format!("Failed to write temp file: {}", e),
            })?;
        tokio::fs::rename(&temp_path, &path)
            .await
            .map_err(|e| FsError::IoError {
                message: format!("Failed to rename temp file: {}", e),
            })?;

        let report_cell_id = new_cell_id_report
            .as_deref()
            .or_else(|| args.cell_id.as_deref());
        let summary = summarize_result(
            &edit_mode,
            cell_type_opt.as_deref().unwrap_or("code"),
            &language,
            &args.new_source,
            report_cell_id,
        );

        Ok(NotebookEditOutput {
            notebook_path: args.notebook_path,
            summary,
        }
        .into_stream())
    }
}

fn summarize_result(
    edit_mode: &str,
    cell_type: &str,
    language: &str,
    new_source: &str,
    cell_id: Option<&str>,
) -> String {
    let preview = truncate_preview(new_source, 400);
    let cid = cell_id.unwrap_or("(n/a)");
    match edit_mode {
        "delete" => format!("Deleted cell {} (language={}).", cid, language),
        "insert" => format!(
            "Inserted {} cell {} (language={}): {}",
            cell_type, cid, language, preview
        ),
        _ => format!(
            "Updated notebook cell {} ({}, language={}): {}",
            cid, cell_type, language, preview
        ),
    }
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    let n = t.chars().count();
    if n <= max_chars {
        t.to_string()
    } else {
        let prefix: String = t.chars().take(max_chars).collect();
        format!("{}… ({} chars total)", prefix, n)
    }
}

fn validate_cell_type(s: &str) -> Result<(), ToolError> {
    match s {
        "code" | "markdown" => Ok(()),
        _ => Err(ToolError::InvalidArguments {
            message: "cell_type must be \"code\" or \"markdown\".".to_string(),
        }),
    }
}

fn should_generate_cell_ids(nb: &Value) -> bool {
    let major = nb.get("nbformat").and_then(|v| v.as_u64()).unwrap_or(0);
    let minor = nb
        .get("nbformat_minor")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    major > 4 || (major == 4 && minor >= 5)
}

fn random_cell_id() -> String {
    uuid::Uuid::new_v4()
        .as_simple()
        .to_string()
        .chars()
        .take(13)
        .collect()
}

fn build_new_cell(cell_type: &str, source: &str, id: Option<String>) -> Result<Value, ToolError> {
    let mut m = serde_json::Map::new();
    m.insert(
        "cell_type".to_string(),
        Value::String(cell_type.to_string()),
    );
    m.insert("source".to_string(), Value::String(source.to_string()));
    m.insert(
        "metadata".to_string(),
        Value::Object(serde_json::Map::new()),
    );
    if let Some(id) = id {
        m.insert("id".to_string(), Value::String(id));
    }
    match cell_type {
        "code" => {
            m.insert("execution_count".to_string(), Value::Null);
            m.insert("outputs".to_string(), Value::Array(vec![]));
        }
        "markdown" => {}
        _ => {
            return Err(ToolError::InvalidArguments {
                message: "cell_type must be code or markdown.".to_string(),
            });
        }
    }
    Ok(Value::Object(m))
}

fn parse_cell_id(id: &str) -> Option<usize> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^cell-(\d+)$").expect("cell id regex"));
    re.captures(id)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn find_cell_index(cells: &[Value], cell_id: &str) -> Option<usize> {
    let by_id = cells.iter().position(|c| {
        c.get("id")
            .and_then(|v| v.as_str())
            .map(|id| id == cell_id)
            .unwrap_or(false)
    });
    if let Some(i) = by_id {
        return Some(i);
    }
    parse_cell_id(cell_id)
}

/// Returns target index for replace/delete, or insert position (after +1 logic for insert).
fn compute_cell_index(
    cells: &[Value],
    args: &NotebookEditArgs,
    mode_in: &str,
) -> Result<usize, ToolError> {
    match mode_in {
        "insert" => {
            if args.cell_id.is_none() {
                return Ok(0);
            }
            let cid = args.cell_id.as_ref().unwrap();
            let base = find_cell_index(cells, cid).ok_or_else(|| ToolError::InvalidArguments {
                message: format!("Cell with ID \"{}\" not found in notebook.", cid),
            })?;
            Ok(base + 1)
        }
        "replace" | "delete" => {
            let cid = args
                .cell_id
                .as_ref()
                .ok_or_else(|| ToolError::InvalidArguments {
                    message: "cell_id is required for replace and delete.".to_string(),
                })?;
            find_cell_index(cells, cid).ok_or_else(|| ToolError::InvalidArguments {
                message: format!("Cell with ID \"{}\" not found in notebook.", cid),
            })
        }
        _ => Err(ToolError::InvalidArguments {
            message: "invalid edit_mode".to_string(),
        }),
    }
}

#[derive(Debug, Clone)]
struct NotebookEditOutput {
    notebook_path: String,
    summary: String,
}

impl StreamOutput for NotebookEditOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        let items = vec![
            StreamOutputItem::Metadata {
                key: "notebook_path".to_string(),
                value: self.notebook_path,
            },
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.summary),
            StreamOutputItem::Complete,
        ];
        Box::pin(stream::iter(items))
    }
}

fn resolve_path(project_root: &std::path::Path, path: &str) -> Result<std::path::PathBuf, FsError> {
    let path_buf = if path.starts_with('/') || path.starts_with("~/") {
        if path.starts_with("~/") {
            let home = std::env::var("HOME").map_err(|_| FsError::InvalidPath {
                path: path.to_string(),
            })?;
            std::path::PathBuf::from(path.replacen("~", &home, 1))
        } else {
            std::path::PathBuf::from(path)
        }
    } else {
        project_root.join(path)
    };

    let canonical_project = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_path = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());

    if !canonical_path.starts_with(&canonical_project)
        && !path.starts_with('/')
        && !path.starts_with("~/")
    {
        return Err(FsError::PathTraversal {
            path: path.to_string(),
        });
    }

    Ok(path_buf)
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "notebook_edit",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "Path to the .ipynb file (project-relative or absolute)"
                },
                "cell_id": {
                    "type": "string",
                    "description": "Cell id from the notebook, or cell-0, cell-1, … for 0-based index. For insert without id, inserts at top."
                },
                "new_source": {
                    "type": "string",
                    "description": "New cell source (replace/insert); omit or empty for delete"
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown"],
                    "description": "Required for insert; optional on replace to change type"
                },
                "edit_mode": {
                    "type": "string",
                    "enum": ["replace", "insert", "delete"],
                    "description": "replace (default), insert (after cell_id), or delete"
                }
            },
            "required": ["notebook_path", "new_source"]
        }),
    )
}
