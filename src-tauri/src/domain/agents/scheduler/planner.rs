//! 任务规划器
//!
//! 将复杂任务分解为可管理的子任务。

use super::{SchedulingRequest, selector::AgentSelector};
use serde::{Deserialize, Serialize};

/// 子任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    /// 子任务 ID
    pub id: String,
    /// 子任务描述
    pub description: String,
    /// 选中的 Agent 类型
    pub agent_type: String,
    /// 依赖的子任务 ID 列表
    pub dependencies: Vec<String>,
    /// 是否关键任务
    pub critical: bool,
    /// 预估执行时间（秒）
    pub estimated_secs: u64,
    /// 任务上下文（传递给 Agent）
    pub context: String,
}

impl SubTask {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            agent_type: "general-purpose".to_string(),
            dependencies: Vec::new(),
            critical: false,
            estimated_secs: 60,
            context: String::new(),
        }
    }

    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent_type = agent.into();
        self
    }

    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = context.into();
        self
    }

    pub fn critical(mut self) -> Self {
        self.critical = true;
        self
    }
}

/// 任务执行计划
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    /// 计划 ID
    pub plan_id: String,
    /// 原始请求
    pub original_request: String,
    /// 子任务列表
    pub subtasks: Vec<SubTask>,
    /// 执行顺序（子任务 ID 列表）
    pub execution_order: Vec<String>,
    /// 是否允许并行
    pub allow_parallel: bool,
    /// 全局上下文
    pub global_context: String,
}

impl TaskPlan {
    pub fn new(request: impl Into<String>) -> Self {
        Self {
            plan_id: uuid::Uuid::new_v4().to_string(),
            original_request: request.into(),
            subtasks: Vec::new(),
            execution_order: Vec::new(),
            allow_parallel: true,
            global_context: String::new(),
        }
    }

    /// 创建单 Agent 计划
    pub fn single_agent(agent: &str, request: &str) -> Self {
        let mut plan = Self::new(request);
        let subtask = SubTask::new("task-1", request)
            .with_agent(agent);
        plan.subtasks.push(subtask);
        plan.execution_order.push("task-1".to_string());
        plan.allow_parallel = false;
        plan
    }

    /// 添加子任务
    pub fn add_subtask(&mut self, subtask: SubTask) {
        self.execution_order.push(subtask.id.clone());
        self.subtasks.push(subtask);
    }

    /// 获取可并行执行的任务组
    pub fn get_parallel_groups(&self) -> Vec<Vec<String>> {
        if !self.allow_parallel {
            return self.execution_order.iter().map(|id| vec![id.clone()]).collect();
        }

        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut remaining: Vec<String> = self.execution_order.clone();

        while !remaining.is_empty() {
            let mut current_group: Vec<String> = Vec::new();
            let mut still_remaining: Vec<String> = Vec::new();

            for task_id in remaining {
                if let Some(task) = self.subtasks.iter().find(|t| t.id == task_id) {
                    // 检查依赖是否都已满足
                    let deps_satisfied = task.dependencies.iter().all(|dep| completed.contains(dep));
                    
                    if deps_satisfied {
                        current_group.push(task_id);
                    } else {
                        still_remaining.push(task_id);
                    }
                }
            }

            if current_group.is_empty() && !still_remaining.is_empty() {
                // 依赖循环或错误
                current_group.push(still_remaining.remove(0));
            }

            for id in &current_group {
                completed.insert(id.clone());
            }

            groups.push(current_group);
            remaining = still_remaining;
        }

        groups
    }

    /// 估算总执行时间
    pub fn estimate_total_duration(&self) -> u64 {
        let groups = self.get_parallel_groups();
        groups.iter().map(|group| {
            group.iter().map(|id| {
                self.subtasks.iter()
                    .find(|t| &t.id == id)
                    .map(|t| t.estimated_secs)
                    .unwrap_or(60)
            }).max().unwrap_or(60)
        }).sum()
    }
}

/// 任务规划器
pub struct TaskPlanner {
    selector: AgentSelector,
}

impl TaskPlanner {
    pub fn new() -> Self {
        Self {
            selector: AgentSelector::new(),
        }
    }

    /// 判断是否需要分解任务
    pub fn should_decompose(&self, request: &str) -> bool {
        let indicators = [
            // 任务量指标
            (" and ", 3),
            (" then ", 2),
            (" after ", 2),
            (" before ", 2),
            // 中文
            ("然后", 2),
            ("接着", 2),
            ("之后", 2),
            ("首先", 2),
            // 复杂度指标
            ("implement", 2),
            ("create", 2),
            ("build", 2),
            ("实现", 2),
            ("创建", 2),
            ("构建", 2),
        ];

        let mut score = 0;
        let lower = request.to_lowercase();

        for (indicator, points) in &indicators {
            if lower.contains(indicator) {
                score += points;
            }
        }

        // 长度也是指标
        if request.len() > 200 {
            score += 2;
        }
        if request.len() > 500 {
            score += 3;
        }

        score >= 5 // 阈值
    }

    /// 分解任务
    pub async fn decompose(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = request.allow_parallel;
        plan.global_context = format!("Project root: {}", request.project_root);

        // 基于规则的任务分解
        let subtasks = self.rule_based_decomposition(&request.user_request);
        
        // 为每个子任务选择 Agent
        for subtask in subtasks {
            let agent = self.selector.select(&subtask.description, &request.project_root);
            let mut task = subtask;
            task.agent_type = agent.to_string();
            plan.add_subtask(task);
        }

        // 如果没有分解出子任务，创建一个默认的
        if plan.subtasks.is_empty() {
            let agent = self.selector.select(&request.user_request, &request.project_root);
            plan.add_subtask(
                SubTask::new("task-1", &request.user_request)
                    .with_agent(agent)
            );
        }

        Ok(plan)
    }

    /// 基于规则的分解
    fn rule_based_decomposition(&self, request: &str) -> Vec<SubTask> {
        let mut subtasks = Vec::new();
        let lower = request.to_lowercase();

        // 模式 1: 先探索后设计
        if self.has_pattern(&lower, &["find", "search"], &["design", "implement", "create"]) {
            subtasks.push(
                SubTask::new("explore", "Explore the codebase to understand the current structure")
                    .with_agent("Explore")
                    .with_context("Focus on finding relevant files and patterns")
            );
            subtasks.push(
                SubTask::new("design", "Design the solution based on findings")
                    .with_agent("Plan")
                    .with_dependencies(vec!["explore".to_string()])
                    .critical()
            );
            if lower.contains("implement") || lower.contains("实现") {
                subtasks.push(
                    SubTask::new("implement", "Implement the designed solution")
                        .with_agent("general-purpose")
                        .with_dependencies(vec!["design".to_string()])
                        .critical()
                );
                subtasks.push(
                    SubTask::new("verify", "Verify the implementation is correct")
                        .with_agent("verification")
                        .with_dependencies(vec!["implement".to_string()])
                );
            }
        }
        // 模式 2: 设计然后实现
        else if self.has_pattern(&lower, &["design", "plan"], &["implement", "build", "create"]) {
            subtasks.push(
                SubTask::new("design", "Design the architecture and approach")
                    .with_agent("Plan")
                    .critical()
            );
            subtasks.push(
                SubTask::new("implement", "Implement the design")
                    .with_agent("general-purpose")
                    .with_dependencies(vec!["design".to_string()])
                    .critical()
            );
            subtasks.push(
                SubTask::new("verify", "Verify the implementation")
                    .with_agent("verification")
                    .with_dependencies(vec!["implement".to_string()])
            );
        }
        // 模式 3: 验证现有代码
        else if lower.contains("verify") || lower.contains("test") || lower.contains("验证") || lower.contains("测试") {
            if lower.contains("codebase") || lower.contains("project") || lower.contains("代码库") {
                subtasks.push(
                    SubTask::new("explore", "Explore the codebase structure")
                        .with_agent("Explore")
                );
                subtasks.push(
                    SubTask::new("verify", "Verify the code quality and correctness")
                        .with_agent("verification")
                        .with_dependencies(vec!["explore".to_string()])
                        .critical()
                );
            } else {
                subtasks.push(
                    SubTask::new("verify", request)
                        .with_agent("verification")
                        .critical()
                );
            }
        }
        // 模式 4: 多步骤任务
        else if lower.contains("refactor") || lower.contains("重构") {
            subtasks.push(
                SubTask::new("explore", "Find all files that need to be refactored")
                    .with_agent("Explore")
            );
            subtasks.push(
                SubTask::new("plan", "Plan the refactoring steps")
                    .with_agent("Plan")
                    .with_dependencies(vec!["explore".to_string()])
                    .critical()
            );
            subtasks.push(
                SubTask::new("refactor", "Execute the refactoring")
                    .with_agent("general-purpose")
                    .with_dependencies(vec!["plan".to_string()])
                    .critical()
            );
            subtasks.push(
                SubTask::new("verify", "Verify the refactoring is correct")
                    .with_agent("verification")
                    .with_dependencies(vec!["refactor".to_string()])
            );
        }

        subtasks
    }

    /// 检查是否有特定模式
    fn has_pattern(&self, text: &str, first: &[&str], second: &[&str]) -> bool {
        let has_first = first.iter().any(|p| text.contains(p));
        let has_second = second.iter().any(|p| text.contains(p));
        has_first && has_second
    }
}

impl Default for TaskPlanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_decompose_complex_task() {
        let planner = TaskPlanner::new();
        
        assert!(planner.should_decompose(
            "Search for all User models and then design a new authentication system"
        ));
        
        assert!(!planner.should_decompose(
            "Find all files"
        ));
    }

    #[test]
    fn test_parallel_groups() {
        let mut plan = TaskPlan::new("Test plan");
        plan.allow_parallel = true;
        
        plan.add_subtask(SubTask::new("a", "Task A"));
        plan.add_subtask(SubTask::new("b", "Task B").with_dependencies(vec!["a".to_string()]));
        plan.add_subtask(SubTask::new("c", "Task C"));
        
        let groups = plan.get_parallel_groups();
        // A 和 C 可以并行，B 必须等 A
        assert!(groups.len() >= 2);
    }
}
