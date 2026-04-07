//! V2 task list (Todo v2) — aligned with `src/utils/tasks.ts` task shape.

use serde::{Deserialize, Serialize};

/// Task status (`pending` | `in_progress` | `completed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskV2Status {
    Pending,
    InProgress,
    Completed,
}

/// One task row (matches TS `Task` for tool I/O).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub subject: String,
    pub description: String,
    #[serde(rename = "activeForm", skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub status: TaskV2Status,
    pub blocks: Vec<String>,
    #[serde(rename = "blockedBy")]
    pub blocked_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

impl AgentTask {
    pub fn is_internal(&self) -> bool {
        self.metadata
            .as_ref()
            .and_then(|m| m.get("_internal"))
            .and_then(|v| v.as_bool())
            == Some(true)
    }
}

/// `from` blocks `to`: append `to` to `from.blocks` and `from` to `to.blocked_by`.
pub fn apply_block_edge(tasks: &mut [AgentTask], from_id: &str, to_id: &str) {
    if let Some(from) = tasks.iter_mut().find(|t| t.id == from_id) {
        if !from.blocks.contains(&to_id.to_string()) {
            from.blocks.push(to_id.to_string());
        }
    }
    if let Some(to) = tasks.iter_mut().find(|t| t.id == to_id) {
        if !to.blocked_by.contains(&from_id.to_string()) {
            to.blocked_by.push(from_id.to_string());
        }
    }
}
