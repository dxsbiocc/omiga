//! App-data compatibility migration for bundle identifier changes.
//!
//! Tauri derives `app_data_dir()` from the bundle identifier. When the
//! identifier changed from `com.omiga.app` to `com.omiga.desktop`, existing
//! users' SQLite sessions and generated artifacts stayed in the old sibling
//! directory. Copy that legacy app-data directory into the new location on
//! first startup, before SQLite creates a fresh `omiga.db`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DATABASE_FILE: &str = "omiga.db";
pub(crate) const APP_DATA_COMPAT_DIR_NAMES: &[&str] =
    &["com.omiga.desktop", "com.omiga.app", "Omiga", "omiga"];

pub(crate) fn migrate_legacy_app_data_if_needed(
    current_app_data_dir: &Path,
) -> io::Result<Option<PathBuf>> {
    if current_app_data_dir.join(DATABASE_FILE).exists() {
        return Ok(None);
    }

    let Some(parent) = current_app_data_dir.parent() else {
        return Ok(None);
    };

    for legacy_name in APP_DATA_COMPAT_DIR_NAMES {
        let legacy_dir = parent.join(legacy_name);
        if legacy_dir == current_app_data_dir || !legacy_dir.join(DATABASE_FILE).is_file() {
            continue;
        }

        copy_dir_contents_without_overwrite(&legacy_dir, current_app_data_dir)?;
        return Ok(Some(legacy_dir));
    }

    Ok(None)
}

fn copy_dir_contents_without_overwrite(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let source_path = entry.path();
        let dest_path = to.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_contents_without_overwrite(&source_path, &dest_path)?;
        } else if file_type.is_file() && !dest_path.exists() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn migrates_legacy_identifier_data_when_current_database_is_missing() {
        let temp = TempDir::new().expect("tempdir");
        let current = temp.path().join("com.omiga.desktop");
        let legacy = temp.path().join("com.omiga.app");
        fs::create_dir_all(legacy.join("tool-results/session-1")).expect("legacy dirs");
        fs::write(legacy.join(DATABASE_FILE), "legacy-db").expect("legacy db");
        fs::write(legacy.join("tool-results/session-1/out.txt"), "artifact")
            .expect("legacy artifact");

        let migrated = migrate_legacy_app_data_if_needed(&current).expect("migration");

        assert_eq!(migrated.as_deref(), Some(legacy.as_path()));
        assert_eq!(
            fs::read_to_string(current.join(DATABASE_FILE)).expect("current db"),
            "legacy-db"
        );
        assert_eq!(
            fs::read_to_string(current.join("tool-results/session-1/out.txt")).expect("artifact"),
            "artifact"
        );
    }

    #[test]
    fn does_not_overwrite_existing_current_database() {
        let temp = TempDir::new().expect("tempdir");
        let current = temp.path().join("com.omiga.desktop");
        let legacy = temp.path().join("com.omiga.app");
        fs::create_dir_all(&current).expect("current dir");
        fs::create_dir_all(&legacy).expect("legacy dir");
        fs::write(current.join(DATABASE_FILE), "current-db").expect("current db");
        fs::write(legacy.join(DATABASE_FILE), "legacy-db").expect("legacy db");

        let migrated = migrate_legacy_app_data_if_needed(&current).expect("migration");

        assert!(migrated.is_none());
        assert_eq!(
            fs::read_to_string(current.join(DATABASE_FILE)).expect("current db"),
            "current-db"
        );
    }
}
