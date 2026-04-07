//! Tool execution commands
//!
//! `execute_tool` applies the same [`permissions.deny`](crate::domain::tool_permission_rules)
//! merge as chat (`send_message` / `execute_tool_calls`), so IPC cannot bypass tool blocks.

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::domain::tool_permission_rules::{
    load_merged_permission_deny_rule_entries, matching_deny_entry,
};
use crate::domain::tools::{Tool, ToolContext};
use crate::errors::{AppError, ToolError};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, State};

/// Execute a tool and stream results
#[tauri::command]
pub async fn execute_tool(
    _app: AppHandle,
    state: State<'_, OmigaAppState>,
    tool: Tool,
    project_root: String,
) -> CommandResult<ToolExecutionResponse> {
    let project = Path::new(&project_root);
    let deny_entries = load_merged_permission_deny_rule_entries(project);
    let name = tool.name();
    if let Some(hit) = matching_deny_entry(name, &deny_entries) {
        tracing::debug!(
            target: "omiga::permissions",
            tool = %name,
            matched_rule = %hit.rule,
            source = %hit.source.display(),
            "execute_tool IPC blocked by permissions.deny"
        );
        return Err(AppError::Tool(ToolError::PermissionDenied {
            action: format!(
                "Tool `{name}` is denied by permissions.deny (rule `{}` from {})",
                hit.rule,
                hit.source.display()
            ),
        }));
    }

    let brave = state.chat.brave_search_api_key.lock().await.clone();
    let ctx = ToolContext::new(&project_root).with_brave_search_api_key(brave);

    // Execute the tool and get stream
    let _stream = tool.execute(&ctx).await?;

    // Stream results to frontend
    // TODO: Actually consume the stream and emit events

    Ok(ToolExecutionResponse {
        tool_id: uuid::Uuid::new_v4().to_string(),
        status: "pending".to_string(),
    })
}

/// Response from tool execution
#[derive(Debug, Serialize)]
pub struct ToolExecutionResponse {
    pub tool_id: String,
    pub status: String,
}

/// Cancel an in-progress tool execution
#[tauri::command]
pub async fn cancel_tool(_tool_id: String) -> CommandResult<()> {
    // TODO: Implement cancellation via CancellationToken
    Ok(())
}
