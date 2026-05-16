//! `PushNotification` tool — aligned with `PushNotificationTool` in Claude Code.
//!
//! Sends a desktop notification via platform-native commands so no AppHandle is needed.

use super::{ToolContext, ToolError, ToolSchema};
use crate::infrastructure::streaming::{StreamOutput, StreamOutputItem};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub const DESCRIPTION: &str =
    "Send a desktop push notification to the user. Use for long-running tasks that complete in the background.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNotificationArgs {
    /// Notification title shown in bold
    pub title: String,
    /// Notification body text
    pub body: String,
}

pub struct PushNotificationTool;

#[async_trait]
impl super::ToolImpl for PushNotificationTool {
    type Args = PushNotificationArgs;

    const DESCRIPTION: &'static str = DESCRIPTION;

    async fn execute(
        _ctx: &ToolContext,
        args: Self::Args,
    ) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
        send_desktop_notification(&args.title, &args.body);

        let text = serde_json::to_string_pretty(&serde_json::json!({
            "sent": true,
            "title": args.title,
            "body": args.body,
        }))
        .map_err(|e| ToolError::ExecutionFailed {
            message: e.to_string(),
        })?;

        Ok(PushNotificationOutput { text }.into_stream())
    }
}

fn send_desktop_notification(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!("display notification {:?} with title {:?}", body, title);
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([title, body])
            .output();
    }

    #[cfg(target_os = "windows")]
    {
        // Pass title and body via environment variables to avoid any shell injection.
        // The PowerShell script reads $env:OMIGA_NOTIF_TITLE and $env:OMIGA_NOTIF_BODY.
        let script = r#"[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); $t.GetElementsByTagName('text')[0].AppendChild($t.CreateTextNode($env:OMIGA_NOTIF_TITLE)); $t.GetElementsByTagName('text')[1].AppendChild($t.CreateTextNode($env:OMIGA_NOTIF_BODY)); [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Omiga').Show([Windows.UI.Notifications.ToastNotification]::new($t))"#;
        let _ = std::process::Command::new("powershell")
            .args(["-Command", script])
            .env("OMIGA_NOTIF_TITLE", title)
            .env("OMIGA_NOTIF_BODY", body)
            .output();
    }
}

struct PushNotificationOutput {
    text: String,
}

impl StreamOutput for PushNotificationOutput {
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
        "PushNotification",
        DESCRIPTION,
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Notification title" },
                "body":  { "type": "string", "description": "Notification body text" }
            },
            "required": ["title", "body"]
        }),
    )
}
