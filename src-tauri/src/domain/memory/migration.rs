//! Migration logic for transitioning from old memory structure to unified structure
//!
//! Detects and migrates:
//! - `.omiga/wiki/` → `.omiga/memory/wiki/`
//! - `.omiga/memory/` (old pageindex) → `.omiga/memory/implicit/`

use std::path::Path;
use tokio::fs;
use tracing::info;
use walkdir::WalkDir;

use crate::errors::AppError;

/// Check if migration is needed and perform it
pub async fn migrate_if_needed(project_root: impl AsRef<Path>) -> Result<(), AppError> {
    let project_root = project_root.as_ref();

    // Check for old structures
    let old_wiki = project_root.join(".omiga").join("wiki");
    let old_memory = project_root.join(".omiga").join("memory");
    let new_memory = project_root.join(".omiga").join("memory");
    let new_wiki = new_memory.join("wiki");
    let new_implicit = new_memory.join("implicit");

    // Case 1: Old wiki exists but new structure doesn't
    if old_wiki.exists() && !new_wiki.exists() {
        info!("Detected old wiki structure, migrating...");
        migrate_wiki(&old_wiki, &new_wiki).await?;
    }

    // Case 2: Old pageindex exists (tree.json directly in .omiga/memory/)
    if old_memory.join("tree.json").exists() && !new_implicit.exists() {
        info!("Detected old pageindex structure, migrating...");
        migrate_pageindex(&old_memory, &new_implicit).await?;
    }

    backfill_wiki_metadata(&new_wiki).await?;
    backfill_wiki_metadata(&crate::domain::memory::config::permanent_wiki_path()).await?;

    Ok(())
}

pub async fn backfill_wiki_metadata(wiki_root: &Path) -> Result<(), AppError> {
    if !wiki_root.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(wiki_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path().to_path_buf();
        if !entry.file_type().is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let raw = match fs::read_to_string(&path).await {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        if raw.trim_start().starts_with("---") {
            continue;
        }
        let title = raw
            .lines()
            .find(|line| line.starts_with("# "))
            .map(|line| line.trim_start_matches("# ").trim().to_string())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            });
        let doc_id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled");
        let enriched = format!(
            "---\n\
             title: \"{}\"\n\
             knowledge_layer: knowledge_base\n\
             template: legacy_backfill\n\
             doc_id: {}\n\
             migrated_at: {}\n\
             ---\n\n{}",
            title.replace('"', "\\\""),
            doc_id,
            chrono::Utc::now().to_rfc3339(),
            raw
        );
        fs::write(&path, enriched).await.map_err(|e| {
            AppError::Unknown(format!("Failed to backfill {}: {}", path.display(), e))
        })?;
    }
    Ok(())
}

/// Migrate wiki from old location to new unified structure
async fn migrate_wiki(old_wiki: &Path, new_wiki: &Path) -> Result<(), AppError> {
    fs::create_dir_all(new_wiki)
        .await
        .map_err(|e| AppError::Unknown(format!("Failed to create new wiki dir: {}", e)))?;

    // Copy all .md files recursively, preserving subdirectories.
    for entry in WalkDir::new(old_wiki)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !path.extension().map(|e| e == "md").unwrap_or(false) {
            continue;
        }
        let relative = path
            .strip_prefix(old_wiki)
            .map_err(|e| AppError::Unknown(format!("Invalid wiki path: {}", e)))?;
        let dest = new_wiki.join(relative);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                AppError::Unknown(format!(
                    "Failed to create migrated wiki parent {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        fs::copy(path, &dest).await.map_err(|e| {
            AppError::Unknown(format!("Failed to copy {}: {}", relative.display(), e))
        })?;
        info!("Migrated wiki file: {:?}", relative);
    }

    info!("Wiki migration completed");
    Ok(())
}

/// Migrate pageindex from old location to new unified structure
async fn migrate_pageindex(old_memory: &Path, new_implicit: &Path) -> Result<(), AppError> {
    fs::create_dir_all(new_implicit)
        .await
        .map_err(|e| AppError::Unknown(format!("Failed to create new implicit dir: {}", e)))?;

    // Files to migrate
    let files_to_move = ["tree.json", "cache.json"];

    for file in &files_to_move {
        let old_path = old_memory.join(file);
        if old_path.exists() {
            let new_path = new_implicit.join(file);
            fs::rename(&old_path, &new_path)
                .await
                .map_err(|e| AppError::Unknown(format!("Failed to move {}: {}", file, e)))?;
            info!("Migrated pageindex file: {}", file);
        }
    }

    // Migrate content directory
    let old_content = old_memory.join("content");
    if old_content.exists() {
        let new_content = new_implicit.join("content");
        fs::rename(&old_content, &new_content)
            .await
            .map_err(|e| AppError::Unknown(format!("Failed to move content dir: {}", e)))?;
        info!("Migrated content directory");
    }

    info!("PageIndex migration completed");
    Ok(())
}

/// Detect current memory structure version
pub async fn detect_structure_version(project_root: impl AsRef<Path>) -> MemoryStructureVersion {
    let project_root = project_root.as_ref();
    let omiga = project_root.join(".omiga");

    // Check for new unified structure
    let new_memory = omiga.join("memory");
    if new_memory.join("config.json").exists() {
        return MemoryStructureVersion::Unified;
    }

    // Check for old separate structures
    let has_old_wiki = omiga.join("wiki").exists();
    let has_old_memory = omiga.join("memory").join("tree.json").exists();

    match (has_old_wiki, has_old_memory) {
        (true, true) => MemoryStructureVersion::LegacyBoth,
        (true, false) => MemoryStructureVersion::LegacyWikiOnly,
        (false, true) => MemoryStructureVersion::LegacyPageIndexOnly,
        (false, false) => MemoryStructureVersion::None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MemoryStructureVersion {
    /// New unified structure with config.json
    Unified,
    /// Old structure with both wiki and pageindex
    LegacyBoth,
    /// Old structure with wiki only
    LegacyWikiOnly,
    /// Old structure with pageindex only
    LegacyPageIndexOnly,
    /// No memory structure exists
    None,
}

impl MemoryStructureVersion {
    pub fn needs_migration(&self) -> bool {
        matches!(
            self,
            MemoryStructureVersion::LegacyBoth
                | MemoryStructureVersion::LegacyWikiOnly
                | MemoryStructureVersion::LegacyPageIndexOnly
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_detect_unified() {
        let temp = TempDir::new().unwrap();
        let config_path = temp
            .path()
            .join(".omiga")
            .join("memory")
            .join("config.json");
        fs::create_dir_all(config_path.parent().unwrap())
            .await
            .unwrap();
        fs::write(&config_path, "{}").await.unwrap();

        let version = detect_structure_version(temp.path()).await;
        assert_eq!(version, MemoryStructureVersion::Unified);
    }

    #[tokio::test]
    async fn test_detect_legacy_wiki() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join(".omiga").join("wiki"))
            .await
            .unwrap();

        let version = detect_structure_version(temp.path()).await;
        assert_eq!(version, MemoryStructureVersion::LegacyWikiOnly);
    }

    #[tokio::test]
    async fn test_detect_none() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join(".omiga"))
            .await
            .unwrap();

        let version = detect_structure_version(temp.path()).await;
        assert_eq!(version, MemoryStructureVersion::None);
    }

    #[tokio::test]
    async fn backfill_wiki_metadata_recurses_into_nested_directories() {
        let temp = TempDir::new().unwrap();
        let nested_dir = temp.path().join("wiki").join("concepts");
        fs::create_dir_all(&nested_dir).await.unwrap();
        let nested_file = nested_dir.join("memory.md");
        fs::write(&nested_file, "# Memory\n\nLegacy content")
            .await
            .unwrap();

        backfill_wiki_metadata(temp.path().join("wiki").as_path())
            .await
            .unwrap();

        let enriched = fs::read_to_string(&nested_file).await.unwrap();
        assert!(enriched.starts_with("---"));
        assert!(enriched.contains("knowledge_layer: knowledge_base"));
    }
}
