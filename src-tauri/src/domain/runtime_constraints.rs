//! Extensible runtime constraint harness for agent turns.
//!
//! This module is intentionally heuristic and policy-oriented:
//! - constraints can inspect the current request, round state, and pending tools
//! - constraints can inject model-time notices
//! - constraints can gate tool execution before side effects happen
//!
//! The goal is to make it easy to add more constraints later without hard-coding
//! one-off checks throughout the chat loop.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::domain::permissions::canonical_permission_tool_name;
use crate::domain::tools::ask_user_question;
use crate::llm::LlmMessage;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeConstraintRuleConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub severity: Option<ConstraintSeverity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConstraintConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub buffer_responses: bool,
    #[serde(default)]
    pub policy_pack: ConstraintPolicyPack,
    #[serde(default)]
    pub rules: HashMap<String, RuntimeConstraintRuleConfig>,
}

impl Default for RuntimeConstraintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            buffer_responses: true,
            policy_pack: ConstraintPolicyPack::default(),
            rules: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeConstraintConfig {
    pub enabled: bool,
    pub buffer_responses: bool,
    pub policy_pack: ConstraintPolicyPack,
    pub rules: HashMap<String, RuntimeConstraintRuleConfig>,
}

impl Default for ResolvedRuntimeConstraintConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            buffer_responses: true,
            policy_pack: ConstraintPolicyPack::default(),
            rules: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintPolicyPack {
    #[default]
    Balanced,
    CodingStrict,
    ExplanationStrict,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeConstraintState {
    emitted_notice_ids: HashSet<&'static str>,
    post_action_attempts: HashMap<&'static str, usize>,
    seen_tool_names: Vec<String>,
    clarification_requested: bool,
}

impl RuntimeConstraintState {
    pub fn mark_notice_emitted(&mut self, id: &'static str) {
        self.emitted_notice_ids.insert(id);
    }

    pub fn notice_emitted(&self, id: &'static str) -> bool {
        self.emitted_notice_ids.contains(id)
    }

    pub fn emitted_notice_ids(&self) -> Vec<&'static str> {
        self.emitted_notice_ids.iter().copied().collect()
    }

    pub fn post_action_attempts(&self, id: &'static str) -> usize {
        self.post_action_attempts.get(id).copied().unwrap_or(0)
    }

    pub fn mark_post_action_attempted(&mut self, id: &'static str) {
        let entry = self.post_action_attempts.entry(id).or_insert(0);
        *entry += 1;
    }

    pub fn record_tool_names<'a, I>(&mut self, tool_names: I) -> usize
    where
        I: IntoIterator<Item = &'a str>,
    {
        let before = self.seen_tool_names.len();
        for tool_name in tool_names {
            let canonical = canonical_permission_tool_name(tool_name);
            if canonical == "ask_user_question" {
                self.clarification_requested = true;
            }
            self.seen_tool_names.push(canonical);
        }
        self.seen_tool_names.len().saturating_sub(before)
    }

    pub fn mark_clarification_requested(&mut self) {
        self.clarification_requested = true;
    }

    pub fn clarification_requested(&self) -> bool {
        self.clarification_requested
    }

    pub fn has_used_retrieval_tool(&self) -> bool {
        self.seen_tool_names.iter().any(|name| {
            matches!(
                name.as_str(),
                "glob"
                    | "grep"
                    | "ripgrep"
                    | "file_read"
                    | "fetch"
                    | "search"
                    | "list_skills"
                    | "skills_list"
                    | "skill_view"
                    | "tool_search"
                    | "list_mcp_resources"
                    | "read_mcp_resource"
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintSeverity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintPhase {
    ModelNotice,
    ToolGate,
    PostResponse,
}

#[derive(Debug, Clone)]
pub struct ConstraintMetadata {
    pub id: &'static str,
    pub description: &'static str,
    pub severity: ConstraintSeverity,
    pub phases: &'static [ConstraintPhase],
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ModelConstraintContext<'a> {
    pub request_text: &'a str,
    pub project_root: &'a Path,
    pub use_tools: bool,
    pub is_subagent: bool,
}

#[derive(Debug, Clone)]
pub struct ToolConstraintContext<'a> {
    pub request_text: &'a str,
    pub assistant_text: &'a str,
    pub pending_tool_names: &'a [String],
    pub is_subagent: bool,
}

#[derive(Debug, Clone)]
pub struct PostResponseConstraintContext<'a> {
    pub request_text: &'a str,
    pub assistant_text: &'a str,
    pub pending_tool_names: &'a [String],
    pub is_subagent: bool,
}

#[derive(Debug, Clone)]
pub struct ConstraintNotice {
    pub id: &'static str,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ConstraintToolBlock {
    pub id: &'static str,
    pub tool_result_message: String,
    pub assistant_response: String,
    pub interactive_question: Option<ask_user_question::AskUserQuestionArgs>,
    pub post_answer_response: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConstraintPostAction {
    pub id: &'static str,
    pub instruction: String,
    pub max_attempts: usize,
}

pub trait RuntimeConstraint: Send + Sync {
    fn metadata(&self) -> ConstraintMetadata;

    fn model_notice(
        &self,
        _ctx: &ModelConstraintContext<'_>,
        _state: &RuntimeConstraintState,
    ) -> Option<ConstraintNotice> {
        None
    }

    fn tool_gate(
        &self,
        _ctx: &ToolConstraintContext<'_>,
        _state: &RuntimeConstraintState,
    ) -> Option<ConstraintToolBlock> {
        None
    }

    fn post_response_block(
        &self,
        _ctx: &PostResponseConstraintContext<'_>,
        _state: &RuntimeConstraintState,
    ) -> Option<ConstraintToolBlock> {
        None
    }

    fn post_response_action(
        &self,
        _ctx: &PostResponseConstraintContext<'_>,
        _state: &RuntimeConstraintState,
    ) -> Option<ConstraintPostAction> {
        None
    }
}

struct ConstraintRegistration {
    metadata: ConstraintMetadata,
    implementation: Box<dyn RuntimeConstraint>,
}

pub struct RuntimeConstraintHarness {
    constraints: Vec<ConstraintRegistration>,
    resolved_config: ResolvedRuntimeConstraintConfig,
}

impl Default for RuntimeConstraintHarness {
    fn default() -> Self {
        Self::from_config(ResolvedRuntimeConstraintConfig::default())
    }
}

impl RuntimeConstraintHarness {
    pub fn from_config(config: ResolvedRuntimeConstraintConfig) -> Self {
        let mut constraints = vec![
            register_constraint(EvidenceFirstConstraint),
            register_constraint(ClarificationFirstConstraint),
            register_constraint(LargeOutputConstraint),
        ];
        apply_policy_pack_overrides(&mut constraints, config.policy_pack);
        for registration in &mut constraints {
            if let Some(rule) = config.rules.get(registration.metadata.id) {
                if let Some(enabled) = rule.enabled {
                    registration.metadata.enabled = enabled;
                }
                if let Some(severity) = rule.severity {
                    registration.metadata.severity = severity;
                }
            }
        }
        Self {
            constraints,
            resolved_config: config,
        }
    }

    pub fn registry(&self) -> Vec<ConstraintMetadata> {
        self.constraints
            .iter()
            .map(|r| r.metadata.clone())
            .collect()
    }

    pub fn resolved_config(&self) -> &ResolvedRuntimeConstraintConfig {
        &self.resolved_config
    }

    pub fn augment_model_messages(
        &self,
        base_messages: &[LlmMessage],
        ctx: &ModelConstraintContext<'_>,
        state: &mut RuntimeConstraintState,
    ) -> Vec<LlmMessage> {
        let mut out = base_messages.to_vec();
        if !self.resolved_config.enabled {
            return out;
        }
        for registration in &self.constraints {
            if !registration.metadata.enabled {
                continue;
            }
            let id = registration.metadata.id;
            if state.notice_emitted(id) {
                continue;
            }
            if let Some(notice) = registration.implementation.model_notice(ctx, state) {
                out.push(LlmMessage::system(notice.text));
                state.mark_notice_emitted(notice.id);
            }
        }
        out
    }

    pub fn tool_gate(
        &self,
        ctx: &ToolConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintToolBlock> {
        if !self.resolved_config.enabled {
            return None;
        }
        self.constraints
            .iter()
            .filter(|registration| registration.metadata.enabled)
            .find_map(|registration| registration.implementation.tool_gate(ctx, state))
    }

    pub fn post_response_action(
        &self,
        ctx: &PostResponseConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintPostAction> {
        if !self.resolved_config.enabled {
            return None;
        }
        self.constraints
            .iter()
            .filter(|registration| registration.metadata.enabled)
            .find_map(|registration| {
                let id = registration.metadata.id;
                let action = registration
                    .implementation
                    .post_response_action(ctx, state)?;
                if state.post_action_attempts(id) >= action.max_attempts {
                    return None;
                }
                Some(action)
            })
    }

    pub fn post_response_block(
        &self,
        ctx: &PostResponseConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintToolBlock> {
        if !self.resolved_config.enabled {
            return None;
        }
        self.constraints
            .iter()
            .filter(|registration| registration.metadata.enabled)
            .find_map(|registration| registration.implementation.post_response_block(ctx, state))
    }
}

fn register_constraint<T: RuntimeConstraint + 'static>(constraint: T) -> ConstraintRegistration {
    let metadata = constraint.metadata();
    ConstraintRegistration {
        metadata,
        implementation: Box::new(constraint),
    }
}

fn apply_policy_pack_overrides(
    constraints: &mut [ConstraintRegistration],
    policy_pack: ConstraintPolicyPack,
) {
    for registration in constraints {
        match (policy_pack, registration.metadata.id) {
            (ConstraintPolicyPack::Balanced, _) => {}
            (ConstraintPolicyPack::CodingStrict, "clarification_first") => {
                registration.metadata.severity = ConstraintSeverity::Error;
                registration.metadata.enabled = true;
            }
            (ConstraintPolicyPack::CodingStrict, "evidence_first") => {
                registration.metadata.severity = ConstraintSeverity::Warn;
                registration.metadata.enabled = true;
            }
            (ConstraintPolicyPack::ExplanationStrict, "evidence_first") => {
                registration.metadata.severity = ConstraintSeverity::Error;
                registration.metadata.enabled = true;
            }
            (ConstraintPolicyPack::ExplanationStrict, "clarification_first") => {
                registration.metadata.severity = ConstraintSeverity::Warn;
                registration.metadata.enabled = true;
            }
            _ => {}
        }
    }
}

fn config_path(project_root: &Path) -> std::path::PathBuf {
    project_root.join(".omiga").join("runtime_constraints.yaml")
}

pub fn save_project_runtime_constraint_config(
    project_root: &Path,
    config: &RuntimeConstraintConfig,
) -> Result<(), String> {
    let path = config_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create runtime constraint config dir: {}", e))?;
    }
    let content = serde_yaml::to_string(config)
        .map_err(|e| format!("Failed to serialize runtime constraint config: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write runtime constraint config: {}", e))
}

pub fn load_project_runtime_constraint_config(project_root: &Path) -> RuntimeConstraintConfig {
    let path = config_path(project_root);
    if !path.exists() {
        return RuntimeConstraintConfig::default();
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "Failed to read runtime constraint config at {}: {}",
                path.display(),
                e
            );
            return RuntimeConstraintConfig::default();
        }
    };
    match serde_yaml::from_str(&content) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!(
                "Failed to parse runtime constraint config at {}: {}",
                path.display(),
                e
            );
            RuntimeConstraintConfig::default()
        }
    }
}

pub fn resolve_runtime_constraint_config(
    project_root: &Path,
    session_override: Option<&RuntimeConstraintConfig>,
) -> ResolvedRuntimeConstraintConfig {
    let mut resolved = ResolvedRuntimeConstraintConfig::default();
    let project = load_project_runtime_constraint_config(project_root);
    resolved.enabled = project.enabled;
    resolved.buffer_responses = project.buffer_responses;
    resolved.policy_pack = project.policy_pack;
    resolved.rules.extend(project.rules);

    if let Some(session) = session_override {
        resolved.enabled = session.enabled;
        resolved.buffer_responses = session.buffer_responses;
        resolved.policy_pack = session.policy_pack;
        for (key, value) in &session.rules {
            resolved.rules.insert(key.clone(), value.clone());
        }
    }

    resolved
}

struct EvidenceFirstConstraint;

impl RuntimeConstraint for EvidenceFirstConstraint {
    fn metadata(&self) -> ConstraintMetadata {
        ConstraintMetadata {
            id: "evidence_first",
            description: "Require retrieval before confident factual / explanatory answers.",
            severity: ConstraintSeverity::Warn,
            phases: &[ConstraintPhase::ModelNotice, ConstraintPhase::PostResponse],
            enabled: true,
        }
    }

    fn model_notice(
        &self,
        ctx: &ModelConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintNotice> {
        if !ctx.use_tools || state.has_used_retrieval_tool() {
            return None;
        }
        if !looks_like_evidence_needed_request(ctx.request_text) {
            return None;
        }

        Some(ConstraintNotice {
            id: self.metadata().id,
            text: format!(
                "## Runtime constraint: evidence first\n\
                 This {}request looks factual / explanatory / architecture-sensitive. \
                 Retrieve evidence before answering. Prefer project files, memory/wiki, docs, \
                 or other read-only tools first. In the final answer, clearly separate verified \
                 facts from inference.",
                if ctx.is_subagent { "sub-agent " } else { "" }
            ),
        })
    }

    fn post_response_action(
        &self,
        ctx: &PostResponseConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintPostAction> {
        if !looks_like_evidence_needed_request(ctx.request_text)
            || state.has_used_retrieval_tool()
            || !ctx.pending_tool_names.is_empty()
            || assistant_shows_uncertainty(ctx.assistant_text)
        {
            return None;
        }

        Some(ConstraintPostAction {
            id: self.metadata().id,
            instruction: "Your previous answer appears to address a factual / explanatory question \
                          without first retrieving evidence. Write a short correction that is honest \
                          about the missing verification, clearly marks any uncertainty, and invites \
                          the user to let you inspect the codebase or docs before giving a stronger answer."
                .to_string(),
            max_attempts: 1,
        })
    }
}

struct ClarificationFirstConstraint;

impl RuntimeConstraint for ClarificationFirstConstraint {
    fn metadata(&self) -> ConstraintMetadata {
        ConstraintMetadata {
            id: "clarification_first",
            description: "Block risky side effects when the request is materially ambiguous.",
            severity: ConstraintSeverity::Error,
            phases: &[
                ConstraintPhase::ModelNotice,
                ConstraintPhase::ToolGate,
                ConstraintPhase::PostResponse,
            ],
            enabled: true,
        }
    }

    fn model_notice(
        &self,
        ctx: &ModelConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintNotice> {
        if !ctx.use_tools || state.clarification_requested() {
            return None;
        }
        if !looks_like_materially_ambiguous(ctx.request_text) {
            return None;
        }

        Some(ConstraintNotice {
            id: self.metadata().id,
            text: "## Runtime constraint: clarify before acting\n\
                   The current request appears materially ambiguous. Do not guess. \
                   Call `ask_user_question` to clarify scope, target files/modules, or \
                   success criteria before taking side-effectful action; do not ask the \
                   clarification only in plain assistant text."
                .to_string(),
        })
    }

    fn tool_gate(
        &self,
        ctx: &ToolConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintToolBlock> {
        if state.clarification_requested() || !looks_like_materially_ambiguous(ctx.request_text) {
            return None;
        }
        if !ctx
            .pending_tool_names
            .iter()
            .any(|name| is_side_effectful_tool(name))
        {
            return None;
        }

        Some(build_clarification_first_block(
            ctx.request_text,
            "Blocked by runtime constraint `clarification_first`: the request is materially ambiguous, so side-effectful tools cannot run until the user clarifies the scope."
                .to_string(),
        ))
    }

    fn post_response_block(
        &self,
        ctx: &PostResponseConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintToolBlock> {
        if ctx.is_subagent
            || state.clarification_requested()
            || !looks_like_materially_ambiguous(ctx.request_text)
            || !ctx.pending_tool_names.is_empty()
        {
            return None;
        }

        Some(build_clarification_first_block(
            ctx.request_text,
            "Blocked by runtime constraint `clarification_first`: the request is materially ambiguous, so the assistant must use `ask_user_question` instead of asking for clarification in plain text."
                .to_string(),
        ))
    }

    fn post_response_action(
        &self,
        ctx: &PostResponseConstraintContext<'_>,
        state: &RuntimeConstraintState,
    ) -> Option<ConstraintPostAction> {
        if state.clarification_requested()
            || !looks_like_materially_ambiguous(ctx.request_text)
            || !ctx.pending_tool_names.is_empty()
            || assistant_is_clarifying(ctx.assistant_text)
        {
            return None;
        }

        Some(ConstraintPostAction {
            id: self.metadata().id,
            instruction:
                "Your previous answer responded to a materially ambiguous request without \
                          first clarifying it. Write a concise follow-up that asks exactly one \
                          clarification question instead of guessing or proceeding."
                    .to_string(),
            max_attempts: 1,
        })
    }
}

fn build_clarification_first_block(
    request_text: &str,
    tool_result_message: String,
) -> ConstraintToolBlock {
    let scope_hint = if has_specific_anchor(request_text) {
        "the exact requirement or acceptance criteria"
    } else {
        "the target scope (files/modules/behavior) and desired outcome"
    };
    let assistant_response = format!(
        "I need one clarification before I make changes: the current request is broad enough \
         that different implementations could produce different results. Please specify {}, \
         and then I can continue.",
        scope_hint
    );
    let structured_question = ask_user_question::AskUserQuestionArgs {
        questions: vec![ask_user_question::QuestionItem {
            question: "Which missing detail should we pin down first before making changes?"
                .to_string(),
            header: "Clarify".to_string(),
            multi_select: false,
            options: vec![
                ask_user_question::QuestionOption {
                    label: "Scope".to_string(),
                    description:
                        "Clarify the exact file, module, component, or surface that should change."
                            .to_string(),
                    preview: None,
                },
                ask_user_question::QuestionOption {
                    label: "Outcome".to_string(),
                    description: "Clarify the exact behavior or result you want after the change."
                        .to_string(),
                    preview: None,
                },
                ask_user_question::QuestionOption {
                    label: "Guardrails".to_string(),
                    description: "Clarify constraints, risks, or what must not change.".to_string(),
                    preview: None,
                },
            ],
        }],
        answers: None,
        annotations: None,
        metadata: Some(serde_json::json!({
            "source": "runtime_constraint",
            "constraint_id": "clarification_first",
        })),
    };

    ConstraintToolBlock {
        id: "clarification_first",
        tool_result_message,
        assistant_response,
        interactive_question: Some(structured_question),
        post_answer_response: Some(
            "Thanks — now please reply in free text with the concrete detail you selected. \
             Include exact file/module names, desired behavior, and any constraints that matter."
                .to_string(),
        ),
    }
}

struct LargeOutputConstraint;

impl RuntimeConstraint for LargeOutputConstraint {
    fn metadata(&self) -> ConstraintMetadata {
        ConstraintMetadata {
            id: "large_output_discipline",
            description: "Warn when the likely answer should be moved into a file instead of chat.",
            severity: ConstraintSeverity::Info,
            phases: &[ConstraintPhase::ModelNotice],
            enabled: true,
        }
    }

    fn model_notice(
        &self,
        ctx: &ModelConstraintContext<'_>,
        _state: &RuntimeConstraintState,
    ) -> Option<ConstraintNotice> {
        if !ctx.use_tools
            || looks_like_deliverable_request(ctx.request_text)
            || !looks_like_large_output_risk(ctx.request_text)
        {
            return None;
        }

        Some(ConstraintNotice {
            id: self.metadata().id,
            text: "## Runtime constraint: long output discipline\n\
                   If the result will be long, prefer writing the full artifact to a file and \
                   returning a concise summary plus the path. Avoid dumping oversized walls of \
                   text directly into chat unless the user explicitly asked for the final document \
                   inline."
                .to_string(),
        })
    }
}

fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn has_specific_anchor(text: &str) -> bool {
    let t = normalize(text);
    text.contains('`')
        || contains_any(
            &t,
            &[
                "src/",
                "src-tauri/",
                ".rs",
                ".ts",
                ".tsx",
                ".js",
                ".jsx",
                ".json",
                ".toml",
                ".yaml",
                ".yml",
                ".md",
                " file ",
                " files ",
                " path ",
                " paths ",
                " module ",
                " modules ",
                " function ",
                " functions ",
                " component ",
                " components ",
                " class ",
                " classes ",
                "文件",
                "函数",
                "组件",
                "模块",
                "类",
                "路径",
            ],
        )
}

fn looks_like_evidence_needed_request(text: &str) -> bool {
    let t = normalize(text);
    text.contains('?')
        || contains_any(
            &t,
            &[
                "how ",
                "how do",
                "how does",
                "why ",
                "what ",
                "which ",
                "where ",
                "explain",
                "architecture",
                "data flow",
                "root cause",
                "reason about",
                "how should",
                "how to",
                "怎么",
                "如何",
                "为什么",
                "原理",
                "架构",
                "数据流",
                "是怎么",
                "怎么做",
            ],
        )
}

fn looks_like_materially_ambiguous(text: &str) -> bool {
    let t = normalize(text);
    let has_vague_pointer = contains_any(
        &t,
        &[
            "fix it",
            "improve it",
            "optimize it",
            "refactor it",
            "clean it up",
            "make it better",
            "fix this",
            "improve this",
            "optimize this",
            "refactor this",
            "clean this up",
            "that part",
            "this part",
            "这个",
            "这个地方",
            "这里",
            "那个",
            "那块",
            "修一下",
            "改一下",
            "优化一下",
            "重构一下",
            "整理一下",
            "弄一下",
        ],
    );
    let broad_action = contains_any(
        &t,
        &[
            "fix",
            "improve",
            "optimize",
            "refactor",
            "clean",
            "rewrite",
            "support",
            "update",
            "add",
            "implement",
            "修复",
            "优化",
            "重构",
            "清理",
            "整理",
            "改造",
            "新增",
            "实现",
            "支持",
        ],
    );

    (has_vague_pointer && broad_action && !has_specific_anchor(text))
        || t.len() < 24 && has_vague_pointer
}

fn looks_like_deliverable_request(text: &str) -> bool {
    let t = normalize(text);
    contains_any(
        &t,
        &[
            "itinerary",
            "travel plan",
            "day-by-day",
            "report for me",
            "write the full report",
            "proposal",
            "schedule",
            "旅",
            "行程",
            "攻略",
            "计划书",
            "报告正文",
            "完整文档",
        ],
    )
}

fn looks_like_large_output_risk(text: &str) -> bool {
    let t = normalize(text);
    contains_any(
        &t,
        &[
            "comprehensive",
            "detailed",
            "full report",
            "deep dive",
            "all files",
            "complete analysis",
            "详细",
            "全面",
            "完整分析",
            "完整报告",
            "深度分析",
            "所有文件",
            "全部整理",
        ],
    )
}

fn assistant_shows_uncertainty(text: &str) -> bool {
    let t = normalize(text);
    contains_any(
        &t,
        &[
            "i'm not sure",
            "i am not sure",
            "uncertain",
            "i haven't verified",
            "i have not verified",
            "i haven't checked",
            "not verified",
            "need to inspect",
            "would need to inspect",
            "我不确定",
            "尚未确认",
            "还没验证",
            "需要检查",
            "需要确认",
        ],
    )
}

fn assistant_is_clarifying(text: &str) -> bool {
    let t = normalize(text);
    text.contains('?')
        || contains_any(
            &t,
            &[
                "please specify",
                "can you clarify",
                "which one",
                "what exactly",
                "before i proceed",
                "clarify",
                "请说明",
                "请明确",
                "请补充",
                "请确认",
            ],
        )
}

fn is_side_effectful_tool(tool_name: &str) -> bool {
    matches!(
        canonical_permission_tool_name(tool_name).as_str(),
        "bash"
            | "file_edit"
            | "file_write"
            | "notebook_edit"
            | "skill"
            | "skill_manage"
            | "agent"
            | "task_create"
            | "task_update"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_notice_triggers_for_how_question() {
        let harness = RuntimeConstraintHarness::default();
        let mut state = RuntimeConstraintState::default();
        let base = vec![LlmMessage::user("How does auth work in this repo?")];
        let out = harness.augment_model_messages(
            &base,
            &ModelConstraintContext {
                request_text: "How does auth work in this repo?",
                project_root: Path::new("/tmp"),
                use_tools: true,
                is_subagent: false,
            },
            &mut state,
        );

        assert_eq!(out.len(), 2);
        assert!(out[1].text_content().contains("evidence first"));
    }

    #[test]
    fn clarification_gate_blocks_ambiguous_mutation() {
        let harness = RuntimeConstraintHarness::default();
        let state = RuntimeConstraintState::default();
        let tool_names = vec!["file_edit".to_string()];

        let block = harness.tool_gate(
            &ToolConstraintContext {
                request_text: "Please optimize this.",
                assistant_text: "",
                pending_tool_names: &tool_names,
                is_subagent: false,
            },
            &state,
        );

        assert!(block.is_some());
        let block = block.unwrap();
        assert_eq!(block.id, "clarification_first");
        assert!(block.assistant_response.contains("clarification"));
        assert!(block.interactive_question.is_some());
    }

    #[test]
    fn clarification_post_response_prefers_interactive_ask_over_plain_text() {
        let harness = RuntimeConstraintHarness::default();
        let state = RuntimeConstraintState::default();
        let pending = Vec::<String>::new();

        let block = harness.post_response_block(
            &PostResponseConstraintContext {
                request_text: "Please improve this.",
                assistant_text: "Which file or module should I improve first?",
                pending_tool_names: &pending,
                is_subagent: false,
            },
            &state,
        );

        assert!(block.is_some());
        let block = block.unwrap();
        assert_eq!(block.id, "clarification_first");
        assert!(block.tool_result_message.contains("ask_user_question"));
        assert!(block.interactive_question.is_some());
    }

    #[test]
    fn clarification_post_response_does_not_block_subagents() {
        let harness = RuntimeConstraintHarness::default();
        let state = RuntimeConstraintState::default();
        let pending = Vec::<String>::new();

        let block = harness.post_response_block(
            &PostResponseConstraintContext {
                request_text: "Please improve this.",
                assistant_text: "Which file or module should I improve first?",
                pending_tool_names: &pending,
                is_subagent: true,
            },
            &state,
        );

        assert!(block.is_none());
    }

    #[test]
    fn clarification_gate_allows_specific_request() {
        let harness = RuntimeConstraintHarness::default();
        let state = RuntimeConstraintState::default();
        let tool_names = vec!["file_edit".to_string()];

        let block = harness.tool_gate(
            &ToolConstraintContext {
                request_text: "Refactor src/main.rs to extract the auth bootstrap into a helper.",
                assistant_text: "",
                pending_tool_names: &tool_names,
                is_subagent: false,
            },
            &state,
        );

        assert!(block.is_none());
    }

    #[test]
    fn large_output_notice_skips_deliverables() {
        let harness = RuntimeConstraintHarness::default();
        let mut state = RuntimeConstraintState::default();
        let base = vec![LlmMessage::user("Write a day-by-day itinerary for Kyoto.")];
        let out = harness.augment_model_messages(
            &base,
            &ModelConstraintContext {
                request_text: "Write a day-by-day itinerary for Kyoto.",
                project_root: Path::new("/tmp"),
                use_tools: true,
                is_subagent: false,
            },
            &mut state,
        );

        assert_eq!(out.len(), 1);
    }

    #[test]
    fn registry_contains_expected_constraints() {
        let harness = RuntimeConstraintHarness::default();
        let ids: Vec<_> = harness.registry().into_iter().map(|m| m.id).collect();
        assert!(ids.contains(&"evidence_first"));
        assert!(ids.contains(&"clarification_first"));
        assert!(ids.contains(&"large_output_discipline"));
    }

    #[test]
    fn post_response_requests_retry_for_unverified_answer() {
        let harness = RuntimeConstraintHarness::default();
        let state = RuntimeConstraintState::default();
        let pending = Vec::<String>::new();

        let action = harness.post_response_action(
            &PostResponseConstraintContext {
                request_text: "How does auth work here?",
                assistant_text:
                    "Auth works by checking the token in middleware and then loading the user.",
                pending_tool_names: &pending,
                is_subagent: false,
            },
            &state,
        );

        assert!(action.is_some());
        let action = action.unwrap();
        assert_eq!(action.id, "evidence_first");
        assert!(action
            .instruction
            .contains("without first retrieving evidence"));
    }

    #[test]
    fn harness_applies_config_overrides() {
        let mut cfg = ResolvedRuntimeConstraintConfig {
            buffer_responses: false,
            ..Default::default()
        };
        cfg.rules.insert(
            "clarification_first".to_string(),
            RuntimeConstraintRuleConfig {
                enabled: Some(false),
                severity: Some(ConstraintSeverity::Warn),
            },
        );

        let harness = RuntimeConstraintHarness::from_config(cfg);
        let registry = harness.registry();
        let clarification = registry
            .iter()
            .find(|m| m.id == "clarification_first")
            .expect("clarification rule");

        assert!(!clarification.enabled);
        assert_eq!(clarification.severity, ConstraintSeverity::Warn);
        assert!(!harness.resolved_config().buffer_responses);
    }
}
