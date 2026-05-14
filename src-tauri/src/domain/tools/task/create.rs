//! Create a V2 task — aligned with `TaskCreateTool` (`TaskCreate`).

use super::{ToolContext, ToolError, ToolSchema};
use crate::domain::session::{AgentTask, TaskV2Status};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = r#"Create a task in the session task list (title + description). Returns the new task id.

Use with `TaskGet`, `TaskUpdate`, `TaskList` to manage structured work beyond the lightweight `todo_write` checklist."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreateArgs {
    pub subject: String,
    pub description: String,
    #[serde(rename = "activeForm")]
    pub active_form: Option<String>,
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

pub struct TaskCreateTool;

#[async_trait]
impl super::ToolImpl for TaskCreateTool {
    type Args = TaskCreateArgs;

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

        let subject = args.subject.trim().to_string();
        let description = args.description.trim().to_string();
        if subject.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`subject` must not be empty.".to_string(),
            });
        }

        let id = uuid::Uuid::new_v4().to_string();
        let task = AgentTask {
            id: id.clone(),
            subject: subject.clone(),
            description,
            active_form: args.active_form,
            owner: None,
            status: TaskV2Status::Pending,
            blocks: vec![],
            blocked_by: vec![],
            metadata: args.metadata,
        };

        {
            let mut g = store.lock().await;
            g.push(task);
        }

        let text = serde_json::json!({
            "task": { "id": id, "subject": subject }
        });
        let s = serde_json::to_string_pretty(&text).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;
        Ok(JsonToolOutput { text: s }.into_stream())
    }
}

struct JsonToolOutput {
    text: String,
}

impl StreamOutput for JsonToolOutput {
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
        "TaskCreate",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string", "description": "Brief title for the task" },
                "description": { "type": "string", "description": "What needs to be done" },
                "activeForm": { "type": "string", "description": "Present continuous label when in_progress" },
                "metadata": { "type": "object", "description": "Optional metadata map" }
            },
            "required": ["subject", "description"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolImpl;
    use futures::StreamExt;
    use std::sync::Arc;

    #[tokio::test]
    async fn create_persists_in_store() {
        let store = Arc::new(tokio::sync::Mutex::new(vec![]));
        let ctx = ToolContext::new("/tmp").with_agent_tasks(Some(store.clone()));
        let args = TaskCreateArgs {
            subject: "S".to_string(),
            description: "D".to_string(),
            active_form: None,
            metadata: None,
        };
        let mut stream = TaskCreateTool::execute(&ctx, args).await.unwrap();
        while stream.next().await.is_some() {}
        let g = store.lock().await;
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].subject, "S");
    }
}
