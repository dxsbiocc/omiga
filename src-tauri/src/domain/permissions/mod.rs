//! Fine-grained permission system for skills (aligned with SkillTool.checkPermissions in TS).
//!
//! Provides three-state permission decisions: allow | deny | ask
//! with rule-based matching (exact match, prefix match) and user suggestions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Permission Behaviors
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionBehavior {
    /// Allow the operation without user confirmation
    Allow,
    /// Deny the operation entirely
    Deny,
    /// Ask the user for confirmation
    Ask,
}

// ============================================================================
// Permission Rules
// ============================================================================

/// Source of a permission rule
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionRuleSource {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    FlagSettings,
    CliArg,
    Command,
    Session,
}

/// The value of a permission rule - specifies which skill and optional content pattern
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRuleValue {
    pub skill_name: String,
    /// Optional content pattern for fine-grained matching (e.g., "skill:*" for prefix)
    pub rule_content: Option<String>,
}

/// A permission rule with its source and behavior
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRule {
    pub source: PermissionRuleSource,
    pub behavior: PermissionBehavior,
    pub rule_value: PermissionRuleValue,
}

/// Collection of permission rules grouped by source
pub type PermissionRulesBySource = HashMap<PermissionRuleSource, Vec<PermissionRule>>;

// ============================================================================
// Permission Decisions
// ============================================================================

/// Reason for a permission decision
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PermissionDecisionReason {
    Rule { rule: PermissionRule },
    Mode { mode: String },
    Other { reason: String },
}

/// Suggested permission update for user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSuggestion {
    pub skill_name: String,
    pub behavior: PermissionBehavior,
    pub destination: PermissionUpdateDestination,
}

/// Where to persist permission updates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionUpdateDestination {
    UserSettings,
    ProjectSettings,
    LocalSettings,
}

/// Result when permission is granted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionAllowDecision {
    pub behavior: PermissionBehavior,
    pub decision_reason: Option<PermissionDecisionReason>,
}

/// Result when user should be prompted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionAskDecision {
    pub behavior: PermissionBehavior,
    pub message: String,
    pub decision_reason: Option<PermissionDecisionReason>,
    pub suggestions: Vec<PermissionSuggestion>,
}

/// Result when permission is denied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDenyDecision {
    pub behavior: PermissionBehavior,
    pub message: String,
    pub decision_reason: PermissionDecisionReason,
}

/// A permission decision - allow, ask, or deny
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision_type", rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow(PermissionAllowDecision),
    Ask(PermissionAskDecision),
    Deny(PermissionDenyDecision),
}

impl PermissionDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, PermissionDecision::Allow(_))
    }

    pub fn is_deny(&self) -> bool {
        matches!(self, PermissionDecision::Deny(_))
    }

    pub fn is_ask(&self) -> bool {
        matches!(self, PermissionDecision::Ask(_))
    }
}

// ============================================================================
// Permission Context
// ============================================================================

/// Context needed for permission checking
#[derive(Debug, Clone, Default)]
pub struct PermissionContext {
    pub allow_rules: PermissionRulesBySource,
    pub deny_rules: PermissionRulesBySource,
    pub ask_rules: PermissionRulesBySource,
}

impl PermissionContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a rule to the context
    pub fn add_rule(&mut self, rule: PermissionRule) {
        let map = match rule.behavior {
            PermissionBehavior::Allow => &mut self.allow_rules,
            PermissionBehavior::Deny => &mut self.deny_rules,
            PermissionBehavior::Ask => &mut self.ask_rules,
        };
        map.entry(rule.source.clone())
            .or_default()
            .push(rule);
    }
}

// ============================================================================
// Permission Checking (Core Logic - aligned with SkillTool.checkPermissions)
// ============================================================================

/// Check if a rule matches the skill name.
/// Supports exact match and prefix match with wildcard (e.g., "review:*")
fn rule_matches(rule_value: &PermissionRuleValue, skill_name: &str) -> bool {
    let rule_skill = rule_value.skill_name.trim();
    let normalized_skill = skill_name.trim();

    // Strip leading slash from skill name for consistent matching
    let normalized_skill = normalized_skill
        .strip_prefix('/')
        .unwrap_or(normalized_skill);

    // Check exact match
    if rule_skill == normalized_skill {
        return true;
    }

    // Check prefix match (e.g., "review:*" matches "review-pr", "review-something")
    if let Some(prefix) = rule_skill.strip_suffix("*") {
        return normalized_skill.starts_with(prefix)
            || (prefix.ends_with('-') && normalized_skill.starts_with(prefix.trim_end_matches('-')));
    }

    false
}

/// Check if rule content matches (if specified)
fn rule_content_matches(rule_value: &PermissionRuleValue, skill_content: Option<&str>) -> bool {
    // If no rule content, it's a blanket rule - matches all
    let Some(pattern) = &rule_value.rule_content else {
        return true;
    };

    // If no skill content to match against, can't match
    let Some(content) = skill_content else {
        return false;
    };

    // Check for wildcard pattern (e.g., "*")
    if pattern == "*" {
        return true;
    }

    // Check for prefix match (e.g., "review:*" in content)
    if pattern.ends_with("*") {
        let prefix = pattern.trim_end_matches("*");
        return content.starts_with(prefix);
    }

    // Exact content match
    content == pattern
}

/// Generate permission suggestions for user when asking
fn generate_suggestions(skill_name: &str, behavior: PermissionBehavior) -> Vec<PermissionSuggestion> {
    vec![
        PermissionSuggestion {
            skill_name: skill_name.to_string(),
            behavior,
            destination: PermissionUpdateDestination::ProjectSettings,
        },
        PermissionSuggestion {
            skill_name: skill_name.to_string(),
            behavior,
            destination: PermissionUpdateDestination::UserSettings,
        },
    ]
}

/// Core permission check - aligned with SkillTool.checkPermissions in TypeScript
pub fn check_permissions(
    skill_name: &str,
    skill_content: Option<&str>,
    allowed_tools: Option<&[String]>,
    ctx: &PermissionContext,
) -> PermissionDecision {
    let normalized_skill = skill_name.trim().to_string();

    // 1. Check for deny rules (highest priority)
    for (_source, rules) in &ctx.deny_rules {
        for rule in rules {
            if rule_matches(&rule.rule_value, &normalized_skill)
                && rule_content_matches(&rule.rule_value, skill_content)
            {
                return PermissionDecision::Deny(PermissionDenyDecision {
                    behavior: PermissionBehavior::Deny,
                    message: format!(
                        "Skill '{}' is denied by rule from {:?}.",
                        normalized_skill, rule.source
                    ),
                    decision_reason: PermissionDecisionReason::Rule { rule: rule.clone() },
                });
            }
        }
    }

    // 2. Check for ask rules (second priority - must prompt user)
    for (_source, rules) in &ctx.ask_rules {
        for rule in rules {
            if rule_matches(&rule.rule_value, &normalized_skill)
                && rule_content_matches(&rule.rule_value, skill_content)
            {
                let suggestions = generate_suggestions(&normalized_skill, PermissionBehavior::Ask);
                return PermissionDecision::Ask(PermissionAskDecision {
                    behavior: PermissionBehavior::Ask,
                    message: format!(
                        "Permission required to use skill '{}'.",
                        normalized_skill
                    ),
                    decision_reason: Some(PermissionDecisionReason::Rule { rule: rule.clone() }),
                    suggestions,
                });
            }
        }
    }

    // 3. Check for allow rules (third priority)
    for (_source, rules) in &ctx.allow_rules {
        for rule in rules {
            if rule_matches(&rule.rule_value, &normalized_skill)
                && rule_content_matches(&rule.rule_value, skill_content)
            {
                return PermissionDecision::Allow(PermissionAllowDecision {
                    behavior: PermissionBehavior::Allow,
                    decision_reason: Some(PermissionDecisionReason::Rule { rule: rule.clone() }),
                });
            }
        }
    }

    // 4. Auto-allow skills that only use "safe" properties (no allowed_tools or simple ones)
    // This is an allowlist of safe skill usage patterns
    let is_safe_skill = allowed_tools.map(|tools| tools.is_empty()).unwrap_or(true);
    if is_safe_skill {
        return PermissionDecision::Allow(PermissionAllowDecision {
            behavior: PermissionBehavior::Allow,
            decision_reason: Some(PermissionDecisionReason::Other {
                reason: "Auto-allowed: skill has no restricted tool usage".to_string(),
            }),
        });
    }

    // 5. Default behavior: ask user for permission
    let suggestions = generate_suggestions(&normalized_skill, PermissionBehavior::Allow);
    PermissionDecision::Ask(PermissionAskDecision {
        behavior: PermissionBehavior::Ask,
        message: format!(
            "Permission required to use skill '{}' (no matching permission rule found).",
            normalized_skill
        ),
        decision_reason: Some(PermissionDecisionReason::Other {
            reason: "No matching permission rules found".to_string(),
        }),
        suggestions,
    })
}

// ============================================================================
// Permission Configuration Loading
// ============================================================================

use crate::domain::integrations_config;

/// Build permission context from integrations config
pub fn build_permission_context(project_root: &std::path::Path) -> PermissionContext {
    let config = integrations_config::load_integrations_config(project_root);
    let mut ctx = PermissionContext::new();

    // Load deny rules from disabled skills
    for disabled_skill in &config.disabled_skills {
        ctx.add_rule(PermissionRule {
            source: PermissionRuleSource::ProjectSettings,
            behavior: PermissionBehavior::Deny,
            rule_value: PermissionRuleValue {
                skill_name: disabled_skill.clone(),
                rule_content: None,
            },
        });
    }

    // TODO: Load allow/ask rules from a more detailed permissions file
    // e.g., ~/.omiga/permissions.json or .omiga/permissions.json

    ctx
}

/// Check if a skill is permitted without asking (returns true for allow, false for deny/ask)
pub fn is_skill_permitted(skill_name: &str, ctx: &PermissionContext) -> bool {
    matches!(
        check_permissions(skill_name, None, None, ctx),
        PermissionDecision::Allow(_)
    )
}

/// Check if a skill is denied
pub fn is_skill_denied(skill_name: &str, ctx: &PermissionContext) -> bool {
    matches!(
        check_permissions(skill_name, None, None, ctx),
        PermissionDecision::Deny(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_matches_exact() {
        let rule = PermissionRuleValue {
            skill_name: "review".to_string(),
            rule_content: None,
        };
        assert!(rule_matches(&rule, "review"));
        assert!(!rule_matches(&rule, "review-pr"));
    }

    #[test]
    fn test_rule_matches_prefix() {
        let rule = PermissionRuleValue {
            skill_name: "review:*".to_string(),
            rule_content: None,
        };
        assert!(rule_matches(&rule, "review"));
        assert!(rule_matches(&rule, "review-pr"));
        assert!(rule_matches(&rule, "review-security"));
        assert!(!rule_matches(&rule, "other"));
    }

    #[test]
    fn test_check_permissions_deny_priority() {
        let mut ctx = PermissionContext::new();
        ctx.add_rule(PermissionRule {
            source: PermissionRuleSource::UserSettings,
            behavior: PermissionBehavior::Deny,
            rule_value: PermissionRuleValue {
                skill_name: "dangerous".to_string(),
                rule_content: None,
            },
        });
        ctx.add_rule(PermissionRule {
            source: PermissionRuleSource::UserSettings,
            behavior: PermissionBehavior::Allow,
            rule_value: PermissionRuleValue {
                skill_name: "dangerous".to_string(),
                rule_content: None,
            },
        });

        let result = check_permissions("dangerous", None, None, &ctx);
        assert!(result.is_deny());
    }

    #[test]
    fn test_check_permissions_allow() {
        let mut ctx = PermissionContext::new();
        ctx.add_rule(PermissionRule {
            source: PermissionRuleSource::ProjectSettings,
            behavior: PermissionBehavior::Allow,
            rule_value: PermissionRuleValue {
                skill_name: "safe-skill".to_string(),
                rule_content: None,
            },
        });

        let result = check_permissions("safe-skill", None, None, &ctx);
        assert!(result.is_allow());
    }

    #[test]
    fn test_check_permissions_ask() {
        let mut ctx = PermissionContext::new();
        ctx.add_rule(PermissionRule {
            source: PermissionRuleSource::UserSettings,
            behavior: PermissionBehavior::Ask,
            rule_value: PermissionRuleValue {
                skill_name: "ask-skill".to_string(),
                rule_content: None,
            },
        });

        let result = check_permissions("ask-skill", None, None, &ctx);
        assert!(result.is_ask());
    }

    #[test]
    fn test_check_permissions_default_ask() {
        let ctx = PermissionContext::new();
        let result = check_permissions("unknown-skill", None, Some(&["bash".to_string()]), &ctx);
        assert!(result.is_ask());
    }

    #[test]
    fn test_check_permissions_auto_allow_safe() {
        let ctx = PermissionContext::new();
        let result = check_permissions("safe-skill", None, None, &ctx);
        assert!(result.is_allow());
    }
}