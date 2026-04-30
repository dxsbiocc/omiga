//! Permission deny rules — aligned with `filterToolsByDenyRules` / `toolMatchesRule` in
//! `src/tools.ts` + `src/utils/permissions/permissions.ts` + `permissionRuleParser.ts`.
//!
//! Only **blanket** denies (no `ruleContent`) affect tool-list filtering; rules with
//! `Tool(content)` are ignored for whole-tool matching, matching TS `toolMatchesRule`.

use crate::domain::mcp::names::mcp_info_from_string;
use crate::domain::tools::ToolSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Parsed permission rule (`Tool` or `Tool(content)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRuleValue {
    pub tool_name: String,
    pub rule_content: Option<String>,
}

/// Legacy Claude Code tool names → current canonical names (see `permissionRuleParser.ts`).
fn normalize_legacy_tool_name(name: &str) -> String {
    match name {
        "Task" => "Agent".to_string(),
        "KillShell" => "TaskStop".to_string(),
        "AgentOutputTool" | "BashOutputTool" => "TaskOutput".to_string(),
        _ => name.to_string(),
    }
}

/// Map Claude-style wire names and Omiga `from_json_str` aliases to the Omiga schema name
/// used in `ToolSchema::name` (for built-ins). MCP tools (`mcp__…`) are returned unchanged.
#[must_use]
pub fn canonical_permission_tool_name(name: &str) -> String {
    if name.starts_with("mcp__") {
        return name.to_string();
    }
    let n = normalize_legacy_tool_name(name);
    match n.as_str() {
        "bash" | "Bash" => "bash".to_string(),
        "file_read" | "Read" | "FileRead" => "file_read".to_string(),
        "file_write" | "Write" | "FileWriteTool" | "FileWrite" => "file_write".to_string(),
        "file_edit"
        | "Edit"
        | "FileEditTool"
        | "FileEdit"
        | "MultiEdit"
        | "str_replace_based_edit_tool" => "file_edit".to_string(),
        "ripgrep" | "Ripgrep" | "grep" | "Grep" => "ripgrep".to_string(),
        "glob" | "Glob" => "glob".to_string(),
        "fetch" | "Fetch" | "web_fetch" | "WebFetch" | "webFetch" => "fetch".to_string(),
        "search" | "Search" | "web_search" | "WebSearch" | "webSearch" => "search".to_string(),
        "todo_write" | "TodoWrite" => "todo_write".to_string(),
        "notebook_edit" | "NotebookEdit" => "notebook_edit".to_string(),
        "sleep" | "Sleep" => "sleep".to_string(),
        "ask_user_question" | "AskUserQuestion" => "ask_user_question".to_string(),
        "list_mcp_resources" | "ListMcpResourcesTool" | "ListMcpResources" => {
            "list_mcp_resources".to_string()
        }
        "read_mcp_resource" | "ReadMcpResourceTool" | "ReadMcpResource" => {
            "read_mcp_resource".to_string()
        }
        "Agent" | "agent" | "Task" => "agent".to_string(),
        "SendUserMessage" | "Brief" | "send_user_message" => "send_user_message".to_string(),
        "ExitPlanMode" | "exit_plan_mode" | "ExitPlanModeTool" => "exit_plan_mode".to_string(),
        "EnterPlanMode" | "enter_plan_mode" | "EnterPlanModeTool" => "enter_plan_mode".to_string(),
        "TaskStop" | "task_stop" | "KillShell" => "task_stop".to_string(),
        "TaskOutput" | "task_output" => "task_output".to_string(),
        "ToolSearch" | "tool_search" => "tool_search".to_string(),
        "TaskCreate" | "task_create" => "task_create".to_string(),
        "TaskGet" | "task_get" => "task_get".to_string(),
        "TaskList" | "task_list" => "task_list".to_string(),
        "TaskUpdate" | "task_update" => "task_update".to_string(),
        "workflow" | "Workflow" => "workflow".to_string(),
        "list_skills" | "ListSkillsTool" | "ListSkills" => "list_skills".to_string(),
        "skills_list" => "skills_list".to_string(),
        "skill_view" => "skill_view".to_string(),
        "skill_manage" => "skill_manage".to_string(),
        "skill_config" => "skill_config".to_string(),
        "skill" | "Skill" | "SkillTool" => "skill".to_string(),
        _ => n,
    }
}

fn find_first_unescaped_char(s: &str, ch: char) -> Option<usize> {
    for (i, c) in s.char_indices() {
        if c == ch {
            let mut backslashes = 0usize;
            let mut j = i;
            while j > 0 {
                j -= 1;
                if s.as_bytes()[j] == b'\\' {
                    backslashes += 1;
                } else {
                    break;
                }
            }
            if backslashes % 2 == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn find_last_unescaped_char(s: &str, ch: char) -> Option<usize> {
    for (i, c) in s.char_indices().rev() {
        if c == ch {
            let mut backslashes = 0usize;
            let mut j = i;
            while j > 0 {
                j -= 1;
                if s.as_bytes()[j] == b'\\' {
                    backslashes += 1;
                } else {
                    break;
                }
            }
            if backslashes % 2 == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn unescape_rule_content(raw: &str) -> String {
    raw.replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\\\", "\\")
}

/// Parse a permission rule string (`permissionRuleValueFromString` in TS).
#[must_use]
pub fn permission_rule_value_from_string(rule_string: &str) -> PermissionRuleValue {
    let open = match find_first_unescaped_char(rule_string, '(') {
        Some(i) => i,
        None => {
            return PermissionRuleValue {
                tool_name: normalize_legacy_tool_name(rule_string),
                rule_content: None,
            };
        }
    };
    let close = match find_last_unescaped_char(rule_string, ')') {
        Some(i) => i,
        None => {
            return PermissionRuleValue {
                tool_name: normalize_legacy_tool_name(rule_string),
                rule_content: None,
            };
        }
    };
    if close <= open || close != rule_string.len().saturating_sub(1) {
        return PermissionRuleValue {
            tool_name: normalize_legacy_tool_name(rule_string),
            rule_content: None,
        };
    }
    let tool_name = &rule_string[..open];
    if tool_name.is_empty() {
        return PermissionRuleValue {
            tool_name: normalize_legacy_tool_name(rule_string),
            rule_content: None,
        };
    }
    let raw_content = &rule_string[open + 1..close];
    if raw_content.is_empty() || raw_content == "*" {
        return PermissionRuleValue {
            tool_name: normalize_legacy_tool_name(tool_name),
            rule_content: None,
        };
    }
    PermissionRuleValue {
        tool_name: normalize_legacy_tool_name(tool_name),
        rule_content: Some(unescape_rule_content(raw_content)),
    }
}

fn names_match_blanket_deny(rule_tool: &str, actual_tool: &str) -> bool {
    if rule_tool == actual_tool {
        return true;
    }
    if rule_tool.starts_with("mcp__") || actual_tool.starts_with("mcp__") {
        return false;
    }
    canonical_permission_tool_name(rule_tool) == canonical_permission_tool_name(actual_tool)
}

/// Whether a blanket deny rule matches a tool wire name (built-in or `mcp__server__tool`).
#[must_use]
pub fn blanket_deny_rule_matches(rule: &PermissionRuleValue, actual_tool_name: &str) -> bool {
    if rule.rule_content.is_some() {
        return false;
    }
    let rt = rule.tool_name.as_str();
    if names_match_blanket_deny(rt, actual_tool_name) {
        return true;
    }
    let rule_info = mcp_info_from_string(rt);
    let tool_info = mcp_info_from_string(actual_tool_name);
    match (rule_info, tool_info) {
        (Some((rs, rtool)), Some((ts, ttool))) => {
            rs == ts && (rtool.is_none() || rtool.as_deref() == Some("*")) && ttool.is_some()
        }
        _ => false,
    }
}

#[must_use]
pub fn tool_denied_by_any_rule(actual_tool_name: &str, deny_rule_strings: &[String]) -> bool {
    deny_rule_strings.iter().any(|s| {
        let v = permission_rule_value_from_string(s);
        blanket_deny_rule_matches(&v, actual_tool_name)
    })
}

/// One deny rule with the settings file it came from (for logs).
#[derive(Debug, Clone)]
pub struct DenyRuleEntry {
    pub source: PathBuf,
    pub rule: String,
}

/// First deny entry that matches `actual_tool_name` (blanket rules only), same order as merged list.
#[must_use]
pub fn matching_deny_entry<'a>(
    actual_tool_name: &str,
    entries: &'a [DenyRuleEntry],
) -> Option<&'a DenyRuleEntry> {
    entries.iter().find(|e| {
        let v = permission_rule_value_from_string(&e.rule);
        blanket_deny_rule_matches(&v, actual_tool_name)
    })
}

#[must_use]
pub fn filter_tool_schemas_by_deny_rule_entries(
    schemas: Vec<ToolSchema>,
    entries: &[DenyRuleEntry],
) -> Vec<ToolSchema> {
    schemas
        .into_iter()
        .filter(|t| {
            if let Some(hit) = matching_deny_entry(&t.name, entries) {
                tracing::debug!(
                    target: "omiga::permissions",
                    tool = %t.name,
                    matched_rule = %hit.rule,
                    source = %hit.source.display(),
                    "tool filtered from LLM tool list by permissions.deny"
                );
                return false;
            }
            true
        })
        .collect()
}

/// Backward-compatible filter using raw rule strings (no per-rule source in logs).
#[must_use]
pub fn filter_tool_schemas_by_deny_rules(
    schemas: Vec<ToolSchema>,
    deny_rule_strings: &[String],
) -> Vec<ToolSchema> {
    let parsed: Vec<PermissionRuleValue> = deny_rule_strings
        .iter()
        .map(|s| permission_rule_value_from_string(s))
        .collect();
    schemas
        .into_iter()
        .filter(|t| !parsed.iter().any(|r| blanket_deny_rule_matches(r, &t.name)))
        .collect()
}

#[derive(Deserialize)]
struct ClaudeSettingsFile {
    permissions: Option<ClaudePermissions>,
}

#[derive(Deserialize)]
struct ClaudePermissions {
    deny: Option<Vec<String>>,
}

fn settings_paths_to_scan(project_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".claude/settings.json"));
        paths.push(home.join(".claude/settings.local.json"));
    }
    paths.push(project_root.join(".claude/settings.json"));
    paths.push(project_root.join(".claude/settings.local.json"));
    paths
}

/// Merge `permissions.deny` from Claude Code-style JSON settings (user + project), with source paths.
#[must_use]
pub fn load_merged_permission_deny_rule_entries(project_root: &Path) -> Vec<DenyRuleEntry> {
    let mut out = Vec::new();
    for p in settings_paths_to_scan(project_root) {
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let f: ClaudeSettingsFile = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    target: "omiga::permissions",
                    path = %p.display(),
                    error = %e,
                    "failed to parse Claude settings JSON; permissions from this file ignored"
                );
                continue;
            }
        };
        let Some(perms) = f.permissions else {
            continue;
        };
        let Some(deny) = perms.deny else {
            continue;
        };
        let mut loaded = 0usize;
        for rule in deny {
            let rule = rule.trim();
            if rule.is_empty() {
                tracing::warn!(
                    target: "omiga::permissions",
                    path = %p.display(),
                    "permissions.deny: skipping empty or whitespace-only entry"
                );
                continue;
            }
            out.push(DenyRuleEntry {
                source: p.clone(),
                rule: rule.to_string(),
            });
            loaded += 1;
        }
        if loaded > 0 {
            tracing::debug!(
                target: "omiga::permissions",
                path = %p.display(),
                count = loaded,
                "loaded permissions.deny entries from settings file"
            );
        }
    }
    append_omiga_project_permissions(project_root, &mut out);
    tracing::debug!(
        target: "omiga::permissions",
        total = out.len(),
        project_root = %project_root.display(),
        "merged permission deny rules (user + project .claude + .omiga/permissions.json)"
    );
    out
}

/// Omiga UI–edited deny list (`<project>/.omiga/permissions.json`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OmigaPermissionsFile {
    #[serde(default)]
    pub deny: Vec<String>,
}

fn append_omiga_project_permissions(project_root: &Path, out: &mut Vec<DenyRuleEntry>) {
    let path = project_root.join(".omiga/permissions.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return,
    };
    let f: OmigaPermissionsFile = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                target: "omiga::permissions",
                path = %path.display(),
                error = %e,
                "failed to parse .omiga/permissions.json"
            );
            return;
        }
    };
    let mut loaded = 0usize;
    for rule in f.deny {
        let rule = rule.trim();
        if rule.is_empty() {
            continue;
        }
        out.push(DenyRuleEntry {
            source: path.clone(),
            rule: rule.to_string(),
        });
        loaded += 1;
    }
    if loaded > 0 {
        tracing::debug!(
            target: "omiga::permissions",
            path = %path.display(),
            count = loaded,
            "loaded permissions.deny from .omiga/permissions.json"
        );
    }
}

/// Read only the Omiga-managed file (for Settings UI). Does not include `~/.claude` merge.
#[must_use]
pub fn read_omiga_permissions_file(project_root: &Path) -> Vec<String> {
    let path = project_root.join(".omiga/permissions.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return vec![],
    };
    let f: OmigaPermissionsFile = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    f.deny
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Write `<project>/.omiga/permissions.json` (pretty JSON).
pub fn write_omiga_permissions_file(project_root: &Path, deny: &[String]) -> Result<(), String> {
    let dir = project_root.join(".omiga");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("permissions.json");
    let cleaned: Vec<String> = deny
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let f = OmigaPermissionsFile { deny: cleaned };
    let json =
        serde_json::to_string_pretty(&f).map_err(|e| format!("serialize permissions: {e}"))?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// Merge `permissions.deny` from Claude Code-style JSON settings (user + project).
#[must_use]
pub fn load_merged_permission_deny_rules(project_root: &Path) -> Vec<String> {
    load_merged_permission_deny_rule_entries(project_root)
        .into_iter()
        .map(|e| e.rule)
        .collect()
}

/// Warn on patterns that often indicate typos or copy-paste mistakes (non-fatal).
pub fn validate_permission_deny_entries(entries: &[DenyRuleEntry]) {
    for e in entries {
        if e.rule.contains('(') && !e.rule.ends_with(')') {
            tracing::warn!(
                target: "omiga::permissions",
                rule = %e.rule,
                path = %e.source.display(),
                "permissions.deny: rule has '(' but does not end with ')' — parsed as a single tool name (see Claude Code permission rule format)"
            );
        }
        if e.rule.len() > 2048 {
            tracing::warn!(
                target: "omiga::permissions",
                len = e.rule.len(),
                path = %e.source.display(),
                "permissions.deny: unusually long rule string"
            );
        }
    }
}
