//! Second-pass LLM call (independent of the main reply): propose follow-up composer prompts after a turn completes.
//! Runs in parallel with [`crate::domain::agents::output_formatter::run_turn_summary_pass`] in `emit_post_turn_meta_then_complete`.

use crate::errors::ApiError;
use crate::infrastructure::streaming::FollowUpSuggestion;
use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk};
use futures::StreamExt;
use regex::Regex;

const MAX_REPLY_CHARS: usize = 12_000;

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

/// Collect assistant text from a streaming call without emitting UI events.
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

fn extract_json_array_slice(raw: &str) -> Option<&str> {
    let t = raw.trim();
    let start = t.find('[')?;
    let end = t.rfind(']')?;
    (end > start).then_some(&t[start..=end])
}

/// 正文中已有 `##/### 下一步建议` + 编号列表时，由前端解析为按钮，跳过第二次 follow-up 模型调用。
fn assistant_has_embedded_next_steps_section(s: &str) -> bool {
    let Ok(heading) = Regex::new(r"(?m)^#{2,3}\s*下一步建议") else {
        return false;
    };
    if !heading.is_match(s) {
        return false;
    }
    let Ok(num) = Regex::new(r"(?m)^\s*\d+[.、]\s+\S") else {
        return false;
    };
    num.is_match(s)
}

fn clamp_label(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    let count = t.chars().count();
    if count <= 14 {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(13).collect::<String>())
    }
}

pub fn parse_follow_up_suggestions_json(raw: &str) -> Vec<FollowUpSuggestion> {
    let Some(slice) = extract_json_array_slice(raw) else {
        return vec![];
    };
    let Ok(rows) = serde_json::from_slice::<Vec<serde_json::Value>>(slice.as_bytes()) else {
        return vec![];
    };
    let mut out = Vec::new();
    for v in rows {
        let label = v
            .get("label")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .unwrap_or("");
        let prompt = v
            .get("prompt")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .unwrap_or("");
        if label.is_empty() || prompt.is_empty() {
            continue;
        }
        out.push(FollowUpSuggestion {
            label: clamp_label(label),
            prompt: prompt.to_string(),
        });
        if out.len() >= 5 {
            break;
        }
    }
    out
}

/// Calls the configured model with a short meta-prompt; returns 0–5 suggestions.
pub async fn generate_follow_up_suggestions(
    client: &dyn LlmClient,
    assistant_reply: &str,
    setting_enabled: bool,
) -> Result<Vec<FollowUpSuggestion>, ApiError> {
    if !setting_enabled {
        return Ok(vec![]);
    }
    if std::env::var("OMIGA_DISABLE_FOLLOW_UP_SUGGESTIONS")
        .ok()
        .as_deref()
        == Some("1")
    {
        return Ok(vec![]);
    }
    let trimmed = assistant_reply.trim();
    if trimmed.chars().count() < 12 {
        return Ok(vec![]);
    }
    if assistant_has_embedded_next_steps_section(trimmed) {
        return Ok(vec![]);
    }
    let body = truncate_chars(trimmed, MAX_REPLY_CHARS);
    let system = r#"你是对话助手。本请求与生成主回复无关，是回合结束后的第二次独立模型调用。用户会贴出「助手对用户的最终回复」。

**首先判断是否需要建议**，以下情况请直接返回空数组 []：
- 回复是简单的事实陈述、是/否、数字等（无需追问）
- 回复是纯代码、配置或数据文件（用户更需要直接使用，而非继续追问）
- 回复已明确说明「无更多内容」或对话自然结束
- 回复是问候、致谢、道歉等礼貌性话语
- 用户的问题已被完整、自洽地回答，没有明显的追问空间

**有追问价值时**，生成 3～5 条建议：
- 每条包含 label（按钮短文案，≤14 字）与 prompt（填入输入框的完整追问，一条可直接发送的完整句子）
- 建议必须紧扣上文已讨论的主题、结论、代码或文件，禁止泛泛的万能模板
- 只输出一个 JSON 数组，不要 Markdown、不要代码围栏、不要任何解释文字

格式（有建议时）：[{"label":"示例","prompt":"请展开说明上文中关于……"}]
格式（无建议时）：[]"#;

    let user = format!("助手回复如下：\n\n{}", body);
    let messages = vec![LlmMessage::system(system), LlmMessage::user(user)];
    let raw = collect_llm_text_only(client, messages).await?;
    Ok(parse_follow_up_suggestions_json(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_llm_when_markdown_next_steps_embedded() {
        let s = r#"## 结论

### 下一步建议（条件出现）

1. 细化托斯卡纳行程
2. 对比两种方案"#;
        assert!(assistant_has_embedded_next_steps_section(s));
    }

    #[test]
    fn no_skip_without_numbered_list() {
        let s = "### 下一步建议\n\n无编号";
        assert!(!assistant_has_embedded_next_steps_section(s));
    }
}
