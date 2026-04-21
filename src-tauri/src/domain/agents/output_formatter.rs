//! # Output Formatter Agent（输出格式化 Agent）
//!
//! 内部专用、**无工具** 的格式化代理：在回合结束后对「助手最终回复」做一次结构化输出（JSON），
//! 供 UI 展示「本轮要点」等。不参与 [`super::router::AgentRouter`] 与用户可选的 Subagent 列表，
//! 仅由聊天管道 [`crate::commands::chat::emit_post_turn_meta_then_complete`] 显式调用。
//!
//! **预检**：[`preflight_skip_turn_summary`] 在回合开始时根据用户输入同步判定；命中「短确认」等模式时跳过
//! [`run_turn_summary_pass`]，减少一次模型调用并更快结束回合。
//!
//! 与主对话 Agent 的关系：独立 system 角色、独立一次 `send_message_streaming`，模型按契约只输出 JSON，
//! 相当于 TS 侧 `forkedAgent` / tool-less 的专用格式化通道。

use crate::errors::ApiError;
use crate::llm::{LlmClient, LlmMessage, LlmStreamChunk};
use futures::StreamExt;
use serde::Deserialize;

/// 与遥测/日志对齐的稳定标识（不注册到 Agent 路由，仅供 tracing 与文档）
pub const OUTPUT_FORMATTER_AGENT_ID: &str = "output-formatter";

const MAX_REPLY_CHARS: usize = 12_000;
const MAX_SUMMARY_CHARS: usize = 480;

/// 回合 **开始前** 根据用户本轮输入做轻量判断：若几乎肯定不需要「本轮要点」摘要，则返回 `true`，
/// 流式结束后 **跳过** [`run_turn_summary_pass`]，减少一次模型调用、加快到 `Complete`。
///
/// 策略分三层：
/// 1. 用户明确要求摘要/总结/要点 → 强制保留（返回 `false`）
/// 2. 输出本身即为结构化交付物（计划、代码、配置、列表等）→ 跳过（返回 `true`）
/// 3. 短确认类输入 → 跳过（返回 `true`）
/// 4. 其余情况（长问题、推理/解释类、不确定）→ 保留（返回 `false`）
pub fn preflight_skip_turn_summary(user_message: &str) -> bool {
    let t = user_message.trim();
    if t.is_empty() {
        return true;
    }

    let lower = t.to_lowercase();

    // ── 层 1：用户明确要求摘要/要点，必须走完整格式化通道 ────────────────────────
    const SUMMARY_KEYWORDS: &[&str] = &[
        "总结",
        "摘要",
        "要点",
        "归纳",
        "概括",
        "recap",
        "summary",
        "summarize",
        "highlight",
        "highlights",
        "takeaway",
        "takeaways",
    ];
    if SUMMARY_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return false;
    }

    // ── 层 2：输出即为结构化交付物，摘要价值极低 ─────────────────────────────────
    // 2a. 计划 / 方案 / 路线图生成
    const PLAN_KEYWORDS: &[&str] = &[
        "制定计划",
        "生成计划",
        "制定方案",
        "生成方案",
        "规划",
        "计划书",
        "实施计划",
        "行动计划",
        "项目计划",
        "路线图",
        "roadmap",
        "make a plan",
        "create a plan",
        "write a plan",
        "plan for",
        "planning",
    ];
    if PLAN_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // 2b. 代码 / 配置 / 脚本生成
    const CODE_KEYWORDS: &[&str] = &[
        "写代码",
        "生成代码",
        "写一个函数",
        "写一段代码",
        "帮我写",
        "帮我实现",
        "实现一个",
        "实现以下",
        "写出",
        "给我写",
        "生成配置",
        "写配置",
        "生成脚本",
        "写脚本",
        "write code",
        "write a function",
        "write a script",
        "implement ",
        "generate code",
        "create a function",
        "create a class",
    ];
    if CODE_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // 2c. 列表 / 表格 / 清单 / 大纲输出
    const LIST_KEYWORDS: &[&str] = &[
        "列出",
        "列举",
        "给我列",
        "整理成列表",
        "整理成表格",
        "生成列表",
        "生成表格",
        "制作表格",
        "生成大纲",
        "大纲",
        "目录",
        "list all",
        "list the",
        "enumerate",
        "make a list",
        "create a list",
        "create a table",
        "in a table",
        "as a table",
        "outline",
    ];
    if LIST_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // 2d. 研究现状 / 综述类查询（详细报告即交付物，不需要二次摘要）
    const RESEARCH_KEYWORDS: &[&str] = &[
        "研究现状",
        "研究进展",
        "综述",
        "领域综述",
        "研究综述",
        "领域分析",
        "领域研究",
        "最新进展",
        "研究动态",
        "领域现状",
        "现状分析",
        "进展综述",
        "分析领域",
        "research review",
        "state of the art",
        "literature review",
        "survey of",
        "research landscape",
        "research status",
        "field overview",
        "review of the field",
        "overview of the field",
    ];
    if RESEARCH_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // 2e. 翻译 / 格式转换（输出即交付物）
    const TRANSFORM_KEYWORDS: &[&str] = &[
        "翻译",
        "translate",
        "转换成",
        "convert to",
        "format as",
        "格式化",
        "转成 json",
        "转成json",
        "转成 yaml",
        "转成yaml",
        "转成 csv",
        "转成csv",
    ];
    if TRANSFORM_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return true;
    }

    // ── 层 3：短确认类输入 ────────────────────────────────────────────────────────
    let count = t.chars().count();
    if count <= 8 {
        const SHORT_ACK: &[&str] = &[
            "好的", "好", "ok", "okay", "行", "嗯", "噢", "哦", "是", "对", "嗯嗯", "继续", "y",
            "yes", "no", "嗯好",
        ];
        if SHORT_ACK.contains(&lower.as_str()) {
            return true;
        }
        for s in SHORT_ACK {
            if lower.starts_with(s) && count <= s.chars().count() + 2 {
                return true;
            }
        }
    }
    if count <= 16 {
        const PHRASES: &[&str] = &[
            "没问题",
            "可以",
            "谢谢",
            "谢谢！",
            "明白了",
            "明白",
            "收到",
            "了解了",
            "知道了",
            "好的谢谢",
            "好的，谢谢",
            "多谢",
            "辛苦了",
        ];
        if PHRASES.iter().any(|p| lower == *p) {
            return true;
        }
        if lower == "好的" || lower.starts_with("好的 ") || lower.starts_with("好，") {
            return true;
        }
    }

    // ── 层 4：其余情况保留摘要通道 ───────────────────────────────────────────────
    false
}

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
    let t = raw.trim();
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    (end > start).then_some(&t[start..=end])
}

#[derive(Debug, Deserialize)]
struct FormatterTurnSummaryJson {
    need_summary: bool,
    #[serde(default)]
    summary: Option<String>,
}

fn clamp_summary(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.chars().count() <= MAX_SUMMARY_CHARS {
        return t.to_string();
    }
    format!(
        "{}…",
        t.chars()
            .take(MAX_SUMMARY_CHARS.saturating_sub(1))
            .collect::<String>()
    )
}

/// System prompt：定义 Output Formatter Agent 的唯一职责（JSON 契约 + 何时跳过摘要）。
fn system_prompt_turn_summary() -> &'static str {
    r#"你是 **Output Formatter Agent**：专门把「助手对用户的最终回复」整理成机器可解析的结构化输出。
你不调用工具、不执行代码、不延续对话，只根据下文规则输出 **一个 JSON 对象**。

任务：判断是否需要为本次回复生成「简短要点摘要」，并在需要时写出摘要正文。

**需要摘要（need_summary=true）的典型情况**
- 解释、推理、对比、排查过程较长，用户可能想先抓结论
- 多步骤操作说明、架构/设计讨论等非纯列表类内容

**不要摘要（need_summary=false，summary 必须为 null）的典型情况**
- 内容主要是执行计划、任务拆解表、甘特式步骤（计划本身已是结构化概要）
- 大段生成的代码、配置文件、JSON/YAML/日志、数据清单（用户更需要原文，二次概括价值低）
- 以 EnterPlanMode / 纯工具输出整理为主、或明显是「交付生成物」而非讨论
- 回复已经很短（寥寥数语已说清），或正文主要是引用用户原话
- 用户明确只要代码/表格/文件内容，且助手已按要求交付

只输出一个 JSON 对象，不要 Markdown、不要代码围栏、不要解释：
{"need_summary": true|false, "summary": "1-3句中文要点" | null}

若 need_summary 为 true，summary 为 1-3 句中文，总长度不超过 200 字；为 false 时 summary 必须为 null。"#
}

/// 回合结束后的格式化通道：返回 `Some(摘要)` 或 `None`（跳过、解析失败、关闭）。
pub async fn run_turn_summary_pass(
    client: &dyn LlmClient,
    assistant_reply: &str,
    setting_enabled: bool,
) -> Result<Option<String>, ApiError> {
    if !setting_enabled {
        return Ok(None);
    }
    if std::env::var("OMIGA_DISABLE_POST_TURN_SUMMARY")
        .ok()
        .as_deref()
        == Some("1")
    {
        return Ok(None);
    }
    let trimmed = assistant_reply.trim();
    if trimmed.chars().count() < 24 {
        return Ok(None);
    }
    let body = truncate_chars(trimmed, MAX_REPLY_CHARS);

    let user = format!(
        "[{}] 输入：助手最终回复全文如下。\n\n{}",
        OUTPUT_FORMATTER_AGENT_ID, body
    );
    let messages = vec![
        LlmMessage::system(system_prompt_turn_summary()),
        LlmMessage::user(user),
    ];
    let raw = collect_llm_text_only(client, messages).await?;
    let Some(slice) = extract_json_object_slice(&raw) else {
        return Ok(None);
    };
    let parsed: FormatterTurnSummaryJson = match serde_json::from_str(slice) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if !parsed.need_summary {
        return Ok(None);
    }
    let Some(s) = parsed.summary else {
        return Ok(None);
    };
    let out = clamp_summary(&s);
    if out.is_empty() {
        return Ok(None);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_skip_short_ack() {
        assert!(preflight_skip_turn_summary("好的"));
        assert!(preflight_skip_turn_summary(" 继续 "));
        assert!(!preflight_skip_turn_summary(
            "请总结一下刚才的方案并对比两种做法"
        ));
        assert!(!preflight_skip_turn_summary(&"好".repeat(100)));
    }

    #[test]
    fn preflight_skip_plan_generation() {
        assert!(preflight_skip_turn_summary("帮我制定一个项目计划"));
        assert!(preflight_skip_turn_summary("生成方案"));
        assert!(preflight_skip_turn_summary(
            "create a plan for the migration"
        ));
        assert!(preflight_skip_turn_summary("给这个项目做一个roadmap"));
    }

    #[test]
    fn preflight_skip_code_generation() {
        assert!(preflight_skip_turn_summary("帮我写一个排序函数"));
        assert!(preflight_skip_turn_summary(
            "write a function to parse JSON"
        ));
        assert!(preflight_skip_turn_summary("实现一个二叉树"));
        assert!(preflight_skip_turn_summary("生成配置文件"));
    }

    #[test]
    fn preflight_skip_list_generation() {
        assert!(preflight_skip_turn_summary("列出所有需要修改的文件"));
        assert!(preflight_skip_turn_summary("整理成表格"));
        assert!(preflight_skip_turn_summary("list all the API endpoints"));
        assert!(preflight_skip_turn_summary("生成大纲"));
    }

    #[test]
    fn preflight_skip_translation() {
        assert!(preflight_skip_turn_summary("翻译成英文"));
        assert!(preflight_skip_turn_summary("translate this to Chinese"));
        assert!(preflight_skip_turn_summary("转成json格式"));
    }

    #[test]
    fn preflight_keep_summary_keywords() {
        // 用户明确要摘要，不能跳过
        assert!(!preflight_skip_turn_summary("帮我做个摘要"));
        assert!(!preflight_skip_turn_summary("给出要点"));
        assert!(!preflight_skip_turn_summary("give me a summary of this"));
        assert!(!preflight_skip_turn_summary("recap what we discussed"));
    }

    #[test]
    fn preflight_keep_explanation_requests() {
        // 解释/分析类，可能需要摘要
        assert!(!preflight_skip_turn_summary("解释一下这个算法的时间复杂度"));
        assert!(!preflight_skip_turn_summary("分析这段代码的潜在问题"));
        assert!(!preflight_skip_turn_summary("compare the two approaches"));
    }
}
