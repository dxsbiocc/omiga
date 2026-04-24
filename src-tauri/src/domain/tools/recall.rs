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
            if let Some(implicit_text) = query_implicit_memory(project_root, &query, limit).await {
                total_results += 1;
                sections.push(implicit_text);
            }
        }

        // ── 2. Wiki pages (explicit memory) ─────────────────────────────────────
        if search_wiki {
            if let Some(wiki_text) = search_wiki_pages(project_root, &query, limit, false).await {
                total_results += 1;
                sections.push(wiki_text);
            }
        }

        // ── 3. Permanent (cross-project) wiki ────────────────────────────────────
        if search_permanent {
            if let Some(perm_text) = search_permanent_wiki(&query, limit).await {
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
    Some(format!(
        "### Implicit memory (session history)\n\n{formatted}"
    ))
}

/// Keyword search in wiki markdown pages under the project memory directory.
async fn search_wiki_pages(
    project_root: &std::path::Path,
    query: &str,
    limit: usize,
    permanent: bool,
) -> Option<String> {
    use crate::domain::memory::load_resolved_config;

    let wiki_dir = if permanent {
        crate::domain::memory::config::permanent_wiki_path()
    } else {
        let mem_cfg = load_resolved_config(project_root).await.ok()?;
        mem_cfg.wiki_path(project_root)
    };

    let results = crate::domain::memory::search_markdown_wiki(&wiki_dir, query, limit).await;
    if results.is_empty() {
        return None;
    }

    let label = if permanent {
        "Permanent (cross-project) wiki"
    } else {
        "Project wiki (explicit memory)"
    };

    let mut out = format!("### {label}\n\n");
    for result in &results {
        let name = std::path::Path::new(&result.path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| result.path.clone());
        out.push_str(&format!("**`{name}`**\n\n{}\n\n---\n\n", result.excerpt));
    }
    Some(out)
}

/// Search the permanent cross-project wiki.
async fn search_permanent_wiki(query: &str, limit: usize) -> Option<String> {
    search_wiki_pages(std::path::Path::new(""), query, limit, true).await
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
