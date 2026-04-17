//! 调度策略
//!
//! 定义不同的 Agent 调度策略。

use serde::{Deserialize, Serialize};

/// 调度策略
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SchedulingStrategy {
    /// 自动选择最佳策略
    Auto,
    /// 单 Agent 执行（简单任务）
    Single,
    /// 顺序执行（依赖任务）
    Sequential,
    /// 并行执行（独立任务）
    Parallel,
    /// 分阶段执行（探索→设计→实现→验证）
    Phased,
    /// 竞争执行（多个 Agent 同时处理，取最佳结果）
    Competitive,
    /// 验证优先（先验证后执行）
    VerificationFirst,
}

impl SchedulingStrategy {
    /// 获取策略名称
    pub fn name(&self) -> &'static str {
        match self {
            SchedulingStrategy::Auto => "自动",
            SchedulingStrategy::Single => "单 Agent",
            SchedulingStrategy::Sequential => "顺序执行",
            SchedulingStrategy::Parallel => "并行执行",
            SchedulingStrategy::Phased => "分阶段",
            SchedulingStrategy::Competitive => "竞争执行",
            SchedulingStrategy::VerificationFirst => "验证优先",
        }
    }

    /// 获取策略描述
    pub fn description(&self) -> &'static str {
        match self {
            SchedulingStrategy::Auto => "根据任务复杂度自动选择最佳执行策略",
            SchedulingStrategy::Single => "使用单个 Agent 完成任务，适用于简单直接的请求",
            SchedulingStrategy::Sequential => "按顺序执行多个 Agent，每个 Agent 依赖前一个的结果",
            SchedulingStrategy::Parallel => "同时启动多个 Agent，各自处理不同方面，最后合并结果",
            SchedulingStrategy::Phased => "分阶段执行：探索→设计→实现→验证，适用于复杂功能开发",
            SchedulingStrategy::Competitive => "多个 Agent 同时解决同一问题，选择最佳结果",
            SchedulingStrategy::VerificationFirst => "先验证现有代码，再进行修改，适用于重构和优化",
        }
    }

    /// 是否允许并行
    pub fn allows_parallel(&self) -> bool {
        matches!(
            self,
            SchedulingStrategy::Auto
                | SchedulingStrategy::Parallel
                | SchedulingStrategy::Competitive
        )
    }

    /// 是否需要分解
    pub fn requires_decomposition(&self) -> bool {
        matches!(
            self,
            SchedulingStrategy::Phased
                | SchedulingStrategy::Sequential
                | SchedulingStrategy::Parallel
        )
    }

    /// 默认 Agent 数量限制
    pub fn default_max_agents(&self) -> usize {
        match self {
            SchedulingStrategy::Auto => 5,
            SchedulingStrategy::Single => 1,
            SchedulingStrategy::Sequential => 4,
            SchedulingStrategy::Parallel => 6,
            SchedulingStrategy::Phased => 4,
            SchedulingStrategy::Competitive => 3,
            SchedulingStrategy::VerificationFirst => 4,
        }
    }
}

impl Default for SchedulingStrategy {
    fn default() -> Self {
        SchedulingStrategy::Auto
    }
}

/// 策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// 默认策略
    pub default_strategy: SchedulingStrategy,
    /// 复杂度阈值（超过则使用多 Agent）
    pub complexity_threshold: u8,
    /// 最大并行 Agent 数
    pub max_parallel_agents: usize,
    /// 是否自动降级（复杂策略失败时降级为简单策略）
    pub auto_fallback: bool,
    /// 是否启用竞争策略
    pub enable_competitive: bool,
    /// 超时设置（秒）
    pub timeout_secs: u64,
}

impl StrategyConfig {
    pub fn new() -> Self {
        Self {
            default_strategy: SchedulingStrategy::Auto,
            complexity_threshold: 5,
            max_parallel_agents: 5,
            auto_fallback: true,
            enable_competitive: false,
            timeout_secs: 300,
        }
    }

    /// 根据任务特征选择策略
    pub fn select_strategy(&self, task_complexity: u8, _task_type: &str) -> SchedulingStrategy {
        if task_complexity < self.complexity_threshold {
            SchedulingStrategy::Single
        } else if task_complexity < 8 {
            SchedulingStrategy::Sequential
        } else {
            self.default_strategy
        }
    }

    /// 启用竞争策略
    pub fn with_competitive(mut self) -> Self {
        self.enable_competitive = true;
        self
    }

    /// 设置并行限制
    pub fn with_max_parallel(mut self, max: usize) -> Self {
        self.max_parallel_agents = max;
        self
    }

    /// 设置超时
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// 任务复杂度评估器
pub struct ComplexityEvaluator;

impl ComplexityEvaluator {
    /// 评估任务复杂度（0-10）
    pub fn evaluate(task_description: &str) -> u8 {
        let mut score = 0;
        let lower = task_description.to_lowercase();

        // 长度因素
        let len = task_description.len();
        if len > 100 {
            score += 1;
        }
        if len > 300 {
            score += 1;
        }
        if len > 600 {
            score += 1;
        }

        // 关键词因素
        let complexity_indicators = [
            ("implement", 2),
            ("design", 2),
            ("architecture", 3),
            ("refactor", 2),
            ("multiple", 1),
            ("complex", 2),
            ("integrate", 2),
            ("optimize", 2),
            ("and then", 2),
            ("after", 1),
            ("before", 1),
            // 中文
            ("实现", 2),
            ("设计", 2),
            ("架构", 3),
            ("重构", 2),
            ("多个", 1),
            ("复杂", 2),
            ("集成", 2),
            ("优化", 2),
            ("然后", 2),
            ("之后", 1),
            ("之前", 1),
        ];

        for (indicator, points) in &complexity_indicators {
            if lower.contains(indicator) {
                score += points;
            }
        }

        // 标点符号（列表项越多越复杂）
        let bullet_count = lower.matches("\n-").count() + lower.matches("\n*").count();
        score += (bullet_count as u8).min(3);

        score.min(10)
    }

    /// 获取复杂度描述
    pub fn complexity_description(score: u8) -> &'static str {
        match score {
            0..=2 => "简单",
            3..=5 => "中等",
            6..=8 => "复杂",
            9..=10 => "非常复杂",
            _ => "未知",
        }
    }

    /// 获取推荐策略
    pub fn recommend_strategy(score: u8) -> SchedulingStrategy {
        match score {
            0..=2 => SchedulingStrategy::Single,
            3..=4 => SchedulingStrategy::Sequential,
            5..=6 => SchedulingStrategy::Phased,
            7..=8 => SchedulingStrategy::Parallel,
            9..=10 => SchedulingStrategy::Competitive,
            _ => SchedulingStrategy::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_selection() {
        let config = StrategyConfig::new();

        assert_eq!(
            config.select_strategy(3, "test"),
            SchedulingStrategy::Single
        );

        assert_eq!(
            config.select_strategy(6, "test"),
            SchedulingStrategy::Sequential
        );
    }

    #[test]
    fn test_complexity_evaluation() {
        let simple = "Find all files";
        assert!(ComplexityEvaluator::evaluate(simple) <= 3);

        let complex = "Design and implement a new authentication system with multiple providers and then optimize the performance";
        assert!(ComplexityEvaluator::evaluate(complex) >= 6);
    }

    #[test]
    fn test_strategy_names() {
        assert_eq!(SchedulingStrategy::Auto.name(), "自动");
        assert_eq!(SchedulingStrategy::Phased.name(), "分阶段");
    }
}
