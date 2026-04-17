//! Global registry of project memory locations under `~/.omiga/memory/registry.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::fs;

use crate::errors::AppError;

/// Path to `~/.omiga/memory/registry.json`
pub fn registry_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".omiga")
        .join("memory")
        .join("registry.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryRegistry {
    pub version: u32,
    /// Canonical project path string → entry
    pub projects: HashMap<String, MemoryRegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRegistryEntry {
    pub project_path: String,
    pub memory_root: String,
    pub wiki_path: String,
    pub implicit_path: String,
    #[serde(default)]
    pub permanent_wiki_path: String,
    pub updated_at_unix: i64,
}

pub async fn upsert_project_paths(
    project_root: &Path,
    memory_root: &Path,
    wiki_path: &Path,
    implicit_path: &Path,
    permanent_wiki: &Path,
) -> Result<(), AppError> {
    let key = canonical_project_key(project_root);
    let path = registry_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to create registry parent: {}", e)))?;
    }

    let mut reg: MemoryRegistry = if path.exists() {
        let raw = fs::read_to_string(&path)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to read registry: {}", e)))?;
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        MemoryRegistry {
            version: 1,
            projects: HashMap::new(),
        }
    };
    reg.version = 1;
    reg.projects.insert(
        key,
        MemoryRegistryEntry {
            project_path: project_root.to_string_lossy().to_string(),
            memory_root: memory_root.to_string_lossy().to_string(),
            wiki_path: wiki_path.to_string_lossy().to_string(),
            implicit_path: implicit_path.to_string_lossy().to_string(),
            permanent_wiki_path: permanent_wiki.to_string_lossy().to_string(),
            updated_at_unix: chrono::Utc::now().timestamp(),
        },
    );

    let json = serde_json::to_string_pretty(&reg)
        .map_err(|e| AppError::Unknown(format!("Failed to serialize registry: {}", e)))?;
    fs::write(&path, json)
        .await
        .map_err(|e| AppError::Unknown(format!("Failed to write registry: {}", e)))?;
    Ok(())
}

fn canonical_project_key(project_root: &Path) -> String {
    std::fs::canonicalize(project_root)
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}
