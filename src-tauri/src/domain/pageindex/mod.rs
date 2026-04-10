//! PageIndex — Hierarchical document indexing for semantic memory retrieval.
//!
//! Inspired by rusty-pageindex, this module implements a high-performance,
//! vectorless RAG system that transforms documents into hierarchical
//! "Table-of-Contents" trees for reasoning-based retrieval.
//!
//! ## Architecture
//!
//! ```text
//! Project Root
//! └── .omiga/memory/
//!     ├── index.json          # Unified tree index (all documents)
//!     ├── cache/              # Content hash cache for incremental updates
//!     │   └── {file_hash}.json
//!     └── content/            # Processed content storage
//!         └── {doc_id}.md
//! ```
//!
//! ## Key Features
//!
//! - **Hierarchical Tree**: Folder → File → Section structure preserves document organization
//! - **Incremental Updates**: Hash-based caching skips unchanged files
//! - **Multi-format Support**: Markdown, text files, PDF (future)
//! - **Context Retrieval**: Smart search with auto-unwrap for LLM context
//!
//! ## Usage
//!
//! ```rust
//! use crate::domain::pageindex::{PageIndex, IndexConfig};
//!
//! let config = IndexConfig::default();
//! let mut index = PageIndex::new(project_root, config);
//!
//! // Index the project
//! index.build().await?;
//!
//! // Query for relevant context
//! let results = index.query("how does authentication work", 3).await?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

mod parser;
mod query;
mod storage;
mod tree;

pub use parser::{DocumentParser, ParseResult};
pub use query::{MatchType, QueryEngine, QueryResult};
pub use storage::{CacheEntry, IndexStorage};
pub use tree::{DocumentNode, DocumentTree, NodeType, SectionNode};

/// Configuration for the PageIndex system.
/// 
/// NOTE: PageIndex now indexes CHAT CONTENT (implicit memory) rather than project code.
/// Project code should be accessed via tools (ripgrep, glob, file_read) in real-time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Maximum content size to index (in bytes). Default: 10MB
    pub max_file_size: usize,
    /// File extensions to index (for external documents in wiki). 
    /// Default: ["md", "txt"] - focused on document formats
    pub include_extensions: Vec<String>,
    /// Directories to exclude. Default: ["node_modules", ".git", "target", ".omiga"]
    pub exclude_dirs: Vec<String>,
    /// Maximum depth for section tree. Default: 6
    pub max_section_depth: usize,
    /// Enable content hashing for incremental updates. Default: true
    pub enable_cache: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB
            // Focus on document formats for explicit knowledge base
            include_extensions: vec![
                "md".to_string(),
                "txt".to_string(),
                "pdf".to_string(),
                "html".to_string(),
                "htm".to_string(),
            ],
            exclude_dirs: vec![
                "node_modules".to_string(),
                ".git".to_string(),
                "target".to_string(),
                ".omiga".to_string(),
                "dist".to_string(),
                "build".to_string(),
                ".next".to_string(),
            ],
            max_section_depth: 6,
            enable_cache: true,
        }
    }
}

/// The main PageIndex structure.
pub struct PageIndex {
    project_root: PathBuf,
    memory_dir: PathBuf,
    config: IndexConfig,
    storage: IndexStorage,
    tree: DocumentTree,
    query_engine: QueryEngine,
}

impl PageIndex {
    /// Create a new PageIndex instance with default storage at `.omiga/memory/implicit/`
    /// (unified memory layout; matches [`crate::domain::memory::MemoryConfig`] defaults).
    pub fn new(project_root: impl AsRef<Path>, config: IndexConfig) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let memory_dir = project_root.join(".omiga").join("memory").join("implicit");
        Self::with_memory_dir(project_root, memory_dir, config)
    }

    /// Create PageIndex with an explicit storage directory (e.g. configured `implicit_path`).
    pub fn with_memory_dir(
        project_root: impl AsRef<Path>,
        memory_dir: impl AsRef<Path>,
        config: IndexConfig,
    ) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let memory_dir = memory_dir.as_ref().to_path_buf();
        let storage = IndexStorage::new(&memory_dir);
        let tree = DocumentTree::new();
        let query_engine = QueryEngine::new();

        Self {
            project_root,
            memory_dir,
            config,
            storage,
            tree,
            query_engine,
        }
    }

    /// Get the memory directory path.
    pub fn memory_dir(&self) -> &Path {
        &self.memory_dir
    }

    /// Get the storage instance.
    pub fn storage(&self) -> &IndexStorage {
        &self.storage
    }

    /// Load the tree from disk.
    pub async fn load_tree(&self) -> Result<Option<DocumentTree>, AppError> {
        self.storage.load_tree().await
    }

    /// Mutable access to the in-memory tree (allows callers to inject a loaded tree).
    pub fn tree_mut(&mut self) -> &mut DocumentTree {
        &mut self.tree
    }

    /// Get statistics using a provided tree.
    pub fn stats_with_tree(&self, tree: &DocumentTree) -> IndexStats {
        IndexStats {
            total_documents: tree.document_count(),
            total_sections: tree.section_count(),
            total_bytes: tree.total_bytes(),
        }
    }

    /// Initialize the memory directory structure.
    pub async fn init(&self) -> std::io::Result<()> {
        tokio::fs::create_dir_all(&self.memory_dir).await?;
        tokio::fs::create_dir_all(self.memory_dir.join("cache")).await?;
        tokio::fs::create_dir_all(self.memory_dir.join("content")).await?;
        Ok(())
    }

    /// Build or rebuild the full index.
    pub async fn build(&mut self) -> Result<(), AppError> {
        info!("Starting PageIndex build for {:?}", self.project_root);
        self.init().await.map_err(|e| AppError::Unknown(e.to_string()))?;

        // Load existing cache
        let cache: HashMap<String, CacheEntry> = if self.config.enable_cache {
            self.storage.load_cache().await.unwrap_or_default()
        } else {
            HashMap::new()
        };

        // Collect all files to index
        let files = self.collect_files().await?;
        info!("Found {} files to index", files.len());

        // Process each file
        let mut new_tree = DocumentTree::new();
        let mut new_cache = HashMap::new();

        for file_path in files {
            match self.process_file(&file_path, &cache, &mut new_cache).await {
                Ok(Some(doc_node)) => {
                    new_tree.add_document(doc_node);
                }
                Ok(None) => {
                    // File skipped (unchanged)
                    debug!("Skipped unchanged file: {:?}", file_path);
                }
                Err(e) => {
                    warn!("Failed to process file {:?}: {}", file_path, e);
                }
            }
        }

        // Update tree and save
        self.tree = new_tree;
        self.storage.save_tree(&self.tree).await?;
        
        if self.config.enable_cache {
            self.storage.save_cache(&new_cache).await?;
        }

        info!("PageIndex build completed");
        Ok(())
    }

    /// Query the index for relevant context.
    pub async fn query(&self, query: &str, limit: usize) -> Result<Vec<QueryResult>, AppError> {
        self.query_engine.search(&self.tree, query, limit).await
    }

    /// Get a document by its ID.
    pub fn get_document(&self, doc_id: &str) -> Option<&DocumentNode> {
        self.tree.get_document(doc_id)
    }

    /// Get the full tree structure.
    pub fn tree(&self) -> &DocumentTree {
        &self.tree
    }

    /// Get statistics about the index.
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            total_documents: self.tree.document_count(),
            total_sections: self.tree.section_count(),
            total_bytes: self.tree.total_bytes(),
        }
    }

    /// Collect all files that should be indexed.
    async fn collect_files(&self) -> Result<Vec<PathBuf>, AppError> {
        let mut files = Vec::new();
        let mut dirs_to_process = vec![self.project_root.clone()];

        while let Some(dir) = dirs_to_process.pop() {
            let mut entries = tokio::fs::read_dir(&dir)
                .await
                .map_err(|e| AppError::Unknown(e.to_string()))?;

            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| AppError::Unknown(e.to_string()))?
            {
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                if path.is_dir() {
                    // Skip excluded directories
                    if self.config.exclude_dirs.contains(&file_name) {
                        continue;
                    }
                    // Add to queue for processing
                    dirs_to_process.push(path);
                } else if path.is_file() {
                    // Check file extension
                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if self.config.include_extensions.contains(&ext) {
                            // Check file size
                            if let Ok(metadata) = tokio::fs::metadata(&path).await {
                                if metadata.len() <= self.config.max_file_size as u64 {
                                    files.push(path);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(files)
    }

    /// Process a single file, returning None if unchanged.
    async fn process_file(
        &self,
        file_path: &Path,
        cache: &HashMap<String, CacheEntry>,
        new_cache: &mut HashMap<String, CacheEntry>,
    ) -> Result<Option<DocumentNode>, AppError> {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?;

        // Calculate content hash
        let hash = calculate_hash(&content);
        let relative_path = file_path
            .strip_prefix(&self.project_root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        // Check cache
        if let Some(cached) = cache.get(&relative_path) {
            if cached.hash == hash {
                // File unchanged, copy cache entry
                new_cache.insert(relative_path.clone(), cached.clone());
                return Ok(None);
            }
        }

        // Parse the document
        let parser = DocumentParser::new(self.config.max_section_depth);
        let parse_result = parser.parse(&relative_path, &content)?;

        // Create document node
        let doc_node = DocumentNode {
            id: format!("doc_{}", hash[..16].to_string()),
            path: relative_path.clone(),
            title: parse_result.title,
            content: parse_result.content,
            sections: parse_result.sections,
            hash: hash.clone(),
            metadata: parse_result.metadata,
        };

        // Update cache
        new_cache.insert(
            relative_path,
            CacheEntry {
                hash,
                last_indexed: chrono::Utc::now().timestamp(),
                doc_id: doc_node.id.clone(),
            },
        );

        Ok(Some(doc_node))
    }
}

/// Statistics about the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_documents: usize,
    pub total_sections: usize,
    pub total_bytes: usize,
}

/// Calculate a hash of content.
fn calculate_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

use crate::errors::AppError;

// Re-export types for convenience
pub use tree::DocumentMetadata;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = IndexConfig::default();
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
        assert!(config.include_extensions.contains(&"md".to_string()));
        assert!(config.exclude_dirs.contains(&"node_modules".to_string()));
    }

    #[test]
    fn test_hash_calculation() {
        let content = "Hello, World!";
        let hash1 = calculate_hash(content);
        let hash2 = calculate_hash(content);
        let hash3 = calculate_hash("Different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 hex string
    }
}
