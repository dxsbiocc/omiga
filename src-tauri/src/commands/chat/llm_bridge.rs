use crate::api::{ContentBlock, Role};
use crate::domain::chat_state::ChatState;
use crate::domain::runtime_constraints::{
    ModelConstraintContext, RuntimeConstraintHarness, RuntimeConstraintState,
};
use crate::domain::session::ToolCall;
use crate::domain::tools::{
    normalize_legacy_retrieval_tool_arguments, normalize_legacy_retrieval_tool_name,
};
use crate::errors::{ChatError, OmigaError};
use crate::llm::{load_config_from_env, LlmConfig, LlmContent, LlmMessage, LlmRole};
use std::path::Path;

/// Get or create LLM config from environment or state
pub(super) async fn get_llm_config(chat_state: &ChatState) -> Result<LlmConfig, OmigaError> {
    // First check if we have a stored config
    let stored = chat_state.llm_config.lock().await;
    if let Some(config) = stored.as_ref() {
        if !config.api_key.is_empty() {
            return Ok(config.clone());
        }
    }
    drop(stored);

    // Prefer merged config: `omiga.yaml` default_provider + env overrides (`LLM_PROVIDER`, keys, …).
    // Using only `load_config_from_env()` ignored the file and caused UI (yaml default → "Kimi") to
    // disagree with runtime (env → e.g. deepseek) and token_usage labels.
    match crate::llm::load_config() {
        Ok(config) => {
            let mut stored = chat_state.llm_config.lock().await;
            *stored = Some(config.clone());
            drop(stored);
            if let Ok(cf) = crate::llm::config::load_config_file() {
                *chat_state.active_provider_entry_name.lock().await = cf.default_provider;
            }
            Ok(config)
        }
        Err(_) => match load_config_from_env() {
            Ok(config) => {
                let mut stored = chat_state.llm_config.lock().await;
                *stored = Some(config.clone());
                drop(stored);
                *chat_state.active_provider_entry_name.lock().await = None;
                Ok(config)
            }
            Err(_e) => Err(OmigaError::Chat(ChatError::ApiKeyMissing)),
        },
    }
}

pub(super) fn completed_to_tool_calls(calls: &[(String, String, String)]) -> Option<Vec<ToolCall>> {
    if calls.is_empty() {
        return None;
    }
    Some(
        calls
            .iter()
            .map(|(id, name, args)| ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: args.clone(),
            })
            .collect(),
    )
}

pub(super) fn tool_calls_json_opt(calls: &[(String, String, String)]) -> Option<String> {
    completed_to_tool_calls(calls).and_then(|v| serde_json::to_string(&v).ok())
}

pub(super) fn api_messages_to_llm(messages: &[crate::api::Message]) -> Vec<LlmMessage> {
    messages
        .iter()
        .map(|msg| LlmMessage {
            role: match msg.role {
                Role::User => LlmRole::User,
                Role::Assistant => LlmRole::Assistant,
            },
            content: msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => LlmContent::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => {
                        let (name, arguments) = normalize_llm_tool_history_for_model(name, input);
                        LlmContent::ToolUse {
                            id: id.clone(),
                            name,
                            arguments,
                        }
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => LlmContent::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    },
                })
                .collect(),
            name: None,
            tool_calls: None,
            reasoning_content: msg.reasoning_content.clone(),
        })
        .collect()
}

pub(super) fn augment_llm_messages_with_runtime_constraints(
    base_messages: &[LlmMessage],
    harness: &RuntimeConstraintHarness,
    state: &mut RuntimeConstraintState,
    request_text: &str,
    project_root: &Path,
    use_tools: bool,
    is_subagent: bool,
) -> (Vec<LlmMessage>, Vec<String>) {
    let before = state
        .emitted_notice_ids()
        .into_iter()
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let messages = harness.augment_model_messages(
        base_messages,
        &ModelConstraintContext {
            request_text,
            project_root,
            use_tools,
            is_subagent,
        },
        state,
    );
    let newly_emitted = state
        .emitted_notice_ids()
        .into_iter()
        .map(str::to_string)
        .filter(|id| !before.contains(id))
        .collect();
    (messages, newly_emitted)
}

pub(super) fn normalize_llm_tool_history_for_model(
    name: &str,
    input: &serde_json::Value,
) -> (String, serde_json::Value) {
    let normalized_name = normalize_legacy_retrieval_tool_name(name);
    let serialized_input = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
    let normalized_input =
        normalize_legacy_retrieval_tool_arguments(name, &normalized_name, &serialized_input);
    let value = serde_json::from_str(&normalized_input).unwrap_or_else(|_| input.clone());
    (normalized_name, value)
}

#[cfg(test)]
mod tests {
    use super::normalize_llm_tool_history_for_model;

    #[test]
    fn legacy_mcp_tool_history_is_normalized_before_model_context() {
        let (name, input) = normalize_llm_tool_history_for_model(
            "mcp__pubmed__pubmed_search_articles",
            &serde_json::json!({"term":"TP53","retmax":2}),
        );

        assert_eq!(name, "search");
        assert_eq!(input["category"], "literature");
        assert_eq!(input["source"], "pubmed");
        assert_eq!(input["query"], "TP53");
        assert_eq!(input["max_results"], 2);
    }
}
