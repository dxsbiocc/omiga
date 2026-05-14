//! `CronList` — list all scheduled cron jobs.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = "List all scheduled cron jobs.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronListArgs {
    /// If provided, filter jobs by session_id.
    pub session_id: Option<String>,
}

pub struct CronListTool;

#[async_trait]
impl super::ToolImpl for CronListTool {
    type Args = CronListArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let Some(pool) = ctx.db.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: "Database is not available in this context.".to_string(),
            });
        };

        // Build query with optional session_id filter.
        let rows: Vec<serde_json::Value> = if let Some(ref sid) = args.session_id {
            sqlx::query(
                r#"
                SELECT id, schedule, task_description, session_id, created_at,
                       last_run_at, run_count, enabled
                FROM cron_jobs
                WHERE enabled = 1 AND session_id = ?
                ORDER BY created_at ASC
                "#,
            )
            .bind(sid)
            .fetch_all(pool)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to list cron jobs: {}", e),
            })?
            .into_iter()
            .map(row_to_json)
            .collect()
        } else {
            sqlx::query(
                r#"
                SELECT id, schedule, task_description, session_id, created_at,
                       last_run_at, run_count, enabled
                FROM cron_jobs
                WHERE enabled = 1
                ORDER BY created_at ASC
                "#,
            )
            .fetch_all(pool)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to list cron jobs: {}", e),
            })?
            .into_iter()
            .map(row_to_json)
            .collect()
        };

        let text = serde_json::to_string_pretty(&rows).map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;

        Ok(JsonToolOutput { text }.into_stream())
    }
}

fn row_to_json(row: sqlx::sqlite::SqliteRow) -> serde_json::Value {
    use sqlx::Row;
    serde_json::json!({
        "id": row.get::<String, _>("id"),
        "schedule": row.get::<String, _>("schedule"),
        "task": row.get::<String, _>("task_description"),
        "session_id": row.get::<Option<String>, _>("session_id"),
        "created_at": row.get::<String, _>("created_at"),
        "last_run_at": row.get::<Option<String>, _>("last_run_at"),
        "run_count": row.get::<i64, _>("run_count"),
        "enabled": row.get::<i64, _>("enabled") == 1,
    })
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
        "CronList",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Optional session id to filter results"
                }
            }
        }),
    )
}
