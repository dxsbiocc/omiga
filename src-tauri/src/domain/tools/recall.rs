//! `recall` — search the local knowledge base (wiki + implicit memory via PageIndex).
//!
//! This tool is the primary interface for retrieving project-specific knowledge
//! accumulated across sessions.  It searches two stores:
//!
//! 1. **Implicit memory** (pageindex) — auto-indexed chat sessions.
//! 2. **Wiki pages** (explicit memory) — user-curated markdown files.
//!
//! The permanent (cross-project) wiki is also checked when `scope` includes it.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const DESCRIPTION: &str = r#"Search the local knowledge base (wiki + session memory) for relevant information.

Use this tool BEFORE web_search whenever you need to find information about:
- Past conversations, decisions, or results
- Project-specific knowledge or notes
- Prior analyses, findings, or summaries
- Any information the user may have shared in previous sessions

Arguments:
- `query`: natural-language query or keyword phrase
- `limit`: max results to return (default 5, max 20)
- `scope`: which stores to search — "implicit" (session history), "wiki" (explicit pages),
           "permanent" (cross-project wiki), or "all" (default)

Returns a formatted excerpt of matching content with source paths."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallArgs {
    /// Natural-language query or keyword phrase.
    pub query: String,
    /// Maximum results (default 5, max 20).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Search scope: "implicit" | "wiki" | "permanent" | "all" (default).
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_limit() -> usize {
    5
}
fn default_scope() -> String {
    "all".to_string()
}

#[derive(Debug, Serialize)]
struct RecallOutput {
    query: String,
    results_found: usize,
    content: String,
}

impl StreamOutput for RecallOutput {
    fn into_stream(self) -> crate::infrastructure::streaming::StreamOutputBox {
        let text = if self.results_found == 0 {
            format!(
                "No knowledge base results found for query: \"{}\"\n\n\
                 The knowledge base is empty or does not contain relevant content.\n\
                 Proceed with web_search or other sources.",
                self.query
            )
        } else {
            self.content
        };
        Box::pin(futures::stream::once(async move {
            StreamOutputItem::Text(text)
        }))
    }
}

pub struct RecallTool;

#[async_trait]
impl super::ToolImpl for RecallTool {
    type Args = RecallArgs;
    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let limit = args.limit.clamp(1, 20);
        let query = args.query.trim().to_string();
        let scope = args.scope.to_lowercase();
        let project_root = &ctx.project_root;

        if query.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "query must not be empty".to_string(),
            });
        }

        let search_implicit = scope == "all" || scope == "implicit";
        let search_wiki = scope == "all" || scope == "wiki";
        let search_permanent = scope == "all" || scope == "permanent";

        let mut sections: Vec<String> = Vec::new();
        let mut total_results = 0usize;

        // ── 1. Implicit memory (PageIndex over session history) ──────────────────
        if search_implicit {
            if let Some(implicit_text) =
                query_implicit_memory(project_root, &query, limit).await
            {
                total_results += 1;
                sections.push(implicit_text);
            }
        }

        // ── 2. Wiki pages (explicit memory) ─────────────────────────────────────
        if search_wiki {
            if let Some(wiki_text) =
                search_wiki_pages(project_root, &query, limit, false).await
            {
                total_results += 1;
                sections.push(wiki_text);
            }
        }

        // ── 3. Permanent (cross-project) wiki ────────────────────────────────────
        if search_permanent {
            if let Some(perm_text) =
                search_permanent_wiki(&query, limit).await
            {
                total_results += 1;
                sections.push(perm_text);
            }
        }

        let content = if sections.is_empty() {
            String::new()
        } else {
            sections.join("\n\n")
        };

        let out = RecallOutput {
            query,
            results_found: total_results,
            content,
        };
        Ok(out.into_stream())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Query the pageindex (implicit / session memory).
async fn query_implicit_memory(
    project_root: &std::path::Path,
    query: &str,
    limit: usize,
) -> Option<String> {
    use crate::domain::memory::load_resolved_config;
    use crate::domain::pageindex::{IndexConfig, PageIndex, QueryEngine};

    let mem_cfg = load_resolved_config(project_root).await.ok()?;
    let implicit = mem_cfg.implicit_path(project_root);
    let mut pageindex = PageIndex::with_memory_dir(project_root, &implicit, IndexConfig::default());

    match pageindex.load_tree().await {
        Ok(Some(tree)) => *pageindex.tree_mut() = tree,
        Ok(None) => return None,
        Err(e) => {
            tracing::debug!(target: "omiga::recall", "implicit memory load failed: {e}");
            return None;
        }
    }

    let results = pageindex.query(query, limit).await.ok()?;
    if results.is_empty() {
        return None;
    }

    let formatted = QueryEngine::new().format_results_as_context(&results);
    Some(format!("### Implicit memory (session history)\n\n{formatted}"))
}

/// Keyword search in wiki markdown pages under the project memory directory.
async fn search_wiki_pages(
    project_root: &std::path::Path,
    query: &str,
    limit: usize,
    permanent: bool,
) -> Option<String> {
    use crate::domain::memory::load_resolved_config;
    use tokio::fs;

    let wiki_dir = if permanent {
        crate::domain::memory::config::permanent_wiki_path()
    } else {
        let mem_cfg = load_resolved_config(project_root).await.ok()?;
        mem_cfg.wiki_path(project_root)
    };

    if !wiki_dir.is_dir() {
        return None;
    }

    let mut read_dir = fs::read_dir(&wiki_dir).await.ok()?;
    let mut pages: Vec<(String, String)> = Vec::new(); // (filename, content)
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if path.extension().map_or(false, |e| e.eq_ignore_ascii_case("md")) {
            if let Ok(content) = fs::read_to_string(&path).await {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                pages.push((name, content));
            }
        }
    }

    if pages.is_empty() {
        return None;
    }

    // Simple case-insensitive keyword match — score by number of query-term hits.
    let keywords: Vec<String> = query
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .map(|w| w.to_lowercase())
        .collect();

    let mut scored: Vec<(usize, String, String)> = pages
        .into_iter()
        .filter_map(|(name, content)| {
            let lc = content.to_lowercase();
            let score: usize = keywords.iter().map(|kw| lc.matches(kw.as_str()).count()).sum();
            if score == 0 {
                None
            } else {
                Some((score, name, content))
            }
        })
        .collect();

    if scored.is_empty() {
        return None;
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(limit);

    let label = if permanent {
        "Permanent (cross-project) wiki"
    } else {
        "Project wiki (explicit memory)"
    };

    let mut out = format!("### {label}\n\n");
    for (_, name, content) in &scored {
        let excerpt = excerpt_around_keywords(&content, &keywords, 400);
        out.push_str(&format!(
            "**`{name}`**\n\n{excerpt}\n\n---\n\n"
        ));
    }
    Some(out)
}

/// Search the permanent cross-project wiki.
async fn search_permanent_wiki(query: &str, limit: usize) -> Option<String> {
    search_wiki_pages(std::path::Path::new(""), query, limit, true).await
}

/// Return up to `max_chars` of content centred on the first keyword hit.
fn excerpt_around_keywords(content: &str, keywords: &[String], max_chars: usize) -> String {
    let lc = content.to_lowercase();
    let hit_pos = keywords
        .iter()
        .filter_map(|kw| lc.find(kw.as_str()))
        .min()
        .unwrap_or(0);

    let half = max_chars / 2;
    let start = hit_pos.saturating_sub(half);
    // Align to a char boundary.
    let start = content
        .char_indices()
        .map(|(i, _)| i)
        .filter(|&i| i >= start)
        .next()
        .unwrap_or(0);
    let end = (start + max_chars).min(content.len());
    let end = content
        .char_indices()
        .map(|(i, _)| i)
        .filter(|&i| i >= end)
        .next()
        .unwrap_or(content.len());

    let mut excerpt = content[start..end].to_string();
    if start > 0 {
        excerpt = format!("…{excerpt}");
    }
    if end < content.len() {
        excerpt.push('…');
    }
    excerpt
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "recall",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language query or keyword phrase to search the knowledge base"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 5, max 20)",
                    "minimum": 1,
                    "maximum": 20
                },
                "scope": {
                    "type": "string",
                    "description": "Which memory stores to search: \"implicit\" (session history), \"wiki\" (project wiki), \"permanent\" (cross-project wiki), or \"all\" (default)",
                    "enum": ["all", "implicit", "wiki", "permanent"]
                }
            },
            "required": ["query"]
        }),
    )
}
