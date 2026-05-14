//! `CronDelete` — soft-delete a scheduled cron job by id.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str = "Delete a scheduled cron job by id.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronDeleteArgs {
    pub id: String,
}

pub struct CronDeleteTool;

#[async_trait]
impl super::ToolImpl for CronDeleteTool {
    type Args = CronDeleteArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        let id = args.id.trim().to_string();

        if id.is_empty() {
            return Err(ToolError::InvalidArguments {
                message: "`id` must not be empty.".to_string(),
            });
        }

        let Some(pool) = ctx.db.as_ref() else {
            return Err(ToolError::ExecutionFailed {
                message: "Database is not available in this context.".to_string(),
            });
        };

        let result = sqlx::query(
            r#"UPDATE cron_jobs SET enabled = 0 WHERE id = ? AND enabled = 1"#,
        )
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("Failed to delete cron job: {}", e),
        })?;

        let deleted = result.rows_affected() > 0;

        let text = serde_json::to_string_pretty(&serde_json::json!({
            "deleted": deleted,
            "id": id
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
        "CronDelete",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The id of the cron job to delete"
                }
            },
            "required": ["id"]
        }),
    )
}
