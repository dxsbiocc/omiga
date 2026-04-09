//! Unified Memory System
//!
//! Provides a unified interface for all types of persistent knowledge:
//! - Explicit Memory (Wiki): User-curated knowledge
//! - Implicit Memory (PageIndex): Auto-indexed project files
//!
//! ## Directory Structure
//!
//! ```text
//! .omiga/memory/              # Unified memory root (configurable)
//! ├── wiki/                   # Explicit memory
//! ├── implicit/              # Implicit memory (auto-indexed)
//! └── config.json            # Memory configuration
//! ```

pub mod chat_indexer;
pub mod config;
pub mod migration;

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};

pub use chat_indexer::{ChatIndexer, ChatMessage, ChatRole};
pub use config::MemoryConfig;

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

    /// Initialize memory directory structure
    pub async fn init(&self) -> std::io::Result<()> {
        let root = self.root_path();
        fs::create_dir_all(&root).await?;
        fs::create_dir_all(self.wiki_path()).await?;
        fs::create_dir_all(self.implicit_path()).await?;
        fs::create_dir_all(self.implicit_path().join("content")).await?;
        
        info!("Initialized memory system at {:?}", root);
        Ok(())
    }

    /// Load configuration from disk
    pub async fn load_config(project_root: impl AsRef<Path>) -> Option<MemoryConfig> {
        let config_path = MemoryConfig::default().config_path(&project_root);
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
        
        let config_path = self.config.config_path(&self.project_root);
        let content = serde_json::to_string_pretty(&self.config)
            .map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;
        
        fs::write(&config_path, content).await
            .map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;
        
        info!("Saved memory config to {:?}", config_path);
        Ok(())
    }

    /// Get unified memory statistics
    pub async fn stats(&self) -> MemoryStats {
        let mut stats = MemoryStats::default();
        
        // Check explicit memory (wiki)
        let wiki_path = self.wiki_path();
        if wiki_path.exists() {
            if let Ok(mut entries) = fs::read_dir(&wiki_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.ends_with(".md") && name_str != "index.md" && name_str != "log.md" {
                        stats.explicit_documents += 1;
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
        let limit = limit.max(1);
        let mut explicit_results = Vec::new();
        let mut implicit_results = Vec::new();

        // ── Explicit: 搜索 wiki ────────────────────────────────────────────
        let wiki_path = self.wiki_path();
        if wiki_path.exists() {
            if let Ok(mut entries) = fs::read_dir(&wiki_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if ext != "md" {
                        continue;
                    }
                    let Ok(content) = fs::read_to_string(&path).await else {
                        continue;
                    };
                    if !content.to_lowercase().contains(&query.to_lowercase()) {
                        continue;
                    }

                    // 提取标题（第一个 # 行 或文件名）
                    let title = content
                        .lines()
                        .find(|l| l.starts_with("# "))
                        .map(|l| l.trim_start_matches('#').trim().to_string())
                        .unwrap_or_else(|| {
                            path.file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("Untitled")
                                .to_string()
                        });

                    let excerpt = extract_excerpt(&content, query, 300);
                    explicit_results.push(ExplicitResult {
                        title,
                        path: path.to_string_lossy().to_string(),
                        excerpt,
                    });

                    if explicit_results.len() >= limit {
                        break;
                    }
                }
            }
        }

        // ── Implicit: 搜索 PageIndex ──────────────────────────────────────
        let implicit_path = self.implicit_path();
        let storage = crate::domain::pageindex::IndexStorage::new(&implicit_path);
        match storage.load_tree().await {
            Ok(Some(tree)) => {
                let engine = crate::domain::pageindex::QueryEngine::new();
                match engine.search(&tree, query, limit).await {
                    Ok(results) => {
                        for r in results {
                            implicit_results.push(ImplicitResult {
                                title: r.title,
                                path: r.path,
                                breadcrumb: r.breadcrumb,
                                excerpt: r.excerpt,
                                score: r.score,
                            });
                        }
                    }
                    Err(e) => {
                        warn!("PageIndex search error: {}", e);
                    }
                }
            }
            Ok(None) => {} // 尚未建立索引
            Err(e) => {
                warn!("Failed to load PageIndex tree: {}", e);
            }
        }

        let total_matches = explicit_results.len() + implicit_results.len();
        UnifiedQueryResult {
            query: query.to_string(),
            explicit_results,
            implicit_results,
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
}

/// Unified query result
#[derive(Debug, Clone)]
pub struct UnifiedQueryResult {
    pub query: String,
    pub explicit_results: Vec<ExplicitResult>,
    pub implicit_results: Vec<ImplicitResult>,
    pub total_matches: usize,
}

#[derive(Debug, Clone)]
pub struct ExplicitResult {
    pub title: String,
    pub path: String,
    pub excerpt: String,
}

#[derive(Debug, Clone)]
pub struct ImplicitResult {
    pub title: String,
    pub path: String,
    pub breadcrumb: Vec<String>,
    pub excerpt: String,
    pub score: f64,
}

/// Initialize memory system with migration
pub async fn init_memory_system(project_root: impl AsRef<Path>) -> Result<MemorySystem, crate::errors::AppError> {
    let project_root = project_root.as_ref();
    
    // Try to load existing config
    let config = if let Some(config) = MemorySystem::load_config(project_root).await {
        config
    } else {
        // Check if migration is needed
        migration::migrate_if_needed(project_root).await?;
        MemoryConfig::default()
    };
    
    let memory = MemorySystem::with_config(project_root, config);
    memory.init().await.map_err(|e| crate::errors::AppError::Unknown(e.to_string()))?;
    
    Ok(memory)
}

/// 在文本中找到最密集的关键词窗口，返回摘要（带省略号）。
fn extract_excerpt(content: &str, query: &str, max_len: usize) -> String {
    let query_lower = query.to_lowercase();
    let content_lower = content.to_lowercase();

    // 找到第一个匹配位置作为锚点
    let anchor = content_lower.find(&query_lower).unwrap_or(0);

    // 窗口从锚点前 60 字符开始
    let start = anchor.saturating_sub(60);
    // 对齐到行首（避免从行中间截断）
    let start = content[..start]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(start);

    let end = (start + max_len).min(content.len());
    // 对齐到行尾
    let end = content[end..]
        .find('\n')
        .map(|p| end + p)
        .unwrap_or(end);

    let slice = content[start..end].trim().to_string();

    match (start > 0, end < content.len()) {
        (true, true)  => format!("…{}…", slice),
        (true, false) => format!("…{}", slice),
        (false, true) => format!("{}…", slice),
        _             => slice,
    }
}

// Explicit memory importer
pub mod explicit_importer;

/// System prompt for memory-agent sub-agent
/// Injected by `run_subagent_session` when subagent_type is memory-agent/wiki-agent
pub fn memory_agent_system_prompt(project_root: &std::path::Path) -> String {
    use crate::domain::memory::config::MemoryConfig;
    let config = MemoryConfig::default();
    let wiki_path = config.wiki_path(project_root);
    let wiki_path_str = wiki_path.to_string_lossy();
    
    format!(
        "## Memory Agent Mode\n\
         You are a specialized memory agent for the Omiga project memory system.\n\
         \n\
         Memory structure:\n\
         - `{}` — Explicit memory (Wiki): User-curated knowledge\n\
         - `{}` — Implicit memory: Auto-indexed project files\n\
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
        wiki_path_str,
        config.implicit_path(project_root).to_string_lossy()
    )
}
