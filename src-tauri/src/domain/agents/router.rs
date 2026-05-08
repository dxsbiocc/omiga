//! Agent 路由器
//!
//! 负责根据 `subagent_type` 选择并返回对应的 Agent 定义。

use super::builtins::register_built_in_agents;
use super::definition::{AgentDefEntry, AgentDefinition};
use std::collections::HashMap;

/// Agent 路由器
pub struct AgentRouter {
    agents: HashMap<String, AgentDefEntry>,
    default_agent: String,
}

impl AgentRouter {
    /// 创建新的路由器并注册所有内置 Agent
    pub fn new() -> Self {
        let mut router = Self {
            agents: HashMap::new(),
            default_agent: "general-purpose".to_string(),
        };

        register_built_in_agents(&mut router);
        router
    }

    /// 注册一个 Agent
    pub fn register(&mut self, agent: Box<dyn AgentDefinition>) {
        let agent_type = agent.agent_type().to_string();
        self.agents.insert(agent_type, AgentDefEntry::new(agent));
    }

    /// 注销一个 Agent（用于热重载）
    pub fn unregister(&mut self, agent_type: &str) -> bool {
        // 不能注销内置 Agent
        if let Some(entry) = self.agents.get(agent_type) {
            if entry.inner.source() == super::definition::AgentSource::BuiltIn {
                return false;
            }
        }
        self.agents.remove(agent_type).is_some()
    }

    /// 清空所有非内置 Agent（用于热重载）
    pub fn clear_custom_agents(&mut self) {
        self.agents
            .retain(|_, entry| entry.inner.source() == super::definition::AgentSource::BuiltIn);
    }

    /// 根据 subagent_type 选择 Agent
    /// - 指定了类型且存在 → 返回对应 Agent
    /// - 指定了类型但不存在 → 返回默认 Agent
    /// - 未指定类型 → 返回默认 Agent
    pub fn select_agent(&self, subagent_type: Option<&str>) -> &dyn AgentDefinition {
        let agent_type = subagent_type.unwrap_or(&self.default_agent);

        match self.agents.get(agent_type) {
            Some(entry) => &*entry.inner,
            None => {
                // 如果指定的 Agent 不存在，回退到默认 Agent
                self.agents
                    .get(&self.default_agent)
                    .map(|e| &*e.inner)
                    .expect("Default agent must be registered")
            }
        }
    }

    /// 获取特定 Agent
    pub fn get_agent(&self, agent_type: &str) -> Option<&dyn AgentDefinition> {
        self.agents.get(agent_type).map(|e| &*e.inner)
    }

    /// 检查 Agent 是否存在
    pub fn has_agent(&self, agent_type: &str) -> bool {
        self.agents.contains_key(agent_type)
    }

    /// 列出所有可用的 Agent 类型
    pub fn list_agents(&self) -> Vec<&str> {
        self.agents.keys().map(|s| s.as_str()).collect()
    }

    /// 列出所有 Agent 及其描述
    pub fn list_agents_with_description(&self) -> Vec<(&str, &str)> {
        self.agents
            .values()
            .map(|e| (e.inner.agent_type(), e.inner.when_to_use()))
            .collect()
    }

    /// 列出所有 Agent 及其描述和后台标志
    pub fn list_agents_full(&self) -> Vec<(&str, &str, bool)> {
        self.agents
            .values()
            .map(|e| {
                (
                    e.inner.agent_type(),
                    e.inner.when_to_use(),
                    e.inner.background(),
                )
            })
            .collect()
    }

    /// 列出面向用户的 Agent（user_facing() == true），用于 UI 选择器
    pub fn list_user_facing_agents(&self) -> Vec<(&str, &str, bool)> {
        self.agents
            .values()
            .filter(|e| e.inner.user_facing())
            .map(|e| {
                (
                    e.inner.agent_type(),
                    e.inner.when_to_use(),
                    e.inner.background(),
                )
            })
            .collect()
    }

    /// 设置默认 Agent
    pub fn set_default_agent(&mut self, agent_type: &str) {
        if self.agents.contains_key(agent_type) {
            self.default_agent = agent_type.to_string();
        }
    }
}

impl Default for AgentRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_selects_default() {
        let router = AgentRouter::new();
        let agent = router.select_agent(None);
        assert_eq!(agent.agent_type(), "general-purpose");
    }

    #[test]
    fn test_router_selects_explore() {
        let router = AgentRouter::new();
        let agent = router.select_agent(Some("Explore"));
        assert_eq!(agent.agent_type(), "Explore");
    }

    #[test]
    fn test_router_fallback_to_default() {
        let router = AgentRouter::new();
        let agent = router.select_agent(Some("non-existent"));
        assert_eq!(agent.agent_type(), "general-purpose");
    }
}
