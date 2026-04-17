//! Sleep tool — wait for a specified duration without holding a shell process.
//!
//! Aligns with `src/tools/SleepTool/prompt.ts`.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Maximum allowed sleep duration (1 hour).
const MAX_DURATION_SECS: f64 = 3600.0;

pub const DESCRIPTION: &str = "Wait for a specified duration. The user can interrupt the sleep at any time.\n\
\n\
Use this when the user tells you to sleep or rest, when you have nothing to do, or when you're waiting for something.\n\
\n\
Prefer this over `bash(sleep ...)` — it doesn't hold a shell process.\n\
\n\
You can call this concurrently with other tools — it won't interfere with them.";

/// Arguments for the Sleep tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepArgs {
    /// Duration to sleep in seconds (0 to 3600). Fractional seconds are supported.
    pub duration: f64,
}

/// Sleep tool implementation.
pub struct SleepTool;

#[async_trait]
impl super::ToolImpl for SleepTool {
    type Args = SleepArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        if args.duration < 0.0 {
            return Err(ToolError::InvalidArguments {
                message: "duration must be non-negative".to_string(),
            });
        }
        if args.duration > MAX_DURATION_SECS {
            return Err(ToolError::InvalidArguments {
                message: format!("duration must not exceed {MAX_DURATION_SECS} seconds (1 hour)"),
            });
        }

        let millis = (args.duration * 1000.0) as u64;
        let cancel = ctx.cancel.clone();

        tokio::select! {
            _ = cancel.cancelled() => return Err(ToolError::Cancelled),
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(millis)) => {}
        }

        Ok(SleepOutput {
            duration: args.duration,
        }
        .into_stream())
    }
}

pub struct SleepOutput {
    pub duration: f64,
}

impl StreamOutput for SleepOutput {
    fn into_stream(self) -> Pin<Box<dyn futures::Stream<Item = StreamOutputItem> + Send>> {
        use futures::stream;
        Box::pin(stream::iter(vec![
            StreamOutputItem::Start,
            StreamOutputItem::Content(format!("Slept for {:.3}s", self.duration)),
            StreamOutputItem::Complete,
        ]))
    }
}

pub fn schema() -> ToolSchema {
    ToolSchema::new(
        "sleep",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "duration": {
                    "type": "number",
                    "description": "Duration to sleep in seconds (0 to 3600). Fractional seconds are supported."
                }
            },
            "required": ["duration"]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolImpl;
    use futures::StreamExt;

    #[tokio::test]
    async fn sleep_zero_completes_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(dir.path());
        let mut stream = SleepTool::execute(&ctx, SleepArgs { duration: 0.0 })
            .await
            .unwrap();
        let mut saw_complete = false;
        while let Some(item) = stream.next().await {
            if matches!(item, StreamOutputItem::Complete) {
                saw_complete = true;
            }
        }
        assert!(saw_complete);
    }

    #[tokio::test]
    async fn negative_duration_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(dir.path());
        let result = SleepTool::execute(&ctx, SleepArgs { duration: -1.0 }).await;
        assert!(matches!(result, Err(ToolError::InvalidArguments { .. })));
    }

    #[tokio::test]
    async fn too_large_duration_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(dir.path());
        let result = SleepTool::execute(&ctx, SleepArgs { duration: 7200.0 }).await;
        assert!(matches!(result, Err(ToolError::InvalidArguments { .. })));
    }

    #[tokio::test]
    async fn cancellation_returns_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(dir.path());
        ctx.cancel.cancel();
        let result = SleepTool::execute(&ctx, SleepArgs { duration: 60.0 }).await;
        assert!(matches!(result, Err(ToolError::Cancelled)));
    }
}
