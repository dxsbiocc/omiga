//! 任务规划器
//!
//! 将复杂任务分解为可管理的子任务。

use super::{SchedulingRequest, selector::AgentSelector};
use serde::{Deserialize, Serialize};

/// 子任务
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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

        // Build lookup map once to avoid O(n²) linear scans inside the while loop.
        let task_map: std::collections::HashMap<&str, &SubTask> =
            self.subtasks.iter().map(|t| (t.id.as_str(), t)).collect();

        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut remaining: Vec<String> = self.execution_order.clone();

        while !remaining.is_empty() {
            let mut current_group: Vec<String> = Vec::new();
            let mut still_remaining: Vec<String> = Vec::new();

            for task_id in remaining {
                if let Some(task) = task_map.get(task_id.as_str()) {
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
        let task_map: std::collections::HashMap<&str, &SubTask> =
            self.subtasks.iter().map(|t| (t.id.as_str(), t)).collect();
        self.get_parallel_groups().iter().map(|group| {
            group.iter().map(|id| {
                task_map.get(id.as_str()).map(|t| t.estimated_secs).unwrap_or(60)
            }).max().unwrap_or(60)
        }).sum()
    }
}

/// Minimum heuristic score required to decompose a task into subtasks.
/// Score is accumulated from keyword matches (2–4 pts each), length bonuses,
/// and content-generation detection (+3 pts), so a threshold of 5 requires
/// roughly two distinct indicator matches.
const DECOMPOSITION_SCORE_THRESHOLD: i32 = 5;

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
            // 内容生成任务指标
            ("plan", 3),
            ("itinerary", 4),
            ("travel", 3),
            ("guide", 3),
            ("write", 3),
            ("draft", 3),
            ("设计", 3),
            ("计划", 3),
            ("旅行", 4),
            ("行程", 3),
            ("编写", 3),
            ("起草", 3),
            ("攻略", 4),
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
        
        // 内容生成类任务的特殊检测
        if self.is_content_generation_task(&lower) {
            score += 3;
        }

        score >= DECOMPOSITION_SCORE_THRESHOLD
    }

    /// 判断是否为内容生成类任务
    fn is_content_generation_task(&self, text: &str) -> bool {
        let travel_keywords = ["travel", "itinerary", "trip", "vacation", "tour", "visit",
            "旅行", "旅游", "行程", "度假", "游玩", "攻略"];
        let doc_keywords = ["write", "draft", "create", "generate", "compose",
            "写", "编写", "撰写", "起草", "创作"];
        let plan_keywords = ["plan", "schedule", "arrange", "design",
            "计划", "规划", "安排", "设计"];
        
        let has_travel = travel_keywords.iter().any(|k| text.contains(k));
        let has_doc = doc_keywords.iter().any(|k| text.contains(k));
        let has_plan = plan_keywords.iter().any(|k| text.contains(k));
        
        // 如果同时包含计划和旅行相关词汇，或者是明确的文档编写任务
        (has_plan && has_travel) || (has_doc && has_plan)
    }

    /// 分解任务
    pub async fn decompose(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = request.allow_parallel;
        plan.global_context = format!("Project root: {}", request.project_root);

        // 基于规则的任务分解（预计算 is_content_generation 避免重复调用）
        let lower = request.user_request.to_lowercase();
        let is_content = self.is_content_generation_task(&lower);
        let subtasks = self.rule_based_decomposition_inner(&request.user_request, is_content);
        
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

    /// 基于规则的分解（接受预计算的 is_content 标志避免重复 is_content_generation_task 调用）
    fn rule_based_decomposition_inner(&self, request: &str, is_content: bool) -> Vec<SubTask> {
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
        // 模式 5: 内容生成类任务（旅行计划、文档编写等）
        else if is_content {
            // 收集需求
            subtasks.push(
                SubTask::new("gather-requirements", "收集需求并确定关键要素")
                    .with_agent("Plan")
                    .with_context("明确用户的核心需求、约束条件和期望输出格式")
                    .critical()
            );
            // 研究和信息收集
            subtasks.push(
                SubTask::new("research", "研究和收集必要信息")
                    .with_agent("Explore")
                    .with_dependencies(vec!["gather-requirements".to_string()])
                    .with_context("搜索相关信息、数据、案例，为内容生成做准备")
            );
            // 生成主要内容
            subtasks.push(
                SubTask::new("generate-content", "生成完整详细的主要内容")
                    .with_agent("general-purpose")
                    .with_dependencies(vec!["research".to_string()])
                    .with_context("生成完整、详细、有实用价值的内容，不要只是概述。必须包含具体的细节、数据、建议")
                    .critical()
            );
            // 验证和完善
            subtasks.push(
                SubTask::new("verify-complete", "验证内容完整性并补充细节")
                    .with_agent("verification")
                    .with_dependencies(vec!["generate-content".to_string()])
                    .with_context("检查内容是否完整、实用，补充缺失的细节和具体信息")
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
