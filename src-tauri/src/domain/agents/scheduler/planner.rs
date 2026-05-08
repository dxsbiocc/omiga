//! 任务规划器
//!
//! 将复杂任务分解为可管理的子任务。
//!
//! 分解路径：
//! 1. `plan_with_llm` — 使用 LLM 理解语义意图，输出结构化 `TaskPlan` JSON（首选）。
//! 2. `decompose_heuristic` — 纯关键词启发式分解，作为 LLM 失败时的兜底。

use super::{selector::AgentSelector, SchedulingRequest};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

pub const TEAM_VERIFY_TASK_ID: &str = "team-verify";
pub const TEAM_VERIFY_DESCRIPTION: &str = "【Team 核查】核查所有分析结果，确认原始科研问题已被回答";

const SHORT_REQUEST_LEN: usize = 100;
const MEDIUM_REQUEST_LEN: usize = 300;

const TIMEOUT_STANDARD_SECS: u64 = 300;
const TIMEOUT_DEEP_SECS: u64 = 600;

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
    /// Per-subtask timeout (seconds). None = global default (600 s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Maximum retry attempts on failure (0 = no retry). Default: 2.
    #[serde(default = "SubTask::default_max_retries")]
    pub max_retries: u32,
    /// Logical supervisor for this worker. In v1 this is metadata only: the Rust orchestrator
    /// still owns spawning/retry/cancel, while the UI renders an executor-supervised hierarchy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervisor_agent_type: Option<String>,
    /// Logical execution stage used by dashboards and event payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<SubTaskStage>,
    /// 任务上下文（传递给 Agent）
    pub context: String,
}

/// Bounded logical stage vocabulary for executor-supervised plans.
///
/// These are observability lanes, not a mandatory lifecycle. The approved project plan remains
/// the source of truth for what should run, which dependencies matter, and which lanes can be
/// skipped, merged, or repeated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubTaskStage {
    Intent,
    Retrieve,
    Download,
    Analyze,
    Visualize,
    Report,
    Verify,
    Debug,
    Synthesize,
    Other,
}

impl SubTaskStage {
    pub fn infer(agent_type: &str, id: &str, description: &str) -> Self {
        let text = format!(
            "{} {} {}",
            agent_type.to_lowercase(),
            id.to_lowercase(),
            description.to_lowercase()
        );

        if text.contains("debug")
            || text.contains("fix")
            || text.contains("排查")
            || text.contains("修复")
        {
            return Self::Debug;
        }
        if text.contains("verify")
            || text.contains("validation")
            || text.contains("review")
            || text.contains("核查")
            || text.contains("验证")
            || text.contains("检查")
        {
            return Self::Verify;
        }
        if text.contains("visual")
            || text.contains("figure")
            || text.contains("plot")
            || text.contains("chart")
            || text.contains("可视化")
            || text.contains("作图")
            || text.contains("图表")
        {
            return Self::Visualize;
        }
        if text.contains("report")
            || text.contains("write")
            || text.contains("draft")
            || text.contains("撰写")
            || text.contains("报告")
        {
            return Self::Report;
        }
        if text.contains("synthesize") || text.contains("synthesis") || text.contains("综合") {
            return Self::Synthesize;
        }
        if text.contains("download")
            || text.contains("fetch data")
            || text.contains("dataset")
            || text.contains("下载")
            || text.contains("数据集")
        {
            return Self::Download;
        }
        if text.contains("analysis")
            || text.contains("analyze")
            || text.contains("data-analysis")
            || text.contains("统计")
            || text.contains("分析")
            || text.contains("预处理")
        {
            return Self::Analyze;
        }
        if text.contains("retrieve")
            || text.contains("search")
            || text.contains("literature-search")
            || text.contains("pubmed")
            || text.contains("检索")
            || text.contains("文献")
        {
            return Self::Retrieve;
        }
        if text.contains("plan")
            || text.contains("scope")
            || text.contains("intent")
            || text.contains("意图")
            || text.contains("界定")
        {
            return Self::Intent;
        }

        match agent_type {
            "Plan" => Self::Intent,
            "literature-search" => Self::Retrieve,
            "data-analysis" => Self::Analyze,
            "data-visual" => Self::Visualize,
            "verification" => Self::Verify,
            "debugger" => Self::Debug,
            "deep-research" => Self::Synthesize,
            _ => Self::Other,
        }
    }
}

impl SubTask {
    fn default_max_retries() -> u32 {
        2
    }

    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            agent_type: "general-purpose".to_string(),
            dependencies: Vec::new(),
            critical: false,
            estimated_secs: 60,
            timeout_secs: None,
            max_retries: Self::default_max_retries(),
            supervisor_agent_type: None,
            stage: None,
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

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn with_supervisor(mut self, supervisor: impl Into<String>) -> Self {
        let supervisor = supervisor.into();
        self.supervisor_agent_type = if supervisor.trim().is_empty() {
            None
        } else {
            Some(supervisor)
        };
        self
    }

    pub fn with_stage(mut self, stage: SubTaskStage) -> Self {
        self.stage = Some(stage);
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
    /// Logical top-level user-facing leader. Kept as metadata for UI/event rendering.
    #[serde(
        default = "TaskPlan::default_entry_agent_type",
        skip_serializing_if = "Option::is_none"
    )]
    pub entry_agent_type: Option<String>,
    /// Logical execution leader for child workers. Real spawning remains backend-controlled.
    #[serde(
        default = "TaskPlan::default_execution_supervisor_agent_type",
        skip_serializing_if = "Option::is_none"
    )]
    pub execution_supervisor_agent_type: Option<String>,
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
    fn default_entry_agent_type() -> Option<String> {
        Some("general-purpose".to_string())
    }

    fn default_execution_supervisor_agent_type() -> Option<String> {
        Some("executor".to_string())
    }

    pub fn new(request: impl Into<String>) -> Self {
        Self {
            plan_id: uuid::Uuid::new_v4().to_string(),
            original_request: request.into(),
            entry_agent_type: Self::default_entry_agent_type(),
            execution_supervisor_agent_type: Self::default_execution_supervisor_agent_type(),
            subtasks: Vec::new(),
            execution_order: Vec::new(),
            allow_parallel: true,
            global_context: String::new(),
        }
    }

    /// 创建单 Agent 计划
    pub fn single_agent(agent: &str, request: &str) -> Self {
        let mut plan = Self::new(request);
        let subtask = SubTask::new("task-1", request).with_agent(agent);
        plan.subtasks.push(subtask);
        plan.execution_order.push("task-1".to_string());
        plan.allow_parallel = false;
        plan
    }

    /// 添加子任务
    pub fn add_subtask(&mut self, subtask: SubTask) {
        let mut subtask = subtask;
        self.apply_subtask_execution_defaults(&mut subtask);
        self.execution_order.push(subtask.id.clone());
        self.subtasks.push(subtask);
    }

    fn apply_subtask_execution_defaults(&self, subtask: &mut SubTask) {
        if subtask
            .supervisor_agent_type
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            subtask.supervisor_agent_type = self
                .execution_supervisor_agent_type
                .clone()
                .or_else(Self::default_execution_supervisor_agent_type);
        }
        if subtask.stage.is_none() {
            subtask.stage = Some(SubTaskStage::infer(
                &subtask.agent_type,
                &subtask.id,
                &subtask.description,
            ));
        }
    }

    pub fn with_execution_defaults(mut self) -> Self {
        if self
            .entry_agent_type
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            self.entry_agent_type = Self::default_entry_agent_type();
        }
        if self
            .execution_supervisor_agent_type
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            self.execution_supervisor_agent_type = Self::default_execution_supervisor_agent_type();
        }
        let plan_defaults = Self {
            plan_id: self.plan_id.clone(),
            original_request: self.original_request.clone(),
            entry_agent_type: self.entry_agent_type.clone(),
            execution_supervisor_agent_type: self.execution_supervisor_agent_type.clone(),
            subtasks: vec![],
            execution_order: vec![],
            allow_parallel: self.allow_parallel,
            global_context: self.global_context.clone(),
        };
        for subtask in &mut self.subtasks {
            plan_defaults.apply_subtask_execution_defaults(subtask);
        }
        self
    }

    /// 获取可并行执行的任务组
    pub fn get_parallel_groups(&self) -> Vec<Vec<String>> {
        if !self.allow_parallel {
            return self
                .execution_order
                .iter()
                .map(|id| vec![id.clone()])
                .collect();
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
                    let deps_satisfied =
                        task.dependencies.iter().all(|dep| completed.contains(dep));
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
        self.get_parallel_groups()
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|id| {
                        task_map
                            .get(id.as_str())
                            .map(|t| t.estimated_secs)
                            .unwrap_or(60)
                    })
                    .max()
                    .unwrap_or(60)
            })
            .sum()
    }
}

/// Minimum heuristic score required to decompose a task into subtasks.
/// A threshold of 4 requires roughly two distinct signals (keyword + length, or two keywords).
const DECOMPOSITION_SCORE_THRESHOLD: i32 = 4;

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

    /// 判断是否需要分解任务（启发式快速预检，不调 LLM）。
    ///
    /// 阈值 = 4。任意两个明显信号即可触发 LLM planner 进行语义判断。
    /// 宁可多调一次 planner（planner 返回 single 则 fallback），
    /// 也不要漏掉本该多 Agent 的复杂查询。
    pub fn should_decompose(&self, request: &str) -> bool {
        let indicators: &[(&str, i32)] = &[
            // ── 多步骤顺序信号 ──────────────────────────────
            (" and ", 2),
            (" then ", 2),
            (" after ", 2),
            (" before ", 2),
            ("然后", 2),
            ("接着", 2),
            ("之后", 2),
            ("首先", 2),
            ("分步", 2),
            ("逐步", 2),
            ("step by step", 2),
            // ── 复杂实现 ────────────────────────────────────
            ("implement", 2),
            ("create", 2),
            ("build", 2),
            ("实现", 2),
            ("创建", 2),
            ("构建", 2),
            ("重构", 2),
            ("refactor", 2),
            ("架构", 3),
            ("architecture", 3),
            // ── 分析 / 比较 / 评估 ─────────────────────────
            ("analyze", 3),
            ("analysis", 3),
            ("分析", 3),
            ("compare", 4),
            ("comparison", 3),
            ("比较", 4),
            ("对比", 4),
            ("versus", 3),
            (" vs ", 3),
            ("evaluate", 2),
            ("evaluation", 2),
            ("评估", 2),
            ("assess", 2),
            ("调研", 2),
            ("调查", 2),
            ("investigate", 2),
            // ── 诊断 / 调试 ─────────────────────────────────
            ("debug", 2),
            ("diagnose", 2),
            ("调试", 2),
            ("诊断", 2),
            ("troubleshoot", 2),
            ("为什么", 2),
            ("why is", 2),
            ("why does", 2),
            ("why are", 2),
            // ── 内容生成 ────────────────────────────────────
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
            ("报告", 3),
            ("report", 3),
            ("overview", 3),
            ("概述", 3),
            ("总结", 2),
            ("summarize", 2),
            ("summary", 2),
            // ── 研究 / 文献 ─────────────────────────────────
            ("研究现状", 4),
            ("研究进展", 4),
            ("综述", 4),
            ("领域综述", 5),
            ("研究综述", 5),
            ("领域分析", 4),
            ("领域研究", 3),
            ("最新进展", 3),
            ("research review", 4),
            ("state of the art", 4),
            ("literature review", 4),
            ("survey of", 3),
            ("research landscape", 4),
            ("research status", 4),
            ("field overview", 3),
            ("优化", 2),
            ("optimize", 2),
        ];

        let mut score = 0;
        let lower = request.to_lowercase();

        for (indicator, points) in indicators {
            if lower.contains(indicator) {
                score += points;
            }
        }

        // 长度加分：短消息几乎不需要分解
        let len = request.len();
        if len > SHORT_REQUEST_LEN {
            score += 2;
        }
        if len > MEDIUM_REQUEST_LEN {
            score += 3;
        }

        // 多个问号 = 多个并列问题
        let question_count = lower.matches('?').count() + lower.matches('？').count();
        if question_count >= 2 {
            score += 2 * question_count as i32;
        }

        // 内容生成类特殊检测
        if self.is_content_generation_task(&lower) {
            score += 3;
        }

        score >= DECOMPOSITION_SCORE_THRESHOLD
    }

    /// 判断是否为内容生成类任务
    fn is_content_generation_task(&self, text: &str) -> bool {
        let travel_keywords = [
            "travel",
            "itinerary",
            "trip",
            "vacation",
            "tour",
            "visit",
            "旅行",
            "旅游",
            "行程",
            "度假",
            "游玩",
            "攻略",
        ];
        let doc_keywords = [
            "write", "draft", "create", "generate", "compose", "写", "编写", "撰写", "起草", "创作",
        ];
        let plan_keywords = [
            "plan", "schedule", "arrange", "design", "计划", "规划", "安排", "设计",
        ];
        let research_keywords = [
            "研究现状",
            "研究进展",
            "综述",
            "领域综述",
            "研究综述",
            "领域分析",
            "领域研究",
            "最新进展",
            "研究动态",
            "领域现状",
            "research review",
            "state of the art",
            "literature review",
            "survey of",
            "research landscape",
            "research status",
            "field overview",
        ];

        let has_travel = travel_keywords.iter().any(|k| text.contains(k));
        let has_doc = doc_keywords.iter().any(|k| text.contains(k));
        let has_plan = plan_keywords.iter().any(|k| text.contains(k));
        let has_research = research_keywords.iter().any(|k| text.contains(k));

        // Research analysis queries are always treated as structured content generation
        has_research || (has_plan && has_travel) || (has_doc && has_plan)
    }

    /// Team 模式保障：确保 LLM 产出的 TaskPlan 末尾有 team-verify 验收节点。
    /// 如果 plan 中已有 verification agent 作为收尾，直接复用；否则追加一个。
    pub fn ensure_team_verify(&self, mut plan: TaskPlan, request: &SchedulingRequest) -> TaskPlan {
        // Check if the last subtask is already a verification step
        let already_has_verify = plan
            .subtasks
            .last()
            .map(|t| t.agent_type == "verification")
            .unwrap_or(false);
        if already_has_verify {
            return plan;
        }
        // Collect all current subtask ids as dependencies for team-verify
        let all_ids: Vec<String> = plan.subtasks.iter().map(|t| t.id.clone()).collect();
        plan.add_subtask(
            SubTask::new(TEAM_VERIFY_TASK_ID, TEAM_VERIFY_DESCRIPTION)
                .with_agent("verification")
                .with_dependencies(all_ids)
                .with_context(format!(
                    "原始目标: {}\n\n共享黑板包含所有 Worker 的输出结果。请确认：\n\
                     1) 原始目标是否完全达成\n2) 有无错误或遗漏\n3) 输出质量是否符合要求",
                    request.user_request
                ))
                .with_timeout(TIMEOUT_STANDARD_SECS),
        );
        plan
    }

    /// 分解任务（根据 request.strategy 路由到对应的分解模板）
    pub async fn decompose(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        use crate::domain::agents::scheduler::SchedulingStrategy;
        match request.strategy {
            SchedulingStrategy::Team => self.decompose_team(request),
            SchedulingStrategy::Phased => self.decompose_phased(request),
            SchedulingStrategy::Competitive => self.decompose_competitive(request),
            SchedulingStrategy::VerificationFirst => self.decompose_verification_first(request),
            // Auto/Parallel/Sequential/Single all use the heuristic rule path
            _ => self.decompose_heuristic(request),
        }
    }

    /// Team 模式分解：Leader 统筹，专职 Worker 并行执行，末尾统一追加 team-verify。
    ///
    /// 核心规则：
    /// - Worker 不使用 general-purpose（替换为 executor 或专域 Agent）
    /// - 跳过启发式规则产生的中间验证步骤，由 team-verify 统一验收
    fn decompose_team(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = true;
        plan.global_context = format!(
            "Project root: {}\n[Team 模式] Leader 统筹规划，Worker 专职执行后将结果写入共享黑板。",
            request.project_root
        );

        let lower = request.user_request.to_lowercase();
        let is_content = self.is_content_generation_task(&lower);
        let base_tasks = self.rule_based_decomposition_inner(&request.user_request, is_content);

        let mut worker_ids: Vec<String> = Vec::new();

        for subtask in base_tasks {
            // 跳过中间验证步骤：team-verify 统一验收
            if subtask.agent_type == "verification"
                || subtask.id.starts_with("verify")
                || subtask.id.starts_with("post-verify")
                || subtask.id == "verify-complete"
            {
                continue;
            }

            // Team 模式禁止 general-purpose 作为 Worker
            let agent: String = if subtask.agent_type == "general-purpose" {
                let selected = self
                    .selector
                    .select(&subtask.description, &request.project_root);
                if selected == "general-purpose" {
                    "executor".to_string()
                } else {
                    selected.to_string()
                }
            } else {
                subtask.agent_type.clone()
            };

            let id = subtask.id.clone();
            worker_ids.push(id.clone());

            let mut task = SubTask::new(&id, &subtask.description)
                .with_agent(agent)
                .with_dependencies(subtask.dependencies.clone())
                .with_context(format!(
                    "{}\n\n[Worker 职责] 你是专职 Worker。完成上述任务后将完整结果输出，\
                     供 Leader 汇总和后续 Worker 通过共享黑板读取。",
                    subtask.context
                ))
                .with_timeout(subtask.timeout_secs.unwrap_or(600))
                .with_max_retries(subtask.max_retries);
            if subtask.critical {
                task.critical = true;
            }
            plan.add_subtask(task);
        }

        // 无 Worker 时兜底：选最合适的专域 Agent
        if worker_ids.is_empty() {
            let agent: String = {
                let selected = self
                    .selector
                    .select(&request.user_request, &request.project_root);
                if selected == "general-purpose" {
                    "executor".to_string()
                } else {
                    selected.to_string()
                }
            };
            plan.add_subtask(
                SubTask::new("team-worker-1", &request.user_request)
                    .with_agent(agent)
                    .with_context(
                        "[Worker 职责] 你是专职 Worker，完成任务后将完整结果输出。".to_string(),
                    )
                    .with_timeout(TIMEOUT_DEEP_SECS),
            );
            worker_ids.push("team-worker-1".to_string());
        }

        // 统一追加 team-verify — 依赖所有 Worker，读取黑板进行综合验收
        plan.add_subtask(
            SubTask::new(TEAM_VERIFY_TASK_ID, TEAM_VERIFY_DESCRIPTION)
                .with_agent("verification")
                .with_dependencies(worker_ids)
                .with_context(format!(
                    "原始目标: {}\n\n共享黑板包含所有 Worker 的输出结果。请确认：\n\
                     1) 原始目标是否完全达成\n2) 有无错误或遗漏\n3) 输出质量是否符合要求",
                    request.user_request
                ))
                .with_timeout(TIMEOUT_STANDARD_SECS),
        );

        Ok(plan)
    }

    /// 分阶段策略：先做项目计划，再按计划表达必要的执行/验证阶段。
    ///
    /// Stage labels are descriptive lanes, not a hard-coded pipeline; the plan should choose the
    /// minimum useful graph for the task.
    fn decompose_phased(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = false; // phases are sequential
        plan.global_context = format!(
            "Project root: {}\nProject plan is authoritative; stage labels are flexible lanes, not a fixed workflow.",
            request.project_root
        );

        let research_like = Self::is_research_analysis_task(&request.user_request)
            || self.is_content_generation_task(&request.user_request);
        if research_like {
            let lowered = request.user_request.to_lowercase();
            let quick_or_literature_only = Self::contains_any(
                &lowered,
                &[
                    "只要文献",
                    "只做文献",
                    "仅文献",
                    "文献综述",
                    "综述",
                    "overview",
                    "review",
                    "brief",
                    "quick",
                    "简要",
                    "简单",
                    "不需要数据",
                    "无需数据",
                ],
            );
            let needs_data_lane =
                Self::is_research_analysis_task(&request.user_request) && !quick_or_literature_only;
            let needs_visual_lane = needs_data_lane
                && !Self::contains_any(
                    &lowered,
                    &["不需要可视化", "无需可视化", "不画图", "无需图表"],
                )
                && Self::contains_any(
                    &lowered,
                    &[
                        "可视化",
                        "图表",
                        "作图",
                        "figure",
                        "plot",
                        "表达",
                        "差异",
                        "生信",
                        "tcga",
                        "geo",
                        "肝癌",
                        "癌",
                        "肿瘤",
                        "tumor",
                        "cancer",
                        "功能作用",
                    ],
                );
            let needs_report_lane = !Self::contains_any(
                &lowered,
                &["只要结论", "无需报告", "不需要报告", "no report"],
            );

            plan.add_subtask(
                SubTask::new(
                    "phase-scope",
                    "【项目计划】明确目标、边界、证据路径、数据/文献范围与交付格式",
                )
                .with_agent("Plan")
                .with_stage(SubTaskStage::Intent)
                .with_context(format!(
                    "科研分析目标: {}\n请先形成项目计划：研究问题、关键词/实体、可用证据路径、是否需要数据下载/统计/可视化、排除标准、预期输出。后续 Worker 必须以该计划为依据；不要机械套固定流程。",
                    request.user_request
                ))
                .critical(),
            );
            plan.add_subtask(
                SubTask::new(
                    "phase-retrieve",
                    "【检索阶段】检索并整理相关文献、公共数据库、方法和可靠来源",
                )
                .with_agent("literature-search")
                .with_dependencies(vec!["phase-scope".to_string()])
                .with_stage(SubTaskStage::Retrieve)
                .with_context(format!(
                    "基于项目计划的范围，为科研分析收集证据。\
                    优先 PubMed / Google Scholar / arXiv / bioRxiv / GEO / TCGA / 官方数据库；可选数据源（如 Semantic Scholar、微信公众号）只有在用户显式启用后才使用；\
                    每条证据或数据线索必须保留标题/数据集编号、年份、来源、DOI/URL、关键结论和适用边界。\
                    如果项目计划判定某类来源不必要，请说明跳过理由。原始目标: {}",
                    request.user_request
                ))
                .with_timeout(TIMEOUT_STANDARD_SECS)
                .critical(),
            );

            let analysis_dependency = if needs_data_lane {
                plan.add_subtask(
                    SubTask::new(
                        "phase-download",
                        "【数据获取】按项目计划确认可用公开数据并准备下载/获取方案",
                    )
                    .with_agent("executor")
                    .with_dependencies(vec!["phase-retrieve".to_string()])
                    .with_stage(SubTaskStage::Download)
                    .with_context(format!(
                        "读取项目计划和检索结果，筛选可复现、可访问的数据来源。\
                        v1 不强求真实下载成功；请明确数据集编号/URL、获取方式、文件类型、预期输出路径、失败替代方案。\
                        若项目计划或证据质量表明无需数据下载，请说明并给出替代分析依据。原始目标: {}",
                        request.user_request
                    ))
                    .with_timeout(TIMEOUT_STANDARD_SECS)
                    .critical(),
                );
                "phase-download"
            } else {
                "phase-retrieve"
            };

            plan.add_subtask(
                SubTask::new(
                    "phase-analysis",
                    if needs_data_lane {
                        "【分析阶段】按项目计划对可用文献/数据进行预处理、统计分析与功能解释"
                    } else {
                        "【综合分析】按项目计划综合证据并形成可追溯结论"
                    },
                )
                .with_agent(if needs_data_lane {
                    "data-analysis"
                } else {
                    "deep-research"
                })
                .with_dependencies(vec![analysis_dependency.to_string()])
                .with_stage(if needs_data_lane {
                    SubTaskStage::Analyze
                } else {
                    SubTaskStage::Synthesize
                })
                .with_context(format!(
                    "读取项目计划、前序证据和可用数据/替代依据，围绕原始科研问题形成结构化分析。\
                    必须按项目计划执行；如果需要调整路线，请说明原因和影响。\
                    要求区分事实、推断和不确定性；结论需绑定引用或数据来源；必要时给出下一步实验/分析建议。原始目标: {}",
                    request.user_request
                ))
                .with_timeout(TIMEOUT_DEEP_SECS)
                .critical(),
            );

            let mut final_dependency = "phase-analysis".to_string();
            if needs_visual_lane {
                plan.add_subtask(
                    SubTask::new(
                        "phase-visualize",
                        "【可视化】按项目计划根据分析结果规划或生成关键图表",
                    )
                    .with_agent("data-visual")
                    .with_dependencies(vec!["phase-analysis".to_string()])
                    .with_stage(SubTaskStage::Visualize)
                    .with_context(
                        "读取项目计划和分析结果，只生成能支撑结论的必要图表；如果已有数据文件则生成图表，否则输出可复现绘图代码/图表规格。若计划判定不需要可视化，请说明跳过理由。",
                    )
                    .with_timeout(TIMEOUT_STANDARD_SECS),
                );
                final_dependency = "phase-visualize".to_string();
            }

            if needs_report_lane {
                let mut report_dependencies = vec!["phase-analysis".to_string()];
                if needs_visual_lane {
                    report_dependencies.push("phase-visualize".to_string());
                }
                plan.add_subtask(
                    SubTask::new(
                        "phase-report",
                        "【报告阶段】按项目计划汇总证据、分析、图表与局限性",
                    )
                    .with_agent("deep-research")
                    .with_dependencies(report_dependencies)
                    .with_stage(SubTaskStage::Report)
                    .with_context(format!(
                        "综合项目计划、检索、数据/分析和可视化结果，撰写面向科研用户的结构化报告。\
                        必须保留可追溯引用、数据来源、局限性和下一步建议。原始目标: {}",
                        request.user_request
                    ))
                    .with_timeout(TIMEOUT_DEEP_SECS)
                    .critical(),
                );
                final_dependency = "phase-report".to_string();
            }

            plan.add_subtask(
                SubTask::new(
                    "phase-check",
                    "【核查阶段】检查项目计划是否被正确执行，以及结论边界和交付完整性",
                )
                .with_agent("verification")
                .with_dependencies(vec![final_dependency])
                .with_stage(SubTaskStage::Verify)
                .with_context(
                    "核查最终结果是否遵循项目计划并回答原始科研问题；引用/URL 是否可追溯；是否存在过度推断；是否明确局限性、数据口径和后续建议。",
                ),
            );
            return Ok(plan);
        }

        plan.add_subtask(
            SubTask::new(
                "phase-explore",
                "【探索阶段】理解代码库结构、找到相关文件和模式",
            )
            .with_agent("Explore")
            .with_context(format!("Goal: {}", request.user_request))
            .with_timeout(TIMEOUT_STANDARD_SECS),
        );
        plan.add_subtask(
            SubTask::new("phase-design", "【设计阶段】根据探索结果制定详细实现方案")
                .with_agent("Plan")
                .with_dependencies(vec!["phase-explore".to_string()])
                .with_context(format!("Goal: {}", request.user_request))
                .critical(),
        );
        plan.add_subtask(
            SubTask::new("phase-implement", "【实现阶段】按设计方案执行代码修改")
                .with_agent("executor")
                .with_dependencies(vec!["phase-design".to_string()])
                .with_context(format!("Goal: {}", request.user_request))
                .critical(),
        );
        plan.add_subtask(
            SubTask::new("phase-verify", "【验证阶段】运行测试、检查正确性")
                .with_agent("verification")
                .with_dependencies(vec!["phase-implement".to_string()])
                .with_context(format!("Goal: {}", request.user_request)),
        );
        Ok(plan)
    }

    /// 竞争策略：同一任务由 Executor / Debugger / GeneralPurpose 三个 Agent 并行执行，
    /// 取最先成功完成的结果（通过 max_retries=0 + 黑板让 Architect 选择最佳输出）
    fn decompose_competitive(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = true;
        plan.global_context = format!(
            "Project root: {}\n\n[竞争模式] 三个 Agent 并行解决同一问题，Architect 将从黑板中选择最佳结果。",
            request.project_root
        );

        let context = format!(
            "竞争模式 — 请独立完成以下任务并将完整输出写入结果，不要假设其他 Agent 会补充：\n{}",
            request.user_request
        );

        // Three independent agents attack the same problem; no inter-dependencies
        plan.add_subtask(
            SubTask::new("competitor-a", &request.user_request)
                .with_agent("executor")
                .with_context(format!("{context}\n\n[你是 Competitor A — 专注代码正确性]"))
                .with_max_retries(0), // no retry — competition, not recovery
        );
        plan.add_subtask(
            SubTask::new("competitor-b", &request.user_request)
                .with_agent("general-purpose")
                .with_context(format!(
                    "{context}\n\n[你是 Competitor B — 专注可读性和最佳实践]"
                ))
                .with_max_retries(0),
        );
        plan.add_subtask(
            SubTask::new("competitor-c", &request.user_request)
                .with_agent("debugger")
                .with_context(format!(
                    "{context}\n\n[你是 Competitor C — 专注边界情况和健壮性]"
                ))
                .with_max_retries(0),
        );
        // Architect reads the blackboard and picks the winner
        plan.add_subtask(
            SubTask::new(
                "arbitrate",
                "【仲裁】从黑板中读取三个竞争结果，选择最优方案并整合输出",
            )
            .with_agent("architect")
            .with_dependencies(vec![
                "competitor-a".to_string(),
                "competitor-b".to_string(),
                "competitor-c".to_string(),
            ])
            .with_context(
                "黑板中包含 competitor-a / competitor-b / competitor-c 三个结果。\
                 评估标准：正确性 > 可读性 > 健壮性。选择最佳方案输出，或合并各方优点。"
                    .to_string(),
            )
            .critical(),
        );
        Ok(plan)
    }

    /// 验证优先策略：先验证现状 → 发现问题 → 实现修复 → 再验证
    fn decompose_verification_first(
        &self,
        request: &SchedulingRequest,
    ) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = false;
        plan.global_context = format!("Project root: {}", request.project_root);

        plan.add_subtask(
            SubTask::new("pre-verify", "【预验证】运行现有测试，找出当前问题和失败项")
                .with_agent("verification")
                .with_context(format!(
                    "任务上下文: {}\n请先运行测试套件，记录所有失败和警告。",
                    request.user_request
                ))
                .with_timeout(TIMEOUT_STANDARD_SECS),
        );
        plan.add_subtask(
            SubTask::new("implement", "【实现】根据预验证结果完成任务目标")
                .with_agent("executor")
                .with_dependencies(vec!["pre-verify".to_string()])
                .with_context(format!(
                    "任务目标: {}\n黑板中有预验证结果，根据已知问题制定最小化修改方案。",
                    request.user_request
                ))
                .critical(),
        );
        plan.add_subtask(
            SubTask::new("post-verify", "【后验证】验证修改正确、所有原有测试仍通过")
                .with_agent("verification")
                .with_dependencies(vec!["implement".to_string()])
                .with_context(
                    "对比黑板中的预验证结果，确认：\
                     1) 目标任务已完成 2) 无回归 3) 代码质量不降"
                        .to_string(),
                )
                .critical(),
        );
        Ok(plan)
    }

    /// 启发式分解（Auto/Parallel/Sequential/Single 共用的原有逻辑）
    fn decompose_heuristic(&self, request: &SchedulingRequest) -> Result<TaskPlan, String> {
        let mut plan = TaskPlan::new(&request.user_request);
        plan.allow_parallel = request.allow_parallel;
        plan.global_context = format!("Project root: {}", request.project_root);

        // 基于规则的任务分解（预计算 is_content_generation 避免重复调用）
        let lower = request.user_request.to_lowercase();
        let is_content = self.is_content_generation_task(&lower);
        let subtasks = self.rule_based_decomposition_inner(&request.user_request, is_content);

        // 为每个子任务选择 Agent
        for subtask in subtasks {
            let agent = self
                .selector
                .select(&subtask.description, &request.project_root);
            let mut task = subtask;
            task.agent_type = agent.to_string();
            plan.add_subtask(task);
        }

        // 如果没有分解出子任务，创建一个默认的
        if plan.subtasks.is_empty() {
            let agent = self
                .selector
                .select(&request.user_request, &request.project_root);
            plan.add_subtask(SubTask::new("task-1", &request.user_request).with_agent(agent));
        }

        Ok(plan)
    }

    /// 基于规则的分解（接受预计算的 is_content 标志避免重复 is_content_generation_task 调用）
    fn rule_based_decomposition_inner(&self, request: &str, is_content: bool) -> Vec<SubTask> {
        let mut subtasks = Vec::new();
        let lower = request.to_lowercase();

        // 模式 1: 先探索后设计
        if self.has_pattern(
            &lower,
            &["find", "search"],
            &["design", "implement", "create"],
        ) {
            subtasks.push(
                SubTask::new(
                    "explore",
                    "Explore the codebase to understand the current structure",
                )
                .with_agent("Explore")
                .with_context("Focus on finding relevant files and patterns"),
            );
            subtasks.push(
                SubTask::new("design", "Design the solution based on findings")
                    .with_agent("Plan")
                    .with_dependencies(vec!["explore".to_string()])
                    .critical(),
            );
            if lower.contains("implement") || lower.contains("实现") {
                subtasks.push(
                    SubTask::new("implement", "Implement the designed solution")
                        .with_agent("general-purpose")
                        .with_dependencies(vec!["design".to_string()])
                        .critical(),
                );
                subtasks.push(
                    SubTask::new("verify", "Verify the implementation is correct")
                        .with_agent("verification")
                        .with_dependencies(vec!["implement".to_string()]),
                );
            }
        }
        // 模式 2: 设计然后实现
        else if self.has_pattern(
            &lower,
            &["design", "plan"],
            &["implement", "build", "create"],
        ) {
            subtasks.push(
                SubTask::new("design", "Design the architecture and approach")
                    .with_agent("Plan")
                    .critical(),
            );
            subtasks.push(
                SubTask::new("implement", "Implement the design")
                    .with_agent("general-purpose")
                    .with_dependencies(vec!["design".to_string()])
                    .critical(),
            );
            subtasks.push(
                SubTask::new("verify", "Verify the implementation")
                    .with_agent("verification")
                    .with_dependencies(vec!["implement".to_string()]),
            );
        }
        // 模式 3: 验证现有代码
        else if lower.contains("verify")
            || lower.contains("test")
            || lower.contains("验证")
            || lower.contains("测试")
        {
            if lower.contains("codebase") || lower.contains("project") || lower.contains("代码库")
            {
                subtasks.push(
                    SubTask::new("explore", "Explore the codebase structure").with_agent("Explore"),
                );
                subtasks.push(
                    SubTask::new("verify", "Verify the code quality and correctness")
                        .with_agent("verification")
                        .with_dependencies(vec!["explore".to_string()])
                        .critical(),
                );
            } else {
                subtasks.push(
                    SubTask::new("verify", request)
                        .with_agent("verification")
                        .critical(),
                );
            }
        }
        // 模式 4: 多步骤任务
        else if lower.contains("refactor") || lower.contains("重构") {
            subtasks.push(
                SubTask::new("explore", "Find all files that need to be refactored")
                    .with_agent("Explore"),
            );
            subtasks.push(
                SubTask::new("plan", "Plan the refactoring steps")
                    .with_agent("Plan")
                    .with_dependencies(vec!["explore".to_string()])
                    .critical(),
            );
            subtasks.push(
                SubTask::new("refactor", "Execute the refactoring")
                    .with_agent("general-purpose")
                    .with_dependencies(vec!["plan".to_string()])
                    .critical(),
            );
            subtasks.push(
                SubTask::new("verify", "Verify the refactoring is correct")
                    .with_agent("verification")
                    .with_dependencies(vec!["refactor".to_string()]),
            );
        }
        // 模式 5: 研究分析类任务（研究现状、综述、领域分析等）
        else if Self::is_research_analysis_task(&lower) {
            // Step 1: 学术文献专项检索（literature-search agent）
            subtasks.push(
                SubTask::new("search-papers", "专项检索高质量学术文献和预印本")
                    .with_agent("literature-search")
                    .with_context(format!(
                        "研究任务: {}\n\n请并行搜索以下学术数据库：\
                        1) PubMed: search(category=\"literature\", source=\"pubmed\", query=\"<topic>\")\
                        2) arXiv: search(category=\"literature\", source=\"arxiv\", query=\"<topic>\")\
                        3) Crossref: search(category=\"literature\", source=\"crossref\", query=\"<topic>\")\
                        4) OpenAlex: search(category=\"literature\", source=\"openalex\", query=\"<topic>\")\
                        5) bioRxiv/medRxiv: search(category=\"literature\", source=\"biorxiv\", query=\"<topic>\") and search(category=\"literature\", source=\"medrxiv\", query=\"<topic>\")\
                        6) Recent reviews: search(category=\"web\", source=\"auto\", query=\"<topic> recent review OR latest review\")\
                        7) Foundational papers: search(category=\"web\", source=\"auto\", query=\"<topic> seminal OR foundational OR landmark OR classic\")\
                        可选数据源（如 Semantic Scholar、微信公众号）只有在用户显式启用后才使用。\
                        每条结果必须包含：标题、作者、发表年份、期刊/来源、DOI 或完整 URL。\
                        最少检索 10 篇论文；默认以最近几年为主（按当前日期动态判断，不得写死年份），同时补充早期奠基/方法学/高影响力文献。\
                        最后必须给出参考文献列表，并在综合结论中使用可点击/可 hover 的链接引用；如用编号，编号本身必须带 DOI/URL 链接。",
                        request
                    ))
                    .with_timeout(TIMEOUT_STANDARD_SECS)
                    .critical(),
            );
            // Step 2: 综合深度研究报告（deep-research agent，依赖文献检索结果）
            subtasks.push(
                SubTask::new("synthesize", "综合文献检索结果，撰写详细研究现状报告")
                    .with_agent("deep-research")
                    .with_dependencies(vec!["search-papers".to_string()])
                    .with_context(format!(
                        "原始需求: {}\n\n请从黑板中读取 search-papers 的文献检索结果，\
                        撰写详细的研究现状报告，要求：\
                        1) 按主题/方向分节组织（Markdown 标题层级）\
                        2) 每个论点/进展必须附内联引用，格式：[作者, 年份](DOI 或 URL)\
                        3) 覆盖：研究背景 → 主流方法 → 最新进展 → 核心挑战 → 未来方向\
                        4) 每个方向必须举具体论文/项目/团队作为例子\
                        5) 报告不少于 800 字，引用不少于 5 条，不得编造引用",
                        request
                    ))
                    .with_timeout(TIMEOUT_DEEP_SECS)
                    .critical(),
            );
        }
        // 模式 6: 内容生成类任务（旅行计划、文档编写等）
        else if is_content {
            // 收集需求
            subtasks.push(
                SubTask::new("gather-requirements", "收集需求并确定关键要素")
                    .with_agent("Plan")
                    .with_context("明确用户的核心需求、约束条件和期望输出格式")
                    .critical(),
            );
            // 研究和信息收集
            subtasks.push(
                SubTask::new("research", "研究和收集必要信息")
                    .with_agent("Explore")
                    .with_dependencies(vec!["gather-requirements".to_string()])
                    .with_context("搜索相关信息、数据、案例，为内容生成做准备"),
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
                    .with_context("检查内容是否完整、实用，补充缺失的细节和具体信息"),
            );
        }

        subtasks
    }

    fn contains_any(text: &str, patterns: &[&str]) -> bool {
        patterns.iter().any(|p| text.contains(p))
    }

    /// 判断是否为研究分析类任务（研究现状、综述、领域分析）
    fn is_research_analysis_task(text: &str) -> bool {
        let text = text.to_lowercase();
        const RESEARCH_PATTERNS: &[&str] = &[
            "研究现状",
            "研究进展",
            "综述",
            "领域综述",
            "研究综述",
            "领域分析",
            "领域研究",
            "最新进展",
            "研究动态",
            "领域现状",
            "现状分析",
            "进展综述",
            "分析领域",
            "research review",
            "state of the art",
            "literature review",
            "survey of",
            "research landscape",
            "research status",
            "field overview",
            "review of the",
            "overview of the field",
            "功能作用",
            "肝癌",
            "癌",
            "肿瘤",
            "cancer",
            "tumor",
            "oncology",
        ];
        Self::contains_any(&text, RESEARCH_PATTERNS)
    }

    /// 检查是否有特定模式
    fn has_pattern(&self, text: &str, first: &[&str], second: &[&str]) -> bool {
        let has_first = first.iter().any(|p| text.contains(p));
        let has_second = second.iter().any(|p| text.contains(p));
        has_first && has_second
    }
}

// ── LLM-based planning ─────────────────────────────────────────────────────────
//
// `plan_with_llm` calls the already-configured LLM to semantically understand
// the user request and output a `TaskPlan` JSON directly.  This completely
// replaces the keyword heuristic for the Auto scheduling path — it understands
// "分析某领域研究现状", "survey the field of X", "what's the state of Y" with
// no need for keyword lists.
//
// Protocol:
//   1. Send a compact system prompt + user message to the LLM (no tools).
//   2. The LLM responds with a JSON block.
//   3. Parse → `LlmPlanResponse`.
//   4. Map to `TaskPlan` (or return `None` → caller falls back to heuristic).
//
// The LLM is used with a small, cheap model (haiku/flash) when available, but
// falls back to whatever is configured.  The call uses a 20-second timeout so
// it never blocks the main response for long.

#[derive(Debug, Deserialize)]
struct LlmSubtask {
    id: String,
    description: String,
    agent: String,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    critical: bool,
    #[serde(default)]
    stage: Option<SubTaskStage>,
    #[serde(default)]
    supervisor_agent_type: Option<String>,
    #[serde(default)]
    context: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LlmPlanResponse {
    /// "single" or "multi"
    mode: String,
    /// Recommended execution strategy: "team" | "phased" | "sequential" | "parallel" | "single"
    #[serde(default)]
    strategy: Option<String>,
    #[serde(default)]
    subtasks: Vec<LlmSubtask>,
}

/// Result returned by `plan_with_llm`: the decomposed plan plus the recommended strategy.
pub struct LlmPlanResult {
    pub plan: TaskPlan,
    /// Strategy the planner recommends (may override the caller's Auto default).
    pub strategy: super::SchedulingStrategy,
}

const LLM_PLANNER_SYSTEM_PROMPT: &str = r#"You are a task router for an AI assistant. Given a user request, decide:
1. Whether to use a single agent or multiple specialized agents.
2. Which execution strategy best fits the task.

Respond with ONLY a JSON object — no markdown fences, no prose.

━━━ STRATEGY GUIDE ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
"single"     — simple Q&A, greeting, factual lookup, short translation, one-file edit
"team"       — research / survey / literature review / multi-source analysis / report generation
               → parallel workers gather info independently, Leader synthesizes
"phased"     — single-theme research analysis pipeline: scope → evidence/data collection → analysis → citation/quality check
"sequential" — strict ordering required (debug: pre-verify → fix → post-verify)
"parallel"   — fully independent workstreams with no synthesis needed (rare)

━━━ MANDATORY ROUTING RULES ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
These override all other rules:
• Research / domain survey / 综述 / state-of-the-art / literature review / 研究现状 / 领域分析
  → strategy:"team", use "literature-search" + "deep-research" workers
• "compare" / "比较" / "对比" multiple options, OR multi-perspective analysis
  → strategy:"team", parallel analysis workers
• "analyze" / "分析" a substantial technical or domain question
  → strategy:"team"
• Multi-file codebase feature / refactor requiring Explore + Design + Implement
  → strategy:"phased"
• Debugging an existing bug (pre-check → fix → verify)
  → strategy:"sequential"
• Simple greetings / factual questions / short single-step tasks
  → strategy:"single"

━━━ AVAILABLE AGENTS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
- "general-purpose"    — default; search, file tools, code execution
- "Explore"            — codebase search and exploration
- "Plan"               — architecture and design planning
- "executor"           — code writing and implementation
- "verification"       — testing and quality checks
- "architect"          — high-level design evaluation
- "debugger"           — root-cause analysis and bug fixing
- "literature-search"  — academic: PubMed, arXiv, bioRxiv, Google Scholar in parallel; optional user-enabled sources; DOI/URL per result; min 10 papers
- "deep-research"      — domain synthesis: parallel web searches, ≥800-word report, inline citations [Author, Year](URL)
- "data-analysis"      — scientific data preprocessing/statistics using Python/R
- "data-visual"        — publication-quality scientific figures

━━━ JSON FORMATS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Single agent:
{"mode":"single","strategy":"single"}

Multiple agents:
{"mode":"multi","strategy":"team|phased|sequential|parallel","subtasks":[
  {"id":"t1","description":"...","agent":"...","dependencies":[],"critical":false,"stage":"intent|retrieve|download|analyze|visualize|report|verify|debug|synthesize|other","context":"...","timeout_secs":300}
]}

Rules for subtasks:
- Treat the plan as a project plan: it should explain the work graph, evidence path, dependencies, and acceptance checks that execution will follow
- Stage labels are observability lanes, not a fixed workflow; do NOT force retrieve/download/analyze/visualize/report/verify unless the task actually needs those lanes
- 2–6 subtasks is usually enough; use up to 7 only when the project plan justifies separate retrieve/download/analyze/visualize/report/verify lanes
- Tasks with no dependencies run in parallel automatically
- For executor-supervised research pipelines, use stage labels consistently; use "executor" for glue work only when no dedicated agent exists
- Set critical:true for tasks whose failure should abort the plan
- timeout_secs: 300 for literature-search, 600 for deep-research/synthesis, 120 default
- context: give the worker precise instructions from the project plan (databases to search, citation format, data criteria, skip/merge rules, acceptance checks, etc.)

Respond with JSON only."#;

/// Use the LLM to understand the user's request and produce a `TaskPlan`.
///
/// Returns `None` when:
/// - The request is too short / obviously simple (planner says `"mode":"single"`).
/// - The LLM call fails or times out (caller falls back to heuristic).
///
/// On success returns `LlmPlanResult` carrying both the plan and the recommended strategy.
pub async fn plan_with_llm(
    user_message: &str,
    llm_config: &crate::llm::LlmConfig,
) -> Option<LlmPlanResult> {
    use super::SchedulingStrategy;
    use crate::llm::{create_client, LlmMessage, LlmStreamChunk};

    let trimmed = user_message.trim();
    if trimmed.chars().count() < 15 {
        return None;
    }

    let client = create_client(llm_config.clone()).ok()?;

    let messages = vec![
        LlmMessage::system(LLM_PLANNER_SYSTEM_PROMPT),
        LlmMessage::user(trimmed),
    ];

    // 8-second hard timeout — fast enough not to block the UI noticeably.
    // The planner prompt is compact; modern LLMs respond in 1–3 s.
    let stream_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(8),
        client.send_message_streaming(messages, vec![]),
    )
    .await
    .ok()?
    .ok()?;

    let mut text = String::new();
    let mut stream = stream_result;
    let collect_result = tokio::time::timeout(tokio::time::Duration::from_secs(8), async {
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(LlmStreamChunk::Text(t)) => text.push_str(&t),
                Ok(LlmStreamChunk::Stop { .. }) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;

    if collect_result.is_err() {
        tracing::warn!(target: "omiga::planner", "LLM plan collection timed out");
        return None;
    }

    // Strip markdown fences if the model added them
    let json_str = if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            &text[start..=end]
        } else {
            return None;
        }
    } else {
        return None;
    };

    let response: LlmPlanResponse = serde_json::from_str(json_str)
        .map_err(|e| {
            tracing::warn!(target: "omiga::planner", err = %e, raw = json_str, "LLM plan parse failed")
        })
        .ok()?;

    if response.mode == "single" || response.subtasks.is_empty() {
        tracing::debug!(target: "omiga::planner", strategy = ?response.strategy, "LLM planner: single-agent");
        return None;
    }

    // Parse recommended strategy; default to Team for multi-agent plans when unspecified.
    let strategy = response
        .strategy
        .as_deref()
        .map(SchedulingStrategy::from_planner_hint)
        .unwrap_or(SchedulingStrategy::Team);

    let mut plan = TaskPlan::new(user_message);
    plan.allow_parallel = matches!(
        strategy,
        SchedulingStrategy::Team | SchedulingStrategy::Parallel | SchedulingStrategy::Auto
    );

    for st in response.subtasks {
        let mut subtask = SubTask::new(&st.id, &st.description)
            .with_agent(st.agent)
            .with_dependencies(st.dependencies)
            .with_context(st.context);
        if let Some(supervisor) = st.supervisor_agent_type {
            subtask = subtask.with_supervisor(supervisor);
        }
        if let Some(stage) = st.stage {
            subtask = subtask.with_stage(stage);
        }
        if st.critical {
            subtask = subtask.critical();
        }
        if let Some(t) = st.timeout_secs {
            subtask = subtask.with_timeout(t);
        }
        plan.add_subtask(subtask);
    }

    tracing::info!(
        target: "omiga::planner",
        subtasks = plan.subtasks.len(),
        ?strategy,
        "LLM planner: multi-agent plan accepted"
    );
    Some(LlmPlanResult { plan, strategy })
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

        // Original cases still pass
        assert!(planner.should_decompose(
            "Search for all User models and then design a new authentication system"
        ));
        assert!(!planner.should_decompose("Find all files"));
        assert!(!planner.should_decompose("你好"));

        // New: analysis / comparison queries
        assert!(
            planner.should_decompose("分析一下当前项目中工具调用为什么是顺序执行的，应该如何优化")
        );
        assert!(planner.should_decompose(
            "Compare React vs Vue vs Svelte for our use case and give a recommendation"
        ));
        assert!(planner.should_decompose("对比 PostgreSQL 和 MongoDB 在高并发场景下的性能差异"));

        // New: research / survey
        assert!(planner.should_decompose("综述一下大语言模型在代码生成领域的研究现状"));
        assert!(planner.should_decompose(
            "Analyze the state of the art in transformer architectures for code generation"
        ));

        // New: debug / diagnostic
        assert!(
            planner.should_decompose("为什么工具调用还是顺序执行，没有并行的样子，请帮我诊断问题")
        );

        // New: multi-step implementation
        assert!(planner.should_decompose(
            "Refactor the authentication module to use JWT, then update all dependent services and add tests"
        ));

        // Short / trivial should NOT decompose
        assert!(!planner.should_decompose("hello"));
        assert!(!planner.should_decompose("What time is it?"));
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

    #[test]
    fn old_task_plan_json_gets_executor_supervision_defaults() {
        let raw = r#"{
            "planId":"legacy-plan",
            "originalRequest":"legacy",
            "subtasks":[{
                "id":"task-1",
                "description":"Verify result",
                "agentType":"verification",
                "dependencies":[],
                "critical":false,
                "estimatedSecs":60,
                "maxRetries":2,
                "context":""
            }],
            "executionOrder":["task-1"],
            "allowParallel":true,
            "globalContext":""
        }"#;

        let plan: TaskPlan = serde_json::from_str(raw).expect("legacy plan deserialize");
        let plan = plan.with_execution_defaults();
        assert_eq!(plan.entry_agent_type.as_deref(), Some("general-purpose"));
        assert_eq!(
            plan.execution_supervisor_agent_type.as_deref(),
            Some("executor")
        );
        assert_eq!(
            plan.subtasks[0].supervisor_agent_type.as_deref(),
            Some("executor")
        );
        assert_eq!(plan.subtasks[0].stage, Some(SubTaskStage::Verify));
    }

    #[test]
    fn phased_research_plan_has_executor_supervised_scientific_stages() {
        use crate::domain::agents::scheduler::SchedulingStrategy;

        let planner = TaskPlanner::new();
        let request = SchedulingRequest::new("分析MID1IP1在肝癌中的功能作用")
            .with_project_root(".")
            .with_strategy(SchedulingStrategy::Phased);

        let plan =
            futures::executor::block_on(planner.decompose(&request)).expect("phased research plan");
        let stages: Vec<_> = plan
            .subtasks
            .iter()
            .filter_map(|task| task.stage.clone())
            .collect();

        assert_eq!(plan.entry_agent_type.as_deref(), Some("general-purpose"));
        assert_eq!(
            plan.execution_supervisor_agent_type.as_deref(),
            Some("executor")
        );
        assert!(stages.contains(&SubTaskStage::Retrieve));
        assert!(stages.contains(&SubTaskStage::Download));
        assert!(stages.contains(&SubTaskStage::Analyze));
        assert!(stages.contains(&SubTaskStage::Visualize));
        assert!(stages.contains(&SubTaskStage::Report));
        assert!(stages.contains(&SubTaskStage::Verify));
        assert!(plan
            .subtasks
            .iter()
            .all(|task| task.supervisor_agent_type.as_deref() == Some("executor")));
    }

    #[test]
    fn literature_review_plan_does_not_force_data_pipeline_lanes() {
        use crate::domain::agents::scheduler::SchedulingStrategy;

        let planner = TaskPlanner::new();
        let request = SchedulingRequest::new("写一份肿瘤免疫治疗的文献综述，不需要数据分析")
            .with_project_root(".")
            .with_strategy(SchedulingStrategy::Phased);

        let plan =
            futures::executor::block_on(planner.decompose(&request)).expect("phased review plan");
        let stages: Vec<_> = plan
            .subtasks
            .iter()
            .filter_map(|task| task.stage.clone())
            .collect();

        assert!(plan
            .global_context
            .contains("Project plan is authoritative"));
        assert!(stages.contains(&SubTaskStage::Retrieve));
        assert!(stages.contains(&SubTaskStage::Synthesize));
        assert!(stages.contains(&SubTaskStage::Report));
        assert!(stages.contains(&SubTaskStage::Verify));
        assert!(!stages.contains(&SubTaskStage::Download));
        assert!(!stages.contains(&SubTaskStage::Visualize));
        assert!(!plan
            .subtasks
            .iter()
            .any(|task| task.agent_type == "data-analysis"));
    }
}
