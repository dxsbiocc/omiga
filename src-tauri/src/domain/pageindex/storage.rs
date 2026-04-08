//! Storage layer for PageIndex.
//!
//! Manages:
//! - Document tree persistence (JSON)
//! - Content hash cache for incremental updates
//! - Content storage for processed documents

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{debug, info, warn};

use super::tree::DocumentTree;
use crate::errors::AppError;

/// Cache entry for a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// SHA-256 hash of file content
    pub hash: String,
    /// Unix timestamp of last indexing
    pub last_indexed: i64,
    /// Document ID in the tree
    pub doc_id: String,
}

/// Storage manager for the PageIndex system.
pub struct IndexStorage {
    memory_dir: PathBuf,
    #[allow(dead_code)]
    cache_dir: PathBuf,
    #[allow(dead_code)]
    content_dir: PathBuf,
}

impl IndexStorage {
    /// Create a new storage manager.
    pub fn new(memory_dir: impl AsRef<Path>) -> Self {
        let memory_dir = memory_dir.as_ref().to_path_buf();
        Self {
            cache_dir: memory_dir.join("cache"),
            content_dir: memory_dir.join("content"),
            memory_dir,
        }
    }

    /// Get the path to the tree index file.
    pub fn tree_path(&self) -> PathBuf {
        self.memory_dir.join("tree.json")
    }

    /// Get the path to the cache file.
    pub fn cache_path(&self) -> PathBuf {
        self.memory_dir.join("cache.json")
    }

    /// Load the document tree from disk.
    pub async fn load_tree(&self) -> Result<Option<DocumentTree>, AppError> {
        let path = self.tree_path();
        if !path.exists() {
            return Ok(None);
        }

        debug!("Loading document tree from {:?}", path);
        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to read tree: {}", e)))?;

        let tree: DocumentTree = serde_json::from_str(&content)
            .map_err(|e| AppError::Unknown(format!("Failed to parse tree: {}", e)))?;

        info!("Loaded document tree with {} documents", tree.document_count());
        Ok(Some(tree))
    }

    /// Save the document tree to disk.
    pub async fn save_tree(&self, tree: &DocumentTree) -> Result<(), AppError> {
        let path = self.tree_path();
        fs::create_dir_all(&self.memory_dir)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to create memory dir: {}", e)))?;

        let content = serde_json::to_string_pretty(tree)
            .map_err(|e| AppError::Unknown(format!("Failed to serialize tree: {}", e)))?;

        fs::write(&path, content)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to write tree: {}", e)))?;

        debug!("Saved document tree to {:?}", path);
        Ok(())
    }

    /// Load the cache from disk.
    pub async fn load_cache(&self) -> Result<HashMap<String, CacheEntry>, AppError> {
        let path = self.cache_path();
        if !path.exists() {
            return Ok(HashMap::new());
        }

        debug!("Loading cache from {:?}", path);
        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to read cache: {}", e)))?;

        let cache: HashMap<String, CacheEntry> = serde_json::from_str(&content)
            .map_err(|e| AppError::Unknown(format!("Failed to parse cache: {}", e)))?;

        debug!("Loaded cache with {} entries", cache.len());
        Ok(cache)
    }

    /// Save the cache to disk.
    pub async fn save_cache(&self, cache: &HashMap<String, CacheEntry>) -> Result<(), AppError> {
        let path = self.cache_path();

        let content = serde_json::to_string_pretty(cache)
            .map_err(|e| AppError::Unknown(format!("Failed to serialize cache: {}", e)))?;

        fs::write(&path, content)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to write cache: {}", e)))?;

        debug!("Saved cache with {} entries", cache.len());
        Ok(())
    }

    /// Save processed content for a document.
    pub async fn save_content(&self, doc_id: &str, content: &str) -> Result<(), AppError> {
        let path = self.content_dir.join(format!("{}.md", doc_id));
        fs::create_dir_all(&self.content_dir)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to create content dir: {}", e)))?;

        fs::write(&path, content)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to write content: {}", e)))?;

        debug!("Saved content for document {} to {:?}", doc_id, path);
        Ok(())
    }

    /// Load processed content for a document.
    pub async fn load_content(&self, doc_id: &str) -> Result<Option<String>, AppError> {
        let path = self.content_dir.join(format!("{}.md", doc_id));
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to read content: {}", e)))?;

        Ok(Some(content))
    }

    /// Check if the index exists.
    pub async fn index_exists(&self) -> bool {
        self.tree_path().exists()
    }

    /// Get the last modified time of the index.
    pub async fn index_modified_time(&self) -> Option<i64> {
        let path = self.tree_path();
        if !path.exists() {
            return None;
        }

        match fs::metadata(&path).await {
            Ok(metadata) => metadata
                .modified()
                .ok()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64),
            Err(_) => None,
        }
    }

    /// Clear all stored data (cache, tree, content).
    pub async fn clear_all(&self) -> Result<(), AppError> {
        warn!("Clearing all PageIndex data");

        // Remove tree
        let tree_path = self.tree_path();
        if tree_path.exists() {
            fs::remove_file(&tree_path)
                .await
                .map_err(|e| AppError::Unknown(format!("Failed to remove tree: {}", e)))?;
        }

        // Remove cache
        let cache_path = self.cache_path();
        if cache_path.exists() {
            fs::remove_file(&cache_path)
                .await
                .map_err(|e| AppError::Unknown(format!("Failed to remove cache: {}", e)))?;
        }

        // Clear content directory
        if self.content_dir.exists() {
            fs::remove_dir_all(&self.content_dir)
                .await
                .map_err(|e| AppError::Unknown(format!("Failed to remove content dir: {}", e)))?;
        }

        info!("Cleared all PageIndex data");
        Ok(())
    }

    /// Get storage statistics.
    pub async fn get_stats(&self) -> StorageStats {
        let mut stats = StorageStats::default();

        // Tree size
        if let Ok(metadata) = fs::metadata(self.tree_path()).await {
            stats.tree_size_bytes = metadata.len() as usize;
        }

        // Cache size
        if let Ok(metadata) = fs::metadata(self.cache_path()).await {
            stats.cache_size_bytes = metadata.len() as usize;
        }

        // Content directory size
        if let Ok(mut entries) = fs::read_dir(&self.content_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    stats.content_size_bytes += metadata.len() as usize;
                    stats.content_file_count += 1;
                }
            }
        }

        stats
    }

    /// Export the entire index to a portable format.
    pub async fn export(&self, export_path: impl AsRef<Path>) -> Result<(), AppError> {
        let export_path = export_path.as_ref();
        
        // Load tree
        let tree = self.load_tree().await?.unwrap_or_default();
        
        // Create export structure
        let export = ExportData {
            version: 1,
            exported_at: chrono::Utc::now().timestamp(),
            tree,
        };

        // Serialize and save
        let content = serde_json::to_string_pretty(&export)
            .map_err(|e| AppError::Unknown(format!("Failed to serialize export: {}", e)))?;

        fs::write(export_path, content)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to write export: {}", e)))?;

        info!("Exported index to {:?}", export_path);
        Ok(())
    }

    /// Import an index from a portable format.
    pub async fn import(&self, import_path: impl AsRef<Path>) -> Result<DocumentTree, AppError> {
        let import_path = import_path.as_ref();
        
        let content = fs::read_to_string(import_path)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to read import: {}", e)))?;

        let export: ExportData = serde_json::from_str(&content)
            .map_err(|e| AppError::Unknown(format!("Failed to parse import: {}", e)))?;

        // Validate version
        if export.version != 1 {
            return Err(AppError::Unknown(format!(
                "Unsupported export version: {}",
                export.version
            )));
        }

        // Save the imported tree
        self.save_tree(&export.tree).await?;

        info!(
            "Imported index with {} documents",
            export.tree.document_count()
        );
        Ok(export.tree)
    }
}

/// Storage statistics.
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    pub tree_size_bytes: usize,
    pub cache_size_bytes: usize,
    pub content_size_bytes: usize,
    pub content_file_count: usize,
}

/// Export data structure for portable index.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExportData {
    pub version: i32,
    pub exported_at: i64,
    pub tree: DocumentTree,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_save_and_load_tree() {
        let temp_dir = TempDir::new().unwrap();
        let storage = IndexStorage::new(&temp_dir);

        let tree = DocumentTree::new();
        storage.save_tree(&tree).await.unwrap();

        let loaded = storage.load_tree().await.unwrap();
        assert!(loaded.is_some());
    }

    #[tokio::test]
    async fn test_save_and_load_cache() {
        let temp_dir = TempDir::new().unwrap();
        let storage = IndexStorage::new(&temp_dir);

        let mut cache = HashMap::new();
        cache.insert(
            "test.md".to_string(),
            CacheEntry {
                hash: "abc123".to_string(),
                last_indexed: 1234567890,
                doc_id: "doc_1".to_string(),
            },
        );

        storage.save_cache(&cache).await.unwrap();

        let loaded = storage.load_cache().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get("test.md").unwrap().hash, "abc123");
    }

    #[tokio::test]
    async fn test_content_storage() {
        let temp_dir = TempDir::new().unwrap();
        let storage = IndexStorage::new(&temp_dir);

        storage.save_content("doc_1", "Hello World").await.unwrap();

        let loaded = storage.load_content("doc_1").await.unwrap();
        assert_eq!(loaded, Some("Hello World".to_string()));

        let not_found = storage.load_content("nonexistent").await.unwrap();
        assert_eq!(not_found, None);
    }

    #[tokio::test]
    async fn test_clear_all() {
        let temp_dir = TempDir::new().unwrap();
        let storage = IndexStorage::new(&temp_dir);

        // Save some data
        let tree = DocumentTree::new();
        storage.save_tree(&tree).await.unwrap();
        storage.save_content("doc_1", "test").await.unwrap();

        // Clear it
        storage.clear_all().await.unwrap();

        // Verify cleared
        assert!(!storage.tree_path().exists());
        assert!(!storage.content_dir.exists());
    }
}
