//! Second-pass LLM call (independent of the main reply): propose follow-up composer prompts after a turn completes.
//! Runs in parallel with [`crate::domain::agents::output_formatter::run_turn_summary_pass`] in `emit_post_turn_meta_then_complete`.

use crate::errors::ApiError;
use crate::infrastructure::streaming::FollowUpSuggestion;
use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk};
use futures::StreamExt;

const MAX_REPLY_CHARS: usize = 12_000;
const MAX_USER_REQUEST_CHARS: usize = 4_000;
const FOLLOW_UP_TIMEOUT_SECS: u64 = 15;

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

fn build_follow_up_suggestion_messages(
    user_request: Option<&str>,
    assistant_reply: &str,
) -> Vec<LlmMessage> {
    let body = truncate_chars(assistant_reply.trim(), MAX_REPLY_CHARS);
    let request = user_request
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, MAX_USER_REQUEST_CHARS));
    let system = r#"你是对话助手。本请求与生成主回复无关，是回合结束后的第二次独立模型调用。你会收到「用户原始请求」（可能为空）和「助手对用户的最终回复」，任务是生成可放进输入框的下一步追问按钮。

**首先判断是否需要建议**，只有以下低价值场景才直接返回空数组 []：
- 回复是简单的事实陈述、是/否、数字等，继续追问价值很低
- 回复是纯代码、配置或数据文件，用户更需要直接使用而不是继续追问
- 回复已明确说明「无更多内容」、拒绝继续、或对话自然结束
- 回复是问候、致谢、道歉等礼貌性话语
- 原始请求与回复都缺少足够主题信息，无法生成紧扣上下文的具体追问

重要约束：
- 完整回答本身不能作为返回 [] 的理由。下一步建议是帮助用户继续探索，不是表示上一轮没答完。
- 对有实质信息密度的分析、研究、文件总结、报告、方案或实现结果，通常应生成 3～5 条建议，即使主问题已经被完整回答。
- 不要按关键词或领域硬编码；必须基于原始请求与回复里的具体实体、结论、文件、代码或证据生成建议。

**有追问价值时**，生成 3～5 条建议：
- 每条包含 label（按钮短文案，≤14 字）与 prompt（填入输入框的完整追问，一条可直接发送的完整句子）
- 建议必须紧扣上文已讨论的主题、结论、代码或文件，禁止泛泛的万能模板
- 只输出一个 JSON 数组，不要 Markdown、不要代码围栏、不要任何解释文字

格式（有建议时）：[{"label":"示例","prompt":"请展开说明上文中关于……"}]
格式（无建议时）：[]"#;

    let user = match request {
        Some(request) => format!(
            "用户原始请求：\n\n{}\n\n助手最终回复：\n\n{}",
            request, body
        ),
        None => format!("用户原始请求：\n\n（未提供）\n\n助手最终回复：\n\n{}", body),
    };
    vec![LlmMessage::system(system), LlmMessage::user(user)]
}

/// Calls the configured model with a short meta-prompt; returns 0–5 suggestions.
pub async fn generate_follow_up_suggestions(
    client: &dyn LlmClient,
    user_request: Option<&str>,
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
    let messages = build_follow_up_suggestion_messages(user_request, trimmed);
    let raw = tokio::time::timeout(
        std::time::Duration::from_secs(FOLLOW_UP_TIMEOUT_SECS),
        collect_llm_text_only(client, messages),
    )
    .await
    .map_err(|_| ApiError::Timeout)??;
    Ok(parse_follow_up_suggestions_json(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmConfig, LlmProvider};
    use async_trait::async_trait;
    use futures::{stream, Stream};
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    #[test]
    fn parses_json_array_from_model_output() {
        let raw = r#"```json
        [{"label":"展开实现细节","prompt":"请展开说明上文方案的实现细节。"}]
        ```"#;
        let suggestions = parse_follow_up_suggestions_json(raw);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].label, "展开实现细节");
        assert_eq!(suggestions[0].prompt, "请展开说明上文方案的实现细节。");
    }

    struct StaticClient {
        config: LlmConfig,
        raw: String,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl LlmClient for StaticClient {
        async fn send_message_streaming(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<crate::domain::tools::ToolSchema>,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(stream::iter(vec![
                Ok(LlmStreamChunk::Text(self.raw.clone())),
                Ok(LlmStreamChunk::Stop { stop_reason: None }),
            ])))
        }

        async fn health_check(&self) -> Result<bool, ApiError> {
            Ok(true)
        }

        fn config(&self) -> &LlmConfig {
            &self.config
        }
    }

    struct RecordingClient {
        config: LlmConfig,
        raw: String,
        calls: Arc<AtomicUsize>,
        seen_messages: Arc<Mutex<Vec<Vec<LlmMessage>>>>,
    }

    #[async_trait]
    impl LlmClient for RecordingClient {
        async fn send_message_streaming(
            &self,
            messages: Vec<LlmMessage>,
            _tools: Vec<crate::domain::tools::ToolSchema>,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmStreamChunk, ApiError>> + Send>>, ApiError>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen_messages.lock().unwrap().push(messages);
            Ok(Box::pin(stream::iter(vec![
                Ok(LlmStreamChunk::Text(self.raw.clone())),
                Ok(LlmStreamChunk::Stop { stop_reason: None }),
            ])))
        }

        async fn health_check(&self) -> Result<bool, ApiError> {
            Ok(true)
        }

        fn config(&self) -> &LlmConfig {
            &self.config
        }
    }

    fn message_text(message: &LlmMessage) -> String {
        message
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn substantive_research_reply_prompt_does_not_treat_completion_as_empty() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen_messages = Arc::new(Mutex::new(Vec::new()));
        let client = RecordingClient {
            config: LlmConfig::new(LlmProvider::OpenAi, "test-key"),
            raw:
                r#"[{"label":"比较通路","prompt":"请比较上文文献中肝再生与肝癌共享通路的差异。"}]"#
                    .to_string(),
            calls: Arc::clone(&calls),
            seen_messages: Arc::clone(&seen_messages),
        };
        let reply = r#"## 核心参考文献
1. Integrative Genomics: Liver Regeneration and Hepatocellular Carcinoma
2. Signaling pathways of liver regeneration

一句话总结：肝再生与肝癌共享几乎所有增殖信号通路和代谢重编程程序，区别在于前者是精确受控、有始有终的生理过程，后者是突变驱动、永不终止的病理过程。"#;

        let user_request = "概览这些关于肝再生与肝癌关系的 PDF 文献，并总结后续研究方向。";

        let suggestions = generate_follow_up_suggestions(&client, Some(user_request), reply, true)
            .await
            .expect("suggestions");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(suggestions.len(), 1);
        let messages = seen_messages.lock().unwrap();
        let system = message_text(&messages[0][0]);
        assert!(
            system.contains("完整回答本身不能作为返回 [] 的理由"),
            "prompt must not suppress useful research follow-ups just because the answer is complete: {system}"
        );
        assert!(
            !system.contains("用户的问题已被完整、自洽地回答，没有明显的追问空间"),
            "old suppressive rule makes completed literature reviews return []"
        );
        let user = message_text(&messages[0][1]);
        assert!(user.contains("用户原始请求"));
        assert!(user.contains(user_request));
        assert!(user.contains("助手最终回复"));
    }

    #[tokio::test]
    async fn embedded_markdown_next_steps_do_not_bypass_independent_llm() {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = StaticClient {
            config: LlmConfig::new(LlmProvider::OpenAi, "test-key"),
            raw: r#"[{"label":"检查回归","prompt":"请基于上文继续检查相关回归风险。"}]"#
                .to_string(),
            calls: Arc::clone(&calls),
        };
        let reply = r#"已经完成修复。

### 下一步建议（条件出现）

1. 本地 Markdown 建议"#;

        let suggestions = generate_follow_up_suggestions(&client, None, reply, true)
            .await
            .expect("suggestions");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].label, "检查回归");
    }

    #[tokio::test]
    async fn empty_model_output_returns_no_suggestions() {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = StaticClient {
            config: LlmConfig::new(LlmProvider::OpenAi, "test-key"),
            raw: "[]".to_string(),
            calls: Arc::clone(&calls),
        };
        let reply = r#"## QS群体感应研究分析

本轮完成了针对 QS 群体感应的文献梳理，整理了关键机制、疾病关联、参考文献和后续研究方向。

八、💡 总结与展望
1. 研究持续升温 — 近20年 QS 论文量保持增长。
2. 应用转化加速 — QSIs 已进入临床前/临床试验。

需要我对其中某一信号通路、特定应用场景或实验技术方案做更深入的检索分析吗？"#;

        let suggestions = generate_follow_up_suggestions(&client, None, reply, true)
            .await
            .expect("suggestions");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(suggestions.is_empty());
    }
}
