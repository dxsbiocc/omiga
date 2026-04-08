//! Memory system configuration
//!
//! Supports customizable memory paths with security validation.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::errors::AppError;

/// Memory system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Memory root directory (relative or absolute path)
    /// Default: ".omiga/memory"
    pub root_dir: PathBuf,

    /// Wiki subdirectory name
    /// Default: "wiki"
    pub wiki_dir: String,

    /// Implicit index subdirectory name
    /// Default: "implicit"
    pub implicit_dir: String,

    /// Whether to auto-build implicit index
    /// Default: true
    pub auto_build_index: bool,

    /// Auto-build interval in seconds (0 = disabled)
    /// Default: 3600 (1 hour)
    pub auto_build_interval: u64,

    /// File extensions to index for implicit memory
    pub index_extensions: Vec<String>,

    /// Directories to exclude from indexing
    pub exclude_dirs: Vec<String>,

    /// Maximum file size to index (bytes)
    /// Default: 10MB
    pub max_file_size: usize,

    /// Whether to use shared memory across worktrees
    /// Default: false
    pub shared_memory: bool,

    /// Shared memory path (when shared_memory is true)
    /// Default: None
    pub shared_path: Option<PathBuf>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            root_dir: PathBuf::from(".omiga/memory"),
            wiki_dir: "wiki".to_string(),
            implicit_dir: "implicit".to_string(),
            auto_build_index: true,
            auto_build_interval: 3600,
            index_extensions: vec![
                "md".to_string(),
                "txt".to_string(),
                "rs".to_string(),
                "py".to_string(),
                "js".to_string(),
                "ts".to_string(),
                "tsx".to_string(),
                "jsx".to_string(),
                "go".to_string(),
                "java".to_string(),
                "cpp".to_string(),
                "c".to_string(),
                "h".to_string(),
                "hpp".to_string(),
                "json".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
                "toml".to_string(),
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
            max_file_size: 10 * 1024 * 1024,
            shared_memory: false,
            shared_path: None,
        }
    }
}

impl MemoryConfig {
    /// Get the effective root directory (resolves relative to project root)
    pub fn effective_root(&self, project_root: impl AsRef<Path>) -> PathBuf {
        if self.root_dir.is_absolute() {
            self.root_dir.clone()
        } else {
            project_root.as_ref().join(&self.root_dir)
        }
    }

    /// Get wiki directory path
    pub fn wiki_path(&self, project_root: impl AsRef<Path>) -> PathBuf {
        self.effective_root(&project_root).join(&self.wiki_dir)
    }

    /// Get implicit index directory path
    pub fn implicit_path(&self, project_root: impl AsRef<Path>) -> PathBuf {
        self.effective_root(&project_root).join(&self.implicit_dir)
    }

    /// Get config file path
    pub fn config_path(&self, project_root: impl AsRef<Path>) -> PathBuf {
        self.effective_root(&project_root).join("config.json")
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), AppError> {
        let root_str = self.root_dir.to_string_lossy();
        if root_str.contains("..") {
            return Err(AppError::Unknown(
                "Memory root directory cannot contain '..'".to_string()
            ));
        }

        if self.wiki_dir.contains('/') || self.wiki_dir.contains('\\') {
            return Err(AppError::Unknown(
                "Wiki directory name cannot contain path separators".to_string()
            ));
        }
        if self.implicit_dir.contains('/') || self.implicit_dir.contains('\\') {
            return Err(AppError::Unknown(
                "Implicit directory name cannot contain path separators".to_string()
            ));
        }

        if self.root_dir.as_os_str() == "/" || 
           self.root_dir.as_os_str() == "\\" ||
           root_str == "~" ||
           root_str == "$HOME" {
            return Err(AppError::Unknown(
                "Memory root directory cannot be a system root directory".to_string()
            ));
        }

        Ok(())
    }
}
