//! Agent model router — centralizes three-tier role → model resolution.

use super::definition::{AgentDefinition, ModelTier};
use crate::llm::{LlmConfig, LlmProvider};

pub fn default_alias_for_tier(tier: ModelTier) -> Option<&'static str> {
    match tier {
        ModelTier::Frontier => Some("opus"),
        ModelTier::Standard => None, // inherit parent choice by default
        ModelTier::Spark => Some("haiku"),
    }
}

fn alias_matches_parent_tier(alias: &str, parent_model: &str) -> bool {
    let p = parent_model.to_ascii_lowercase();
    match alias.to_ascii_lowercase().as_str() {
        "opus" | "claude-opus" => p.contains("opus"),
        "sonnet" | "claude-sonnet" => p.contains("sonnet"),
        "haiku" | "claude-haiku" => p.contains("haiku"),
        _ => false,
    }
}

/// Resolve an alias or explicit model string for a sub-agent session.
///
/// Rules:
/// - environment overrides win
/// - `inherit` uses the parent model
/// - aliases matching the parent tier inherit the exact parent model id
/// - anthropic tier aliases expand to concrete model ids
pub fn resolve_subagent_model(base: &LlmConfig, alias: Option<&str>) -> String {
    if let Ok(env_override) = std::env::var("CLAUDE_CODE_SUBAGENT_MODEL") {
        let t = env_override.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(env_override) = std::env::var("OMIGA_SUBAGENT_MODEL") {
        let t = env_override.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }

    let Some(a) = alias.map(str::trim).filter(|s| !s.is_empty()) else {
        return base.model.clone();
    };
    if a.eq_ignore_ascii_case("inherit") {
        return base.model.clone();
    }
    let parent = base.model.as_str();
    if alias_matches_parent_tier(a, parent) {
        return base.model.clone();
    }

    let a_lower = a.to_ascii_lowercase();
    if base.provider == LlmProvider::Anthropic {
        if a_lower == "sonnet" || a_lower == "claude-sonnet" {
            return "claude-sonnet-4-20250514".to_string();
        }
        if a_lower == "opus" || a_lower == "claude-opus" {
            return "claude-opus-4-20250514".to_string();
        }
        if a_lower == "haiku" || a_lower == "claude-haiku" {
            return "claude-haiku-4-20250514".to_string();
        }
        if a.starts_with("claude-") {
            return a.to_string();
        }
    }

    if a.len() > 6 && (a.contains('-') || a.contains('/') || a.contains('.')) {
        return a.to_string();
    }
    base.model.clone()
}

pub fn effective_alias_for_agent(agent: &dyn AgentDefinition) -> Option<&str> {
    agent
        .model()
        .or_else(|| default_alias_for_tier(agent.model_tier()))
}

pub fn resolve_model_for_agent(
    base: &LlmConfig,
    agent: &dyn AgentDefinition,
    explicit_override: Option<&str>,
) -> String {
    let explicit_override = explicit_override.map(str::trim).filter(|s| !s.is_empty());
    if explicit_override.is_some() {
        return resolve_subagent_model(base, explicit_override);
    }

    let alias = effective_alias_for_agent(agent);
    if alias.map(|m| m != "inherit").unwrap_or(false) {
        resolve_subagent_model(base, alias)
    } else {
        base.model.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agents::builtins::{
        architect::ArchitectAgent, critic::CriticAgent, explore::ExploreAgent,
        security_reviewer::SecurityReviewerAgent,
    };

    fn anthropic_base(model: &str) -> LlmConfig {
        let mut cfg = LlmConfig::new(LlmProvider::Anthropic, "test");
        cfg.model = model.to_string();
        cfg
    }

    #[test]
    fn resolves_model_for_frontier_agent() {
        let cfg = anthropic_base("claude-sonnet-4-20250514");
        let model = resolve_model_for_agent(&cfg, &ArchitectAgent, None);
        assert!(model.contains("opus"));
    }

    #[test]
    fn resolves_model_for_spark_agent() {
        let cfg = anthropic_base("claude-sonnet-4-20250514");
        let model = resolve_model_for_agent(&cfg, &ExploreAgent, None);
        assert!(model.contains("haiku"));
    }

    #[test]
    fn resolves_model_for_security_reviewer_as_frontier() {
        let cfg = anthropic_base("claude-sonnet-4-20250514");
        let model = resolve_model_for_agent(&cfg, &SecurityReviewerAgent, None);
        assert!(model.contains("opus"));
    }

    #[test]
    fn resolves_model_for_critic_as_frontier() {
        let cfg = anthropic_base("claude-sonnet-4-20250514");
        let model = resolve_model_for_agent(&cfg, &CriticAgent, None);
        assert!(model.contains("opus"));
    }
}
