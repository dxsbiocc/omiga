//! Memory commands — Unified interface for the Memory system.
//!
//! Provides both explicit (wiki) and implicit (pageindex) memory management
//! with configurable paths.

use crate::domain::memory::{
    config::{MemoryConfig, MemoryMode},
    load_resolved_config,
    migration::{detect_structure_version, MemoryStructureVersion},
    permanent_wiki_path,
    registry,
    MemorySystem,
};
use crate::domain::pageindex::{IndexConfig, IndexStorage, PageIndex, QueryEngine, QueryResult};
use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
            MemoryLevel::User => {
                permanent_wiki_path()
            }
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
    pub memory_mode: String,
    pub auto_build_index: bool,
    pub index_extensions: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub max_file_size: usize,
}

impl From<MemoryConfig> for MemoryConfigResponse {
    fn from(c: MemoryConfig) -> Self {
        Self {
            root_dir: c.root_dir.to_string_lossy().to_string(),
            wiki_dir: c.wiki_dir,
            implicit_dir: c.implicit_dir,
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
pub struct MemoryPaths {
    pub root: String,
    pub wiki: String,
    pub implicit: String,
    /// User-level permanent wiki (`~/.omiga/memory/permanent/wiki`)
    pub permanent_wiki: String,
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
pub async fn memory_set_config(req: SetMemoryConfigRequest) -> Result<MemoryConfigResponse, AppError> {
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
    
    config.validate()?;
    
    let memory = MemorySystem::with_config(&root, config);
    memory.save_config().await?;
    memory.init().await.map_err(|e| AppError::Unknown(e.to_string()))?;
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
pub async fn memory_get_unified_status(project_path: String) -> Result<UnifiedMemoryStatus, AppError> {
    let root = project_root(&project_path);
    let version = detect_structure_version(&root).await;
    
    let config = load_resolved_config(&root).await.unwrap_or_default();
    let memory = MemorySystem::with_config(&root, config.clone());
    let stats = memory.stats().await;
    
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
        paths: MemoryPaths {
            root: memory.root_path().to_string_lossy().to_string(),
            wiki: memory.wiki_path().to_string_lossy().to_string(),
            implicit: memory.implicit_path().to_string_lossy().to_string(),
            permanent_wiki: permanent_wiki_path().to_string_lossy().to_string(),
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
    let content = tokio::fs::read_to_string(&tree_path).await
        .map_err(|e| AppError::Unknown(e.to_string()))?;
    
    let tree: crate::domain::pageindex::DocumentTree = serde_json::from_str(&content)
        .map_err(|e| AppError::Unknown(e.to_string()))?;
    
    // Get modification time
    let last_build = tokio::fs::metadata(&tree_path).await
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
            let content = tokio::fs::read_to_string(&tree_path).await
                .map_err(|e| AppError::Unknown(e.to_string()))?;
            let tree: crate::domain::pageindex::DocumentTree = serde_json::from_str(&content)
                .map_err(|e| AppError::Unknown(e.to_string()))?;
            
            let last_build = tokio::fs::metadata(&tree_path).await
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64);
            
            (tree.document_count(), tree.section_count(), tree.total_bytes(), last_build)
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
        
        memory.init().await.map_err(|e| AppError::Unknown(e.to_string()))?;
        
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
        pageindex.build().await.map_err(|e| AppError::Unknown(e.to_string()))?;
        
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
        }).await
    }

    #[tauri::command]
    pub async fn memory_query(req: QueryRequest) -> Result<QueryResponse, AppError> {
        let root = project_root(&req.project_path);
        let config = load_resolved_config(&root).await.unwrap_or_default();
        let implicit_path = config.implicit_path(&root);
        let tree_path = implicit_path.join("tree.json");
        
        if !tree_path.exists() {
            return Err(AppError::Unknown("Memory index not found".to_string()));
        }
        
        let limit = req.limit.unwrap_or(5);
        let storage = IndexStorage::new(&implicit_path);
        let tree = match storage.load_tree().await {
            Ok(Some(t)) => t,
            Ok(None) => {
                return Err(AppError::Unknown("Memory index not found".to_string()));
            }
            Err(e) => return Err(AppError::Unknown(e.to_string())),
        };
        let engine = QueryEngine::new();
        let results = engine
            .search(&tree, &req.query, limit)
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?;
        
        Ok(QueryResponse {
            total_matches: results.len(),
            query: req.query,
            results: results.into_iter().map(Into::into).collect(),
        })
    }

    #[tauri::command]
    pub async fn memory_clear_index(project_path: String) -> Result<(), AppError> {
        let root = project_root(&project_path);
        let config = load_resolved_config(&root).await.unwrap_or_default();
        let implicit_path = config.implicit_path(&root);
        
        if implicit_path.exists() {
            tokio::fs::remove_dir_all(&implicit_path).await
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

/// Get relevant context for chat (internal use)
pub async fn get_memory_context(
    project_path: &std::path::Path,
    query: &str,
    limit: usize,
) -> Option<String> {
    let mem_cfg = load_resolved_config(project_path).await.ok()?;
    let implicit = mem_cfg.implicit_path(project_path);
    let index_config = IndexConfig::default();
    let mut pageindex = PageIndex::with_memory_dir(project_path, &implicit, index_config);

    // load_tree returns Ok(None) when no index exists — no pre-check needed.
    match pageindex.load_tree().await {
        Ok(Some(tree)) => {
            *pageindex.tree_mut() = tree;
        }
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(error = %e, "failed to load memory index tree; skipping context injection");
            return None;
        }
    }

    let results = pageindex.query(query, limit).await.ok()?;
    if results.is_empty() {
        return None;
    }

    Some(crate::domain::pageindex::QueryEngine::new().format_results_as_context(&results))
}

// ---------------------------------------------------------------------------
// Import to Explicit Memory (Wiki)
// ---------------------------------------------------------------------------

use crate::domain::memory::explicit_importer::{ExplicitImporter, ImportOptions, ImportResult, ImportSource};

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
pub async fn memory_import_to_wiki(req: ImportToWikiRequest) -> Result<ImportToWikiResponse, AppError> {
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
    tokio::fs::create_dir_all(&wiki_dir).await
        .map_err(|e| AppError::Unknown(format!("Failed to create wiki dir: {}", e)))?;
    
    // Build import options
    let options = ImportOptions {
        include_content: req.include_content.unwrap_or(true),
        max_section_length: 5000,
        create_index_pages: true,
        tags: req.tags.unwrap_or_default(),
        source_ref: None,
    };
    
    // Create importer
    let importer = ExplicitImporter::new(&root, &wiki_dir, options);
    
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
        _ => return Err(AppError::Unknown(
            format!("Unknown source_type: {}", req.source_type)
        )),
    };
    
    // Execute import
    let result = importer.import(source).await?;
    
    Ok(result.into())
}

/// Get supported file extensions for explicit memory import
/// 
/// Returns text-based content formats suitable for explicit memory.
/// Source code files should be indexed via implicit memory instead.
#[tauri::command]
pub fn memory_get_import_extensions() -> Vec<String> {
    vec![
        // Document formats
        "md".to_string(),    // Markdown
        "txt".to_string(),   // Plain text
        "rtf".to_string(),   // Rich text
        "pdf".to_string(),   // PDF documents
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
