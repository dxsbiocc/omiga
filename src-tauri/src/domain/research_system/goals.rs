//! Persistent research goals layered on top of the Research System.
//!
//! This module intentionally mirrors the *semantics* of Codex thread goals
//! (persistent objective, status lifecycle, continuation, completion audit)
//! without importing Codex app-server protocol/state machinery.

use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk, TokenUsage as LlmTokenUsage};
use chrono::Utc;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const DEFAULT_MAX_CYCLES: u32 = 3;
const MAX_GOAL_CYCLES: u32 = 20;
const MAX_AUTO_RUN_CYCLES: u32 = 10;
const DEFAULT_AUTO_RUN_IDLE_DELAY_MS: u64 = 650;
const MIN_AUTO_RUN_IDLE_DELAY_MS: u64 = 250;
const MAX_AUTO_RUN_IDLE_DELAY_MS: u64 = 60_000;
const MAX_AUTO_RUN_ELAPSED_MINUTES: u32 = 24 * 60;
const MAX_AUTO_RUN_TOKENS: u64 = 100_000_000;
const GOAL_AUDIT_TIMEOUT_SECS: u64 = 45;
const GOAL_PROVIDER_PROBE_TIMEOUT_SECS: u64 = 20;
const MAX_AUDIT_PAYLOAD_CHARS: usize = 18_000;
const MAX_AUDIT_FIELD_CHARS: usize = 900;
const MAX_AUDIT_ARRAY_ITEMS: usize = 12;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchGoalStatus {
    Active,
    Paused,
    BudgetLimited,
    Complete,
}

impl ResearchGoalStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::BudgetLimited => "budget_limited",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CriteriaAudit {
    #[serde(default)]
    pub criterion_id: String,
    pub criterion: String,
    pub covered: bool,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalCriterion {
    pub criterion_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalAudit {
    pub complete: bool,
    #[serde(default = "default_audit_review_source")]
    pub review_source: String,
    #[serde(default = "default_audit_confidence")]
    pub confidence: String,
    #[serde(default)]
    pub final_report_ready: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub criteria: Vec<CriteriaAudit>,
    #[serde(default)]
    pub missing_requirements: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub conflicting_evidence: Vec<String>,
    #[serde(default)]
    pub second_opinion: Option<ResearchGoalSecondOpinion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalSecondOpinion {
    #[serde(default = "default_audit_review_source")]
    pub review_source: String,
    pub agrees_complete: bool,
    #[serde(default = "default_audit_confidence")]
    pub confidence: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub blocking_concerns: Vec<String>,
    #[serde(default)]
    pub required_next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoal {
    pub goal_id: String,
    pub session_id: String,
    pub objective: String,
    pub status: ResearchGoalStatus,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub success_criterion_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_opinion_provider_entry: Option<String>,
    #[serde(default)]
    pub auto_run_policy: ResearchGoalAutoRunPolicy,
    #[serde(default)]
    pub token_usage: ResearchGoalTokenUsage,
    pub max_cycles: u32,
    pub current_cycle: u32,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub last_audit: Option<ResearchGoalAudit>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub last_run_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalAutoRunPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_auto_run_cycles_per_run")]
    pub cycles_per_run: u32,
    #[serde(default = "default_auto_run_idle_delay_ms")]
    pub idle_delay_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_elapsed_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
}

impl Default for ResearchGoalAutoRunPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            cycles_per_run: default_auto_run_cycles_per_run(),
            idle_delay_ms: default_auto_run_idle_delay_ms(),
            max_elapsed_minutes: None,
            max_tokens: None,
            started_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalAutoRunPolicyUpdate {
    pub enabled: bool,
    pub cycles_per_run: u32,
    pub idle_delay_ms: u64,
    #[serde(default)]
    pub max_elapsed_minutes: Option<u32>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalTokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

impl ResearchGoalTokenUsage {
    fn from_llm_usage(usage: &LlmTokenUsage) -> Self {
        let input_tokens = u64::from(usage.prompt_tokens);
        let output_tokens = u64::from(usage.completion_tokens);
        let total_tokens = if usage.total_tokens > 0 {
            u64::from(usage.total_tokens)
        } else {
            input_tokens.saturating_add(output_tokens)
        };
        Self {
            input_tokens,
            output_tokens,
            total_tokens,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            total_tokens: self.total_tokens.saturating_add(other.total_tokens),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalCycleTokenUsage {
    #[serde(default)]
    pub research_system: ResearchGoalTokenUsage,
    #[serde(default)]
    pub audit: ResearchGoalTokenUsage,
    #[serde(default)]
    pub total: ResearchGoalTokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalCycle {
    pub cycle_id: String,
    pub goal_id: String,
    pub cycle_index: u32,
    pub request: String,
    pub graph_id: Option<String>,
    pub research_status: Option<String>,
    pub audit: ResearchGoalAudit,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub token_usage: ResearchGoalCycleTokenUsage,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGoalCommandResult {
    pub goal: Option<ResearchGoal>,
    #[serde(default)]
    pub cycle: Option<ResearchGoalCycle>,
    pub assistant_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedResearchGoalCommand {
    Help,
    Status,
    Set {
        objective: String,
        max_cycles: Option<u32>,
    },
    Run {
        auto_cycles: u32,
    },
    Budget {
        max_cycles: u32,
    },
    Pause,
    Resume,
    Clear,
}

pub fn parse_research_goal_body(body: &str) -> ParsedResearchGoalCommand {
    let trimmed = body.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("status") {
        return ParsedResearchGoalCommand::Status;
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "help" | "--help" | "-h" => ParsedResearchGoalCommand::Help,
        "pause" | "暂停" => ParsedResearchGoalCommand::Pause,
        "resume" | "恢复" => ParsedResearchGoalCommand::Resume,
        "clear" | "remove" | "清除" => ParsedResearchGoalCommand::Clear,
        _ => parse_run_or_budget_command(trimmed).unwrap_or_else(|| parse_set_command(trimmed)),
    }
}

pub fn run_research_goal_command(
    workspace_root: &Path,
    session_id: &str,
    body: &str,
) -> Result<ResearchGoalCommandResult, String> {
    let layout = ResearchGoalLayout::new(workspace_root);
    let command = parse_research_goal_body(body);

    match command {
        ParsedResearchGoalCommand::Help => Ok(ResearchGoalCommandResult {
            goal: load_goal_if_exists(&layout, session_id)?,
            cycle: None,
            assistant_content: goal_help_text(),
        }),
        ParsedResearchGoalCommand::Status => {
            let goal = load_goal_if_exists(&layout, session_id)?;
            let assistant_content = match goal.as_ref() {
                Some(goal) => format_goal_status(goal),
                None => "当前会话还没有科研目标。\n\n使用 `/goal <科研目标>` 创建一个长期目标。"
                    .to_string(),
            };
            Ok(ResearchGoalCommandResult {
                goal,
                cycle: None,
                assistant_content,
            })
        }
        ParsedResearchGoalCommand::Set {
            objective,
            max_cycles,
        } => {
            let goal = set_goal(&layout, session_id, &objective, max_cycles)?;
            Ok(ResearchGoalCommandResult {
                assistant_content: format!(
                    "已设置科研目标。\n\n{}\n\n下一步：使用 `/goal run` 进行一轮“分析 → 解读 → 再分析”的目标推进。",
                    format_goal_status(&goal)
                ),
                goal: Some(goal),
                cycle: None,
            })
        }
        ParsedResearchGoalCommand::Run { .. } => Err(
            "/goal run 需要 LLM 完成审计，不能使用启发式判定；请通过应用命令入口运行。".to_string(),
        ),
        ParsedResearchGoalCommand::Budget { max_cycles } => {
            let goal = update_research_goal_settings(
                workspace_root,
                session_id,
                ResearchGoalSettingsUpdate {
                    criteria: None,
                    max_cycles: Some(max_cycles),
                    second_opinion_provider_entry: None,
                    auto_run_policy: None,
                },
            )?;
            Ok(ResearchGoalCommandResult {
                assistant_content: format!(
                    "已更新科研目标轮次预算。\n\n{}\n\n可使用 `/goal run --cycles {}` 在一次命令中自动续跑多轮。",
                    format_goal_status(&goal),
                    max_cycles.saturating_sub(goal.current_cycle).clamp(1, MAX_AUTO_RUN_CYCLES),
                ),
                goal: Some(goal),
                cycle: None,
            })
        }
        ParsedResearchGoalCommand::Pause => update_goal_status(
            &layout,
            session_id,
            ResearchGoalStatus::Paused,
            "科研目标已暂停。使用 `/goal resume` 恢复。",
        ),
        ParsedResearchGoalCommand::Resume => update_goal_status(
            &layout,
            session_id,
            ResearchGoalStatus::Active,
            "科研目标已恢复。使用 `/goal run` 继续推进。",
        ),
        ParsedResearchGoalCommand::Clear => clear_goal(&layout, session_id),
    }
}

pub async fn run_research_goal_command_with_llm(
    workspace_root: &Path,
    session_id: &str,
    body: &str,
    audit_client: &dyn LlmClient,
    second_opinion_client: Option<&dyn LlmClient>,
) -> Result<ResearchGoalCommandResult, String> {
    let layout = ResearchGoalLayout::new(workspace_root);
    let command = parse_research_goal_body(body);

    match command {
        ParsedResearchGoalCommand::Run { auto_cycles } => {
            continue_goal_with_llm(
                &layout,
                session_id,
                auto_cycles,
                audit_client,
                second_opinion_client,
            )
            .await
        }
        _ => run_research_goal_command(workspace_root, session_id, body),
    }
}

pub async fn suggest_research_goal_criteria_with_llm(
    audit_client: &dyn LlmClient,
    goal: &ResearchGoal,
) -> Result<Vec<String>, String> {
    let messages = build_goal_criteria_suggestion_messages(goal);
    let response = tokio::time::timeout(
        Duration::from_secs(GOAL_AUDIT_TIMEOUT_SECS),
        collect_llm_text_and_usage(audit_client, messages),
    )
    .await
    .map_err(|_| "LLM 成功标准生成超时，请稍后重试。".to_string())??;

    parse_llm_research_goal_criteria(&response.text)
}

pub async fn probe_research_goal_second_opinion_provider_with_llm(
    client: &dyn LlmClient,
) -> Result<String, String> {
    let messages = build_goal_second_opinion_probe_messages();
    let response = tokio::time::timeout(
        Duration::from_secs(GOAL_PROVIDER_PROBE_TIMEOUT_SECS),
        collect_llm_text_and_usage(client, messages),
    )
    .await
    .map_err(|_| "LLM 二审 provider 真实调用超时，请检查网络、额度或模型名称。".to_string())??;

    parse_llm_second_opinion_provider_probe(&response.text)
}

pub fn read_research_goal(
    workspace_root: &Path,
    session_id: &str,
) -> Result<Option<ResearchGoal>, String> {
    let layout = ResearchGoalLayout::new(workspace_root);
    load_goal_if_exists(&layout, session_id)
}

#[derive(Debug, Clone, Default)]
pub struct ResearchGoalSettingsUpdate {
    pub criteria: Option<Vec<String>>,
    pub max_cycles: Option<u32>,
    pub second_opinion_provider_entry: Option<String>,
    pub auto_run_policy: Option<ResearchGoalAutoRunPolicyUpdate>,
}

pub fn update_research_goal_settings(
    workspace_root: &Path,
    session_id: &str,
    update: ResearchGoalSettingsUpdate,
) -> Result<ResearchGoal, String> {
    let layout = ResearchGoalLayout::new(workspace_root);
    layout.ensure_dirs()?;
    let mut goal = load_goal(&layout, session_id)?;
    let mut criteria_changed = false;
    let mut budget_changed = false;
    let mut second_opinion_changed = false;
    let mut auto_run_policy_changed = false;
    let now = now_string();

    if let Some(criteria) = update.criteria {
        update_goal_success_criteria(&mut goal, criteria)?;
        criteria_changed = true;
    }

    if let Some(max_cycles) = update.max_cycles {
        let max_cycles = normalize_max_cycles(max_cycles)?;
        if max_cycles < goal.current_cycle {
            return Err(format!(
                "轮次预算不能小于当前已运行轮次（当前 {} 轮）。",
                goal.current_cycle
            ));
        }
        goal.max_cycles = max_cycles;
        budget_changed = true;
    }

    if let Some(entry) = update.second_opinion_provider_entry {
        let normalized = normalize_second_opinion_provider_entry(&entry);
        if goal.second_opinion_provider_entry != normalized {
            goal.second_opinion_provider_entry = normalized;
            second_opinion_changed = true;
        }
    }

    if let Some(auto_run_policy) = update.auto_run_policy {
        let normalized = normalize_auto_run_policy(auto_run_policy, &goal.auto_run_policy, &now)?;
        if goal.auto_run_policy != normalized {
            goal.auto_run_policy = normalized;
            auto_run_policy_changed = true;
        }
    }

    if !criteria_changed && !budget_changed && !second_opinion_changed && !auto_run_policy_changed {
        return Ok(goal);
    }

    if criteria_changed || second_opinion_changed {
        goal.last_audit = None;
    }
    goal.updated_at = now.clone();
    goal.notes.push(format!(
        "Goal settings updated at {now}: criteria_changed={criteria_changed}, budget_changed={budget_changed}, second_opinion_changed={second_opinion_changed}, auto_run_policy_changed={auto_run_policy_changed}."
    ));
    if (criteria_changed || second_opinion_changed) && goal.status == ResearchGoalStatus::Complete {
        goal.status = ResearchGoalStatus::Active;
    }
    if goal.status != ResearchGoalStatus::Complete {
        if goal.current_cycle >= goal.max_cycles {
            goal.status = ResearchGoalStatus::BudgetLimited;
        } else if budget_changed && goal.status == ResearchGoalStatus::BudgetLimited {
            goal.status = ResearchGoalStatus::Active;
        }
    }

    save_goal(&layout, &goal)?;
    Ok(goal)
}

pub fn update_research_goal_criteria(
    workspace_root: &Path,
    session_id: &str,
    criteria: Vec<String>,
) -> Result<ResearchGoal, String> {
    update_research_goal_settings(
        workspace_root,
        session_id,
        ResearchGoalSettingsUpdate {
            criteria: Some(criteria),
            max_cycles: None,
            second_opinion_provider_entry: None,
            auto_run_policy: None,
        },
    )
}

fn parse_run_or_budget_command(trimmed: &str) -> Option<ParsedResearchGoalCommand> {
    let lower = trimmed.to_ascii_lowercase();
    let command = lower.split_whitespace().next()?;
    let rest = lower[command.len()..].trim();

    match command {
        "run" | "continue" | "next" | "推进" | "继续" => {
            let auto_cycles = parse_cycle_count(rest).unwrap_or(1);
            Some(ParsedResearchGoalCommand::Run {
                auto_cycles: auto_cycles.clamp(1, MAX_AUTO_RUN_CYCLES),
            })
        }
        "auto" | "autorun" | "run-auto" | "run_all" => {
            let auto_cycles = parse_cycle_count(rest).unwrap_or(MAX_AUTO_RUN_CYCLES);
            Some(ParsedResearchGoalCommand::Run {
                auto_cycles: auto_cycles.clamp(1, MAX_AUTO_RUN_CYCLES),
            })
        }
        "budget" | "max-cycles" | "max_cycles" | "cycles" | "预算" | "轮次" => {
            let max_cycles = parse_cycle_count(rest)?.clamp(1, MAX_GOAL_CYCLES);
            Some(ParsedResearchGoalCommand::Budget { max_cycles })
        }
        _ => None,
    }
}

fn parse_cycle_count(rest: &str) -> Option<u32> {
    let rest = rest.trim();
    let rest = rest
        .strip_prefix("--cycles")
        .or_else(|| rest.strip_prefix("--max-cycles"))
        .or_else(|| rest.strip_prefix("--auto"))
        .unwrap_or(rest)
        .trim_start_matches(['=', ' ', ':'])
        .trim();
    rest.split_whitespace().next()?.parse::<u32>().ok()
}

fn parse_set_command(trimmed: &str) -> ParsedResearchGoalCommand {
    let mut max_cycles = None;
    let mut rest = trimmed;

    if let Some(after_flag) = rest.strip_prefix("--max-cycles ") {
        let mut parts = after_flag.trim_start().splitn(2, char::is_whitespace);
        if let Some(raw) = parts.next() {
            max_cycles = raw.parse::<u32>().ok().filter(|value| *value > 0);
            rest = parts.next().unwrap_or_default().trim_start();
        }
    } else if let Some(after_flag) = rest.strip_prefix("--cycles ") {
        let mut parts = after_flag.trim_start().splitn(2, char::is_whitespace);
        if let Some(raw) = parts.next() {
            max_cycles = raw.parse::<u32>().ok().filter(|value| *value > 0);
            rest = parts.next().unwrap_or_default().trim_start();
        }
    }

    ParsedResearchGoalCommand::Set {
        objective: rest.trim().to_string(),
        max_cycles,
    }
}

fn set_goal(
    layout: &ResearchGoalLayout,
    session_id: &str,
    objective: &str,
    max_cycles: Option<u32>,
) -> Result<ResearchGoal, String> {
    let objective = objective.trim();
    if objective.is_empty() {
        return Err("科研目标不能为空".to_string());
    }

    layout.ensure_dirs()?;
    let now = now_string();
    let success_criteria = default_success_criteria();
    let success_criterion_ids = criterion_ids_for(&success_criteria, &BTreeMap::new());
    let goal = ResearchGoal {
        goal_id: format!("goal-{}", Uuid::new_v4()),
        session_id: session_id.to_string(),
        objective: objective.to_string(),
        status: ResearchGoalStatus::Active,
        success_criteria,
        success_criterion_ids,
        second_opinion_provider_entry: None,
        auto_run_policy: ResearchGoalAutoRunPolicy::default(),
        token_usage: ResearchGoalTokenUsage::default(),
        max_cycles: max_cycles
            .map(normalize_max_cycles)
            .transpose()?
            .unwrap_or(DEFAULT_MAX_CYCLES),
        current_cycle: 0,
        evidence_refs: Vec::new(),
        artifact_refs: Vec::new(),
        notes: vec!["Goal created from /goal command.".to_string()],
        last_audit: None,
        created_at: now.clone(),
        updated_at: now,
        last_run_at: None,
    };
    save_goal(layout, &goal)?;
    Ok(goal)
}

async fn continue_goal_with_llm(
    layout: &ResearchGoalLayout,
    session_id: &str,
    auto_cycles: u32,
    audit_client: &dyn LlmClient,
    second_opinion_client: Option<&dyn LlmClient>,
) -> Result<ResearchGoalCommandResult, String> {
    let requested_cycles = auto_cycles.clamp(1, MAX_AUTO_RUN_CYCLES);
    let mut cycles = Vec::new();
    let mut last_result = None;
    let mut auto_run_stop_reason = None;

    for _ in 0..requested_cycles {
        let goal_before_cycle = load_goal(layout, session_id)?;
        if let Some(reason) = auto_run_budget_reached_reason(&goal_before_cycle) {
            let goal = disable_auto_run_for_budget(layout, goal_before_cycle, &reason)?;
            auto_run_stop_reason = Some(reason.clone());
            last_result = Some(ResearchGoalCommandResult {
                assistant_content: format!(
                    "自动续跑预算已达到，未继续运行：{reason}\n\n{}",
                    format_goal_status(&goal)
                ),
                goal: Some(goal),
                cycle: None,
            });
            break;
        }

        let mut result =
            continue_goal_once_with_llm(layout, session_id, audit_client, second_opinion_client)
                .await?;
        let mut should_stop = result
            .goal
            .as_ref()
            .map_or(true, |goal| goal.status != ResearchGoalStatus::Active)
            || result.cycle.is_none();
        if let Some(goal) = result.goal.clone() {
            if let Some(reason) = auto_run_budget_reached_reason(&goal) {
                let goal = disable_auto_run_for_budget(layout, goal, &reason)?;
                result.goal = Some(goal);
                auto_run_stop_reason = Some(reason);
                should_stop = true;
            }
        }
        if let Some(cycle) = result.cycle.clone() {
            cycles.push(cycle);
        }
        last_result = Some(result);
        if should_stop {
            break;
        }
    }

    let mut result =
        last_result.ok_or_else(|| "未能执行科研目标推进，请检查轮次预算。".to_string())?;
    if requested_cycles > 1 && !cycles.is_empty() {
        if let Some(goal) = result.goal.as_ref() {
            result.assistant_content = format_goal_auto_run_result(goal, &cycles);
            if let Some(reason) = auto_run_stop_reason {
                result
                    .assistant_content
                    .push_str(&format!("\n\n自动续跑已停止：{reason}"));
            }
        }
    }
    Ok(result)
}

async fn continue_goal_once_with_llm(
    layout: &ResearchGoalLayout,
    session_id: &str,
    audit_client: &dyn LlmClient,
    second_opinion_client: Option<&dyn LlmClient>,
) -> Result<ResearchGoalCommandResult, String> {
    layout.ensure_dirs()?;
    let mut goal = load_goal(layout, session_id)?;

    match goal.status {
        ResearchGoalStatus::Paused => {
            return Ok(ResearchGoalCommandResult {
                assistant_content: "科研目标处于暂停状态。使用 `/goal resume` 后再运行。"
                    .to_string(),
                goal: Some(goal),
                cycle: None,
            });
        }
        ResearchGoalStatus::Complete => {
            return Ok(ResearchGoalCommandResult {
                assistant_content: format!("科研目标已经完成。\n\n{}", format_goal_status(&goal)),
                goal: Some(goal),
                cycle: None,
            });
        }
        ResearchGoalStatus::BudgetLimited => {
            return Ok(ResearchGoalCommandResult {
                assistant_content: format!(
                    "科研目标已达到轮次预算，未继续运行。\n\n{}",
                    format_goal_status(&goal)
                ),
                goal: Some(goal),
                cycle: None,
            });
        }
        ResearchGoalStatus::Active => {}
    }

    if goal.current_cycle >= goal.max_cycles {
        goal.status = ResearchGoalStatus::BudgetLimited;
        goal.updated_at = now_string();
        save_goal(layout, &goal)?;
        return Ok(ResearchGoalCommandResult {
            assistant_content: format!(
                "科研目标已达到最大轮次预算，状态已设为 `budget_limited`。\n\n{}",
                format_goal_status(&goal)
            ),
            goal: Some(goal),
            cycle: None,
        });
    }

    let request = build_continuation_request(&goal);
    let args = vec!["run".to_string(), request.clone()];
    let output = super::cli::run_research_cli(&args, &layout.root)?;
    let output_json = serde_json::from_str::<Value>(&output).unwrap_or_else(|_| {
        serde_json::json!({
            "status": "completed",
            "final_output": { "summary": output },
        })
    });

    let research_token_usage = collect_research_output_token_usage(&output_json);
    let (audit, audit_token_usage) =
        audit_research_output_with_llm(audit_client, second_opinion_client, &goal, &output_json)
            .await?;
    let cycle_token_usage = ResearchGoalCycleTokenUsage {
        research_system: research_token_usage,
        audit: audit_token_usage,
        total: research_token_usage.add(audit_token_usage),
    };
    let evidence_refs = collect_string_refs(&output_json, "evidence_refs");
    let artifact_refs = collect_string_refs(&output_json, "artifact_refs");
    let cycle_index = goal.current_cycle + 1;
    let cycle = ResearchGoalCycle {
        cycle_id: format!("cycle-{}", Uuid::new_v4()),
        goal_id: goal.goal_id.clone(),
        cycle_index,
        request,
        graph_id: output_json
            .get("graph_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        research_status: output_json
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_string),
        audit: audit.clone(),
        evidence_refs: evidence_refs.clone(),
        artifact_refs: artifact_refs.clone(),
        token_usage: cycle_token_usage,
        created_at: now_string(),
    };

    goal.current_cycle = cycle_index;
    goal.evidence_refs = merge_refs(&goal.evidence_refs, &evidence_refs);
    goal.artifact_refs = merge_refs(&goal.artifact_refs, &artifact_refs);
    goal.token_usage = goal.token_usage.add(cycle.token_usage.total);
    goal.last_audit = Some(audit);
    goal.last_run_at = Some(cycle.created_at.clone());
    goal.updated_at = now_string();
    goal.status = if goal.last_audit.as_ref().is_some_and(|audit| audit.complete) {
        ResearchGoalStatus::Complete
    } else if goal.current_cycle >= goal.max_cycles {
        ResearchGoalStatus::BudgetLimited
    } else {
        ResearchGoalStatus::Active
    };

    save_goal(layout, &goal)?;
    save_cycle(layout, &cycle)?;

    let assistant_content = format_goal_cycle_result(&goal, &cycle);
    Ok(ResearchGoalCommandResult {
        goal: Some(goal),
        cycle: Some(cycle),
        assistant_content,
    })
}

fn update_goal_status(
    layout: &ResearchGoalLayout,
    session_id: &str,
    status: ResearchGoalStatus,
    message: &str,
) -> Result<ResearchGoalCommandResult, String> {
    let mut goal = load_goal(layout, session_id)?;
    goal.status = status;
    goal.updated_at = now_string();
    save_goal(layout, &goal)?;
    Ok(ResearchGoalCommandResult {
        assistant_content: format!("{message}\n\n{}", format_goal_status(&goal)),
        goal: Some(goal),
        cycle: None,
    })
}

fn clear_goal(
    layout: &ResearchGoalLayout,
    session_id: &str,
) -> Result<ResearchGoalCommandResult, String> {
    let path = layout.goal_path(session_id);
    if path.exists() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
        Ok(ResearchGoalCommandResult {
            goal: None,
            cycle: None,
            assistant_content: "科研目标已清除。".to_string(),
        })
    } else {
        Ok(ResearchGoalCommandResult {
            goal: None,
            cycle: None,
            assistant_content: "当前会话没有可清除的科研目标。".to_string(),
        })
    }
}

fn load_goal(layout: &ResearchGoalLayout, session_id: &str) -> Result<ResearchGoal, String> {
    load_goal_if_exists(layout, session_id)?
        .ok_or_else(|| "当前会话还没有科研目标。使用 `/goal <科研目标>` 创建。".to_string())
}

fn load_goal_if_exists(
    layout: &ResearchGoalLayout,
    session_id: &str,
) -> Result<Option<ResearchGoal>, String> {
    let path = layout.goal_path(session_id);
    if !path.exists() {
        return Ok(None);
    }
    let mut goal: ResearchGoal = read_json(path)?;
    if ensure_goal_criterion_ids(&mut goal) {
        save_goal(layout, &goal)?;
    }
    Ok(Some(goal))
}

fn save_goal(layout: &ResearchGoalLayout, goal: &ResearchGoal) -> Result<(), String> {
    write_json(&layout.goals_dir, &safe_file_stem(&goal.session_id), goal)
}

fn save_cycle(layout: &ResearchGoalLayout, cycle: &ResearchGoalCycle) -> Result<(), String> {
    write_json(
        &layout.runs_dir.join(&cycle.goal_id),
        &cycle.cycle_id,
        cycle,
    )
}

fn build_continuation_request(goal: &ResearchGoal) -> String {
    let audit_context = goal
        .last_audit
        .as_ref()
        .map(|audit| {
            format!(
                "上一轮审计摘要：{}\n缺口：{}\n建议下一步：{}",
                audit.summary,
                audit.missing_requirements.join("；"),
                audit.next_actions.join("；")
            )
        })
        .unwrap_or_else(|| "这是第一轮推进，先建立证据链、分析框架和可检验结论边界。".to_string());

    format!(
        "长期科研目标：{objective}\n\n\
当前轮次：{next_cycle}/{max_cycles}\n\
成功标准：\n{criteria}\n\n\
已有证据引用：{evidence}\n\
已有产物引用：{artifacts}\n\n\
{audit_context}\n\n\
请执行下一轮科研推进，严格遵循：\n\
1. 先分析目标还缺什么证据或数据解释；\n\
2. 再围绕缺口进行检索、数据/机制分析或方法比较；\n\
3. 明确区分已证实结论、合理推断、未知/待验证部分；\n\
4. 产出可追溯 evidence_refs / artifact_refs；\n\
5. 给出下一轮是否仍需继续的判断依据。\n\n\
不要因为已经产生报告就默认完成，必须满足成功标准并通过审计。",
        objective = goal.objective,
        next_cycle = goal.current_cycle + 1,
        max_cycles = goal.max_cycles,
        criteria = goal_success_criteria_for_audit(goal)
            .iter()
            .map(|item| format!("- [{}] {}", item.criterion_id, item.text))
            .collect::<Vec<_>>()
            .join("\n"),
        evidence = if goal.evidence_refs.is_empty() {
            "无".to_string()
        } else {
            goal.evidence_refs.join(", ")
        },
        artifacts = if goal.artifact_refs.is_empty() {
            "无".to_string()
        } else {
            goal.artifact_refs.join(", ")
        },
    )
}

async fn audit_research_output_with_llm(
    audit_client: &dyn LlmClient,
    second_opinion_client: Option<&dyn LlmClient>,
    goal: &ResearchGoal,
    output: &Value,
) -> Result<(ResearchGoalAudit, ResearchGoalTokenUsage), String> {
    let messages = build_goal_audit_messages(goal, output);
    let primary_response = tokio::time::timeout(
        Duration::from_secs(GOAL_AUDIT_TIMEOUT_SECS),
        collect_llm_text_and_usage(audit_client, messages),
    )
    .await
    .map_err(|_| "LLM 科研目标审计超时，请稍后重试 /goal run。".to_string())??;

    let mut token_usage = primary_response.token_usage;
    let mut audit = parse_llm_research_goal_audit(&primary_response.text, goal, output)?;
    if audit.complete {
        let second_client = second_opinion_client.unwrap_or(audit_client);
        let second_messages = build_goal_second_opinion_messages(goal, output, &audit);
        let second_response = tokio::time::timeout(
            Duration::from_secs(GOAL_AUDIT_TIMEOUT_SECS),
            collect_llm_text_and_usage(second_client, second_messages),
        )
        .await
        .map_err(|_| "LLM 科研目标二次审计超时，请稍后重试 /goal run。".to_string())??;
        token_usage = token_usage.add(second_response.token_usage);
        let second_opinion = parse_llm_second_opinion(&second_response.text)?;
        apply_second_opinion_gate(&mut audit, second_opinion);
    }
    Ok((audit, token_usage))
}

fn build_goal_audit_messages(goal: &ResearchGoal, output: &Value) -> Vec<LlmMessage> {
    let output_json = serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string());
    let payload = json!({
        "objective": &goal.objective,
        "successCriteria": goal_success_criteria_for_audit(goal),
        "cycle": {
            "current": goal.current_cycle + 1,
            "max": goal.max_cycles,
        },
        "accumulatedEvidenceRefs": &goal.evidence_refs,
        "accumulatedArtifactRefs": &goal.artifact_refs,
        "previousAudit": &goal.last_audit,
        "researchOutputJson": truncate_chars(&output_json, MAX_AUDIT_PAYLOAD_CHARS),
    });
    let payload_text =
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());

    let system = r#"你是严谨的科研目标完成度审稿人，而不是执行者。

你的职责：基于用户的长期科研目标、成功标准和 Research System 本轮输出，判断目标是否真的完成。
必须进行语义审计：分析证据链、解释充分性、局限性、冲突证据、可复用产物是否满足目标。
不要使用“有 evidence_refs 就算完成”等启发式规则；不要因为文本看起来完整就默认完成。
如果证据不足、结论不可追溯、缺少独立验证、还有关键歧义或 Research System 输出显示执行失败，则 complete 必须为 false。

只输出一个 JSON object，不要 Markdown、不要代码围栏、不要解释文字。字段如下：
{
  "complete": false,
  "summary": "一句话审计结论",
  "confidence": "low|medium|high",
  "criteria": [
    {"criterionId": "必须引用输入 successCriteria[].criterionId", "criterion": "可简写该标准文本", "covered": false, "evidence": "对应证据或缺口"}
  ],
  "missingRequirements": ["仍缺什么，完成时为空数组"],
  "nextActions": ["下一轮最应该做什么，完成时给最终报告整理动作"],
  "limitations": ["已知局限，完成时也要保留"],
  "conflictingEvidence": ["冲突或不一致证据，没有则为空数组"],
  "finalReportReady": false
}

完成条件必须同时满足：
1. 每条 successCriteria 都必须用 criterionId 显式覆盖且 covered=true；
2. missingRequirements 为空；
3. finalReportReady=true，且 summary 明确说明可交付最终科研报告/结论；
4. 对局限与冲突证据已有清楚说明，未解决的关键冲突不得判定完成。"#;
    let user = format!("请审计以下科研目标推进结果：\n\n{payload_text}");

    vec![LlmMessage::system(system), LlmMessage::user(user)]
}

fn build_goal_second_opinion_messages(
    goal: &ResearchGoal,
    output: &Value,
    primary_audit: &ResearchGoalAudit,
) -> Vec<LlmMessage> {
    let output_json = serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string());
    let payload = json!({
        "objective": &goal.objective,
        "successCriteria": goal_success_criteria_for_audit(goal),
        "primaryAudit": primary_audit,
        "researchOutputJson": truncate_chars(&output_json, MAX_AUDIT_PAYLOAD_CHARS),
    });
    let payload_text =
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    let system = r#"你是第二位独立科研审稿人，专门复核第一位 LLM 审计是否过早判定科研目标完成。

请保持严格怀疑：只有在证据链、成功标准覆盖、局限性/冲突证据说明、最终报告可交付性都清楚时，才能同意完成。
如果仍有关键证据缺口、方法不可复现、结论边界不清、冲突证据未解决，必须 disagrees/false。

只输出一个 JSON object，不要 Markdown、不要代码围栏、不要解释文字：
{
  "agreesComplete": false,
  "summary": "二次审计结论",
  "confidence": "low|medium|high",
  "blockingConcerns": ["若不同意完成，列出阻断点；同意时为空数组"],
  "requiredNextActions": ["若不同意完成，列出下一步；同意时可为空数组"]
}"#;
    let user = format!("请对以下科研目标完成判定做第二意见审计：\n\n{payload_text}");

    vec![LlmMessage::system(system), LlmMessage::user(user)]
}

fn build_goal_criteria_suggestion_messages(goal: &ResearchGoal) -> Vec<LlmMessage> {
    let payload = json!({
        "objective": &goal.objective,
        "currentSuccessCriteria": goal_success_criteria_for_audit(goal),
        "cycle": {
            "current": goal.current_cycle,
            "max": goal.max_cycles,
        },
        "evidenceRefs": &goal.evidence_refs,
        "artifactRefs": &goal.artifact_refs,
        "lastAudit": &goal.last_audit,
    });
    let payload_text =
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    let system = r#"你是科研项目评审专家，负责为一个长期科研目标制定可验证的成功标准。

请基于目标语义、当前证据状态和已有审计，生成 5～8 条成功标准。标准必须：
- 面向科研完成度，而不是泛泛的任务清单；
- 可由后续 LLM 审计逐条判断 true/false；
- 覆盖研究边界、证据链、方法/数据可复现性、机制/解释、局限性/冲突证据、最终可交付报告；
- 避免空泛词，如“充分”“完善”，除非说明可检查对象；
- 每条不超过 240 个字符。

只输出 JSON 字符串数组，不要 Markdown、不要代码围栏、不要解释文字。
格式：["标准 1","标准 2"]"#;
    let user = format!("请为以下科研目标生成成功标准：\n\n{payload_text}");

    vec![LlmMessage::system(system), LlmMessage::user(user)]
}

fn build_goal_second_opinion_probe_messages() -> Vec<LlmMessage> {
    let system = r#"你是科研目标二次审计 provider 连通性探针。

请证明你能执行独立的科研完成度复核：只输出一个 JSON object，不要 Markdown、不要代码围栏、不要额外解释。
必须使用以下字段和值：
{
  "ok": true,
  "role": "research_goal_second_opinion_probe",
  "summary": "我可以作为科研目标二次审计模型，独立复核成功标准、证据链、局限性与最终报告可交付性。"
}"#;
    let user = "请执行一次 /goal 二审 provider 真实 LLM 探测，并严格返回要求的 JSON。";

    vec![LlmMessage::system(system), LlmMessage::user(user)]
}

struct LlmTextResponse {
    text: String,
    token_usage: ResearchGoalTokenUsage,
}

async fn collect_llm_text_and_usage(
    client: &dyn LlmClient,
    messages: Vec<LlmMessage>,
) -> Result<LlmTextResponse, String> {
    let stream = client
        .send_message_streaming(messages, vec![])
        .await
        .map_err(|err| format!("LLM 科研目标审计调用失败：{err}"))?;
    let mut stream = stream;
    let mut out = String::new();
    let mut usage: Option<LlmTokenUsage> = None;
    while let Some(res) = stream.next().await {
        match res {
            Ok(LlmStreamChunk::Text(text)) => out.push_str(&text),
            Ok(LlmStreamChunk::Usage(next_usage)) => usage = Some(next_usage),
            Ok(LlmStreamChunk::Stop { .. }) => break,
            Ok(LlmStreamChunk::Error(message)) => {
                return Err(format!("LLM 科研目标审计返回错误：{message}"));
            }
            Ok(_) => {}
            Err(err) => return Err(format!("LLM 科研目标审计流失败：{err}")),
        }
    }
    Ok(LlmTextResponse {
        text: out,
        token_usage: usage
            .as_ref()
            .map(ResearchGoalTokenUsage::from_llm_usage)
            .unwrap_or_default(),
    })
}

fn parse_llm_research_goal_criteria(raw: &str) -> Result<Vec<String>, String> {
    if let Some(slice) = extract_json_array_slice(raw) {
        let parsed = serde_json::from_str::<Vec<String>>(slice)
            .map_err(|err| format!("LLM 成功标准 JSON 解析失败：{err}"))?;
        return normalize_success_criteria(parsed);
    }

    if let Some(slice) = extract_json_object_slice(raw) {
        let value = serde_json::from_str::<Value>(slice)
            .map_err(|err| format!("LLM 成功标准 JSON 解析失败：{err}"))?;
        let criteria = value
            .get("criteria")
            .or_else(|| value.get("successCriteria"))
            .and_then(Value::as_array)
            .ok_or_else(|| "LLM 成功标准结果缺少 criteria 数组。".to_string())?
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .or_else(|| item.get("text").and_then(Value::as_str).map(str::to_string))
            })
            .collect::<Vec<_>>();
        return normalize_success_criteria(criteria);
    }

    Err("LLM 成功标准生成未返回 JSON 数组，无法使用启发式兜底。".to_string())
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawResearchGoalAudit {
    #[serde(default)]
    complete: bool,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    confidence: String,
    #[serde(default)]
    criteria: Vec<RawCriteriaAudit>,
    #[serde(default, alias = "missing_requirements")]
    missing_requirements: Vec<String>,
    #[serde(default, alias = "next_actions")]
    next_actions: Vec<String>,
    #[serde(default)]
    limitations: Vec<String>,
    #[serde(default, alias = "conflicting_evidence")]
    conflicting_evidence: Vec<String>,
    #[serde(default, alias = "final_report_ready")]
    final_report_ready: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawCriteriaAudit {
    #[serde(default, alias = "criterion_id")]
    criterion_id: String,
    #[serde(default)]
    criterion: String,
    #[serde(default)]
    covered: bool,
    #[serde(default)]
    evidence: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawSecondOpinion {
    #[serde(default, alias = "agree_complete", alias = "complete")]
    agrees_complete: bool,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    confidence: String,
    #[serde(default, alias = "blocking_concerns")]
    blocking_concerns: Vec<String>,
    #[serde(default, alias = "required_next_actions")]
    required_next_actions: Vec<String>,
}

fn parse_llm_research_goal_audit(
    raw: &str,
    goal: &ResearchGoal,
    output: &Value,
) -> Result<ResearchGoalAudit, String> {
    let Some(slice) = extract_json_object_slice(raw) else {
        return Err("LLM 科研目标审计未返回 JSON object，无法进行完成度判定。".to_string());
    };
    let parsed = serde_json::from_str::<RawResearchGoalAudit>(slice)
        .map_err(|err| format!("LLM 科研目标审计 JSON 解析失败：{err}"))?;
    Ok(normalize_llm_research_goal_audit(parsed, goal, output))
}

fn parse_llm_second_opinion(raw: &str) -> Result<ResearchGoalSecondOpinion, String> {
    let Some(slice) = extract_json_object_slice(raw) else {
        return Err("LLM 科研目标二次审计未返回 JSON object，无法确认完成。".to_string());
    };
    let parsed = serde_json::from_str::<RawSecondOpinion>(slice)
        .map_err(|err| format!("LLM 科研目标二次审计 JSON 解析失败：{err}"))?;
    Ok(ResearchGoalSecondOpinion {
        review_source: "llm_second_opinion".to_string(),
        agrees_complete: parsed.agrees_complete,
        confidence: normalize_confidence(&parsed.confidence),
        summary: clamp_text(
            if parsed.summary.trim().is_empty() {
                "LLM 二次审计未给出摘要。"
            } else {
                parsed.summary.trim()
            },
            MAX_AUDIT_FIELD_CHARS,
        ),
        blocking_concerns: sanitize_string_list(parsed.blocking_concerns),
        required_next_actions: sanitize_string_list(parsed.required_next_actions),
    })
}

fn parse_llm_second_opinion_provider_probe(raw: &str) -> Result<String, String> {
    let Some(slice) = extract_json_object_slice(raw) else {
        return Err("LLM 二审 provider 探测未返回 JSON object。".to_string());
    };
    let value = serde_json::from_str::<Value>(slice)
        .map_err(|err| format!("LLM 二审 provider 探测 JSON 解析失败：{err}"))?;
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !ok || role != "research_goal_second_opinion_probe" {
        return Err(
            "LLM 二审 provider 探测返回内容不符合预期，未确认其可执行二次审计。".to_string(),
        );
    }

    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("二审 provider 已完成真实 LLM 探测。");
    Ok(clamp_text(summary, MAX_AUDIT_FIELD_CHARS))
}

fn apply_second_opinion_gate(
    audit: &mut ResearchGoalAudit,
    second_opinion: ResearchGoalSecondOpinion,
) {
    if !second_opinion.agrees_complete {
        audit.complete = false;
        audit.final_report_ready = false;
        audit.missing_requirements.push(format!(
            "二次 LLM 审计未同意完成：{}",
            second_opinion.summary
        ));
        audit
            .missing_requirements
            .extend(second_opinion.blocking_concerns.iter().cloned());
        audit
            .next_actions
            .extend(second_opinion.required_next_actions.iter().cloned());
        sort_and_dedup(&mut audit.missing_requirements);
        sort_and_dedup(&mut audit.next_actions);
        audit.summary = format!("二次审计未通过：{}", audit.summary);
    } else {
        audit.summary = format!("双重 LLM 审计通过：{}", audit.summary);
    }
    audit.second_opinion = Some(second_opinion);
}

fn normalize_llm_research_goal_audit(
    parsed: RawResearchGoalAudit,
    goal: &ResearchGoal,
    output: &Value,
) -> ResearchGoalAudit {
    let mut missing_requirements = sanitize_string_list(parsed.missing_requirements);
    let mut next_actions = sanitize_string_list(parsed.next_actions);
    let limitations = sanitize_string_list(parsed.limitations);
    let conflicting_evidence = sanitize_string_list(parsed.conflicting_evidence);
    let summary = clamp_text(
        if parsed.summary.trim().is_empty() {
            "LLM 审计未给出摘要。"
        } else {
            parsed.summary.trim()
        },
        MAX_AUDIT_FIELD_CHARS,
    );
    let expected_criteria = goal_success_criteria_for_audit(goal);
    let expected_by_id = expected_criteria
        .iter()
        .map(|criterion| (criterion.criterion_id.clone(), criterion.text.clone()))
        .collect::<BTreeMap<_, _>>();
    let expected_id_by_text = expected_criteria
        .iter()
        .map(|criterion| {
            (
                normalize_criterion_text(&criterion.text).to_lowercase(),
                criterion.criterion_id.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut criteria = Vec::new();
    let mut seen = BTreeSet::new();
    for item in parsed.criteria.into_iter().take(MAX_AUDIT_ARRAY_ITEMS) {
        let criterion = item
            .criterion
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let criterion_id = normalize_criterion_id(&item.criterion_id).or_else(|| {
            expected_id_by_text
                .get(&normalize_criterion_text(&criterion).to_lowercase())
                .cloned()
        });
        let Some(criterion_id) = criterion_id else {
            continue;
        };
        if !seen.insert(criterion_id.clone()) {
            continue;
        }
        let criterion_text = expected_by_id
            .get(&criterion_id)
            .cloned()
            .unwrap_or_else(|| criterion.clone());
        criteria.push(CriteriaAudit {
            criterion_id,
            criterion: clamp_text(&criterion_text, MAX_AUDIT_FIELD_CHARS),
            covered: item.covered,
            evidence: clamp_text(item.evidence.trim(), MAX_AUDIT_FIELD_CHARS),
        });
    }

    for required in &expected_criteria {
        if !seen.contains(&required.criterion_id) {
            criteria.push(CriteriaAudit {
                criterion_id: required.criterion_id.clone(),
                criterion: required.text.clone(),
                covered: false,
                evidence: "LLM 审计未显式覆盖该成功标准。".to_string(),
            });
            missing_requirements.push(format!(
                "LLM 审计未覆盖成功标准 {}：{}",
                required.criterion_id, required.text
            ));
        }
    }

    for blocker in execution_validity_blockers(output) {
        missing_requirements.push(blocker);
    }

    if criteria.iter().any(|criterion| !criterion.covered) {
        missing_requirements.push("LLM 审计指出仍有成功标准未满足。".to_string());
    }

    if parsed.complete && !parsed.final_report_ready {
        missing_requirements.push("LLM 审计未确认 finalReportReady=true。".to_string());
    }

    sort_and_dedup(&mut missing_requirements);
    sort_and_dedup(&mut next_actions);
    if next_actions.is_empty() {
        next_actions = if missing_requirements.is_empty() {
            vec!["整理最终科研报告，保留证据边界、局限性和可检验假设。".to_string()]
        } else {
            vec!["围绕 LLM 审计缺口继续执行 `/goal run`。".to_string()]
        };
    }

    let criteria_complete =
        !criteria.is_empty() && criteria.iter().all(|criterion| criterion.covered);
    let complete = parsed.complete
        && parsed.final_report_ready
        && criteria_complete
        && missing_requirements.is_empty();

    ResearchGoalAudit {
        complete,
        review_source: "llm".to_string(),
        confidence: normalize_confidence(&parsed.confidence),
        final_report_ready: parsed.final_report_ready,
        summary,
        criteria,
        missing_requirements,
        next_actions,
        limitations,
        conflicting_evidence,
        second_opinion: None,
    }
}

fn execution_validity_blockers(output: &Value) -> Vec<String> {
    // This is a hard execution-validity guard, not a completion heuristic:
    // even an LLM cannot mark a run complete if the Research System failed,
    // omitted final output, or returned failing reviewer signals.
    let status_complete = output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "completed");
    let final_output_present = output
        .get("final_output")
        .is_some_and(|value| !value_is_empty(value));
    let issues = output
        .get("issues")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let review_failures = collect_review_failures(output);
    let mut missing_requirements = Vec::new();
    if !status_complete {
        missing_requirements.push("Research System 本轮未完成所有任务。".to_string());
    }
    if !final_output_present {
        missing_requirements.push("本轮缺少 final_output，不能进行完成确认。".to_string());
    }
    if !issues.is_empty() {
        missing_requirements.extend(issues.iter().map(|issue| format!("执行问题：{issue}")));
    }
    if !review_failures.is_empty() {
        missing_requirements.extend(
            review_failures
                .iter()
                .map(|issue| format!("审查未通过：{issue}")),
        );
    }

    missing_requirements
}

fn extract_json_object_slice(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in raw[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&raw[start..start + offset + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }

    None
}

fn extract_json_array_slice(raw: &str) -> Option<&str> {
    let start = raw.find('[')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in raw[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&raw[start..start + offset + ch.len_utf8()]);
                }
            }
            _ => {}
        }
    }

    None
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn clamp_text(text: &str, max_chars: usize) -> String {
    truncate_chars(text.trim(), max_chars)
}

fn sanitize_string_list(items: Vec<String>) -> Vec<String> {
    let mut out = items
        .into_iter()
        .map(|item| clamp_text(&item, MAX_AUDIT_FIELD_CHARS))
        .filter(|item| !item.is_empty())
        .take(MAX_AUDIT_ARRAY_ITEMS)
        .collect::<Vec<_>>();
    sort_and_dedup(&mut out);
    out
}

fn sort_and_dedup(items: &mut Vec<String>) {
    items.sort();
    items.dedup();
}

fn normalize_confidence(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "high" | "medium" | "low" => value.trim().to_ascii_lowercase(),
        _ => "low".to_string(),
    }
}

fn collect_review_failures(output: &Value) -> Vec<String> {
    output
        .get("review_results")
        .and_then(Value::as_object)
        .map(|reviews| {
            reviews
                .iter()
                .filter_map(|(task_id, review)| {
                    let status = review.get("status").and_then(Value::as_str).unwrap_or("");
                    let allowed = review
                        .get("final_answer_allowed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if status == "pass" && allowed {
                        None
                    } else {
                        Some(format!(
                            "{task_id}: status={status}, final_answer_allowed={allowed}"
                        ))
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn collect_string_refs(value: &Value, key: &str) -> Vec<String> {
    let mut refs = BTreeSet::new();
    collect_string_refs_inner(value, key, &mut refs);
    refs.into_iter().collect()
}

fn collect_string_refs_inner(value: &Value, key: &str, refs: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    if let Some(items) = v.as_array() {
                        for item in items {
                            if let Some(text) = item.as_str() {
                                refs.insert(text.to_string());
                            }
                        }
                    }
                }
                collect_string_refs_inner(v, key, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_string_refs_inner(item, key, refs);
            }
        }
        _ => {}
    }
}

fn collect_research_output_token_usage(output: &Value) -> ResearchGoalTokenUsage {
    let Some(results) = output.get("task_results").and_then(Value::as_object) else {
        return ResearchGoalTokenUsage::default();
    };

    results
        .values()
        .filter_map(|result| {
            result
                .get("token_usage")
                .or_else(|| result.get("tokenUsage"))
        })
        .filter_map(token_usage_from_value)
        .fold(
            ResearchGoalTokenUsage::default(),
            ResearchGoalTokenUsage::add,
        )
}

fn token_usage_from_value(value: &Value) -> Option<ResearchGoalTokenUsage> {
    let input = value
        .get("input_tokens")
        .or_else(|| value.get("inputTokens"))
        .or_else(|| value.get("prompt_tokens"))
        .or_else(|| value.get("promptTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = value
        .get("output_tokens")
        .or_else(|| value.get("outputTokens"))
        .or_else(|| value.get("completion_tokens"))
        .or_else(|| value.get("completionTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = value
        .get("total_tokens")
        .or_else(|| value.get("totalTokens"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input.saturating_add(output));

    if input == 0 && output == 0 && total == 0 {
        return None;
    }

    Some(ResearchGoalTokenUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
    })
}

fn merge_refs(existing: &[String], additions: &[String]) -> Vec<String> {
    existing
        .iter()
        .chain(additions.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn format_goal_status(goal: &ResearchGoal) -> String {
    let second_opinion = goal
        .second_opinion_provider_entry
        .as_deref()
        .map(|entry| format!("二审模型：{entry}\n"))
        .unwrap_or_default();
    let auto_run = if goal.auto_run_policy.enabled {
        format!(
            "自动续跑：开启（每次最多 {} 轮，空闲 {}ms{}{}）\n",
            goal.auto_run_policy.cycles_per_run,
            goal.auto_run_policy.idle_delay_ms,
            goal.auto_run_policy
                .max_elapsed_minutes
                .map(|minutes| format!("，最长 {minutes} 分钟"))
                .unwrap_or_default(),
            goal.auto_run_policy
                .max_tokens
                .map(|tokens| format!("，最多 {tokens} tokens"))
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    let audit = goal
        .last_audit
        .as_ref()
        .map(|audit| {
            format!(
                "\n\n最近审计（{}）：{}\n缺口：{}\n下一步：{}",
                format_audit_source(audit),
                audit.summary,
                if audit.missing_requirements.is_empty() {
                    "无".to_string()
                } else {
                    audit.missing_requirements.join("；")
                },
                audit.next_actions.join("；")
            )
        })
        .unwrap_or_default();

    format!(
        "## 科研目标\n\n\
状态：`{status}`\n\
轮次：{cycle}/{max_cycles}\n\
目标：{objective}\n\
{second_opinion}\
{auto_run}\
Token 使用：{tokens}\n\
证据引用：{evidence_count}\n\
产物引用：{artifact_count}{audit}",
        status = goal.status.label(),
        cycle = goal.current_cycle,
        max_cycles = goal.max_cycles,
        objective = goal.objective,
        second_opinion = second_opinion,
        auto_run = auto_run,
        tokens = goal.token_usage.total_tokens,
        evidence_count = goal.evidence_refs.len(),
        artifact_count = goal.artifact_refs.len(),
        audit = audit,
    )
}

fn format_goal_cycle_result(goal: &ResearchGoal, cycle: &ResearchGoalCycle) -> String {
    let audit = &cycle.audit;
    let criteria = audit
        .criteria
        .iter()
        .map(|item| {
            format!(
                "- [{}] {} — {}",
                if item.covered { "x" } else { " " },
                item.criterion,
                item.evidence
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "已完成第 {cycle}/{max_cycles} 轮科研目标推进。\n\n\
{}\n\n\
### LLM 完成审计\n\
审计来源：{audit_source}\n\n\
{}\n\n\
{}\n\n\
缺口：{}\n\n\
下一步：{}\n\n\
当前状态：`{status}`",
        format_goal_status(goal),
        audit.summary,
        criteria,
        if audit.missing_requirements.is_empty() {
            "无".to_string()
        } else {
            audit.missing_requirements.join("；")
        },
        audit.next_actions.join("；"),
        cycle = cycle.cycle_index,
        max_cycles = goal.max_cycles,
        status = goal.status.label(),
        audit_source = format_audit_source(audit),
    )
}

fn format_goal_auto_run_result(goal: &ResearchGoal, cycles: &[ResearchGoalCycle]) -> String {
    let last = cycles.last();
    let audit_summary = last
        .map(|cycle| cycle.audit.summary.as_str())
        .unwrap_or("未产生审计摘要。");
    let missing = last
        .map(|cycle| {
            if cycle.audit.missing_requirements.is_empty() {
                "无".to_string()
            } else {
                cycle.audit.missing_requirements.join("；")
            }
        })
        .unwrap_or_else(|| "无".to_string());
    let next_actions = last
        .map(|cycle| cycle.audit.next_actions.join("；"))
        .unwrap_or_else(|| "查看 `/goal status`。".to_string());

    format!(
        "自动续跑完成：本次执行 {ran} 轮，当前进度 {current}/{max_cycles}。\n\n\
{}\n\n\
### 最近一轮 LLM 完成审计\n\
审计来源：{audit_source}\n\n\
{audit_summary}\n\n\
缺口：{missing}\n\n\
下一步：{next_actions}\n\n\
当前状态：`{status}`",
        format_goal_status(goal),
        ran = cycles.len(),
        current = goal.current_cycle,
        max_cycles = goal.max_cycles,
        status = goal.status.label(),
        audit_source = last
            .map(|cycle| format_audit_source(&cycle.audit))
            .unwrap_or_else(|| "未知".to_string()),
    )
}

fn format_audit_source(audit: &ResearchGoalAudit) -> String {
    let source = match audit.review_source.as_str() {
        "llm" => "LLM",
        "" | "unknown" => "未知",
        other => other,
    };
    if audit.confidence.trim().is_empty() || audit.confidence == "unknown" {
        source.to_string()
    } else {
        format!("{source}，置信度：{}", audit.confidence)
    }
}

fn goal_help_text() -> String {
    [
        "## `/goal` 科研长期目标",
        "",
        "- `/goal <科研目标>`：设置或替换当前会话的科研目标",
        "- `/goal run`：执行一轮分析 → 解读 → 再分析，并调用 LLM 做完成审计",
        "- `/goal run --cycles 3`：在当前轮次预算内自动续跑最多 3 轮",
        "- `/goal budget 5`：把当前目标的最大轮次预算设为 5",
        "- `/goal status`：查看目标状态",
        "- `/goal pause` / `/goal resume`：暂停或恢复目标",
        "- `/goal clear`：清除目标",
        "",
        "示例：`/goal --max-cycles 5 解析 QS 核心基因在肿瘤免疫微环境中的作用机制`",
    ]
    .join("\n")
}

fn default_success_criteria() -> Vec<String> {
    vec![
        "形成可追溯的证据或数据分析记录".to_string(),
        "给出围绕目标的解释、局限与下一步".to_string(),
        "产出可复用的科研结论或报告草稿".to_string(),
    ]
}

fn default_audit_review_source() -> String {
    "unknown".to_string()
}

fn default_audit_confidence() -> String {
    "unknown".to_string()
}

fn goal_success_criteria_for_audit(goal: &ResearchGoal) -> Vec<ResearchGoalCriterion> {
    let mut snapshot = goal.clone();
    ensure_goal_criterion_ids(&mut snapshot);
    snapshot
        .success_criteria
        .iter()
        .zip(snapshot.success_criterion_ids.iter())
        .map(|(text, criterion_id)| ResearchGoalCriterion {
            criterion_id: criterion_id.clone(),
            text: text.clone(),
        })
        .collect()
}

fn update_goal_success_criteria(
    goal: &mut ResearchGoal,
    criteria: Vec<String>,
) -> Result<(), String> {
    let existing_ids = criterion_id_map(goal);
    goal.success_criteria = normalize_success_criteria(criteria)?;
    goal.success_criterion_ids = criterion_ids_for(&goal.success_criteria, &existing_ids);
    Ok(())
}

fn normalize_second_opinion_provider_entry(entry: &str) -> Option<String> {
    let normalized = entry.replace(|ch: char| ch.is_whitespace(), " ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn ensure_goal_criterion_ids(goal: &mut ResearchGoal) -> bool {
    if goal.success_criteria.is_empty() {
        goal.success_criteria = default_success_criteria();
    }
    let existing_ids = criterion_id_map(goal);
    let expected_ids = criterion_ids_for(&goal.success_criteria, &existing_ids);
    if goal.success_criterion_ids == expected_ids {
        return false;
    }
    goal.success_criterion_ids = expected_ids;
    true
}

fn criterion_id_map(goal: &ResearchGoal) -> BTreeMap<String, String> {
    goal.success_criteria
        .iter()
        .zip(goal.success_criterion_ids.iter())
        .filter_map(|(criterion, criterion_id)| {
            normalize_criterion_id(criterion_id)
                .map(|id| (normalize_criterion_text(criterion).to_lowercase(), id))
        })
        .collect()
}

fn criterion_ids_for(criteria: &[String], existing_ids: &BTreeMap<String, String>) -> Vec<String> {
    let mut used = BTreeSet::new();
    criteria
        .iter()
        .enumerate()
        .map(|(index, criterion)| {
            let key = normalize_criterion_text(criterion).to_lowercase();
            let base = existing_ids
                .get(&key)
                .cloned()
                .unwrap_or_else(|| criterion_id_for_text(criterion));
            unique_criterion_id(base, index, &mut used)
        })
        .collect()
}

fn unique_criterion_id(mut candidate: String, index: usize, used: &mut BTreeSet<String>) -> String {
    if used.insert(candidate.clone()) {
        return candidate;
    }

    let base = candidate.clone();
    let mut suffix = index + 1;
    loop {
        candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn criterion_id_for_text(text: &str) -> String {
    let normalized = normalize_criterion_text(text);
    let mut hash = 0xcbf29ce484222325u64;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("crit-{hash:016x}")
}

fn normalize_criterion_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_criterion_id(raw: &str) -> Option<String> {
    let id = raw.trim();
    if id.is_empty() {
        return None;
    }
    Some(
        id.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '-'
                }
            })
            .collect::<String>(),
    )
}

fn normalize_success_criteria(criteria: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for criterion in criteria {
        let item = criterion.split_whitespace().collect::<Vec<_>>().join(" ");
        if item.is_empty() {
            continue;
        }
        if item.chars().count() > 240 {
            return Err("单条成功标准不能超过 240 个字符".to_string());
        }
        if seen.insert(item.to_lowercase()) {
            normalized.push(item);
        }
    }

    if normalized.is_empty() {
        return Err("成功标准不能为空".to_string());
    }
    if normalized.len() > 12 {
        return Err("成功标准最多支持 12 条".to_string());
    }
    Ok(normalized)
}

fn normalize_max_cycles(max_cycles: u32) -> Result<u32, String> {
    if max_cycles == 0 {
        return Err("轮次预算必须大于 0".to_string());
    }
    if max_cycles > MAX_GOAL_CYCLES {
        return Err(format!("轮次预算最多支持 {MAX_GOAL_CYCLES} 轮"));
    }
    Ok(max_cycles)
}

fn normalize_auto_run_policy(
    update: ResearchGoalAutoRunPolicyUpdate,
    previous: &ResearchGoalAutoRunPolicy,
    now: &str,
) -> Result<ResearchGoalAutoRunPolicy, String> {
    if update.cycles_per_run == 0 || update.cycles_per_run > MAX_AUTO_RUN_CYCLES {
        return Err(format!(
            "自动续跑每次轮数必须在 1 到 {MAX_AUTO_RUN_CYCLES} 之间。"
        ));
    }
    if !(MIN_AUTO_RUN_IDLE_DELAY_MS..=MAX_AUTO_RUN_IDLE_DELAY_MS).contains(&update.idle_delay_ms) {
        return Err(format!(
            "自动续跑空闲延迟必须在 {MIN_AUTO_RUN_IDLE_DELAY_MS} 到 {MAX_AUTO_RUN_IDLE_DELAY_MS} ms 之间。"
        ));
    }
    if let Some(minutes) = update.max_elapsed_minutes {
        if minutes == 0 || minutes > MAX_AUTO_RUN_ELAPSED_MINUTES {
            return Err(format!(
                "自动续跑最长耗时必须在 1 到 {MAX_AUTO_RUN_ELAPSED_MINUTES} 分钟之间。"
            ));
        }
    }
    if let Some(tokens) = update.max_tokens {
        if tokens == 0 || tokens > MAX_AUTO_RUN_TOKENS {
            return Err(format!(
                "自动续跑 token 预算必须在 1 到 {MAX_AUTO_RUN_TOKENS} 之间。"
            ));
        }
    }

    let started_at = if update.enabled {
        previous
            .started_at
            .clone()
            .filter(|_| previous.enabled)
            .or_else(|| Some(now.to_string()))
    } else {
        None
    };

    Ok(ResearchGoalAutoRunPolicy {
        enabled: update.enabled,
        cycles_per_run: update.cycles_per_run,
        idle_delay_ms: update.idle_delay_ms,
        max_elapsed_minutes: update.max_elapsed_minutes,
        max_tokens: update.max_tokens,
        started_at,
    })
}

fn auto_run_budget_reached_reason(goal: &ResearchGoal) -> Option<String> {
    if !goal.auto_run_policy.enabled {
        return None;
    }

    if let Some(max_tokens) = goal.auto_run_policy.max_tokens {
        if goal.token_usage.total_tokens >= max_tokens {
            return Some(format!(
                "token 预算已达到（{}/{max_tokens}）",
                goal.token_usage.total_tokens
            ));
        }
    }

    if let (Some(max_elapsed_minutes), Some(started_at)) = (
        goal.auto_run_policy.max_elapsed_minutes,
        goal.auto_run_policy.started_at.as_deref(),
    ) {
        if let Ok(started_at) = chrono::DateTime::parse_from_rfc3339(started_at) {
            let elapsed_minutes = Utc::now()
                .signed_duration_since(started_at.with_timezone(&Utc))
                .num_minutes();
            if elapsed_minutes >= i64::from(max_elapsed_minutes) {
                return Some(format!(
                    "最长耗时预算已达到（{elapsed_minutes}/{max_elapsed_minutes} 分钟）"
                ));
            }
        }
    }

    None
}

fn disable_auto_run_for_budget(
    layout: &ResearchGoalLayout,
    mut goal: ResearchGoal,
    reason: &str,
) -> Result<ResearchGoal, String> {
    if !goal.auto_run_policy.enabled {
        return Ok(goal);
    }
    let now = now_string();
    goal.auto_run_policy.enabled = false;
    goal.auto_run_policy.started_at = None;
    goal.updated_at = now.clone();
    goal.notes
        .push(format!("Goal auto-run stopped at {now}: {reason}."));
    save_goal(layout, &goal)?;
    Ok(goal)
}

fn value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn default_auto_run_cycles_per_run() -> u32 {
    MAX_AUTO_RUN_CYCLES
}

fn default_auto_run_idle_delay_ms() -> u64 {
    DEFAULT_AUTO_RUN_IDLE_DELAY_MS
}

#[derive(Debug, Clone)]
struct ResearchGoalLayout {
    root: PathBuf,
    goals_dir: PathBuf,
    runs_dir: PathBuf,
}

impl ResearchGoalLayout {
    fn new(root: &Path) -> Self {
        let state_dir = root.join(".research");
        Self {
            root: root.to_path_buf(),
            goals_dir: state_dir.join("goals"),
            runs_dir: state_dir.join("goal-runs"),
        }
    }

    fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(&self.goals_dir).map_err(|err| err.to_string())?;
        fs::create_dir_all(&self.runs_dir).map_err(|err| err.to_string())?;
        Ok(())
    }

    fn goal_path(&self, session_id: &str) -> PathBuf {
        self.goals_dir
            .join(format!("{}.json", safe_file_stem(session_id)))
    }
}

fn safe_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn write_json<T: Serialize>(dir: &Path, id: &str, value: &T) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let json = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    fs::write(dir.join(format!("{id}.json")), json).map_err(|err| err.to_string())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&raw).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::ToolSchema;
    use crate::errors::ApiError;
    use crate::llm::{LlmConfig, LlmProvider, TokenUsage as TestLlmTokenUsage};
    use async_trait::async_trait;
    use futures::{stream, Stream};
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct StaticAuditClient {
        config: LlmConfig,
        raws: Vec<String>,
        usages: Vec<Option<TestLlmTokenUsage>>,
        calls: Arc<AtomicUsize>,
    }

    impl StaticAuditClient {
        fn new(raw: String) -> Self {
            Self::new_sequence(vec![raw])
        }

        fn new_sequence(raws: Vec<String>) -> Self {
            Self {
                config: LlmConfig::new(LlmProvider::OpenAi, "test-key"),
                raws,
                usages: Vec::new(),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn new_sequence_with_usage(
            raws: Vec<String>,
            usages: Vec<Option<TestLlmTokenUsage>>,
        ) -> Self {
            Self {
                config: LlmConfig::new(LlmProvider::OpenAi, "test-key"),
                raws,
                usages,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmClient for StaticAuditClient {
        async fn send_message_streaming(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolSchema>,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
        {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let raw = self
                .raws
                .get(index)
                .or_else(|| self.raws.last())
                .cloned()
                .unwrap_or_default();
            let mut chunks = Vec::new();
            if let Some(Some(usage)) = self
                .usages
                .get(index)
                .or_else(|| self.usages.last())
                .cloned()
            {
                chunks.push(Ok(LlmStreamChunk::Usage(usage)));
            }
            chunks.push(Ok(LlmStreamChunk::Text(raw)));
            chunks.push(Ok(LlmStreamChunk::Stop { stop_reason: None }));
            Ok(Box::pin(stream::iter(chunks)))
        }

        async fn health_check(&self) -> Result<bool, ApiError> {
            Ok(true)
        }

        fn config(&self) -> &LlmConfig {
            &self.config
        }
    }

    fn llm_audit_json(complete: bool) -> String {
        let criteria = default_success_criteria()
            .into_iter()
            .map(|criterion| {
                json!({
                    "criterionId": criterion_id_for_text(&criterion),
                    "criterion": criterion,
                    "covered": complete,
                    "evidence": if complete {
                        "LLM 认为该标准已由本轮 Research System 输出充分覆盖。"
                    } else {
                        "LLM 认为仍缺少独立证据或解释闭环。"
                    },
                })
            })
            .collect::<Vec<_>>();

        json!({
            "complete": complete,
            "summary": if complete {
                "LLM 审计通过：目标已具备可交付科研报告。"
            } else {
                "LLM 审计未通过：仍需要继续补充证据。"
            },
            "confidence": "medium",
            "criteria": criteria,
            "missingRequirements": if complete {
                Vec::<String>::new()
            } else {
                vec!["缺少独立证据交叉验证".to_string()]
            },
            "nextActions": if complete {
                vec!["整理最终科研报告".to_string()]
            } else {
                vec!["继续执行 /goal run".to_string()]
            },
            "limitations": ["mock audit"],
            "conflictingEvidence": [],
            "finalReportReady": complete,
        })
        .to_string()
    }

    fn test_goal() -> ResearchGoal {
        let success_criteria = default_success_criteria();
        let success_criterion_ids = criterion_ids_for(&success_criteria, &BTreeMap::new());
        ResearchGoal {
            goal_id: "goal-1".to_string(),
            session_id: "session-1".to_string(),
            objective: "研究目标".to_string(),
            status: ResearchGoalStatus::Active,
            success_criteria,
            success_criterion_ids,
            second_opinion_provider_entry: None,
            auto_run_policy: ResearchGoalAutoRunPolicy::default(),
            token_usage: ResearchGoalTokenUsage::default(),
            max_cycles: 3,
            current_cycle: 0,
            evidence_refs: Vec::new(),
            artifact_refs: Vec::new(),
            notes: Vec::new(),
            last_audit: None,
            created_at: now_string(),
            updated_at: now_string(),
            last_run_at: None,
        }
    }

    fn llm_suggested_criteria_json() -> String {
        json!([
            "明确研究对象、关键变量、样本/实验条件与科研目标边界。",
            "形成可追溯证据链，记录文献、数据、分析步骤和证据强度。",
            "给出围绕目标的机制解释或可检验假设，并区分结论、推断与未知。",
            "识别冲突证据、局限性、混杂因素和下一步验证方案。",
            "产出可复用科研报告草稿，包含结论、证据引用、图表/表格和后续任务。"
        ])
        .to_string()
    }

    fn llm_second_opinion_json(agrees_complete: bool) -> String {
        json!({
            "agreesComplete": agrees_complete,
            "summary": if agrees_complete {
                "二次审计同意完成，证据链与报告边界可接受。"
            } else {
                "二次审计不同意完成，仍缺少独立验证。"
            },
            "confidence": "high",
            "blockingConcerns": if agrees_complete {
                Vec::<String>::new()
            } else {
                vec!["缺少独立验证".to_string()]
            },
            "requiredNextActions": if agrees_complete {
                Vec::<String>::new()
            } else {
                vec!["补充独立数据或文献交叉验证".to_string()]
            },
        })
        .to_string()
    }

    #[test]
    fn research_goal_parser_recognizes_controls_and_set() {
        assert_eq!(
            parse_research_goal_body(""),
            ParsedResearchGoalCommand::Status
        );
        assert_eq!(
            parse_research_goal_body("run"),
            ParsedResearchGoalCommand::Run { auto_cycles: 1 }
        );
        assert_eq!(
            parse_research_goal_body("run --cycles 3"),
            ParsedResearchGoalCommand::Run { auto_cycles: 3 }
        );
        assert_eq!(
            parse_research_goal_body("budget 5"),
            ParsedResearchGoalCommand::Budget { max_cycles: 5 }
        );
        assert_eq!(
            parse_research_goal_body("runaway 机制研究"),
            ParsedResearchGoalCommand::Set {
                objective: "runaway 机制研究".to_string(),
                max_cycles: None,
            }
        );
        assert_eq!(
            parse_research_goal_body("pause"),
            ParsedResearchGoalCommand::Pause
        );
        assert_eq!(
            parse_research_goal_body("--max-cycles 5 解析 QS 机制"),
            ParsedResearchGoalCommand::Set {
                objective: "解析 QS 机制".to_string(),
                max_cycles: Some(5),
            }
        );
    }

    #[test]
    fn goal_lifecycle_persists_and_clears_goal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let created = run_research_goal_command(tmp.path(), "session-1", "研究 QS 核心基因")
            .expect("set goal")
            .goal
            .expect("goal");

        assert_eq!(created.status, ResearchGoalStatus::Active);
        assert_eq!(created.current_cycle, 0);
        assert_eq!(
            created.success_criterion_ids.len(),
            created.success_criteria.len()
        );
        assert!(created
            .success_criterion_ids
            .iter()
            .all(|id| id.starts_with("crit-")));
        assert_eq!(
            read_research_goal(tmp.path(), "session-1")
                .expect("read goal")
                .as_ref()
                .map(|goal| goal.objective.as_str()),
            Some("研究 QS 核心基因")
        );

        let status = run_research_goal_command(tmp.path(), "session-1", "status")
            .expect("status")
            .assistant_content;
        assert!(status.contains("研究 QS 核心基因"));

        let paused = run_research_goal_command(tmp.path(), "session-1", "pause")
            .expect("pause")
            .goal
            .expect("goal");
        assert_eq!(paused.status, ResearchGoalStatus::Paused);

        let cleared = run_research_goal_command(tmp.path(), "session-1", "clear")
            .expect("clear")
            .assistant_content;
        assert!(cleared.contains("已清除"));
    }

    #[test]
    fn update_goal_criteria_persists_and_invalidates_previous_audit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut goal = run_research_goal_command(tmp.path(), "session-1", "研究 QS 核心基因")
            .expect("set goal")
            .goal
            .expect("goal");
        goal.status = ResearchGoalStatus::Complete;
        goal.last_audit = Some(ResearchGoalAudit {
            complete: true,
            review_source: "llm".to_string(),
            confidence: "high".to_string(),
            final_report_ready: true,
            summary: "old audit".to_string(),
            criteria: Vec::new(),
            missing_requirements: Vec::new(),
            next_actions: Vec::new(),
            limitations: Vec::new(),
            conflicting_evidence: Vec::new(),
            second_opinion: None,
        });
        save_goal(&ResearchGoalLayout::new(tmp.path()), &goal).expect("save complete goal");

        let updated = update_research_goal_criteria(
            tmp.path(),
            "session-1",
            vec![
                "  形成证据链  ".to_string(),
                "形成证据链".to_string(),
                "解释局限与下一步".to_string(),
            ],
        )
        .expect("update criteria");

        assert_eq!(
            updated.success_criteria,
            vec!["形成证据链".to_string(), "解释局限与下一步".to_string()]
        );
        assert_eq!(updated.success_criterion_ids.len(), 2);
        assert_ne!(
            updated.success_criterion_ids[0],
            updated.success_criterion_ids[1]
        );
        assert_eq!(updated.status, ResearchGoalStatus::Active);
        assert!(updated.last_audit.is_none());

        let previous_first_id = updated.success_criterion_ids[0].clone();
        let reordered = update_research_goal_criteria(
            tmp.path(),
            "session-1",
            vec!["解释局限与下一步".to_string(), "形成证据链".to_string()],
        )
        .expect("reorder criteria");
        assert_eq!(reordered.success_criterion_ids[1], previous_first_id);
    }

    #[test]
    fn update_goal_budget_persists_and_can_reactivate_budget_limited_goal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut goal =
            run_research_goal_command(tmp.path(), "session-1", "--max-cycles 1 研究 QS 核心基因")
                .expect("set goal")
                .goal
                .expect("goal");
        goal.current_cycle = 1;
        goal.status = ResearchGoalStatus::BudgetLimited;
        save_goal(&ResearchGoalLayout::new(tmp.path()), &goal).expect("save budget goal");

        let updated = update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: Some(3),
                second_opinion_provider_entry: None,
                auto_run_policy: None,
            },
        )
        .expect("update budget");

        assert_eq!(updated.max_cycles, 3);
        assert_eq!(updated.status, ResearchGoalStatus::Active);
    }

    #[test]
    fn update_goal_auto_run_policy_persists_without_invalidating_audit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut goal = run_research_goal_command(tmp.path(), "session-1", "研究 QS 核心基因")
            .expect("set goal")
            .goal
            .expect("goal");
        goal.last_audit = Some(ResearchGoalAudit {
            complete: false,
            review_source: "llm".to_string(),
            confidence: "medium".to_string(),
            final_report_ready: false,
            summary: "old audit".to_string(),
            criteria: Vec::new(),
            missing_requirements: vec!["仍缺证据".to_string()],
            next_actions: vec!["继续".to_string()],
            limitations: Vec::new(),
            conflicting_evidence: Vec::new(),
            second_opinion: None,
        });
        save_goal(&ResearchGoalLayout::new(tmp.path()), &goal).expect("save goal");

        let enabled = update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: None,
                second_opinion_provider_entry: None,
                auto_run_policy: Some(ResearchGoalAutoRunPolicyUpdate {
                    enabled: true,
                    cycles_per_run: 2,
                    idle_delay_ms: 1_000,
                    max_elapsed_minutes: Some(30),
                    max_tokens: Some(10_000),
                }),
            },
        )
        .expect("enable auto run");

        assert!(enabled.auto_run_policy.enabled);
        assert_eq!(enabled.auto_run_policy.cycles_per_run, 2);
        assert_eq!(enabled.auto_run_policy.idle_delay_ms, 1_000);
        assert_eq!(enabled.auto_run_policy.max_elapsed_minutes, Some(30));
        assert_eq!(enabled.auto_run_policy.max_tokens, Some(10_000));
        assert!(enabled.auto_run_policy.started_at.is_some());
        assert!(enabled.last_audit.is_some());

        let disabled = update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: None,
                second_opinion_provider_entry: None,
                auto_run_policy: Some(ResearchGoalAutoRunPolicyUpdate {
                    enabled: false,
                    cycles_per_run: 2,
                    idle_delay_ms: 1_000,
                    max_elapsed_minutes: Some(30),
                    max_tokens: Some(10_000),
                }),
            },
        )
        .expect("disable auto run");

        assert!(!disabled.auto_run_policy.enabled);
        assert!(disabled.auto_run_policy.started_at.is_none());

        let err = update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: None,
                second_opinion_provider_entry: None,
                auto_run_policy: Some(ResearchGoalAutoRunPolicyUpdate {
                    enabled: true,
                    cycles_per_run: MAX_AUTO_RUN_CYCLES + 1,
                    idle_delay_ms: 1_000,
                    max_elapsed_minutes: None,
                    max_tokens: None,
                }),
            },
        )
        .expect_err("cycles_per_run should be bounded");
        assert!(err.contains("自动续跑每次轮数"));
    }

    #[test]
    fn auto_run_budget_reached_reason_checks_token_and_elapsed_budgets() {
        let mut goal = test_goal();
        assert!(auto_run_budget_reached_reason(&goal).is_none());

        goal.auto_run_policy.enabled = true;
        goal.auto_run_policy.max_tokens = Some(100);
        goal.token_usage.total_tokens = 100;
        assert!(auto_run_budget_reached_reason(&goal)
            .expect("token budget reason")
            .contains("token 预算"));

        goal.auto_run_policy.max_tokens = None;
        goal.auto_run_policy.max_elapsed_minutes = Some(1);
        goal.auto_run_policy.started_at = Some("2000-01-01T00:00:00Z".to_string());
        assert!(auto_run_budget_reached_reason(&goal)
            .expect("elapsed budget reason")
            .contains("最长耗时预算"));
    }

    #[test]
    fn update_goal_second_opinion_provider_persists_and_invalidates_audit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut goal = run_research_goal_command(tmp.path(), "session-1", "研究 QS 核心基因")
            .expect("set goal")
            .goal
            .expect("goal");
        goal.status = ResearchGoalStatus::Complete;
        goal.last_audit = Some(ResearchGoalAudit {
            complete: true,
            review_source: "llm".to_string(),
            confidence: "high".to_string(),
            final_report_ready: true,
            summary: "old audit".to_string(),
            criteria: Vec::new(),
            missing_requirements: Vec::new(),
            next_actions: Vec::new(),
            limitations: Vec::new(),
            conflicting_evidence: Vec::new(),
            second_opinion: None,
        });
        save_goal(&ResearchGoalLayout::new(tmp.path()), &goal).expect("save complete goal");

        let updated = update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: None,
                second_opinion_provider_entry: Some("  goal-second-opinion  ".to_string()),
                auto_run_policy: None,
            },
        )
        .expect("update provider entry");

        assert_eq!(
            updated.second_opinion_provider_entry.as_deref(),
            Some("goal-second-opinion")
        );
        assert_eq!(updated.status, ResearchGoalStatus::Active);
        assert!(updated.last_audit.is_none());

        let cleared = update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: None,
                second_opinion_provider_entry: Some("   ".to_string()),
                auto_run_policy: None,
            },
        )
        .expect("clear provider entry");

        assert!(cleared.second_opinion_provider_entry.is_none());
    }

    #[test]
    fn sync_goal_run_requires_llm_audit_entrypoint() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 2 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");

        let err = run_research_goal_command(tmp.path(), "session-1", "run")
            .expect_err("sync run should reject heuristic audit");
        assert!(err.contains("LLM 完成审计"));
    }

    #[tokio::test]
    async fn goal_run_executes_research_cycle_and_updates_llm_audit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 2 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");
        let client = StaticAuditClient::new_sequence_with_usage(
            vec![llm_audit_json(false)],
            vec![Some(TestLlmTokenUsage {
                prompt_tokens: 11,
                completion_tokens: 7,
                total_tokens: 18,
            })],
        );

        let result =
            run_research_goal_command_with_llm(tmp.path(), "session-1", "run", &client, None)
                .await
                .expect("run goal");
        let goal = result.goal.expect("goal after run");
        let cycle = result.cycle.expect("cycle after run");
        let audit = goal.last_audit.as_ref().expect("llm audit");

        assert_eq!(goal.current_cycle, 1);
        assert_eq!(audit.review_source, "llm");
        assert_eq!(audit.confidence, "medium");
        assert!(!audit.complete);
        assert_eq!(cycle.cycle_index, 1);
        assert!(cycle.request.contains("分析目标还缺什么证据"));
        assert_eq!(cycle.token_usage.audit.input_tokens, 11);
        assert_eq!(cycle.token_usage.audit.output_tokens, 7);
        assert!(cycle.token_usage.research_system.total_tokens > 0);
        assert_eq!(
            goal.token_usage.total_tokens,
            cycle.token_usage.total.total_tokens
        );
        assert!(result.assistant_content.contains("LLM 完成审计"));
        assert_eq!(client.calls(), 1);
    }

    #[tokio::test]
    async fn goal_run_cycles_auto_continues_within_budget_using_llm_audit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 3 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");
        let client = StaticAuditClient::new(llm_audit_json(false));

        let result = run_research_goal_command_with_llm(
            tmp.path(),
            "session-1",
            "run --cycles 2",
            &client,
            None,
        )
        .await
        .expect("auto run");
        let goal = result.goal.expect("goal after auto run");
        let cycle = result.cycle.expect("last cycle after auto run");

        assert_eq!(goal.current_cycle, 2);
        assert_eq!(cycle.cycle_index, goal.current_cycle);
        assert!(result.assistant_content.contains("自动续跑完成"));
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn goal_auto_run_stops_between_cycles_when_token_budget_is_reached() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 3 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");
        update_research_goal_settings(
            tmp.path(),
            "session-1",
            ResearchGoalSettingsUpdate {
                criteria: None,
                max_cycles: None,
                second_opinion_provider_entry: None,
                auto_run_policy: Some(ResearchGoalAutoRunPolicyUpdate {
                    enabled: true,
                    cycles_per_run: 3,
                    idle_delay_ms: 1_000,
                    max_elapsed_minutes: None,
                    max_tokens: Some(1),
                }),
            },
        )
        .expect("enable auto run with tiny token budget");
        let client = StaticAuditClient::new_sequence_with_usage(
            vec![llm_audit_json(false), llm_audit_json(false)],
            vec![
                Some(TestLlmTokenUsage {
                    prompt_tokens: 11,
                    completion_tokens: 7,
                    total_tokens: 18,
                }),
                Some(TestLlmTokenUsage {
                    prompt_tokens: 11,
                    completion_tokens: 7,
                    total_tokens: 18,
                }),
            ],
        );

        let result = run_research_goal_command_with_llm(
            tmp.path(),
            "session-1",
            "run --cycles 3",
            &client,
            None,
        )
        .await
        .expect("auto run should stop after budget");
        let goal = result.goal.expect("goal after auto run");

        assert_eq!(goal.current_cycle, 1);
        assert!(!goal.auto_run_policy.enabled);
        assert!(goal.token_usage.total_tokens >= 1);
        assert_eq!(client.calls(), 1);
        assert!(result.assistant_content.contains("自动续跑已停止"));
        assert!(result.assistant_content.contains("token 预算"));
    }

    #[test]
    fn llm_audit_is_hard_blocked_by_failed_research_output() {
        let goal = test_goal();

        let failed = parse_llm_research_goal_audit(
            &llm_audit_json(true),
            &goal,
            &serde_json::json!({"status": "failed"}),
        )
        .expect("parse failed audit");
        assert!(!failed.complete);
        assert!(!failed.missing_requirements.is_empty());

        let passed = parse_llm_research_goal_audit(
            &llm_audit_json(true),
            &goal,
            &serde_json::json!({
                "status": "completed",
                "final_output": {"summary": "ok", "evidence_refs": ["ev-1"]},
                "review_results": {
                    "task-1": {"status": "pass", "final_answer_allowed": true}
                }
            }),
        )
        .expect("parse passed audit");
        assert!(passed.complete);
        assert_eq!(passed.review_source, "llm");
    }

    #[test]
    fn llm_audit_matches_success_criteria_by_stable_ids() {
        let goal = test_goal();
        let criteria = goal_success_criteria_for_audit(&goal)
            .into_iter()
            .map(|criterion| {
                json!({
                    "criterionId": criterion.criterion_id,
                    "criterion": "LLM 可使用简写，不需要逐字复述成功标准",
                    "covered": true,
                    "evidence": "按稳定 criterionId 匹配。",
                })
            })
            .collect::<Vec<_>>();
        let raw = json!({
            "complete": true,
            "summary": "LLM 审计通过：可交付最终科研报告。",
            "confidence": "high",
            "criteria": criteria,
            "missingRequirements": [],
            "nextActions": ["整理最终科研报告"],
            "limitations": [],
            "conflictingEvidence": [],
            "finalReportReady": true,
        })
        .to_string();

        let audit = parse_llm_research_goal_audit(
            &raw,
            &goal,
            &serde_json::json!({
                "status": "completed",
                "final_output": {"summary": "ok", "evidence_refs": ["ev-1"]},
                "review_results": {
                    "task-1": {"status": "pass", "final_answer_allowed": true}
                }
            }),
        )
        .expect("parse id matched audit");

        assert!(audit.complete);
        assert_eq!(audit.criteria[0].criterion, goal.success_criteria[0]);
    }

    #[tokio::test]
    async fn complete_goal_requires_second_opinion_agreement() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 2 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");
        let client = StaticAuditClient::new_sequence(vec![
            llm_audit_json(true),
            llm_second_opinion_json(false),
        ]);

        let result =
            run_research_goal_command_with_llm(tmp.path(), "session-1", "run", &client, None)
                .await
                .expect("run goal");
        let goal = result.goal.expect("goal");
        let audit = goal.last_audit.expect("audit");

        assert_eq!(client.calls(), 2);
        assert_eq!(goal.status, ResearchGoalStatus::Active);
        assert!(!audit.complete);
        assert!(audit.second_opinion.is_some());
        assert!(audit
            .missing_requirements
            .iter()
            .any(|item| item.contains("二次 LLM 审计未同意完成")));
    }

    #[tokio::test]
    async fn complete_goal_passes_when_second_opinion_agrees() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 2 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");
        let client = StaticAuditClient::new_sequence(vec![
            llm_audit_json(true),
            llm_second_opinion_json(true),
        ]);

        let result =
            run_research_goal_command_with_llm(tmp.path(), "session-1", "run", &client, None)
                .await
                .expect("run goal");
        let goal = result.goal.expect("goal");
        let audit = goal.last_audit.expect("audit");

        assert_eq!(client.calls(), 2);
        assert_eq!(goal.status, ResearchGoalStatus::Complete);
        assert!(audit.complete);
        assert!(audit
            .second_opinion
            .as_ref()
            .is_some_and(|opinion| opinion.agrees_complete));
    }

    #[tokio::test]
    async fn second_opinion_can_use_independent_llm_client() {
        let tmp = tempfile::tempdir().expect("tempdir");
        run_research_goal_command(
            tmp.path(),
            "session-1",
            "--max-cycles 2 解析 QS 核心基因的免疫机制",
        )
        .expect("set goal");
        let primary_client = StaticAuditClient::new(llm_audit_json(true));
        let second_client = StaticAuditClient::new(llm_second_opinion_json(false));

        let result = run_research_goal_command_with_llm(
            tmp.path(),
            "session-1",
            "run",
            &primary_client,
            Some(&second_client),
        )
        .await
        .expect("run goal");
        let goal = result.goal.expect("goal");

        assert_eq!(primary_client.calls(), 1);
        assert_eq!(second_client.calls(), 1);
        assert_eq!(goal.status, ResearchGoalStatus::Active);
    }

    #[tokio::test]
    async fn llm_generates_success_criteria_without_heuristic_fallback() {
        let goal = test_goal();
        let client =
            StaticAuditClient::new(format!("```json\n{}\n```", llm_suggested_criteria_json()));

        let criteria = suggest_research_goal_criteria_with_llm(&client, &goal)
            .await
            .expect("criteria suggestions");

        assert_eq!(client.calls(), 1);
        assert_eq!(criteria.len(), 5);
        assert!(criteria.join("\n").contains("冲突证据"));
    }

    #[tokio::test]
    async fn second_opinion_provider_probe_requires_real_llm_json_ack() {
        let client = StaticAuditClient::new(
            r#"```json
{
  "ok": true,
  "role": "research_goal_second_opinion_probe",
  "summary": "我可以作为科研目标二次审计模型。"
}
```"#
                .to_string(),
        );

        let summary = probe_research_goal_second_opinion_provider_with_llm(&client)
            .await
            .expect("probe should parse live JSON ack");

        assert_eq!(client.calls(), 1);
        assert!(summary.contains("二次审计模型"));

        let invalid_client = StaticAuditClient::new("仅本地配置可用".to_string());
        let err = probe_research_goal_second_opinion_provider_with_llm(&invalid_client)
            .await
            .expect_err("probe must not pass without LLM JSON ack");
        assert!(err.contains("JSON object"));
    }

    #[test]
    fn invalid_llm_criteria_output_errors_instead_of_using_heuristics() {
        let err = parse_llm_research_goal_criteria("请考虑证据链和局限性")
            .expect_err("invalid output must not fallback");

        assert!(err.contains("无法使用启发式兜底"));
    }

    #[test]
    fn parses_fenced_llm_json_without_heuristic_fallback() {
        let goal = test_goal();
        let raw = format!("```json\n{}\n```", llm_audit_json(false));
        let audit = parse_llm_research_goal_audit(
            &raw,
            &goal,
            &serde_json::json!({
                "status": "completed",
                "final_output": {"summary": "ok", "evidence_refs": ["ev-1"]},
                "review_results": {
                    "task-1": {"status": "pass", "final_answer_allowed": true}
                }
            }),
        )
        .expect("parse fenced json");

        assert_eq!(audit.review_source, "llm");
        assert!(!audit.complete);
        assert!(audit
            .missing_requirements
            .iter()
            .any(|item| item.contains("独立证据")));
    }
}
