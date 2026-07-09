use super::parser::{FileHunk, Patch, UpdateChunk};
use crate::errors::{FsError, ToolError};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Add,
    Modify,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSummary {
    pub path: String,
    pub kind: ChangeKind,
    pub old_lines: usize,
    pub new_lines: usize,
}

#[derive(Debug, Clone)]
enum PlannedState {
    Exists(String),
    Deleted,
}

pub async fn apply_patch_atomic(
    project_root: &Path,
    cwd: &Path,
    patch: Patch,
) -> Result<Vec<ChangeSummary>, ToolError> {
    let mut planned: BTreeMap<PathBuf, PlannedState> = BTreeMap::new();
    let mut summaries: BTreeMap<PathBuf, ChangeSummary> = BTreeMap::new();

    for (hunk_index, hunk) in patch.hunks.iter().enumerate() {
        match hunk {
            FileHunk::Add { path, contents } => {
                let resolved = resolve_patch_path(project_root, cwd, path)?;
                let current = load_state(path, &resolved, &planned).await?;
                if matches!(current, Some(_)) {
                    return Err(patch_error(
                        path,
                        hunk_index,
                        "Add File target already exists",
                    ));
                }
                planned.insert(resolved.clone(), PlannedState::Exists(contents.clone()));
                summaries.insert(
                    resolved,
                    ChangeSummary {
                        path: path.clone(),
                        kind: ChangeKind::Add,
                        old_lines: 0,
                        new_lines: count_lines(contents),
                    },
                );
            }
            FileHunk::Update { path, chunks } => {
                let resolved = resolve_patch_path(project_root, cwd, path)?;
                let Some(current) = load_state(path, &resolved, &planned).await? else {
                    return Err(patch_error(
                        path,
                        hunk_index,
                        "Update File target not found",
                    ));
                };
                let new_content = apply_update_chunks(path, hunk_index, &current, chunks)?;
                planned.insert(resolved.clone(), PlannedState::Exists(new_content.clone()));
                summaries.insert(
                    resolved,
                    ChangeSummary {
                        path: path.clone(),
                        kind: ChangeKind::Modify,
                        old_lines: count_lines(&current),
                        new_lines: count_lines(&new_content),
                    },
                );
            }
            FileHunk::Delete { path } => {
                let resolved = resolve_patch_path(project_root, cwd, path)?;
                let Some(current) = load_state(path, &resolved, &planned).await? else {
                    return Err(patch_error(
                        path,
                        hunk_index,
                        "Delete File target not found",
                    ));
                };
                planned.insert(resolved.clone(), PlannedState::Deleted);
                summaries.insert(
                    resolved,
                    ChangeSummary {
                        path: path.clone(),
                        kind: ChangeKind::Delete,
                        old_lines: count_lines(&current),
                        new_lines: 0,
                    },
                );
            }
        }
    }

    for (path, state) in &planned {
        match state {
            PlannedState::Exists(content) => {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| FsError::IoError {
                            message: format!("Failed to create parent directories: {e}"),
                        })?;
                }
                let temp_path = temp_path_for(path);
                tokio::fs::write(&temp_path, content)
                    .await
                    .map_err(|e| FsError::IoError {
                        message: format!("Failed to write temp file: {e}"),
                    })?;
                tokio::fs::rename(&temp_path, path)
                    .await
                    .map_err(|e| FsError::IoError {
                        message: format!("Failed to rename temp file: {e}"),
                    })?;
            }
            PlannedState::Deleted => {
                tokio::fs::remove_file(path)
                    .await
                    .map_err(|e| FsError::IoError {
                        message: format!("Failed to delete file '{}': {e}", path.display()),
                    })?;
            }
        }
    }

    Ok(summaries.into_values().collect())
}

async fn load_state(
    display_path: &str,
    resolved: &Path,
    planned: &BTreeMap<PathBuf, PlannedState>,
) -> Result<Option<String>, ToolError> {
    if let Some(state) = planned.get(resolved) {
        return Ok(match state {
            PlannedState::Exists(content) => Some(content.clone()),
            PlannedState::Deleted => None,
        });
    }

    let meta = match tokio::fs::metadata(resolved).await {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FsError::from(e).into()),
    };
    if !meta.is_file() {
        return Err(FsError::InvalidPath {
            path: display_path.to_string(),
        }
        .into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(FsError::FileTooLarge {
            path: display_path.to_string(),
            size: meta.len(),
            max: MAX_FILE_BYTES,
        }
        .into());
    }

    let raw = tokio::fs::read(resolved).await.map_err(FsError::from)?;
    if raw.iter().take(8192).any(|&b| b == 0) {
        return Err(FsError::BinaryFile {
            path: display_path.to_string(),
        }
        .into());
    }
    let content = String::from_utf8(raw).map_err(|_| FsError::BinaryFile {
        path: display_path.to_string(),
    })?;
    Ok(Some(content.replace("\r\n", "\n")))
}

fn apply_update_chunks(
    path: &str,
    hunk_index: usize,
    original: &str,
    chunks: &[UpdateChunk],
) -> Result<String, ToolError> {
    let mut lines = split_lines(original);
    let mut search_from = 0usize;

    for (chunk_index, chunk) in chunks.iter().enumerate() {
        if let Some(context) = &chunk.change_context {
            let context_pattern = vec![context.clone()];
            let found = find_unique(&lines, &context_pattern, search_from, false).map_err(|e| {
                patch_error(path, hunk_index, format!("chunk {}: {e}", chunk_index + 1))
            })?;
            let Some(index) = found else {
                return Err(patch_error(
                    path,
                    hunk_index,
                    format!("chunk {}: context not found: {context}", chunk_index + 1),
                ));
            };
            search_from = index + 1;
        }

        let replace_at = if chunk.old_lines.is_empty() {
            search_from.min(lines.len())
        } else {
            match find_unique(&lines, &chunk.old_lines, search_from, chunk.is_end_of_file).map_err(
                |e| patch_error(path, hunk_index, format!("chunk {}: {e}", chunk_index + 1)),
            )? {
                Some(index) => index,
                None => {
                    return Err(patch_error(
                        path,
                        hunk_index,
                        format!(
                            "chunk {}: expected context not found:\n{}",
                            chunk_index + 1,
                            chunk.old_lines.join("\n")
                        ),
                    ))
                }
            }
        };

        let replacement = replacement_lines_preserving_context(&lines, replace_at, chunk);
        lines.splice(replace_at..replace_at + chunk.old_lines.len(), replacement);
        search_from = replace_at + chunk.new_lines.len();
    }

    Ok(join_lines(&lines))
}

fn find_unique(
    lines: &[String],
    pattern: &[String],
    start: usize,
    eof: bool,
) -> Result<Option<usize>, String> {
    if pattern.is_empty() {
        return Ok(Some(start.min(lines.len())));
    }
    if pattern.len() > lines.len() || start > lines.len().saturating_sub(pattern.len()) {
        return Ok(None);
    }

    for mode in [MatchMode::Exact, MatchMode::TrimEnd, MatchMode::Trim] {
        let matches = find_matches(lines, pattern, start, eof, mode);
        match matches.len() {
            0 => continue,
            1 => return Ok(matches.into_iter().next()),
            count => {
                return Err(format!(
                    "context is not unique ({count} matches under {} matching)",
                    mode.name()
                ))
            }
        }
    }
    Ok(None)
}

fn replacement_lines_preserving_context(
    lines: &[String],
    replace_at: usize,
    chunk: &UpdateChunk,
) -> Vec<String> {
    let actual_old = &lines[replace_at..replace_at + chunk.old_lines.len()];
    let mut old_index = 0usize;
    let mut replacement = Vec::with_capacity(chunk.new_lines.len());

    for new_line in &chunk.new_lines {
        if old_index < chunk.old_lines.len()
            && MatchMode::Trim.eq(&chunk.old_lines[old_index], new_line)
            && MatchMode::Trim.eq(&actual_old[old_index], new_line)
        {
            replacement.push(actual_old[old_index].clone());
            old_index += 1;
        } else {
            replacement.push(new_line.clone());
        }
    }

    replacement
}

fn find_matches(
    lines: &[String],
    pattern: &[String],
    start: usize,
    eof: bool,
    mode: MatchMode,
) -> Vec<usize> {
    let max_start = lines.len().saturating_sub(pattern.len());
    let starts: Box<dyn Iterator<Item = usize>> = if eof {
        Box::new(std::iter::once(max_start))
    } else {
        Box::new(start..=max_start)
    };

    starts
        .filter(|&index| {
            pattern
                .iter()
                .enumerate()
                .all(|(offset, expected)| mode.eq(&lines[index + offset], expected))
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum MatchMode {
    Exact,
    TrimEnd,
    Trim,
}

impl MatchMode {
    fn eq(self, actual: &str, expected: &str) -> bool {
        match self {
            MatchMode::Exact => actual == expected,
            MatchMode::TrimEnd => actual.trim_end() == expected.trim_end(),
            MatchMode::Trim => actual.trim() == expected.trim(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            MatchMode::Exact => "exact",
            MatchMode::TrimEnd => "trailing-whitespace-insensitive",
            MatchMode::Trim => "leading-and-trailing-whitespace-insensitive",
        }
    }
}

fn split_lines(content: &str) -> Vec<String> {
    let mut lines: Vec<String> = content.split('\n').map(ToOwned::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn join_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        let mut content = lines.join("\n");
        content.push('\n');
        content
    }
}

fn count_lines(content: &str) -> usize {
    split_lines(content).len()
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string();
    if extension.is_empty() {
        extension.push_str("apply_patch_tmp");
    } else {
        extension.push_str(".apply_patch_tmp");
    }
    path.with_extension(extension)
}

fn patch_error(path: &str, hunk_index: usize, message: impl Into<String>) -> ToolError {
    ToolError::ExecutionFailed {
        message: format!(
            "apply_patch failed for '{}' hunk {}: {}",
            path,
            hunk_index + 1,
            message.into()
        ),
    }
}

fn resolve_patch_path(project_root: &Path, cwd: &Path, path: &str) -> Result<PathBuf, FsError> {
    if path.trim().is_empty() || path.contains('\0') {
        return Err(FsError::InvalidPath {
            path: path.to_string(),
        });
    }

    let input = Path::new(path);
    if input
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(FsError::PathTraversal {
            path: path.to_string(),
        });
    }

    let base = if input.is_absolute() {
        PathBuf::new()
    } else {
        cwd.canonicalize().unwrap_or_else(|_| normalize_path(cwd))
    };
    let candidate = normalize_path(&base.join(input));
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| normalize_path(project_root));

    let checked = canonicalize_existing_prefix(&candidate);
    if !checked.starts_with(&root) {
        return Err(FsError::PathTraversal {
            path: path.to_string(),
        });
    }

    Ok(checked)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::ParentDir => {}
        }
    }
    normalized
}

fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    let mut missing = Vec::new();
    let mut cursor = path;
    while let Some(parent) = cursor.parent() {
        if let Some(name) = cursor.file_name() {
            missing.push(name.to_owned());
        }
        if let Ok(mut canonical_parent) = parent.canonicalize() {
            for component in missing.iter().rev() {
                canonical_parent.push(component);
            }
            return normalize_path(&canonical_parent);
        }
        cursor = parent;
    }

    normalize_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn unique_match_rejects_ambiguous_trimmed_context() {
        let haystack = lines(&["  target", "\ttarget"]);
        let pattern = lines(&["target"]);
        let err = find_unique(&haystack, &pattern, 0, false).unwrap_err();
        assert!(err.contains("not unique"));
    }
}
