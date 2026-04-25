use crate::domain::pageindex::{derive_query_terms, score_terms_against_text};
use crate::domain::persistence::SessionRepository;
use crate::domain::session::Message;
use crate::errors::ApiError;
use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_TOKENS: usize = 1_000;
const APPROX_CHARS_PER_TOKEN: usize = 4;
const HOUSEKEEP_TURN_INTERVAL: u32 = 8;
const HOUSEKEEP_MINUTES: i64 = 25;
const LONG_REPLY_REFRESH_CHARS: usize = 600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkingMemoryItemStatus {
    Active,
    Resolved,
    Replaced,
    Expired,
}

impl Default for WorkingMemoryItemStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkingMemoryItem {
    pub text: String,
    #[serde(default)]
    pub source_message_ids: Vec<String>,
    pub confidence: f32,
    pub last_touched_turn: u32,
    pub expires_after_turns: u32,
    #[serde(default)]
    pub status: WorkingMemoryItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default)]
    pub touch_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkingMemoryState {
    pub session_goal: Option<String>,
    pub active_topic: Option<String>,
    #[serde(default)]
    pub decisions: Vec<WorkingMemoryItem>,
    #[serde(default)]
    pub constraints: Vec<WorkingMemoryItem>,
    #[serde(default)]
    pub working_facts: Vec<WorkingMemoryItem>,
    #[serde(default)]
    pub open_questions: Vec<WorkingMemoryItem>,
    #[serde(default)]
    pub artifacts: Vec<WorkingMemoryItem>,
    #[serde(default)]
    pub next_steps: Vec<WorkingMemoryItem>,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub user_turn_count: u32,
    #[serde(default)]
    pub last_refreshed_turn: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorkingMemoryStatus {
    pub enabled: bool,
    pub dirty: bool,
    pub item_count: usize,
    pub last_refreshed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkingMemoryDraft {
    #[serde(default)]
    session_goal: Option<String>,
    #[serde(default)]
    active_topic: Option<String>,
    #[serde(default)]
    decisions: Vec<WorkingMemoryDraftItem>,
    #[serde(default)]
    constraints: Vec<WorkingMemoryDraftItem>,
    #[serde(default)]
    working_facts: Vec<WorkingMemoryDraftItem>,
    #[serde(default)]
    open_questions: Vec<WorkingMemoryDraftItem>,
    #[serde(default)]
    artifacts: Vec<WorkingMemoryDraftItem>,
    #[serde(default)]
    next_steps: Vec<WorkingMemoryDraftItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkingMemoryDraftItem {
    text: String,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    expires_after_turns: Option<u32>,
    #[serde(default)]
    status: Option<WorkingMemoryItemStatus>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    source_message_ids: Vec<String>,
}

pub async fn load_state(
    repo: &SessionRepository,
    session_id: &str,
) -> Result<WorkingMemoryState, sqlx::Error> {
    Ok(repo
        .get_session_working_memory(session_id)
        .await?
        .unwrap_or_default())
}

pub async fn mark_user_turn_started(
    repo: &SessionRepository,
    session_id: &str,
) -> Result<WorkingMemoryState, sqlx::Error> {
    let mut state = load_state(repo, session_id).await?;
    state.user_turn_count = state.user_turn_count.saturating_add(1);
    state.dirty = true;
    state.updated_at = Some(now_rfc3339());
    if should_housekeep(&state) {
        cleanup_state(&mut state);
    }
    repo.upsert_session_working_memory(session_id, &state)
        .await?;
    Ok(state)
}

pub async fn prepare_for_auto_compact(
    repo: &SessionRepository,
    session_id: &str,
    messages: &[Message],
) -> Result<WorkingMemoryState, sqlx::Error> {
    let mut state = load_state(repo, session_id).await?;
    if state.session_goal.is_none() || state.item_count() == 0 {
        bootstrap_from_messages(&mut state, messages);
    }
    summarize_pre_compaction_tail(&mut state, messages);
    cleanup_state(&mut state);
    state.updated_at = Some(now_rfc3339());
    repo.upsert_session_working_memory(session_id, &state)
        .await?;
    Ok(state)
}

pub async fn sync_after_turn(
    repo: &SessionRepository,
    session_id: &str,
    client: &dyn LlmClient,
    user_message: &str,
    assistant_reply: &str,
) -> Result<WorkingMemoryState, String> {
    let previous = load_state(repo, session_id)
        .await
        .map_err(|e| format!("load working memory: {e}"))?;
    let should_refresh = should_refresh_after_turn(&previous, user_message, assistant_reply);
    let source_message_ids = collect_recent_source_ids(repo, session_id)
        .await
        .map_err(|e| format!("collect recent source ids: {e}"))?;
    let mut merged = if should_refresh {
        let draft = extract_draft(client, &previous, user_message, assistant_reply)
            .await
            .unwrap_or_else(|_| heuristic_draft(user_message, assistant_reply));
        apply_draft(previous, draft, &source_message_ids)
    } else {
        let mut carried = previous;
        if carried.session_goal.is_none() && !user_message.trim().is_empty() {
            carried.session_goal = Some(truncate_chars(user_message.trim(), 220));
        }
        if carried.active_topic.is_none() && !user_message.trim().is_empty() {
            carried.active_topic = derive_topic(user_message.trim());
        }
        carried
    };
    merged.dirty = !should_refresh;
    if should_refresh {
        let refreshed_at = now_rfc3339();
        merged.last_refreshed_turn = merged.user_turn_count;
        merged.last_refreshed_at = Some(refreshed_at.clone());
        merged.updated_at = Some(refreshed_at);
    } else {
        merged.updated_at = Some(now_rfc3339());
    }
    cleanup_state(&mut merged);
    repo.upsert_session_working_memory(session_id, &merged)
        .await
        .map_err(|e| format!("save working memory: {e}"))?;
    Ok(merged)
}

pub async fn render_context(
    repo: &SessionRepository,
    session_id: &str,
    query: &str,
    max_tokens: usize,
) -> Result<Option<String>, sqlx::Error> {
    let state = load_state(repo, session_id).await?;
    Ok(state.render_for_prompt(query, max_tokens))
}

impl WorkingMemoryState {
    pub fn item_count(&self) -> usize {
        [
            self.decisions.len(),
            self.constraints.len(),
            self.working_facts.len(),
            self.open_questions.len(),
            self.artifacts.len(),
            self.next_steps.len(),
        ]
        .into_iter()
        .sum()
    }

    pub fn status(&self) -> WorkingMemoryStatus {
        WorkingMemoryStatus {
            enabled: self.session_goal.is_some() || self.item_count() > 0,
            dirty: self.dirty,
            item_count: self.item_count(),
            last_refreshed_at: self.last_refreshed_at.clone(),
        }
    }

    pub fn render_for_prompt(&self, query: &str, max_tokens: usize) -> Option<String> {
        let max_chars = max_tokens
            .saturating_mul(APPROX_CHARS_PER_TOKEN)
            .clamp(2_000, 6_000);
        let query_terms = derive_query_terms(query);
        let mut out = String::from("## Working Memory (session scratchpad)\n\n");
        let mut has_any = false;

        if let Some(goal) = self.session_goal.as_ref().filter(|s| !s.trim().is_empty()) {
            let section = format!("### Session Goal\n- {goal}\n\n");
            if out.chars().count().saturating_add(section.chars().count()) <= max_chars {
                out.push_str(&section);
                has_any = true;
            }
        }

        if let Some(topic) = self.active_topic.as_ref().filter(|s| !s.trim().is_empty()) {
            let section = format!("### Active Topic\n- {topic}\n\n");
            if out.chars().count().saturating_add(section.chars().count()) <= max_chars {
                out.push_str(&section);
                has_any = true;
            }
        }

        for (heading, items) in [
            (
                "Decisions",
                select_relevant_items(&self.decisions, &query_terms, 4),
            ),
            (
                "Constraints",
                select_relevant_items(&self.constraints, &query_terms, 4),
            ),
            (
                "Working Facts",
                select_relevant_items(&self.working_facts, &query_terms, 4),
            ),
            (
                "Open Questions",
                select_relevant_items(&self.open_questions, &query_terms, 4),
            ),
            (
                "Artifacts",
                select_relevant_items(&self.artifacts, &query_terms, 4),
            ),
            (
                "Next Steps",
                select_relevant_items(&self.next_steps, &query_terms, 4),
            ),
        ] {
            if items.is_empty() {
                continue;
            }
            let mut section = format!("### {heading}\n");
            for item in items {
                section.push_str("- ");
                section.push_str(&item.text);
                section.push('\n');
            }
            section.push('\n');
            if out.chars().count().saturating_add(section.chars().count()) > max_chars {
                break;
            }
            out.push_str(&section);
            has_any = true;
        }

        has_any.then_some(out.trim().to_string())
    }
}

fn select_relevant_items<'a>(
    items: &'a [WorkingMemoryItem],
    query_terms: &[String],
    limit: usize,
) -> Vec<&'a WorkingMemoryItem> {
    let mut scored: Vec<(&WorkingMemoryItem, f64)> = items
        .iter()
        .filter(|item| item.status == WorkingMemoryItemStatus::Active)
        .map(|item| {
            let mut score = f64::from(item.confidence);
            if !query_terms.is_empty() {
                score += score_terms_against_text(&item.text, query_terms);
            }
            if item.touch_count > 1 {
                score += 0.1;
            }
            (item, score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored.into_iter().map(|(item, _)| item).collect()
}

fn bootstrap_from_messages(state: &mut WorkingMemoryState, messages: &[Message]) {
    if let Some(last_user) = messages.iter().rev().find_map(|message| match message {
        Message::User { content } => Some(content.trim().to_string()),
        _ => None,
    }) {
        if state.session_goal.is_none() {
            state.session_goal = Some(truncate_chars(&last_user, 220));
        }
        if state.active_topic.is_none() {
            state.active_topic = derive_topic(&last_user);
        }
    }

    if let Some(summary) = messages.iter().rev().find_map(|message| match message {
        Message::Assistant { turn_summary, .. } => turn_summary.clone(),
        _ => None,
    }) {
        upsert_text_item(
            &mut state.next_steps,
            WorkingMemoryItem {
                text: truncate_chars(&summary, 220),
                source_message_ids: vec![],
                confidence: 0.55,
                last_touched_turn: state.user_turn_count,
                expires_after_turns: 8,
                status: WorkingMemoryItemStatus::Active,
                kind: None,
                touch_count: 1,
            },
        );
    }
}

fn summarize_pre_compaction_tail(state: &mut WorkingMemoryState, messages: &[Message]) {
    let summarized = messages
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => {
                let trimmed = content.trim();
                (!trimmed.is_empty()).then(|| format!("User: {}", truncate_chars(trimmed, 180)))
            }
            Message::Assistant {
                turn_summary: Some(summary),
                ..
            } => {
                let trimmed = summary.trim();
                (!trimmed.is_empty())
                    .then(|| format!("Assistant summary: {}", truncate_chars(trimmed, 180)))
            }
            Message::Assistant { content, .. } => {
                let trimmed = content.trim();
                (trimmed.chars().count() >= 80)
                    .then(|| format!("Assistant: {}", truncate_chars(trimmed, 180)))
            }
            _ => None,
        })
        .take(6)
        .collect::<Vec<_>>();

    if summarized.is_empty() {
        return;
    }

    upsert_text_item(
        &mut state.working_facts,
        WorkingMemoryItem {
            text: format!("Pre-compact recap: {}", summarized.join(" | ")),
            source_message_ids: vec![],
            confidence: 0.65,
            last_touched_turn: state.user_turn_count,
            expires_after_turns: 12,
            status: WorkingMemoryItemStatus::Active,
            kind: Some("pre_compact_summary".to_string()),
            touch_count: 1,
        },
    );
}

fn apply_draft(
    mut previous: WorkingMemoryState,
    draft: WorkingMemoryDraft,
    source_message_ids: &[String],
) -> WorkingMemoryState {
    if let Some(goal) = clean_optional_text(draft.session_goal) {
        previous.session_goal = Some(goal);
    }
    if let Some(topic) = clean_optional_text(draft.active_topic) {
        previous.active_topic = Some(topic);
    } else if let Some(goal) = previous.session_goal.clone() {
        previous.active_topic = derive_topic(&goal);
    }

    let current_turn = previous.user_turn_count.max(1);
    previous.decisions = merge_section(
        &previous.decisions,
        draft.decisions,
        current_turn,
        source_message_ids,
        24,
    );
    previous.constraints = merge_section(
        &previous.constraints,
        draft.constraints,
        current_turn,
        source_message_ids,
        24,
    );
    previous.working_facts = merge_section(
        &previous.working_facts,
        draft.working_facts,
        current_turn,
        source_message_ids,
        12,
    );
    previous.open_questions = merge_section(
        &previous.open_questions,
        draft.open_questions,
        current_turn,
        source_message_ids,
        16,
    );
    previous.artifacts = merge_section(
        &previous.artifacts,
        draft.artifacts,
        current_turn,
        source_message_ids,
        18,
    );
    previous.next_steps = merge_section(
        &previous.next_steps,
        draft.next_steps,
        current_turn,
        source_message_ids,
        8,
    );

    previous
}

fn merge_section(
    previous: &[WorkingMemoryItem],
    incoming: Vec<WorkingMemoryDraftItem>,
    current_turn: u32,
    source_message_ids: &[String],
    default_expiry: u32,
) -> Vec<WorkingMemoryItem> {
    let mut merged = Vec::new();
    for item in incoming {
        let Some(text) = clean_optional_text(Some(item.text)) else {
            continue;
        };
        let key = normalize_text(&text);
        let previous_match = previous
            .iter()
            .find(|existing| normalize_text(&existing.text) == key);
        let mut source_ids = item.source_message_ids;
        for source_id in source_message_ids {
            if !source_ids.contains(source_id) {
                source_ids.push(source_id.clone());
            }
        }
        if let Some(existing) = previous_match {
            for source_id in &existing.source_message_ids {
                if !source_ids.contains(source_id) {
                    source_ids.push(source_id.clone());
                }
            }
        }

        merged.push(WorkingMemoryItem {
            text,
            source_message_ids: source_ids,
            confidence: item.confidence.unwrap_or(0.7).clamp(0.05, 1.0),
            last_touched_turn: current_turn,
            expires_after_turns: item
                .expires_after_turns
                .unwrap_or(default_expiry)
                .clamp(4, 64),
            status: item.status.unwrap_or(WorkingMemoryItemStatus::Active),
            kind: clean_optional_text(item.kind),
            touch_count: previous_match
                .map(|entry| entry.touch_count + 1)
                .unwrap_or(1),
        });
    }
    dedupe_items(merged)
}

fn dedupe_items(items: Vec<WorkingMemoryItem>) -> Vec<WorkingMemoryItem> {
    let mut out: Vec<WorkingMemoryItem> = Vec::new();
    for item in items {
        upsert_text_item(&mut out, item);
    }
    out
}

fn upsert_text_item(items: &mut Vec<WorkingMemoryItem>, item: WorkingMemoryItem) {
    let key = normalize_text(&item.text);
    if key.is_empty() {
        return;
    }
    if let Some(existing) = items
        .iter_mut()
        .find(|current| normalize_text(&current.text) == key)
    {
        if item.last_touched_turn >= existing.last_touched_turn {
            *existing = item;
        }
        return;
    }
    items.push(item);
}

fn cleanup_state(state: &mut WorkingMemoryState) {
    prune_items(&mut state.decisions, state.user_turn_count);
    prune_items(&mut state.constraints, state.user_turn_count);
    prune_items(&mut state.working_facts, state.user_turn_count);
    prune_items(&mut state.open_questions, state.user_turn_count);
    prune_items(&mut state.artifacts, state.user_turn_count);
    prune_items(&mut state.next_steps, state.user_turn_count);
}

fn prune_items(items: &mut Vec<WorkingMemoryItem>, current_turn: u32) {
    items.retain(|item| match item.status {
        WorkingMemoryItemStatus::Resolved | WorkingMemoryItemStatus::Replaced => false,
        WorkingMemoryItemStatus::Expired => false,
        WorkingMemoryItemStatus::Active => {
            let expired = current_turn
                > item
                    .last_touched_turn
                    .saturating_add(item.expires_after_turns.max(1));
            !(expired && item.confidence < 0.7)
        }
    });
}

fn should_housekeep(state: &WorkingMemoryState) -> bool {
    if !state.dirty {
        return false;
    }
    if state
        .user_turn_count
        .saturating_sub(state.last_refreshed_turn)
        >= HOUSEKEEP_TURN_INTERVAL
    {
        return true;
    }
    let Some(ref refreshed_at) = state.last_refreshed_at else {
        return true;
    };
    chrono::DateTime::parse_from_rfc3339(refreshed_at)
        .map(|dt| {
            chrono::Utc::now()
                .signed_duration_since(dt.with_timezone(&chrono::Utc))
                .num_minutes()
                >= HOUSEKEEP_MINUTES
        })
        .unwrap_or(true)
}

fn should_refresh_after_turn(
    state: &WorkingMemoryState,
    user_message: &str,
    assistant_reply: &str,
) -> bool {
    state.item_count() == 0
        || should_housekeep(state)
        || contains_user_direction_change(user_message)
        || contains_user_correction(user_message)
        || contains_assistant_decision_signal(assistant_reply)
        || assistant_reply.chars().count() >= LONG_REPLY_REFRESH_CHARS
}

fn contains_user_direction_change(user_message: &str) -> bool {
    let lower = user_message.to_lowercase();
    [
        "改成",
        "换成",
        "重新",
        "改为",
        "目标",
        "方向",
        "改一下",
        "instead",
        "switch to",
        "change the goal",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_user_correction(user_message: &str) -> bool {
    let lower = user_message.to_lowercase();
    [
        "不是",
        "不对",
        "更正",
        "纠正",
        "其实",
        "我指的是",
        "correction",
        "actually",
        "that's not",
        "i meant",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_assistant_decision_signal(assistant_reply: &str) -> bool {
    let lower = assistant_reply.to_lowercase();
    [
        "结论",
        "决定",
        "约束",
        "下一步",
        "开放问题",
        "decision",
        "constraint",
        "next step",
        "open question",
        "we should",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

async fn extract_draft(
    client: &dyn LlmClient,
    previous: &WorkingMemoryState,
    user_message: &str,
    assistant_reply: &str,
) -> Result<WorkingMemoryDraft, ApiError> {
    let previous_json = serde_json::to_string(previous).unwrap_or_else(|_| "{}".to_string());
    let user = format!(
        "Current working memory JSON:\n{}\n\nUser message:\n{}\n\nAssistant reply:\n{}\n",
        truncate_chars(&previous_json, 4_000),
        truncate_chars(user_message.trim(), 3_000),
        truncate_chars(assistant_reply.trim(), 5_000)
    );
    let raw = collect_llm_text_only(
        client,
        vec![
            LlmMessage::system(system_prompt_working_memory()),
            LlmMessage::user(user),
        ],
    )
    .await?;
    let Some(slice) = extract_json_object_slice(&raw) else {
        return Ok(heuristic_draft(user_message, assistant_reply));
    };
    serde_json::from_str(slice).map_err(|_| ApiError::Server {
        message: "working memory extractor returned invalid JSON".to_string(),
    })
}

fn heuristic_draft(user_message: &str, assistant_reply: &str) -> WorkingMemoryDraft {
    let mut draft = WorkingMemoryDraft::default();
    let user_trimmed = user_message.trim();
    let assistant_trimmed = assistant_reply.trim();
    if !user_trimmed.is_empty() {
        draft.session_goal = Some(truncate_chars(user_trimmed, 220));
        draft.active_topic = derive_topic(user_trimmed);
        draft.open_questions.push(WorkingMemoryDraftItem {
            text: truncate_chars(user_trimmed, 220),
            confidence: Some(0.55),
            expires_after_turns: Some(12),
            status: Some(WorkingMemoryItemStatus::Active),
            kind: None,
            source_message_ids: vec![],
        });
    }
    if !assistant_trimmed.is_empty() {
        draft.next_steps.push(WorkingMemoryDraftItem {
            text: truncate_chars(assistant_trimmed, 220),
            confidence: Some(0.45),
            expires_after_turns: Some(8),
            status: Some(WorkingMemoryItemStatus::Active),
            kind: None,
            source_message_ids: vec![],
        });
    }
    draft
}

fn system_prompt_working_memory() -> &'static str {
    r#"You maintain a compact session working-memory scratchpad for Omiga.
Return one JSON object only. Keep only information needed to continue the task.

Rules:
- Keep the scratchpad short and stable.
- Do not restate the whole conversation.
- Prefer concrete goals, decisions, constraints, reusable facts, unresolved questions, named artifacts, and next steps.
- Drop resolved or obsolete items.
- "session_goal" and "active_topic" should be short strings or null.
- Every list item must have at least "text". Optional keys: "confidence", "expires_after_turns", "status", "kind", "source_message_ids".
- Valid status values: "active", "resolved", "replaced", "expired".
- If the turn changes direction, replace old next steps/open questions with the new active ones.

JSON schema:
{
  "session_goal": "string or null",
  "active_topic": "string or null",
  "decisions": [{"text":"", "confidence":0.0, "expires_after_turns":24, "status":"active", "kind":"task_conclusion"}],
  "constraints": [],
  "working_facts": [],
  "open_questions": [],
  "artifacts": [],
  "next_steps": []
}"#
}

async fn collect_llm_text_only(
    client: &dyn LlmClient,
    messages: Vec<LlmMessage>,
) -> Result<String, ApiError> {
    let stream = client.send_message_streaming(messages, vec![]).await?;
    let mut out = String::new();
    let mut stream = stream;
    while let Some(res) = stream.next().await {
        match res {
            Ok(LlmStreamChunk::Text(t)) => out.push_str(&t),
            Ok(LlmStreamChunk::Stop { .. }) => break,
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

fn extract_json_object_slice(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (end > start).then_some(&trimmed[start..=end])
}

fn derive_topic(text: &str) -> Option<String> {
    let terms = derive_query_terms(text);
    if terms.is_empty() {
        return None;
    }
    Some(terms.into_iter().take(3).collect::<Vec<_>>().join(" / "))
}

fn clean_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

async fn collect_recent_source_ids(
    repo: &SessionRepository,
    session_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let recent = repo.get_session_messages_paged(session_id, 6, 0).await?;
    let mut source_ids = Vec::new();
    for record in recent {
        if (record.role == "assistant" || record.role == "user")
            && !source_ids.iter().any(|id| id == &record.id)
        {
            source_ids.push(record.id);
        }
    }
    Ok(source_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_drops_resolved_and_expired_low_confidence_items() {
        let mut state = WorkingMemoryState {
            user_turn_count: 20,
            decisions: vec![WorkingMemoryItem {
                text: "done".to_string(),
                source_message_ids: vec![],
                confidence: 0.8,
                last_touched_turn: 2,
                expires_after_turns: 4,
                status: WorkingMemoryItemStatus::Resolved,
                kind: None,
                touch_count: 1,
            }],
            open_questions: vec![WorkingMemoryItem {
                text: "stale".to_string(),
                source_message_ids: vec![],
                confidence: 0.4,
                last_touched_turn: 1,
                expires_after_turns: 4,
                status: WorkingMemoryItemStatus::Active,
                kind: None,
                touch_count: 1,
            }],
            ..WorkingMemoryState::default()
        };

        cleanup_state(&mut state);

        assert!(state.decisions.is_empty());
        assert!(state.open_questions.is_empty());
    }

    #[test]
    fn bootstrap_seeds_goal_and_topic_from_recent_messages() {
        let mut state = WorkingMemoryState::default();
        let messages = vec![
            Message::User {
                content: "完成 recall 分层记忆改造".to_string(),
            },
            Message::Assistant {
                content: "好的".to_string(),
                tool_calls: None,
                token_usage: None,
                reasoning_content: None,
                follow_up_suggestions: None,
                turn_summary: Some("下一步先实现 working memory".to_string()),
            },
        ];

        bootstrap_from_messages(&mut state, &messages);

        assert!(state.session_goal.unwrap().contains("recall"));
        assert!(state.active_topic.unwrap().contains("recall"));
        assert_eq!(state.next_steps.len(), 1);
    }

    #[test]
    fn prepare_for_auto_compact_adds_recap_fact() {
        let mut state = WorkingMemoryState::default();
        let messages = vec![
            Message::User {
                content: "先分析 recall 的召回顺序".to_string(),
            },
            Message::Assistant {
                content: "这里已经确认 recall 需要优先查 working memory，再查 long-term。"
                    .to_string(),
                tool_calls: None,
                token_usage: None,
                reasoning_content: None,
                follow_up_suggestions: None,
                turn_summary: Some("已确认 recall 的分层顺序".to_string()),
            },
            Message::User {
                content: "继续".to_string(),
            },
        ];

        summarize_pre_compaction_tail(&mut state, &messages);

        assert_eq!(state.working_facts.len(), 1);
        assert!(state.working_facts[0].text.contains("Pre-compact recap"));
        assert!(state.working_facts[0].text.contains("recall"));
    }

    #[test]
    fn render_prioritizes_goal_and_relevant_items() {
        let state = WorkingMemoryState {
            session_goal: Some("完成分层记忆重构".to_string()),
            decisions: vec![WorkingMemoryItem {
                text: "永久记忆只注入稳定 profile".to_string(),
                source_message_ids: vec![],
                confidence: 0.9,
                last_touched_turn: 3,
                expires_after_turns: 24,
                status: WorkingMemoryItemStatus::Active,
                kind: None,
                touch_count: 2,
            }],
            next_steps: vec![WorkingMemoryItem {
                text: "实现 recall 新排序".to_string(),
                source_message_ids: vec![],
                confidence: 0.8,
                last_touched_turn: 3,
                expires_after_turns: 8,
                status: WorkingMemoryItemStatus::Active,
                kind: None,
                touch_count: 1,
            }],
            ..WorkingMemoryState::default()
        };

        let rendered = state
            .render_for_prompt("recall 记忆", DEFAULT_CONTEXT_TOKENS)
            .unwrap();
        assert!(rendered.contains("Session Goal"));
        assert!(rendered.contains("recall"));
    }

    #[test]
    fn refresh_trigger_requires_boundary_or_housekeep() {
        let state = WorkingMemoryState {
            user_turn_count: 3,
            last_refreshed_turn: 3,
            last_refreshed_at: Some(now_rfc3339()),
            dirty: true,
            decisions: vec![WorkingMemoryItem {
                text: "keep existing plan".to_string(),
                source_message_ids: vec![],
                confidence: 0.8,
                last_touched_turn: 3,
                expires_after_turns: 24,
                status: WorkingMemoryItemStatus::Active,
                kind: None,
                touch_count: 1,
            }],
            ..WorkingMemoryState::default()
        };

        assert!(!should_refresh_after_turn(
            &state,
            "好的，继续",
            "已完成一个小步骤。"
        ));
        assert!(should_refresh_after_turn(
            &state,
            "不是这个方向，改成修 recall",
            "收到，改成 recall 方向。"
        ));
    }
}
