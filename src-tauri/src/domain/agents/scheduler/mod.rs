//! Agent 调度系统
//!
//! 提供自动 Agent 选择、任务分解和多 Agent 编排功能。

pub mod orchestrator;
pub mod planner;
pub mod selector;
pub mod strategy;

pub use orchestrator::{AgentOrchestrator, OrchestrationResult};
pub use planner::{LlmPlanResult, SubTask, TaskPlan, TaskPlanner};
pub use selector::{select_agent_for_task, AgentSelector};
pub use strategy::{SchedulingStrategy, StrategyConfig};

use serde::{Deserialize, Serialize};

/// 调度请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingRequest {
    pub user_request: String,
    pub description: String,
    pub project_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_hint: Option<String>,
    pub strategy: SchedulingStrategy,
    pub allow_parallel: bool,
    pub max_agents: usize,
    pub auto_decompose: bool,
}

impl SchedulingRequest {
    pub fn new(user_request: impl Into<String>) -> Self {
        Self {
            user_request: user_request.into(),
            description: String::new(),
            project_root: ".".to_string(),
            mode_hint: None,
            strategy: SchedulingStrategy::Auto,
            allow_parallel: true,
            max_agents: 5,
            auto_decompose: true,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_project_root(mut self, root: impl Into<String>) -> Self {
        self.project_root = root.into();
        self
    }

    pub fn with_mode_hint(mut self, mode: impl Into<String>) -> Self {
        let mode = mode.into();
        self.mode_hint = if mode.trim().is_empty() {
            None
        } else {
            Some(mode)
        };
        self
    }

    pub fn with_strategy(mut self, strategy: SchedulingStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn with_parallel(mut self, allow: bool) -> Self {
        self.allow_parallel = allow;
        self
    }

    pub fn with_max_agents(mut self, max: usize) -> Self {
        self.max_agents = max;
        self
    }

    pub fn with_auto_decompose(mut self, auto: bool) -> Self {
        self.auto_decompose = auto;
        self
    }
}

/// 调度结果
#[derive(Debug, Clone, Serialize)]
pub struct SchedulingResult {
    /// 执行计划（扁平化序列化）
    #[serde(flatten)]
    pub plan: TaskPlan,
    /// 选中的 Agent 列表
    pub selected_agents: Vec<String>,
    /// 预估执行时间（秒）
    pub estimated_duration_secs: u64,
    /// Reviewer subtasks attached by mode-aware scheduling (if any).
    pub reviewer_agents: Vec<String>,
    /// 是否需要人工确认
    pub requires_confirmation: bool,
    /// 确认提示信息
    pub confirmation_message: Option<String>,
    /// LLM planner 推荐的执行策略（覆盖用户选择的 Auto）
    pub recommended_strategy: SchedulingStrategy,
}

/// 调度器主体
pub struct AgentScheduler {
    selector: AgentSelector,
    planner: TaskPlanner,
    orchestrator: AgentOrchestrator,
}

impl AgentScheduler {
    pub fn new() -> Self {
        Self {
            selector: AgentSelector::new(),
            planner: TaskPlanner::new(),
            orchestrator: AgentOrchestrator::new(),
        }
    }

    /// 执行完整调度流程。
    ///
    /// `llm_config` — 对 Auto/Team 策略调用 `plan_with_llm()` 理解用户意图并推荐策略，
    /// 失败时 fallback 到关键词启发式分解。
    pub async fn schedule(
        &self,
        request: SchedulingRequest,
        llm_config: Option<&crate::llm::LlmConfig>,
    ) -> Result<SchedulingResult, String> {
        let force_single = request.strategy == SchedulingStrategy::Single
            || (!request.auto_decompose
                && !matches!(
                    request.strategy,
                    SchedulingStrategy::Team
                        | SchedulingStrategy::Phased
                        | SchedulingStrategy::Competitive
                        | SchedulingStrategy::VerificationFirst
                        | SchedulingStrategy::Parallel
                        | SchedulingStrategy::Sequential
                ));

        let needs_decomposition = if force_single {
            false
        } else {
            matches!(
                request.strategy,
                SchedulingStrategy::Team
                    | SchedulingStrategy::Phased
                    | SchedulingStrategy::Competitive
                    | SchedulingStrategy::VerificationFirst
                    | SchedulingStrategy::Parallel
                    | SchedulingStrategy::Sequential
            ) || (request.auto_decompose && self.planner.should_decompose(&request.user_request))
        };

        if needs_decomposition {
            // Auto/Team: LLM planner 优先，失败 fallback 到启发式
            let use_llm_planner = llm_config.is_some()
                && matches!(
                    request.strategy,
                    SchedulingStrategy::Auto | SchedulingStrategy::Team
                );

            // (plan, effective_strategy) — effective_strategy 可被 LLM planner 覆盖
            let (mut plan, effective_strategy) = if use_llm_planner {
                let cfg = llm_config.unwrap();
                match planner::plan_with_llm(&request.user_request, cfg).await {
                    Some(llm_result) => {
                        let mut p = llm_result.plan;
                        let strat = llm_result.strategy;

                        // Team 模式（显式或 LLM 推荐）：追加 team-verify 尾节点
                        let is_team_effective = strat == SchedulingStrategy::Team
                            || request.strategy == SchedulingStrategy::Team;
                        if is_team_effective {
                            p = self.planner.ensure_team_verify(p, &request);
                        }

                        tracing::info!(
                            target: "omiga::scheduler",
                            subtasks = p.subtasks.len(),
                            strategy = ?strat,
                            "LLM planner accepted"
                        );
                        (p, strat)
                    }
                    None => {
                        tracing::info!(
                            target: "omiga::scheduler",
                            "LLM planner returned None, falling back to heuristic"
                        );
                        let p = self.planner.decompose(&request).await?;
                        (p, request.strategy)
                    }
                }
            } else {
                let p = self.planner.decompose(&request).await?;
                (p, request.strategy)
            };

            plan = self
                .attach_mode_specific_reviewers(plan, &request)
                .with_execution_defaults();

            let selected_agents: Vec<String> =
                plan.subtasks.iter().map(|t| t.agent_type.clone()).collect();
            let reviewer_agents = self.reviewer_agents(&selected_agents);

            let estimated = self.estimate_duration(&plan, &selected_agents);

            let requires_confirmation = selected_agents.len() > 3
                || plan.subtasks.iter().any(|t| t.critical)
                || matches!(effective_strategy, SchedulingStrategy::Competitive);

            let confirmation_message = if requires_confirmation {
                Some(self.generate_confirmation_message(&plan, &selected_agents))
            } else {
                None
            };

            Ok(SchedulingResult {
                plan,
                selected_agents,
                estimated_duration_secs: estimated,
                reviewer_agents,
                requires_confirmation,
                confirmation_message,
                recommended_strategy: effective_strategy,
            })
        } else {
            let agent = self
                .selector
                .select(&request.user_request, &request.project_root);
            let plan =
                TaskPlan::single_agent(&agent, &request.user_request).with_execution_defaults();

            Ok(SchedulingResult {
                plan,
                selected_agents: vec![agent.to_string()],
                estimated_duration_secs: 60,
                reviewer_agents: vec![],
                requires_confirmation: false,
                confirmation_message: None,
                recommended_strategy: SchedulingStrategy::Single,
            })
        }
    }

    pub(crate) async fn execute_plan_with_runtime(
        &self,
        plan: &TaskPlan,
        request: &SchedulingRequest,
        app: &tauri::AppHandle,
        runtime: &crate::commands::chat::AgentLlmRuntime,
        session_id: &str,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<OrchestrationResult, String> {
        self.orchestrator
            .execute_with_runtime(plan, request, app, runtime, session_id, cancel)
            .await
    }

    fn estimate_duration(&self, plan: &TaskPlan, agents: &[String]) -> u64 {
        let base_time = match plan.subtasks.len() {
            0 => 30,
            1 => 60,
            2..=3 => 120,
            4..=5 => 180,
            _ => 300,
        };

        let agent_multiplier: f64 = agents
            .iter()
            .map(|a| match a.as_str() {
                "Explore" => 0.8,
                "Plan" => 1.5,
                "verification" => 2.0,
                _ => 1.0,
            })
            .sum::<f64>()
            / agents.len().max(1) as f64;

        (base_time as f64 * agent_multiplier) as u64
    }

    fn generate_confirmation_message(&self, plan: &TaskPlan, agents: &[String]) -> String {
        let agent_list = agents.join(", ");
        let task_count = plan.subtasks.len();

        format!(
            "此请求将分解为 {} 个子任务，使用以下 Agent: {}。\n\
             预计执行时间: ~{} 分钟。是否继续？",
            task_count,
            agent_list,
            (self.estimate_duration(plan, agents) + 59) / 60
        )
    }

    fn attach_mode_specific_reviewers(
        &self,
        mut plan: TaskPlan,
        request: &SchedulingRequest,
    ) -> TaskPlan {
        let Some(mode) = request.mode_hint.as_deref() else {
            return plan;
        };

        let already_has_reviewers = plan.subtasks.iter().any(|t| {
            matches!(
                t.agent_type.as_str(),
                "code-reviewer"
                    | "security-reviewer"
                    | "performance-reviewer"
                    | "quality-reviewer"
                    | "api-reviewer"
                    | "critic"
                    | "test-engineer"
            )
        });
        if already_has_reviewers {
            return plan;
        }

        let anchor_ids = match mode {
            "team" => {
                if plan
                    .subtasks
                    .iter()
                    .any(|t| t.id == planner::TEAM_VERIFY_TASK_ID)
                {
                    vec![planner::TEAM_VERIFY_TASK_ID.to_string()]
                } else {
                    self.review_anchor_ids(&plan)
                }
            }
            "autopilot" | "ralph" => self.review_anchor_ids(&plan),
            _ => vec![],
        };

        if anchor_ids.is_empty() {
            return plan;
        }

        let reviewers: Vec<(&str, &str, &str)> = match mode {
            "autopilot" => vec![
                (
                    "autopilot-review-quality",
                    "quality-reviewer",
                    "Review maintainability and structural quality before declaring Autopilot complete.",
                ),
                (
                    "autopilot-review-api",
                    "api-reviewer",
                    "Review caller-facing interfaces and compatibility impacts before declaring Autopilot complete.",
                ),
                (
                    "autopilot-review-code",
                    "code-reviewer",
                    "Review logic correctness and maintainability before declaring Autopilot complete.",
                ),
                (
                    "autopilot-review-security",
                    "security-reviewer",
                    "Review trust boundaries, secret handling, and injection risks before declaring Autopilot complete.",
                ),
                (
                    "autopilot-review-critic",
                    "critic",
                    "Challenge the completion claim and surface the strongest remaining objection, if any.",
                ),
            ],
            "team" => vec![
                (
                    "team-review-quality",
                    "quality-reviewer",
                    "Review the combined Team output for maintainability, consistency, and hidden quality risks.",
                ),
                (
                    "team-review-api",
                    "api-reviewer",
                    "Review whether Team output changes any public or internal contracts in unsafe ways.",
                ),
                (
                    "team-review-code",
                    "code-reviewer",
                    "Review the Team result for correctness and design quality.",
                ),
                (
                    "team-review-security",
                    "security-reviewer",
                    "Review the Team result for auth, trust boundary, and injection risks.",
                ),
                (
                    "team-review-critic",
                    "critic",
                    "Challenge whether the Team result is truly complete and point out the strongest remaining objection.",
                ),
            ],
            "ralph" => vec![
                (
                    "ralph-review-quality",
                    "quality-reviewer",
                    "Review the current Ralph result for maintainability, consistency, and long-term quality.",
                ),
                (
                    "ralph-review-code",
                    "code-reviewer",
                    "Review the current Ralph result for correctness and maintainability.",
                ),
                (
                    "ralph-review-critic",
                    "critic",
                    "Challenge whether the current Ralph result is truly complete and surface the highest-risk remaining issue.",
                ),
            ],
            _ => vec![],
        };

        let base_context = format!(
            "Original goal: {}\nMode: {}\nReview outputs from the verification anchor and focus only on the reviewer perspective assigned below.",
            request.user_request, mode
        );
        for (id, agent, extra) in reviewers {
            plan.add_subtask(
                planner::SubTask::new(id, extra)
                    .with_agent(agent)
                    .with_dependencies(anchor_ids.clone())
                    .with_context(format!("{}\n\n{}", base_context, extra))
                    .with_timeout(300),
            );
        }

        plan
    }

    fn review_anchor_ids(&self, plan: &TaskPlan) -> Vec<String> {
        let mut anchors: Vec<String> = plan
            .subtasks
            .iter()
            .filter(|t| {
                t.agent_type == "verification"
                    || t.id.contains("verify")
                    || t.id.contains("validation")
            })
            .map(|t| t.id.clone())
            .collect();
        if anchors.is_empty() {
            if let Some(last) = plan.subtasks.last() {
                anchors.push(last.id.clone());
            }
        }
        anchors
    }

    fn reviewer_agents(&self, selected_agents: &[String]) -> Vec<String> {
        let mut reviewers: Vec<String> = selected_agents
            .iter()
            .filter(|agent| {
                matches!(
                    agent.as_str(),
                    "verification"
                        | "code-reviewer"
                        | "security-reviewer"
                        | "performance-reviewer"
                        | "quality-reviewer"
                        | "api-reviewer"
                        | "critic"
                        | "test-engineer"
                )
            })
            .cloned()
            .collect();
        reviewers.sort();
        reviewers.dedup();
        reviewers
    }
}

impl Default for AgentScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachs_reviewer_subtasks_for_autopilot_mode() {
        let scheduler = AgentScheduler::new();
        let mut plan = TaskPlan::new("ship feature");
        plan.add_subtask(
            planner::SubTask::new("verify", "verify result").with_agent("verification"),
        );
        let request = SchedulingRequest::new("ship feature")
            .with_project_root(".")
            .with_mode_hint("autopilot")
            .with_strategy(SchedulingStrategy::VerificationFirst);

        let plan = scheduler.attach_mode_specific_reviewers(plan, &request);
        assert!(plan
            .subtasks
            .iter()
            .any(|t| t.agent_type == "code-reviewer"));
        assert!(plan
            .subtasks
            .iter()
            .any(|t| t.agent_type == "security-reviewer"));
        assert!(plan.subtasks.iter().any(|t| t.agent_type == "critic"));
        let reviewers = scheduler.reviewer_agents(
            &plan
                .subtasks
                .iter()
                .map(|t| t.agent_type.clone())
                .collect::<Vec<_>>(),
        );
        assert!(reviewers.contains(&"critic".to_string()));
    }

    #[test]
    fn attachs_reviewer_subtasks_for_team_mode_after_team_verify() {
        let scheduler = AgentScheduler::new();
        let mut plan = TaskPlan::new("parallel task");
        plan.add_subtask(
            planner::SubTask::new(
                planner::TEAM_VERIFY_TASK_ID,
                planner::TEAM_VERIFY_DESCRIPTION,
            )
            .with_agent("verification"),
        );
        let request = SchedulingRequest::new("parallel task")
            .with_project_root(".")
            .with_mode_hint("team")
            .with_strategy(SchedulingStrategy::Team);

        let plan = scheduler.attach_mode_specific_reviewers(plan, &request);
        let team_reviews: Vec<_> = plan
            .subtasks
            .iter()
            .filter(|t| t.id.starts_with("team-review-"))
            .collect();
        assert!(!team_reviews.is_empty());
        assert!(team_reviews
            .iter()
            .all(|t| t.dependencies == vec![planner::TEAM_VERIFY_TASK_ID.to_string()]));
    }

    #[test]
    fn attachs_reviewer_subtasks_for_ralph_mode() {
        let scheduler = AgentScheduler::new();
        let mut plan = TaskPlan::new("ralph task");
        plan.add_subtask(
            planner::SubTask::new("verify-final", "verify final").with_agent("verification"),
        );
        let request = SchedulingRequest::new("ralph task")
            .with_project_root(".")
            .with_mode_hint("ralph")
            .with_strategy(SchedulingStrategy::VerificationFirst);

        let plan = scheduler.attach_mode_specific_reviewers(plan, &request);
        assert!(plan
            .subtasks
            .iter()
            .any(|t| t.agent_type == "quality-reviewer"));
        assert!(plan.subtasks.iter().any(|t| t.agent_type == "critic"));
    }
}
