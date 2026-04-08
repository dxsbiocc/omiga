//! Agent 编排器
//!
//! 协调多个 Agent 的执行，管理依赖关系和结果传递。

use super::{TaskPlan, SchedulingRequest, SubTask};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;


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
        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut result = OrchestrationResult {
            plan_id: plan.plan_id.clone(),
            status: ExecutionStatus::Running,
            subtask_results: HashMap::new(),
            execution_log: Vec::new(),
            started_at: Some(start_time),
            completed_at: None,
            final_summary: String::new(),
        };

        self.log(&mut result, None, "开始执行计划", LogLevel::Info);

        // 获取并行执行组
        let groups = plan.get_parallel_groups();
        
        for (group_idx, group) in groups.iter().enumerate() {
            self.log(&mut result, None, &format!("执行组 {}: {:?}", group_idx + 1, group), LogLevel::Info);

            // 并行执行组内任务
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

            // 等待组内所有任务完成
            for (task_id, handle) in handles {
                match handle.await {
                    Ok(subtask_result) => {
                        let status = subtask_result.status.clone();
                        result.subtask_results.insert(task_id.clone(), subtask_result);
                        
                        if status == ExecutionStatus::Failed {
                            self.log(&mut result, Some(&task_id), "子任务失败", LogLevel::Error);
                            
                            // 如果有关键任务失败，中止整个计划
                            if plan.subtasks.iter().any(|t| t.id == task_id && t.critical) {
                                result.status = ExecutionStatus::Failed;
                                result.final_summary = format!("关键任务 {} 失败，中止执行", task_id);
                                return Ok(result);
                            }
                        } else {
                            self.log(&mut result, Some(&task_id), "子任务完成", LogLevel::Info);
                        }
                    }
                    Err(e) => {
                        self.log(&mut result, Some(&task_id), &format!("执行错误: {}", e), LogLevel::Error);
                        result.subtask_results.insert(task_id.clone(), SubTaskResult {
                            subtask_id: task_id,
                            status: ExecutionStatus::Failed,
                            output: None,
                            error: Some(e.to_string()),
                            started_at: None,
                            completed_at: None,
                        });
                    }
                }
            }
        }

        // 生成最终摘要
        let completed = result.subtask_results.values()
            .filter(|r| r.status == ExecutionStatus::Completed)
            .count();
        let failed = result.subtask_results.values()
            .filter(|r| r.status == ExecutionStatus::Failed)
            .count();

        result.status = if failed == 0 {
            ExecutionStatus::Completed
        } else if completed == 0 {
            ExecutionStatus::Failed
        } else {
            ExecutionStatus::Completed // 部分完成也算完成
        };

        result.completed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );

        let summary = format!(
            "计划执行完成: {}/{} 成功, {} 失败",
            completed,
            plan.subtasks.len(),
            failed
        );
        result.final_summary = summary.clone();

        self.log(&mut result, None, &summary, LogLevel::Info);

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
        Ok(format!("Agent {} executed with prompt: {}", agent_type, prompt))
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
                subtask.agent_type,
                subtask.description
            ));
            if !subtask.dependencies.is_empty() {
                summary.push_str(&format!(
                    "  依赖: {}\n",
                    subtask.dependencies.join(", ")
                ));
            }
        }
        
        summary.push_str(&format!(
            "\n预估执行时间: {} 分钟",
            plan.estimate_total_duration() / 60
        ));
        
        summary
    }

    /// 添加日志条目
    fn log(&self, result: &mut OrchestrationResult, subtask_id: Option<&str>, message: &str, level: LogLevel) {
        let entry = ExecutionLogEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            subtask_id: subtask_id.map(|s| s.to_string()),
            message: message.to_string(),
            level,
        };
        result.execution_log.push(entry);
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
            let start_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            // 这里应该调用实际的 Agent 执行
            // 目前模拟执行
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

            // 模拟成功
            SubTaskResult {
                subtask_id: subtask_id.clone(),
                status: ExecutionStatus::Completed,
                output: Some(format!(
                    "Agent '{}' completed task: {}",
                    agent_type,
                    description
                )),
                error: None,
                started_at: Some(start_time),
                completed_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                ),
            }
        })
    }
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// 快速编排执行
pub async fn execute_single_task(
    agent_type: &str,
    prompt: &str,
    app: &tauri::AppHandle,
) -> Result<String, String> {
    let orchestrator = AgentOrchestrator::new();
    orchestrator.execute_single_agent(agent_type, prompt, app).await
}
