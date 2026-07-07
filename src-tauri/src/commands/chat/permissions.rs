//! Permission management and ask-user interactive question helpers.

use super::process_tool_output_for_model;
use crate::app_state::OmigaAppState;
use crate::constants::tool_limits::{
    truncate_utf8_prefix, PREVIEW_SIZE_BYTES, TOOL_DISPLAY_MAX_INPUT_CHARS,
};
use crate::domain::chat_state::{AskUserWaiter, PermissionToolWaiter};
use crate::domain::computer_use::redact_json_value;
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

/// Build a human-readable summary of what the AI is trying to do with this tool call.
fn build_plain_description(tool_name: &str, args_value: &serde_json::Value) -> String {
    let canonical = tool_name.to_ascii_lowercase();
    match canonical.as_str() {
        "bash" | "shell" | "run_bash" | "run_shell" => {
            let cmd = args_value
                .get("command")
                .or_else(|| args_value.get("cmd"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let preview: String = cmd.chars().take(120).collect();
            let suffix = if cmd.chars().count() > 120 { "…" } else { "" };
            format!("AI wants to run: {preview}{suffix}")
        }
        "file_write" | "write_file" => {
            let path = args_value
                .get("path")
                .or_else(|| args_value.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            format!("AI wants to overwrite: {path}")
        }
        "file_edit" | "edit_file" | "str_replace_editor" => {
            let path = args_value
                .get("path")
                .or_else(|| args_value.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            format!("AI wants to modify: {path}")
        }
        _ => format!("AI wants to use tool: {tool_name}"),
    }
}

pub(super) fn build_permission_request_event_json(
    tool_name: &str,
    session_id: &str,
    args_value: &serde_json::Value,
    req: &PermissionRequest,
) -> serde_json::Value {
    let risk_level_str = permission_risk_level_event_str(req.risk.level);
    let display_args = redact_json_value(args_value);
    let plain_description = build_plain_description(tool_name, &display_args);
    serde_json::json!({
        "type": "permission_request",
        "request_id": req.request_id,
        "tool_name": tool_name,
        "risk_level": risk_level_str,
        "risk_description": req.risk.description,
        "plain_description": plain_description,
        "session_id": session_id,
        "project_root": req.context.project_root.as_ref().map(|root| root.to_string_lossy().to_string()),
        "arguments": display_args,
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
pub(super) struct PermissionToolResolutionRequest<'a> {
    pub app: &'a AppHandle,
    pub app_state: &'a OmigaAppState,
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub tool_use_id: &'a str,
    pub stream_tool_name: &'a str,
    pub tool_name_for_event: &'a str,
    pub arguments_display: &'a str,
    pub args_value: &'a serde_json::Value,
    pub req: &'a PermissionRequest,
    pub cancel_flag: Option<Arc<RwLock<bool>>>,
}

pub(super) async fn wait_for_permission_tool_resolution(
    request: PermissionToolResolutionRequest<'_>,
) -> Result<(), String> {
    let PermissionToolResolutionRequest {
        app,
        app_state,
        session_id,
        message_id,
        tool_use_id,
        stream_tool_name,
        tool_name_for_event,
        arguments_display,
        args_value,
        req,
        cancel_flag,
    } = request;
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
                context: req.context.clone(),
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

const REPAIRED_ASK_USER_HEADER_MAX_CHARS: usize = 12;
const REPAIRED_ASK_USER_MAX_QUESTIONS: usize = 4;
const REPAIRED_ASK_USER_MAX_OPTIONS: usize = 5;

fn parse_or_repair_ask_user_question_args(
    arguments: &str,
) -> Result<ask_user_question::AskUserQuestionArgs, String> {
    match serde_json::from_str::<ask_user_question::AskUserQuestionArgs>(arguments) {
        Ok(args) => match ask_user_question::validate_ask_user_question_args(&args) {
            Ok(()) => Ok(args),
            Err(validation_err) => repair_open_ended_ask_user_question_args(arguments)?
                .ok_or_else(|| format!("Invalid ask_user_question arguments: {}", validation_err)),
        },
        Err(parse_err) => repair_open_ended_ask_user_question_args(arguments)?
            .ok_or_else(|| format!("Failed to parse ask_user_question arguments: {}", parse_err)),
    }
}

fn repair_open_ended_ask_user_question_args(
    arguments: &str,
) -> Result<Option<ask_user_question::AskUserQuestionArgs>, String> {
    let value: serde_json::Value = serde_json::from_str(arguments)
        .map_err(|e| format!("Failed to parse ask_user_question arguments: {}", e))?;
    let Some(items) = value.get("questions").and_then(|v| v.as_array()) else {
        return Ok(None);
    };
    if items.is_empty() {
        return Ok(None);
    }

    let mut questions = Vec::new();
    let mut repaired = items.len() > REPAIRED_ASK_USER_MAX_QUESTIONS;
    for (index, item) in items
        .iter()
        .take(REPAIRED_ASK_USER_MAX_QUESTIONS)
        .enumerate()
    {
        match item {
            serde_json::Value::String(text) => {
                repaired = true;
                questions.push(open_ended_ask_user_question(text, index));
            }
            serde_json::Value::Object(map) => {
                let raw_question = ask_user_string_field(map, "question");
                let question = raw_question
                    .clone()
                    .or_else(|| ask_user_string_field(map, "prompt"))
                    .or_else(|| ask_user_string_field(map, "label"))
                    .unwrap_or_else(|| format!("请补充第 {} 项信息？", index + 1));
                if raw_question.is_none() {
                    repaired = true;
                }
                let raw_header = ask_user_string_field(map, "header");
                let header = raw_header
                    .as_deref()
                    .map(|h| compact_ask_user_header(h, index))
                    .unwrap_or_else(|| compact_ask_user_header(&question, index));
                if raw_header.as_deref() != Some(header.as_str()) {
                    repaired = true;
                }
                let multi_select = map
                    .get("multiSelect")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut options =
                    coerce_ask_user_options(map.get("options"), multi_select, &mut repaired);
                if options.len() < 2 {
                    repaired = true;
                    options = open_ended_ask_user_options(&question);
                }
                if options.len() > REPAIRED_ASK_USER_MAX_OPTIONS {
                    repaired = true;
                    options.truncate(REPAIRED_ASK_USER_MAX_OPTIONS);
                }
                let param = ask_user_string_field(map, "param");
                let show_when = map
                    .get("showWhen")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                questions.push(ask_user_question::QuestionItem {
                    question: ensure_question_mark(question),
                    header,
                    options,
                    multi_select,
                    param,
                    show_when,
                });
            }
            _ => {
                repaired = true;
            }
        }
    }

    if questions.is_empty() || !repaired {
        return Ok(None);
    }

    let repaired_args = ask_user_question::AskUserQuestionArgs {
        questions,
        answers: value.get("answers").cloned(),
        annotations: value.get("annotations").cloned(),
        metadata: value.get("metadata").cloned(),
    };

    ask_user_question::validate_ask_user_question_args(&repaired_args)
        .map_err(|e| format!("Invalid repaired ask_user_question arguments: {}", e))?;
    Ok(Some(repaired_args))
}

fn ask_user_string_field(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    map.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn compact_ask_user_header(value: &str, index: usize) -> String {
    let trimmed = value.trim().trim_end_matches(['?', '？', ':', '：']).trim();
    let fallback;
    let raw = if trimmed.is_empty() {
        fallback = format!("信息{}", index + 1);
        fallback.as_str()
    } else {
        trimmed
    };
    raw.chars()
        .take(REPAIRED_ASK_USER_HEADER_MAX_CHARS)
        .collect::<String>()
}

fn ensure_question_mark(value: String) -> String {
    let trimmed = value.trim();
    if trimmed.ends_with('?') || trimmed.ends_with('？') {
        trimmed.to_string()
    } else {
        format!("{}？", trimmed)
    }
}

fn coerce_ask_user_options(
    value: Option<&serde_json::Value>,
    multi_select: bool,
    repaired: &mut bool,
) -> Vec<ask_user_question::QuestionOption> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Ok(mut options) =
        serde_json::from_value::<Vec<ask_user_question::QuestionOption>>(value.clone())
    else {
        *repaired = true;
        return Vec::new();
    };

    for (index, option) in options.iter_mut().enumerate() {
        if option.label.trim().is_empty() {
            *repaired = true;
            option.label = format!("选项{}", index + 1);
        }
        if option.description.trim().is_empty() {
            *repaired = true;
            option.description = option.label.clone();
        }
        if multi_select {
            if option.preview.is_some() || option.custom || option.custom_placeholder.is_some() {
                *repaired = true;
            }
            option.preview = None;
            option.custom = false;
            option.custom_placeholder = None;
        } else if !option.custom && option.custom_placeholder.is_some() {
            *repaired = true;
            option.custom_placeholder = None;
        }
    }

    options
}

fn open_ended_ask_user_question(question: &str, index: usize) -> ask_user_question::QuestionItem {
    ask_user_question::QuestionItem {
        question: ensure_question_mark(question.to_string()),
        header: compact_ask_user_header(question, index),
        options: open_ended_ask_user_options(question),
        multi_select: false,
        param: None,
        show_when: None,
    }
}

fn open_ended_ask_user_options(question: &str) -> Vec<ask_user_question::QuestionOption> {
    vec![
        ask_user_question::QuestionOption {
            label: "我来填写".to_string(),
            description: "直接输入这项具体信息。".to_string(),
            preview: None,
            recommended: true,
            custom: true,
            custom_placeholder: Some(question.trim().to_string()),
        },
        ask_user_question::QuestionOption {
            label: "暂不确定".to_string(),
            description: "先记录为待补充信息，让 Omiga 继续说明需要什么。".to_string(),
            preview: None,
            recommended: false,
            custom: false,
            custom_placeholder: None,
        },
    ]
}

/// Chat path: block until the user submits answers in the Omiga UI (or cancel).
pub(super) struct AskUserQuestionExecution<'a> {
    pub tool_use_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub app: AppHandle,
    pub message_id: String,
    pub session_id: String,
    pub tool_results_dir: &'a Path,
    pub waiters: Arc<Mutex<HashMap<String, AskUserWaiter>>>,
    pub cancel_flag: Option<Arc<RwLock<bool>>>,
}

pub(super) async fn execute_ask_user_question_interactive(
    request: AskUserQuestionExecution<'_>,
) -> (String, String, bool) {
    let AskUserQuestionExecution {
        tool_use_id,
        tool_name,
        arguments,
        app,
        message_id,
        session_id,
        tool_results_dir,
        waiters,
        cancel_flag,
    } = request;
    let args = match parse_or_repair_ask_user_question_args(&arguments) {
        Ok(args) => args,
        Err(error_msg) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::permissions::types::{
        PermissionContext, PermissionMode, PermissionRequest, RiskAssessment, RiskLevel,
    };
    use serde_json::json;

    #[test]
    fn repairs_open_ended_ask_user_question_into_custom_picker() {
        let args = json!({
            "questions": [
                {
                    "question": "研究问题是什么？",
                    "header": "研究问题"
                }
            ]
        });

        let repaired = parse_or_repair_ask_user_question_args(&args.to_string()).unwrap();

        assert_eq!(repaired.questions.len(), 1);
        assert_eq!(repaired.questions[0].question, "研究问题是什么？");
        assert_eq!(repaired.questions[0].options.len(), 2);
        assert_eq!(repaired.questions[0].options[0].label, "我来填写");
        assert!(repaired.questions[0].options[0].custom);
        assert!(repaired.questions[0].options[0].recommended);
        ask_user_question::validate_ask_user_question_args(&repaired).unwrap();
    }

    #[test]
    fn repairs_string_questions_from_model_tool_args() {
        let args = json!({
            "questions": [
                "研究对象或领域是什么？",
                "关键词或时间范围是什么？"
            ],
            "metadata": { "source": "test" }
        });

        let repaired = parse_or_repair_ask_user_question_args(&args.to_string()).unwrap();

        assert_eq!(repaired.questions.len(), 2);
        assert_eq!(repaired.questions[0].header, "研究对象或领域是什么");
        assert!(repaired.questions[1].options[0].custom);
        assert_eq!(repaired.metadata, Some(json!({ "source": "test" })));
        ask_user_question::validate_ask_user_question_args(&repaired).unwrap();
    }

    #[test]
    fn keeps_valid_ask_user_question_args_unchanged() {
        let args = json!({
            "questions": [
                {
                    "question": "选择综述范围？",
                    "header": "范围",
                    "options": [
                        { "label": "近五年", "description": "聚焦 2021 年后的研究。" },
                        { "label": "全时期", "description": "覆盖历史背景和最新研究。" }
                    ]
                }
            ]
        });

        let parsed = parse_or_repair_ask_user_question_args(&args.to_string()).unwrap();

        assert_eq!(parsed.questions[0].options[0].label, "近五年");
        assert!(!parsed.questions[0].options[0].custom);
    }

    #[test]
    fn permission_request_event_redacts_display_arguments() {
        let args = json!({
            "command": "echo token=ghp_1234567890abcdef && export OPENAI_API_KEY=sk-1234567890abcdef",
            "token": "secret-token-value"
        });
        let req = PermissionRequest {
            request_id: "request-redact".to_string(),
            context: PermissionContext {
                tool_name: "bash".to_string(),
                arguments: args.clone(),
                session_id: "session-redact".to_string(),
                file_paths: None,
                timestamp: chrono::Utc::now(),
                project_root: Some(std::path::PathBuf::from("/tmp/project")),
            },
            risk: RiskAssessment {
                level: RiskLevel::High,
                categories: Vec::new(),
                description: "risk".to_string(),
                recommendations: Vec::new(),
                detected_risks: Vec::new(),
            },
            suggested_mode: PermissionMode::AskEveryTime,
        };

        let event = build_permission_request_event_json("bash", "session-redact", &args, &req);
        let serialized = event.to_string();

        assert!(!serialized.contains("secret-token-value"));
        assert!(!serialized.contains("ghp_1234567890abcdef"));
        assert!(!serialized.contains("sk-1234567890abcdef"));
        assert!(serialized.contains("[REDACTED]"));
        assert_eq!(event["project_root"], "/tmp/project");
    }
}
