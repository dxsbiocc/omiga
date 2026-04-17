//! Update or delete a V2 task — aligned with `TaskUpdateTool` (`TaskUpdate`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::session::agent_task::apply_block_edge;
use crate::domain::session::TaskV2Status;
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Update fields on a task, add dependency edges, merge metadata, or set `status` to `deleted` to remove it."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdateArgs {
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub subject: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "activeForm")]
    pub active_form: Option<String>,
    /// `pending` | `in_progress` | `completed` | `deleted`
    pub status: Option<String>,
    #[serde(rename = "addBlocks")]
    pub add_blocks: Option<Vec<String>>,
    #[serde(rename = "addBlockedBy")]
    pub add_blocked_by: Option<Vec<String>>,
    pub owner: Option<String>,
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

fn parse_status(s: &str) -> Result<Option<TaskV2Status>, ToolError> {
    match s {
        "deleted" => Ok(None),
        "pending" => Ok(Some(TaskV2Status::Pending)),
        "in_progress" => Ok(Some(TaskV2Status::InProgress)),
        "completed" => Ok(Some(TaskV2Status::Completed)),
        _ => Err(ToolError::InvalidArguments {
            message: format!("Invalid status: {}", s),
        }),
    }
}

pub struct TaskUpdateTool;

#[async_trait]
impl super::ToolImpl for TaskUpdateTool {
    type Args = TaskUpdateArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let Some(store) = ctx.agent_tasks.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: "Task list is not available in this context (no session task store)."
                    .to_string(),
            });
        };

        let task_id = args.task_id.trim().to_string();
        if task_id.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`taskId` must not be empty.".to_string(),
            });
        }

        let mut tasks = store.lock().await;
        let Some(pos) = tasks.iter().position(|t| t.id == task_id) else {
            let text = serde_json::json!({
                "success": false,
                "taskId": task_id,
                "updatedFields": [],
                "error": "Task not found"
            });
            let s =
                serde_json::to_string_pretty(&text).map_err(|e| ToolError::ExecutionFailed {
                    message: e.to_string(),
                })?;
            return Ok(JsonOut { text: s }.into_stream());
        };

        let existing = tasks[pos].clone();

        if let Some(ref st) = args.status {
            if st == "deleted" {
                tasks.remove(pos);
                let text = serde_json::json!({
                    "success": true,
                    "taskId": task_id,
                    "updatedFields": ["deleted"],
                    "statusChange": { "from": existing.status, "to": "deleted" }
                });
                let s = serde_json::to_string_pretty(&text).map_err(|e| {
                    ToolError::ExecutionFailed {
                        message: e.to_string(),
                    }
                })?;
                return Ok(JsonOut { text: s }.into_stream());
            }
        }

        let mut updated_fields: Vec<String> = vec![];

        if let Some(ref st) = args.status {
            if let Some(new_status) = parse_status(st)? {
                if new_status != existing.status {
                    tasks[pos].status = new_status;
                    updated_fields.push("status".to_string());
                }
            }
        }

        if let Some(ref s) = args.subject {
            if s != &existing.subject {
                tasks[pos].subject = s.clone();
                updated_fields.push("subject".to_string());
            }
        }
        if let Some(ref d) = args.description {
            if d != &existing.description {
                tasks[pos].description = d.clone();
                updated_fields.push("description".to_string());
            }
        }
        if let Some(ref a) = args.active_form {
            if Some(a.as_str()) != existing.active_form.as_deref() {
                tasks[pos].active_form = Some(a.clone());
                updated_fields.push("activeForm".to_string());
            }
        }
        if let Some(ref o) = args.owner {
            if Some(o.as_str()) != existing.owner.as_deref() {
                tasks[pos].owner = Some(o.clone());
                updated_fields.push("owner".to_string());
            }
        }

        if let Some(meta) = args.metadata {
            let mut merged = existing.metadata.clone().unwrap_or_default();
            for (k, v) in meta {
                if v.is_null() {
                    merged.remove(&k);
                } else {
                    merged.insert(k, v);
                }
            }
            tasks[pos].metadata = if merged.is_empty() {
                None
            } else {
                Some(merged)
            };
            updated_fields.push("metadata".to_string());
        }

        let tid = task_id.clone();
        if let Some(ids) = args.add_blocks {
            let before = tasks[pos].blocks.clone();
            for other in ids {
                let o = other.trim().to_string();
                if o.is_empty() || o == tid {
                    continue;
                }
                if tasks.iter().any(|t| t.id == o) {
                    apply_block_edge(&mut tasks, &tid, &o);
                }
            }
            if tasks[pos].blocks != before {
                updated_fields.push("blocks".to_string());
            }
        }

        if let Some(ids) = args.add_blocked_by {
            let before = tasks[pos].blocked_by.clone();
            for other in ids {
                let o = other.trim().to_string();
                if o.is_empty() || o == tid {
                    continue;
                }
                if tasks.iter().any(|t| t.id == o) {
                    apply_block_edge(&mut tasks, &o, &tid);
                }
            }
            if tasks[pos].blocked_by != before {
                updated_fields.push("blockedBy".to_string());
            }
        }

        let status_change = if updated_fields.iter().any(|f| f == "status") {
            Some(serde_json::json!({
                "from": existing.status,
                "to": tasks[pos].status
            }))
        } else {
            None
        };

        let text = serde_json::json!({
            "success": true,
            "taskId": task_id,
            "updatedFields": updated_fields,
            "statusChange": status_change,
            "verificationNudgeNeeded": false
        });
        let s = serde_json::to_string_pretty(&text).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(JsonOut { text: s }.into_stream())
    }
}

struct JsonOut {
    text: String,
}

impl StreamOutput for JsonOut {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(self.text),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "TaskUpdate",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "subject": { "type": "string" },
                "description": { "type": "string" },
                "activeForm": { "type": "string" },
                "status": { "type": "string", "description": "pending | in_progress | completed | deleted" },
                "addBlocks": { "type": "array", "items": { "type": "string" } },
                "addBlockedBy": { "type": "array", "items": { "type": "string" } },
                "owner": { "type": "string" },
                "metadata": { "type": "object" }
            },
            "required": ["taskId"]
        }),
    )
}
