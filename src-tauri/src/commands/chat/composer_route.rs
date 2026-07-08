use super::*;

/// Resolve session `project_path` to an absolute-ish root for tools (glob, bash, file_read).
pub(super) fn resolve_session_project_root(project_path: &str) -> std::path::PathBuf {
    let p = project_path.trim();
    if p.is_empty() || p == "." {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(p)
    }
}

/// Normalize composer `sandboxBackend` from the UI (`docker` | `singularity`).
/// Note: `ssh` is no longer a sandbox backend - it's now a separate execution environment.
pub(super) fn normalize_sandbox_backend(raw: Option<&String>) -> String {
    let Some(r) = raw else {
        return "docker".to_string();
    };
    let s = r.trim().to_lowercase();
    if s.is_empty() {
        return "docker".to_string();
    }
    match s.as_str() {
        "docker" | "singularity" => s,
        "auto" => "docker".to_string(),
        // Cloud backends are not user-facing until their real runtime is implemented.
        "modal" | "daytona" => "docker".to_string(),
        // Legacy: ssh was moved to be an execution environment, not a sandbox backend
        "ssh" => "docker".to_string(),
        _ => "docker".to_string(),
    }
}

/// Normalize composer `executionEnvironment` from the UI (`local` | `ssh` | `sandbox`).
///
/// - `local`: Run tools and terminal on the local machine
/// - `ssh`: Run tools and terminal on a remote SSH server
/// - `sandbox`: Run tools and terminal in a remote sandbox (Modal, Daytona, Docker, Singularity)
pub(super) fn normalize_execution_environment(raw: Option<&String>) -> String {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("ssh") => "ssh".to_string(),
        Some("sandbox") | Some("remote") => "sandbox".to_string(),
        _ => "local".to_string(),
    }
}

pub(super) fn composer_execution_addendum(
    env: &str,
    ssh_server: Option<&str>,
    venv_type: &str,
    venv_name: &str,
) -> Option<String> {
    // 虚拟环境说明行（非空时追加）
    let venv_line = {
        let name = venv_name.trim();
        if !name.is_empty() && venv_type != "none" && !venv_type.is_empty() {
            let kind_label = match venv_type {
                "conda" => "conda env",
                "venv" => "venv",
                "pyenv" => "pyenv",
                other => other,
            };
            format!(
                "\nActive Python environment: **{kind_label} `{name}`** — \
                 all `bash` tool commands are automatically wrapped with the activation \
                 preamble before execution. \
                 **Do NOT** write `conda activate`, `source activate`, or \
                 `pyenv shell` manually in bash commands — it is already done for you. \
                 Use `python` / `pip` / `jupyter` directly. \
                 When creating `.ipynb` notebooks, set kernelspec `name` to `{name}` \
                 so the notebook uses this environment's kernel.",
            )
        } else {
            String::new()
        }
    };

    match env {
        "ssh" => {
            let server_info = ssh_server.map(|s| format!(" (server: `{}`)", s)).unwrap_or_default();
            Some(format!(
                "### Composer execution environment\nThe user chose **SSH**{server_info} for this session turn: assume tools and shell should run on the configured SSH server when available; local-only tools may error until remote is fully wired.{venv_line}",
            ))
        }
        "sandbox" => Some(format!(
            "### Composer execution environment\nThe user chose **sandbox** for this session turn: assume tools and shell should run on the configured remote sandbox when available; local-only tools may error until remote is fully wired.{venv_line}",
        )),
        _ => Some(format!(
            "### Composer execution environment\nThe user chose **local**: run terminal commands and workspace tools on this machine.{venv_line}",
        )),
    }
}

/// 格式化调度计划为 system prompt 的一部分
pub(super) fn format_scheduler_plan(
    result: &crate::domain::agents::scheduler::SchedulingResult,
) -> String {
    let is_content_generation = result
        .plan
        .subtasks
        .iter()
        .any(|t| t.id == "generate-content" || t.id == "gather-requirements");

    let mut plan_text = if is_content_generation {
        String::from("## 内容生成任务执行计划\n\n")
    } else {
        String::from("## 任务执行计划\n\n")
    };

    plan_text.push_str(&format!(
        "此任务已自动分解为 **{}** 个子任务，将按以下顺序执行：\n\n",
        result.plan.subtasks.len()
    ));
    plan_text.push_str(&format!(
        "调度层级：`{}`（主入口） → `{}`（执行监督） → 专职子 Agent。\
         项目计划是执行依据；阶段标签仅用于观测，不代表固定流水线。\
         真实派发、重试、取消和状态汇总由后端编排器负责。\n\n",
        result
            .plan
            .entry_agent_type
            .as_deref()
            .unwrap_or("general-purpose"),
        result
            .plan
            .execution_supervisor_agent_type
            .as_deref()
            .unwrap_or("executor")
    ));

    // 获取并行执行组
    let groups = result.plan.get_parallel_groups();
    let mut task_idx = 1;

    for (group_idx, group) in groups.iter().enumerate() {
        if groups.len() > 1 {
            plan_text.push_str(&format!("### 阶段 {}\n", group_idx + 1));
        }

        for task_id in group {
            if let Some(task) = result.plan.subtasks.iter().find(|t| &t.id == task_id) {
                plan_text.push_str(&format!(
                    "{}. **{}** - 使用 `{}` Agent\n",
                    task_idx, task.description, task.agent_type
                ));
                if let Some(stage) = task.stage.as_ref().and_then(|s| {
                    serde_json::to_value(s)
                        .ok()
                        .and_then(|value| value.as_str().map(ToString::to_string))
                }) {
                    plan_text.push_str(&format!("   - 阶段: `{}`\n", stage));
                }
                if let Some(supervisor) = task.supervisor_agent_type.as_deref() {
                    plan_text.push_str(&format!("   - 上级: `{}`\n", supervisor));
                }
                if !task.context.is_empty() {
                    plan_text.push_str(&format!("   - 要求: {}\n", task.context));
                }
                if !task.dependencies.is_empty() {
                    plan_text.push_str(&format!("   - 依赖: {}\n", task.dependencies.join(", ")));
                }
                if task.critical {
                    plan_text.push_str("   - ⚠️ 关键任务\n");
                }
                task_idx += 1;
            }
        }
        plan_text.push('\n');
    }

    plan_text.push_str(&format!(
        "\n预估执行时间: ~{} 分钟\n",
        result.estimated_duration_secs / 60
    ));
    if !result.reviewer_agents.is_empty() {
        plan_text.push_str(&format!(
            "Reviewer 结构化结论将由: {}\n",
            result.reviewer_agents.join(", ")
        ));
    }

    // 对于内容生成任务，添加重要提示
    if is_content_generation {
        plan_text.push_str("\n### ⚠️ 重要提示\n");
        plan_text.push_str("这是一个**内容生成任务**。你必须：\n");
        plan_text.push_str("1. **生成完整、详细的内容**，不要只是概述或框架\n");
        plan_text.push_str("2. **包含具体的细节**：名称、地址、时间、价格、建议等\n");
        plan_text.push_str("3. **确保内容实用可读**，用户可以直接使用\n");
        plan_text.push_str(
            "4. 如果这是默认 General 路径，请先向用户展示计划，等待计划卡片按钮确认后再执行\n",
        );
    } else {
        plan_text.push_str(
            "\n请向用户展示该计划并说明可通过计划卡片按钮执行；不要在默认 General 路径中自行执行子任务。",
        );
    }

    plan_text
}

pub(super) fn looks_like_resume_request(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "resume",
        "continue",
        "继续",
        "恢复",
        "从上次继续",
        "继续上次",
        "pick up where",
    ]
    .iter()
    .any(|token| lower.contains(token))
}

pub(super) struct ChatOrchestrationEvent<'a> {
    pub(super) session_id: &'a str,
    pub(super) round_id: Option<&'a str>,
    pub(super) message_id: Option<&'a str>,
    pub(super) mode: Option<&'a str>,
    pub(super) event_type: &'a str,
    pub(super) phase: Option<&'a str>,
    pub(super) task_id: Option<&'a str>,
    pub(super) payload: serde_json::Value,
}

pub(super) async fn append_orchestration_event(
    repo: &crate::domain::persistence::SessionRepository,
    event: ChatOrchestrationEvent<'_>,
) {
    let payload_json = serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());
    if let Err(e) = repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id: event.session_id,
            round_id: event.round_id,
            message_id: event.message_id,
            mode: event.mode,
            event_type: event.event_type,
            phase: event.phase,
            task_id: event.task_id,
            payload_json: &payload_json,
        })
        .await
    {
        tracing::warn!(target: "omiga::orchestration_events", session_id = event.session_id, event_type = event.event_type, error = %e, "append_orchestration_event failed");
    }
}

pub(super) async fn append_preflight_stage_event(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    message_id: &str,
    mode: Option<&str>,
    stage: &str,
    duration_ms: u128,
    payload: serde_json::Value,
) {
    append_orchestration_event(
        repo,
        ChatOrchestrationEvent {
            session_id,
            round_id: None,
            message_id: Some(message_id),
            mode,
            event_type: "preflight_stage_completed",
            phase: Some("preflight"),
            task_id: None,
            payload: serde_json::json!({
                "stage": stage,
                "durationMs": duration_ms,
                "payload": payload,
            }),
        },
    )
    .await;
}

pub(super) async fn append_preflight_stage_failed_event(
    repo: &crate::domain::persistence::SessionRepository,
    session_id: &str,
    message_id: &str,
    mode: Option<&str>,
    stage: &str,
    duration_ms: u128,
    error: &str,
) {
    append_orchestration_event(
        repo,
        ChatOrchestrationEvent {
            session_id,
            round_id: None,
            message_id: Some(message_id),
            mode,
            event_type: "preflight_stage_failed",
            phase: Some("preflight"),
            task_id: None,
            payload: serde_json::json!({
                "stage": stage,
                "durationMs": duration_ms,
                "error": error,
            }),
        },
    )
    .await;
}
