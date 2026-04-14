//! Detect locally installed Python virtual environments (conda, venv, pyenv).
//! Used by the frontend to populate the environment picker sub-menu.

use serde::Serialize;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct LocalVenvInfo {
    /// `"conda"` | `"venv"` | `"pyenv"`
    pub kind: String,
    /// Display name shown in the UI
    pub label: String,
    /// The value stored in the store: conda env name, venv dir path, pyenv version
    pub name: String,
}

// ─── conda ────────────────────────────────────────────────────────────────────

async fn list_conda() -> Vec<LocalVenvInfo> {
    let out = Command::new("conda")
        .args(["env", "list", "--json"])
        .output()
        .await;
    let Ok(out) = out else { return vec![] };
    if !out.status.success() {
        return vec![];
    }
    let json: serde_json::Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let envs = match json.get("envs").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    envs.iter()
        .filter_map(|v| v.as_str())
        .map(|path| {
            // The last path component is the env name; base env path → "base"
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string());
            LocalVenvInfo {
                kind: "conda".into(),
                label: format!("conda: {}", name),
                name,
            }
        })
        .collect()
}

// ─── pyenv ────────────────────────────────────────────────────────────────────

async fn list_pyenv() -> Vec<LocalVenvInfo> {
    let out = Command::new("pyenv").args(["versions", "--bare"]).output().await;
    let Ok(out) = out else { return vec![] };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|ver| LocalVenvInfo {
            kind: "pyenv".into(),
            label: format!("pyenv: {}", ver),
            name: ver.to_string(),
        })
        .collect()
}

// ─── venv dirs inside cwd ─────────────────────────────────────────────────────

fn list_venv_dirs(project_root: &str) -> Vec<LocalVenvInfo> {
    // Common venv directory names to probe.
    let candidates = [".venv", "venv", "env", ".env"];
    let root = std::path::Path::new(project_root);
    candidates
        .iter()
        .filter_map(|name| {
            let dir = root.join(name);
            // A venv has activate script and pyvenv.cfg
            let activate = dir.join("bin").join("activate");
            if activate.exists() {
                Some(LocalVenvInfo {
                    kind: "venv".into(),
                    label: format!("venv: {}", name),
                    name: dir.to_string_lossy().into_owned(),
                })
            } else {
                None
            }
        })
        .collect()
}

// ─── Command ──────────────────────────────────────────────────────────────────

/// List all detectable local Python virtual environments.
/// Checks conda, pyenv, and common `.venv`/`venv` directories under `project_root`.
#[tauri::command]
pub async fn list_local_venvs(project_root: String) -> Vec<LocalVenvInfo> {
    let (conda, pyenv) = tokio::join!(list_conda(), list_pyenv());
    let venv = list_venv_dirs(&project_root);
    let mut result = Vec::new();
    result.extend(conda);
    result.extend(pyenv);
    result.extend(venv);
    result
}
