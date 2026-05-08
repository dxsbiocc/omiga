//! Agent registry — unified metadata surface over the underlying router.
//!
//! The router remains the execution lookup path, while the registry provides a
//! stable place to ask "which roles exist and what are their characteristics?"

use super::definition::{AgentSource, ModelTier};
use super::{AgentDefinition, AgentRouter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRoleInfo {
    pub agent_type: String,
    pub when_to_use: String,
    pub source: AgentSource,
    pub model_tier: ModelTier,
    pub explicit_model: Option<String>,
    pub background: bool,
    pub user_facing: bool,
}

pub struct AgentRegistry {
    router: AgentRouter,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            router: AgentRouter::new(),
        }
    }

    pub fn router(&self) -> &AgentRouter {
        &self.router
    }

    pub fn get(&self, agent_type: &str) -> Option<&dyn AgentDefinition> {
        self.router.get_agent(agent_type)
    }

    pub fn list_roles(&self) -> Vec<AgentRoleInfo> {
        let mut roles = self
            .router
            .list_agents()
            .into_iter()
            .filter_map(|agent_type| {
                let agent = self.router.get_agent(agent_type)?;
                Some(AgentRoleInfo {
                    agent_type: agent.agent_type().to_string(),
                    when_to_use: agent.when_to_use().to_string(),
                    source: agent.source(),
                    model_tier: agent.model_tier(),
                    explicit_model: agent.model().map(|m| m.to_string()),
                    background: agent.background(),
                    user_facing: agent.user_facing(),
                })
            })
            .collect::<Vec<_>>();
        roles.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
        roles
    }

    pub fn list_user_facing_roles(&self) -> Vec<AgentRoleInfo> {
        self.list_roles()
            .into_iter()
            .filter(|r| r.user_facing)
            .collect()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_known_roles_and_metadata() {
        let registry = AgentRegistry::new();
        let roles = registry.list_roles();
        assert!(roles.iter().any(|r| r.agent_type == "general-purpose"));
        let explore = roles
            .iter()
            .find(|r| r.agent_type == "Explore")
            .expect("Explore role");
        assert_eq!(explore.model_tier, ModelTier::Spark);
        assert!(!explore.user_facing);
        assert!(roles.iter().any(|r| r.agent_type == "code-reviewer"));
        assert!(roles.iter().any(|r| r.agent_type == "api-reviewer"));
        assert!(roles.iter().any(|r| r.agent_type == "critic"));
        assert!(roles.iter().any(|r| r.agent_type == "security-reviewer"));
        assert!(roles.iter().any(|r| r.agent_type == "performance-reviewer"));
        assert!(roles.iter().any(|r| r.agent_type == "quality-reviewer"));
        assert!(roles.iter().any(|r| r.agent_type == "test-engineer"));
        assert!(registry
            .list_user_facing_roles()
            .iter()
            .all(|r| r.user_facing));
    }
}
