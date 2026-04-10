//! 权限系统核心类型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 最大 TimeWindow 时长（分钟），防止溢出（7天）
pub const MAX_TIME_WINDOW_MINUTES: u32 = 10_080;

/// 用户规则最低优先级（低于此值的优先级不允许用户设置，留给系统规则）
pub const MIN_USER_RULE_PRIORITY: i32 = -100;

/// 权限模式
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PermissionMode {
    /// 每次调用都询问
    AskEveryTime,
    /// 同一会话内相同操作只问一次
    Session,
    /// 时间窗口内自动批准
    TimeWindow { minutes: u32 },
    /// Plan 模式 - 批量确认
    Plan,
    /// 自动批准安全操作
    Auto,
    /// 完全绕过（危险！仅供系统规则使用，不允许从前端设置）
    Bypass,
}

impl PermissionMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AskEveryTime => "每次询问",
            Self::Session => "本次会话",
            Self::TimeWindow { minutes: 60 } => "1小时内",
            Self::TimeWindow { minutes: 240 } => "4小时内",
            Self::TimeWindow { minutes: 1440 } => "24小时内",
            Self::TimeWindow { .. } => "自定义",
            Self::Plan => "Plan模式",
            Self::Auto => "自动批准",
            Self::Bypass => "完全绕过",
        }
    }

    /// 校验用户提交的模式是否合法（不允许 Bypass）
    pub fn validate_user_mode(&self) -> Result<(), String> {
        match self {
            Self::Bypass => Err("Bypass 模式不允许通过前端设置".to_string()),
            Self::TimeWindow { minutes } if *minutes > MAX_TIME_WINDOW_MINUTES => Err(format!(
                "TimeWindow 最大为 {} 分钟（7天），收到: {}",
                MAX_TIME_WINDOW_MINUTES, minutes
            )),
            Self::TimeWindow { minutes: 0 } => Err("TimeWindow 不能为 0 分钟".to_string()),
            _ => Ok(()),
        }
    }
}

/// 工具匹配器
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "pattern")]
pub enum ToolMatcher {
    Exact(String),
    Wildcard(String),
    Regex(String),
    Any,
}

/// 路径匹配器
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "pattern")]
pub enum PathMatcher {
    Exact(String),
    Prefix(String),
    Glob(String),
    Regex(String),
    WithinProject,
    FileExtension(Vec<String>),
}

/// 参数条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgumentCondition {
    pub key: String,
    pub operator: ConditionOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum ConditionOperator {
    Eq,
    Ne,
    Contains,
    StartsWith,
    Matches,
    In,
}

/// 规则有效期
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleValidity {
    Permanent,
    Until(DateTime<Utc>),
    UseLimit(u64),
    /// 仅对创建该规则时所在的会话有效
    CurrentSession { session_id: String },
}

/// 权限规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tool_matcher: ToolMatcher,
    pub path_matcher: Option<PathMatcher>,
    pub argument_conditions: Vec<ArgumentCondition>,
    pub mode: PermissionMode,
    pub validity: RuleValidity,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: u64,
}

impl PermissionRule {
    /// 校验用户提交的规则是否合法
    pub fn validate_user_rule(&self) -> Result<(), String> {
        self.mode.validate_user_mode()?;

        if self.priority < MIN_USER_RULE_PRIORITY {
            return Err(format!(
                "规则优先级不能低于 {}，收到: {}",
                MIN_USER_RULE_PRIORITY, self.priority
            ));
        }

        // 校验正则表达式模式
        self.validate_tool_matcher_regex()?;

        Ok(())
    }

    fn validate_tool_matcher_regex(&self) -> Result<(), String> {
        match &self.tool_matcher {
            ToolMatcher::Wildcard(pattern) => {
                if pattern.len() > 256 {
                    return Err("Wildcard 模式长度不能超过 256 字符".to_string());
                }
                let regex_str = format!(
                    "^{}$",
                    regex::escape(pattern).replace(r"\*", ".*").replace(r"\?", ".")
                );
                regex::RegexBuilder::new(&regex_str)
                    .size_limit(1_000_000)
                    .dfa_size_limit(1_000_000)
                    .build()
                    .map_err(|e| format!("Wildcard 模式无效: {}", e))?;
            }
            ToolMatcher::Regex(pattern) => {
                if pattern.len() > 256 {
                    return Err("Regex 模式长度不能超过 256 字符".to_string());
                }
                regex::RegexBuilder::new(pattern)
                    .size_limit(1_000_000)
                    .dfa_size_limit(1_000_000)
                    .build()
                    .map_err(|e| format!("Regex 模式无效: {}", e))?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// 风险等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    Safe = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl RiskLevel {
    pub fn color(&self) -> &'static str {
        match self {
            Self::Safe => "#4caf50",
            Self::Low => "#8bc34a",
            Self::Medium => "#ff9800",
            Self::High => "#f44336",
            Self::Critical => "#b71c1c",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Safe => "✓",
            Self::Low => "ℹ",
            Self::Medium => "⚠",
            Self::High => "⚠",
            Self::Critical => "☠",
        }
    }

    pub fn needs_confirmation(&self) -> bool {
        *self >= Self::Medium
    }
}

/// 风险类别
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskCategory {
    FileSystem,
    System,
    Network,
    DataLoss,
    Security,
    Privacy,
}

/// 检测到的风险
#[derive(Debug, Clone)]
pub struct DetectedRisk {
    pub category: RiskCategory,
    pub severity: RiskLevel,
    pub description: String,
    pub mitigation: Option<String>,
}

/// 风险评估结果
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub categories: Vec<RiskCategory>,
    pub description: String,
    pub recommendations: Vec<String>,
    pub detected_risks: Vec<DetectedRisk>,
}

/// 权限检查上下文
#[derive(Debug, Clone)]
pub struct PermissionContext {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub session_id: String,
    pub file_paths: Option<Vec<std::path::PathBuf>>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// 权限请求
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// 唯一请求 ID，供前端关联批准/拒绝操作
    pub request_id: String,
    pub context: PermissionContext,
    pub risk: RiskAssessment,
    pub suggested_mode: PermissionMode,
}

/// 前端友好的权限模式输入（避免序列化问题）
#[derive(Debug, Clone, serde::Deserialize)]
pub enum PermissionModeInput {
    AskEveryTime,
    Session,
    TimeWindow { minutes: u32 },
    Plan,
    Auto,
}

impl From<PermissionModeInput> for PermissionMode {
    fn from(input: PermissionModeInput) -> Self {
        match input {
            PermissionModeInput::AskEveryTime => PermissionMode::AskEveryTime,
            PermissionModeInput::Session => PermissionMode::Session,
            PermissionModeInput::TimeWindow { minutes } => PermissionMode::TimeWindow { minutes },
            PermissionModeInput::Plan => PermissionMode::Plan,
            PermissionModeInput::Auto => PermissionMode::Auto,
        }
    }
}

/// 权限决定
#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
    RequireApproval(PermissionRequest),
}

/// 拒绝记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenialRecord {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub reason: String,
    pub session_id: String,
}
