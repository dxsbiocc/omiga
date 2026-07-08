use super::*;

pub(super) fn last_turn_input_tokens_for_compaction(messages: &[Message]) -> Option<u32> {
    messages.iter().rev().find_map(|message| {
        if let Message::Assistant {
            token_usage: Some(usage),
            ..
        } = message
        {
            Some(full_prefix_input_tokens_for_compaction(usage))
        } else {
            None
        }
    })
}

fn full_prefix_input_tokens_for_compaction(usage: &MessageTokenUsage) -> u32 {
    if provider_reports_cache_read_outside_input(usage.provider.as_deref()) {
        // Anthropic reports cache_read_input_tokens separately from input_tokens, so the
        // next-request prefix scale is input + cache_read. OpenAI prompt_tokens already includes
        // cached prompt tokens, and unknown providers conservatively keep input only.
        usage.input.saturating_add(usage.cache_read.unwrap_or(0))
    } else {
        usage.input
    }
}

fn provider_reports_cache_read_outside_input(provider: Option<&str>) -> bool {
    let Some(provider) = provider else {
        return false;
    };
    let provider = provider.trim().to_ascii_lowercase();
    matches!(provider.as_str(), "anthropic" | "claude")
        || provider.starts_with("anthropic-")
        || provider.starts_with("anthropic_")
        || provider.starts_with("claude-")
        || provider.starts_with("claude_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_with_usage(provider: Option<&str>) -> Message {
        Message::Assistant {
            content: "answer".to_string(),
            tool_calls: None,
            token_usage: Some(MessageTokenUsage {
                input: 1_000,
                output: 100,
                total: Some(1_100),
                provider: provider.map(str::to_string),
                cache_read: Some(5_000),
                cache_creation: None,
            }),
            reasoning_content: None,
            follow_up_suggestions: None,
            turn_summary: None,
        }
    }

    #[test]
    fn last_turn_input_tokens_adds_anthropic_cache_read() {
        let messages = vec![assistant_with_usage(Some("anthropic"))];

        assert_eq!(
            last_turn_input_tokens_for_compaction(&messages),
            Some(6_000)
        );
    }

    #[test]
    fn last_turn_input_tokens_does_not_double_count_openai_cached_tokens() {
        let messages = vec![assistant_with_usage(Some("openai"))];

        assert_eq!(
            last_turn_input_tokens_for_compaction(&messages),
            Some(1_000)
        );
    }

    #[test]
    fn last_turn_input_tokens_keeps_input_for_missing_provider() {
        let messages = vec![assistant_with_usage(None)];

        assert_eq!(
            last_turn_input_tokens_for_compaction(&messages),
            Some(1_000)
        );
    }
}
