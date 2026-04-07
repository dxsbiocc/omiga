//! Wiki service — implements the Karpathy LLM Wiki pattern for Omiga.
//!
//! Directory layout (relative to `project_root`):
//! ```text
//! .omiga/wiki/
//!   index.md   — content catalog (all pages with one-line summaries)
//!   log.md     — append-only operations log
//!   <slug>.md  — individual wiki pages
//! ```
//!
//! The transparent hook in `commands/chat.rs` calls `query_relevant_context`
//! to inject a short wiki excerpt into the system prompt before every LLM call
//! (when a wiki exists). No extra token cost when wiki is absent.

use std::path::{Path, PathBuf};
use tokio::fs;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Root wiki directory for a project.
pub fn wiki_dir(project_root: &Path) -> PathBuf {
    project_root.join(".omiga").join("wiki")
}

/// Path to `index.md` inside the wiki directory.
pub fn index_path(project_root: &Path) -> PathBuf {
    wiki_dir(project_root).join("index.md")
}

/// Path to `log.md` inside the wiki directory.
pub fn log_path(project_root: &Path) -> PathBuf {
    wiki_dir(project_root).join("log.md")
}

/// Path for a named wiki page (slugified).
pub fn page_path(project_root: &Path, slug: &str) -> PathBuf {
    wiki_dir(project_root).join(format!("{}.md", sanitize_slug(slug)))
}

/// Sanitize a page slug to safe filename characters.
fn sanitize_slug(slug: &str) -> String {
    slug.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Metadata snapshot of the wiki (returned to frontend).
#[derive(Debug, serde::Serialize)]
pub struct WikiStatus {
    pub exists: bool,
    pub page_count: u32,
    pub index_summary: Option<String>,
    pub wiki_dir: String,
    pub last_log_entry: Option<String>,
}

/// Return current wiki status for the project.
pub async fn get_status(project_root: &Path) -> WikiStatus {
    let dir = wiki_dir(project_root);
    if !dir.exists() {
        return WikiStatus {
            exists: false,
            page_count: 0,
            index_summary: None,
            wiki_dir: dir.to_string_lossy().to_string(),
            last_log_entry: None,
        };
    }

    let page_count = count_pages(project_root).await;
    let index_summary = read_file_opt(&index_path(project_root)).await
        .map(|s| s.lines().take(5).collect::<Vec<_>>().join("\n"));
    let last_log_entry = read_last_log_entry(project_root).await;

    WikiStatus {
        exists: true,
        page_count,
        index_summary,
        wiki_dir: dir.to_string_lossy().to_string(),
        last_log_entry,
    }
}

async fn count_pages(project_root: &Path) -> u32 {
    let dir = wiki_dir(project_root);
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return 0;
    };
    let mut count = 0u32;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if n.ends_with(".md") && n != "index.md" && n != "log.md" {
            count += 1;
        }
    }
    count
}

async fn read_last_log_entry(project_root: &Path) -> Option<String> {
    let content = read_file_opt(&log_path(project_root)).await?;
    content.lines().rev().find(|l| !l.trim().is_empty()).map(|l| l.to_string())
}

// ---------------------------------------------------------------------------
// Page operations
// ---------------------------------------------------------------------------

/// Write (create or overwrite) a wiki page.
pub async fn write_page(project_root: &Path, slug: &str, content: &str) -> std::io::Result<()> {
    let dir = wiki_dir(project_root);
    fs::create_dir_all(&dir).await?;
    fs::write(page_path(project_root, slug), content).await
}

/// Read a wiki page. Returns `None` if it does not exist.
pub async fn read_page(project_root: &Path, slug: &str) -> Option<String> {
    read_file_opt(&page_path(project_root, slug)).await
}

/// Delete a wiki page. Silently succeeds if it does not exist.
pub async fn delete_page(project_root: &Path, slug: &str) -> std::io::Result<()> {
    let p = page_path(project_root, slug);
    if p.exists() {
        fs::remove_file(p).await?;
    }
    Ok(())
}

/// List all page slugs (excluding `index` and `log`).
pub async fn list_pages(project_root: &Path) -> Vec<String> {
    let dir = wiki_dir(project_root);
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return vec![];
    };
    let mut pages = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let n = name.to_string_lossy().to_string();
        if n.ends_with(".md") && n != "index.md" && n != "log.md" {
            pages.push(n.trim_end_matches(".md").to_string());
        }
    }
    pages.sort();
    pages
}

// ---------------------------------------------------------------------------
// Index operations
// ---------------------------------------------------------------------------

/// Write the wiki index (overwrites `index.md`).
pub async fn write_index(project_root: &Path, content: &str) -> std::io::Result<()> {
    let dir = wiki_dir(project_root);
    fs::create_dir_all(&dir).await?;
    fs::write(index_path(project_root), content).await
}

/// Read the wiki index. Returns `None` if it does not exist.
pub async fn read_index(project_root: &Path) -> Option<String> {
    read_file_opt(&index_path(project_root)).await
}

// ---------------------------------------------------------------------------
// Log operations
// ---------------------------------------------------------------------------

/// Append a timestamped entry to `log.md`.
pub async fn append_log(project_root: &Path, entry: &str) -> std::io::Result<()> {
    let dir = wiki_dir(project_root);
    fs::create_dir_all(&dir).await?;
    let path = log_path(project_root);
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let line = format!("[{ts}] {entry}\n");
    let existing = read_file_opt(&path).await.unwrap_or_default();
    fs::write(path, format!("{existing}{line}")).await
}

/// Read the full log. Returns `None` if it does not exist.
pub async fn read_log(project_root: &Path) -> Option<String> {
    read_file_opt(&log_path(project_root)).await
}

// ---------------------------------------------------------------------------
// Transparent hook: query_relevant_context
// ---------------------------------------------------------------------------

/// Called by the chat system prompt builder before every LLM call.
///
/// If a wiki exists and relevant pages are found for `user_message`, returns
/// a formatted section to prepend to the system prompt. Otherwise `None`.
/// Cost: one async file read (index.md) + up to 3 page reads.
pub async fn query_relevant_context(user_message: &str, project_root: &Path) -> Option<String> {
    let index = read_index(project_root).await?;
    if index.trim().is_empty() {
        return None;
    }

    let keywords = extract_keywords(user_message);
    if keywords.is_empty() {
        return None;
    }

    // Parse index.md: look for lines that match keywords.
    // Index format (by convention): `- [Title](slug.md) — description`
    // or plain `## Category\n- slug: description`
    let mut matched_slugs: Vec<String> = Vec::new();
    for line in index.lines() {
        let lower = line.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(kw)) {
            // Try to extract slug from markdown link syntax: [Title](slug.md)
            if let Some(slug) = extract_slug_from_index_line(line) {
                if !matched_slugs.contains(&slug) {
                    matched_slugs.push(slug);
                }
            }
        }
        if matched_slugs.len() >= 3 {
            break;
        }
    }

    if matched_slugs.is_empty() {
        // Fallback: no keyword match — don't inject anything
        return None;
    }

    // Read page excerpts (first 400 chars each)
    let mut snippets: Vec<String> = Vec::new();
    for slug in &matched_slugs {
        if let Some(content) = read_page(project_root, slug).await {
            let excerpt: String = content.chars().take(400).collect();
            snippets.push(format!("### {slug}\n{excerpt}"));
        }
    }

    if snippets.is_empty() {
        return None;
    }

    Some(format_wiki_context_section(&snippets))
}

/// Format the wiki context into a system prompt section.
fn format_wiki_context_section(snippets: &[String]) -> String {
    format!(
        "## Project Knowledge Base (Wiki)\n\
         The following pages from the project wiki are relevant to the current task. \
         Use this context to inform your response. For more pages use the `Agent` tool \
         with `subagent_type: \"wiki-agent\"`.\n\n{}",
        snippets.join("\n\n---\n\n")
    )
}

/// Return the system prompt for a wiki-agent sub-agent.
/// Injected by `run_subagent_session` when `subagent_type == "wiki-agent"`.
pub fn wiki_agent_system_prompt(project_root: &Path) -> String {
    let wiki_path = wiki_dir(project_root).to_string_lossy().to_string();
    format!(
        "## Wiki Agent Mode\n\
         You are a specialized wiki agent for the Omiga project wiki located at `{wiki_path}`.\n\
         \n\
         Your responsibilities:\n\
         - **Ingest**: When given source material (articles, code, docs), extract key information \
           and create/update wiki pages (10–15 per source). Write summaries, entity pages, concept \
           pages, and maintain cross-references in `index.md`. Log each ingest in `log.md`.\n\
         - **Query**: Search wiki pages for relevant information and synthesize a concise answer \
           with citations. Query results may become new wiki pages.\n\
         - **Lint**: Audit wiki health — check for contradictions, stale claims, orphaned pages, \
           missing cross-references. Suggest new investigations. Log lint passes in `log.md`.\n\
         \n\
         Wiki structure:\n\
         - `index.md` — content catalog, one entry per page: `- [Title](slug.md) — description`\n\
         - `log.md` — append-only log: `[timestamp] operation: details`\n\
         - `<slug>.md` — individual pages\n\
         \n\
         Always read `index.md` first to understand existing content. Prefer editing existing \
         pages over creating duplicates. Keep page excerpts under 800 lines. Return a concise \
         summary of what was ingested / queried / linted to the caller."
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_keywords(text: &str) -> Vec<String> {
    // Simple tokenizer: lowercase words, filter stop words and short tokens
    const STOP_WORDS: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "can", "i", "you", "he", "she", "it", "we",
        "they", "this", "that", "these", "those", "and", "or", "but", "in",
        "on", "at", "to", "for", "of", "with", "by", "from", "up", "about",
        "into", "through", "during", "what", "how", "why", "when", "where",
        "who", "which", "my", "your", "his", "her", "its", "our", "their",
        "me", "him", "us", "them", "so", "if", "then", "than", "as", "not",
        "no", "yes", "just", "also", "please", "help", "need",
    ];

    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .take(10)
        .collect()
}

fn extract_slug_from_index_line(line: &str) -> Option<String> {
    // Match `[Title](slug.md)` pattern
    let start = line.find("](")? + 2;
    let end = line[start..].find(')')?;
    let raw = &line[start..start + end];
    let slug = raw.trim_end_matches(".md");
    if slug.is_empty() || slug.contains('/') || slug.contains('\\') {
        return None;
    }
    Some(slug.to_string())
}

async fn read_file_opt(path: &Path) -> Option<String> {
    fs::read_to_string(path).await.ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_slug_replaces_special_chars() {
        assert_eq!(sanitize_slug("Hello World!"), "hello-world-");
        assert_eq!(sanitize_slug("my-page_v2"), "my-page_v2");
    }

    #[test]
    fn extract_keywords_filters_stop_words() {
        let kws = extract_keywords("What is the best way to implement a cache?");
        assert!(kws.contains(&"best".to_string()) || kws.contains(&"implement".to_string()));
        assert!(!kws.contains(&"what".to_string()));
        assert!(!kws.contains(&"the".to_string()));
    }

    #[test]
    fn extract_slug_from_index_line_parses_link() {
        let line = "- [My Topic](my-topic.md) — description here";
        assert_eq!(extract_slug_from_index_line(line), Some("my-topic".to_string()));
    }

    #[test]
    fn extract_slug_none_without_link() {
        let line = "- plain text line without link";
        assert_eq!(extract_slug_from_index_line(line), None);
    }
}
