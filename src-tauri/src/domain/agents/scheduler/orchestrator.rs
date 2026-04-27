//! Agent 编排器
//!
//! 协调多个 Agent 的执行，管理依赖关系和结果传递。
//!
//! 主要入口：
//! - `execute_with_runtime`：真实模式，通过 `spawn_background_agent` 驱动 LLM 子 Agent

use super::{SchedulingRequest, SchedulingStrategy, SubTask, TaskPlan};

#[inline]
fn unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{Emitter, Manager};

use crate::app_state::OmigaAppState;
use crate::domain::agents::reviewer_verdict::parse_reviewer_verdict;
use crate::domain::persistence::NewOrchestrationEventRecord;
use crate::domain::tools::WebSearchApiKeys;

struct RuntimeOrchestrationEvent<'a> {
    session_id: &'a str,
    round_id: Option<&'a str>,
    mode: Option<&'a str>,
    event_type: &'a str,
    phase: Option<&'a str>,
    task_id: Option<&'a str>,
    payload: serde_json::Value,
}

async fn append_orchestration_runtime_event(
    repo: &crate::domain::persistence::SessionRepository,
    event: RuntimeOrchestrationEvent<'_>,
) {
    let payload_json = serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());
    if let Err(e) = repo
        .append_orchestration_event(NewOrchestrationEventRecord {
            session_id: event.session_id,
            round_id: event.round_id,
            message_id: None,
            mode: event.mode,
            event_type: event.event_type,
            phase: event.phase,
            task_id: event.task_id,
            payload_json: &payload_json,
        })
        .await
    {
        tracing::warn!(target: "omiga::orchestration_events", session_id = event.session_id, event_type = event.event_type, error = %e, "append_orchestration_runtime_event failed");
    }
}

/// 编排结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationResult {
    /// 计划 ID
    pub plan_id: String,
    /// 整体状态
    pub status: ExecutionStatus,
    /// 子任务结果
    pub subtask_results: HashMap<String, SubTaskResult>,
    /// 执行日志
    pub execution_log: Vec<ExecutionLogEntry>,
    /// 开始时间
    pub started_at: Option<u64>,
    /// 完成时间
    pub completed_at: Option<u64>,
    /// 最终结果摘要
    pub final_summary: String,
}

/// 执行状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    Pending,
    Running,
    /// 全部子任务成功
    Completed,
    /// 全部子任务失败
    Failed,
    /// 部分子任务成功、部分失败（非关键任务失败时）
    PartiallyCompleted,
    Cancelled,
}

/// 子任务结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskResult {
    pub subtask_id: String,
    pub agent_type: Option<String>,
    pub status: ExecutionStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
}

/// 执行日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLogEntry {
    pub timestamp: u64,
    pub subtask_id: Option<String>,
    pub message: String,
    pub level: LogLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

fn is_reviewer_agent(agent_type: &str) -> bool {
    matches!(
        agent_type,
        "verification"
            | "code-reviewer"
            | "security-reviewer"
            | "performance-reviewer"
            | "quality-reviewer"
            | "api-reviewer"
            | "critic"
            | "test-engineer"
    )
}

fn subtask_stage_label(subtask: &SubTask) -> Option<String> {
    subtask
        .stage
        .as_ref()
        .and_then(|stage| serde_json::to_value(stage).ok())
        .and_then(|value| value.as_str().map(ToString::to_string))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewerBlockingLevel {
    None,
    Soft,
    Hard,
}

fn reviewer_blocking_level(results: &HashMap<String, SubTaskResult>) -> ReviewerBlockingLevel {
    use crate::domain::agents::reviewer_verdict::{
        parse_reviewer_verdict, ReviewerSeverity, ReviewerVerdictKind,
    };

    let mut level = ReviewerBlockingLevel::None;

    for result in results.values() {
        let Some(agent_type) = result.agent_type.as_deref() else {
            continue;
        };
        if !is_reviewer_agent(agent_type) {
            continue;
        }
        let text = result
            .output
            .as_deref()
            .or(result.error.as_deref())
            .unwrap_or("");
        let verdict = parse_reviewer_verdict(agent_type, text);

        let hard = matches!(
            verdict.verdict,
            ReviewerVerdictKind::Fail | ReviewerVerdictKind::Reject
        ) || verdict.severity == ReviewerSeverity::Critical
            || ((agent_type == "security-reviewer" || agent_type == "critic")
                && verdict.severity == ReviewerSeverity::High);
        if hard {
            return ReviewerBlockingLevel::Hard;
        }

        let soft = matches!(verdict.verdict, ReviewerVerdictKind::Partial)
            || matches!(
                verdict.severity,
                ReviewerSeverity::High | ReviewerSeverity::Medium
            );
        if soft {
            level = ReviewerBlockingLevel::Soft;
        }
    }

    level
}

/// Agent 编排器
pub struct AgentOrchestrator {
    // 执行状态跟踪
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self {}
    }

    /// 生成动态执行计划（基于 LLM 分析）
    pub async fn generate_dynamic_plan(
        &self,
        request: &str,
        _project_root: &str,
    ) -> Result<TaskPlan, String> {
        // 这里可以调用 LLM 来分析任务并生成计划
        // 目前使用基于规则的计划
        let planner = super::planner::TaskPlanner::new();
        let scheduling_request = SchedulingRequest::new(request);
        planner.decompose(&scheduling_request).await
    }

    /// 添加日志条目
    fn log(
        &self,
        result: &mut OrchestrationResult,
        subtask_id: Option<&str>,
        message: &str,
        level: LogLevel,
    ) {
        result.execution_log.push(ExecutionLogEntry {
            timestamp: unix_timestamp_secs(),
            subtask_id: subtask_id.map(|s| s.to_string()),
            message: message.to_string(),
            level,
        });
    }

    /// Shared finalization: compute status, set completed_at, write final_summary.
    fn finalize_result(&self, result: &mut OrchestrationResult, total_subtasks: usize) {
        let completed = result
            .subtask_results
            .values()
            .filter(|r| r.status == ExecutionStatus::Completed)
            .count();
        let failed = result
            .subtask_results
            .values()
            .filter(|r| r.status == ExecutionStatus::Failed)
            .count();

        result.status = if failed == 0 {
            ExecutionStatus::Completed
        } else if completed == 0 {
            ExecutionStatus::Failed
        } else {
            ExecutionStatus::PartiallyCompleted
        };

        let reviewer_level = reviewer_blocking_level(&result.subtask_results);
        match (result.status.clone(), reviewer_level) {
            (ExecutionStatus::Completed, ReviewerBlockingLevel::Soft) => {
                result.status = ExecutionStatus::PartiallyCompleted;
            }
            (ExecutionStatus::Completed, ReviewerBlockingLevel::Hard)
            | (ExecutionStatus::PartiallyCompleted, ReviewerBlockingLevel::Hard) => {
                result.status = ExecutionStatus::Failed;
            }
            _ => {}
        }
        result.completed_at = Some(unix_timestamp_secs());

        let summary = format!(
            "计划执行完成: {}/{} 成功, {} 失败",
            completed, total_subtasks, failed
        );
        result.final_summary = summary.clone();
        let reviewer_count = result
            .subtask_results
            .values()
            .filter(|r| {
                r.agent_type
                    .as_deref()
                    .map(is_reviewer_agent)
                    .unwrap_or(false)
            })
            .count();
        if reviewer_count > 0 {
            result.final_summary.push_str(&format!(
                "\nReviewer 子任务数: {}（已纳入最终综合）",
                reviewer_count
            ));
            match reviewer_level {
                ReviewerBlockingLevel::Soft => {
                    result.final_summary.push_str(
                        "\nReviewer 阻断级别: soft（存在需要在最终结论中保留的风险/部分通过项）",
                    );
                }
                ReviewerBlockingLevel::Hard => {
                    result.final_summary.push_str(
                        "\nReviewer 阻断级别: hard（存在明确 FAIL/REJECT/CRITICAL 结论）",
                    );
                }
                ReviewerBlockingLevel::None => {}
            }
        }
        self.log(result, None, &summary, LogLevel::Info);
    }
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// 核心执行路径：使用 `spawn_background_agent` 驱动并行子 Agent。
///
/// 需要调用方提供已构建好的 `AgentLlmRuntime`（携带 LLM config 和执行环境）。
/// 调用方通过 `from_app()` 构建 runtime 后传入，`cancel` token 贯穿整个执行生命周期。
impl AgentOrchestrator {
    pub(crate) async fn execute_with_runtime(
        &self,
        plan: &TaskPlan,
        request: &SchedulingRequest,
        app: &tauri::AppHandle,
        runtime: &crate::commands::chat::AgentLlmRuntime,
        session_id: &str,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<OrchestrationResult, String> {
        use crate::domain::agents::background::get_background_agent_manager;
        use crate::domain::blackboard::{self as bb, Blackboard};
        use crate::domain::team_state::{self, TeamPhase, TeamState, TeamSubtaskState};

        let plan = plan.clone().with_execution_defaults();
        let project_root = std::path::Path::new(&request.project_root);
        let plan_id = plan.plan_id.clone();
        let orchestration_mode = request
            .mode_hint
            .as_deref()
            .filter(|mode| !mode.trim().is_empty())
            .unwrap_or(if request.strategy == SchedulingStrategy::Team {
                "team"
            } else {
                "schedule"
            });
        let repo = &**runtime.repo();

        // ── Team crash recovery: initialise & persist state ──────────────────
        let mut team_st = TeamState::new(
            session_id.to_string(),
            plan.original_request.clone(),
            request.project_root.clone(),
        );
        team_st.subtasks = plan
            .subtasks
            .iter()
            .map(|t| TeamSubtaskState {
                id: t.id.clone(),
                description: t.description.clone(),
                agent_type: t.agent_type.clone(),
                status: "pending".to_string(),
                attempt: 0,
                max_retries: t.max_retries,
                error: None,
                bg_task_id: None,
            })
            .collect();
        let is_team = request.strategy == SchedulingStrategy::Team;

        // Team 模式：Planning → Executing；其他模式直接进入 Executing
        team_st.phase = TeamPhase::Executing;
        let _ = team_state::write_state(project_root, &team_st).await;
        append_orchestration_runtime_event(
            repo,
            RuntimeOrchestrationEvent {
                session_id,
                round_id: Some(runtime.round_id()),
                mode: Some(orchestration_mode),
                event_type: "phase_changed",
                phase: Some("executing"),
                task_id: None,
                payload: serde_json::json!({ "planId": plan_id }),
            },
        )
        .await;
        append_orchestration_runtime_event(
            repo,
            RuntimeOrchestrationEvent {
                session_id,
                round_id: Some(runtime.round_id()),
                mode: Some(orchestration_mode),
                event_type: "executor_started",
                phase: Some("executing"),
                task_id: None,
                payload: serde_json::json!({
                    "planId": plan_id,
                    "entryAgentType": plan.entry_agent_type.clone(),
                    "executionSupervisorAgentType": plan.execution_supervisor_agent_type.clone(),
                    "taskCount": plan.subtasks.len(),
                }),
            },
        )
        .await;

        // ── Gap 4: Shared blackboard — resume from prior run if it exists ────
        let mut blackboard: Blackboard = bb::read_board(project_root, session_id)
            .await
            .unwrap_or_else(|| Blackboard::new(session_id.to_string()));
        let mut executor_debug_rounds = 0u32;
        const MAX_EXECUTOR_DEBUG_ROUNDS: u32 = 3;

        // Helper: update subtask status in the persisted state
        macro_rules! persist_subtask {
            ($id:expr, $status:expr, $attempt:expr, $bg:expr, $err:expr) => {{
                if let Some(s) = team_st.subtasks.iter_mut().find(|s| &s.id == $id) {
                    s.status = $status.to_string();
                    s.attempt = $attempt;
                    if let Some(bg) = $bg {
                        s.bg_task_id = Some(bg);
                    }
                    s.error = $err;
                }
                team_st.touch();
                let _ = team_state::write_state(project_root, &team_st).await;
            }};
        }

        let mut result = OrchestrationResult {
            plan_id: plan.plan_id.clone(),
            status: ExecutionStatus::Running,
            subtask_results: HashMap::new(),
            execution_log: Vec::new(),
            started_at: Some(unix_timestamp_secs()),
            completed_at: None,
            final_summary: String::new(),
        };

        self.log(
            &mut result,
            None,
            "开始执行计划（真实 Agent 模式）",
            LogLevel::Info,
        );

        let tool_results_dir = crate::commands::chat::tool_results_dir_for_session(app, session_id);

        // 获取 skill_cache（从 Tauri 进程状态）
        let skill_cache = app
            .try_state::<OmigaAppState>()
            .map(|s| s.skill_cache.clone())
            .unwrap_or_else(|| {
                std::sync::Arc::new(std::sync::Mutex::new(
                    crate::domain::skills::SkillCacheMap::default(),
                ))
            });

        let web_search_keys = if let Some(st) = app.try_state::<OmigaAppState>() {
            st.chat.web_search_api_keys.lock().await.clone()
        } else {
            WebSearchApiKeys::default()
        };

        let groups = plan.get_parallel_groups();

        for (group_idx, group) in groups.iter().enumerate() {
            // Check for cancellation before starting each group.
            if cancel.is_cancelled() {
                self.log(&mut result, None, "编排已被取消", LogLevel::Warning);
                result.status = ExecutionStatus::Cancelled;
                result.completed_at = Some(unix_timestamp_secs());
                result.final_summary = "编排已被用户取消".to_string();
                return Ok(result);
            }

            self.log(
                &mut result,
                None,
                &format!("执行组 {}/{}: {:?}", group_idx + 1, groups.len(), group),
                LogLevel::Info,
            );

            // Team 模式：当当前组全是 verification agent 时进入 Verifying 阶段
            if is_team {
                let is_verify_group = group.iter().all(|task_id| {
                    plan.subtasks
                        .iter()
                        .find(|t| &t.id == task_id)
                        .map(|t| t.agent_type == "verification")
                        .unwrap_or(false)
                });
                if is_verify_group {
                    team_st.phase = TeamPhase::Verifying;
                    team_st.touch();
                    let _ = team_state::write_state(project_root, &team_st).await;
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "phase_changed",
                            phase: Some("verifying"),
                            task_id: None,
                            payload: serde_json::json!({ "group": group }),
                        },
                    )
                    .await;
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "verification_started",
                            phase: Some("verifying"),
                            task_id: None,
                            payload: serde_json::json!({ "group": group }),
                        },
                    )
                    .await;
                    self.log(&mut result, None, "[Team] 进入验证阶段", LogLevel::Info);
                }
            }

            // 并行启动组内所有后台 Agent，收集 bg_task_id
            let mut bg_task_ids: Vec<(String, String)> = Vec::new(); // (subtask_id, bg_task_id)

            for task_id_str in group {
                if let Some(subtask) = plan.subtasks.iter().find(|t| &t.id == task_id_str) {
                    let effective_root = std::path::Path::new(&request.project_root);

                    // Build prompt, injecting blackboard entries for all upstream dependencies.
                    // This lets the arbitrate/architect subtask (Competitive) and post-verify
                    // (VerificationFirst) actually see prior outputs rather than just a
                    // placeholder "黑板中包含…" instruction.
                    let dep_context = if subtask.dependencies.is_empty() {
                        String::new()
                    } else {
                        let mut dep_ctx = String::new();
                        for dep_id in &subtask.dependencies {
                            let entries = blackboard.query_by_subtask(dep_id);
                            for e in entries {
                                dep_ctx.push_str(&format!(
                                    "\n\n---\n**[{}] {} 的输出**\n\n{}",
                                    e.subtask_id,
                                    e.agent_type,
                                    e.value.trim()
                                ));
                            }
                        }
                        if dep_ctx.is_empty() {
                            String::new()
                        } else {
                            format!("\n\n## 上游子任务结果（来自共享黑板）{}", dep_ctx)
                        }
                    };

                    let base_prompt = if subtask.context.is_empty() {
                        subtask.description.clone()
                    } else {
                        format!("{}\n\n{}", subtask.description, subtask.context)
                    };
                    let full_prompt = if dep_context.is_empty() {
                        base_prompt
                    } else {
                        format!("{}{}", base_prompt, dep_context)
                    };

                    let agent_args = crate::domain::tools::agent::AgentArgs {
                        description: subtask.description.clone(),
                        prompt: full_prompt,
                        subagent_type: Some(subtask.agent_type.clone()),
                        model: None,
                        run_in_background: None,
                        cwd: None,
                    };

                    let router = crate::domain::agents::get_agent_router();
                    let agent_def = router.select_agent(Some(&subtask.agent_type));
                    let message_id = uuid::Uuid::new_v4().to_string();

                    match crate::commands::chat::spawn_background_agent(
                        crate::commands::chat::BackgroundAgentRequest {
app,
message_id: &message_id,
session_id,
plan_id: Some(plan_id.as_str()),
tool_results_dir: &tool_results_dir,
effective_root,
session_todos: None,
session_agent_tasks: // session_todos
                        None,
args: // session_agent_tasks
                        &agent_args,
runtime,
subagent_execute_depth: 1,
web_search_api_keys: // subagent_execute_depth
                        web_search_keys.clone(),
skill_cache: skill_cache.clone(),
agent_def,
},
                    )
                    .await
                    {
                        Ok(bg_task_id) => {
                            self.log(
                                &mut result,
                                Some(task_id_str),
                                &format!("后台 Agent 已启动 (bg_task_id={})", bg_task_id),
                                LogLevel::Info,
                            );
                            persist_subtask!(
                                task_id_str,
                                "running",
                                0u32,
                                Some(bg_task_id.clone()),
                                None::<String>
                            );
                            // Notify frontend: worker started
                            let _ = app.emit(
                                "team-worker-update",
                                serde_json::json!({
                                    "sessionId": session_id,
                                    "subtaskId": task_id_str,
                                    "agentType": subtask.agent_type,
                                    "status": "running",
                                    "description": subtask.description,
                                }),
                            );
                            append_orchestration_runtime_event(
                                repo,
                                RuntimeOrchestrationEvent {
                                    session_id,
                                    round_id: Some(runtime.round_id()),
                                    mode: Some(orchestration_mode),
                                    event_type: "worker_started",
                                    phase: Some("executing"),
                                    task_id: Some(task_id_str),
                                    payload: serde_json::json!({
                                        "planId": plan_id,
                                        "agentType": subtask.agent_type,
                                        "supervisorAgentType": subtask.supervisor_agent_type,
                                        "stage": subtask_stage_label(subtask),
                                        "description": subtask.description,
                                        "backgroundTaskId": bg_task_id,
                                    }),
                                },
                            )
                            .await;
                            append_orchestration_runtime_event(
                                repo,
                                RuntimeOrchestrationEvent {
                                    session_id,
                                    round_id: Some(runtime.round_id()),
                                    mode: Some(orchestration_mode),
                                    event_type: "executor_child_started",
                                    phase: Some("executing"),
                                    task_id: Some(task_id_str),
                                    payload: serde_json::json!({
                                        "planId": plan_id,
                                        "agentType": subtask.agent_type,
                                        "supervisorAgentType": subtask.supervisor_agent_type,
                                        "stage": subtask_stage_label(subtask),
                                        "description": subtask.description,
                                        "backgroundTaskId": bg_task_id,
                                    }),
                                },
                            )
                            .await;
                            bg_task_ids.push((task_id_str.clone(), bg_task_id));
                        }
                        Err(e) => {
                            self.log(
                                &mut result,
                                Some(task_id_str),
                                &format!("启动后台 Agent 失败: {}", e),
                                LogLevel::Error,
                            );
                            persist_subtask!(
                                task_id_str,
                                "failed",
                                0u32,
                                None::<String>,
                                Some(e.clone())
                            );
                            // Notify frontend of launch failure so UI doesn't stall.
                            let _ = app.emit(
                                "team-worker-update",
                                serde_json::json!({
                                    "sessionId": session_id,
                                    "subtaskId": task_id_str,
                                    "agentType": subtask.agent_type,
                                    "status": "failed",
                                    "error": e,
                                }),
                            );
                            append_orchestration_runtime_event(
                                repo,
                                RuntimeOrchestrationEvent {
                                    session_id,
                                    round_id: Some(runtime.round_id()),
                                    mode: Some(orchestration_mode),
                                    event_type: "worker_launch_failed",
                                    phase: Some("executing"),
                                    task_id: Some(task_id_str),
                                    payload: serde_json::json!({
                                        "planId": plan_id,
                                        "agentType": subtask.agent_type,
                                        "supervisorAgentType": subtask.supervisor_agent_type,
                                        "stage": subtask_stage_label(subtask),
                                        "description": subtask.description,
                                        "error": e,
                                    }),
                                },
                            )
                            .await;
                            append_orchestration_runtime_event(
                                repo,
                                RuntimeOrchestrationEvent {
                                    session_id,
                                    round_id: Some(runtime.round_id()),
                                    mode: Some(orchestration_mode),
                                    event_type: "executor_child_failed",
                                    phase: Some("executing"),
                                    task_id: Some(task_id_str),
                                    payload: serde_json::json!({
                                        "planId": plan_id,
                                        "agentType": subtask.agent_type,
                                        "supervisorAgentType": subtask.supervisor_agent_type,
                                        "stage": subtask_stage_label(subtask),
                                        "description": subtask.description,
                                        "error": e,
                                        "reason": "launch_failed",
                                    }),
                                },
                            )
                            .await;
                            result.subtask_results.insert(
                                task_id_str.clone(),
                                SubTaskResult {
                                    subtask_id: task_id_str.clone(),
                                    agent_type: Some(subtask.agent_type.clone()),
                                    status: ExecutionStatus::Failed,
                                    output: None,
                                    error: Some(e),
                                    started_at: None,
                                    completed_at: None,
                                },
                            );
                        }
                    }
                }
            }

            // ── Gap 5: Streaming aggregation via FuturesUnordered ────────────
            // Each worker result is processed as soon as it arrives instead of
            // waiting for the slowest task in the group (join_all behaviour).
            // Completed results are posted to the shared blackboard immediately,
            // letting the Architect read partial results while other workers run.
            //
            // Retry / per-subtask timeout are preserved from the earlier design.
            let manager = get_background_agent_manager();

            // Build per-subtask metadata lookup: id → (timeout_secs, max_retries, subtask)
            let subtask_meta: std::collections::HashMap<
                String,
                (Option<u64>, u32, crate::domain::agents::scheduler::SubTask),
            > = bg_task_ids
                .iter()
                .filter_map(|(sid, _)| {
                    plan.subtasks
                        .iter()
                        .find(|t| &t.id == sid)
                        .map(|t| (sid.clone(), (t.timeout_secs, t.max_retries, t.clone())))
                })
                .collect();

            // Seed the stream: (subtask_id, bg_task_id, attempt)
            // We use a VecDeque so re-spawned retries can be pushed back cheaply.
            use futures::stream::{FuturesUnordered, StreamExt};
            use std::pin::Pin;
            type PollFut = Pin<
                Box<dyn std::future::Future<Output = (String, u32, (SubTaskResult, bool))> + Send>,
            >;
            let mut stream: FuturesUnordered<PollFut> = FuturesUnordered::new();

            for (sid, bg_id) in &bg_task_ids {
                let timeout = subtask_meta.get(sid).and_then(|(t, _, _)| *t);
                let sid_c = sid.clone();
                let bg_c = bg_id.clone();
                let agent_c = subtask_meta
                    .get(sid)
                    .map(|(_, _, task)| task.agent_type.clone())
                    .unwrap_or_else(|| "general-purpose".to_string());
                let cancel_c = cancel.clone();
                stream.push(Box::pin(async move {
                    let res = poll_background_agent(
                        manager,
                        sid_c.clone(),
                        agent_c,
                        bg_c,
                        timeout,
                        cancel_c,
                    )
                    .await;
                    (sid_c, 0u32, res)
                }));
            }

            while let Some((sid, attempt, (subtask_result, _))) = stream.next().await {
                let is_fail = subtask_result.status == ExecutionStatus::Failed;
                // Cancelled subtasks must NOT be retried — propagate immediately.
                let is_cancelled = subtask_result.status == ExecutionStatus::Cancelled;

                if is_cancelled {
                    let cancelled_supervisor = subtask_meta
                        .get(&sid)
                        .and_then(|(_, _, t)| t.supervisor_agent_type.clone());
                    let cancelled_stage = subtask_meta
                        .get(&sid)
                        .and_then(|(_, _, t)| subtask_stage_label(t));
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "worker_cancelled",
                            phase: Some("executing"),
                            task_id: Some(&sid),
                            payload: serde_json::json!({
                                "planId": plan_id,
                                "agentType": subtask_result.agent_type.clone(),
                                "supervisorAgentType": cancelled_supervisor,
                                "stage": cancelled_stage,
                                "error": subtask_result.error.clone(),
                            }),
                        },
                    )
                    .await;
                    result.subtask_results.insert(sid.clone(), subtask_result);
                    continue;
                }

                if is_fail {
                    let max_retries = subtask_meta.get(&sid).map(|(_, r, _)| *r).unwrap_or(2);

                    if attempt < max_retries {
                        let backoff = 1u64 << attempt.min(4);
                        self.log(
                            &mut result,
                            Some(&sid),
                            &format!(
                                "子任务失败，{} s 后重试（第 {}/{} 次）",
                                backoff,
                                attempt + 1,
                                max_retries
                            ),
                            LogLevel::Warning,
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;

                        let Some(subtask) = subtask_meta.get(&sid).map(|(_, _, t)| t.clone())
                        else {
                            tracing::warn!(target: "omiga::scheduler", sid = %sid, "subtask missing from meta on retry; skipping");
                            continue;
                        };
                        let effective_root = std::path::Path::new(&request.project_root);
                        // Re-inject blackboard context on retry as well.
                        let retry_dep_context = if subtask.dependencies.is_empty() {
                            String::new()
                        } else {
                            let mut dep_ctx = String::new();
                            for dep_id in &subtask.dependencies {
                                for e in blackboard.query_by_subtask(dep_id) {
                                    dep_ctx.push_str(&format!(
                                        "\n\n---\n**[{}] {} 的输出**\n\n{}",
                                        e.subtask_id,
                                        e.agent_type,
                                        e.value.trim()
                                    ));
                                }
                            }
                            if dep_ctx.is_empty() {
                                String::new()
                            } else {
                                format!("\n\n## 上游子任务结果（来自共享黑板）{}", dep_ctx)
                            }
                        };
                        let retry_base = if subtask.context.is_empty() {
                            subtask.description.clone()
                        } else {
                            format!("{}\n\n{}", subtask.description, subtask.context)
                        };
                        let agent_args = crate::domain::tools::agent::AgentArgs {
                            description: subtask.description.clone(),
                            prompt: if retry_dep_context.is_empty() {
                                retry_base
                            } else {
                                format!("{}{}", retry_base, retry_dep_context)
                            },
                            subagent_type: Some(subtask.agent_type.clone()),
                            model: None,
                            run_in_background: None,
                            cwd: None,
                        };
                        let router = crate::domain::agents::get_agent_router();
                        let agent_def = router.select_agent(Some(&subtask.agent_type));
                        let new_msg_id = uuid::Uuid::new_v4().to_string();
                        let next_attempt = attempt + 1;
                        match crate::commands::chat::spawn_background_agent(
                            crate::commands::chat::BackgroundAgentRequest {
                                app,
                                message_id: &new_msg_id,
                                session_id,
                                plan_id: Some(plan_id.as_str()),
                                tool_results_dir: &tool_results_dir,
                                effective_root,
                                session_todos: None,
                                session_agent_tasks: None,
                                args: &agent_args,
                                runtime,
                                subagent_execute_depth: 1,
                                web_search_api_keys: web_search_keys.clone(),
                                skill_cache: skill_cache.clone(),
                                agent_def,
                            },
                        )
                        .await
                        {
                            Ok(new_bg_id) => {
                                persist_subtask!(
                                    &sid,
                                    "running",
                                    next_attempt,
                                    Some(new_bg_id.clone()),
                                    None::<String>
                                );
                                let timeout = subtask_meta.get(&sid).and_then(|(t, _, _)| *t);
                                let sid_c = sid.clone();
                                let agent_c = subtask_meta
                                    .get(&sid)
                                    .map(|(_, _, task)| task.agent_type.clone())
                                    .unwrap_or_else(|| "general-purpose".to_string());
                                let cancel_c = cancel.clone();
                                stream.push(Box::pin(async move {
                                    let res = poll_background_agent(
                                        manager,
                                        sid_c.clone(),
                                        agent_c,
                                        new_bg_id,
                                        timeout,
                                        cancel_c,
                                    )
                                    .await;
                                    (sid_c, next_attempt, res)
                                }));
                            }
                            Err(e) => {
                                self.log(
                                    &mut result,
                                    Some(&sid),
                                    &format!("重试启动失败: {}", e),
                                    LogLevel::Error,
                                );
                                persist_subtask!(
                                    &sid,
                                    "failed",
                                    attempt,
                                    None::<String>,
                                    Some(e.clone())
                                );
                                append_orchestration_runtime_event(
                                    repo,
                                    RuntimeOrchestrationEvent {
                                        session_id,
                                        round_id: Some(runtime.round_id()),
                                        mode: Some(orchestration_mode),
                                        event_type: "worker_launch_failed",
                                        phase: Some("executing"),
                                        task_id: Some(&sid),
                                        payload: serde_json::json!({
                                            "planId": plan_id,
                                            "agentType": subtask.agent_type,
                                            "supervisorAgentType": subtask.supervisor_agent_type,
                                            "stage": subtask_stage_label(&subtask),
                                            "description": subtask.description,
                                            "error": e,
                                            "attempt": next_attempt,
                                        }),
                                    },
                                )
                                .await;
                                append_orchestration_runtime_event(
                                    repo,
                                    RuntimeOrchestrationEvent {
                                        session_id,
                                        round_id: Some(runtime.round_id()),
                                        mode: Some(orchestration_mode),
                                        event_type: "executor_child_failed",
                                        phase: Some("executing"),
                                        task_id: Some(&sid),
                                        payload: serde_json::json!({
                                            "planId": plan_id,
                                            "agentType": subtask.agent_type,
                                            "supervisorAgentType": subtask.supervisor_agent_type,
                                            "stage": subtask_stage_label(&subtask),
                                            "description": subtask.description,
                                            "error": e,
                                            "attempt": next_attempt,
                                            "reason": "retry_launch_failed",
                                        }),
                                    },
                                )
                                .await;
                                result.subtask_results.insert(sid, subtask_result);
                            }
                        }
                        continue;
                    }

                    // Exhausted retries
                    let fail_err = subtask_result
                        .error
                        .clone()
                        .unwrap_or_else(|| "exceeded max retries".to_string());
                    self.log(
                        &mut result,
                        Some(&sid),
                        "子任务失败（已用尽重试次数）",
                        LogLevel::Error,
                    );
                    let failed_supervisor = subtask_meta
                        .get(&sid)
                        .and_then(|(_, _, t)| t.supervisor_agent_type.clone());
                    let failed_stage = subtask_meta
                        .get(&sid)
                        .and_then(|(_, _, t)| subtask_stage_label(t));
                    let failed_description = subtask_meta
                        .get(&sid)
                        .map(|(_, _, t)| t.description.clone())
                        .unwrap_or_default();
                    persist_subtask!(&sid, "failed", attempt, None::<String>, Some(fail_err));
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "worker_failed",
                            phase: Some("executing"),
                            task_id: Some(&sid),
                            payload: serde_json::json!({
                                "planId": plan_id,
                                "agentType": subtask_result.agent_type.clone(),
                                "supervisorAgentType": failed_supervisor.clone(),
                                "stage": failed_stage.clone(),
                                "description": failed_description.clone(),
                                "error": subtask_result.error.clone(),
                                "attempt": attempt,
                            }),
                        },
                    )
                    .await;
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "executor_child_failed",
                            phase: Some("executing"),
                            task_id: Some(&sid),
                            payload: serde_json::json!({
                                "planId": plan_id,
                                "agentType": subtask_result.agent_type.clone(),
                                "supervisorAgentType": failed_supervisor.clone(),
                                "stage": failed_stage.clone(),
                                "description": failed_description.clone(),
                                "error": subtask_result.error.clone(),
                                "attempt": attempt,
                                "reason": "retries_exhausted",
                            }),
                        },
                    )
                    .await;
                    if executor_debug_rounds < MAX_EXECUTOR_DEBUG_ROUNDS && !cancel.is_cancelled() {
                        executor_debug_rounds += 1;
                        let blackboard_snapshot = blackboard.snapshot_markdown();
                        let debug_result =
                            run_executor_debug_attempt(ExecutorDebugAttemptRequest {
                                app,
                                runtime,
                                repo,
                                session_id,
                                plan_id: &plan_id,
                                orchestration_mode,
                                request,
                                tool_results_dir: &tool_results_dir,
                                web_search_keys: web_search_keys.clone(),
                                skill_cache: skill_cache.clone(),
                                cancel: cancel.clone(),
                                failed_subtask_id: &sid,
                                failed_agent_type: subtask_result.agent_type.as_deref(),
                                failed_description: &failed_description,
                                failed_error: subtask_result
                                    .error
                                    .as_deref()
                                    .unwrap_or("subtask failed after retries"),
                                blackboard_snapshot: &blackboard_snapshot,
                                debug_round: executor_debug_rounds,
                            })
                            .await;
                        if let Some(debug_result) = debug_result {
                            let debug_output = debug_result
                                .output
                                .clone()
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| "（debugger 已完成，无文本输出）".to_string());
                            blackboard.post(crate::domain::blackboard::BlackboardEntry {
                                subtask_id: debug_result.subtask_id.clone(),
                                agent_type: "debugger".to_string(),
                                key: "result".to_string(),
                                value: debug_output,
                                posted_at: chrono::Utc::now(),
                            });
                            let _ =
                                crate::domain::blackboard::write_board(project_root, &blackboard)
                                    .await;
                            result
                                .subtask_results
                                .insert(debug_result.subtask_id.clone(), debug_result);
                        }
                    } else {
                        append_orchestration_runtime_event(
                            repo,
                            RuntimeOrchestrationEvent {
                                session_id,
                                round_id: Some(runtime.round_id()),
                                mode: Some(orchestration_mode),
                                event_type: "executor_escalated",
                                phase: Some("executing"),
                                task_id: Some(&sid),
                                payload: serde_json::json!({
                                    "planId": plan_id,
                                    "agentType": subtask_result.agent_type.clone(),
                                    "supervisorAgentType": failed_supervisor.clone(),
                                    "stage": failed_stage.clone(),
                                    "description": failed_description.clone(),
                                    "error": subtask_result.error.clone(),
                                    "reason": "debug_budget_exhausted",
                                    "debugRounds": executor_debug_rounds,
                                }),
                            },
                        )
                        .await;
                    }
                    let is_critical = subtask_meta
                        .get(&sid)
                        .map(|(_, _, t)| t.critical)
                        .unwrap_or(false);
                    let critical_err = subtask_result.error.clone();
                    let critical_agent = subtask_meta
                        .get(&sid)
                        .map(|(_, _, t)| t.agent_type.clone())
                        .unwrap_or_default();
                    result.subtask_results.insert(sid.clone(), subtask_result);
                    if is_critical {
                        team_st.phase = TeamPhase::Failed;
                        team_st.touch();
                        let _ = team_state::write_state(project_root, &team_st).await;
                        let _ = app.emit(
                            "team-worker-update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "subtaskId": sid,
                                "agentType": critical_agent.clone(),
                                "status": "failed_critical",
                                "error": critical_err.as_deref().unwrap_or("critical failure"),
                            }),
                        );
                        result.status = ExecutionStatus::Failed;
                        result.final_summary = format!("关键任务 {} 失败，中止执行", sid);
                        append_orchestration_runtime_event(
                            repo,
                            RuntimeOrchestrationEvent {
                                session_id,
                                round_id: Some(runtime.round_id()),
                                mode: Some(orchestration_mode),
                                event_type: "executor_escalated",
                                phase: Some("executing"),
                                task_id: Some(&sid),
                                payload: serde_json::json!({
                                    "planId": plan_id,
                                    "agentType": critical_agent,
                                    "supervisorAgentType": failed_supervisor,
                                    "stage": failed_stage,
                                    "description": failed_description,
                                    "error": critical_err.clone(),
                                    "reason": "critical_failure",
                                    "debugRounds": executor_debug_rounds,
                                }),
                            },
                        )
                        .await;
                        return Ok(result);
                    }
                } else {
                    let completed_supervisor = subtask_meta
                        .get(&sid)
                        .and_then(|(_, _, t)| t.supervisor_agent_type.clone());
                    let completed_stage = subtask_meta
                        .get(&sid)
                        .and_then(|(_, _, t)| subtask_stage_label(t));
                    let completed_description = subtask_meta
                        .get(&sid)
                        .map(|(_, _, t)| t.description.clone())
                        .unwrap_or_default();
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "worker_completed",
                            phase: Some("executing"),
                            task_id: Some(&sid),
                            payload: serde_json::json!({
                                "planId": plan_id,
                                "agentType": subtask_result.agent_type.clone(),
                                "supervisorAgentType": completed_supervisor.clone(),
                                "stage": completed_stage.clone(),
                                "description": completed_description.clone(),
                                "attempt": attempt,
                            }),
                        },
                    )
                    .await;
                    append_orchestration_runtime_event(
                        repo,
                        RuntimeOrchestrationEvent {
                            session_id,
                            round_id: Some(runtime.round_id()),
                            mode: Some(orchestration_mode),
                            event_type: "executor_child_completed",
                            phase: Some("executing"),
                            task_id: Some(&sid),
                            payload: serde_json::json!({
                                "planId": plan_id,
                                "agentType": subtask_result.agent_type.clone(),
                                "supervisorAgentType": completed_supervisor,
                                "stage": completed_stage,
                                "description": completed_description,
                                "attempt": attempt,
                            }),
                        },
                    )
                    .await;
                    if let (Some(agent_type), Some(output)) = (
                        subtask_result.agent_type.as_deref(),
                        subtask_result.output.as_deref(),
                    ) {
                        if [
                            "verification",
                            "code-reviewer",
                            "security-reviewer",
                            "performance-reviewer",
                            "quality-reviewer",
                            "api-reviewer",
                            "critic",
                            "test-engineer",
                        ]
                        .contains(&agent_type)
                        {
                            let verdict = parse_reviewer_verdict(agent_type, output);
                            append_orchestration_runtime_event(
                                repo,
                                RuntimeOrchestrationEvent {
                                    session_id,
                                    round_id: Some(runtime.round_id()),
                                    mode: Some(orchestration_mode),
                                    event_type: "reviewer_verdict",
                                    phase: Some("verifying"),
                                    task_id: Some(&sid),
                                    payload: serde_json::json!({
                                        "planId": plan_id,
                                        "agentType": agent_type,
                                        "severity": format!("{:?}", verdict.severity),
                                        "verdict": format!("{:?}", verdict.verdict),
                                        "summary": verdict.summary,
                                    }),
                                },
                            )
                            .await;
                        }
                    }
                    // ── Gap 4: post to shared blackboard immediately ──────────
                    self.log(
                        &mut result,
                        Some(&sid),
                        "子任务完成，写入黑板",
                        LogLevel::Info,
                    );
                    persist_subtask!(&sid, "completed", attempt, None::<String>, None::<String>);
                    let agent_type = subtask_meta
                        .get(&sid)
                        .map(|(_, _, t)| t.agent_type.clone())
                        .unwrap_or_default();
                    // Notify frontend: worker completed (include a short preview of output)
                    let preview = subtask_result
                        .output
                        .as_deref()
                        .map(|s| s.chars().take(200).collect::<String>())
                        .unwrap_or_default();
                    let _ = app.emit(
                        "team-worker-update",
                        serde_json::json!({
                            "sessionId": session_id,
                            "subtaskId": sid,
                            "agentType": agent_type,
                            "status": "completed",
                            "preview": preview,
                        }),
                    );
                    // Always post to the blackboard so downstream query_by_subtask
                    // always finds an entry for completed dependencies — even when
                    // output is empty we post a sentinel so injection isn't silently skipped.
                    let output_val = subtask_result
                        .output
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "（任务已完成，无文本输出）".to_string());
                    blackboard.post(crate::domain::blackboard::BlackboardEntry {
                        subtask_id: sid.clone(),
                        agent_type,
                        key: "result".to_string(),
                        value: output_val,
                        posted_at: chrono::Utc::now(),
                    });
                    let _ = crate::domain::blackboard::write_board(project_root, &blackboard).await;
                    result.subtask_results.insert(sid, subtask_result);
                }
            }
        }

        self.finalize_result(&mut result, plan.subtasks.len());

        // ── Team 模式：verify 失败 → Fixing 循环（最多 3 轮）────────────────────
        // 当 team-verify 子任务失败时，派遣 debugger 读取黑板并修复问题，之后重新验证。
        if is_team && !cancel.is_cancelled() {
            let verify_failed = result
                .subtask_results
                .get(crate::domain::agents::scheduler::planner::TEAM_VERIFY_TASK_ID)
                .map(|r| r.status == ExecutionStatus::Failed)
                .unwrap_or(false);

            let mut fix_round = 0u32;
            const MAX_FIX_ROUNDS: u32 = 3;
            let mut verify_failed_now = verify_failed;

            while verify_failed_now && fix_round < MAX_FIX_ROUNDS && !cancel.is_cancelled() {
                fix_round += 1;
                team_st.phase = TeamPhase::Fixing;
                team_st.touch();
                let _ = team_state::write_state(project_root, &team_st).await;
                append_orchestration_runtime_event(
                    repo,
                    RuntimeOrchestrationEvent {
                        session_id,
                        round_id: Some(runtime.round_id()),
                        mode: Some(orchestration_mode),
                        event_type: "phase_changed",
                        phase: Some("fixing"),
                        task_id: None,
                        payload: serde_json::json!({ "round": fix_round }),
                    },
                )
                .await;
                append_orchestration_runtime_event(
                    repo,
                    RuntimeOrchestrationEvent {
                        session_id,
                        round_id: Some(runtime.round_id()),
                        mode: Some(orchestration_mode),
                        event_type: "fix_started",
                        phase: Some("fixing"),
                        task_id: None,
                        payload: serde_json::json!({ "round": fix_round }),
                    },
                )
                .await;
                self.log(
                    &mut result,
                    None,
                    &format!(
                        "[Team] 验证失败，启动修复轮次 {}/{}",
                        fix_round, MAX_FIX_ROUNDS
                    ),
                    LogLevel::Warning,
                );

                // Build fix prompt from blackboard
                let verify_error = result
                    .subtask_results
                    .get(crate::domain::agents::scheduler::planner::TEAM_VERIFY_TASK_ID)
                    .and_then(|r| r.error.clone())
                    .unwrap_or_else(|| "验证失败，详见黑板".to_string());
                let fix_id = format!("team-fix-{}", fix_round);
                let fix_subtask = crate::domain::agents::scheduler::SubTask::new(
                    &fix_id,
                    format!(
                        "【修复 {}/{}】根据验证失败原因修复问题",
                        fix_round, MAX_FIX_ROUNDS
                    ),
                )
                .with_agent("debugger")
                .with_context(format!(
                    "原始目标: {}\n\n验证阶段发现的问题:\n{}\n\n\
                     请从共享黑板中读取所有 Worker 输出，定位根本原因并修复。\
                     修复后将完整的修复结果输出，供重新验证使用。",
                    request.user_request, verify_error
                ))
                .with_timeout(600)
                .critical();

                let effective_root = std::path::Path::new(&request.project_root);
                let fix_agent_args = crate::domain::tools::agent::AgentArgs {
                    description: fix_subtask.description.clone(),
                    prompt: format!(
                        "{}\n\n{}\n\n## 共享黑板内容\n\n{}",
                        fix_subtask.description,
                        fix_subtask.context,
                        blackboard.snapshot_markdown()
                    ),
                    subagent_type: Some("debugger".to_string()),
                    model: None,
                    run_in_background: None,
                    cwd: None,
                };
                let router = crate::domain::agents::get_agent_router();
                if cancel.is_cancelled() {
                    break;
                }

                let fix_agent_def = router.select_agent(Some("debugger"));
                let fix_msg_id = uuid::Uuid::new_v4().to_string();

                match crate::commands::chat::spawn_background_agent(
                    crate::commands::chat::BackgroundAgentRequest {
                        app,
                        message_id: &fix_msg_id,
                        session_id,
                        plan_id: Some(plan_id.as_str()),
                        tool_results_dir: &tool_results_dir,
                        effective_root,
                        session_todos: None,
                        session_agent_tasks: None,
                        args: &fix_agent_args,
                        runtime,
                        subagent_execute_depth: 1,
                        web_search_api_keys: web_search_keys.clone(),
                        skill_cache: skill_cache.clone(),
                        agent_def: fix_agent_def,
                    },
                )
                .await
                {
                    Ok(fix_bg_id) => {
                        let manager = get_background_agent_manager();
                        let (fix_result, _) = poll_background_agent(
                            manager,
                            fix_id.clone(),
                            "debugger".to_string(),
                            fix_bg_id,
                            Some(600),
                            cancel.clone(),
                        )
                        .await;

                        // Post fix output to blackboard
                        let fix_output = fix_result
                            .output
                            .clone()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "（修复完成，无文本输出）".to_string());
                        blackboard.post(crate::domain::blackboard::BlackboardEntry {
                            subtask_id: fix_id.clone(),
                            agent_type: "debugger".to_string(),
                            key: "result".to_string(),
                            value: fix_output,
                            posted_at: chrono::Utc::now(),
                        });
                        let _ =
                            crate::domain::blackboard::write_board(project_root, &blackboard).await;
                        result.subtask_results.insert(fix_id.clone(), fix_result);

                        // Re-run team-verify
                        team_st.phase = TeamPhase::Verifying;
                        team_st.touch();
                        let _ = team_state::write_state(project_root, &team_st).await;
                        append_orchestration_runtime_event(
                            repo,
                            RuntimeOrchestrationEvent {
                                session_id,
                                round_id: Some(runtime.round_id()),
                                mode: Some(orchestration_mode),
                                event_type: "phase_changed",
                                phase: Some("verifying"),
                                task_id: None,
                                payload: serde_json::json!({ "round": fix_round }),
                            },
                        )
                        .await;
                        append_orchestration_runtime_event(
                            repo,
                            RuntimeOrchestrationEvent {
                                session_id,
                                round_id: Some(runtime.round_id()),
                                mode: Some(orchestration_mode),
                                event_type: "verification_started",
                                phase: Some("verifying"),
                                task_id: None,
                                payload: serde_json::json!({ "round": fix_round }),
                            },
                        )
                        .await;
                        self.log(
                            &mut result,
                            None,
                            &format!("[Team] 修复完成，重新验证（轮次 {}）", fix_round),
                            LogLevel::Info,
                        );

                        let reverify_id = format!("team-verify-{}", fix_round);
                        let reverify_context = format!(
                            "原始目标: {}\n\n这是第 {} 次重新验证。\
                             黑板中包含原始 Worker 输出和 {} 的修复结果。\
                             请确认修复是否解决了所有问题。",
                            request.user_request, fix_round, fix_id
                        );
                        let reverify_args = crate::domain::tools::agent::AgentArgs {
                            description: "重新验证修复结果".to_string(),
                            prompt: format!(
                                "{}\n\n## 共享黑板\n\n{}",
                                reverify_context,
                                blackboard.snapshot_markdown()
                            ),
                            subagent_type: Some("verification".to_string()),
                            model: None,
                            run_in_background: None,
                            cwd: None,
                        };
                        if cancel.is_cancelled() {
                            break;
                        }

                        let reverify_agent_def = router.select_agent(Some("verification"));
                        let reverify_msg_id = uuid::Uuid::new_v4().to_string();

                        match crate::commands::chat::spawn_background_agent(
                            crate::commands::chat::BackgroundAgentRequest {
                                app,
                                message_id: &reverify_msg_id,
                                session_id,
                                plan_id: Some(plan_id.as_str()),
                                tool_results_dir: &tool_results_dir,
                                effective_root,
                                session_todos: None,
                                session_agent_tasks: None,
                                args: &reverify_args,
                                runtime,
                                subagent_execute_depth: 1,
                                web_search_api_keys: web_search_keys.clone(),
                                skill_cache: skill_cache.clone(),
                                agent_def: reverify_agent_def,
                            },
                        )
                        .await
                        {
                            Ok(rv_bg_id) => {
                                let (rv_result, _) = poll_background_agent(
                                    manager,
                                    reverify_id.clone(),
                                    "verification".to_string(),
                                    rv_bg_id,
                                    Some(300),
                                    cancel.clone(),
                                )
                                .await;
                                verify_failed_now = rv_result.status == ExecutionStatus::Failed;
                                let rv_output = rv_result
                                    .output
                                    .clone()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or_else(|| "（重新验证完成）".to_string());
                                blackboard.post(crate::domain::blackboard::BlackboardEntry {
                                    subtask_id: reverify_id.clone(),
                                    agent_type: "verification".to_string(),
                                    key: "result".to_string(),
                                    value: rv_output,
                                    posted_at: chrono::Utc::now(),
                                });
                                let _ = crate::domain::blackboard::write_board(
                                    project_root,
                                    &blackboard,
                                )
                                .await;
                                result.subtask_results.insert(reverify_id, rv_result);
                                // Update overall status based on latest verify
                                if !verify_failed_now {
                                    result.status = ExecutionStatus::Completed;
                                    self.log(
                                        &mut result,
                                        None,
                                        "[Team] 修复后验证通过",
                                        LogLevel::Info,
                                    );
                                }
                            }
                            Err(e) => {
                                self.log(
                                    &mut result,
                                    None,
                                    &format!("[Team] 重新验证启动失败: {}", e),
                                    LogLevel::Error,
                                );
                                verify_failed_now = false; // stop loop on spawn error
                            }
                        }
                    }
                    Err(e) => {
                        self.log(
                            &mut result,
                            None,
                            &format!("[Team] 修复 Agent 启动失败: {}", e),
                            LogLevel::Error,
                        );
                        verify_failed_now = false;
                    }
                }
            }
        }

        // Append blackboard snapshot to the final summary so the caller/Architect can read it
        if !blackboard.is_empty() {
            result.final_summary.push_str("\n\n");
            result
                .final_summary
                .push_str(&blackboard.snapshot_markdown());
        }

        // Team 模式：成功后进入 Synthesizing，信号化给 inject_schedule_summary_message 执行 Leader 综合
        if is_team
            && result.status != ExecutionStatus::Failed
            && result.status != ExecutionStatus::Cancelled
        {
            team_st.phase = TeamPhase::Synthesizing;
            team_st.touch();
            let _ = team_state::write_state(project_root, &team_st).await;
            append_orchestration_runtime_event(
                repo,
                RuntimeOrchestrationEvent {
                    session_id,
                    round_id: Some(runtime.round_id()),
                    mode: Some(orchestration_mode),
                    event_type: "phase_changed",
                    phase: Some("synthesizing"),
                    task_id: None,
                    payload: serde_json::json!({ "planId": plan_id }),
                },
            )
            .await;
            append_orchestration_runtime_event(
                repo,
                RuntimeOrchestrationEvent {
                    session_id,
                    round_id: Some(runtime.round_id()),
                    mode: Some(orchestration_mode),
                    event_type: "synthesizing_started",
                    phase: Some("synthesizing"),
                    task_id: None,
                    payload: serde_json::json!({ "planId": plan_id }),
                },
            )
            .await;
            self.log(
                &mut result,
                None,
                "[Team] 进入综合阶段，Leader 将汇总所有 Worker 输出",
                LogLevel::Info,
            );
        }

        // Persist final Team state
        team_st.phase = if result.status == ExecutionStatus::Failed {
            TeamPhase::Failed
        } else {
            TeamPhase::Complete
        };
        team_st.touch();
        let _ = team_state::write_state(project_root, &team_st).await;

        Ok(result)
    }
}

struct ExecutorDebugAttemptRequest<'a> {
    app: &'a tauri::AppHandle,
    runtime: &'a crate::commands::chat::AgentLlmRuntime,
    repo: &'a crate::domain::persistence::SessionRepository,
    session_id: &'a str,
    plan_id: &'a str,
    orchestration_mode: &'a str,
    request: &'a SchedulingRequest,
    tool_results_dir: &'a std::path::Path,
    web_search_keys: WebSearchApiKeys,
    skill_cache: std::sync::Arc<std::sync::Mutex<crate::domain::skills::SkillCacheMap>>,
    cancel: tokio_util::sync::CancellationToken,
    failed_subtask_id: &'a str,
    failed_agent_type: Option<&'a str>,
    failed_description: &'a str,
    failed_error: &'a str,
    blackboard_snapshot: &'a str,
    debug_round: u32,
}

async fn run_executor_debug_attempt(
    request: ExecutorDebugAttemptRequest<'_>,
) -> Option<SubTaskResult> {
    let ExecutorDebugAttemptRequest {
        app,
        runtime,
        repo,
        session_id,
        plan_id,
        orchestration_mode,
        request,
        tool_results_dir,
        web_search_keys,
        skill_cache,
        cancel,
        failed_subtask_id,
        failed_agent_type,
        failed_description,
        failed_error,
        blackboard_snapshot,
        debug_round,
    } = request;
    let debug_id = format!("executor-debug-{}-{}", debug_round, failed_subtask_id);
    append_orchestration_runtime_event(
        repo,
        RuntimeOrchestrationEvent {
            session_id,
            round_id: Some(runtime.round_id()),
            mode: Some(orchestration_mode),
            event_type: "executor_debug_started",
            phase: Some("debug"),
            task_id: Some(&debug_id),
            payload: serde_json::json!({
                "planId": plan_id,
                "failedSubtaskId": failed_subtask_id,
                "failedAgentType": failed_agent_type,
                "failedDescription": failed_description,
                "failedError": failed_error,
                "debugRound": debug_round,
                "supervisorAgentType": "executor",
                "stage": "debug",
            }),
        },
    )
    .await;

    let prompt = format!(
        "你是 executor 旗下的 debugger Agent。请诊断失败子任务并给出最小修复/重规划建议。\n\n\
         原始目标:\n{}\n\n\
         失败子任务: {}\n\
         失败 Agent: {}\n\
         子任务描述: {}\n\
         错误信息:\n{}\n\n\
         当前共享黑板:\n{}\n\n\
         输出要求：\n\
         1) 根因判断\n2) 可执行修复步骤\n3) 是否需要替代 Agent 或调整计划\n4) 仍需上级 General 告知用户的风险",
        request.user_request,
        failed_subtask_id,
        failed_agent_type.unwrap_or("unknown"),
        failed_description,
        failed_error,
        blackboard_snapshot
    );

    let args = crate::domain::tools::agent::AgentArgs {
        description: format!("诊断失败子任务 {}", failed_subtask_id),
        prompt,
        subagent_type: Some("debugger".to_string()),
        model: None,
        run_in_background: None,
        cwd: None,
    };
    let router = crate::domain::agents::get_agent_router();
    let agent_def = router.select_agent(Some("debugger"));
    let message_id = uuid::Uuid::new_v4().to_string();
    let effective_root = std::path::Path::new(&request.project_root);

    match crate::commands::chat::spawn_background_agent(
        crate::commands::chat::BackgroundAgentRequest {
            app,
            message_id: &message_id,
            session_id,
            plan_id: Some(plan_id),
            tool_results_dir,
            effective_root,
            session_todos: None,
            session_agent_tasks: None,
            args: &args,
            runtime,
            subagent_execute_depth: 1,
            web_search_api_keys: web_search_keys,
            skill_cache,
            agent_def,
        },
    )
    .await
    {
        Ok(bg_task_id) => {
            let (debug_result, _) = poll_background_agent(
                crate::domain::agents::background::get_background_agent_manager(),
                debug_id.clone(),
                "debugger".to_string(),
                bg_task_id,
                Some(600),
                cancel,
            )
            .await;
            let event_type = if debug_result.status == ExecutionStatus::Completed {
                "executor_child_completed"
            } else {
                "executor_child_failed"
            };
            append_orchestration_runtime_event(
                repo,
                RuntimeOrchestrationEvent {
                    session_id,
                    round_id: Some(runtime.round_id()),
                    mode: Some(orchestration_mode),
                    event_type,
                    phase: Some("debug"),
                    task_id: Some(&debug_id),
                    payload: serde_json::json!({
                        "planId": plan_id,
                        "agentType": "debugger",
                        "supervisorAgentType": "executor",
                        "stage": "debug",
                        "failedSubtaskId": failed_subtask_id,
                        "debugRound": debug_round,
                        "error": debug_result.error.clone(),
                    }),
                },
            )
            .await;
            Some(debug_result)
        }
        Err(e) => {
            append_orchestration_runtime_event(
                repo,
                RuntimeOrchestrationEvent {
                    session_id,
                    round_id: Some(runtime.round_id()),
                    mode: Some(orchestration_mode),
                    event_type: "executor_child_failed",
                    phase: Some("debug"),
                    task_id: Some(&debug_id),
                    payload: serde_json::json!({
                        "planId": plan_id,
                        "agentType": "debugger",
                        "supervisorAgentType": "executor",
                        "stage": "debug",
                        "failedSubtaskId": failed_subtask_id,
                        "debugRound": debug_round,
                        "error": e,
                        "reason": "debugger_launch_failed",
                    }),
                },
            )
            .await;
            None
        }
    }
}

/// Poll a single background agent until it reaches a terminal state, times out, or is cancelled.
/// Returns `(SubTaskResult, is_critical_failure)`.
/// Runs independently so callers can drive multiple polls concurrently with `join_all`.
/// `timeout_secs` overrides the global 600 s default when `Some`.
async fn poll_background_agent(
    manager: &'static crate::domain::agents::background::BackgroundAgentManager,
    subtask_id: String,
    agent_type: String,
    bg_task_id: String,
    timeout_secs: Option<u64>,
    cancel: tokio_util::sync::CancellationToken,
) -> (SubTaskResult, bool) {
    use crate::domain::agents::background::BackgroundAgentStatus;

    let effective_timeout = timeout_secs.unwrap_or(600);
    let poll_deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(effective_timeout);

    loop {
        // Bail immediately if the orchestration was cancelled.
        if cancel.is_cancelled() {
            return (
                SubTaskResult {
                    subtask_id,
                    agent_type: Some(agent_type.clone()),
                    status: ExecutionStatus::Cancelled,
                    output: None,
                    error: Some("Orchestration cancelled by user.".to_string()),
                    started_at: None,
                    completed_at: None,
                },
                false,
            );
        }

        match manager.get_task(&bg_task_id).await {
            Some(task) => match task.status {
                BackgroundAgentStatus::Completed => {
                    let output = if let Some(path) = &task.output_path {
                        tokio::fs::read_to_string(path).await.ok()
                    } else {
                        task.result_summary.clone()
                    };
                    return (
                        SubTaskResult {
                            subtask_id,
                            agent_type: Some(agent_type.clone()),
                            status: ExecutionStatus::Completed,
                            output,
                            error: None,
                            started_at: task.started_at,
                            completed_at: task.completed_at,
                        },
                        false,
                    );
                }
                BackgroundAgentStatus::Failed => {
                    let err = task
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "Agent failed.".to_string());
                    return (
                        SubTaskResult {
                            subtask_id,
                            agent_type: Some(agent_type.clone()),
                            status: ExecutionStatus::Failed,
                            output: None,
                            error: Some(err),
                            started_at: task.started_at,
                            completed_at: task.completed_at,
                        },
                        false, // critical-failure check is done by the caller after join_all
                    );
                }
                BackgroundAgentStatus::Cancelled => {
                    return (
                        SubTaskResult {
                            subtask_id,
                            agent_type: Some(agent_type.clone()),
                            status: ExecutionStatus::Cancelled,
                            output: None,
                            error: Some("Cancelled.".to_string()),
                            started_at: task.started_at,
                            completed_at: task.completed_at,
                        },
                        false,
                    );
                }
                BackgroundAgentStatus::Pending | BackgroundAgentStatus::Running => {}
            },
            None => {
                return (
                    SubTaskResult {
                        subtask_id,
                        agent_type: Some(agent_type.clone()),
                        status: ExecutionStatus::Failed,
                        output: None,
                        error: Some("Task vanished from manager.".to_string()),
                        started_at: None,
                        completed_at: None,
                    },
                    false,
                );
            }
        }

        if tokio::time::Instant::now() >= poll_deadline {
            return (
                SubTaskResult {
                    subtask_id,
                    agent_type: Some(agent_type),
                    status: ExecutionStatus::Failed,
                    output: None,
                    error: Some("Timeout waiting for background agent.".to_string()),
                    started_at: None,
                    completed_at: None,
                },
                false,
            );
        }

        tokio::select! {
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {}
            _ = cancel.cancelled() => {}
        }
    }
}

#[cfg(test)]
mod finalize_tests {
    use super::*;

    fn subtask(agent_type: &str, output: &str) -> SubTaskResult {
        SubTaskResult {
            subtask_id: format!("{}-1", agent_type),
            agent_type: Some(agent_type.to_string()),
            status: ExecutionStatus::Completed,
            output: Some(output.to_string()),
            error: None,
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn soft_reviewer_findings_downgrade_completed_to_partial() {
        let mut result = OrchestrationResult {
            plan_id: "p".to_string(),
            status: ExecutionStatus::Running,
            subtask_results: HashMap::from([
                ("exec".to_string(), subtask("executor", "done")),
                (
                    "review".to_string(),
                    subtask("quality-reviewer", "VERDICT: PARTIAL\nNeed follow-up"),
                ),
            ]),
            execution_log: vec![],
            started_at: None,
            completed_at: None,
            final_summary: String::new(),
        };
        AgentOrchestrator::new().finalize_result(&mut result, 2);
        assert_eq!(result.status, ExecutionStatus::PartiallyCompleted);
        assert!(result.final_summary.contains("soft"));
    }

    #[test]
    fn hard_reviewer_findings_fail_the_result() {
        let mut result = OrchestrationResult {
            plan_id: "p".to_string(),
            status: ExecutionStatus::Running,
            subtask_results: HashMap::from([
                ("exec".to_string(), subtask("executor", "done")),
                (
                    "review".to_string(),
                    subtask("security-reviewer", "CRITICAL: secret exposed"),
                ),
            ]),
            execution_log: vec![],
            started_at: None,
            completed_at: None,
            final_summary: String::new(),
        };
        AgentOrchestrator::new().finalize_result(&mut result, 2);
        assert_eq!(result.status, ExecutionStatus::Failed);
        assert!(result.final_summary.contains("hard"));
    }
}
