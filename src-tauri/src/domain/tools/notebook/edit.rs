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
        // ── Validate args that apply to both local and remote paths ──────────
        let mode_in = args.edit_mode.as_deref().unwrap_or("replace");
        if !matches!(mode_in, "replace" | "insert" | "delete") {
            return Err(ToolError::InvalidArguments {
                message: "edit_mode must be replace, insert, or delete.".to_string(),
            });
        }

        // ── Remote/SSH/sandbox path ──────────────────────────────────────────
        if ctx.execution_environment != "local" {
            return if let Some(ref store) = ctx.env_store {
                let remote_path =
                    crate::domain::tools::env_store::remote_path(ctx, &args.notebook_path);

                // Validate extension on the remote path string
                if !remote_path.to_lowercase().ends_with(".ipynb") {
                    return Err(ToolError::InvalidArguments {
                        message: "File must be a Jupyter notebook (.ipynb). For other files use file_edit or file_write."
                            .to_string(),
                    });
                }

                let env_arc = store.get_or_create(ctx, 30_000).await?;

                // Read existing content, or scaffold a new notebook on first insert
                let raw_content = {
                    let mut guard = env_arc.lock().await;
                    let mut ops =
                        crate::domain::tools::shell_file_ops::ShellFileOps::new(&mut *guard);
                    match ops.read_file(&remote_path, 0, usize::MAX).await {
                        Ok(result) => result.content,
                        Err(_) if mode_in == "insert" => {
                            // File absent — scaffold on remote (write_file does mkdir -p internally)
                            let scaffold =
                                scaffold_empty_notebook(&ctx.local_venv_type, &ctx.local_venv_name);
                            ops.write_file(&remote_path, &scaffold).await?;
                            scaffold
                        }
                        Err(e) => return Err(e.into()),
                    }
                };

                // Apply the edit to the in-memory JSON
                let (updated_json, summary) = apply_edit_to_notebook_json(&raw_content, &args)?;

                // Write modified notebook back to remote
                {
                    let mut guard = env_arc.lock().await;
                    let mut ops =
                        crate::domain::tools::shell_file_ops::ShellFileOps::new(&mut *guard);
                    ops.write_file(&remote_path, &updated_json).await?;
                }

                Ok(NotebookEditOutput {
                    notebook_path: args.notebook_path,
                    summary,
                }
                .into_stream())
            } else {
                Err(ToolError::ExecutionFailed {
                    message: format!(
                        "远程执行环境 '{}' 下 env_store 未初始化，无法访问远程文件系统",
                        ctx.execution_environment
                    ),
                })
            };
        }

        // ── Local path ───────────────────────────────────────────────────────
        let path = resolve_path(&ctx.project_root, &args.notebook_path)?;

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("ipynb") {
            return Err(ToolError::InvalidArguments {
                message: "File must be a Jupyter notebook (.ipynb). For other files use file_edit or file_write."
                    .to_string(),
            });
        }

        // notebook が存在しない場合は空ノートブックを自動生成する（初回 insert 専用）。
        // 存在する notebook に対しては kernelspec を一切変更しない。
        if !path.exists() {
            if mode_in == "insert" {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(FsError::from)?;
                }
                let scaffold = scaffold_empty_notebook(&ctx.local_venv_type, &ctx.local_venv_name);
                tokio::fs::write(&path, scaffold.as_bytes())
                    .await
                    .map_err(FsError::from)?;
            } else {
                return Err(FsError::InvalidPath {
                    path: args.notebook_path.clone(),
                }
                .into());
            }
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

        let (updated, summary) = apply_edit_to_notebook_json(&content, &args)?;

        // Atomic write: temp → rename
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

        Ok(NotebookEditOutput {
            notebook_path: args.notebook_path,
            summary,
        }
        .into_stream())
    }
}

/// Apply a notebook edit operation to raw JSON content.
/// Returns `(updated_json, summary_string)`.
fn apply_edit_to_notebook_json(
    content: &str,
    args: &NotebookEditArgs,
) -> Result<(String, String), ToolError> {
    let mode_in = args.edit_mode.as_deref().unwrap_or("replace");

    let mut notebook: Value = serde_json::from_str(content).map_err(|_| FsError::IoError {
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

    let cell_index = compute_cell_index(cells_ref, args, mode_in)?;

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
            let new_cell = build_new_cell(cell_type_opt.as_deref().unwrap(), &args.new_source, id)?;
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

    let report_cell_id = new_cell_id_report.as_deref().or(args.cell_id.as_deref());
    let summary = summarize_result(
        &edit_mode,
        cell_type_opt.as_deref().unwrap_or("code"),
        &language,
        &args.new_source,
        report_cell_id,
    );

    Ok((updated, summary))
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

/// 生成空的 notebook JSON，在 notebook 首次创建（不存在时首次 insert）时使用。
/// kernelspec 根据当前选定的虚拟环境绑定一次，后续编辑不再修改。
///
/// - conda env → kernelspec.name = env_name
/// - venv      → kernelspec.name = "python3"（venv 通过 bash wrapper 激活）
/// - pyenv     → kernelspec.name = "python{ver}"
/// - 无环境    → kernelspec.name = "python3"（系统默认）
fn scaffold_empty_notebook(venv_type: &str, venv_name: &str) -> String {
    let name = venv_name.trim();
    let (kernel_name, display_name) =
        if !name.is_empty() && venv_type != "none" && !venv_type.is_empty() {
            match venv_type {
                "conda" => (name.to_string(), format!("Python (conda: {})", name)),
                "venv" => {
                    let label = std::path::Path::new(name)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| name.to_string());
                    ("python3".to_string(), format!("Python (venv: {})", label))
                }
                "pyenv" => (
                    format!("python{}", name),
                    format!("Python {} (pyenv)", name),
                ),
                _ => ("python3".to_string(), "Python 3".to_string()),
            }
        } else {
            ("python3".to_string(), "Python 3".to_string())
        };

    format!(
        r#"{{
 "cells": [],
 "metadata": {{
  "kernelspec": {{
   "display_name": "{display_name}",
   "language": "python",
   "name": "{kernel_name}"
  }},
  "language_info": {{
   "name": "python"
  }}
 }},
 "nbformat": 4,
 "nbformat_minor": 5
}}"#
    )
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

// ─── Shared kernelspec utility (used by file_write interception) ──────────────

/// 判断 notebook JSON 中的 kernelspec 是否是通用默认值（python3 / python / 缺失），
/// 即 AI 没有显式选择特定 kernel。
fn kernelspec_is_generic(nb: &Value) -> bool {
    let name = nb
        .get("metadata")
        .and_then(|m| m.get("kernelspec"))
        .and_then(|ks| ks.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("python3"); // 缺失时视为默认
    matches!(name, "python3" | "python" | "")
}

/// 构造 venv 对应的 kernelspec JSON 对象。
fn venv_kernelspec(venv_type: &str, venv_name: &str) -> Option<serde_json::Map<String, Value>> {
    let name = venv_name.trim();
    if name.is_empty() || venv_type == "none" || venv_type.is_empty() {
        return None;
    }
    let (kernel_name, display_name) = match venv_type {
        "conda" => (name.to_string(), format!("Python (conda: {})", name)),
        "venv" => {
            let label = std::path::Path::new(name)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.to_string());
            ("python3".to_string(), format!("Python (venv: {})", label))
        }
        "pyenv" => (
            format!("python{}", name),
            format!("Python {} (pyenv)", name),
        ),
        _ => return None,
    };
    let mut m = serde_json::Map::new();
    m.insert("display_name".to_string(), Value::String(display_name));
    m.insert("language".to_string(), Value::String("python".to_string()));
    m.insert("name".to_string(), Value::String(kernel_name));
    Some(m)
}

/// 若内容是合法的 `.ipynb` JSON，且 kernelspec 是通用默认值，
/// 并且当前会话选定了 venv，则将 kernelspec 替换为对应 venv 的值后返回修正后的字符串。
///
/// 其他情况（kernelspec 已经是非默认值、JSON 解析失败、未选 venv）原样返回 None。
pub(crate) fn fix_ipynb_kernelspec_if_default(
    content: &str,
    venv_type: &str,
    venv_name: &str,
) -> Option<String> {
    // 未选 venv，不修改
    let ks = venv_kernelspec(venv_type, venv_name)?;

    let mut nb: Value = serde_json::from_str(content).ok()?;

    // kernelspec 已经是非默认值（AI 或用户明确设置），尊重并保留
    if !kernelspec_is_generic(&nb) {
        return None;
    }

    // 替换 kernelspec
    if let Some(meta) = nb.get_mut("metadata").and_then(|m| m.as_object_mut()) {
        meta.insert("kernelspec".to_string(), Value::Object(ks));
    } else {
        // metadata 缺失，创建一个
        let mut meta = serde_json::Map::new();
        meta.insert("kernelspec".to_string(), Value::Object(ks));
        if let Some(obj) = nb.as_object_mut() {
            obj.insert("metadata".to_string(), Value::Object(meta));
        }
    }

    serde_json::to_string_pretty(&nb).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_notebook(kernel_name: &str) -> String {
        format!(
            r#"{{"cells":[],"metadata":{{"kernelspec":{{"display_name":"Python 3","language":"python","name":"{kernel_name}"}}}},"nbformat":4,"nbformat_minor":5}}"#
        )
    }

    fn notebook_without_kernelspec() -> &'static str {
        r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#
    }

    #[test]
    fn fix_replaces_default_python3_with_conda_env() {
        let nb = minimal_notebook("python3");
        let fixed = fix_ipynb_kernelspec_if_default(&nb, "conda", "myenv")
            .expect("should fix default kernel");
        let v: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        let name = v["metadata"]["kernelspec"]["name"].as_str().unwrap();
        assert_eq!(name, "myenv");
    }

    #[test]
    fn fix_replaces_missing_kernelspec_with_conda_env() {
        let nb = notebook_without_kernelspec();
        let fixed = fix_ipynb_kernelspec_if_default(nb, "conda", "base")
            .expect("should fix missing kernel");
        let v: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(
            v["metadata"]["kernelspec"]["name"].as_str().unwrap(),
            "base"
        );
    }

    #[test]
    fn fix_leaves_explicitly_set_kernel_untouched() {
        // AI already set the correct non-default kernel — must not be overwritten
        let nb = minimal_notebook("data-analysis-env");
        let result = fix_ipynb_kernelspec_if_default(&nb, "conda", "other-env");
        assert!(
            result.is_none(),
            "should not override an explicitly set kernel"
        );
    }

    #[test]
    fn fix_returns_none_when_no_venv_selected() {
        let nb = minimal_notebook("python3");
        assert!(fix_ipynb_kernelspec_if_default(&nb, "none", "").is_none());
        assert!(fix_ipynb_kernelspec_if_default(&nb, "", "").is_none());
    }

    #[test]
    fn fix_venv_uses_python3_kernel_name_with_label() {
        let nb = minimal_notebook("python3");
        let fixed = fix_ipynb_kernelspec_if_default(&nb, "venv", "/project/.venv")
            .expect("should fix venv");
        let v: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        // venv uses "python3" kernel name, display_name annotates the path
        assert_eq!(
            v["metadata"]["kernelspec"]["name"].as_str().unwrap(),
            "python3"
        );
        assert!(v["metadata"]["kernelspec"]["display_name"]
            .as_str()
            .unwrap()
            .contains(".venv"));
    }

    #[test]
    fn fix_pyenv_sets_versioned_kernel_name() {
        let nb = minimal_notebook("python3");
        let fixed =
            fix_ipynb_kernelspec_if_default(&nb, "pyenv", "3.11.5").expect("should fix pyenv");
        let v: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(
            v["metadata"]["kernelspec"]["name"].as_str().unwrap(),
            "python3.11.5"
        );
    }
}
