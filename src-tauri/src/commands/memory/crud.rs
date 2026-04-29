//! Long-term memory CRUD, Dossier, and Source Registry Tauri commands.

use crate::domain::memory::load_resolved_config;
use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Long-term entry DTO ───────────────────────────────────────────────────────

/// DTO returned to the frontend for each long-term memory entry.
#[derive(Debug, Serialize)]
pub struct LongTermEntryDto {
    pub path: String,
    pub topic: String,
    pub summary: String,
    pub kind: String,
    pub confidence: f32,
    pub stability: f32,
    pub importance: f32,
    pub reuse_probability: f32,
    pub retention_class: String,
    pub status: String,
    pub created_at: String,
    pub last_reused_at: Option<String>,
    pub expires_at: Option<String>,
    pub source_sessions: Vec<String>,
    pub entities: Vec<String>,
    pub global: bool,
}

impl LongTermEntryDto {
    fn from_entry(
        path: std::path::PathBuf,
        entry: crate::domain::memory::long_term::LongTermMemoryEntry,
        global: bool,
    ) -> Self {
        Self {
            path: path.to_string_lossy().to_string(),
            topic: entry.topic,
            summary: entry.summary,
            kind: entry.kind.to_string(),
            confidence: entry.confidence,
            stability: entry.stability,
            importance: entry.importance,
            reuse_probability: entry.reuse_probability,
            retention_class: format!("{:?}", entry.retention_class),
            status: format!("{:?}", entry.status),
            created_at: entry.created_at,
            last_reused_at: entry.last_reused_at,
            expires_at: entry.expires_at,
            source_sessions: entry.source_sessions,
            entities: entry.entities,
            global,
        }
    }
}

// ── Scoped path helpers ──────────────────────────────────────────────────────

async fn canonical_memory_root(root: &Path) -> Result<PathBuf, AppError> {
    tokio::fs::create_dir_all(root)
        .await
        .map_err(|e| AppError::Unknown(format!("prepare memory root: {e}")))?;
    tokio::fs::canonicalize(root)
        .await
        .map_err(|e| AppError::Unknown(format!("resolve memory root: {e}")))
}

async fn canonical_existing_file(path: &Path) -> Result<Option<PathBuf>, AppError> {
    match tokio::fs::metadata(path).await {
        Ok(meta) => {
            if !meta.is_file() {
                return Err(AppError::Unknown(
                    "memory entry path must point to a file".to_string(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(AppError::Unknown(format!(
                "read memory entry metadata: {e}"
            )))
        }
    }

    tokio::fs::canonicalize(path)
        .await
        .map(Some)
        .map_err(|e| AppError::Unknown(format!("resolve memory entry path: {e}")))
}

async fn scoped_existing_file_path(
    entry_path: &str,
    allowed_roots: &[PathBuf],
    label: &str,
) -> Result<Option<PathBuf>, AppError> {
    let trimmed = entry_path.trim();
    if trimmed.is_empty() {
        return Err(AppError::Unknown(format!("{label} path is empty")));
    }

    let Some(target) = canonical_existing_file(Path::new(trimmed)).await? else {
        return Ok(None);
    };

    for root in allowed_roots {
        let root = canonical_memory_root(root).await?;
        if target.starts_with(&root) {
            return Ok(Some(target));
        }
    }

    Err(AppError::Unknown(format!(
        "{label} path outside resolved memory root"
    )))
}

async fn scoped_long_term_entry_path(
    project_path: &str,
    entry_path: &str,
) -> Result<Option<PathBuf>, AppError> {
    use crate::domain::memory::config::permanent_long_term_path;

    let root = super::project_root(project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    scoped_existing_file_path(
        entry_path,
        &[cfg.long_term_path(&root), permanent_long_term_path()],
        "long-term memory entry",
    )
    .await
}

async fn scoped_source_entry_path(
    project_path: &str,
    entry_path: &str,
) -> Result<Option<PathBuf>, AppError> {
    use crate::domain::memory::source_registry::sources_dir;

    let root = super::project_root(project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    scoped_existing_file_path(entry_path, &[sources_dir(&lt)], "source memory entry").await
}

// ── Long-term memory commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn memory_list_long_term(
    project_path: String,
    scope: Option<String>,
) -> Result<Vec<LongTermEntryDto>, AppError> {
    use crate::domain::memory::{config::permanent_long_term_path, long_term::list_entries};
    let root = super::project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    let perm_lt = permanent_long_term_path();
    let scope = scope.as_deref().unwrap_or("all");

    let mut entries: Vec<LongTermEntryDto> = Vec::new();
    if scope == "project" || scope == "all" {
        let project = list_entries(&lt).await;
        entries.extend(
            project
                .into_iter()
                .map(|(p, e)| LongTermEntryDto::from_entry(p, e, false)),
        );
    }
    if scope == "global" || scope == "all" {
        let global = list_entries(&perm_lt).await;
        entries.extend(
            global
                .into_iter()
                .map(|(p, e)| LongTermEntryDto::from_entry(p, e, true)),
        );
    }
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

#[tauri::command]
pub async fn memory_archive_long_term_entry(
    project_path: String,
    entry_path: String,
) -> Result<(), AppError> {
    use crate::domain::memory::long_term::{EntryStatus, LongTermMemoryEntry};
    if let Some(path) = scoped_long_term_entry_path(&project_path, &entry_path).await? {
        let raw = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("read entry: {e}")))?;
        if let Ok(mut entry) = serde_json::from_str::<LongTermMemoryEntry>(&raw) {
            entry.status = EntryStatus::Archived;
            if let Ok(json) = serde_json::to_string_pretty(&entry) {
                tokio::fs::write(&path, json)
                    .await
                    .map_err(|e| AppError::Unknown(format!("archive entry: {e}")))?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn memory_delete_long_term_entry(
    project_path: String,
    entry_path: String,
) -> Result<(), AppError> {
    if let Some(path) = scoped_long_term_entry_path(&project_path, &entry_path).await? {
        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("delete entry: {e}")))?;
    }
    Ok(())
}

// ── Dossier ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DossierDto {
    pub slug: String,
    pub title: String,
    pub brief: String,
    pub current_beliefs: Vec<String>,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub next_steps: Vec<String>,
    pub updated_at: String,
    pub rendered: String,
}

#[tauri::command]
pub async fn memory_get_dossier(project_path: String) -> Result<DossierDto, AppError> {
    use crate::domain::memory::dossier::load_latest_dossier;
    let root = super::project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    if let Some((slug, dossier)) = load_latest_dossier(&lt).await {
        let rendered = dossier.render_for_hot_memory();
        Ok(DossierDto {
            slug,
            title: dossier.title,
            brief: dossier.brief,
            current_beliefs: dossier.current_beliefs,
            decisions: dossier.decisions,
            open_questions: dossier.open_questions,
            next_steps: dossier.next_steps,
            updated_at: dossier.updated_at,
            rendered,
        })
    } else {
        Ok(DossierDto {
            slug: String::new(),
            title: String::new(),
            brief: String::new(),
            current_beliefs: vec![],
            decisions: vec![],
            open_questions: vec![],
            next_steps: vec![],
            updated_at: String::new(),
            rendered: String::new(),
        })
    }
}

/// Request body for memory_save_dossier — consolidates the many fields
/// into a single struct to satisfy the Clippy too-many-arguments lint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDossierRequest {
    pub project_path: String,
    /// Slug returned by memory_get_dossier; empty → derived from project dir name.
    pub slug: String,
    pub title: String,
    pub brief: String,
    pub current_beliefs: Vec<String>,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub next_steps: Vec<String>,
}

#[tauri::command]
pub async fn memory_save_dossier(req: SaveDossierRequest) -> Result<(), AppError> {
    use crate::domain::memory::dossier::{save_dossier, Dossier};
    let root = super::project_root(&req.project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    let slug = if req.slug.trim().is_empty() {
        crate::domain::memory::long_term::slugify_pub(
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project"),
        )
    } else {
        req.slug
    };
    let dossier = Dossier {
        title: req.title,
        brief: req.brief,
        current_beliefs: req.current_beliefs,
        decisions: req.decisions,
        open_questions: req.open_questions,
        next_steps: req.next_steps,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    save_dossier(&lt, &slug, &dossier)
        .await
        .map_err(|e| AppError::Unknown(format!("save dossier: {e}")))?;
    Ok(())
}

// ── Prune + Source Registry ───────────────────────────────────────────────────

#[tauri::command]
pub async fn memory_prune_stale(project_path: String) -> Result<usize, AppError> {
    use crate::domain::memory::{
        config::permanent_long_term_path, long_term::prune_stale_entries,
        source_registry::prune_stale_sources,
    };
    let root = super::project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    let perm_lt = permanent_long_term_path();
    let removed = prune_stale_entries(&lt, false).await
        + prune_stale_entries(&perm_lt, false).await
        + prune_stale_sources(&lt, false).await;
    Ok(removed)
}

#[tauri::command]
pub async fn memory_list_sources(project_path: String) -> Result<Vec<SourceEntryDto>, AppError> {
    use crate::domain::memory::source_registry::list_active_sources_with_paths;
    let root = super::project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    let mut entries = list_active_sources_with_paths(&lt).await;
    entries.sort_by(|a, b| b.1.last_used_at.cmp(&a.1.last_used_at));
    Ok(entries
        .into_iter()
        .map(|(path, entry)| {
            let mut dto = SourceEntryDto::from(entry);
            dto.path = path.to_string_lossy().into_owned();
            dto
        })
        .collect())
}

#[tauri::command]
pub async fn memory_delete_source(
    project_path: String,
    entry_path: String,
) -> Result<(), AppError> {
    if let Some(path) = scoped_source_entry_path(&project_path, &entry_path).await? {
        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("delete source: {e}")))?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEntryDto {
    pub path: String,
    pub url: String,
    pub canonical_url: String,
    pub title: Option<String>,
    pub domain: String,
    pub gist: Option<String>,
    pub accessed_at: String,
    pub last_used_at: String,
    pub use_count: u32,
    pub sessions: Vec<String>,
    pub query_context: Vec<String>,
    pub expires_at: Option<String>,
}

impl From<crate::domain::memory::source_registry::SourceEntry> for SourceEntryDto {
    fn from(e: crate::domain::memory::source_registry::SourceEntry) -> Self {
        Self {
            path: String::new(),
            url: e.url,
            canonical_url: e.canonical_url,
            title: e.title,
            domain: e.domain,
            gist: e.gist,
            accessed_at: e.accessed_at,
            last_used_at: e.last_used_at,
            use_count: e.use_count,
            sessions: e.sessions,
            query_context: e.query_context,
            expires_at: e.expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scoped_existing_file_path_allows_files_under_memory_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("memory");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let entry = root.join("entry.json");
        tokio::fs::write(&entry, "{}").await.unwrap();

        let resolved = scoped_existing_file_path(entry.to_str().unwrap(), &[root], "test entry")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(resolved, tokio::fs::canonicalize(entry).await.unwrap());
    }

    #[tokio::test]
    async fn scoped_existing_file_path_rejects_files_outside_memory_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("memory");
        let outside = temp.path().join("outside.json");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(&outside, "{}").await.unwrap();

        let err = scoped_existing_file_path(outside.to_str().unwrap(), &[root], "test entry")
            .await
            .unwrap_err();

        assert!(format!("{err:?}").contains("outside resolved memory root"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn scoped_existing_file_path_rejects_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("memory");
        let outside = temp.path().join("outside.json");
        let symlink = root.join("link.json");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(&outside, "{}").await.unwrap();
        std::os::unix::fs::symlink(&outside, &symlink).unwrap();

        let err = scoped_existing_file_path(symlink.to_str().unwrap(), &[root], "test entry")
            .await
            .unwrap_err();

        assert!(format!("{err:?}").contains("outside resolved memory root"));
    }
}
