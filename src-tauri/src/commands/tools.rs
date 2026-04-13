//! Tool execution commands
//!
//! `execute_tool` applies the same [`permissions.deny`](crate::domain::permissions::tool_rules)
//! merge as chat (`send_message` / `execute_tool_calls`), so IPC cannot bypass tool blocks.

use super::CommandResult;
use crate::app_state::OmigaAppState;
use crate::domain::permissions::{
    load_merged_permission_deny_rule_entries, matching_deny_entry,
};

use crate::domain::tools::{Tool, ToolContext};
use crate::errors::{AppError, ToolError};
use futures::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

// ─── 全局取消令牌注册表 ────────────────────────────────────────────────────────

static TOOL_CANCEL_MAP: OnceLock<TokioMutex<HashMap<String, CancellationToken>>> =
    OnceLock::new();

fn tool_cancel_map() -> &'static TokioMutex<HashMap<String, CancellationToken>> {
    TOOL_CANCEL_MAP.get_or_init(|| TokioMutex::new(HashMap::new()))
}

// ─── execute_tool ─────────────────────────────────────────────────────────────

/// 工具流事件名称：`tool-stream-{tool_id}`
pub fn tool_stream_event(tool_id: &str) -> String {
    format!("tool-stream-{}", tool_id)
}

/// Execute a tool and stream results to the frontend via Tauri events.
///
/// 返回一个 `tool_id`；前端订阅 `tool-stream-{tool_id}` 事件接收
/// [`StreamOutputItem`](crate::infrastructure::streaming::StreamOutputItem) 帧，
/// 直到收到 `Complete` 或 `Error` 帧为止。
#[tauri::command]
pub async fn execute_tool(
    app: AppHandle,
    state: State<'_, OmigaAppState>,
    tool: Tool,
    project_root: String,
) -> CommandResult<ToolExecutionResponse> {
    let project = Path::new(&project_root);
    let name = tool.name();
    
    // 新权限系统检查 (PermissionManager)
    let permission_manager = state.permission_manager.clone();
    let args_value = serde_json::to_value(&tool)
        .unwrap_or_else(|_| serde_json::json!({"tool_name": name}));
    let session_id = "execute_tool"; // IPC 调用使用固定 session_id
    
    let perm_decision = permission_manager
        .check_tool(session_id, name, &args_value)
        .await;
    
    match perm_decision {
        crate::domain::permissions::PermissionDecision::Deny(ref reason) => {
            tracing::warn!(
                tool = %name,
                reason = %reason,
                "Tool denied by permission manager"
            );
            return Err(AppError::Tool(ToolError::PermissionDenied {
                action: format!("Tool `{name}` is denied: {}", reason),
            }));
        }
        crate::domain::permissions::PermissionDecision::RequireApproval(ref req) => {
            tracing::info!(
                tool = %name,
                risk_level = ?req.risk.level,
                "Tool requires user approval"
            );
            // Emit permission request event to frontend
            // Risk level uses lowercase to match frontend RiskLevel type
            let risk_level_str = match req.risk.level {
                crate::domain::permissions::RiskLevel::Safe => "safe",
                crate::domain::permissions::RiskLevel::Low => "low",
                crate::domain::permissions::RiskLevel::Medium => "medium",
                crate::domain::permissions::RiskLevel::High => "high",
                crate::domain::permissions::RiskLevel::Critical => "critical",
            };
            let permission_event = serde_json::json!({
                "type": "permission_request",
                "request_id": req.request_id,
                "tool_name": name,
                "risk_level": risk_level_str,
                "risk_description": req.risk.description,
                "session_id": session_id,
                "arguments": args_value,
            });
            let _ = app.emit("permission-request", &permission_event);
            
            return Err(AppError::Tool(ToolError::PermissionDenied {
                action: format!(
                    "Tool `{name}` requires approval. Risk: {:?}. Please approve in the permission dialog.",
                    req.risk.level
                ),
            }));
        }
        crate::domain::permissions::PermissionDecision::Allow => {
            tracing::debug!(tool = %name, "Tool allowed by permission manager");
        }
    }
    
    // 旧权限系统检查（作为后备）
    let deny_entries = load_merged_permission_deny_rule_entries(project);
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

    let tool_id = uuid::Uuid::new_v4().to_string();
    let cancel_token = CancellationToken::new();

    // 注册取消令牌
    tool_cancel_map()
        .lock()
        .await
        .insert(tool_id.clone(), cancel_token.clone());

    // 构建工具上下文，注入取消令牌
    let web_keys = state.chat.web_search_api_keys.lock().await.clone();
    let mut ctx = ToolContext::new(&project_root).with_web_search_api_keys(web_keys);
    ctx.cancel = cancel_token.clone();

    // 执行工具，获取流
    let stream = match tool.execute(&ctx).await {
        Ok(s) => s,
        Err(e) => {
            tool_cancel_map().lock().await.remove(&tool_id);
            return Err(AppError::Tool(e));
        }
    };

    // 后台消费流，逐帧 emit 给前端
    let app_clone = app.clone();
    let tool_id_clone = tool_id.clone();
    tokio::spawn(async move {
        let event = tool_stream_event(&tool_id_clone);
        let mut s = stream;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    let _ = app_clone.emit(
                        &event,
                        &crate::infrastructure::streaming::StreamOutputItem::Cancelled,
                    );
                    break;
                }
                item = s.next() => {
                    match item {
                        None => break,
                        Some(chunk) => {
                            let done = matches!(
                                chunk,
                                crate::infrastructure::streaming::StreamOutputItem::Complete
                                    | crate::infrastructure::streaming::StreamOutputItem::Error { .. }
                            );
                            let _ = app_clone.emit(&event, &chunk);
                            if done {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 清理注册表
        tool_cancel_map().lock().await.remove(&tool_id_clone);
    });

    Ok(ToolExecutionResponse {
        tool_id,
        status: "streaming".to_string(),
    })
}

/// Response from tool execution
#[derive(Debug, Serialize)]
pub struct ToolExecutionResponse {
    pub tool_id: String,
    /// `"streaming"` — 前端应监听 `tool-stream-{tool_id}` 事件
    pub status: String,
}

// ─── cancel_tool ──────────────────────────────────────────────────────────────

/// Cancel an in-progress tool execution identified by `tool_id`.
///
/// 向对应的 `CancellationToken` 发送取消信号；后台流任务在下一帧检测到后
/// 会 emit `Cancelled` 帧并退出。
#[tauri::command]
pub async fn cancel_tool(tool_id: String) -> CommandResult<()> {
    if let Some(token) = tool_cancel_map().lock().await.get(&tool_id) {
        token.cancel();
        tracing::debug!(target: "omiga::tools", tool_id = %tool_id, "cancel_tool: cancellation signal sent");
    } else {
        tracing::debug!(target: "omiga::tools", tool_id = %tool_id, "cancel_tool: tool_id not found (already finished?)");
    }
    Ok(())
}
