//! Unified Memory System
//!
//! Provides a unified interface for all types of persistent knowledge:
//! - Explicit Memory (Wiki): User-curated knowledge
//! - Implicit Memory (PageIndex): Auto-indexed project files
//!
//! ## Directory Structure
//!
//! Default ([`crate::domain::memory::config::MemoryMode::UserHome`]):
//!
//! ```text
//! ~/.omiga/memory/
//! ├── registry.json           # Index of all projects → memory paths
//! ├── permanent/wiki/         # User-level permanent explicit memory
//! └── projects/<id>/          # Per-project wiki + implicit (id = hash of project path)
//! ```
//!
//! Project-local `config.json` always lives at `<project>/.omiga/memory/config.json`.

pub mod chat_indexer;
pub mod config;
pub mod dossier;
pub mod long_term;
pub mod migration;
pub mod permanent_profile;
pub mod registry;
pub mod source_registry;
pub mod working_memory;

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};
use walkdir::WalkDir;

pub use chat_indexer::{ChatIndexer, ChatMessage, ChatRole};
pub use config::{
    permanent_long_term_path, permanent_wiki_path, project_storage_key, user_omiga_root,
    MemoryConfig, MemoryMode,
};

/// Unified memory system handle
pub struct MemorySystem {
    project_root: PathBuf,
    config: MemoryConfig,
}

impl MemorySystem {
    /// Create a new memory system instance
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let config = MemoryConfig::default();

        Self {
            project_root,
            config,
        }
    }

    /// Create with custom config
    pub fn with_config(project_root: impl AsRef<Path>, config: MemoryConfig) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            config,
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Get memory root path
    pub fn root_path(&self) -> PathBuf {
        self.config.effective_root(&self.project_root)
    }

    /// Get wiki path
    pub fn wiki_path(&self) -> PathBuf {
        self.config.wiki_path(&self.project_root)
    }

    /// Get implicit index path
    pub fn implicit_path(&self) -> PathBuf {
        self.config.implicit_path(&self.project_root)
    }

    /// Get project-scoped long-term memory path
    pub fn long_term_path(&self) -> PathBuf {
        self.config.long_term_path(&self.project_root)
    }

    /// Get project-scoped source registry directory
    pub fn sources_path(&self) -> PathBuf {
        source_registry::sources_dir(&self.long_term_path())
    }

    /// Initialize memory directory structure
    pub async fn init(&self) -> std::io::Result<()> {
        let root = self.root_path();
        fs::create_dir_all(&root).await?;
        fs::create_dir_all(self.wiki_path()).await?;
        fs::create_dir_all(self.implicit_path()).await?;
        fs::create_dir_all(self.implicit_path().join("content")).await?;
        fs::create_dir_all(self.long_term_path()).await?;
        fs::create_dir_all(config::permanent_wiki_path()).await?;
        fs::create_dir_all(config::permanent_long_term_path()).await?;
        if let Some(parent) = registry::registry_file_path().parent() {
            fs::create_dir_all(parent).await?;
        }
        let _ = migration::backfill_wiki_metadata(&self.wiki_path()).await;
        let _ = migration::backfill_wiki_metadata(&config::permanent_wiki_path()).await;

        // Probabilistic cleanup: prune stale long-term and source entries ~10% of the time on init.
        if rand_one_in_n(10) {
            let lt = self.long_term_path();
            let perm_lt = config::permanent_long_term_path();
            let removed_project = long_term::prune_stale_entries(&lt, false).await;
            let removed_global = long_term::prune_stale_entries(&perm_lt, false).await;
            if removed_project + removed_global > 0 {
                info!(
                    "Pruned {} stale long-term entries ({} project, {} global)",
                    removed_project + removed_global,
                    removed_project,
                    removed_global
                );
            }
            let removed_sources = source_registry::prune_stale_sources(&lt, false).await;
            if removed_sources > 0 {
                info!("Pruned {} expired source registry entries", removed_sources);
            }
        }

        info!("Initialized memory system at {:?}", root);
        Ok(())
    }

    /// Load configuration from disk
    pub async fn load_config(project_root: impl AsRef<Path>) -> Option<MemoryConfig> {
        let config_path = MemoryConfig::project_config_path(&project_root);
        if !config_path.exists() {
            return None;
        }

        match fs::read_to_string(&config_path).await {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(config) => Some(config),
                Err(e) => {
                    warn!("Failed to parse memory config: {}", e);
                    None
                }
            },
            Err(e) => {
                warn!("Failed to read memory config: {}", e);
                None
            }
        }
    }

    /// Save configuration to disk
    pub async fn save_config(&self) -> Result<(), crate::errors::AppError> {
        self.config.validate()?;

        let config_path = MemoryConfig::project_config_path(&self.project_root);
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;
        }
        let content = serde_json::to_string_pretty(&self.config)
            .map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;

        fs::write(&config_path, content)
            .await
            .map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;

        info!("Saved memory config to {:?}", config_path);
        Ok(())
    }

    /// Get unified memory statistics
    pub async fn stats(&self) -> MemoryStats {
        let lt_project_path = self.long_term_path();
        let lt_global_path = config::permanent_long_term_path();
        let stale_project = long_term::count_stale_entries(&lt_project_path).await;
        let stale_global = long_term::count_stale_entries(&lt_global_path).await;
        let src_count = source_registry::count_sources(&lt_project_path).await;
        let stale_src = source_registry::count_stale_sources(&lt_project_path).await;
        let mut stats = MemoryStats {
            project_knowledge_pages: count_markdown_pages(&self.wiki_path()).await,
            global_knowledge_pages: count_markdown_pages(&config::permanent_wiki_path()).await,
            long_term_project_entries: long_term::count_entries(&lt_project_path).await,
            long_term_global_entries: long_term::count_entries(&lt_global_path).await,
            stale_long_term_entries: stale_project + stale_global,
            source_registry_count: src_count,
            stale_source_count: stale_src,
            ..Default::default()
        };

        // Check explicit memory (wiki): project + permanent
        for wiki_root in [self.wiki_path(), config::permanent_wiki_path()] {
            if wiki_root.exists() {
                if let Ok(mut entries) = fs::read_dir(&wiki_root).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.ends_with(".md")
                            && name_str != "index.md"
                            && name_str != "log.md"
                        {
                            stats.explicit_documents += 1;
                        }
                    }
                }
            }
        }

        // Check implicit memory
        let implicit_path = self.implicit_path();
        if implicit_path.join("tree.json").exists() {
            stats.implicit_exists = true;
        }

        stats
    }

    /// Query both explicit and implicit memory.
    ///
    /// - **Explicit (Wiki)**：扫描 wiki 目录下所有 `.md` 文件，按关键词匹配返回标题 + 摘要。
    /// - **Implicit (PageIndex)**：从磁盘加载 `tree.json`，用 `QueryEngine` 做 TF-IDF 检索。
    pub async fn query(&self, query: &str, limit: usize) -> UnifiedQueryResult {
        self.query_with_session(None, query, limit).await
    }

    /// Hot + Warm memory only — excludes Implicit (Cold / raw chat logs).
    ///
    /// Use this for preflight auto-injection. Cold memory is noisy and large;
    /// it should only surface through explicit `recall` tool calls.
    pub async fn query_warm(
        &self,
        working_memory_excerpt: Option<&str>,
        query: &str,
        limit: usize,
    ) -> UnifiedQueryResult {
        let limit = limit.max(1);
        // Phase 1: wider candidate pool — Phase 2 rerank then selects top `limit`.
        let phase1_limit = (limit * 3).max(12);
        let mut results = Vec::new();

        if let Some(excerpt) = working_memory_excerpt {
            results.extend(query_working_memory(excerpt, query));
        }

        let lt_query = enrich_query_with_working_memory(query, working_memory_excerpt);

        results.extend(
            long_term::search_entries(&self.long_term_path(), &lt_query, phase1_limit, false)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "summary".to_string(),
                    source_type: MemorySourceType::LongTermProject,
                }),
        );
        results.extend(
            long_term::search_entries(&config::permanent_long_term_path(), &lt_query, phase1_limit, true)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "summary".to_string(),
                    source_type: MemorySourceType::LongTermGlobal,
                }),
        );
        results.extend(
            search_markdown_wiki(&self.wiki_path(), query, phase1_limit)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "Wiki".to_string(),
                    source_type: MemorySourceType::KnowledgeBaseProject,
                }),
        );
        results.extend(
            search_markdown_wiki(&config::permanent_wiki_path(), query, phase1_limit)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "Wiki".to_string(),
                    source_type: MemorySourceType::KnowledgeBaseGlobal,
                }),
        );

        // Implicit (Cold) intentionally excluded — available only via explicit `recall` calls.

        sort_memory_results(&mut results);
        dedupe_matches(&mut results);
        let phase1_count = results.len();
        two_phase_rerank(&mut results, working_memory_excerpt);
        let total_matches = results.len();
        results.truncate(limit);

        tracing::debug!(
            target: "omiga::memory::recall",
            query = %query,
            phase1_candidates = phase1_count,
            returned = results.len(),
            rerank = working_memory_excerpt.is_some(),
            "query_warm completed"
        );

        UnifiedQueryResult {
            query: query.to_string(),
            results,
            total_matches,
        }
    }

    pub async fn query_with_session(
        &self,
        working_memory_excerpt: Option<&str>,
        query: &str,
        limit: usize,
    ) -> UnifiedQueryResult {
        let limit = limit.max(1);
        // Phase 1: wider candidate pool for Phase 2 reranking.
        let phase1_limit = (limit * 3).max(12);
        let mut results = Vec::new();

        if let Some(excerpt) = working_memory_excerpt {
            results.extend(query_working_memory(excerpt, query));
        }

        // Augment long-term query with active topic from working memory for context boost.
        let lt_query = enrich_query_with_working_memory(query, working_memory_excerpt);

        results.extend(
            long_term::search_entries(&self.long_term_path(), &lt_query, phase1_limit, false)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "summary".to_string(),
                    source_type: MemorySourceType::LongTermProject,
                }),
        );
        results.extend(
            long_term::search_entries(&config::permanent_long_term_path(), &lt_query, phase1_limit, true)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "summary".to_string(),
                    source_type: MemorySourceType::LongTermGlobal,
                }),
        );

        results.extend(
            search_markdown_wiki(&self.wiki_path(), query, phase1_limit)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "Wiki".to_string(),
                    source_type: MemorySourceType::KnowledgeBaseProject,
                }),
        );
        results.extend(
            search_markdown_wiki(&config::permanent_wiki_path(), query, phase1_limit)
                .await
                .into_iter()
                .map(|result| MemoryQueryMatch {
                    title: result.title,
                    path: result.path,
                    breadcrumb: vec![],
                    excerpt: result.excerpt,
                    score: result.score,
                    match_type: "Wiki".to_string(),
                    source_type: MemorySourceType::KnowledgeBaseGlobal,
                }),
        );

        results.extend(query_implicit_matches(&self.implicit_path(), query, limit).await);

        sort_memory_results(&mut results);
        dedupe_matches(&mut results);
        two_phase_rerank(&mut results, working_memory_excerpt);
        let total_matches = results.len();
        results.truncate(limit);

        UnifiedQueryResult {
            query: query.to_string(),
            results,
            total_matches,
        }
    }
}

/// Memory statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub explicit_documents: usize,
    pub implicit_exists: bool,
    pub implicit_documents: usize,
    pub implicit_sections: usize,
    pub project_knowledge_pages: usize,
    pub global_knowledge_pages: usize,
    pub long_term_project_entries: usize,
    pub long_term_global_entries: usize,
    /// Long-term entries not reused in >90 days with stability < 0.4.
    pub stale_long_term_entries: usize,
    /// Number of active (non-expired) web sources tracked in the source registry.
    pub source_registry_count: usize,
    /// Number of expired source entries not yet pruned.
    pub stale_source_count: usize,
}

/// Unified query result
#[derive(Debug, Clone)]
pub struct UnifiedQueryResult {
    pub query: String,
    pub results: Vec<MemoryQueryMatch>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySourceType {
    WorkingMemory,
    LongTermProject,
    LongTermGlobal,
    KnowledgeBaseProject,
    KnowledgeBaseGlobal,
    Implicit,
}

impl MemorySourceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::WorkingMemory => "WorkingMemory",
            Self::LongTermProject => "LongTermProject",
            Self::LongTermGlobal => "LongTermGlobal",
            Self::KnowledgeBaseProject => "KnowledgeBaseProject",
            Self::KnowledgeBaseGlobal => "KnowledgeBaseGlobal",
            Self::Implicit => "Implicit",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::WorkingMemory => 6,
            Self::LongTermProject => 5,
            Self::LongTermGlobal => 4,
            Self::KnowledgeBaseProject => 3,
            Self::KnowledgeBaseGlobal => 2,
            Self::Implicit => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryQueryMatch {
    pub title: String,
    pub path: String,
    pub breadcrumb: Vec<String>,
    pub excerpt: String,
    pub score: f64,
    pub match_type: String,
    pub source_type: MemorySourceType,
}

#[derive(Debug, Clone)]
pub struct ExplicitResult {
    pub title: String,
    pub path: String,
    pub excerpt: String,
    pub score: f64,
}

/// Update `~/.omiga/memory/registry.json` for this project (paths on disk).
pub async fn touch_project_registry(project_root: &Path) {
    let Ok(cfg) = load_resolved_config(project_root).await else {
        return;
    };
    if let Err(e) = registry::upsert_project_paths(
        project_root,
        &cfg.effective_root(project_root),
        &cfg.wiki_path(project_root),
        &cfg.implicit_path(project_root),
        &permanent_wiki_path(),
    )
    .await
    {
        tracing::warn!(error = %e, "memory registry update failed");
    }
}

/// If config says [`MemoryMode::UserHome`] but project still has data only under `<project>/.omiga/memory/`
/// and nothing under `~/.omiga/memory/projects/<id>/`, keep using project-relative storage.
fn finalize_config_for_existing_data(mut c: MemoryConfig, project_root: &Path) -> MemoryConfig {
    if c.memory_mode != MemoryMode::UserHome {
        return c;
    }
    let key = config::project_storage_key(project_root);
    let user_base = config::user_omiga_root()
        .join("memory")
        .join("projects")
        .join(&key);
    let user_wiki = user_base.join(&c.wiki_dir);
    let user_implicit_tree = user_base.join(&c.implicit_dir).join("tree.json");
    let has_user_data = user_wiki.exists() || user_implicit_tree.exists();

    let legacy_wiki = project_root.join(".omiga/memory/wiki");
    let legacy_implicit = project_root.join(".omiga/memory/implicit/tree.json");
    let legacy_tree = project_root.join(".omiga/memory/tree.json");
    let has_legacy_only =
        (legacy_wiki.exists() || legacy_implicit.exists() || legacy_tree.exists())
            && !has_user_data;

    if has_legacy_only {
        c.memory_mode = MemoryMode::ProjectRelative;
        c.root_dir = PathBuf::from(".omiga/memory");
    }
    c
}

/// Load config with migration, legacy layout detection, and [`MemoryMode::UserHome`] default for new projects.
pub async fn load_resolved_config(
    project_root: &Path,
) -> Result<MemoryConfig, crate::errors::AppError> {
    migration::migrate_if_needed(project_root).await?;
    if let Some(c) = MemorySystem::load_config(project_root).await {
        return Ok(finalize_config_for_existing_data(c, project_root));
    }
    let legacy = project_root.join(".omiga/memory");
    let has_legacy = legacy.join("wiki").exists()
        || legacy.join("implicit").join("tree.json").exists()
        || legacy.join("tree.json").exists();
    if has_legacy {
        return Ok(MemoryConfig {
            memory_mode: MemoryMode::ProjectRelative,
            root_dir: PathBuf::from(".omiga/memory"),
            ..Default::default()
        });
    }
    Ok(MemoryConfig::default())
}

/// Initialize memory system with migration
pub async fn init_memory_system(
    project_root: impl AsRef<Path>,
) -> Result<MemorySystem, crate::errors::AppError> {
    let project_root = project_root.as_ref();
    let config = load_resolved_config(project_root).await?;
    let memory = MemorySystem::with_config(project_root, config);
    memory
        .init()
        .await
        .map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;
    Ok(memory)
}

/// 在文本中找到最密集的关键词窗口，返回摘要（带省略号）。
fn extract_excerpt(content: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let content_lower = content.to_lowercase();
    let query_terms = crate::domain::pageindex::derive_query_terms(query);

    // 优先定位整句命中，否则回退到查询词命中。
    let anchor = content_lower
        .find(&query_lower)
        .or_else(|| {
            query_terms
                .iter()
                .filter_map(|term| content_lower.find(term))
                .min()
        })
        .unwrap_or(0);

    // 窗口从锚点前 60 字符开始
    let start = anchor.saturating_sub(60);
    // 对齐到行首（避免从行中间截断）
    let start = content[..start].rfind('\n').map(|p| p + 1).unwrap_or(start);

    let end = (start + max_len).min(content.len());
    // 对齐到行尾
    let end = content[end..].find('\n').map(|p| end + p).unwrap_or(end);

    let slice = content[start..end].trim().to_string();

    match (start > 0, end < content.len()) {
        (true, true) => format!("…{}…", slice),
        (true, false) => format!("…{}", slice),
        (false, true) => format!("{}…", slice),
        _ => slice,
    }
}

async fn count_markdown_pages(root: &Path) -> usize {
    markdown_files(root).len()
}

pub async fn search_markdown_wiki(root: &Path, query: &str, limit: usize) -> Vec<ExplicitResult> {
    if limit == 0 || !root.is_dir() {
        return vec![];
    }

    let query_terms = crate::domain::pageindex::derive_query_terms(query);
    if query_terms.is_empty() {
        return vec![];
    }

    let query_lower = query.trim().to_lowercase();
    let mut results = Vec::new();

    for path in markdown_files(root) {
        let Ok(content) = fs::read_to_string(&path).await else {
            continue;
        };
        let title = markdown_title(&path, &content);
        let score = explicit_match_score(&title, &content, &query_lower, &query_terms);
        if score <= 0.0 {
            continue;
        }

        results.push(ExplicitResult {
            title,
            path: path.to_string_lossy().to_string(),
            excerpt: extract_excerpt(&content, query, 300),
            score,
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    results.truncate(limit);
    results
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .map(|entry| entry.into_path())
        .collect()
}

fn markdown_title(path: &Path, content: &str) -> String {
    content
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches('#').trim().to_string())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled")
                .to_string()
        })
}

fn explicit_match_score(
    title: &str,
    content: &str,
    query_lower: &str,
    query_terms: &[String],
) -> f64 {
    let mut score = 0.0;
    let title_lower = title.to_lowercase();
    let content_lower = content.to_lowercase();

    if !query_lower.is_empty() {
        if title_lower.contains(query_lower) {
            score += 0.45;
        }
        if content_lower.contains(query_lower) {
            score += 0.2;
        }
    }

    score += crate::domain::pageindex::score_terms_against_text(&title_lower, query_terms) * 1.4;
    score += crate::domain::pageindex::score_terms_against_text(&content_lower, query_terms);

    score.min(2.0)
}

fn query_working_memory(excerpt: &str, query: &str) -> Vec<MemoryQueryMatch> {
    let query_terms = crate::domain::pageindex::derive_query_terms(query);
    if query_terms.is_empty() || excerpt.trim().is_empty() {
        return vec![];
    }
    let score = crate::domain::pageindex::score_terms_against_text(excerpt, &query_terms);
    if score <= 0.0 {
        return vec![];
    }
    vec![MemoryQueryMatch {
        title: "Session scratchpad".to_string(),
        path: "session_working_memory".to_string(),
        breadcrumb: vec![],
        excerpt: extract_excerpt(excerpt, query, 320),
        score,
        match_type: "summary".to_string(),
        source_type: MemorySourceType::WorkingMemory,
    }]
}

async fn query_implicit_matches(
    implicit_path: &Path,
    query: &str,
    limit: usize,
) -> Vec<MemoryQueryMatch> {
    let storage = crate::domain::pageindex::IndexStorage::new(implicit_path);
    match storage.load_tree().await {
        Ok(Some(tree)) => {
            let engine = crate::domain::pageindex::QueryEngine::new();
            match engine.search(&tree, query, limit).await {
                Ok(results) => results
                    .into_iter()
                    .map(|result| MemoryQueryMatch {
                        title: result.title,
                        path: result.path,
                        breadcrumb: result.breadcrumb,
                        excerpt: result.excerpt,
                        score: result.score,
                        match_type: format!("{:?}", result.match_type),
                        source_type: MemorySourceType::Implicit,
                    })
                    .collect(),
                Err(e) => {
                    warn!("PageIndex search error: {}", e);
                    vec![]
                }
            }
        }
        Ok(None) => vec![],
        Err(e) => {
            warn!("Failed to load PageIndex tree: {}", e);
            vec![]
        }
    }
}

fn dedupe_matches(results: &mut Vec<MemoryQueryMatch>) {
    let mut deduped = Vec::new();
    for item in results.drain(..) {
        if deduped.iter().any(|existing: &MemoryQueryMatch| {
            existing.path == item.path && existing.source_type == item.source_type
        }) {
            continue;
        }
        deduped.push(item);
    }
    *results = deduped;
}

/// Phase-2 rerank: boost results whose content overlaps with the current session context
/// (working memory excerpt), blending 70% original score + 30% context overlap.
fn two_phase_rerank(results: &mut [MemoryQueryMatch], working_memory_excerpt: Option<&str>) {
    let Some(excerpt) = working_memory_excerpt else { return };
    let ctx_terms = crate::domain::pageindex::derive_query_terms(excerpt);
    if ctx_terms.is_empty() {
        return;
    }
    // Snapshot top-1 title before rerank to detect ranking changes.
    let top_before = results.first().map(|r| r.title.clone());

    for result in results.iter_mut() {
        let ctx_score = crate::domain::pageindex::score_terms_against_text(
            &format!("{} {}", result.title, result.excerpt),
            &ctx_terms,
        );
        result.score = result.score * 0.70 + ctx_score * 0.30;
    }
    sort_memory_results(results);

    let top_after = results.first().map(|r| r.title.as_str());
    if top_before.as_deref() != top_after {
        tracing::debug!(
            target: "omiga::memory::recall",
            before = top_before.as_deref().unwrap_or("(none)"),
            after = top_after.unwrap_or("(none)"),
            ctx_terms = ctx_terms.len(),
            candidates = results.len(),
            "two-phase rerank changed top result"
        );
    }
}

/// Returns true with probability 1/n using the current timestamp as a cheap entropy source.
fn rand_one_in_n(n: u64) -> bool {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % n == 0)
        .unwrap_or(false)
}

/// Enrich a long-term search query with signals from the current working memory excerpt.
///
/// Extracts `### Session Goal` and `### Active Topic` lines from the rendered excerpt and
/// appends them to the query so that long-term entries matching the current session context
/// receive a higher TF-IDF score.
fn enrich_query_with_working_memory(query: &str, excerpt: Option<&str>) -> String {
    let Some(excerpt) = excerpt else {
        return query.to_string();
    };

    let mut extra_terms = Vec::new();
    for line in excerpt.lines() {
        let trimmed = line.trim();
        // Capture the value that follows "- " under Session Goal / Active Topic sections.
        if trimmed.starts_with("- ") {
            extra_terms.push(trimmed.trim_start_matches("- ").trim().to_string());
            if extra_terms.len() >= 2 {
                break;
            }
        }
    }

    if extra_terms.is_empty() {
        return query.to_string();
    }

    // Cap total enriched query to ~300 chars to avoid diluting the original signal.
    let suffix = extra_terms.join(" ");
    let combined = format!("{} {}", query.trim(), suffix.trim());
    if combined.chars().count() > 300 {
        combined.chars().take(300).collect()
    } else {
        combined
    }
}

pub fn sort_memory_results(results: &mut [MemoryQueryMatch]) {
    results.sort_by(|a, b| {
        b.source_type
            .rank()
            .cmp(&a.source_type.rank())
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.title.cmp(&b.title))
    });
}

// Explicit memory importer
pub mod explicit_importer;

/// System prompt for memory-agent sub-agent
/// Injected by `run_subagent_session` when subagent_type is memory-agent/wiki-agent
pub fn memory_agent_system_prompt_with_config(
    project_root: &std::path::Path,
    config: &MemoryConfig,
) -> String {
    let wiki_pb = config.wiki_path(project_root);
    let wiki_path_str = wiki_pb.to_string_lossy();
    let perm_pb = config::permanent_wiki_path();
    let perm_str = perm_pb.to_string_lossy();
    let long_term_pb = config.long_term_path(project_root);
    let long_term_str = long_term_pb.to_string_lossy();
    let perm_long_term_pb = config::permanent_long_term_path();
    let perm_long_term_str = perm_long_term_pb.to_string_lossy();
    let imp_pb = config.implicit_path(project_root);
    let imp_str = imp_pb.to_string_lossy();
    format!(
        "## Memory Agent Mode\n\
         You are a specialized memory agent for the Omiga project memory system.\n\
         \n\
         Memory structure:\n\
         - `{}` — Project knowledge base (Wiki)\n\
         - `{}` — Project long-term memory\n\
         - `{}` — Global knowledge base (user-level Wiki, applies across projects)\n\
         - `{}` — Global long-term memory\n\
         - `{}` — Implicit memory: Auto-indexed chats / documents\n\
         \n\
         Your responsibilities:\n\
         - **Ingest**: When given source material (articles, code, docs), extract key information \
           and create/update wiki pages. Write summaries, entity pages, concept \
           pages, and maintain cross-references. Log each operation.\n\
         - **Query**: Search memory for relevant information and synthesize a concise answer \
           with citations. Results may become new wiki pages.\n\
         - **Lint**: Audit memory health — check for contradictions, stale claims, orphaned pages, \
           missing cross-references. Suggest new investigations.\n\
         \n\
         Always check existing content first. Prefer editing existing \
         pages over creating duplicates. Keep page excerpts under 800 lines. Return a concise \
         summary of what was done.",
        wiki_path_str, long_term_str, perm_str, perm_long_term_str, imp_str
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_markdown_wiki_matches_cjk_natural_language_queries() {
        let temp = tempfile::tempdir().unwrap();
        let page = temp.path().join("redox.md");
        tokio::fs::write(
            &page,
            "# 氧化还原节律\n\n氧化还原节律与生物钟调控、NRF2 和谷胱甘肽有关。",
        )
        .await
        .unwrap();

        let results =
            search_markdown_wiki(temp.path(), "获取全局记忆中与氧化还原节律相关内容", 5).await;

        assert!(!results.is_empty());
        assert_eq!(results[0].title, "氧化还原节律");
    }

    #[tokio::test]
    async fn search_markdown_wiki_reads_nested_pages() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("topics");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        tokio::fs::write(
            nested.join("circadian.md"),
            "# circadian redox\n\nRedox rhythm couples ROS buffering with circadian control.",
        )
        .await
        .unwrap();

        let results = search_markdown_wiki(temp.path(), "redox circadian rhythm", 5).await;

        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("circadian.md"));
    }

    #[test]
    fn sort_memory_results_prioritizes_memory_layers_before_raw_score() {
        let mut results = vec![
            MemoryQueryMatch {
                title: "Implicit hit".to_string(),
                path: "chat/1.md".to_string(),
                breadcrumb: vec![],
                excerpt: "implicit".to_string(),
                score: 0.95,
                match_type: "Content".to_string(),
                source_type: MemorySourceType::Implicit,
            },
            MemoryQueryMatch {
                title: "Working hit".to_string(),
                path: "session_working_memory".to_string(),
                breadcrumb: vec![],
                excerpt: "working".to_string(),
                score: 0.40,
                match_type: "summary".to_string(),
                source_type: MemorySourceType::WorkingMemory,
            },
            MemoryQueryMatch {
                title: "Knowledge hit".to_string(),
                path: "wiki/page.md".to_string(),
                breadcrumb: vec![],
                excerpt: "wiki".to_string(),
                score: 0.90,
                match_type: "Wiki".to_string(),
                source_type: MemorySourceType::KnowledgeBaseProject,
            },
        ];

        sort_memory_results(&mut results);

        assert_eq!(results[0].title, "Working hit");
        assert_eq!(results[1].title, "Knowledge hit");
        assert_eq!(results[2].title, "Implicit hit");
    }

    #[test]
    fn enrich_query_appends_working_memory_topic() {
        let excerpt = "## Working Memory (session scratchpad)\n\n\
                       ### Session Goal\n- 优化记忆分层检索效率\n\n\
                       ### Active Topic\n- memory recall ranking\n";
        let enriched = enrich_query_with_working_memory("recall", Some(excerpt));
        assert!(
            enriched.contains("recall"),
            "original query must be preserved"
        );
        assert!(
            enriched.len() > "recall".len(),
            "enriched query should be longer than original"
        );
    }

    #[test]
    fn enrich_query_returns_original_when_no_excerpt() {
        let result = enrich_query_with_working_memory("my query", None);
        assert_eq!(result, "my query");
    }

    #[test]
    fn enrich_query_caps_at_300_chars() {
        let long_line = "x".repeat(400);
        let excerpt = format!("### Session Goal\n- {}\n", long_line);
        let result = enrich_query_with_working_memory("q", Some(&excerpt));
        assert!(
            result.chars().count() <= 300,
            "enriched query must not exceed 300 chars"
        );
    }

    #[test]
    fn two_phase_rerank_noop_without_excerpt() {
        // two_phase_rerank with None excerpt is an early return — no reordering at all.
        let mut results = vec![
            MemoryQueryMatch {
                title: "first-inserted".to_string(),
                path: "a.md".to_string(),
                breadcrumb: vec![],
                excerpt: "rust memory recall".to_string(),
                score: 0.8,
                match_type: "summary".to_string(),
                source_type: MemorySourceType::LongTermProject,
            },
            MemoryQueryMatch {
                title: "second-inserted".to_string(),
                path: "b.md".to_string(),
                breadcrumb: vec![],
                excerpt: "python scripting".to_string(),
                score: 0.9,
                match_type: "summary".to_string(),
                source_type: MemorySourceType::LongTermProject,
            },
        ];
        two_phase_rerank(&mut results, None);
        // No excerpt → early return, insertion order preserved.
        assert_eq!(
            results[0].title, "first-inserted",
            "no excerpt must leave order unchanged"
        );
        assert_eq!(results[1].title, "second-inserted");
    }

    #[test]
    fn two_phase_rerank_boosts_context_matching_entry() {
        // Entry A: generic, slightly higher raw score.
        // Entry B: title+excerpt exactly match session context terms, slightly lower raw score.
        // The 30% context boost must be enough to overtake the small raw gap.
        let mut results = vec![
            MemoryQueryMatch {
                title: "networking protocols".to_string(),
                path: "generic.md".to_string(),
                breadcrumb: vec![],
                excerpt: "unrelated topic about TCP UDP networking protocols".to_string(),
                score: 0.55,
                match_type: "summary".to_string(),
                source_type: MemorySourceType::LongTermProject,
            },
            MemoryQueryMatch {
                title: "memory TF-IDF recall".to_string(),
                path: "relevant.md".to_string(),
                breadcrumb: vec![],
                excerpt: "memory recall optimisation TF-IDF ranking strategy".to_string(),
                score: 0.51,
                match_type: "summary".to_string(),
                source_type: MemorySourceType::LongTermProject,
            },
        ];
        // Context: strong signal about memory/recall/TF-IDF — matches "memory TF-IDF recall" entry.
        let session_context = "### Session Goal\n- memory recall optimisation TF-IDF ranking\n\
                               ### Active Topic\n- memory TF-IDF ranking strategy\n";
        two_phase_rerank(&mut results, Some(session_context));
        assert_eq!(
            results[0].title, "memory TF-IDF recall",
            "context-matching entry must rank first after Phase 2 rerank; got: {:?}",
            results.iter().map(|r| (&r.title, r.score)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn two_phase_rerank_blends_scores_70_30() {
        // Verify the blending formula: new_score = original * 0.7 + ctx * 0.3
        // Entry with known original score and zero context overlap → score * 0.7.
        let original_score = 1.0_f64;
        let mut results = vec![MemoryQueryMatch {
            title: "no-ctx-match".to_string(),
            path: "x.md".to_string(),
            breadcrumb: vec![],
            excerpt: "qzzqzzqzz unique gibberish".to_string(),
            score: original_score,
            match_type: "summary".to_string(),
            source_type: MemorySourceType::LongTermProject,
        }];
        // Context is completely unrelated so ctx_score ≈ 0.
        let unrelated_ctx = "### Session Goal\n- aaaabbbbcccc unrelated\n";
        two_phase_rerank(&mut results, Some(unrelated_ctx));
        // Score should have decreased (0.7 * 1.0 + 0.3 * ~0 < 1.0).
        assert!(
            results[0].score < original_score,
            "score must decrease when context is unrelated; got {}",
            results[0].score
        );
    }
}
