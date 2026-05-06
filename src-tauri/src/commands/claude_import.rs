//! Import Claude Code–style MCP (`mcpServers` JSON) and skills directories into the project.

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::commands::integrations_settings;
use crate::errors::{AppError, FsError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::State;

fn io_err(e: std::io::Error) -> AppError {
    AppError::Fs(FsError::IoError {
        message: e.to_string(),
    })
}

fn validate_skill_directory_name(name: &str) -> CommandResult<&str> {
    let t = name.trim();
    if t.is_empty() || t.contains('/') || t.contains('\\') || t.contains("..") || t.contains('\0') {
        return Err(AppError::Config(
            "Invalid skill directory name (no path separators).".to_string(),
        ));
    }
    Ok(t)
}

fn validate_mcp_server_name(name: &str) -> CommandResult<&str> {
    let t = name.trim();
    if t.is_empty() || t.contains('/') || t.contains('\\') || t.contains("..") || t.contains('\0') {
        return Err(AppError::Config(
            "Invalid MCP server name (no path separators).".to_string(),
        ));
    }
    Ok(t)
}

fn resolve_project_root(project_root: &str) -> CommandResult<PathBuf> {
    let p = PathBuf::from(project_root.trim());
    if p.as_os_str().is_empty() {
        return Err(AppError::Config("Project path is empty.".to_string()));
    }
    p.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("Invalid project path {}: {}", project_root, e),
        })
    })
}

fn read_json_file(path: &Path) -> CommandResult<Value> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("read {}: {}", path.display(), e),
        })
    })?;
    serde_json::from_str(&raw)
        .map_err(|e| AppError::Config(format!("Invalid JSON in {}: {}", path.display(), e)))
}

fn read_json_object_if_exists(path: &Path) -> CommandResult<serde_json::Map<String, Value>> {
    if !path.exists() {
        return Ok(serde_json::Map::new());
    }
    let existing = read_json_file(path)?;
    Ok(existing.as_object().cloned().unwrap_or_default())
}

/// Merge `mcpServers` from `incoming` into `base` (incoming keys overwrite).
fn merge_mcp_servers_objects(base: Option<&Value>, incoming: &Value) -> Value {
    let mut merged = serde_json::Map::new();
    if let Some(b) = base {
        if let Some(obj) = b.as_object() {
            for (k, v) in obj {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    if let Some(obj) = incoming.as_object() {
        for (k, v) in obj {
            merged.insert(k.clone(), v.clone());
        }
    }
    Value::Object(merged)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportMcpMergeResult {
    pub wrote_path: String,
    pub server_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMcpServerInput {
    pub name: String,
    pub kind: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub headers: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub cwd: Option<String>,
}

fn string_map_to_json_object(map: HashMap<String, String>) -> serde_json::Map<String, Value> {
    map.into_iter()
        .filter_map(|(k, v)| {
            let key = k.trim();
            if key.is_empty() {
                None
            } else {
                Some((key.to_string(), Value::String(v)))
            }
        })
        .collect()
}

fn project_mcp_config_value(server: ProjectMcpServerInput) -> CommandResult<(String, Value)> {
    let name = validate_mcp_server_name(&server.name)?.to_string();
    let kind = server.kind.trim().to_ascii_lowercase();
    let mut cfg = serde_json::Map::new();

    match kind.as_str() {
        "stdio" => {
            let command = server.command.unwrap_or_default().trim().to_string();
            if command.is_empty() {
                return Err(AppError::Config(
                    "MCP stdio server requires a launch command.".to_string(),
                ));
            }
            cfg.insert("command".to_string(), Value::String(command));

            let args = server.args.unwrap_or_default();
            cfg.insert(
                "args".to_string(),
                Value::Array(args.into_iter().map(Value::String).collect()),
            );

            let env_obj = string_map_to_json_object(server.env.unwrap_or_default());
            if !env_obj.is_empty() {
                cfg.insert("env".to_string(), Value::Object(env_obj));
            }

            if let Some(cwd) = server.cwd {
                let trimmed = cwd.trim();
                if !trimmed.is_empty() {
                    cfg.insert("cwd".to_string(), Value::String(trimmed.to_string()));
                }
            }
        }
        "http" | "url" | "streamable_http" | "streamable-http" => {
            let url = server.url.unwrap_or_default().trim().to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(AppError::Config(
                    "MCP HTTP server URL must start with http:// or https://.".to_string(),
                ));
            }
            cfg.insert("url".to_string(), Value::String(url));

            let headers_obj = string_map_to_json_object(server.headers.unwrap_or_default());
            if !headers_obj.is_empty() {
                cfg.insert("headers".to_string(), Value::Object(headers_obj));
            }
        }
        _ => {
            return Err(AppError::Config(
                "MCP server kind must be \"stdio\" or \"http\".".to_string(),
            ));
        }
    }

    Ok((name, Value::Object(cfg)))
}

fn project_mcp_tombstone_value(name: &str) -> CommandResult<(String, Value)> {
    let name = validate_mcp_server_name(name)?.to_string();
    Ok((name, serde_json::json!({ "disabled": true })))
}

async fn write_project_mcp_servers(
    app_state: &OmigaAppState,
    project_root: &str,
    server_patch: serde_json::Map<String, Value>,
) -> CommandResult<ImportMcpMergeResult> {
    let root = resolve_project_root(project_root)?;
    let dest = root.join(".omiga").join("mcp.json");
    let parent = dest.parent().ok_or_else(|| {
        AppError::Config("Could not determine parent directory for .omiga/mcp.json.".to_string())
    })?;
    tokio::fs::create_dir_all(parent).await.map_err(io_err)?;

    let mut out_obj = read_json_object_if_exists(&dest)?;
    let existing_servers = out_obj.get("mcpServers").cloned();
    let incoming_servers = Value::Object(server_patch);
    let merged_servers = merge_mcp_servers_objects(existing_servers.as_ref(), &incoming_servers);
    let n = merged_servers.as_object().map(|o| o.len()).unwrap_or(0);
    out_obj.insert("mcpServers".to_string(), merged_servers);

    let out = Value::Object(out_obj);
    let pretty = serde_json::to_string_pretty(&out)
        .map_err(|e| AppError::Config(format!("serialize project MCP JSON: {}", e)))?;

    tokio::fs::write(&dest, pretty.as_bytes())
        .await
        .map_err(io_err)?;

    if let Ok(cache_key) = integrations_settings::resolve_project_root(project_root) {
        integrations_settings::invalidate_integrations_catalog_cache(app_state, &cache_key);
    }

    Ok(ImportMcpMergeResult {
        wrote_path: dest.display().to_string(),
        server_count: n,
    })
}

/// Add or update one MCP server in `<project_root>/.omiga/mcp.json`.
///
/// Project-level entries intentionally override bundled/user/plugin entries with the same server
/// name, matching the normal Omiga MCP merge order.
#[tauri::command]
pub async fn upsert_project_mcp_server(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    server: ProjectMcpServerInput,
) -> CommandResult<ImportMcpMergeResult> {
    let (name, cfg) = project_mcp_config_value(server)?;
    let mut patch = serde_json::Map::new();
    patch.insert(name, cfg);
    write_project_mcp_servers(&app_state, &project_root, patch).await
}

/// Hide/remove one MCP server for the current project by writing a project-level tombstone.
///
/// This removes project-owned servers and also lets a project hide bundled/user/plugin servers
/// without mutating global files.
#[tauri::command]
pub async fn delete_project_mcp_server(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    name: String,
) -> CommandResult<ImportMcpMergeResult> {
    let (name, tombstone) = project_mcp_tombstone_value(&name)?;
    let mut patch = serde_json::Map::new();
    patch.insert(name, tombstone);
    write_project_mcp_servers(&app_state, &project_root, patch).await
}

/// Merge `mcpServers` from `source_path` (Claude Code / Cursor `mcp.json` shape) into
/// `<project_root>/.omiga/mcp.json`. Existing file is preserved for unrelated keys; `mcpServers` entries
/// are merged with source winning on name clash.
#[tauri::command]
pub async fn import_merge_project_mcp_json(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    source_path: String,
) -> CommandResult<ImportMcpMergeResult> {
    let root = resolve_project_root(&project_root)?;
    let src = PathBuf::from(source_path.trim());
    if !src.is_file() {
        return Err(AppError::Fs(FsError::NotFound {
            path: src.display().to_string(),
        }));
    }

    let incoming_root = read_json_file(&src)?;
    let incoming_servers = incoming_root
        .get("mcpServers")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    if !incoming_servers.is_object() {
        return Err(AppError::Config(
            "Source file must contain an object \"mcpServers\".".to_string(),
        ));
    }

    let dest = root.join(".omiga").join("mcp.json");
    let parent = dest.parent().ok_or_else(|| {
        AppError::Config("Could not determine parent directory for .omiga/mcp.json.".to_string())
    })?;
    tokio::fs::create_dir_all(parent).await.map_err(io_err)?;

    let mut out_obj = serde_json::Map::new();
    if dest.exists() {
        let existing: Value = read_json_file(&dest)?;
        if let Some(o) = existing.as_object() {
            for (k, v) in o {
                if k != "mcpServers" {
                    out_obj.insert(k.clone(), v.clone());
                }
            }
        }
    }

    let existing_servers = if dest.exists() {
        read_json_file(&dest)?.get("mcpServers").cloned()
    } else {
        None
    };

    let merged_servers = merge_mcp_servers_objects(existing_servers.as_ref(), &incoming_servers);
    let n = merged_servers.as_object().map(|o| o.len()).unwrap_or(0);
    out_obj.insert("mcpServers".to_string(), merged_servers);

    let out = Value::Object(out_obj);
    let pretty = serde_json::to_string_pretty(&out)
        .map_err(|e| AppError::Config(format!("serialize merged MCP JSON: {}", e)))?;

    tokio::fs::write(&dest, pretty.as_bytes())
        .await
        .map_err(io_err)?;

    if let Ok(cache_key) = integrations_settings::resolve_project_root(&project_root) {
        integrations_settings::invalidate_integrations_catalog_cache(&app_state, &cache_key);
    }

    Ok(ImportMcpMergeResult {
        wrote_path: dest.display().to_string(),
        server_count: n,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSkillsResult {
    pub dest_skills_root: String,
    pub imported_skill_dirs: Vec<String>,
}

fn copy_skill_tree_sync(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::fs;
    for e in walkdir::WalkDir::new(src).min_depth(1) {
        let e = e?;
        let p = e.path();
        let rel = p
            .strip_prefix(src)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "strip_prefix"))?;
        let target = dst.join(rel);
        if e.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(par) = target.parent() {
                fs::create_dir_all(par)?;
            }
            fs::copy(p, &target)?;
        }
    }
    Ok(())
}

/// Import target: user `~/.omiga/skills` or project `<project>/.omiga/skills`.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillsImportTarget {
    UserOmiga,
    ProjectOmiga,
}

fn resolve_dest_omiga_skills(
    project_root: &str,
    target: SkillsImportTarget,
) -> CommandResult<PathBuf> {
    match target {
        SkillsImportTarget::UserOmiga => {
            let home = dirs::home_dir().ok_or_else(|| {
                AppError::Config(
                    "Could not resolve home directory for ~/.omiga/skills.".to_string(),
                )
            })?;
            Ok(home.join(".omiga").join("skills"))
        }
        SkillsImportTarget::ProjectOmiga => {
            let root = resolve_project_root(project_root)?;
            Ok(root.join(".omiga").join("skills"))
        }
    }
}

/// Copy each immediate subfolder of `source_skills_dir` that contains `SKILL.md` into
/// `~/.omiga/skills` or `<project_root>/.omiga/skills` (overwrites destination if present).
#[tauri::command]
pub async fn import_skills_from_directory(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    source_skills_dir: String,
    target: SkillsImportTarget,
) -> CommandResult<ImportSkillsResult> {
    let src_root = PathBuf::from(source_skills_dir.trim());
    if !src_root.is_dir() {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: src_root.display().to_string(),
        }));
    }

    let dest_root = resolve_dest_omiga_skills(&project_root, target)?;
    tokio::fs::create_dir_all(&dest_root)
        .await
        .map_err(io_err)?;

    let mut imported = Vec::new();
    let mut rd = tokio::fs::read_dir(&src_root).await.map_err(io_err)?;
    while let Some(e) = rd.next_entry().await.map_err(io_err)? {
        let path = e.path();
        let meta = e.metadata().await.map_err(io_err)?;
        if !meta.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let dest = dest_root.join(&name);
        if dest.exists() {
            tokio::fs::remove_dir_all(&dest).await.map_err(io_err)?;
        }
        let src_p = path.clone();
        let dst_p = dest.clone();
        tokio::task::spawn_blocking(move || copy_skill_tree_sync(&src_p, &dst_p))
            .await
            .map_err(|e| AppError::Config(format!("copy task: {}", e)))?
            .map_err(io_err)?;
        imported.push(name);
    }

    imported.sort();

    if let Ok(cache_key) = integrations_settings::resolve_project_root(&project_root) {
        integrations_settings::invalidate_integrations_catalog_cache(&app_state, &cache_key);
    }

    Ok(ImportSkillsResult {
        dest_skills_root: dest_root.display().to_string(),
        imported_skill_dirs: imported,
    })
}

/// Copy all `skill-dir/SKILL.md` from Claude’s default user skills directory (`~/.claude/skills` or
/// `$CLAUDE_CONFIG_DIR/skills`) into `~/.omiga/skills` or `<project>/.omiga/skills`.
#[tauri::command]
pub async fn import_claude_default_user_skills(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    target: SkillsImportTarget,
) -> CommandResult<ImportSkillsResult> {
    let meta = get_claude_default_paths()?;
    let src = PathBuf::from(&meta.default_user_skills_dir);
    if !src.is_dir() {
        return Err(AppError::Config(format!(
            "Claude skills directory not found: {} (install skills in Claude Code first, or copy manually).",
            src.display()
        )));
    }
    import_skills_from_directory(app_state, project_root, src.display().to_string(), target).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OmigaImportedSkillEntry {
    pub directory_name: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OmigaImportedSkillsList {
    pub user_skills: Vec<OmigaImportedSkillEntry>,
    pub project_skills: Vec<OmigaImportedSkillEntry>,
}

async fn list_skill_subdirs_with_skill_md(
    root: &Path,
) -> CommandResult<Vec<OmigaImportedSkillEntry>> {
    let mut out: Vec<OmigaImportedSkillEntry> = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut rd = tokio::fs::read_dir(root).await.map_err(io_err)?;
    while let Some(e) = rd.next_entry().await.map_err(io_err)? {
        let path = e.path();
        let meta = e.metadata().await.map_err(io_err)?;
        if !meta.is_dir() {
            continue;
        }
        if !path.join("SKILL.md").is_file() {
            continue;
        }
        let directory_name = e.file_name().to_string_lossy().to_string();
        out.push(OmigaImportedSkillEntry {
            directory_name,
            path: path.display().to_string(),
        });
    }
    out.sort_by(|a, b| a.directory_name.cmp(&b.directory_name));
    Ok(out)
}

/// List skill folders under `~/.omiga/skills` and `<project>/.omiga/skills` (each must contain `SKILL.md`).
#[tauri::command]
pub async fn list_omiga_imported_skills(
    project_root: String,
) -> CommandResult<OmigaImportedSkillsList> {
    let user_root = resolve_dest_omiga_skills(&project_root, SkillsImportTarget::UserOmiga)?;
    let proj_root = resolve_dest_omiga_skills(&project_root, SkillsImportTarget::ProjectOmiga)?;
    let user_skills = list_skill_subdirs_with_skill_md(&user_root).await?;
    let project_skills = list_skill_subdirs_with_skill_md(&proj_root).await?;
    Ok(OmigaImportedSkillsList {
        user_skills,
        project_skills,
    })
}

/// Remove one imported skill directory under `~/.omiga/skills` or `<project>/.omiga/skills`.
#[tauri::command]
pub async fn remove_omiga_imported_skill(
    app_state: State<'_, OmigaAppState>,
    project_root: String,
    directory_name: String,
    target: SkillsImportTarget,
) -> CommandResult<()> {
    let name = validate_skill_directory_name(&directory_name)?;
    let dest_root = resolve_dest_omiga_skills(&project_root, target)?;
    let dir = dest_root.join(name);
    if !dir.exists() {
        return Ok(());
    }
    let dest_canon = dest_root.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", dest_root.display(), e),
        })
    })?;
    let dir_canon = dir.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", dir.display(), e),
        })
    })?;
    if !dir_canon.starts_with(&dest_canon) {
        return Err(AppError::Config(
            "Refusing to delete path outside Omiga skills root.".to_string(),
        ));
    }
    tokio::fs::remove_dir_all(&dir_canon)
        .await
        .map_err(io_err)?;

    if let Ok(cache_key) = integrations_settings::resolve_project_root(&project_root) {
        integrations_settings::invalidate_integrations_catalog_cache(&app_state, &cache_key);
    }

    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDefaultPaths {
    /// Resolved `CLAUDE_CONFIG_DIR` or `~/.claude`
    pub claude_config_home: String,
    /// `claude_config_home/skills`
    pub default_user_skills_dir: String,
    pub env_claude_config_dir_set: bool,
    /// `~/.claude.json` (or `$CLAUDE_CONFIG_DIR/.claude.json`) — Claude Code global config with MCP.
    pub global_claude_config: String,
    /// Whether the global Claude config file exists on disk.
    pub global_claude_config_exists: bool,
}

#[tauri::command]
pub fn get_claude_default_paths() -> CommandResult<ClaudeDefaultPaths> {
    let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
        .ok_or_else(|| {
            AppError::Config("Could not resolve home for Claude config path.".to_string())
        })?;

    let env_set = std::env::var_os("CLAUDE_CONFIG_DIR").is_some();
    let skills = claude_home.join("skills");

    // Global Claude Code config: $CLAUDE_CONFIG_DIR/.claude.json or ~/.claude.json
    let global_cfg = if env_set {
        PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap()).join(".claude.json")
    } else {
        dirs::home_dir()
            .map(|h| h.join(".claude.json"))
            .unwrap_or_else(|| PathBuf::from(".claude.json"))
    };
    let global_cfg_exists = global_cfg.is_file();

    Ok(ClaudeDefaultPaths {
        claude_config_home: claude_home.display().to_string(),
        default_user_skills_dir: skills.display().to_string(),
        env_claude_config_dir_set: env_set,
        global_claude_config: global_cfg.display().to_string(),
        global_claude_config_exists: global_cfg_exists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_servers_incoming_wins() {
        let base = json!({"a": 1, "b": 2});
        let inc = json!({"b": 9, "c": 3});
        let m = merge_mcp_servers_objects(Some(&base), &inc);
        assert_eq!(m["a"], 1);
        assert_eq!(m["b"], 9);
        assert_eq!(m["c"], 3);
    }

    #[test]
    fn project_mcp_config_value_builds_stdio_and_http() {
        let mut env = std::collections::HashMap::new();
        env.insert("API_KEY".to_string(), "secret".to_string());

        let (name, stdio) = project_mcp_config_value(ProjectMcpServerInput {
            name: "paperclip-local".to_string(),
            kind: "stdio".to_string(),
            command: Some("uvx".to_string()),
            args: Some(vec!["paperclip-mcp".to_string()]),
            env: Some(env),
            headers: None,
            url: None,
            cwd: Some("./tools".to_string()),
        })
        .expect("stdio input");
        assert_eq!(name, "paperclip-local");
        assert_eq!(stdio["command"], "uvx");
        assert_eq!(stdio["args"], json!(["paperclip-mcp"]));
        assert_eq!(stdio["env"]["API_KEY"], "secret");
        assert_eq!(stdio["cwd"], "./tools");

        let (_, http) = project_mcp_config_value(ProjectMcpServerInput {
            name: "paperclip".to_string(),
            kind: "http".to_string(),
            command: None,
            args: None,
            env: None,
            headers: Some(HashMap::from([(
                "Authorization".to_string(),
                "Bearer ${PAPERCLIP_TOKEN}".to_string(),
            )])),
            url: Some("https://example.com/mcp".to_string()),
            cwd: None,
        })
        .expect("http input");
        assert_eq!(http["url"], "https://example.com/mcp");
        assert_eq!(http["headers"]["Authorization"], "Bearer ${PAPERCLIP_TOKEN}");
    }

    #[test]
    fn project_mcp_tombstone_value_marks_disabled() {
        let (name, value) = project_mcp_tombstone_value("paperclip").expect("tombstone");
        assert_eq!(name, "paperclip");
        assert_eq!(value["disabled"], true);
    }
}
