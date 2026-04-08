//! Agent 调度系统
//!
//! 提供自动 Agent 选择、任务分解和多 Agent 编排功能。

pub mod selector;
pub mod planner;
pub mod orchestrator;
pub mod strategy;

pub use selector::{AgentSelector, select_agent_for_task};
pub use planner::{TaskPlanner, TaskPlan, SubTask};
pub use orchestrator::{AgentOrchestrator, OrchestrationResult};
pub use strategy::{SchedulingStrategy, StrategyConfig};

use serde::{Deserialize, Serialize};

/// 调度请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingRequest {
    /// 用户原始请求
    pub user_request: String,
    /// 任务描述
    pub description: String,
    /// 项目根目录
    pub project_root: String,
    /// 调度策略
    pub strategy: SchedulingStrategy,
    /// 是否允许并行执行
    pub allow_parallel: bool,
    /// 最大 Agent 数量
    pub max_agents: usize,
    /// 是否自动分解任务
    pub auto_decompose: bool,
}

impl SchedulingRequest {
    pub fn new(user_request: impl Into<String>) -> Self {
        Self {
            user_request: user_request.into(),
            description: String::new(),
            project_root: ".".to_string(),
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
    /// 执行计划
    pub plan: TaskPlan,
    /// 选中的 Agent 列表
    pub selected_agents: Vec<String>,
    /// 预估执行时间（秒）
    pub estimated_duration_secs: u64,
    /// 是否需要人工确认
    pub requires_confirmation: bool,
    /// 确认提示信息
    pub confirmation_message: Option<String>,
}

/// 调度器主体
pub struct AgentScheduler {
    selector: AgentSelector,
    planner: TaskPlanner,
    orchestrator: AgentOrchestrator,
    strategy: StrategyConfig,
}

impl AgentScheduler {
    pub fn new() -> Self {
        Self {
            selector: AgentSelector::new(),
            planner: TaskPlanner::new(),
            orchestrator: AgentOrchestrator::new(),
            strategy: StrategyConfig::default(),
        }
    }

    /// 执行完整调度流程
    pub async fn schedule(&self, request: SchedulingRequest) -> Result<SchedulingResult, String> {
        // 使用配置的策略（如果请求是 Auto）
        let _effective_strategy = if request.strategy == SchedulingStrategy::Auto {
            self.strategy.default_strategy
        } else {
            request.strategy
        };
        
        // 注意：_effective_strategy 可用于后续策略特定的逻辑

        // 1. 分析请求，确定是否需要分解
        let needs_decomposition = request.auto_decompose 
            && self.planner.should_decompose(&request.user_request);

        if needs_decomposition {
            // 2. 分解任务
            let plan = self.planner.decompose(&request).await?;
            
            // 3. 为每个子任务选择 Agent
            let mut selected_agents = Vec::new();
            for subtask in &plan.subtasks {
                let agent = self.selector.select(&subtask.description, &request.project_root);
                selected_agents.push(agent.to_string());
            }

            // 4. 估算执行时间
            let estimated = self.estimate_duration(&plan, &selected_agents);

            // 5. 检查是否需要确认
            let requires_confirmation = selected_agents.len() > 3 
                || plan.subtasks.iter().any(|t| t.critical);

            let confirmation_message = if requires_confirmation {
                Some(self.generate_confirmation_message(&plan, &selected_agents))
            } else {
                None
            };

            Ok(SchedulingResult {
                plan,
                selected_agents,
                estimated_duration_secs: estimated,
                requires_confirmation,
                confirmation_message,
            })
        } else {
            // 单 Agent 执行
            let agent = self.selector.select(&request.user_request, &request.project_root);
            let plan = TaskPlan::single_agent(&agent, &request.user_request);
            
            Ok(SchedulingResult {
                plan,
                selected_agents: vec![agent.to_string()],
                estimated_duration_secs: 60, // 默认 60 秒
                requires_confirmation: false,
                confirmation_message: None,
            })
        }
    }

    /// 执行任务计划
    pub async fn execute_plan(
        &self,
        plan: &TaskPlan,
        request: &SchedulingRequest,
        app: &tauri::AppHandle,
    ) -> Result<OrchestrationResult, String> {
        self.orchestrator.execute(plan, request, app).await
    }

    fn estimate_duration(&self, plan: &TaskPlan, agents: &[String]) -> u64 {
        let base_time = match plan.subtasks.len() {
            0 => 30,
            1 => 60,
            2..=3 => 120,
            4..=5 => 180,
            _ => 300,
        };

        // 根据 Agent 类型调整
        let agent_multiplier: f64 = agents.iter().map(|a| match a.as_str() {
            "Explore" => 0.8,
            "Plan" => 1.5,
            "verification" => 2.0,
            _ => 1.0,
        }).sum::<f64>() / agents.len().max(1) as f64;

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
}

impl Default for AgentScheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// 快速调度入口
pub async fn auto_schedule(
    user_request: impl Into<String>,
    project_root: impl Into<String>,
) -> Result<SchedulingResult, String> {
    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(user_request)
        .with_project_root(project_root);
    
    scheduler.schedule(request).await
}

/// 使用特定策略调度
pub async fn schedule_with_strategy(
    user_request: impl Into<String>,
    project_root: impl Into<String>,
    strategy: SchedulingStrategy,
) -> Result<SchedulingResult, String> {
    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(user_request)
        .with_project_root(project_root)
        .with_strategy(strategy);
    
    scheduler.schedule(request).await
}
