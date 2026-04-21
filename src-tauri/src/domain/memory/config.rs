//! Memory system configuration
//!
//! Supports customizable memory paths with security validation.
//!
//! **Default (UserHome)**: data lives under `~/.omiga/memory/projects/<id>/` where `id` is a
//! stable hash of the project path. Permanent cross-project notes live under
//! `~/.omiga/memory/permanent/wiki/`.
//!
//! **Legacy (ProjectRelative)**: `root_dir` relative to the project (e.g. `.omiga/memory`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::errors::AppError;

/// Where project-scoped wiki / implicit index files are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    /// `~/.omiga/memory/projects/<storage_key>/` (default for new projects)
    #[default]
    UserHome,
    /// `<project>/root_dir/` (classic layout)
    ProjectRelative,
}

/// Memory system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Memory root directory (relative to project) — used only in [`MemoryMode::ProjectRelative`].
    /// Default: `.omiga/memory`
    pub root_dir: PathBuf,

    /// Wiki subdirectory name
    /// Default: "wiki"
    pub wiki_dir: String,

    /// Implicit index subdirectory name
    /// Default: "implicit"
    pub implicit_dir: String,

    /// Directory for storing raw original files imported into wiki.
    /// When None, defaults to `~/.omiga/memory/raw`.
    /// Absolute path; set by the user in settings.
    #[serde(default)]
    pub raw_dir: Option<PathBuf>,

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

    /// Where project memory files live (default: user home under `~/.omiga/memory/projects/...`).
    #[serde(default)]
    pub memory_mode: MemoryMode,
}

/// Default raw file storage: `~/.omiga/memory/raw`
pub fn default_raw_path() -> PathBuf {
    user_omiga_root().join("memory").join("raw")
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            root_dir: PathBuf::from(".omiga/memory"),
            wiki_dir: "wiki".to_string(),
            implicit_dir: "implicit".to_string(),
            raw_dir: None,
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
            memory_mode: MemoryMode::default(),
        }
    }
}

/// `~/.omiga` (user-level Omiga data root)
pub fn user_omiga_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".omiga")
}

/// Stable directory name for a project: first 16 hex chars of SHA-256 of canonical path.
pub fn project_storage_key(project_root: &Path) -> String {
    let normalized =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(normalized.to_string_lossy().as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

/// Global permanent wiki: `~/.omiga/memory/permanent/wiki`
pub fn permanent_wiki_path() -> PathBuf {
    user_omiga_root()
        .join("memory")
        .join("permanent")
        .join("wiki")
}

impl MemoryConfig {
    /// Config file always lives in the **project** at `.omiga/memory/config.json`
    /// (regardless of [`MemoryMode`]).
    pub fn project_config_path(project_root: impl AsRef<Path>) -> PathBuf {
        project_root
            .as_ref()
            .join(".omiga")
            .join("memory")
            .join("config.json")
    }

    /// Get the effective root directory for wiki + implicit storage.
    pub fn effective_root(&self, project_root: impl AsRef<Path>) -> PathBuf {
        match self.memory_mode {
            MemoryMode::UserHome => {
                let key = project_storage_key(project_root.as_ref());
                user_omiga_root().join("memory").join("projects").join(key)
            }
            MemoryMode::ProjectRelative => {
                if self.root_dir.is_absolute() {
                    self.root_dir.clone()
                } else {
                    project_root.as_ref().join(&self.root_dir)
                }
            }
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

    /// Get raw file storage path (absolute). Falls back to `~/.omiga/memory/raw`.
    pub fn raw_path(&self) -> PathBuf {
        self.raw_dir.clone().unwrap_or_else(default_raw_path)
    }

    /// Legacy helper — use [`project_config_path`] instead.
    pub fn config_path(&self, project_root: impl AsRef<Path>) -> PathBuf {
        Self::project_config_path(project_root)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), AppError> {
        let root_str = self.root_dir.to_string_lossy();
        if root_str.contains("..") {
            return Err(AppError::Unknown(
                "Memory root directory cannot contain '..'".to_string(),
            ));
        }

        if self.wiki_dir.contains('/') || self.wiki_dir.contains('\\') {
            return Err(AppError::Unknown(
                "Wiki directory name cannot contain path separators".to_string(),
            ));
        }
        if self.implicit_dir.contains('/') || self.implicit_dir.contains('\\') {
            return Err(AppError::Unknown(
                "Implicit directory name cannot contain path separators".to_string(),
            ));
        }

        if self.memory_mode == MemoryMode::ProjectRelative {
            if self.root_dir.as_os_str() == "/"
                || self.root_dir.as_os_str() == "\\"
                || root_str == "~"
                || root_str == "$HOME"
            {
                return Err(AppError::Unknown(
                    "Memory root directory cannot be a system root directory".to_string(),
                ));
            }
        }

        Ok(())
    }
}
