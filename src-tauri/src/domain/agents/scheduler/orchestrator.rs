//! Agent 编排器
//!
//! 协调多个 Agent 的执行，管理依赖关系和结果传递。
//!
//! 两种执行模式：
//! - `execute`: mock 模式（向后兼容，用于测试）
//! - `execute_with_runtime`: 真实模式，通过 `spawn_background_agent` 驱动实际 LLM 子 Agent

use super::{SchedulingRequest, SubTask, TaskPlan};

#[inline]
fn unix_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Manager;

use crate::app_state::OmigaAppState;
use crate::domain::tools::WebSearchApiKeys;

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
    Completed,
    Failed,
    Cancelled,
}

/// 子任务结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTaskResult {
    pub subtask_id: String,
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

/// Agent 编排器
pub struct AgentOrchestrator {
    // 执行状态跟踪
}

impl AgentOrchestrator {
    pub fn new() -> Self {
        Self {}
    }

    /// 执行任务计划
    pub async fn execute(
        &self,
        plan: &TaskPlan,
        _request: &SchedulingRequest,
        app: &tauri::AppHandle,
    ) -> Result<OrchestrationResult, String> {
        let mut result = OrchestrationResult {
            plan_id: plan.plan_id.clone(),
            status: ExecutionStatus::Running,
            subtask_results: HashMap::new(),
            execution_log: Vec::new(),
            started_at: Some(unix_timestamp_secs()),
            completed_at: None,
            final_summary: String::new(),
        };

        self.log(&mut result, None, "开始执行计划", LogLevel::Info);

        let groups = plan.get_parallel_groups();

        for (group_idx, group) in groups.iter().enumerate() {
            self.log(
                &mut result,
                None,
                &format!("执行组 {}: {:?}", group_idx + 1, group),
                LogLevel::Info,
            );

            let mut handles = Vec::new();

            for task_id in group {
                if let Some(subtask) = plan.subtasks.iter().find(|t| &t.id == task_id) {
                    let handle = self.spawn_subtask(
                        subtask.clone(),
                        plan.global_context.clone(),
                        app.clone(),
                    );
                    handles.push((task_id.clone(), handle));
                }
            }

            for (task_id, handle) in handles {
                match handle.await {
                    Ok(subtask_result) => {
                        let status = subtask_result.status.clone();
                        result
                            .subtask_results
                            .insert(task_id.clone(), subtask_result);

                        if status == ExecutionStatus::Failed {
                            self.log(&mut result, Some(&task_id), "子任务失败", LogLevel::Error);
                            if plan.subtasks.iter().any(|t| t.id == task_id && t.critical) {
                                result.status = ExecutionStatus::Failed;
                                result.final_summary =
                                    format!("关键任务 {} 失败，中止执行", task_id);
                                return Ok(result);
                            }
                        } else {
                            self.log(&mut result, Some(&task_id), "子任务完成", LogLevel::Info);
                        }
                    }
                    Err(e) => {
                        self.log(
                            &mut result,
                            Some(&task_id),
                            &format!("执行错误: {}", e),
                            LogLevel::Error,
                        );
                        result.subtask_results.insert(
                            task_id.clone(),
                            SubTaskResult {
                                subtask_id: task_id,
                                status: ExecutionStatus::Failed,
                                output: None,
                                error: Some(e.to_string()),
                                started_at: None,
                                completed_at: None,
                            },
                        );
                    }
                }
            }
        }

        self.finalize_result(&mut result, plan.subtasks.len());
        Ok(result)
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

    /// 执行单个 Agent（简单包装）
    pub async fn execute_single_agent(
        &self,
        agent_type: &str,
        prompt: &str,
        _app: &tauri::AppHandle,
    ) -> Result<String, String> {
        // 这里应该调用实际的 Agent 执行
        // 暂时返回模拟结果
        Ok(format!(
            "Agent {} executed with prompt: {}",
            agent_type, prompt
        ))
    }

    /// 执行带确认的计划
    pub async fn execute_with_confirmation(
        &self,
        plan: &TaskPlan,
        request: &SchedulingRequest,
        app: &tauri::AppHandle,
        confirmation_callback: impl FnOnce(&str) -> bool,
    ) -> Result<Option<OrchestrationResult>, String> {
        let summary = self.generate_execution_summary(plan);

        if confirmation_callback(&summary) {
            let result = self.execute(plan, request, app).await?;
            Ok(Some(result))
        } else {
            Ok(None) // 用户取消
        }
    }

    /// 生成执行摘要
    fn generate_execution_summary(&self, plan: &TaskPlan) -> String {
        let mut summary = format!("执行计划将使用以下 Agent:\n\n");

        for subtask in &plan.subtasks {
            summary.push_str(&format!(
                "- [{}] {}\n",
                subtask.agent_type, subtask.description
            ));
            if !subtask.dependencies.is_empty() {
                summary.push_str(&format!("  依赖: {}\n", subtask.dependencies.join(", ")));
            }
        }

        summary.push_str(&format!(
            "\n预估执行时间: {} 分钟",
            plan.estimate_total_duration() / 60
        ));

        summary
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
            ExecutionStatus::Completed // partial completion counts as completed
        };
        result.completed_at = Some(unix_timestamp_secs());

        let summary = format!(
            "计划执行完成: {}/{} 成功, {} 失败",
            completed, total_subtasks, failed
        );
        result.final_summary = summary.clone();
        self.log(result, None, &summary, LogLevel::Info);
    }

    /// 生成子任务执行句柄
    fn spawn_subtask(
        &self,
        subtask: SubTask,
        _global_context: String,
        _app: tauri::AppHandle,
    ) -> tokio::task::JoinHandle<SubTaskResult> {
        let subtask_id = subtask.id.clone();
        let agent_type = subtask.agent_type.clone();
        let description = subtask.description.clone();

        tokio::spawn(async move {
            let start_time = unix_timestamp_secs();
            // TODO: replace with real agent execution
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            SubTaskResult {
                subtask_id: subtask_id.clone(),
                status: ExecutionStatus::Completed,
                output: Some(format!(
                    "Agent '{}' completed task: {}",
                    agent_type, description
                )),
                error: None,
                started_at: Some(start_time),
                completed_at: Some(unix_timestamp_secs()),
            }
        })
    }
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// 真实执行：使用 `spawn_background_agent` 驱动并行子 Agent。
///
/// 与 `execute` (mock) 不同，此方法会真正启动 LLM 子会话并等待结果。
/// 需要调用方提供 `AgentLlmRuntime`（由 `chat.rs` 的 `send_message` 流程持有）。
impl AgentOrchestrator {
    pub(crate) async fn execute_with_runtime(
        &self,
        plan: &TaskPlan,
        request: &SchedulingRequest,
        app: &tauri::AppHandle,
        runtime: &crate::commands::chat::AgentLlmRuntime,
        session_id: &str,
    ) -> Result<OrchestrationResult, String> {
        use crate::domain::agents::background::get_background_agent_manager;

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
            self.log(
                &mut result,
                None,
                &format!("执行组 {}/{}: {:?}", group_idx + 1, groups.len(), group),
                LogLevel::Info,
            );

            // 并行启动组内所有后台 Agent，收集 bg_task_id
            let mut bg_task_ids: Vec<(String, String)> = Vec::new(); // (subtask_id, bg_task_id)

            for task_id_str in group {
                if let Some(subtask) = plan.subtasks.iter().find(|t| &t.id == task_id_str) {
                    let effective_root = std::path::Path::new(&request.project_root);
                    let agent_args = crate::domain::tools::agent::AgentArgs {
                        description: subtask.description.clone(),
                        prompt: if subtask.context.is_empty() {
                            subtask.description.clone()
                        } else {
                            format!("{}\n\n{}", subtask.description, subtask.context)
                        },
                        subagent_type: Some(subtask.agent_type.clone()),
                        model: None,
                        run_in_background: None,
                        cwd: None,
                    };

                    let router = crate::domain::agents::get_agent_router();
                    let agent_def = router.select_agent(Some(&subtask.agent_type));
                    let message_id = uuid::Uuid::new_v4().to_string();

                    match crate::commands::chat::spawn_background_agent(
                        app,
                        &message_id,
                        session_id,
                        &tool_results_dir,
                        effective_root,
                        None, // session_todos
                        None, // session_agent_tasks
                        &agent_args,
                        runtime,
                        1, // subagent_execute_depth
                        web_search_keys.clone(),
                        skill_cache.clone(),
                        agent_def,
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
                            bg_task_ids.push((task_id_str.clone(), bg_task_id));
                        }
                        Err(e) => {
                            self.log(
                                &mut result,
                                Some(task_id_str),
                                &format!("启动后台 Agent 失败: {}", e),
                                LogLevel::Error,
                            );
                            result.subtask_results.insert(
                                task_id_str.clone(),
                                SubTaskResult {
                                    subtask_id: task_id_str.clone(),
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

            // Poll all background agents in this group concurrently instead of sequentially.
            // Sequential polling would make group[1] wait up to 600 s for group[0] even if
            // group[0] is still running while group[1] finished minutes ago.
            let manager = get_background_agent_manager();
            let poll_futures: Vec<_> = bg_task_ids
                .iter()
                .map(|(subtask_id, bg_task_id)| {
                    poll_background_agent(manager, subtask_id.clone(), bg_task_id.clone())
                })
                .collect();

            let poll_results = futures::future::join_all(poll_futures).await;

            for (subtask_result, is_critical_failure) in poll_results {
                let subtask_id = subtask_result.subtask_id.clone();
                let is_fail = subtask_result.status == ExecutionStatus::Failed;
                result
                    .subtask_results
                    .insert(subtask_id.clone(), subtask_result);

                if is_fail {
                    self.log(
                        &mut result,
                        Some(&subtask_id),
                        "子任务失败",
                        LogLevel::Error,
                    );
                    if is_critical_failure {
                        result.status = ExecutionStatus::Failed;
                        result.final_summary = format!("关键任务 {} 失败，中止执行", subtask_id);
                        return Ok(result);
                    }
                } else {
                    self.log(&mut result, Some(&subtask_id), "子任务完成", LogLevel::Info);
                }
            }
        }

        self.finalize_result(&mut result, plan.subtasks.len());
        Ok(result)
    }
}

/// 快速编排执行
pub async fn execute_single_task(
    agent_type: &str,
    prompt: &str,
    app: &tauri::AppHandle,
) -> Result<String, String> {
    let orchestrator = AgentOrchestrator::new();
    orchestrator
        .execute_single_agent(agent_type, prompt, app)
        .await
}

/// Poll a single background agent until it reaches a terminal state or times out.
/// Returns `(SubTaskResult, is_critical_failure)`.
/// Runs independently so callers can drive multiple polls concurrently with `join_all`.
async fn poll_background_agent(
    manager: &'static crate::domain::agents::background::BackgroundAgentManager,
    subtask_id: String,
    bg_task_id: String,
) -> (SubTaskResult, bool) {
    use crate::domain::agents::background::BackgroundAgentStatus;

    let poll_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(600);

    loop {
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
                    status: ExecutionStatus::Failed,
                    output: None,
                    error: Some("Timeout waiting for background agent.".to_string()),
                    started_at: None,
                    completed_at: None,
                },
                false,
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}
