use serde::{Deserialize, Serialize};

/// Supported lifecycle events for the minimal hook engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookEvent {
    #[serde(rename = "PreToolUse")]
    PreToolUse,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
}

/// Tool matcher for a hook declaration.
///
/// This first implementation intentionally supports only exact tool names plus
/// `*` as a match-all sentinel. That covers targeted hooks and global hooks
/// without introducing glob/regex ambiguity into config semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookMatcher {
    #[serde(default = "default_tool_name")]
    pub tool_name: String,
}

impl Default for HookMatcher {
    fn default() -> Self {
        Self {
            tool_name: default_tool_name(),
        }
    }
}

impl HookMatcher {
    pub fn matches_tool(&self, tool_name: &str) -> bool {
        self.tool_name == "*" || self.tool_name == tool_name
    }
}

fn default_tool_name() -> String {
    "*".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDeclaration {
    pub event: HookEvent,

    #[serde(default)]
    pub matcher: HookMatcher,

    pub command: String,

    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookConfigFile {
    #[serde(default)]
    pub hooks: Vec<HookDeclaration>,
}
