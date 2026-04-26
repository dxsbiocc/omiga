//! LLM-backed chat session title — short label for the sidebar from the first user message.
//! Aligned with `titleFromFirstUserMessage` / `fallback_title_from_message` and the UI `sessionStore`.

use crate::errors::ApiError;
use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk};
use futures::StreamExt;

const MAX_INPUT_CHARS: usize = 6_000;
/// Aligned with `FIRST_MESSAGE_TITLE_MAX_CHARS` in `sessionStore.ts`
const MAX_TITLE_CHARS: usize = 48;

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

/// One-line title from the first user message (no LLM) — same rules as `titleFromFirstUserMessage` in TS.
pub fn fallback_title_from_message(raw: &str) -> String {
    let first_non_empty = raw
        .lines()
        .find_map(|line| {
            let t = line.trim();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .unwrap_or("");
    let collapsed = first_non_empty
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return "Chat".to_string();
    }
    let n = collapsed.chars().count();
    if n <= MAX_TITLE_CHARS {
        collapsed
    } else {
        let prefix: String = collapsed.chars().take(MAX_TITLE_CHARS - 1).collect();
        format!("{}…", prefix)
    }
}

fn system_prompt_session_title() -> &'static str {
    r#"你是「会话标题」生成器。根据用户的第一条聊天消息，只输出一行简短标题（用于侧边栏会话列表）。

规则：
- 使用与用户消息一致的语言（例如用户写中文则用中文标题）
- 精炼概括主题或意图，不要复述整句；长度以 8–28 个字符为宜，最多不超过 32 个字符
- 不要输出引号、书名号、Markdown、列表符号、编号或任何解释性文字
- 只输出标题这一行，不要前缀如「标题：」"#
}

async fn collect_llm_text_only(
    client: &dyn LlmClient,
    messages: Vec<LlmMessage>,
) -> Result<String, ApiError> {
    let mut stream = client.send_message_streaming(messages, vec![]).await?;
    let mut out = String::new();
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

/// Strip model fluff and clamp length — keeps a single line suitable for SQLite `sessions.name`.
pub fn sanitize_session_title(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        return String::new();
    }
    let mut s = first;
    for prefix in [
        "标题：",
        "标题:",
        "Title:",
        "title:",
        "会话标题：",
        "会话标题:",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim();
        }
    }
    s = s.trim_matches(|c| {
        matches!(
            c,
            '"' | '\'' | '「' | '」' | '『' | '』' | '《' | '》' | '【' | '】'
        )
    });
    s = s.trim();
    s = s.trim_start_matches(['#', '*', '-', '•', '·']);
    s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let n = s.chars().count();
    if n <= MAX_TITLE_CHARS {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars()
                .take(MAX_TITLE_CHARS.saturating_sub(1))
                .collect::<String>()
        )
    }
}

/// Single non-tool LLM call: short session title from the first user message.
pub async fn suggest_session_title_llm(
    client: &dyn LlmClient,
    user_message: &str,
) -> Result<String, ApiError> {
    let trimmed = user_message.trim();
    if trimmed.is_empty() {
        return Ok(fallback_title_from_message(user_message));
    }
    let body = truncate_chars(trimmed, MAX_INPUT_CHARS);
    let messages = vec![
        LlmMessage::system(system_prompt_session_title()),
        LlmMessage::user(format!(
            "用户第一条消息如下，请只输出会话标题：\n\n{}",
            body
        )),
    ];
    let raw = collect_llm_text_only(client, messages).await?;
    let cleaned = sanitize_session_title(&raw);
    if cleaned.is_empty() {
        return Ok(fallback_title_from_message(user_message));
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_matches_short() {
        assert_eq!(fallback_title_from_message("Hello world"), "Hello world");
    }

    #[test]
    fn sanitize_strips_prefix_and_quotes() {
        assert_eq!(sanitize_session_title("标题：「测试一下」"), "测试一下");
        assert_eq!(sanitize_session_title("\"Foo bar\"\n"), "Foo bar");
    }
}
