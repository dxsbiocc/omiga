//! Agent integration globals — registry first, router as compatibility facade.

use super::{registry::AgentRegistry, router::AgentRouter};
use std::sync::OnceLock;

static AGENT_REGISTRY: OnceLock<AgentRegistry> = OnceLock::new();

pub fn get_agent_registry() -> &'static AgentRegistry {
    AGENT_REGISTRY.get_or_init(AgentRegistry::new)
}

/// Compatibility facade for existing call sites.
pub fn get_agent_router() -> &'static AgentRouter {
    get_agent_registry().router()
}
