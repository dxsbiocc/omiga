//! VS Code-compatible extension management commands.
//!
//! This intentionally starts with manifest/contribution compatibility rather than
//! a full VS Code extension-host runtime. VSIX packages are unpacked into
//! `~/.omiga/extensions/<publisher.name>` and their `package.json` is exposed to
//! the frontend so Omiga can honor static contribution points such as icon
//! themes, languages, custom editors, and notebook/file associations.

use serde::Serialize;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const EXTENSION_PACKAGE_JSON: &str = "package.json";
const VSIX_EXTENSION_PREFIX: &str = "extension/";
const MAX_EXTENSION_FILE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_VSIX_ENTRY_BYTES: u64 = 25 * 1024 * 1024;
const MAX_VSIX_TOTAL_BYTES: u64 = 250 * 1024 * 1024;

fn extensions_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".omiga").join("extensions")
}

fn sanitize_extension_id(id: &str) -> Result<String, String> {
    let clean: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() || clean.contains("..") {
        return Err(format!("invalid extension id: {id}"));
    }
    Ok(clean)
}

fn ensure_safe_relative_path(rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim_start_matches(['/', '\\']);
    if rel.is_empty() {
        return Err("empty extension-relative path".to_string());
    }

    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(format!("absolute extension path is not allowed: {rel}"));
    }

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("unsafe extension path: {rel}"));
            }
        }
    }

    if out.as_os_str().is_empty() {
        return Err(format!("unsafe extension path: {rel}"));
    }
    Ok(out)
}

fn read_manifest_value(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read manifest: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse manifest: {e}"))
}

fn manifest_identity(
    manifest: &Value,
    fallback_dir: Option<&str>,
) -> Result<ManifestIdentity, String> {
    let publisher = manifest
        .get("publisher")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .or(fallback_dir)
        .ok_or("manifest missing 'name'")?;
    let display_name = manifest
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(name);
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("0.0.0");
    let description = manifest
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();

    Ok(ManifestIdentity {
        id: format!("{publisher}.{name}"),
        name: name.to_string(),
        display_name: display_name.to_string(),
        publisher: publisher.to_string(),
        version: version.to_string(),
        description: description.to_string(),
    })
}

fn extension_info_from_dir(ext_dir: &Path) -> Result<InstalledVscodeExtension, String> {
    let manifest = read_manifest_value(&ext_dir.join(EXTENSION_PACKAGE_JSON))?;
    let fallback_dir = ext_dir.file_name().and_then(|s| s.to_str());
    let identity = manifest_identity(&manifest, fallback_dir)?;

    Ok(InstalledVscodeExtension {
        id: identity.id,
        name: identity.name,
        display_name: identity.display_name,
        publisher: identity.publisher,
        version: identity.version,
        description: identity.description,
        path: ext_dir.to_string_lossy().to_string(),
        enabled: true,
        package_json: manifest,
    })
}

fn read_vsix_manifest(archive: &mut zip::ZipArchive<File>) -> Result<Value, String> {
    let mut entry = archive
        .by_name(&format!("{VSIX_EXTENSION_PREFIX}{EXTENSION_PACKAGE_JSON}"))
        .map_err(|_| "VSIX missing extension/package.json".to_string())?;
    if entry.size() > MAX_EXTENSION_FILE_BYTES {
        return Err(format!(
            "VSIX manifest is too large: {} bytes (max {})",
            entry.size(),
            MAX_EXTENSION_FILE_BYTES
        ));
    }
    let mut buf = String::new();
    let bytes_read = (&mut entry)
        .take(MAX_EXTENSION_FILE_BYTES + 1)
        .read_to_string(&mut buf)
        .map_err(|e| format!("read manifest: {e}"))?;
    if bytes_read as u64 > MAX_EXTENSION_FILE_BYTES {
        return Err(format!(
            "VSIX manifest is too large: {} bytes (max {})",
            bytes_read, MAX_EXTENSION_FILE_BYTES
        ));
    }
    serde_json::from_str(&buf).map_err(|e| format!("parse manifest: {e}"))
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("remove {}: {e}", path.display()))
    } else {
        fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))
    }
}

fn cleanup_path(path: &Path) {
    if let Err(error) = remove_path_if_exists(path) {
        tracing::warn!(path = %path.display(), error = %error, "Failed to clean extension path");
    }
}

fn extract_vsix_archive_to_dir(
    archive: &mut zip::ZipArchive<File>,
    ext_dir: &Path,
    max_entry_bytes: u64,
    max_total_bytes: u64,
) -> Result<(), String> {
    let mut declared_total_bytes = 0_u64;
    let mut extracted_total_bytes = 0_u64;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("read VSIX entry: {e}"))?;
        let raw_name = entry.name().replace('\\', "/");
        if !raw_name.starts_with(VSIX_EXTENSION_PREFIX) {
            continue;
        }

        let rel = &raw_name[VSIX_EXTENSION_PREFIX.len()..];
        if rel.trim().is_empty() {
            continue;
        }
        let safe_rel = match ensure_safe_relative_path(rel) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let target = ext_dir.join(safe_rel);

        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("create directory: {e}"))?;
            continue;
        }

        let declared_size = entry.size();
        if declared_size > max_entry_bytes {
            return Err(format!(
                "VSIX entry is too large: {raw_name} is {declared_size} bytes (max {max_entry_bytes})"
            ));
        }
        declared_total_bytes = declared_total_bytes
            .checked_add(declared_size)
            .ok_or_else(|| "VSIX declared extracted size overflowed".to_string())?;
        if declared_total_bytes > max_total_bytes {
            return Err(format!(
                "VSIX declared extracted size exceeds limit: {declared_total_bytes} bytes (max {max_total_bytes})"
            ));
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create parent directory: {e}"))?;
        }
        let mut buf = Vec::new();
        let bytes_read = (&mut entry)
            .take(max_entry_bytes + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read entry {raw_name}: {e}"))?;
        if bytes_read as u64 > max_entry_bytes {
            return Err(format!(
                "VSIX entry is too large: {raw_name} exceeded {max_entry_bytes} bytes"
            ));
        }
        extracted_total_bytes = extracted_total_bytes
            .checked_add(bytes_read as u64)
            .ok_or_else(|| "VSIX extracted size overflowed".to_string())?;
        if extracted_total_bytes > max_total_bytes {
            return Err(format!(
                "VSIX extracted size exceeds limit: {extracted_total_bytes} bytes (max {max_total_bytes})"
            ));
        }
        fs::write(&target, &buf).map_err(|e| format!("write entry {raw_name}: {e}"))?;
    }

    Ok(())
}

fn replace_extension_dir_atomically(
    temp_dir: &Path,
    ext_dir: &Path,
    backup_dir: &Path,
) -> Result<(), String> {
    remove_path_if_exists(backup_dir)?;

    let had_existing = ext_dir.exists();
    if had_existing {
        fs::rename(ext_dir, backup_dir).map_err(|e| format!("stage existing extension: {e}"))?;
    }

    match fs::rename(temp_dir, ext_dir) {
        Ok(()) => {
            if had_existing {
                cleanup_path(backup_dir);
            }
            Ok(())
        }
        Err(error) => {
            if had_existing && backup_dir.exists() && !ext_dir.exists() {
                if let Err(restore_error) = fs::rename(backup_dir, ext_dir) {
                    tracing::error!(
                        extension = %ext_dir.display(),
                        backup = %backup_dir.display(),
                        error = %restore_error,
                        "Failed to restore previous extension after install failure"
                    );
                }
            }
            Err(format!("activate extension: {error}"))
        }
    }
}

#[derive(Debug)]
struct ManifestIdentity {
    id: String,
    name: String,
    display_name: String,
    publisher: String,
    version: String,
    description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVscodeExtension {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub publisher: String,
    pub version: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
    pub package_json: Value,
}

#[tauri::command]
pub fn vscode_extensions_dir() -> Result<String, String> {
    let dir = extensions_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("create extensions dir: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn install_vscode_extension(
    vsix_path: String,
) -> Result<InstalledVscodeExtension, String> {
    let vsix = Path::new(&vsix_path);
    let base_dir = extensions_dir();
    install_vscode_extension_from_vsix(vsix, &base_dir)
}

fn install_vscode_extension_from_vsix(
    vsix: &Path,
    base_dir: &Path,
) -> Result<InstalledVscodeExtension, String> {
    if !vsix.is_file() {
        return Err(format!("VSIX not found: {}", vsix.display()));
    }

    let file = File::open(vsix).map_err(|e| format!("open VSIX: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("bad VSIX archive: {e}"))?;
    let manifest = read_vsix_manifest(&mut archive)?;
    let identity = manifest_identity(&manifest, None)?;
    let safe_id = sanitize_extension_id(&identity.id)?;

    fs::create_dir_all(base_dir).map_err(|e| format!("create extensions dir: {e}"))?;
    let ext_dir = base_dir.join(&safe_id);
    let install_id = uuid::Uuid::new_v4();
    let temp_dir = base_dir.join(format!(".installing-{safe_id}-{install_id}"));
    let backup_dir = base_dir.join(format!(".backup-{safe_id}-{install_id}"));

    let result = (|| {
        fs::create_dir_all(&temp_dir).map_err(|e| format!("create staging dir: {e}"))?;
        extract_vsix_archive_to_dir(
            &mut archive,
            &temp_dir,
            MAX_VSIX_ENTRY_BYTES,
            MAX_VSIX_TOTAL_BYTES,
        )?;
        extension_info_from_dir(&temp_dir)?;
        replace_extension_dir_atomically(&temp_dir, &ext_dir, &backup_dir)?;
        extension_info_from_dir(&ext_dir)
    })();

    if result.is_err() {
        cleanup_path(&temp_dir);
    }
    if result.is_ok() && backup_dir.exists() {
        cleanup_path(&backup_dir);
    }

    result
}

#[tauri::command]
pub async fn uninstall_vscode_extension(extension_id: String) -> Result<(), String> {
    let safe_id = sanitize_extension_id(&extension_id)?;
    let ext_dir = extensions_dir().join(safe_id);
    if !ext_dir.exists() {
        return Err(format!("extension is not installed: {extension_id}"));
    }
    fs::remove_dir_all(&ext_dir).map_err(|e| format!("remove extension: {e}"))
}

#[tauri::command]
pub async fn list_vscode_extensions() -> Result<Vec<InstalledVscodeExtension>, String> {
    let dir = extensions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| format!("read extensions dir: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join(EXTENSION_PACKAGE_JSON).exists() {
            continue;
        }
        match extension_info_from_dir(&path) {
            Ok(info) => out.push(info),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "Skipping invalid VS Code extension")
            }
        }
    }

    out.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

#[tauri::command]
pub async fn read_vscode_extension_file(
    extension_id: String,
    relative_path: String,
) -> Result<String, String> {
    let safe_id = sanitize_extension_id(&extension_id)?;
    let safe_rel = ensure_safe_relative_path(&relative_path)?;
    let ext_dir = extensions_dir().join(safe_id);
    let canonical_ext_dir =
        fs::canonicalize(&ext_dir).map_err(|e| format!("extension not found: {e}"))?;
    let target = ext_dir.join(safe_rel);
    if !target.is_file() {
        return Err(format!("extension file not found: {relative_path}"));
    }
    let canonical_target =
        fs::canonicalize(&target).map_err(|e| format!("extension file not found: {e}"))?;
    if !canonical_target.starts_with(&canonical_ext_dir) {
        return Err(format!("unsafe extension file path: {relative_path}"));
    }

    let meta = fs::metadata(&canonical_target).map_err(|e| format!("read metadata: {e}"))?;
    if meta.len() > MAX_EXTENSION_FILE_BYTES {
        return Err(format!(
            "extension file is too large: {} bytes (max {})",
            meta.len(),
            MAX_EXTENSION_FILE_BYTES
        ));
    }

    fs::read_to_string(&canonical_target).map_err(|e| format!("read extension file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package_json(version: &str) -> String {
        serde_json::json!({
            "publisher": "acme",
            "name": "demo",
            "displayName": "Demo",
            "version": version
        })
        .to_string()
    }

    fn write_vsix(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("create vsix");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            zip.start_file(name, options).expect("start file");
            zip.write_all(bytes).expect("write file");
        }
        zip.finish().expect("finish vsix");
    }

    #[test]
    fn sanitizes_extension_ids_without_allowing_traversal() {
        assert_eq!(
            sanitize_extension_id("publisher.name").unwrap(),
            "publisher.name"
        );
        assert_eq!(
            sanitize_extension_id("pub-name_ext").unwrap(),
            "pub-name_ext"
        );
        assert!(sanitize_extension_id("../escape").is_err());
        assert!(sanitize_extension_id("..").is_err());
        assert!(sanitize_extension_id("///").is_err());
    }

    #[test]
    fn rejects_unsafe_relative_paths() {
        assert_eq!(
            ensure_safe_relative_path("media/icon.svg").unwrap(),
            PathBuf::from("media").join("icon.svg")
        );
        assert_eq!(
            ensure_safe_relative_path("./themes/icons.json").unwrap(),
            PathBuf::from("themes").join("icons.json")
        );
        assert!(ensure_safe_relative_path("../package.json").is_err());
        assert!(ensure_safe_relative_path("themes/../../package.json").is_err());
        assert!(ensure_safe_relative_path("").is_err());
    }

    #[test]
    fn reads_manifest_identity_like_vscode_extensions() {
        let manifest = serde_json::json!({
            "publisher": "acme",
            "name": "icons",
            "displayName": "Acme Icons",
            "version": "1.2.3",
            "description": "Demo"
        });

        let identity = manifest_identity(&manifest, None).unwrap();
        assert_eq!(identity.id, "acme.icons");
        assert_eq!(identity.display_name, "Acme Icons");
        assert_eq!(identity.version, "1.2.3");
        assert_eq!(identity.description, "Demo");
    }

    #[test]
    fn rejects_vsix_entries_over_configured_size_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vsix = dir.path().join("too-large.vsix");
        let manifest = br#"{"publisher":"a","name":"d"}"#;
        let bytes = vec![b'x'; 65];
        write_vsix(
            &vsix,
            &[
                ("extension/package.json", manifest.as_slice()),
                ("extension/media/large.bin", bytes.as_slice()),
            ],
        );

        let file = File::open(&vsix).expect("open vsix");
        let mut archive = zip::ZipArchive::new(file).expect("archive");
        let out_dir = dir.path().join("out");
        fs::create_dir_all(&out_dir).expect("out dir");

        let error = extract_vsix_archive_to_dir(&mut archive, &out_dir, 64, 1024)
            .expect_err("oversized entry should be rejected");
        assert!(error.contains("VSIX entry is too large"), "{error}");
    }

    #[test]
    fn installs_extension_by_swapping_after_staging_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base_dir = dir.path().join("extensions");
        let existing = base_dir.join("acme.demo");
        fs::create_dir_all(&existing).expect("existing dir");
        fs::write(existing.join("package.json"), package_json("1.0.0")).expect("existing package");

        let vsix = dir.path().join("update.vsix");
        let manifest = package_json("2.0.0");
        write_vsix(
            &vsix,
            &[
                ("extension/package.json", manifest.as_bytes()),
                ("extension/media/icon.svg", b"<svg/>"),
            ],
        );

        let installed = install_vscode_extension_from_vsix(&vsix, &base_dir).expect("install");
        assert_eq!(installed.version, "2.0.0");
        assert_eq!(
            extension_info_from_dir(&existing)
                .expect("installed info")
                .version,
            "2.0.0"
        );
        assert!(existing.join("media").join("icon.svg").is_file());
    }

    #[test]
    fn failed_update_keeps_previous_extension_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base_dir = dir.path().join("extensions");
        let existing = base_dir.join("acme.demo");
        fs::create_dir_all(&existing).expect("existing dir");
        fs::write(existing.join("package.json"), package_json("1.0.0")).expect("existing package");
        fs::write(existing.join("keep.txt"), "still here").expect("existing asset");

        let vsix = dir.path().join("bad-update.vsix");
        let manifest = package_json("2.0.0");
        write_vsix(
            &vsix,
            &[
                ("extension/package.json", manifest.as_bytes()),
                ("extension/media", b"not a directory"),
                ("extension/media/icon.svg", b"<svg/>"),
            ],
        );

        let error = install_vscode_extension_from_vsix(&vsix, &base_dir)
            .expect_err("staging failure should abort update");
        assert!(error.contains("create parent directory"), "{error}");
        assert_eq!(
            extension_info_from_dir(&existing)
                .expect("existing info")
                .version,
            "1.0.0"
        );
        assert_eq!(
            fs::read_to_string(existing.join("keep.txt")).expect("existing asset"),
            "still here"
        );
    }
}
