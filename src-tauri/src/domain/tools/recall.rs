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
- Previously accessed web pages or papers (scope="sources")

Arguments:
- `query`: natural-language query or keyword phrase
- `limit`: max results to return (default 5, max 20)
- `scope`: which stores to search —
    "implicit"   — auto-indexed session history
    "wiki"       — project wiki pages
    "long_term"  — promoted decisions/insights (project + global)
    "permanent"  — cross-project wiki + global long-term
    "sources"    — web pages and papers previously fetched in this project
    "all"        — everything (default)

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

        let mem_cfg = crate::domain::memory::load_resolved_config(project_root)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to load memory config: {}", e),
            })?;
        let memory = crate::domain::memory::MemorySystem::with_config(project_root, mem_cfg);

        // "sources" scope: query the source registry directly.
        if scope == "sources" {
            let lt_root = memory.long_term_path();
            let matches =
                crate::domain::memory::source_registry::search_sources(&lt_root, &query, limit)
                    .await;
            let content = format_source_results(&matches);
            let out = RecallOutput {
                query,
                results_found: matches.len(),
                content,
            };
            return Ok(out.into_stream());
        }

        let mut unified = memory
            .query_with_session(ctx.working_memory_context.as_deref(), &query, limit)
            .await;

        unified.results.retain(|result| match scope.as_str() {
            "implicit" => matches!(
                result.source_type,
                crate::domain::memory::MemorySourceType::Implicit
            ),
            "wiki" => matches!(
                result.source_type,
                crate::domain::memory::MemorySourceType::KnowledgeBaseProject
                    | crate::domain::memory::MemorySourceType::KnowledgeBaseGlobal
            ),
            "long_term" => matches!(
                result.source_type,
                crate::domain::memory::MemorySourceType::LongTermProject
                    | crate::domain::memory::MemorySourceType::LongTermGlobal
            ),
            "permanent" => matches!(
                result.source_type,
                crate::domain::memory::MemorySourceType::LongTermGlobal
                    | crate::domain::memory::MemorySourceType::KnowledgeBaseGlobal
            ),
            _ => true,
        });
        let total_results = unified.results.len();
        let content = format_unified_results(&unified.results);

        let out = RecallOutput {
            query,
            results_found: total_results,
            content,
        };
        Ok(out.into_stream())
    }
}

fn format_source_results(
    matches: &[crate::domain::memory::source_registry::SourceMatch],
) -> String {
    if matches.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for m in matches {
        out.push_str(&format!(
            "### {} [SourceRegistry]\n\n*URL: `{}`*  *Domain: {}*\n\n{}\n\n---\n\n",
            m.title.as_deref().unwrap_or(&m.url),
            m.url,
            m.domain,
            m.gist.as_deref().unwrap_or("(no summary available)"),
        ));
    }
    out
}

fn format_unified_results(results: &[crate::domain::memory::MemoryQueryMatch]) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for result in results {
        out.push_str(&format!(
            "### {} [{}]\n\n*Source: `{}`*\n\n{}\n\n---\n\n",
            result.title,
            result.source_type.label(),
            result.path,
            result.excerpt
        ));
    }
    out
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
                    "description": "Which memory stores to search: \"implicit\" (session history), \"wiki\" (project wiki), \"long_term\" (promoted decisions+insights), \"permanent\" (cross-project wiki+long-term), \"sources\" (previously fetched web pages/papers), or \"all\" (default)",
                    "enum": ["all", "implicit", "wiki", "long_term", "permanent", "sources"]
                }
            },
            "required": ["query"]
        }),
    )
}
