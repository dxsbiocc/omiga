//! 兼容层 - 支持旧的基于文件的权限 API (check_permissions / build_permission_context)
//!
//! 此模块保留与 chat.rs 中 execute_one_tool 使用的旧 API 的兼容性。
//! 旧实现从 `.omiga/permissions.json` 读取 allow/ask/deny 规则并检查 skill 权限。

use std::path::{Path, PathBuf};

/// 旧版权限上下文（持有从文件加载的规则）
#[derive(Debug, Clone)]
pub struct PermissionContextCompat {
    pub project_root: PathBuf,
    /// deny 规则：精确匹配或前缀匹配（以 ":" 结尾表示前缀）
    deny_rules: Vec<String>,
    /// ask 规则
    ask_rules: Vec<String>,
    /// allow 规则（显式允许）
    allow_rules: Vec<String>,
}

impl PermissionContextCompat {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            deny_rules: Vec::new(),
            ask_rules: Vec::new(),
            allow_rules: Vec::new(),
        }
    }
}

/// 拒绝决策
#[derive(Debug, Clone)]
pub struct DenyDecision {
    pub decision_reason: String,
    pub message: String,
}

/// 询问决策
#[derive(Debug, Clone)]
pub struct AskDecision {
    pub decision_reason: String,
    pub suggestions: Vec<String>,
}

/// 旧的权限决策枚举（兼容层）
#[derive(Debug, Clone)]
pub enum PermissionDecisionCompat {
    Allow(()),
    Deny(DenyDecision),
    Ask(AskDecision),
}

/// 从 `.omiga/permissions.json` 及 Claude 全局/项目配置构建权限上下文
pub fn build_permission_context(project_root: &Path) -> PermissionContextCompat {
    let mut ctx = PermissionContextCompat::new(project_root.to_path_buf());

    // 加载项目级权限文件 `.omiga/permissions.json`
    let omiga_perm = project_root.join(".omiga").join("permissions.json");
    load_permissions_file(&omiga_perm, &mut ctx);

    // 加载 Claude 项目级配置 `.claude/settings.json`
    let claude_project = project_root.join(".claude").join("settings.json");
    load_claude_settings(&claude_project, &mut ctx);

    // 加载 Claude 全局配置 `~/.claude/settings.json`
    if let Some(home) = dirs::home_dir().or_else(|| std::env::var("HOME").ok().map(PathBuf::from)) {
        let claude_global = home.join(".claude").join("settings.json");
        load_claude_settings(&claude_global, &mut ctx);
    }

    ctx
}

/// 从 `.omiga/permissions.json` 加载规则
fn load_permissions_file(path: &Path, ctx: &mut PermissionContextCompat) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        tracing::warn!("权限文件格式错误: {:?}", path);
        return;
    };

    if let Some(deny) = json.get("deny").and_then(|v| v.as_array()) {
        for item in deny {
            if let Some(s) = item.as_str() {
                ctx.deny_rules.push(s.to_string());
            }
        }
    }
    if let Some(ask) = json.get("ask").and_then(|v| v.as_array()) {
        for item in ask {
            if let Some(s) = item.as_str() {
                ctx.ask_rules.push(s.to_string());
            }
        }
    }
    if let Some(allow) = json.get("allow").and_then(|v| v.as_array()) {
        for item in allow {
            if let Some(s) = item.as_str() {
                ctx.allow_rules.push(s.to_string());
            }
        }
    }
}

/// 从 Claude settings.json 加载 allowedTools / deniedTools
fn load_claude_settings(path: &Path, ctx: &mut PermissionContextCompat) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };

    if let Some(denied) = json.get("deniedTools").and_then(|v| v.as_array()) {
        for item in denied {
            if let Some(s) = item.as_str() {
                ctx.deny_rules.push(s.to_string());
            }
        }
    }
}

/// 规则匹配：支持精确匹配和前缀匹配（`skill:*` 或 `skill:`）
fn rule_matches_skill(rule: &str, skill_name: &str) -> bool {
    if rule.ends_with(":*") || rule.ends_with(':') {
        let prefix = rule.trim_end_matches(":*").trim_end_matches(':');
        skill_name == prefix || skill_name.starts_with(&format!("{}-", prefix))
    } else {
        rule == skill_name
    }
}

/// 检查权限（兼容旧 API）
///
/// 优先级：deny > allow（显式）> ask > 默认行为
pub fn check_permissions(
    skill_name: &str,
    _args: Option<&str>,
    allowed_tools: Option<&[String]>,
    ctx: &PermissionContextCompat,
) -> PermissionDecisionCompat {
    // 1. Deny 规则优先级最高
    for rule in &ctx.deny_rules {
        if rule_matches_skill(rule, skill_name) {
            return PermissionDecisionCompat::Deny(DenyDecision {
                decision_reason: format!("规则拒绝: {}", rule),
                message: format!("工具 '{}' 已被权限规则拒绝", skill_name),
            });
        }
    }

    // 2. 显式 Allow 规则
    for rule in &ctx.allow_rules {
        if rule_matches_skill(rule, skill_name) {
            return PermissionDecisionCompat::Allow(());
        }
    }

    // 3. allowed_tools 列表（来自 skill 定义的允许工具集）
    // 如果 skill 没有声明任何 allowed_tools，则不受此约束
    if let Some(tools) = allowed_tools {
        if !tools.is_empty() && !tools.iter().any(|t| t == skill_name || t == "*") {
            return PermissionDecisionCompat::Ask(AskDecision {
                decision_reason: format!("'{}' 不在 allowed_tools 列表中", skill_name),
                suggestions: vec![format!("将 '{}' 添加到 allowed_tools", skill_name)],
            });
        }
    }

    // 4. Ask 规则
    for rule in &ctx.ask_rules {
        if rule_matches_skill(rule, skill_name) {
            return PermissionDecisionCompat::Ask(AskDecision {
                decision_reason: format!("规则要求确认: {}", rule),
                suggestions: vec![],
            });
        }
    }

    // 5. 默认允许
    PermissionDecisionCompat::Allow(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(deny: &[&str], ask: &[&str], allow: &[&str]) -> PermissionContextCompat {
        PermissionContextCompat {
            project_root: PathBuf::from("/tmp"),
            deny_rules: deny.iter().map(|s| s.to_string()).collect(),
            ask_rules: ask.iter().map(|s| s.to_string()).collect(),
            allow_rules: allow.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_deny_rule_blocks() {
        let ctx = make_ctx(&["dangerous-skill"], &[], &[]);
        let result = check_permissions("dangerous-skill", None, None, &ctx);
        assert!(matches!(result, PermissionDecisionCompat::Deny(_)));
    }

    #[test]
    fn test_deny_prefix_blocks() {
        let ctx = make_ctx(&["review:*"], &[], &[]);
        assert!(matches!(
            check_permissions("review-pr", None, None, &ctx),
            PermissionDecisionCompat::Deny(_)
        ));
        assert!(matches!(
            check_permissions("review", None, None, &ctx),
            PermissionDecisionCompat::Deny(_)
        ));
        // 不应阻止不相关的工具
        assert!(matches!(
            check_permissions("other", None, None, &ctx),
            PermissionDecisionCompat::Allow(_)
        ));
    }

    #[test]
    fn test_deny_overrides_allow() {
        // deny 优先于 allow
        let ctx = make_ctx(&["dangerous"], &[], &["dangerous"]);
        assert!(matches!(
            check_permissions("dangerous", None, None, &ctx),
            PermissionDecisionCompat::Deny(_)
        ));
    }

    #[test]
    fn test_ask_rule() {
        let ctx = make_ctx(&[], &["maybe-skill"], &[]);
        assert!(matches!(
            check_permissions("maybe-skill", None, None, &ctx),
            PermissionDecisionCompat::Ask(_)
        ));
    }

    #[test]
    fn test_default_allow() {
        let ctx = make_ctx(&[], &[], &[]);
        assert!(matches!(
            check_permissions("any-skill", None, None, &ctx),
            PermissionDecisionCompat::Allow(_)
        ));
    }

    #[test]
    fn test_allowed_tools_ask_when_not_in_list() {
        let ctx = make_ctx(&[], &[], &[]);
        let allowed = vec!["other-tool".to_string()];
        assert!(matches!(
            check_permissions("my-skill", None, Some(&allowed), &ctx),
            PermissionDecisionCompat::Ask(_)
        ));
    }

    #[test]
    fn test_allowed_tools_empty_list_allows() {
        // 空 allowed_tools 不限制
        let ctx = make_ctx(&[], &[], &[]);
        assert!(matches!(
            check_permissions("any-skill", None, Some(&[]), &ctx),
            PermissionDecisionCompat::Allow(_)
        ));
    }
}
