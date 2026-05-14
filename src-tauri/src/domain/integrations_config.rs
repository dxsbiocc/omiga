//! Per-project enable/disable for MCP servers and skills (`.omiga/integrations.json`).

use crate::domain::mcp::names::{mcp_info_from_string, normalize_name_for_mcp};
use crate::domain::skills::SkillEntry;
use crate::domain::tools::ToolSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

const FILE_NAME: &str = "integrations.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationsConfig {
    /// Normalized MCP server segment (same as in `mcp__{here}__tool`).
    #[serde(default)]
    pub disabled_mcp_servers: Vec<String>,
    /// Skill display names (`SkillEntry.name`) to hide from the model and block at invoke.
    #[serde(default)]
    pub disabled_skills: Vec<String>,
}

impl Default for IntegrationsConfig {
    fn default() -> Self {
        Self {
            // Bundled remote MCP servers must be opt-in. If the Paperclip endpoint is
            // temporarily unreachable it can otherwise be probed by chat prewarm and
            // produce noisy rmcp transport errors before the user has chosen to use it.
            disabled_mcp_servers: vec!["paperclip".to_string()],
            disabled_skills: Vec::new(),
        }
    }
}

impl IntegrationsConfig {
    fn mcp_disabled_set(&self) -> HashSet<String> {
        self.disabled_mcp_servers
            .iter()
            .map(|s| normalize_name_for_mcp(s.trim()))
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn skill_disabled_set(&self) -> HashSet<String> {
        self.disabled_skills
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Normalized server segment from `mcp__{here}__` / `tools/list`.
    pub fn is_mcp_normalized_disabled(&self, server_norm: &str) -> bool {
        self.mcp_disabled_set().contains(server_norm)
    }
}

/// Load integrations toggles; missing file → safe defaults.
///
/// Local skills remain enabled. Bundled remote MCP servers are disabled until
/// the user explicitly enables them in Settings, because probing third-party
/// HTTP endpoints during normal chat/session warmup can fail loudly outside the
/// user's control.
pub fn load_integrations_config(project_root: &Path) -> IntegrationsConfig {
    let path = project_root.join(".omiga").join(FILE_NAME);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return IntegrationsConfig::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_integrations_config(
    project_root: &Path,
    config: &IntegrationsConfig,
) -> Result<(), String> {
    let dir = project_root.join(".omiga");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(FILE_NAME);
    let pretty = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty).map_err(|e| e.to_string())
}

/// Drop MCP tools whose server normalized name is disabled.
pub fn filter_mcp_tools_by_integrations(
    tools: Vec<ToolSchema>,
    cfg: &IntegrationsConfig,
) -> Vec<ToolSchema> {
    let disabled = cfg.mcp_disabled_set();
    if disabled.is_empty() {
        return tools;
    }
    tools
        .into_iter()
        .filter(|t| {
            if let Some((srv, _)) = mcp_info_from_string(&t.name) {
                !disabled.contains(&srv)
            } else {
                true
            }
        })
        .collect()
}

/// Keep only skills not listed in `disabled_skills` (match `SkillEntry.name`).
pub fn filter_skill_entries(entries: Vec<SkillEntry>, cfg: &IntegrationsConfig) -> Vec<SkillEntry> {
    let disabled = cfg.skill_disabled_set();
    if disabled.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| !disabled.contains(&e.name))
        .collect()
}

/// Whether this skill display name is disabled.
pub fn is_skill_name_disabled(cfg: &IntegrationsConfig, skill_display_name: &str) -> bool {
    cfg.skill_disabled_set().contains(skill_display_name.trim())
}

/// Whether a merged Omiga MCP server key is disabled (compares normalized tokens).
pub fn is_mcp_config_server_disabled(
    cfg: &IntegrationsConfig,
    server_key_from_merged: &str,
) -> bool {
    cfg.mcp_disabled_set()
        .contains(&normalize_name_for_mcp(server_key_from_merged))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_drops_mcp_tools() {
        let cfg = IntegrationsConfig {
            disabled_mcp_servers: vec!["figma".to_string()],
            disabled_skills: vec![],
        };
        let tools = vec![
            ToolSchema::new("mcp__figma__x", "d", serde_json::json!({})),
            ToolSchema::new("mcp__other__y", "d", serde_json::json!({})),
        ];
        let f = filter_mcp_tools_by_integrations(tools, &cfg);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "mcp__other__y");
    }

    #[test]
    fn default_disables_bundled_remote_paperclip() {
        let cfg = IntegrationsConfig::default();
        assert!(is_mcp_config_server_disabled(&cfg, "paperclip"));
    }
}
