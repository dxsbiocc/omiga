//! Automatic conversation compaction when estimated prompt size approaches the model context limit.
//!
//! Mirrors the intent of Claude Code's `autoCompact` (threshold below context window). Omiga keeps
//! domain [`Message`] rows in SQLite; when compacting we replace the full message list for the session.

use crate::api::{ContentBlock, Message as ApiMessage};
use crate::constants::tool_limits::truncate_utf8_prefix;
use crate::domain::persistence::SessionRepository;
use crate::domain::session::Message;
use crate::domain::session::SessionCodec;
use crate::llm::{LlmConfig, LlmProvider};
use std::collections::HashSet;

/// Tokens reserved below the hard context limit before we trigger compaction (parity ~ Claude Code).
const SAFETY_BUFFER_TOKENS: u32 = 12_000;
/// Rough upper bound for tool schema + definitions serialized into the provider request (when tools on).
const TOOL_SCHEMA_OVERHEAD_TOKENS: u32 = 28_000;
/// Start compacting before the hard context edge. Users can override with
/// `OMIGA_AUTO_COMPACT_THRESHOLD_PERCENT`; default is 80%.
const DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT: u32 = 80;
/// Minimum domain messages to retain after head trimming (last user turn should survive if possible).
const MIN_MESSAGES: usize = 1;
/// When truncating tool output, keep at least this many bytes of prefix.
const MIN_TOOL_OUTPUT_KEEP_BYTES: usize = 2_048;

fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .map(|s| {
            let l = s.to_ascii_lowercase();
            l == "1" || l == "true" || l == "yes"
        })
        .unwrap_or(false)
}

fn auto_compact_threshold_percent() -> u32 {
    if let Ok(s) = std::env::var("OMIGA_AUTO_COMPACT_THRESHOLD_PERCENT") {
        if let Ok(n) = s.parse::<u32>() {
            return n.clamp(50, 95);
        }
    }
    if let Ok(s) = std::env::var("OMIGA_AUTO_COMPACT_RATIO") {
        if let Ok(v) = s.parse::<f32>() {
            if v.is_finite() {
                return ((v * 100.0).round() as i32).clamp(50, 95) as u32;
            }
        }
    }
    DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT
}

/// Whether automatic compaction is enabled (default: on).
pub fn is_auto_compact_enabled() -> bool {
    if env_truthy("OMIGA_DISABLE_AUTO_COMPACT") || env_truthy("DISABLE_AUTO_COMPACT") {
        return false;
    }
    if let Ok(s) = std::env::var("OMIGA_AUTO_COMPACT") {
        let l = s.to_ascii_lowercase();
        if l == "0" || l == "false" || l == "no" {
            return false;
        }
    }
    true
}

/// Provider-aware default context window when `OMIGA_CONTEXT_WINDOW` is unset.
pub fn context_window_tokens(cfg: &LlmConfig) -> u32 {
    if let Ok(v) = std::env::var("OMIGA_CONTEXT_WINDOW") {
        if let Ok(n) = v.parse::<u32>() {
            if n >= 8_192 {
                return n;
            }
        }
    }
    match cfg.provider {
        LlmProvider::Anthropic => 200_000,
        LlmProvider::OpenAi
        | LlmProvider::Azure
        | LlmProvider::Moonshot
        | LlmProvider::Deepseek
        | LlmProvider::Custom => 131_072,
        LlmProvider::Minimax | LlmProvider::Alibaba | LlmProvider::Zhipu => 128_000,
        LlmProvider::Google => 128_000,
    }
}

fn rough_token_estimate_chars(byte_len: usize) -> u32 {
    ((byte_len / 4).max(1)) as u32
}

/// Token estimate for one API message (char/4 heuristic, JSON for tool blocks).
pub fn estimate_tokens_api_message(m: &ApiMessage) -> u32 {
    let mut t = 0u32;
    for block in &m.content {
        match block {
            ContentBlock::Text { text } => {
                t = t.saturating_add(rough_token_estimate_chars(text.len()));
            }
            ContentBlock::ToolUse { name, input, .. } => {
                let payload = format!("{name}{}", input);
                t = t.saturating_add(rough_token_estimate_chars(payload.len()));
            }
            ContentBlock::ToolResult { content, .. } => {
                t = t.saturating_add(rough_token_estimate_chars(content.len()));
            }
        }
    }
    t.max(1)
}

pub fn estimate_tokens_api_messages(msgs: &[ApiMessage]) -> u32 {
    msgs.iter().map(estimate_tokens_api_message).sum()
}

fn system_prompt_tokens(cfg: &LlmConfig) -> u32 {
    cfg.system_prompt
        .as_ref()
        .map(|s| rough_token_estimate_chars(s.len()))
        .unwrap_or(0)
}

fn request_overhead_tokens(cfg: &LlmConfig, tools_enabled: bool) -> u32 {
    let mut o = system_prompt_tokens(cfg);
    if tools_enabled {
        o = o.saturating_add(TOOL_SCHEMA_OVERHEAD_TOKENS);
    }
    o
}

/// Budget for the **chat history** portion only (messages array), in estimated tokens.
/// This is a *trigger* budget, not the absolute hard context limit: by default Omiga compacts
/// around 80% of the provider context window so large requests do not fail during upload/streaming.
pub fn messages_budget_tokens(cfg: &LlmConfig, tools_enabled: bool) -> u32 {
    let ctx = context_window_tokens(cfg);
    let trigger_ctx = ctx
        .saturating_mul(auto_compact_threshold_percent())
        .saturating_div(100);
    let reserve_out = cfg.max_tokens;
    let overhead = request_overhead_tokens(cfg, tools_enabled);
    trigger_ctx
        .saturating_sub(reserve_out)
        .saturating_sub(SAFETY_BUFFER_TOKENS)
        .saturating_sub(overhead)
}

fn take_head_message_block(messages: &mut Vec<Message>) -> Vec<Message> {
    if messages.is_empty() {
        return vec![];
    }
    let first = messages.remove(0);
    let mut removed = vec![first.clone()];
    if let Message::Assistant {
        tool_calls: Some(calls),
        ..
    } = first
    {
        let mut pending: HashSet<String> = calls.iter().map(|c| c.id.clone()).collect();
        while !messages.is_empty() && !pending.is_empty() {
            match &messages[0] {
                Message::Tool { tool_call_id, .. } if pending.contains(tool_call_id) => {
                    let id = tool_call_id.clone();
                    removed.push(messages.remove(0));
                    pending.remove(&id);
                }
                _ => break,
            }
        }
    }
    removed
}

fn pop_head_message(messages: &mut Vec<Message>) -> bool {
    !take_head_message_block(messages).is_empty()
}

fn truncate_tool_results_for_budget(
    messages: &mut [Message],
    cfg: &LlmConfig,
    tools_enabled: bool,
) {
    let budget = messages_budget_tokens(cfg, tools_enabled);
    for _ in 0..256 {
        let api = SessionCodec::to_api_messages(messages);
        let est = estimate_tokens_api_messages(&api);
        if est <= budget {
            return;
        }
        let mut best_i: Option<usize> = None;
        let mut best_len = 0usize;
        for (i, m) in messages.iter().enumerate() {
            if let Message::Tool { output, .. } = m {
                if output.len() > best_len {
                    best_len = output.len();
                    best_i = Some(i);
                }
            }
        }
        let Some(i) = best_i else {
            break;
        };
        let excess_tokens = est.saturating_sub(budget);
        let target_drop_chars = (excess_tokens as usize)
            .saturating_mul(4)
            .saturating_add(512);
        if let Message::Tool { output, .. } = &mut messages[i] {
            if output.len() <= MIN_TOOL_OUTPUT_KEEP_BYTES {
                break;
            }
            let new_len = output
                .len()
                .saturating_sub(target_drop_chars)
                .max(MIN_TOOL_OUTPUT_KEEP_BYTES);
            let prefix = truncate_utf8_prefix(output.as_str(), new_len);
            *output = format!(
                "{prefix}\n\n[Omiga: tool output truncated by auto-compact to fit context window]"
            );
        }
    }
}

fn truncate_text_messages_for_budget(
    messages: &mut [Message],
    cfg: &LlmConfig,
    tools_enabled: bool,
) {
    let budget = messages_budget_tokens(cfg, tools_enabled);
    let api = SessionCodec::to_api_messages(messages);
    let est = estimate_tokens_api_messages(&api);
    if est <= budget {
        return;
    }
    let excess = est.saturating_sub(budget);
    let drop_chars = (excess as usize).saturating_mul(4).saturating_add(256);
    if let Some(Message::User { content }) = messages.first_mut() {
        if content.len() > 512 {
            let new_len = content.len().saturating_sub(drop_chars).max(256);
            let prefix = truncate_utf8_prefix(content.as_str(), new_len);
            *content = format!("{prefix}\n\n[Omiga: message truncated by auto-compact]");
        }
    }
}

/// Result of a compaction pass.
#[derive(Debug, Clone)]
pub struct AutoCompactResult {
    pub estimated_tokens_before: u32,
    pub estimated_tokens_after: u32,
    pub removed_head_blocks: usize,
}

/// Trim oldest logical messages until the estimated history fits `messages_budget_tokens`.
/// May prepend a short system user notice when content was removed.
pub fn compact_session_messages(
    messages: &mut Vec<Message>,
    cfg: &LlmConfig,
    tools_enabled: bool,
) -> Option<AutoCompactResult> {
    if !is_auto_compact_enabled() {
        return None;
    }
    let api_before = SessionCodec::to_api_messages(messages);
    let before = estimate_tokens_api_messages(&api_before);
    let budget = messages_budget_tokens(cfg, tools_enabled);
    if before <= budget {
        return None;
    }

    let initial_len = messages.len();
    let mut removed_blocks = 0usize;

    while messages.len() > MIN_MESSAGES {
        let api = SessionCodec::to_api_messages(messages);
        let est = estimate_tokens_api_messages(&api);
        if est <= budget {
            break;
        }
        let len_before = messages.len();
        pop_head_message(messages);
        removed_blocks = removed_blocks.saturating_add(len_before.saturating_sub(messages.len()));
        if messages.is_empty() {
            break;
        }
    }

    truncate_tool_results_for_budget(messages, cfg, tools_enabled);
    truncate_text_messages_for_budget(messages, cfg, tools_enabled);

    let api_after = SessionCodec::to_api_messages(messages);
    let mut after = estimate_tokens_api_messages(&api_after);

    if initial_len > messages.len() || after < before {
        let notice = format!(
            "[Omiga: Earlier conversation was automatically removed or shortened near the {}% context threshold (window ~{} tokens). Estimated chat history: ~{} → ~{} tokens.]",
            auto_compact_threshold_percent(),
            context_window_tokens(cfg),
            before,
            after
        );
        let notice_cost = rough_token_estimate_chars(notice.len());
        if after.saturating_add(notice_cost) <= budget.saturating_add(SAFETY_BUFFER_TOKENS / 2) {
            messages.insert(0, Message::User { content: notice });
            let api = SessionCodec::to_api_messages(messages);
            after = estimate_tokens_api_messages(&api);
        }
    }

    if after >= before && initial_len == messages.len() {
        return None;
    }

    Some(AutoCompactResult {
        estimated_tokens_before: before,
        estimated_tokens_after: after,
        removed_head_blocks: removed_blocks.max(1),
    })
}

pub fn preview_removed_messages_for_compaction(
    messages: &[Message],
    cfg: &LlmConfig,
    tools_enabled: bool,
) -> Option<Vec<Message>> {
    if !is_auto_compact_enabled() {
        return None;
    }
    let before = estimate_tokens_api_messages(&SessionCodec::to_api_messages(messages));
    let budget = messages_budget_tokens(cfg, tools_enabled);
    if before <= budget {
        return None;
    }

    let mut preview = messages.to_vec();
    let mut removed = Vec::new();
    while preview.len() > MIN_MESSAGES {
        let est = estimate_tokens_api_messages(&SessionCodec::to_api_messages(&preview));
        if est <= budget {
            break;
        }
        let block = take_head_message_block(&mut preview);
        if block.is_empty() {
            break;
        }
        removed.extend(block);
    }
    (!removed.is_empty()).then_some(removed)
}

/// Replace all DB rows for `session_id` with `session.messages` and return the database id of the
/// **last** user message (for linking `conversation_rounds`).
pub async fn replace_session_messages(
    repo: &SessionRepository,
    session_id: &str,
    messages: &[Message],
) -> Result<Option<String>, sqlx::Error> {
    repo.clear_messages(session_id).await?;
    let mut last_user_id: Option<String> = None;
    for msg in messages {
        let id = uuid::Uuid::new_v4().to_string();
        let record = SessionCodec::message_to_record(msg, &id, session_id);
        repo.save_message(record.as_insert()).await?;
        if matches!(msg, Message::User { .. }) {
            last_user_id = Some(id);
        }
    }
    Ok(last_user_id)
}

/// Outcome when compaction persisted new message rows (IDs change — use for `conversation_rounds`).
#[derive(Debug, Clone)]
pub struct AutoCompactPersisted {
    pub log_line: String,
    /// DB id of the latest user message after rewrite (current turn).
    pub last_user_message_id: String,
}

/// Run compaction and persist to SQLite.
pub async fn compact_session_and_persist(
    repo: &SessionRepository,
    session_id: &str,
    session: &mut crate::domain::session::Session,
    cfg: &LlmConfig,
    tools_enabled: bool,
    fallback_user_message_id: &str,
) -> Result<Option<AutoCompactPersisted>, sqlx::Error> {
    let Some(result) = compact_session_messages(&mut session.messages, cfg, tools_enabled) else {
        return Ok(None);
    };
    let last_user = replace_session_messages(repo, session_id, &session.messages).await?;
    session.updated_at = chrono::Utc::now();
    let _ = repo.touch_session(session_id).await;
    let last_user_message_id = last_user.unwrap_or_else(|| fallback_user_message_id.to_string());
    let log_line = format!(
        "Auto-compact: ~{} → ~{} tokens (trimmed {} head block(s)).",
        result.estimated_tokens_before, result.estimated_tokens_after, result.removed_head_blocks
    );
    tracing::info!(target: "omiga::auto_compact", "{}", log_line);
    Ok(Some(AutoCompactPersisted {
        log_line,
        last_user_message_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::ToolCall;

    fn test_config() -> LlmConfig {
        let mut c = LlmConfig::new(LlmProvider::OpenAi, "k");
        c.max_tokens = 4096;
        c.system_prompt = Some("x".repeat(4000));
        c
    }

    #[test]
    fn budget_respects_max_tokens_and_overhead() {
        let c = test_config();
        let b = messages_budget_tokens(&c, true);
        // Default trigger is ~80% of the context window, then output/tool/safety reserves.
        let expected = (131_072u32 * DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT / 100)
            .saturating_sub(c.max_tokens)
            .saturating_sub(SAFETY_BUFFER_TOKENS)
            .saturating_sub(TOOL_SCHEMA_OVERHEAD_TOKENS)
            .saturating_sub(system_prompt_tokens(&c));
        assert_eq!(b, expected);
        assert!(b < 131_072);
    }

    #[test]
    fn pop_head_removes_tool_results_with_assistant() {
        let mut msgs = vec![
            Message::User {
                content: "u1".into(),
            },
            Message::Assistant {
                content: "a".into(),
                tool_calls: Some(vec![ToolCall {
                    id: "t1".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                }]),
                token_usage: None,
                reasoning_content: None,
                follow_up_suggestions: None,
                turn_summary: None,
            },
            Message::Tool {
                tool_call_id: "t1".into(),
                output: "out".into(),
            },
            Message::User {
                content: "u2".into(),
            },
        ];
        pop_head_message(&mut msgs);
        assert_eq!(msgs.len(), 3);
        pop_head_message(&mut msgs);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            Message::User { content } => assert!(content.contains("u2")),
            _ => panic!("expected user u2"),
        }
    }

    #[test]
    fn preview_removed_messages_collects_trimmed_head_blocks() {
        let mut c = test_config();
        c.max_tokens = 4;
        c.system_prompt = None;
        let msgs = vec![
            Message::User {
                content: "old context ".repeat(40_000),
            },
            Message::User {
                content: "latest".into(),
            },
        ];

        let removed = preview_removed_messages_for_compaction(&msgs, &c, false).unwrap();

        assert_eq!(removed.len(), 1);
        match &removed[0] {
            Message::User { content } => assert!(content.contains("old context")),
            _ => panic!("expected removed user message"),
        }
    }
}
