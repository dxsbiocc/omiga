//! `CronCreate` — schedule a recurring task as a cron job.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str =
    "Schedule a recurring task. Use cron syntax for schedule (e.g. '0 9 * * 1-5' = weekdays at \
     9am). The task description tells the agent what to do when triggered.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronCreateArgs {
    /// Cron expression, e.g. "0 9 * * 1-5"
    pub schedule: String,
    /// Human description of what to do when triggered
    pub task: String,
    pub session_id: Option<String>,
}

pub struct CronCreateTool;

/// Validate a cron expression by parsing it with the `cron` crate.
fn validate_schedule(schedule: &str) -> Result<(), String> {
    use std::str::FromStr;
    cron::Schedule::from_str(schedule)
        .map(|_| ())
        .map_err(|e| format!("Invalid cron expression '{}': {}", schedule, e))
}

#[async_trait]
impl super::ToolImpl for CronCreateTool {
    type Args = CronCreateArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let schedule = args.schedule.trim().to_string();
        let task = args.task.trim().to_string();

        if schedule.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`schedule` must not be empty.".to_string(),
            });
        }
        if task.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`task` must not be empty.".to_string(),
            });
        }

        if let Err(msg) = validate_schedule(&schedule) {
            return Err(ToolError::InvalidArguments { message: msg });
        }

        let Some(pool) = ctx.db.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: "Database is not available in this context.".to_string(),
            });
        };

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let session_id = args.session_id.as_deref();

        sqlx::query(
            r#"
            INSERT INTO cron_jobs (id, schedule, task_description, session_id, created_at,
                                   last_run_at, run_count, enabled)
            VALUES (?, ?, ?, ?, ?, NULL, 0, 1)
            "#,
        )
        .bind(&id)
        .bind(&schedule)
        .bind(&task)
        .bind(session_id)
        .bind(&created_at)
        .execute(pool)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Failed to insert cron job: {}", e),
        })?;

        let text = serde_json::to_string_pretty(&serde_json::json!({
            "id": id,
            "schedule": schedule,
            "task": task,
            "message": "Cron job scheduled."
        }))
        .map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;

        Ok(JsonToolOutput { text }.into_stream())
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
        "CronCreate",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "schedule": {
                    "type": "string",
                    "description": "Cron expression, e.g. '0 9 * * 1-5' for weekdays at 9am"
                },
                "task": {
                    "type": "string",
                    "description": "Human-readable description of what to do when this job triggers"
                },
                "session_id": {
                    "type": "string",
                    "description": "Optional session id to associate with this job"
                }
            },
            "required": ["schedule", "task"]
        }),
    )
}
