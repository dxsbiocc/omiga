//! Ripgrep tool (`ripgrep`) — search file contents with regex
//!
//! Aligned with the main repo’s `GrepTool` (ripgrep) in spirit:
//! - Respects `.gitignore` / `.ignore` via the `ignore` crate (same engine as ripgrep).
//! - Includes hidden files (like `rg --hidden`).
//! - Optional `path_pattern` / `glob` filters paths with **glob** semantics (not regex on paths).
//!
//! Not yet ported from TS: `path`, `output_mode` (content / files_with_matches / count), `-A/-B/-C`,
//! `head_limit` / `offset`, `--type`, `multiline`. Those can be added incrementally or by shelling to `rg`.

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::SearchError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Maximum number of results before truncation
const MAX_RESULTS: usize = 1000;

pub const DESCRIPTION: &str = r#"Search for patterns in files using regular expressions (ripgrep semantics).

Use for code references, TODOs, symbol lookup, etc.

Behavior (aligned with upstream ripgrep-based Grep):
- Honors project `.gitignore` and `.ignore` (via the `ignore` crate).
- Searches hidden files and directories (like `rg --hidden`).
- Optional `path_pattern` (alias `glob`) filters **by path glob** (e.g. `*.rs`, `**/*.{ts,tsx}`), not regex.

Not yet available vs the full TS tool: scoped `path`, `output_mode`, context lines (`-C`), `head_limit`/`offset`, `--type`, multiline — use the `bash` tool with `rg` if you need those."#;

/// Arguments for Grep tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepArgs {
    /// Regex pattern to search for
    pub pattern: String,
    /// Glob pattern(s) for file paths to include (comma- or space-separated). Alias: `glob`
    #[serde(default, alias = "glob")]
    pub path_pattern: Option<String>,
    /// Case insensitive search
    #[serde(default)]
    pub case_insensitive: bool,
    /// Maximum results to return (default: 1000, cap 5000)
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    MAX_RESULTS
}

/// A single match result
#[derive(Debug, Clone, Serialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
}

pub struct GrepTool;

#[async_trait]
impl super::ToolImpl for GrepTool {
    type Args = GrepArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let max_results = args.max_results.clamp(1, 5000);

        // Remote/SSH/sandbox: use shell-based grep through the cached environment
        if ctx.execution_environment != "local" {
            if let Some(ref store) = ctx.env_store {
                let search_root = crate::domain::tools::env_store::remote_path(ctx, ".");
                let env_arc = store.get_or_create(ctx, 30_000).await?;
                let raw = {
                    let mut guard = env_arc.lock().await;
                    let mut ops =
                        crate::domain::tools::shell_file_ops::ShellFileOps::new(&mut *guard);
                    ops.grep_raw(
                        &args.pattern,
                        &search_root,
                        args.path_pattern.as_deref(),
                        args.case_insensitive,
                        max_results,
                    )
                    .await?
                };
                // Parse "file:line:content" lines into GrepMatches
                let matches: Vec<GrepMatch> = raw
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.splitn(3, ':');
                        let file = parts.next()?.to_string();
                        let lineno: usize = parts.next()?.parse().ok()?;
                        let content = parts.next().unwrap_or("").to_string();
                        Some(GrepMatch {
                            file,
                            line: lineno,
                            column: 0,
                            content,
                        })
                    })
                    .collect();
                let truncated = matches.len() >= max_results;
                let output = GrepOutput {
                    pattern: args.pattern.clone(),
                    matches,
                    files_searched: 0,
                    truncated,
                };
                return Ok(output.into_stream());
            }
        }

        let project_root = ctx.project_root.clone();
        let pattern = args.pattern.clone();
        let path_pattern = args.path_pattern.clone();
        let case_insensitive = args.case_insensitive;

        let output = tokio::task::spawn_blocking(move || {
            run_grep_sync(
                &project_root,
                &pattern,
                path_pattern.as_deref(),
                case_insensitive,
                max_results,
            )
        })
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("grep task join error: {}", e),
        })??;

        Ok(output.into_stream())
    }
}

fn run_grep_sync(
    project_root: &std::path::Path,
    pattern: &str,
    path_pattern: Option<&str>,
    case_insensitive: bool,
    max_results: usize,
) -> Result<GrepOutput, ToolError> {
    let regex = build_regex(pattern, case_insensitive)
        .map_err(|e| SearchError::RegexError { message: e })?;

    let glob_filter = build_glob_set(path_pattern)?;

    let mut matches = Vec::new();
    let mut files_searched = 0u64;
    let mut truncated = false;

    let walker = WalkBuilder::new(project_root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Some(ref gs) = glob_filter {
            let rel = path.strip_prefix(project_root).unwrap_or(path);
            if !gs.is_match(rel) {
                continue;
            }
        }

        if should_skip_file(path) {
            continue;
        }

        files_searched += 1;

        match search_file(path, &regex) {
            Ok(file_matches) => {
                for m in file_matches {
                    if matches.len() >= max_results {
                        truncated = true;
                        break;
                    }
                    let relative_path = path
                        .strip_prefix(project_root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    matches.push(GrepMatch {
                        file: relative_path,
                        line: m.line,
                        column: m.column,
                        content: m.content,
                    });
                }
            }
            Err(_) => continue,
        }

        if truncated {
            break;
        }
    }

    Ok(GrepOutput {
        pattern: pattern.to_string(),
        matches,
        files_searched,
        truncated,
    })
}

fn build_glob_set(raw: Option<&str>) -> Result<Option<GlobSet>, ToolError> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let mut builder = GlobSetBuilder::new();
    let mut n = 0usize;
    for part in raw.split_whitespace() {
        for g in part.split(',') {
            let g = g.trim();
            if g.is_empty() {
                continue;
            }
            let glob = Glob::new(g).map_err(|e| SearchError::InvalidPattern {
                pattern: format!("{}: {}", g, e),
            })?;
            builder.add(glob);
            n += 1;
        }
    }
    if n == 0 {
        return Ok(None);
    }
    builder
        .build()
        .map(Some)
        .map_err(|e| SearchError::InvalidPattern {
            pattern: e.to_string(),
        })
        .map_err(Into::into)
}

/// Internal match result
#[derive(Debug)]
struct FileMatch {
    line: usize,
    column: usize,
    content: String,
}

/// Grep search output
#[derive(Debug, Clone)]
pub struct GrepOutput {
    pub pattern: String,
    pub matches: Vec<GrepMatch>,
    pub files_searched: u64,
    pub truncated: bool,
}

impl StreamOutput for GrepOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;

        let mut items = vec![
            StreamOutputItem::Metadata {
                key: "pattern".to_string(),
                value: self.pattern.clone(),
            },
            StreamOutputItem::Metadata {
                key: "matches_count".to_string(),
                value: self.matches.len().to_string(),
            },
            StreamOutputItem::Metadata {
                key: "files_searched".to_string(),
                value: self.files_searched.to_string(),
            },
            StreamOutputItem::Metadata {
                key: "truncated".to_string(),
                value: self.truncated.to_string(),
            },
            StreamOutputItem::Start,
        ];

        for m in &self.matches {
            let streaming_match = crate::infrastructure::streaming::GrepMatch {
                file: m.file.clone(),
                line: m.line,
                column: m.column,
                content: m.content.clone(),
            };
            items.push(StreamOutputItem::GrepMatch(streaming_match));
        }

        items.push(StreamOutputItem::Complete);

        Box::pin(stream::iter(items))
    }
}

fn build_regex(pattern: &str, case_insensitive: bool) -> Result<Regex, String> {
    let expr = if case_insensitive {
        format!("(?i){}", pattern)
    } else {
        pattern.to_string()
    };
    Regex::new(&expr).map_err(|e| format!("Invalid regex: {}", e))
}

/// Skip obvious binaries / huge files (ignore crate does not classify binary).
fn should_skip_file(path: &std::path::Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let e = ext.to_ascii_lowercase();
        if matches!(
            e.as_str(),
            "exe"
                | "dll"
                | "so"
                | "dylib"
                | "bin"
                | "o"
                | "a"
                | "lib"
                | "pyc"
                | "jpg"
                | "jpeg"
                | "png"
                | "gif"
                | "bmp"
                | "ico"
                | "webp"
                | "mp3"
                | "mp4"
                | "avi"
                | "mov"
                | "wav"
                | "zip"
                | "tar"
                | "gz"
                | "bz2"
                | "7z"
                | "rar"
                | "pdf"
        ) {
            return true;
        }
    }

    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > 10 * 1024 * 1024 {
            return true;
        }
        if meta.len() > 0 && meta.len() < 8192 {
            if let Ok(raw) = std::fs::read(path) {
                if raw.iter().take(4096).any(|&b| b == 0) {
                    return true;
                }
            }
        }
    }

    false
}

/// Search a single file for pattern matches (sync IO for spawn_blocking)
fn search_file(path: &std::path::Path, regex: &Regex) -> Result<Vec<FileMatch>, std::io::Error> {
    use std::io::BufRead;

    let f = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(f);
    let mut matches = Vec::new();
    for (line_index, line) in reader.lines().enumerate() {
        let line = line?;
        let line_num = line_index + 1;
        for mat in regex.find_iter(&line) {
            matches.push(FileMatch {
                line: line_num,
                column: mat.start() + 1,
                content: line.clone(),
            });
        }
    }

    Ok(matches)
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "ripgrep",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for in file contents"
                },
                "path_pattern": {
                    "type": "string",
                    "description": "Optional glob(s) to filter files (e.g. \"*.rs\", \"**/*.{ts,tsx}\"). Same as alias `glob`."
                },
                "glob": {
                    "type": "string",
                    "description": "Alias for path_pattern"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search (default: false)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 1000, max: 5000)",
                    "minimum": 1,
                    "maximum": 5000
                }
            },
            "required": ["pattern"]
        }),
    )
}
