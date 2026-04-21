//! Permission management and ask-user interactive question helpers.

use super::process_tool_output_for_model;
use crate::app_state::OmigaAppState;
use crate::constants::tool_limits::{
    truncate_utf8_prefix, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::chat_state::{AskUserWaiter, PermissionToolWaiter};
use crate::domain::permissions::PermissionRequest;
use crate::domain::tools::ask_user_question;
use crate::infrastructure::streaming::StreamOutputItem;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
pub(super) fn matches_ask_user_question_name(name: &str) -> bool {
    let n = name.trim();
    n.eq_ignore_ascii_case("ask_user_question") || n.eq_ignore_ascii_case("AskUserQuestion")
}

pub(super) fn ask_user_waiter_key(session_id: &str, message_id: &str, tool_use_id: &str) -> String {
    format!("{}\x1f{}\x1f{}", session_id, message_id, tool_use_id)
}

pub(super) async fn cancel_ask_user_waiters_for_message(
    waiters: &Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    session_id: &str,
    message_id: &str,
) {
    let prefix = format!("{}\x1f{}\x1f", session_id, message_id);
    let mut map = waiters.lock().await;
    let keys: Vec<String> = map
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .cloned()
        .collect();
    for k in keys {
        if let Some(w) = map.remove(&k) {
            let _ =
                w.tx.send(Err("User cancelled before answering.".to_string()));
        }
    }
}

pub(super) async fn cancel_permission_tool_waiters_for_message(
    waiters: &Arc<Mutex<HashMap<String, PermissionToolWaiter>>>,
    session_id: &str,
    message_id: &str,
) {
    let mut map = waiters.lock().await;
    let keys: Vec<String> = map
        .iter()
        .filter(|(_, w)| w.session_id == session_id && w.message_id == message_id)
        .map(|(k, _)| k.clone())
        .collect();
    for k in keys {
        if let Some(w) = map.remove(&k) {
            let _ = w.tx.send(Err("用户已取消".to_string()));
        }
    }
}

pub(super) async fn cancel_permission_tool_waiters_for_session(
    waiters: &Arc<Mutex<HashMap<String, PermissionToolWaiter>>>,
    session_id: &str,
) {
    let mut map = waiters.lock().await;
    let keys: Vec<String> = map
        .iter()
        .filter(|(_, w)| w.session_id == session_id)
        .map(|(k, _)| k.clone())
        .collect();
    for k in keys {
        if let Some(w) = map.remove(&k) {
            let _ = w.tx.send(Err("会话已关闭".to_string()));
        }
    }
}

pub(super) fn permission_risk_level_event_str(
    level: crate::domain::permissions::RiskLevel,
) -> &'static str {
    match level {
        crate::domain::permissions::RiskLevel::Safe => "safe",
        crate::domain::permissions::RiskLevel::Low => "low",
        crate::domain::permissions::RiskLevel::Medium => "medium",
        crate::domain::permissions::RiskLevel::High => "high",
        crate::domain::permissions::RiskLevel::Critical => "critical",
    }
}

pub(super) fn build_permission_request_event_json(
    tool_name: &str,
    session_id: &str,
    args_value: &serde_json::Value,
    req: &PermissionRequest,
) -> serde_json::Value {
    let risk_level_str = permission_risk_level_event_str(req.risk.level);
    serde_json::json!({
        "type": "permission_request",
        "request_id": req.request_id,
        "tool_name": tool_name,
        "risk_level": risk_level_str,
        "risk_description": req.risk.description,
        "session_id": session_id,
        "arguments": args_value.clone(),
        "detected_risks": req.risk.detected_risks.iter().map(|r| {
            let severity_str = permission_risk_level_event_str(r.severity);
            serde_json::json!({
                "category": format!("{:?}", r.category),
                "severity": severity_str,
                "description": r.description,
                "mitigation": r.mitigation,
            })
        }).collect::<Vec<_>>(),
        "recommendations": req.risk.recommendations,
    })
}

/// Register a waiter, emit `permission-request`, then block until approve/deny/cancel.
pub(super) async fn wait_for_permission_tool_resolution(
    app: &AppHandle,
    app_state: &OmigaAppState,
    session_id: &str,
    message_id: &str,
    tool_use_id: &str,
    stream_tool_name: &str,
    tool_name_for_event: &str,
    arguments_display: &str,
    args_value: &serde_json::Value,
    req: &PermissionRequest,
    cancel_flag: Option<Arc<RwLock<bool>>>,
) -> Result<(), String> {
    let request_id = req.request_id.clone();
    let (tx, mut rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    {
        let mut map = app_state.chat.permission_tool_waiters.lock().await;
        map.insert(
            request_id.clone(),
            PermissionToolWaiter {
                tx,
                session_id: session_id.to_string(),
                message_id: message_id.to_string(),
            },
        );
    }

    let permission_event =
        build_permission_request_event_json(tool_name_for_event, session_id, args_value, req);
    let _ = app.emit("permission-request", &permission_event);

    let pending_msg = format!(
        "⏳ 需要权限确认: {}\n\n风险级别: {:?}\n{}\n\n请在输入框上方批准或拒绝。",
        tool_name_for_event, req.risk.level, req.risk.description
    );
    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            name: stream_tool_name.to_string(),
            input: arguments_display.to_string(),
            output: pending_msg,
            is_error: false,
        },
    );

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(120));
    interval.tick().await;
    let outcome = loop {
        tokio::select! {
            res = &mut rx => break res,
            _ = interval.tick() => {
                if let Some(ref f) = cancel_flag {
                    if *f.read().await {
                        let mut map = app_state.chat.permission_tool_waiters.lock().await;
                        map.remove(&request_id);
                        return Err("用户已取消".to_string());
                    }
                }
            }
        }
    };

    {
        let mut map = app_state.chat.permission_tool_waiters.lock().await;
        map.remove(&request_id);
    }

    match outcome {
        Ok(inner) => inner,
        Err(_) => Err("权限确认通道意外关闭".to_string()),
    }
}

pub(super) fn build_ask_user_success_output(
    questions: &[ask_user_question::QuestionItem],
    answers: &serde_json::Value,
) -> Result<String, String> {
    let obj = answers
        .as_object()
        .ok_or_else(|| "answers must be a JSON object".to_string())?;
    for q in questions {
        let qt = q.question.trim();
        if !obj.contains_key(qt) {
            return Err(format!("Missing answer for question: {}", q.question));
        }
    }
    let mut body = serde_json::Map::new();
    body.insert(
        "questions".to_string(),
        serde_json::to_value(questions).map_err(|e| e.to_string())?,
    );
    body.insert(
        "answers".to_string(),
        serde_json::Value::Object(obj.clone()),
    );
    body.insert(
        "_omiga".to_string(),
        serde_json::json!("User answered via Omiga chat UI."),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(body)).map_err(|e| e.to_string())
}

/// Chat path: block until the user submits answers in the Omiga UI (or cancel).
pub(super) async fn execute_ask_user_question_interactive(
    tool_use_id: String,
    tool_name: String,
    arguments: String,
    app: AppHandle,
    message_id: String,
    session_id: String,
    tool_results_dir: &Path,
    waiters: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    cancel_flag: Option<Arc<RwLock<bool>>>,
) -> (String, String, bool) {
    let args: ask_user_question::AskUserQuestionArgs = match serde_json::from_str(&arguments) {
        Err(e) => {
            let error_msg = format!("Failed to parse ask_user_question arguments: {}", e);
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: arguments.clone(),
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id, error_msg, true);
        }
        Ok(a) => a,
    };
    if let Err(e) = ask_user_question::validate_ask_user_question_args(&args) {
        let error_msg = format!("Invalid ask_user_question arguments: {}", e);
        let _ = app.emit(
            &format!("chat-stream-{}", message_id),
            &StreamOutputItem::ToolResult {
                tool_use_id: tool_use_id.clone(),
                name: tool_name.clone(),
                input: arguments.clone(),
                output: error_msg.clone(),
                is_error: true,
            },
        );
        return (tool_use_id, error_msg, true);
    }

    let key = ask_user_waiter_key(&session_id, &message_id, &tool_use_id);
    let (tx, mut rx) = tokio::sync::oneshot::channel::<Result<serde_json::Value, String>>();
    {
        let mut map = waiters.lock().await;
        map.insert(key.clone(), AskUserWaiter { tx });
    }

    let questions_value = match serde_json::to_value(&args.questions) {
        Ok(v) => v,
        Err(e) => {
            let mut map = waiters.lock().await;
            map.remove(&key);
            let error_msg = format!("Failed to serialize questions: {}", e);
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: arguments.clone(),
                    output: error_msg.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id, error_msg, true);
        }
    };

    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::AskUserPending {
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            tool_use_id: tool_use_id.clone(),
            questions: questions_value,
        },
    );

    let mut interval = tokio::time::interval(std::time::Duration::from_millis(120));
    interval.tick().await;
    let outcome = loop {
        tokio::select! {
            res = &mut rx => {
                break res;
            }
            _ = interval.tick() => {
                if let Some(ref f) = cancel_flag {
                    if *f.read().await {
                        let mut map = waiters.lock().await;
                        if let Some(w) = map.remove(&key) {
                            let _ = w.tx.send(Err(
                                "User cancelled before answering.".to_string(),
                            ));
                        }
                    }
                }
            }
        }
    };

    {
        let mut map = waiters.lock().await;
        map.remove(&key);
    }

    let answers_val = match outcome {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: arguments.clone(),
                    output: e.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id, e, true);
        }
        Err(_) => {
            let err = "Ask user channel closed unexpectedly.".to_string();
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: arguments.clone(),
                    output: err.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id, err, true);
        }
    };

    let output_text = match build_ask_user_success_output(&args.questions, &answers_val) {
        Ok(s) => s,
        Err(e) => {
            let _ = app.emit(
                &format!("chat-stream-{}", message_id),
                &StreamOutputItem::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    name: tool_name.clone(),
                    input: arguments.clone(),
                    output: e.clone(),
                    is_error: true,
                },
            );
            return (tool_use_id, e, true);
        }
    };

    let display_output = if output_text.len() > PREVIEW_SIZE_BYTES {
        let prefix = truncate_utf8_prefix(&output_text, PREVIEW_SIZE_BYTES);
        format!(
            "{}\n\n[Output truncated... {} total characters]",
            prefix,
            output_text.len()
        )
    } else {
        output_text.clone()
    };
    let display_input = if arguments.len() > TOOL_DISPLAY_MAX_INPUT_CHARS {
        let prefix = truncate_utf8_prefix(&arguments, TOOL_DISPLAY_MAX_INPUT_CHARS);
        format!(
            "{}\n\n[Input truncated... {} total characters]",
            prefix,
            arguments.len()
        )
    } else {
        arguments.clone()
    };

    let _ = app.emit(
        &format!("chat-stream-{}", message_id),
        &StreamOutputItem::ToolResult {
            tool_use_id: tool_use_id.clone(),
            name: tool_name.clone(),
            input: display_input,
            output: display_output,
            is_error: false,
        },
    );

    let model_output =
        process_tool_output_for_model(output_text.clone(), &tool_use_id, tool_results_dir).await;
    (tool_use_id, model_output, false)
}
