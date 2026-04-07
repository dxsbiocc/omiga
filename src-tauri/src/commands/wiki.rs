//! Wiki commands — Tauri IPC surface for the wiki feature.
//!
//! All commands operate on the current project root (resolved from the `project_path` arg).
//! Frontend should pass `currentSession.projectPath` for every call.

use super::CommandResult;
use crate::domain::wiki;
use crate::errors::AppError;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn project_root(project_path: &str) -> std::path::PathBuf {
    let p = project_path.trim();
    if p.is_empty() || p == "." {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(p)
    }
}

fn io_err(e: std::io::Error) -> AppError {
    AppError::Unknown(e.to_string())
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Return current wiki status (exists, page count, index preview, etc.)
#[tauri::command]
pub async fn wiki_get_status(project_path: String) -> CommandResult<wiki::WikiStatus> {
    Ok(wiki::get_status(&project_root(&project_path)).await)
}

// ---------------------------------------------------------------------------
// Page CRUD
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct WritePageRequest {
    pub project_path: String,
    pub slug: String,
    pub content: String,
}

/// Create or overwrite a wiki page.
#[tauri::command]
pub async fn wiki_write_page(req: WritePageRequest) -> CommandResult<()> {
    let root = project_root(&req.project_path);
    wiki::write_page(&root, &req.slug, &req.content)
        .await
        .map_err(io_err)
}

/// Read a wiki page. Returns `None` if it does not exist.
#[tauri::command]
pub async fn wiki_read_page(
    project_path: String,
    slug: String,
) -> CommandResult<Option<String>> {
    Ok(wiki::read_page(&project_root(&project_path), &slug).await)
}

/// Delete a wiki page.
#[tauri::command]
pub async fn wiki_delete_page(project_path: String, slug: String) -> CommandResult<()> {
    wiki::delete_page(&project_root(&project_path), &slug)
        .await
        .map_err(io_err)
}

/// List all page slugs in the wiki.
#[tauri::command]
pub async fn wiki_list_pages(project_path: String) -> CommandResult<Vec<String>> {
    Ok(wiki::list_pages(&project_root(&project_path)).await)
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

/// Overwrite `index.md` with the provided content.
#[tauri::command]
pub async fn wiki_write_index(project_path: String, content: String) -> CommandResult<()> {
    wiki::write_index(&project_root(&project_path), &content)
        .await
        .map_err(io_err)
}

/// Read `index.md`. Returns `None` if it does not exist.
#[tauri::command]
pub async fn wiki_read_index(project_path: String) -> CommandResult<Option<String>> {
    Ok(wiki::read_index(&project_root(&project_path)).await)
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// Append a timestamped entry to `log.md`.
#[tauri::command]
pub async fn wiki_append_log(project_path: String, entry: String) -> CommandResult<()> {
    wiki::append_log(&project_root(&project_path), &entry)
        .await
        .map_err(io_err)
}

/// Read `log.md`. Returns `None` if it does not exist.
#[tauri::command]
pub async fn wiki_read_log(project_path: String) -> CommandResult<Option<String>> {
    Ok(wiki::read_log(&project_root(&project_path)).await)
}

// ---------------------------------------------------------------------------
// Inline query (no sub-agent)
// ---------------------------------------------------------------------------

/// Response from a keyword search over the wiki index.
#[derive(Debug, Serialize)]
pub struct WikiQueryResult {
    /// Matching page slugs
    pub matched_slugs: Vec<String>,
    /// Page excerpts (first ~400 chars each)
    pub excerpts: Vec<WikiPageExcerpt>,
}

#[derive(Debug, Serialize)]
pub struct WikiPageExcerpt {
    pub slug: String,
    pub excerpt: String,
}

/// Keyword search over wiki pages — same logic as the transparent hook.
/// Useful for frontend wiki search UI.
#[tauri::command]
pub async fn wiki_query(project_path: String, query: String) -> CommandResult<WikiQueryResult> {
    let root = project_root(&project_path);
    let index = wiki::read_index(&root).await.unwrap_or_default();
    let pages = wiki::list_pages(&root).await;

    let keywords: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut matched_slugs: Vec<String> = Vec::new();
    // Match against index
    for line in index.lines() {
        let lower = line.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(kw)) {
            if let Some(slug) = extract_slug_from_line(line) {
                if !matched_slugs.contains(&slug) {
                    matched_slugs.push(slug);
                }
            }
        }
    }
    // Also match directly against page slugs
    for slug in &pages {
        let lower = slug.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(kw)) && !matched_slugs.contains(slug) {
            matched_slugs.push(slug.clone());
        }
    }

    let mut excerpts = Vec::new();
    for slug in &matched_slugs {
        if let Some(content) = wiki::read_page(&root, slug).await {
            let excerpt: String = content.chars().take(400).collect();
            excerpts.push(WikiPageExcerpt {
                slug: slug.clone(),
                excerpt,
            });
        }
    }

    Ok(WikiQueryResult { matched_slugs, excerpts })
}

fn extract_slug_from_line(line: &str) -> Option<String> {
    let start = line.find("](")?  + 2;
    let end = line[start..].find(')')?;
    let raw = &line[start..start + end];
    let slug = raw.trim_end_matches(".md");
    if slug.is_empty() || slug.contains('/') {
        return None;
    }
    Some(slug.to_string())
}

// ---------------------------------------------------------------------------
// Wiki directory path helper (frontend uses this for display)
// ---------------------------------------------------------------------------

/// Return the absolute path to the wiki directory.
#[tauri::command]
pub fn wiki_get_dir(project_path: String) -> String {
    wiki::wiki_dir(&project_root(&project_path))
        .to_string_lossy()
        .to_string()
}
