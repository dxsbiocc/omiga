//! Memory commands — Unified interface for the Memory system.
//!
//! Provides both explicit (wiki) and implicit (pageindex) memory management
//! with configurable paths.

use crate::domain::memory::{
    config::{MemoryConfig, MemoryMode},
    load_resolved_config,
    long_term::LongTermStatus,
    migration::{detect_structure_version, MemoryStructureVersion},
    permanent_long_term_path,
    permanent_profile::PermanentProfileStatus,
    permanent_wiki_path, registry,
    working_memory::WorkingMemoryStatus as SessionWorkingMemoryStatus,
    MemorySystem,
};
use crate::domain::pageindex::{IndexConfig, PageIndex, QueryResult};
use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

/// Memory level for import operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLevel {
    /// Project-level memory (stored in project directory)
    Project,
    /// User-level memory (stored in user's home directory)
    User,
}

impl MemoryLevel {
    /// Get the user memory root directory (~/.omiga)
    pub fn user_root() -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".omiga"))
            .unwrap_or_else(|| PathBuf::from(".omiga"))
    }

    /// Get the wiki path for this memory level
    pub async fn wiki_path(&self, project_path: &PathBuf) -> PathBuf {
        match self {
            MemoryLevel::Project => {
                let config = load_resolved_config(project_path).await.unwrap_or_default();
                config.wiki_path(project_path)
            }
            MemoryLevel::User => permanent_wiki_path(),
        }
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Response for memory configuration
#[derive(Debug, Serialize)]
pub struct MemoryConfigResponse {
    pub root_dir: String,
    pub wiki_dir: String,
    pub implicit_dir: String,
    /// Absolute path where raw original files are stored on wiki import.
    /// Defaults to `~/.omiga/memory/raw` when not configured.
    pub raw_dir: String,
    pub memory_mode: String,
    pub auto_build_index: bool,
    pub index_extensions: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub max_file_size: usize,
}

impl From<MemoryConfig> for MemoryConfigResponse {
    fn from(c: MemoryConfig) -> Self {
        let raw_dir = c.raw_path().to_string_lossy().to_string();
        Self {
            root_dir: c.root_dir.to_string_lossy().to_string(),
            wiki_dir: c.wiki_dir,
            implicit_dir: c.implicit_dir,
            raw_dir,
            memory_mode: match c.memory_mode {
                MemoryMode::UserHome => "user_home".to_string(),
                MemoryMode::ProjectRelative => "project_relative".to_string(),
            },
            auto_build_index: c.auto_build_index,
            index_extensions: c.index_extensions,
            exclude_dirs: c.exclude_dirs,
            max_file_size: c.max_file_size,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetMemoryConfigRequest {
    pub project_path: String,
    pub root_dir: Option<String>,
    pub wiki_dir: Option<String>,
    pub implicit_dir: Option<String>,
    /// Absolute path for raw file storage. Empty string resets to default.
    pub raw_dir: Option<String>,
    /// `user_home` | `project_relative`
    pub memory_mode: Option<String>,
    pub auto_build_index: Option<bool>,
    pub index_extensions: Option<Vec<String>>,
    pub exclude_dirs: Option<Vec<String>>,
    pub max_file_size: Option<usize>,
}

/// Unified memory status
#[derive(Debug, Serialize)]
pub struct UnifiedMemoryStatus {
    pub exists: bool,
    pub version: String,
    pub needs_migration: bool,
    pub explicit: ExplicitMemoryStatus,
    pub implicit: ImplicitMemoryStatus,
    pub permanent_profile: PermanentProfileStatus,
    pub working_memory: SessionWorkingMemoryStatus,
    pub long_term: LongTermStatus,
    pub knowledge_base: KnowledgeBaseStatus,
    pub source_registry: SourceRegistryStatus,
    pub paths: MemoryPaths,
}

#[derive(Debug, Serialize)]
pub struct ExplicitMemoryStatus {
    pub enabled: bool,
    pub document_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ImplicitMemoryStatus {
    pub enabled: bool,
    pub document_count: usize,
    pub section_count: usize,
    pub total_bytes: usize,
    pub last_build_time: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeBaseStatus {
    pub project_page_count: usize,
    pub global_page_count: usize,
}

#[derive(Debug, Serialize)]
pub struct MemoryPaths {
    pub root: String,
    pub wiki: String,
    pub implicit: String,
    /// User-level permanent wiki (`~/.omiga/memory/permanent/wiki`)
    pub permanent_wiki: String,
    /// Project-level long-term memory (`.../long_term`)
    pub long_term: String,
    /// User-level global long-term memory (`~/.omiga/memory/permanent/long_term`)
    pub permanent_long_term: String,
    /// Raw original file storage (`~/.omiga/memory/raw` by default)
    pub raw: String,
    /// Source registry directory (`.../long_term/sources`)
    pub sources: String,
}

#[derive(Debug, Serialize)]
pub struct SourceRegistryStatus {
    pub entry_count: usize,
    pub stale_count: usize,
}

// ---------------------------------------------------------------------------
// Config Management
// ---------------------------------------------------------------------------

/// Get current memory configuration
#[tauri::command]
pub async fn memory_get_config(project_path: String) -> Result<MemoryConfigResponse, AppError> {
    let root = project_root(&project_path);
    let config = load_resolved_config(&root).await.unwrap_or_default();
    Ok(config.into())
}

/// Set memory configuration
#[tauri::command]
pub async fn memory_set_config(
    req: SetMemoryConfigRequest,
) -> Result<MemoryConfigResponse, AppError> {
    let root = project_root(&req.project_path);

    let mut config = if let Some(c) = MemorySystem::load_config(&root).await {
        c
    } else {
        load_resolved_config(&root).await.unwrap_or_default()
    };

    if let Some(root_dir) = req.root_dir {
        config.root_dir = PathBuf::from(root_dir);
    }
    if let Some(wiki_dir) = req.wiki_dir {
        config.wiki_dir = wiki_dir;
    }
    if let Some(implicit_dir) = req.implicit_dir {
        config.implicit_dir = implicit_dir;
    }
    if let Some(mode) = req.memory_mode.as_deref() {
        config.memory_mode = match mode {
            "project_relative" => MemoryMode::ProjectRelative,
            _ => MemoryMode::UserHome,
        };
    }
    if let Some(auto_build) = req.auto_build_index {
        config.auto_build_index = auto_build;
    }
    if let Some(extensions) = req.index_extensions {
        config.index_extensions = extensions;
    }
    if let Some(exclude) = req.exclude_dirs {
        config.exclude_dirs = exclude;
    }
    if let Some(max_size) = req.max_file_size {
        config.max_file_size = max_size;
    }
    if let Some(raw) = req.raw_dir {
        if raw.is_empty() {
            config.raw_dir = None; // reset to default
        } else {
            config.raw_dir = Some(PathBuf::from(raw));
        }
    }

    config.validate()?;

    let memory = MemorySystem::with_config(&root, config);
    memory.save_config().await?;
    memory
        .init()
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?;
    register_project_memory(&root, memory.config()).await;

    Ok(memory.config().clone().into())
}

/// Detect current memory structure version
#[tauri::command]
pub async fn memory_detect_version(project_path: String) -> Result<String, AppError> {
    let root = project_root(&project_path);
    let version = detect_structure_version(&root).await;
    Ok(format!("{:?}", version))
}

/// Run migration if needed
#[tauri::command]
pub async fn memory_migrate(project_path: String) -> Result<bool, AppError> {
    let root = project_root(&project_path);
    let version = detect_structure_version(&root).await;

    if version.needs_migration() {
        crate::domain::memory::migration::migrate_if_needed(&root).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Unified Status
// ---------------------------------------------------------------------------

/// Get unified memory status
#[tauri::command]
pub async fn memory_get_unified_status(
    app_state: State<'_, crate::app_state::OmigaAppState>,
    project_path: String,
) -> Result<UnifiedMemoryStatus, AppError> {
    let root = project_root(&project_path);
    let version = detect_structure_version(&root).await;

    let config = load_resolved_config(&root).await.unwrap_or_default();
    let memory = MemorySystem::with_config(&root, config.clone());
    let stats = memory.stats().await;
    let permanent_profile_status = crate::domain::agents::load_user_omiga_context()
        .permanent_profile
        .status();
    let latest_session_id = app_state
        .repo
        .find_latest_session_id_for_project(&project_path)
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
        .or(app_state
            .repo
            .find_latest_session_id_for_project(&root.to_string_lossy())
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?);
    let working_memory_status = if let Some(session_id) = latest_session_id {
        crate::domain::memory::working_memory::load_state(&app_state.repo, &session_id)
            .await
            .map(|state| state.status())
            .map_err(|e| AppError::Unknown(e.to_string()))?
    } else {
        SessionWorkingMemoryStatus::default()
    };

    // Get implicit memory details
    let implicit_status = get_implicit_status(&root, &config).await?;

    register_project_memory(&root, memory.config()).await;

    Ok(UnifiedMemoryStatus {
        exists: version != MemoryStructureVersion::None,
        version: format!("{:?}", version),
        needs_migration: version.needs_migration(),
        explicit: ExplicitMemoryStatus {
            enabled: true,
            document_count: stats.explicit_documents,
        },
        implicit: implicit_status,
        permanent_profile: permanent_profile_status,
        working_memory: working_memory_status,
        long_term: LongTermStatus {
            project_entry_count: stats.long_term_project_entries,
            global_entry_count: stats.long_term_global_entries,
            stale_entry_count: stats.stale_long_term_entries,
        },
        knowledge_base: KnowledgeBaseStatus {
            project_page_count: stats.project_knowledge_pages,
            global_page_count: stats.global_knowledge_pages,
        },
        source_registry: SourceRegistryStatus {
            entry_count: stats.source_registry_count,
            stale_count: stats.stale_source_count,
        },
        paths: MemoryPaths {
            root: memory.root_path().to_string_lossy().to_string(),
            wiki: memory.wiki_path().to_string_lossy().to_string(),
            implicit: memory.implicit_path().to_string_lossy().to_string(),
            permanent_wiki: permanent_wiki_path().to_string_lossy().to_string(),
            long_term: memory.long_term_path().to_string_lossy().to_string(),
            permanent_long_term: permanent_long_term_path().to_string_lossy().to_string(),
            raw: config.raw_path().to_string_lossy().to_string(),
            sources: memory.sources_path().to_string_lossy().to_string(),
        },
    })
}

async fn get_implicit_status(
    root: &PathBuf,
    config: &MemoryConfig,
) -> Result<ImplicitMemoryStatus, AppError> {
    let implicit_path = config.implicit_path(root);
    let tree_path = implicit_path.join("tree.json");

    if !tree_path.exists() {
        return Ok(ImplicitMemoryStatus {
            enabled: config.auto_build_index,
            document_count: 0,
            section_count: 0,
            total_bytes: 0,
            last_build_time: None,
        });
    }

    // Load tree.json to get stats
    let content = tokio::fs::read_to_string(&tree_path)
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?;

    let tree: crate::domain::pageindex::DocumentTree =
        serde_json::from_str(&content).map_err(|e| AppError::Unknown(e.to_string()))?;

    // Get modification time
    let last_build = tokio::fs::metadata(&tree_path)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64);

    Ok(ImplicitMemoryStatus {
        enabled: config.auto_build_index,
        document_count: tree.document_count(),
        section_count: tree.section_count(),
        total_bytes: tree.total_bytes(),
        last_build_time: last_build,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn register_project_memory(root: &std::path::Path, config: &MemoryConfig) {
    if let Err(e) = registry::upsert_project_paths(
        root,
        &config.effective_root(root),
        &config.wiki_path(root),
        &config.implicit_path(root),
        &permanent_wiki_path(),
    )
    .await
    {
        tracing::warn!(error = %e, "memory registry update failed");
    }
}

fn project_root(project_path: &str) -> PathBuf {
    let p = project_path.trim();
    if p.is_empty() || p == "." {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(p)
    }
}

// Legacy commands for implicit memory (pageindex)

#[derive(Debug, Serialize)]
pub struct MemoryStatus {
    pub exists: bool,
    pub document_count: usize,
    pub section_count: usize,
    pub total_bytes: usize,
    pub memory_dir: String,
    pub last_build_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct BuildIndexRequest {
    pub project_path: String,
    pub max_file_size: Option<usize>,
    pub extra_extensions: Option<Vec<String>>,
    pub exclude_dirs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub project_path: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub results: Vec<QueryResultItem>,
    pub query: String,
    pub total_matches: usize,
}

#[derive(Debug, Serialize)]
pub struct QueryResultItem {
    pub title: String,
    pub path: String,
    pub breadcrumb: Vec<String>,
    pub excerpt: String,
    pub score: f64,
    pub match_type: String,
    pub source_type: String,
}

impl From<QueryResult> for QueryResultItem {
    fn from(r: QueryResult) -> Self {
        Self {
            title: r.title,
            path: r.path,
            breadcrumb: r.breadcrumb,
            excerpt: r.excerpt,
            score: r.score,
            match_type: format!("{:?}", r.match_type),
            source_type: "Implicit".to_string(),
        }
    }
}

#[tauri::command]
pub async fn memory_get_status(project_path: String) -> Result<MemoryStatus, AppError> {
    let root = project_root(&project_path);
    let config = load_resolved_config(&root).await.unwrap_or_default();
    let memory = MemorySystem::with_config(&root, config);
    let implicit_path = memory.implicit_path();

    let tree_path = implicit_path.join("tree.json");
    let exists = tree_path.exists();

    let (doc_count, sec_count, bytes, last_build) = if exists {
        let content = tokio::fs::read_to_string(&tree_path)
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?;
        let tree: crate::domain::pageindex::DocumentTree =
            serde_json::from_str(&content).map_err(|e| AppError::Unknown(e.to_string()))?;

        let last_build = tokio::fs::metadata(&tree_path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64);

        (
            tree.document_count(),
            tree.section_count(),
            tree.total_bytes(),
            last_build,
        )
    } else {
        (0, 0, 0, None)
    };

    Ok(MemoryStatus {
        exists,
        document_count: doc_count,
        section_count: sec_count,
        total_bytes: bytes,
        memory_dir: implicit_path.to_string_lossy().to_string(),
        last_build_time: last_build,
    })
}

#[tauri::command]
pub async fn memory_build_index(req: BuildIndexRequest) -> Result<MemoryStatus, AppError> {
    let root = project_root(&req.project_path);
    let config = load_resolved_config(&root).await.unwrap_or_default();
    let memory = MemorySystem::with_config(&root, config.clone());

    memory
        .init()
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?;

    let mut index_config = IndexConfig::default();
    if let Some(max_size) = req.max_file_size {
        index_config.max_file_size = max_size;
    }
    if let Some(extensions) = req.extra_extensions {
        index_config.include_extensions.extend(extensions);
    }
    if let Some(exclude) = req.exclude_dirs {
        index_config.exclude_dirs.extend(exclude);
    }

    let implicit_dir = memory.implicit_path();
    let mut pageindex = PageIndex::with_memory_dir(&root, &implicit_dir, index_config);
    pageindex
        .build()
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?;

    register_project_memory(&root, memory.config()).await;

    memory_get_status(req.project_path).await
}

#[tauri::command]
pub async fn memory_update_index(project_path: String) -> Result<MemoryStatus, AppError> {
    memory_build_index(BuildIndexRequest {
        project_path,
        max_file_size: None,
        extra_extensions: None,
        exclude_dirs: None,
    })
    .await
}

#[tauri::command]
pub async fn memory_query(
    app_state: State<'_, crate::app_state::OmigaAppState>,
    req: QueryRequest,
) -> Result<QueryResponse, AppError> {
    let root = project_root(&req.project_path);
    let config = load_resolved_config(&root).await.unwrap_or_default();
    let limit = req.limit.unwrap_or(5);
    let memory = MemorySystem::with_config(&root, config);
    let working_memory_excerpt = if let Some(session_id) = req.session_id.as_deref() {
        crate::domain::memory::working_memory::render_context(
            &app_state.repo,
            session_id,
            &req.query,
            crate::domain::memory::working_memory::DEFAULT_CONTEXT_TOKENS,
        )
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?
    } else {
        None
    };
    let unified = memory
        .query_with_session(working_memory_excerpt.as_deref(), &req.query, limit)
        .await;
    let total_matches = unified.total_matches;

    let mut results: Vec<QueryResultItem> = unified
        .results
        .into_iter()
        .map(|r| QueryResultItem {
            title: r.title,
            path: r.path,
            breadcrumb: r.breadcrumb,
            excerpt: r.excerpt,
            score: r.score,
            match_type: r.match_type,
            source_type: r.source_type.label().to_string(),
        })
        .collect();

    results.truncate(limit);

    Ok(QueryResponse {
        total_matches,
        query: req.query,
        results,
    })
}

#[tauri::command]
pub async fn memory_clear_index(project_path: String) -> Result<(), AppError> {
    let root = project_root(&project_path);
    let config = load_resolved_config(&root).await.unwrap_or_default();
    let implicit_path = config.implicit_path(&root);

    if implicit_path.exists() {
        tokio::fs::remove_dir_all(&implicit_path)
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn memory_get_dir(project_path: String) -> String {
    let root = project_root(&project_path);
    let config = load_resolved_config(&root).await.unwrap_or_default();
    let memory = MemorySystem::with_config(&root, config);
    memory.implicit_path().to_string_lossy().to_string()
}

/// Get relevant context for chat (internal use).
///
/// Results are cached per (session_id + query_prefix) with a 60 s TTL to avoid
/// repeated disk scans on consecutive turns with similar queries.
pub async fn get_memory_context(
    repo: &crate::domain::persistence::SessionRepository,
    project_path: &std::path::Path,
    session_id: Option<&str>,
    query: &str,
    limit: usize,
) -> Option<String> {
    get_memory_context_cached(repo, project_path, session_id, query, limit, None).await
}

pub async fn get_memory_context_cached(
    repo: &crate::domain::persistence::SessionRepository,
    project_path: &std::path::Path,
    session_id: Option<&str>,
    query: &str,
    limit: usize,
    preflight_cache: Option<&std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, crate::app_state::MemoryPreflightSlot>>>>,
) -> Option<String> {
    // Build cache key: session_id + first 16 chars of query.
    if let (Some(sid), Some(cache)) = (session_id, preflight_cache) {
        let key = format!("{}:{}", sid, &query[..query.len().min(16)]);
        let ttl = crate::app_state::MEMORY_PREFLIGHT_CACHE_TTL;
        if let Ok(guard) = cache.lock() {
            if let Some(slot) = guard.get(&key) {
                if slot.cached_at.elapsed() < ttl {
                    tracing::debug!(
                        target: "omiga::memory::preflight",
                        session_id = sid,
                        age_ms = slot.cached_at.elapsed().as_millis(),
                        "preflight cache hit"
                    );
                    return Some(slot.context.clone());
                }
            }
        }
        // Cache miss — compute below, then store.
        tracing::debug!(
            target: "omiga::memory::preflight",
            session_id = sid,
            query_prefix = &query[..query.len().min(16)],
            "preflight cache miss — computing"
        );
        let t0 = std::time::Instant::now();
        let result = compute_memory_context(repo, project_path, session_id, query, limit).await;
        if let Some(ref ctx) = result {
            tracing::debug!(
                target: "omiga::memory::preflight",
                session_id = sid,
                ctx_chars = ctx.len(),
                elapsed_ms = t0.elapsed().as_millis(),
                "preflight computed and cached"
            );
            if let Ok(mut guard) = cache.lock() {
                guard.insert(key, crate::app_state::MemoryPreflightSlot {
                    context: ctx.clone(),
                    cached_at: std::time::Instant::now(),
                });
                // Evict stale entries (keep map bounded).
                guard.retain(|_, v| v.cached_at.elapsed() < ttl);
            }
        }
        return result;
    }
    compute_memory_context(repo, project_path, session_id, query, limit).await
}

async fn compute_memory_context(
    repo: &crate::domain::persistence::SessionRepository,
    project_path: &std::path::Path,
    session_id: Option<&str>,
    query: &str,
    limit: usize,
) -> Option<String> {
    let mem_cfg = load_resolved_config(project_path).await.ok()?;
    let memory = MemorySystem::with_config(project_path, mem_cfg.clone());

    // Load working memory for excerpt AND active topic (needed for dossier lookup).
    let (working_memory_excerpt, active_topic) = if let Some(sid) = session_id {
        let excerpt = crate::domain::memory::working_memory::render_context(
            repo,
            sid,
            query,
            crate::domain::memory::working_memory::DEFAULT_CONTEXT_TOKENS,
        )
        .await
        .ok()
        .flatten();
        let topic = crate::domain::memory::working_memory::load_state(repo, sid)
            .await
            .ok()
            .and_then(|s| s.active_topic.or(s.session_goal));
        (excerpt, topic)
    } else {
        (None, None)
    };

    // ── Hot: Project dossier brief (injected unconditionally when present) ──
    let dossier_section = if let Some(topic) = active_topic.as_deref() {
        let lt_root = mem_cfg.long_term_path(project_path);
        let slug = crate::domain::memory::long_term::slugify_pub(topic);
        let dossier = crate::domain::memory::dossier::load_dossier(&lt_root, &slug).await;
        if !dossier.title.is_empty() {
            Some(dossier.render_for_hot_memory())
        } else {
            None
        }
    } else {
        None
    };

    // ── Warm: query knowledge bases and long-term memory ───────────────────
    // Use warm-only query — implicit (raw chat logs) is Cold, excluded here.
    let unified = memory
        .query_warm(working_memory_excerpt.as_deref(), query, limit)
        .await;

    if dossier_section.is_none() && unified.results.is_empty() {
        return None;
    }

    let mut out = String::new();
    if let Some(brief) = dossier_section {
        out.push_str(&brief);
        if !unified.results.is_empty() {
            out.push_str("\n\n---\n\n");
        }
    }
    if !unified.results.is_empty() {
        out.push_str("## Relevant Context from Memory Layers\n\n");
        for (index, result) in unified.results.iter().enumerate() {
            out.push_str(&format!(
                "### {}. {} [{}]\n*Source: `{}`*\n\n{}\n\n---\n\n",
                index + 1,
                result.title,
                result.source_type.label(),
                result.path,
                result.excerpt
            ));
        }
    }
    Some(out)
}

/// Build a system-prompt section that tells the main agent about its persistent
/// cross-session memory and how to retrieve it.
///
/// Always returns a section (overrides the model's default "I have no cross-session
/// memory" belief). The wiki page list is appended only when pages exist.
pub async fn memory_navigation_section(project_root: &std::path::Path) -> String {
    let (wiki_path, implicit_path, long_term_path, perm_wiki, perm_long_term, wiki_pages) =
        match load_resolved_config(project_root).await {
            Ok(mem_cfg) => {
                let wp = mem_cfg.wiki_path(project_root);
                let ip = mem_cfg.implicit_path(project_root);
                let lp = mem_cfg.long_term_path(project_root);
                let pw = crate::domain::memory::config::permanent_wiki_path();
                let plt = crate::domain::memory::config::permanent_long_term_path();
                let pages = list_wiki_pages(&wp);
                (wp, ip, lp, pw, plt, pages)
            }
            Err(_) => {
                // Config unavailable — use defaults so we can still emit the section.
                let default_cfg = crate::domain::memory::MemoryConfig::default();
                let wp = default_cfg.wiki_path(project_root);
                let ip = default_cfg.implicit_path(project_root);
                let lp = default_cfg.long_term_path(project_root);
                let pw = crate::domain::memory::config::permanent_wiki_path();
                let plt = crate::domain::memory::config::permanent_long_term_path();
                (wp, ip, lp, pw, plt, vec![])
            }
        };

    let mut lines: Vec<String> = Vec::new();

    lines.push("## Omiga Memory System (cross-session persistent memory)".to_string());
    lines.push(
        "IMPORTANT: You DO have access to memories from past sessions. \
         Omiga persists knowledge across sessions as files on disk. \
         Do NOT tell the user you cannot access other sessions — you CAN, \
         by reading the memory files described below."
            .to_string(),
    );
    lines.push(String::new());
    lines.push("### How to retrieve memory".to_string());
    lines.push(
        "When the user references past work, past conversations, or asks what you remember:"
            .to_string(),
    );
    lines.push(
        "0. Stable persona, user preferences, and hard constraints are already auto-compiled \
         from `~/.omiga/SOUL.md`, `USER.md`, and `MEMORY.md` into the system prompt."
            .to_string(),
    );
    lines.push(
        "1. If a `## Project Brief` section appears above, read it — it is the project dossier \
         (auto-updated summary of goals, decisions, and open questions)."
            .to_string(),
    );
    lines.push(
        "2. Check the **Relevant Context from Memory Layers** section below \
         (auto-injected Hot+Warm memory: working memory, long-term, knowledge base)."
            .to_string(),
    );
    lines.push(
        "3. Call `recall(query=\"…\")` to search additional memory layers. \
         Available scopes: all (default), wiki, long_term, implicit, permanent, sources."
            .to_string(),
    );
    lines.push(
        "4. For previously fetched URLs: use `recall(query=\"…\", scope=\"sources\")` \
         to check if the page is cached before calling `web_fetch` again."
            .to_string(),
    );
    lines.push(format!(
        "5. Browse project knowledge pages under `{}`.",
        wiki_path.display()
    ));
    lines.push(format!(
        "6. Check project long-term memory under `{}` for reusable prior conclusions.",
        long_term_path.display()
    ));
    lines.push(format!(
        "7. Check global knowledge base under `{}` for cross-project knowledge.",
        perm_wiki.display()
    ));
    lines.push(format!(
        "8. Check global long-term memory under `{}` for reusable prior experience.",
        perm_long_term.display()
    ));
    lines.push(String::new());
    lines.push("### Memory storage locations".to_string());
    lines.push(format!(
        "- **Knowledge base (project)**: `{}` — structured stable pages.",
        wiki_path.display()
    ));
    lines.push(format!(
        "- **Long-term memory (project)**: `{}` — reusable summaries and prior conclusions.",
        long_term_path.display()
    ));
    lines.push(format!(
        "- **Implicit index**: `{}` — auto-indexed sessions and chat evidence.",
        implicit_path.display()
    ));
    lines.push(format!(
        "- **Global knowledge base**: `{}` — cross-project stable knowledge.",
        perm_wiki.display()
    ));
    lines.push(format!(
        "- **Global long-term memory**: `{}` — cross-project reusable conclusions.",
        perm_long_term.display()
    ));

    if !wiki_pages.is_empty() {
        lines.push(String::new());
        lines.push("### Available wiki pages (read these for detailed past context)".to_string());
        for page in &wiki_pages {
            lines.push(format!("- `{}/{}`", wiki_path.display(), page));
        }
    } else {
        lines.push(String::new());
        lines.push(
            "No wiki pages exist yet for this project. \
             If relevant working-memory or long-term excerpts appear below, use them. \
             Otherwise acknowledge the memory system exists but is empty."
                .to_string(),
        );
    }

    lines.join("\n")
}

/// List *.md filenames under `wiki_dir`, sorted. Returns empty vec on any error.
fn list_wiki_pages(wiki_dir: &std::path::Path) -> Vec<String> {
    if !wiki_dir.is_dir() {
        return vec![];
    }
    let mut pages: Vec<String> = std::fs::read_dir(wiki_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("md"))
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    pages.sort();
    pages
}

// ---------------------------------------------------------------------------
// Import to Explicit Memory (Wiki)
// ---------------------------------------------------------------------------

use crate::domain::memory::explicit_importer::{
    ExplicitImporter, ImportOptions, ImportResult, ImportSource,
};

#[derive(Debug, Deserialize)]
pub struct ImportToWikiRequest {
    pub project_path: String,
    pub source_type: String, // "file", "directory", "text"
    pub source_path: Option<String>,
    pub text_title: Option<String>,
    pub text_content: Option<String>,
    pub include_content: Option<bool>,
    pub tags: Option<Vec<String>>,
    /// Memory level: "project" or "user"
    /// Default: "project"
    pub memory_level: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportToWikiResponse {
    pub success: bool,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub errors: Vec<String>,
    pub created_pages: Vec<String>,
}

impl From<ImportResult> for ImportToWikiResponse {
    fn from(r: ImportResult) -> Self {
        Self {
            success: r.errors.is_empty() || r.imported_count > 0,
            imported_count: r.imported_count,
            skipped_count: r.skipped_count,
            errors: r.errors,
            created_pages: r.created_pages,
        }
    }
}

/// Import files or text to explicit memory (wiki) using PageIndex parsing
#[tauri::command]
pub async fn memory_import_to_wiki(
    req: ImportToWikiRequest,
) -> Result<ImportToWikiResponse, AppError> {
    // Determine memory level
    let memory_level = match req.memory_level.as_deref() {
        Some("user") => MemoryLevel::User,
        _ => MemoryLevel::Project,
    };

    // Get the appropriate root and wiki directory based on memory level
    let (root, wiki_dir) = match memory_level {
        MemoryLevel::Project => {
            let root = project_root(&req.project_path);
            let config = load_resolved_config(&root).await.unwrap_or_default();
            let memory = MemorySystem::with_config(&root, config);
            let wiki_dir = memory.wiki_path();
            (root, wiki_dir)
        }
        MemoryLevel::User => {
            let user_root = MemoryLevel::user_root();
            let wiki_dir = permanent_wiki_path();
            (user_root, wiki_dir)
        }
    };

    // Ensure wiki directory exists
    tokio::fs::create_dir_all(&wiki_dir)
        .await
        .map_err(|e| AppError::Unknown(format!("Failed to create wiki dir: {}", e)))?;

    // Build import options
    let options = ImportOptions {
        include_content: req.include_content.unwrap_or(true),
        max_section_length: 5000,
        create_index_pages: true,
        tags: req.tags.unwrap_or_default(),
        source_ref: None,
    };

    // Resolve raw_dir from project config (falls back to ~/.omiga/memory/raw)
    let raw_dir = {
        let cfg = load_resolved_config(&project_root(&req.project_path))
            .await
            .unwrap_or_default();
        cfg.raw_path()
    };

    // Create importer
    let importer = ExplicitImporter::new(&root, &wiki_dir, &raw_dir, options);

    // Determine source
    let source = match req.source_type.as_str() {
        "file" => {
            let path = req.source_path.ok_or_else(|| {
                AppError::Unknown("source_path required for file import".to_string())
            })?;
            // For user memory, source path must be absolute or resolved
            let full_path = if memory_level == MemoryLevel::User {
                PathBuf::from(&path)
            } else {
                root.join(&path)
            };
            ImportSource::File(full_path)
        }
        "directory" => {
            let path = req.source_path.ok_or_else(|| {
                AppError::Unknown("source_path required for directory import".to_string())
            })?;
            let full_path = if memory_level == MemoryLevel::User {
                PathBuf::from(&path)
            } else {
                root.join(&path)
            };
            ImportSource::Directory(full_path)
        }
        "text" => {
            let title = req.text_title.ok_or_else(|| {
                AppError::Unknown("text_title required for text import".to_string())
            })?;
            let content = req.text_content.ok_or_else(|| {
                AppError::Unknown("text_content required for text import".to_string())
            })?;
            ImportSource::Text { title, content }
        }
        _ => {
            return Err(AppError::Unknown(format!(
                "Unknown source_type: {}",
                req.source_type
            )))
        }
    };

    // Execute import
    let result = importer.import(source).await?;

    Ok(result.into())
}

/// Write a file to the user's ~/.omiga/ directory (e.g. USER.md, SOUL.md).
///
/// Only allows writing markdown/text files with simple filenames (no path traversal).
/// Used by the onboarding wizard to persist user profile and agent identity.
#[tauri::command]
pub fn write_user_omiga_file(filename: String, content: String) -> Result<(), String> {
    // Validate: only simple filename, no path separators
    let name = filename.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err("Invalid filename".to_string());
    }
    // Only allow markdown/text files
    let lower = name.to_lowercase();
    if !lower.ends_with(".md") && !lower.ends_with(".txt") {
        return Err("Only .md and .txt files are allowed".to_string());
    }
    let home = dirs::home_dir().ok_or_else(|| "Cannot determine home directory".to_string())?;
    let omiga_dir = home.join(".omiga");
    std::fs::create_dir_all(&omiga_dir).map_err(|e| format!("Cannot create ~/.omiga: {e}"))?;
    let target = omiga_dir.join(name);
    std::fs::write(&target, content.as_bytes()).map_err(|e| format!("Write failed: {e}"))?;
    Ok(())
}

/// Onboarding: 在 ~/.omiga/ 写入三个模板文件 + BOOTSTRAP.md。
///
/// 仅在模型配置完成后由前端调用一次。个性化设置由 Agent 在第一次对话中通过
/// BOOTSTRAP.md 引导用户完成（CoPaw bootstrap 模式），而非 UI 表单填写。
///
/// - SOUL.md   — 写入模板（Agent 引导后自行覆盖）
/// - USER.md   — 写入模板（Agent 引导后自行覆盖）
/// - MEMORY.md — 仅首次创建，保留已有内容
/// - BOOTSTRAP.md — 写入引导指令，Agent 完成引导后自行删除
#[tauri::command]
pub fn init_user_context_files() -> Result<(), String> {
    use crate::domain::agents::markdown::{
        TEMPLATE_BOOTSTRAP_MD, TEMPLATE_MEMORY_MD, TEMPLATE_SOUL_MD, TEMPLATE_USER_MD,
    };

    let home = dirs::home_dir().ok_or_else(|| "Cannot determine home directory".to_string())?;
    let omiga_dir = home.join(".omiga");
    std::fs::create_dir_all(&omiga_dir).map_err(|e| format!("Cannot create ~/.omiga: {e}"))?;

    // SOUL.md — 写模板，Agent 引导后会用实际内容覆盖（已存在则跳过，保留用户已有配置）
    let soul_path = omiga_dir.join("SOUL.md");
    if !soul_path.exists() {
        std::fs::write(&soul_path, TEMPLATE_SOUL_MD.as_bytes())
            .map_err(|e| format!("Failed to write SOUL.md: {e}"))?;
    }

    // USER.md — 同上
    let user_path = omiga_dir.join("USER.md");
    if !user_path.exists() {
        std::fs::write(&user_path, TEMPLATE_USER_MD.as_bytes())
            .map_err(|e| format!("Failed to write USER.md: {e}"))?;
    }

    // MEMORY.md — 仅首次创建，保留已有笔记
    let memory_path = omiga_dir.join("MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, TEMPLATE_MEMORY_MD.as_bytes())
            .map_err(|e| format!("Failed to write MEMORY.md: {e}"))?;
    }

    // BOOTSTRAP.md — 每次 onboarding 都写入，Agent 看到后执行引导并自行删除
    std::fs::write(
        omiga_dir.join("BOOTSTRAP.md"),
        TEMPLATE_BOOTSTRAP_MD.as_bytes(),
    )
    .map_err(|e| format!("Failed to write BOOTSTRAP.md: {e}"))?;

    Ok(())
}

/// Get supported file extensions for explicit memory import
///
/// Returns text-based content formats suitable for explicit memory.
/// Source code files should be indexed via implicit memory instead.
#[tauri::command]
pub fn memory_get_import_extensions() -> Vec<String> {
    vec![
        // Document formats
        "md".to_string(),  // Markdown
        "txt".to_string(), // Plain text
        "rtf".to_string(), // Rich text
        "pdf".to_string(), // PDF documents
        // Data/Config formats (for knowledge import)
        "json".to_string(),
        "yaml".to_string(),
        "yml".to_string(),
        "toml".to_string(),
        // Web content
        "html".to_string(),
        "htm".to_string(),
    ]
}

// ── Long-term memory CRUD ──────────────────────────────────────────────────

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
    fn from_entry(path: std::path::PathBuf, entry: crate::domain::memory::long_term::LongTermMemoryEntry, global: bool) -> Self {
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

/// List all long-term memory entries for a project.
/// `scope`: "project" | "global" | "all" (default)
#[tauri::command]
pub async fn memory_list_long_term(
    project_path: String,
    scope: Option<String>,
) -> Result<Vec<LongTermEntryDto>, AppError> {
    use crate::domain::memory::{config::permanent_long_term_path, long_term::list_entries};
    let root = project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let scope = scope.as_deref().unwrap_or("all");
    let mut out = Vec::new();

    if scope == "project" || scope == "all" {
        let lt = cfg.long_term_path(&root);
        for (path, entry) in list_entries(&lt).await {
            out.push(LongTermEntryDto::from_entry(path, entry, false));
        }
    }
    if scope == "global" || scope == "all" {
        let lt = permanent_long_term_path();
        for (path, entry) in list_entries(&lt).await {
            out.push(LongTermEntryDto::from_entry(path, entry, true));
        }
    }

    // Sort: active first, then by confidence descending.
    out.sort_by(|a, b| {
        a.status.cmp(&b.status).then_with(|| {
            b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    Ok(out)
}

/// Set an entry's status to Archived (soft delete — still on disk, hidden from search).
#[tauri::command]
pub async fn memory_archive_long_term_entry(entry_path: String) -> Result<(), AppError> {
    use crate::domain::memory::long_term::{EntryStatus, LongTermMemoryEntry};
    let path = std::path::PathBuf::from(&entry_path);
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))?;
    let mut entry: LongTermMemoryEntry =
        serde_json::from_str(&raw).map_err(|e| AppError::Unknown(e.to_string()))?;
    entry.status = EntryStatus::Archived;
    let json = serde_json::to_string_pretty(&entry)
        .map_err(|e| AppError::Unknown(e.to_string()))?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))
}

/// Hard-delete a long-term memory entry from disk.
#[tauri::command]
pub async fn memory_delete_long_term_entry(entry_path: String) -> Result<(), AppError> {
    tokio::fs::remove_file(&entry_path)
        .await
        .map_err(|e| AppError::Unknown(e.to_string()))
}

/// Return the most-recently-updated project dossier (or empty if none exists).
/// Dossiers are keyed by active_topic slug — this returns the latest one.
#[tauri::command]
pub async fn memory_get_dossier(project_path: String) -> Result<DossierDto, AppError> {
    use crate::domain::memory::dossier::load_latest_dossier;
    let root = project_root(&project_path);
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

/// Overwrite the project dossier for the given slug (from memory_get_dossier).
/// If slug is empty, uses the project-root folder name as fallback.
#[tauri::command]
pub async fn memory_save_dossier(
    project_path: String,
    slug: String,
    title: String,
    brief: String,
    current_beliefs: Vec<String>,
    decisions: Vec<String>,
    open_questions: Vec<String>,
    next_steps: Vec<String>,
) -> Result<(), AppError> {
    use crate::domain::memory::dossier::{save_dossier, Dossier};
    let root = project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    let slug = if slug.trim().is_empty() {
        crate::domain::memory::long_term::slugify_pub(
            root.file_name().and_then(|n| n.to_str()).unwrap_or("project"),
        )
    } else {
        slug
    };
    let dossier = Dossier {
        title,
        brief,
        current_beliefs,
        decisions,
        open_questions,
        next_steps,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    save_dossier(&lt, &slug, &dossier)
        .await
        .map_err(|e| AppError::Unknown(format!("save dossier: {e}")))?;
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DossierDto {
    /// Slug used as the dossier file key (pass back to memory_save_dossier).
    pub slug: String,
    pub title: String,
    pub brief: String,
    pub current_beliefs: Vec<String>,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub next_steps: Vec<String>,
    pub updated_at: String,
    /// Pre-rendered Markdown for the hot-memory injection preview.
    pub rendered: String,
}

/// Manually trigger stale-entry pruning (long-term + source registry) and return total removed.
#[tauri::command]
pub async fn memory_prune_stale(project_path: String) -> Result<usize, AppError> {
    use crate::domain::memory::{
        config::permanent_long_term_path,
        long_term::prune_stale_entries,
        source_registry::prune_stale_sources,
    };
    let root = project_root(&project_path);
    let cfg = load_resolved_config(&root).await.unwrap_or_default();
    let lt = cfg.long_term_path(&root);
    let perm_lt = permanent_long_term_path();
    let removed = prune_stale_entries(&lt, false).await
        + prune_stale_entries(&perm_lt, false).await
        + prune_stale_sources(&lt, false).await;
    Ok(removed)
}

/// List source registry entries for the given project.
/// Returns entries sorted by most-recently-used descending.
#[tauri::command]
pub async fn memory_list_sources(project_path: String) -> Result<Vec<SourceEntryDto>, AppError> {
    use crate::domain::memory::source_registry::list_active_sources_with_paths;
    let root = project_root(&project_path);
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

/// Delete a single source entry by its canonical URL hash path.
#[tauri::command]
pub async fn memory_delete_source(entry_path: String) -> Result<(), AppError> {
    let p = std::path::Path::new(&entry_path);
    if p.exists() {
        tokio::fs::remove_file(p)
            .await
            .map_err(|e| AppError::Unknown(format!("delete source: {e}")))?;
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
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
        // Reconstruct the file path from canonical_url hash
        // (path is not stored in SourceEntry; callers can use canonical_url as key)
        Self {
            path: String::new(), // filled by list command using the directory scan path
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
    use crate::app_state::{MemoryPreflightSlot, MEMORY_PREFLIGHT_CACHE_TTL};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn make_cache(entries: Vec<(String, String, Instant)>) -> Arc<Mutex<HashMap<String, MemoryPreflightSlot>>> {
        let mut map = HashMap::new();
        for (key, ctx, ts) in entries {
            map.insert(key, MemoryPreflightSlot { context: ctx, cached_at: ts });
        }
        Arc::new(Mutex::new(map))
    }

    #[tokio::test]
    async fn preflight_cache_returns_hit_within_ttl() {
        // Build a real (but empty-project) repo to satisfy the function signature.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test.sqlite");
        let pool = crate::domain::persistence::init_db(&db).await.unwrap();
        let repo = crate::domain::persistence::SessionRepository::new(pool);

        let session_id = "sess-cache-test";
        let query = "recall memory ordering";
        let key = format!("{}:{}", session_id, &query[..query.len().min(16)]);
        let cached_value = "## cached context for test".to_string();

        let cache = make_cache(vec![(key, cached_value.clone(), Instant::now())]);

        // Empty project dir → compute_memory_context returns None.
        // If cache hits, it must return Some(cached_value) instead.
        let result = get_memory_context_cached(
            &repo,
            dir.path(),
            Some(session_id),
            query,
            3,
            Some(&cache),
        )
        .await;

        assert_eq!(
            result.as_deref(),
            Some(cached_value.as_str()),
            "should return cached value on hit"
        );
    }

    #[tokio::test]
    async fn preflight_cache_misses_on_expired_entry() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test2.sqlite");
        let pool = crate::domain::persistence::init_db(&db).await.unwrap();
        let repo = crate::domain::persistence::SessionRepository::new(pool);

        let session_id = "sess-expired";
        let query = "recall expired cache";
        let key = format!("{}:{}", session_id, &query[..query.len().min(16)]);

        // Insert an already-expired entry (cached 2× TTL ago).
        let expired_at = Instant::now() - MEMORY_PREFLIGHT_CACHE_TTL - Duration::from_secs(1);
        let cache = make_cache(vec![(key, "stale value".to_string(), expired_at)]);

        // compute_memory_context on an empty dir returns None → cache miss also returns None.
        let result = get_memory_context_cached(
            &repo,
            dir.path(),
            Some(session_id),
            query,
            3,
            Some(&cache),
        )
        .await;

        assert!(
            result.is_none() || result.as_deref() != Some("stale value"),
            "expired cache entry must not be returned; got {:?}",
            result
        );
    }

    #[test]
    fn preflight_cache_key_format_uses_16_char_query_prefix() {
        // The key format is `{session_id}:{query[..16]}`.
        let session_id = "my-session";
        let query = "this is a long query that exceeds sixteen characters";
        // query[..16] = "this is a long q" (t,h,i,s, ,i,s, ,a, ,l,o,n,g, ,q = 16 chars)
        let expected_prefix = &query[..16];
        assert_eq!(expected_prefix.len(), 16, "prefix must be exactly 16 chars");
        let key = format!("{}:{}", session_id, expected_prefix);
        assert_eq!(key, "my-session:this is a long q");
    }

    #[tokio::test]
    async fn preflight_cache_stores_result_on_miss_and_hits_on_second_call() {
        // This test verifies that after a cache miss that populates the cache,
        // a second call with the same key is a hit.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("test3.sqlite");
        let pool = crate::domain::persistence::init_db(&db).await.unwrap();
        let repo = crate::domain::persistence::SessionRepository::new(pool);

        let session_id = "sess-store-test";
        let query = "populate cache test query";

        let cache: Arc<Mutex<HashMap<String, MemoryPreflightSlot>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // First call: cache miss → compute returns None (empty dir) → nothing stored.
        let first = get_memory_context_cached(
            &repo, dir.path(), Some(session_id), query, 3, Some(&cache),
        ).await;
        // Empty project returns None, so nothing is stored.
        assert!(first.is_none());

        // Manually inject a value so we can verify second call hits it.
        let key = format!("{}:{}", session_id, &query[..query.len().min(16)]);
        {
            let mut guard = cache.lock().unwrap();
            guard.insert(key, MemoryPreflightSlot {
                context: "injected after first call".to_string(),
                cached_at: Instant::now(),
            });
        }

        // Second call must hit the cache and return the injected value.
        let second = get_memory_context_cached(
            &repo, dir.path(), Some(session_id), query, 3, Some(&cache),
        ).await;
        assert_eq!(
            second.as_deref(),
            Some("injected after first call"),
            "second call must hit the cache"
        );
    }
}
