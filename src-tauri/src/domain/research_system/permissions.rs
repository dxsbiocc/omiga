use super::models::{AgentCard, PermissionDecision, PermissionSpec, PermissionStatus, TaskSpec};
use std::collections::BTreeSet;

#[derive(Debug, Default, Clone, Copy)]
pub struct PermissionManager;

impl PermissionManager {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, agent: &AgentCard, task: &TaskSpec) -> PermissionDecision {
        let mut reasons = Vec::new();

        for tool in &task.requested_tools {
            if agent.tools.forbidden.iter().any(|item| item == tool) {
                reasons.push(format!(
                    "tool '{}' is forbidden for agent '{}'",
                    tool, agent.id
                ));
            }
            if !agent.tools.allowed.is_empty()
                && !agent.tools.allowed.iter().any(|item| item == tool)
            {
                reasons.push(format!(
                    "tool '{}' is not in allowed list for agent '{}'",
                    tool, agent.id
                ));
            }
        }

        reasons.extend(missing_scope(
            "read",
            &task.requested_permissions.read,
            &agent.permissions.read,
        ));
        reasons.extend(missing_scope(
            "write",
            &task.requested_permissions.write,
            &agent.permissions.write,
        ));
        reasons.extend(missing_scope(
            "execute",
            &task.requested_permissions.execute,
            &agent.permissions.execute,
        ));
        reasons.extend(missing_scope(
            "external_side_effect",
            &task.requested_permissions.external_side_effect,
            &agent.permissions.external_side_effect,
        ));

        if !reasons.is_empty() {
            return PermissionDecision {
                status: PermissionStatus::Denied,
                reasons,
            };
        }

        if requires_human_approval(
            &agent.permissions,
            &task.requested_permissions,
            &task.requested_tools,
        ) {
            return PermissionDecision {
                status: PermissionStatus::RequiresApproval,
                reasons: vec!["human approval required for requested action".to_string()],
            };
        }

        PermissionDecision {
            status: PermissionStatus::Allowed,
            reasons: Vec::new(),
        }
    }
}

fn missing_scope(scope: &str, requested: &[String], allowed: &[String]) -> Vec<String> {
    if requested.is_empty() {
        return Vec::new();
    }
    let allowed = allowed.iter().cloned().collect::<BTreeSet<_>>();
    requested
        .iter()
        .filter(|item| !allowed.contains(*item))
        .map(|item| format!("{} '{}' is not permitted", scope, item))
        .collect()
}

fn requires_human_approval(
    granted: &PermissionSpec,
    requested: &PermissionSpec,
    requested_tools: &[String],
) -> bool {
    if granted.human_approval_required || requested.human_approval_required {
        return !requested_tools.is_empty()
            || !requested.write.is_empty()
            || !requested.execute.is_empty()
            || !requested.external_side_effect.is_empty();
    }
    false
}
