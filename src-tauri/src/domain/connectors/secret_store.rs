//! Connector secret storage.
//!
//! User-level connector config stores metadata only. OAuth/device-flow tokens live behind this
//! abstraction so native tools can read them without writing secret material to
//! `~/.omiga/connectors/config.json`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const KEYCHAIN_SERVICE: &str = "dev.omiga.connector";

fn normalize_part(value: &str, field: &str) -> Result<String, String> {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '/'], "_")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-' || *ch == '.')
        .collect::<String>();
    if normalized.is_empty() || normalized == "." || normalized.contains("..") {
        return Err(format!("invalid connector secret {field} `{value}`"));
    }
    Ok(normalized)
}

fn account_name(connector_id: &str, secret_name: &str) -> Result<String, String> {
    Ok(format!(
        "{}:{}",
        normalize_part(connector_id, "connector id")?,
        normalize_part(secret_name, "name")?
    ))
}

fn file_store_root() -> Option<PathBuf> {
    std::env::var("OMIGA_CONNECTOR_SECRET_STORE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn file_secret_path(connector_id: &str, secret_name: &str) -> Result<Option<PathBuf>, String> {
    let Some(root) = file_store_root() else {
        return Ok(None);
    };
    Ok(Some(
        root.join(normalize_part(connector_id, "connector id")?)
            .join(format!("{}.secret", normalize_part(secret_name, "name")?)),
    ))
}

pub(crate) fn store_connector_secret(
    connector_id: &str,
    secret_name: &str,
    value: &str,
) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("connector secret value is empty".to_string());
    }
    if let Some(path) = file_secret_path(connector_id, secret_name)? {
        return write_file_secret(&path, value);
    }
    store_platform_secret(connector_id, secret_name, value)
}

pub(crate) fn read_connector_secret(
    connector_id: &str,
    secret_name: &str,
) -> Result<Option<String>, String> {
    if let Some(path) = file_secret_path(connector_id, secret_name)? {
        return read_file_secret(&path);
    }
    read_platform_secret(connector_id, secret_name)
}

pub(crate) fn delete_connector_secret(connector_id: &str, secret_name: &str) -> Result<(), String> {
    if let Some(path) = file_secret_path(connector_id, secret_name)? {
        return delete_file_secret(&path);
    }
    delete_platform_secret(connector_id, secret_name)
}

fn write_file_secret(path: &Path, value: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create connector secret dir: {err}"))?;
    }
    fs::write(path, value).map_err(|err| format!("write connector secret: {err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("chmod connector secret: {err}"))?;
    }
    Ok(())
}

fn read_file_secret(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("read connector secret: {err}")),
    }
}

fn delete_file_secret(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("delete connector secret: {err}")),
    }
}

#[cfg(target_os = "macos")]
fn store_platform_secret(connector_id: &str, secret_name: &str, value: &str) -> Result<(), String> {
    let account = account_name(connector_id, secret_name)?;
    // macOS' `security add-generic-password` does not provide a non-interactive stdin mode for
    // the password field. Prefer the file store in tests via OMIGA_CONNECTOR_SECRET_STORE_DIR; in
    // the desktop app this still keeps tokens out of Omiga config and in the system Keychain.
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            &account,
            "-s",
            KEYCHAIN_SERVICE,
            "-w",
            value,
            "-U",
        ])
        .output()
        .map_err(|err| format!("run macOS Keychain security command: {err}"))?;
    ensure_success(output, "store connector secret in macOS Keychain")
}

#[cfg(target_os = "macos")]
fn read_platform_secret(connector_id: &str, secret_name: &str) -> Result<Option<String>, String> {
    let account = account_name(connector_id, secret_name)?;
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            &account,
            "-s",
            KEYCHAIN_SERVICE,
            "-w",
        ])
        .output()
        .map_err(|err| format!("run macOS Keychain security command: {err}"))?;
    if output.status.success() {
        return Ok(
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                .filter(|value| !value.is_empty()),
        );
    }
    if is_secret_not_found(&output) {
        return Ok(None);
    }
    Err(format_command_error(
        "read connector secret from macOS Keychain",
        &output,
    ))
}

#[cfg(target_os = "macos")]
fn delete_platform_secret(connector_id: &str, secret_name: &str) -> Result<(), String> {
    let account = account_name(connector_id, secret_name)?;
    let output = Command::new("security")
        .args([
            "delete-generic-password",
            "-a",
            &account,
            "-s",
            KEYCHAIN_SERVICE,
        ])
        .output()
        .map_err(|err| format!("run macOS Keychain security command: {err}"))?;
    if output.status.success() || is_secret_not_found(&output) {
        return Ok(());
    }
    Err(format_command_error(
        "delete connector secret from macOS Keychain",
        &output,
    ))
}

#[cfg(not(target_os = "macos"))]
fn store_platform_secret(
    _connector_id: &str,
    _secret_name: &str,
    _value: &str,
) -> Result<(), String> {
    Err("OAuth connector login requires a platform secret store; set OMIGA_CONNECTOR_SECRET_STORE_DIR for development/testing on this OS".to_string())
}

#[cfg(not(target_os = "macos"))]
fn read_platform_secret(_connector_id: &str, _secret_name: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
fn delete_platform_secret(_connector_id: &str, _secret_name: &str) -> Result<(), String> {
    Ok(())
}

fn ensure_success(output: Output, action: &str) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format_command_error(action, &output))
    }
}

fn is_secret_not_found(output: &Output) -> bool {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    text.contains("could not be found") || text.contains("not found") || text.contains("-25300")
}

fn format_command_error(action: &str, output: &Output) -> String {
    let detail = String::from_utf8_lossy(if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    })
    .trim()
    .to_string();
    if detail.is_empty() {
        format!("{action}: command exited with {}", output.status)
    } else {
        format!("{action}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static SECRET_STORE_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn file_secret_store_round_trips_without_config_leak() {
        let _guard = SECRET_STORE_ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("OMIGA_CONNECTOR_SECRET_STORE_DIR", dir.path());
        store_connector_secret("github", "oauth_access_token", "gho_secret").unwrap();
        assert_eq!(
            read_connector_secret("github", "oauth_access_token").unwrap(),
            Some("gho_secret".to_string())
        );
        delete_connector_secret("github", "oauth_access_token").unwrap();
        assert_eq!(
            read_connector_secret("github", "oauth_access_token").unwrap(),
            None
        );
        std::env::remove_var("OMIGA_CONNECTOR_SECRET_STORE_DIR");
    }

    #[test]
    fn invalid_secret_parts_are_rejected() {
        let err = account_name("..", "oauth_access_token").unwrap_err();
        assert!(err.contains("invalid connector secret"));
    }

    #[test]
    fn missing_file_secret_is_empty() {
        let _guard = SECRET_STORE_ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("OMIGA_CONNECTOR_SECRET_STORE_DIR", dir.path());
        assert_eq!(
            read_connector_secret("github", "oauth_access_token").unwrap(),
            None
        );
        std::env::remove_var("OMIGA_CONNECTOR_SECRET_STORE_DIR");
    }

    #[test]
    fn not_found_detector_accepts_macos_security_code() {
        let output = Output {
            status: exit_status(44),
            stdout: Vec::new(),
            stderr: b"security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain. (-25300)".to_vec(),
        };
        assert!(is_secret_not_found(&output));
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: u32) -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code)
    }
}
