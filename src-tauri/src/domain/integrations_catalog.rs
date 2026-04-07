//! Serializable MCP + skills catalog for the Settings UI and warm-cache storage.

use crate::domain::skills::SkillSource;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCatalogEntry {
    pub wire_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCatalogEntry {
    pub config_key: String,
    pub normalized_key: String,
    pub enabled: bool,
    /// When tool discovery failed (timeout, handshake error, etc.); UI can show error state.
    pub list_tools_error: Option<String>,
    pub tools: Vec<McpToolCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalogEntry {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Where the skill was loaded from (for UI labeling).
    pub source: SkillSource,
    /// Folder basename under the skills root (matches `remove_omiga_imported_skill`).
    pub directory_name: String,
    /// Absolute path to `SKILL.md` (for Settings preview).
    pub skill_md_path: String,
    /// Skill lives under `~/.omiga/skills` or `<project>/.omiga/skills` — safe to delete that folder.
    pub can_uninstall_omiga_copy: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationsCatalog {
    pub mcp_servers: Vec<McpServerCatalogEntry>,
    pub skills: Vec<SkillCatalogEntry>,
}
