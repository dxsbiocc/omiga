use super::super::super::permissions::{
    wait_for_permission_tool_resolution, PermissionToolResolutionRequest,
};
use super::super::dispatch::ToolDispatchContext;
use crate::app_state::OmigaAppState;
use crate::domain::permissions::{
    DetectedRisk, PermissionContext, PermissionMode, PermissionRequest, RiskAssessment,
    RiskCategory, RiskLevel,
};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};

const SANDBOX_DENIED_PREFIX: &str = "SANDBOX_DENIED:";
const TOOL_FAILURE_WRAPPER_PREFIX: &str = "Tool execution failed: ";
const SANDBOX_ESCALATION_CONTEXT: &str = "sandbox-escalation";
const SANDBOX_ESCALATION_APPROVED_NOTE: &str = "已经用户批准，本次无沙箱重跑。";
const SANDBOX_ESCALATION_REJECTED_NOTE: &str = "用户拒绝无沙箱重跑。";
const SANDBOX_ESCALATION_CANCELLED_AFTER_APPROVAL_NOTE: &str =
    "审批完成前会话已取消,未执行无沙箱重跑。";
const MAX_TOOL_FAILURE_WRAPPER_STRIP_DEPTH: usize = 3;

pub(super) fn is_exec_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "bash"
            | "exec"
            | "exec_session"
            | "exec_session_create"
            | "exec_session_write"
            | "task_create"
            | "task_get"
            | "task_update"
            | "task_list"
            | "task_output"
            | "task_stop"
    )
}

pub(super) async fn handle_exec_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    let original = super::execute_domain_tool(ctx).await;
    handle_sandbox_escalation_if_needed(ctx, original).await
}

async fn handle_sandbox_escalation_if_needed(
    ctx: &ToolDispatchContext<'_>,
    original: (String, String, bool),
) -> (String, String, bool) {
    let (tool_use_id, output, is_error) = original;
    if !is_sandbox_denied_result(is_error, &output) {
        return (tool_use_id, output, is_error);
    }

    let Ok(args_value) = serde_json::from_str::<Value>(ctx.arguments) else {
        return (tool_use_id, output, is_error);
    };

    if !sandbox_escalation_preconditions_met(
        ctx.tool_name,
        &args_value,
        sandbox_escalation_enabled_from_env(),
        false,
    ) {
        return (tool_use_id, output, is_error);
    }

    if !claim_sandbox_escalation_attempt(ctx.tool_use_id) {
        return (tool_use_id, output, is_error);
    }

    let Some(app_state) = ctx.app.try_state::<OmigaAppState>() else {
        let fallback = append_sandbox_escalation_unavailable_note(
            &output,
            "无法获取应用状态，未能发起无沙箱重跑审批",
        );
        return (tool_use_id, fallback, true);
    };

    let req = build_sandbox_escalation_permission_request(ctx, &args_value, &output);
    let approval = wait_for_permission_tool_resolution(PermissionToolResolutionRequest {
        app: ctx.app,
        app_state: &app_state,
        session_id: ctx.session_id,
        message_id: ctx.message_id,
        tool_use_id: ctx.tool_use_id,
        stream_tool_name: ctx.tool_name,
        tool_name_for_event: ctx.tool_name,
        arguments_display: ctx.arguments,
        args_value: &args_value,
        req: &req,
        cancel_flag: ctx.cancel_flag.clone(),
    })
    .await;

    if let Err(reason) = approval {
        let rejected = append_sandbox_escalation_rejected_note_with_reason(&output, &reason);
        emit_final_sandbox_escalation_tool_result(ctx, &rejected);
        return (tool_use_id, rejected, true);
    }

    if sandbox_escalation_retry_cancelled(&ctx.cancel_flag, &ctx.round_cancel).await {
        let cancelled = append_sandbox_escalation_cancelled_after_approval_note(&output);
        emit_final_sandbox_escalation_tool_result(ctx, &cancelled);
        return (tool_use_id, cancelled, true);
    }

    let retry_arguments = match inject_disable_sandbox_arg(ctx.arguments) {
        Ok(arguments) => arguments,
        Err(reason) => {
            let fallback = append_sandbox_escalation_unavailable_note(&output, &reason);
            emit_final_sandbox_escalation_tool_result(ctx, &fallback);
            return (tool_use_id, fallback, true);
        }
    };
    let retry_ctx = ToolDispatchContext {
        app: ctx.app,
        tool_use_id: ctx.tool_use_id,
        tool_name: ctx.tool_name,
        arguments: &retry_arguments,
        message_id: ctx.message_id,
        session_id: ctx.session_id,
        tool_results_dir: ctx.tool_results_dir,
        project_root: ctx.project_root,
        session_todos: ctx.session_todos.clone(),
        session_agent_tasks: ctx.session_agent_tasks.clone(),
        subagent_depth: ctx.subagent_depth,
        skill_task_context: ctx.skill_task_context,
        web_search_api_keys: ctx.web_search_api_keys.clone(),
        skill_cache: ctx.skill_cache.clone(),
        cancel_flag: ctx.cancel_flag.clone(),
        round_cancel: ctx.round_cancel.clone(),
        execution_environment: ctx.execution_environment.clone(),
        ssh_server: ctx.ssh_server.clone(),
        sandbox_backend: ctx.sandbox_backend.clone(),
        local_venv_type: ctx.local_venv_type.clone(),
        local_venv_name: ctx.local_venv_name.clone(),
        env_store: ctx.env_store.clone(),
        computer_use_enabled: ctx.computer_use_enabled,
        browser_use_enabled: ctx.browser_use_enabled,
        agent_runtime: ctx.agent_runtime,
        hook_engine: ctx.hook_engine,
    };
    let (retry_tool_use_id, retry_output, retry_is_error) =
        super::execute_domain_tool(&retry_ctx).await;
    (
        retry_tool_use_id,
        prepend_sandbox_escalation_approved_note(&retry_output),
        retry_is_error,
    )
}

/// The permission waiter already emitted a pending "需要权限确认" ToolResult;
/// terminal escalation outcomes must overwrite it on the live stream, matching
/// the normal permission-deny flow in dispatch.rs.
fn emit_final_sandbox_escalation_tool_result(ctx: &ToolDispatchContext<'_>, output: &str) {
    let _ = ctx.app.emit(
        &format!("chat-stream-{}", ctx.message_id),
        &crate::infrastructure::streaming::StreamOutputItem::ToolResult {
            tool_use_id: ctx.tool_use_id.to_string(),
            name: ctx.tool_name.to_string(),
            input: ctx.arguments.to_string(),
            output: output.to_string(),
            is_error: true,
        },
    );
}

fn is_sandbox_denied_result(is_error: bool, output: &str) -> bool {
    is_error && strip_tool_failure_wrappers(output).starts_with(SANDBOX_DENIED_PREFIX)
}

fn strip_tool_failure_wrappers(mut output: &str) -> &str {
    for _ in 0..MAX_TOOL_FAILURE_WRAPPER_STRIP_DEPTH {
        let Some(stripped) = output.strip_prefix(TOOL_FAILURE_WRAPPER_PREFIX) else {
            break;
        };
        output = stripped;
    }
    output
}

async fn sandbox_escalation_retry_cancelled(
    cancel_flag: &Option<std::sync::Arc<tokio::sync::RwLock<bool>>>,
    round_cancel: &Option<tokio_util::sync::CancellationToken>,
) -> bool {
    if let Some(cancel_flag) = cancel_flag {
        if *cancel_flag.read().await {
            return true;
        }
    }
    round_cancel
        .as_ref()
        .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
}

fn inject_disable_sandbox_arg(arguments: &str) -> Result<String, String> {
    let mut value = serde_json::from_str::<Value>(arguments)
        .map_err(|e| format!("无法解析 bash 参数，未能注入无沙箱重跑标记: {e}"))?;
    let Some(object) = value.as_object_mut() else {
        return Err("bash 参数不是 JSON object，未能注入无沙箱重跑标记".to_string());
    };
    object.insert("dangerously_disable_sandbox".to_string(), Value::Bool(true));
    serde_json::to_string(&value)
        .map_err(|e| format!("无法序列化无沙箱重跑参数，返回原始沙箱错误: {e}"))
}

fn sandbox_escalation_preconditions_met(
    tool_name: &str,
    args: &Value,
    feature_enabled: bool,
    already_attempted: bool,
) -> bool {
    feature_enabled
        && !already_attempted
        && is_sandbox_escalatable_tool(tool_name)
        && args.get("run_in_background").and_then(Value::as_bool) != Some(true)
        && args
            .get("dangerously_disable_sandbox")
            .and_then(Value::as_bool)
            != Some(true)
}

fn is_sandbox_escalatable_tool(tool_name: &str) -> bool {
    matches!(tool_name.to_ascii_lowercase().as_str(), "bash" | "exec")
}

fn sandbox_escalation_enabled_from_env() -> bool {
    sandbox_escalation_enabled_from_env_value(
        std::env::var("OMIGA_SANDBOX_ESCALATION").ok().as_deref(),
    )
}

fn sandbox_escalation_enabled_from_env_value(value: Option<&str>) -> bool {
    !matches!(value.map(str::trim), Some(raw) if raw.eq_ignore_ascii_case("off"))
}

fn claim_sandbox_escalation_attempt(tool_use_id: &str) -> bool {
    let mut attempts = sandbox_escalation_attempts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Leak guard: tool_use_ids are unique UUIDs that never recur, so dropping
    // old entries only bounds memory — it cannot re-enable a past escalation.
    if attempts.len() >= SANDBOX_ESCALATION_ATTEMPTS_CAP {
        attempts.clear();
    }
    attempts.insert(tool_use_id.to_string())
}

const SANDBOX_ESCALATION_ATTEMPTS_CAP: usize = 1024;

fn sandbox_escalation_attempts() -> &'static Mutex<HashSet<String>> {
    static ATTEMPTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ATTEMPTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn build_sandbox_escalation_permission_request(
    ctx: &ToolDispatchContext<'_>,
    args_value: &Value,
    output: &str,
) -> PermissionRequest {
    let command = command_text(args_value);
    let stderr_summary = sandbox_denial_summary(output);
    let description = format!(
        "命令被本地沙箱拒绝。批准后将无沙箱重跑一次。\n命令: {command}\n沙箱拒绝摘要: {stderr_summary}"
    );

    PermissionRequest {
        request_id: format!("sandbox-escalation-{}", uuid::Uuid::new_v4()),
        context: PermissionContext {
            tool_name: SANDBOX_ESCALATION_CONTEXT.to_string(),
            arguments: args_value.clone(),
            session_id: ctx.session_id.to_string(),
            file_paths: None,
            timestamp: chrono::Utc::now(),
            project_root: Some(ctx.project_root.to_path_buf()),
        },
        risk: RiskAssessment {
            level: RiskLevel::High,
            categories: vec![RiskCategory::System, RiskCategory::FileSystem],
            description,
            recommendations: vec![
                "仅在你信任该命令且理解其文件系统影响时批准。".to_string(),
                "本次批准只会触发一次无沙箱重跑。".to_string(),
            ],
            detected_risks: vec![DetectedRisk {
                category: RiskCategory::System,
                severity: RiskLevel::High,
                description: "无沙箱重跑会绕过本地 seatbelt 限制。".to_string(),
                mitigation: Some("确认命令内容和目标路径后再批准。".to_string()),
            }],
        },
        suggested_mode: PermissionMode::AskEveryTime,
    }
}

fn command_text(args_value: &Value) -> String {
    args_value
        .get("command")
        .or_else(|| args_value.get("cmd"))
        .and_then(Value::as_str)
        .map(|s| truncate_for_permission_summary(s.trim(), 400))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(未提供命令文本)".to_string())
}

fn sandbox_denial_summary(output: &str) -> String {
    let unwrapped = strip_tool_failure_wrappers(output);
    let summary = unwrapped
        .strip_prefix(SANDBOX_DENIED_PREFIX)
        .unwrap_or(unwrapped)
        .trim();
    if summary.is_empty() {
        return "(无 stderr 摘要)".to_string();
    }
    truncate_for_permission_summary(summary, 600)
}

fn truncate_for_permission_summary(text: &str, max_chars: usize) -> String {
    let mut truncated: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn prepend_sandbox_escalation_approved_note(output: &str) -> String {
    if output.is_empty() {
        SANDBOX_ESCALATION_APPROVED_NOTE.to_string()
    } else {
        format!("{SANDBOX_ESCALATION_APPROVED_NOTE}\n{output}")
    }
}

fn append_sandbox_escalation_rejected_note_with_reason(original: &str, reason: &str) -> String {
    let original = strip_tool_failure_wrappers(original);
    let reason = reason.trim();
    if reason.is_empty() {
        format!("{original}\n\n{SANDBOX_ESCALATION_REJECTED_NOTE}")
    } else {
        format!("{original}\n\n{SANDBOX_ESCALATION_REJECTED_NOTE} 原因: {reason}")
    }
}

fn append_sandbox_escalation_unavailable_note(original: &str, reason: &str) -> String {
    let original = strip_tool_failure_wrappers(original);
    format!("{original}\n\n未能发起无沙箱重跑审批，返回原始沙箱拒绝错误。原因: {reason}")
}

fn append_sandbox_escalation_cancelled_after_approval_note(original: &str) -> String {
    let original = strip_tool_failure_wrappers(original);
    format!("{original}\n\n{SANDBOX_ESCALATION_CANCELLED_AFTER_APPROVAL_NOTE}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_only_sandbox_denied_error_prefix() {
        assert!(is_sandbox_denied_result(
            true,
            "Tool execution failed: Tool execution failed: SANDBOX_DENIED: sandbox-exec denied file-write"
        ));
        assert!(is_sandbox_denied_result(
            true,
            "Tool execution failed: SANDBOX_DENIED: sandbox-exec denied file-write"
        ));
        assert!(is_sandbox_denied_result(
            true,
            "SANDBOX_DENIED: sandbox-exec denied file-write"
        ));
        assert!(!is_sandbox_denied_result(
            false,
            "Tool execution failed: SANDBOX_DENIED: successful output text"
        ));
        assert!(!is_sandbox_denied_result(
            true,
            "Command failed without the sandbox marker"
        ));
        assert!(!is_sandbox_denied_result(
            true,
            "cat 输出: SANDBOX_DENIED: sandbox-exec denied file-write"
        ));
    }

    #[test]
    fn strips_up_to_three_tool_failure_wrappers() {
        assert_eq!(
            strip_tool_failure_wrappers("SANDBOX_DENIED: deny file-write /private"),
            "SANDBOX_DENIED: deny file-write /private"
        );
        assert_eq!(
            strip_tool_failure_wrappers(
                "Tool execution failed: SANDBOX_DENIED: deny file-write /private"
            ),
            "SANDBOX_DENIED: deny file-write /private"
        );
        assert_eq!(
            strip_tool_failure_wrappers(
                "Tool execution failed: Tool execution failed: SANDBOX_DENIED: deny file-write /private"
            ),
            "SANDBOX_DENIED: deny file-write /private"
        );
        assert_eq!(
            strip_tool_failure_wrappers(
                "Tool execution failed: Tool execution failed: Tool execution failed: SANDBOX_DENIED: deny file-write /private"
            ),
            "SANDBOX_DENIED: deny file-write /private"
        );
    }

    #[test]
    fn injects_disable_sandbox_arg_and_preserves_existing_fields() {
        let injected = inject_disable_sandbox_arg(
            r#"{"command":"touch /tmp/out","cwd":"/tmp","dangerously_disable_sandbox":false}"#,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&injected).unwrap();

        assert_eq!(value["command"], "touch /tmp/out");
        assert_eq!(value["cwd"], "/tmp");
        assert_eq!(value["dangerously_disable_sandbox"], true);
    }

    #[test]
    fn sandbox_escalation_preconditions_gate_unsafe_or_disabled_cases() {
        let base_args = json!({ "command": "touch /tmp/out" });

        assert!(sandbox_escalation_preconditions_met(
            "bash", &base_args, true, false
        ));
        assert!(sandbox_escalation_preconditions_met(
            "exec", &base_args, true, false
        ));
        assert!(!sandbox_escalation_preconditions_met(
            "exec_session_write",
            &base_args,
            true,
            false
        ));
        assert!(!sandbox_escalation_preconditions_met(
            "bash",
            &json!({ "command": "x", "run_in_background": true }),
            true,
            false
        ));
        assert!(!sandbox_escalation_preconditions_met(
            "bash",
            &json!({ "command": "x", "dangerously_disable_sandbox": true }),
            true,
            false
        ));
        assert!(!sandbox_escalation_preconditions_met(
            "bash", &base_args, false, false
        ));
        assert!(!sandbox_escalation_preconditions_met(
            "bash", &base_args, true, true
        ));
    }

    #[test]
    fn sandbox_escalation_env_gate_uses_pure_value_parser() {
        assert!(sandbox_escalation_enabled_from_env_value(None));
        assert!(sandbox_escalation_enabled_from_env_value(Some("")));
        assert!(sandbox_escalation_enabled_from_env_value(Some("on")));
        assert!(!sandbox_escalation_enabled_from_env_value(Some("off")));
        assert!(!sandbox_escalation_enabled_from_env_value(Some(" OFF ")));
    }

    #[test]
    fn refusal_output_keeps_original_error_and_adds_model_guidance() {
        let original =
            "Tool execution failed: Tool execution failed: SANDBOX_DENIED: deny file-write /private";
        let output = append_sandbox_escalation_rejected_note_with_reason(original, "");

        assert!(output.starts_with("SANDBOX_DENIED: deny file-write /private"));
        assert!(!output.starts_with("Tool execution failed:"));
        assert!(output.contains("用户拒绝无沙箱重跑"));
    }

    #[test]
    fn sandbox_denial_summary_uses_unwrapped_sandbox_message() {
        assert_eq!(
            sandbox_denial_summary(
                "Tool execution failed: Tool execution failed: SANDBOX_DENIED: deny file-write /private"
            ),
            "deny file-write /private"
        );
    }

    #[tokio::test]
    async fn sandbox_escalation_retry_cancelled_checks_cancel_flag() {
        let cancel_flag = Some(std::sync::Arc::new(tokio::sync::RwLock::new(true)));
        let round_cancel = None;

        assert!(sandbox_escalation_retry_cancelled(&cancel_flag, &round_cancel).await);
    }

    #[tokio::test]
    async fn sandbox_escalation_retry_cancelled_checks_round_cancel() {
        let cancel_flag = Some(std::sync::Arc::new(tokio::sync::RwLock::new(false)));
        let round_cancel = Some(tokio_util::sync::CancellationToken::new());
        round_cancel.as_ref().unwrap().cancel();

        assert!(sandbox_escalation_retry_cancelled(&cancel_flag, &round_cancel).await);
    }

    #[tokio::test]
    async fn sandbox_escalation_retry_cancelled_allows_active_session() {
        let cancel_flag = Some(std::sync::Arc::new(tokio::sync::RwLock::new(false)));
        let round_cancel = Some(tokio_util::sync::CancellationToken::new());

        assert!(!sandbox_escalation_retry_cancelled(&cancel_flag, &round_cancel).await);
    }

    #[test]
    fn approval_race_output_keeps_unwrapped_error_and_adds_model_guidance() {
        let original =
            "Tool execution failed: Tool execution failed: SANDBOX_DENIED: deny file-write /private";
        let output = append_sandbox_escalation_cancelled_after_approval_note(original);

        assert!(output.starts_with("SANDBOX_DENIED: deny file-write /private"));
        assert!(output.contains("审批完成前会话已取消,未执行无沙箱重跑"));
    }
}
