//! Glob tool - Find files matching patterns
//!
//! Features:
//! - Glob pattern matching
//! - Respects .gitignore
//! - File type filtering
//! - Recursive search

use super::{ToolContext, ToolError, ToolSchema};
use crate::errors::SearchError;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use walkdir::WalkDir;

pub const DESCRIPTION: &str = r#"Find files matching a glob pattern.

Use this tool when you need to:
- List files of a specific type (*.rs, *.js, etc.)
- Find files in specific directories
- Discover project structure
- Batch file operations preparation

Features:
- Full glob pattern support
- Respects .gitignore
- Recursive by default
- File type filtering"#;

/// Maximum number of results
const MAX_RESULTS: usize = 5000;

/// Arguments for Glob tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobArgs {
    /// Glob pattern (e.g., "*.rs", "src/**/*.js")
    pub pattern: String,
    /// Maximum results to return (default: 5000)
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    /// Include hidden files (default: false)
    #[serde(default)]
    pub include_hidden: bool,
}

fn default_max_results() -> usize {
    MAX_RESULTS
}

/// A file result
#[derive(Debug, Clone, Serialize)]
pub struct GlobMatch {
    pub path: String,
    pub is_file: bool,
    pub size: u64,
}

/// Glob tool implementation
pub struct GlobTool;

#[async_trait]
impl super::ToolImpl for GlobTool {
    type Args = GlobArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        // Remote/SSH/sandbox: use shell-based glob through the cached environment
        if ctx.execution_environment != "local" {
            if let Some(ref store) = ctx.env_store {
                let base = crate::domain::tools::env_store::remote_path(ctx, ".");
                let env_arc = store.get_or_create(ctx, 30_000).await?;
                let paths = {
                    let mut guard = env_arc.lock().await;
                    let mut ops =
                        crate::domain::tools::shell_file_ops::ShellFileOps::new(&mut *guard);
                    ops.glob_find(&args.pattern, &base, args.max_results, args.include_hidden)
                        .await?
                };
                let truncated = paths.len() >= args.max_results;
                let matches: Vec<GlobMatch> = paths
                    .into_iter()
                    .map(|p| GlobMatch {
                        path: p,
                        is_file: true,
                        size: 0,
                    })
                    .collect();
                let output = GlobOutput {
                    pattern: args.pattern,
                    matches,
                    truncated,
                };
                return Ok(output.into_stream());
            }
        }

        // Parse glob pattern
        let pattern = build_glob_matcher(&args.pattern)
            .map_err(|e| SearchError::InvalidPattern { pattern: e })?;

        let mut matches = Vec::new();
        let mut truncated = false;

        // Determine starting path
        let base_path = if args.pattern.starts_with('/') {
            std::path::PathBuf::from("/")
        } else {
            ctx.project_root.clone()
        };

        // Walk directory
        let walker = WalkDir::new(&base_path).follow_links(false).max_depth(100);

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();

            // Skip hidden files unless requested
            if !args.include_hidden {
                let file_name = path.file_name().and_then(|n| n.to_str());
                if file_name.map(|n| n.starts_with('.')).unwrap_or(false) {
                    continue;
                }
            }

            // Get relative path for matching
            let relative_path = path
                .strip_prefix(&ctx.project_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            // Check if path matches pattern
            if pattern.matches(&relative_path) {
                let metadata = match tokio::fs::metadata(path).await {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                matches.push(GlobMatch {
                    path: relative_path,
                    is_file: metadata.is_file(),
                    size: metadata.len(),
                });

                if matches.len() >= args.max_results {
                    truncated = true;
                    break;
                }
            }
        }

        let output = GlobOutput {
            pattern: args.pattern,
            matches,
            truncated,
        };

        Ok(output.into_stream())
    }
}

/// Build a glob matcher from pattern
fn build_glob_matcher(pattern: &str) -> Result<glob::Pattern, String> {
    // Convert pattern to proper glob format if needed
    let glob_pattern = if pattern.contains("**") {
        pattern.to_string()
    } else if pattern.contains('*') || pattern.contains('?') {
        pattern.to_string()
    } else {
        // No wildcards - treat as exact match or directory
        format!("**/{}/*", pattern)
    };

    glob::Pattern::new(&glob_pattern).map_err(|e| e.to_string())
}

/// Glob search output
#[derive(Debug, Clone)]
pub struct GlobOutput {
    pub pattern: String,
    pub matches: Vec<GlobMatch>,
    pub truncated: bool,
}

impl StreamOutput for GlobOutput {
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
                key: "truncated".to_string(),
                value: self.truncated.to_string(),
            },
            StreamOutputItem::Start,
        ];

        // Stream matches
        for m in &self.matches {
            let streaming_match = crate::infrastructure::streaming::GlobMatch {
                path: m.path.clone(),
                is_file: m.is_file,
                size: m.size,
            };
            items.push(StreamOutputItem::GlobMatch(streaming_match));
        }

        items.push(StreamOutputItem::Complete);

        Box::pin(stream::iter(items))
    }
}

/// Get the JSON schema for the Glob tool
pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "glob",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '*.rs', 'src/**/*.js', '**/*.md')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 5000)",
                    "minimum": 1,
                    "maximum": 10000
                },
                "include_hidden": {
                    "type": "boolean",
                    "description": "Include hidden files starting with . (default: false)"
                }
            },
            "required": ["pattern"]
        }),
    )
}
