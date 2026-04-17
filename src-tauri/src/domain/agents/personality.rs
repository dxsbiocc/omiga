//! Agent 人格与身份层 —— 参考 Hermes Agent 的 SOUL + `/personality` 分层模型。
//!
//! - **soul**：持久身份片段（类似 `SOUL.md`），描述语气、价值观与沟通方式。
//! - **personality preset**：命名预设；内置表 + `~/.omiga/config.yaml` / `<project>/.omiga/config.yaml` 中的 `agent.personalities`。

use std::collections::HashMap;

use super::definition::AgentDefinition;
use super::user_context::load_user_omiga_context;
use crate::domain::tools::ToolContext;

/// 内置人格预设（名称不区分大小写）。与 Hermes CLI 文档中的命名对齐。
pub fn builtin_personality_prompt(name: &str) -> Option<&'static str> {
    let k = name.trim().to_lowercase();
    match k.as_str() {
        "helpful" => Some(
            "Be a friendly, general-purpose assistant: clear, supportive, and direct.",
        ),
        "concise" => Some(
            "Keep responses brief and to the point. Prefer short paragraphs and bullet lists when they help scanning. Avoid filler.",
        ),
        "technical" => Some(
            "Be a detailed, accurate technical expert. Prefer precise terminology, explicit assumptions, and structured explanations when complexity warrants it.",
        ),
        "creative" => Some(
            "Think innovatively and consider unconventional angles. Offer multiple options when useful; label speculation clearly.",
        ),
        "teacher" => Some(
            "Act as a patient educator: define terms, give small examples, and check understanding with quick recap questions when appropriate.",
        ),
        "kawaii" => Some(
            "Use a cute, enthusiastic tone with light sparkle-style positivity. Stay substantive — cuteness should not replace clarity or safety.",
        ),
        "catgirl" => Some(
            "Use playful, cat-like verbal tics sparingly (e.g. nya~) while remaining helpful and precise on technical content.",
        ),
        "pirate" => Some(
            "Speak with a light pirate flavor — nautical metaphors welcome — but keep commands, paths, and code exactly literal.",
        ),
        "shakespeare" => Some(
            "Use elevated, slightly theatrical prose sparingly; do not let style obscure instructions, code, or file paths.",
        ),
        "surfer" => Some(
            "Keep a relaxed, chill tone; stay organized and accurate under the casual voice.",
        ),
        "noir" => Some(
            "Lean into terse, hard-boiled narration for flavor; facts, steps, and code remain exact.",
        ),
        "uwu" => Some(
            "Very soft uwu-adjacent cuteness — use minimally and never on safety-critical or formal compliance content.",
        ),
        "philosopher" => Some(
            "Pause to clarify definitions and trade-offs; still answer concretely when the user wants execution or code.",
        ),
        "hype" => Some(
            "High energy and enthusiasm — short bursts — without adding redundant exclamation clutter to technical detail.",
        ),
        _ => None,
    }
}

/// 解析人格键：优先用户/项目 `agent.personalities`，否则内置表。
pub fn resolve_personality_text(key: &str, custom: &HashMap<String, String>) -> Option<String> {
    let k = key.trim().to_lowercase();
    if k.is_empty() {
        return None;
    }
    if let Some(t) = custom.get(&k) {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    builtin_personality_prompt(key).map(|s| s.to_string())
}

/// 将身份层、任务提示、人格预设合并为最终系统提示片段（顺序：身份 → 任务 → 叠层）。
pub fn compose_prompt_layers(
    base_task_prompt: String,
    soul: Option<&str>,
    personality_key: Option<&str>,
    custom_personalities: &HashMap<String, String>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(s) = soul.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(format!("# Agent identity (soul)\n{}", s));
    }

    let base = base_task_prompt.trim().to_string();
    if !base.is_empty() {
        parts.push(base);
    }

    if let Some(key) = personality_key.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(text) = resolve_personality_text(key, custom_personalities) {
            parts.push(format!("# Personality mode: {}\n{}", key, text));
        }
    }

    parts.join("\n\n")
}

/// 从 Agent 定义生成带人格/身份层的完整系统提示（用于主会话与子 Agent）。
pub fn compose_full_agent_system_prompt(agent: &dyn AgentDefinition, ctx: &ToolContext) -> String {
    let uc = load_user_omiga_context();
    let base = agent.system_prompt(ctx);
    compose_prompt_layers(
        base,
        agent.soul_fragment(),
        agent.personality_preset(),
        uc.personalities_ref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_personality_lookup_is_case_insensitive() {
        assert!(builtin_personality_prompt("Concise").is_some());
        assert!(builtin_personality_prompt("UNKNOWN").is_none());
    }

    #[test]
    fn compose_orders_soul_base_personality() {
        let empty = HashMap::new();
        let out = compose_prompt_layers(
            "TASK BODY".to_string(),
            Some("Be kind."),
            Some("concise"),
            &empty,
        );
        assert!(out.starts_with("# Agent identity (soul)"));
        assert!(out.contains("TASK BODY"));
        assert!(out.contains("# Personality mode: concise"));
    }

    #[test]
    fn custom_personality_overrides_builtin_name() {
        let mut m = HashMap::new();
        m.insert(
            "concise".to_string(),
            "Custom concise override.".to_string(),
        );
        let out = compose_prompt_layers("BODY".to_string(), None, Some("concise"), &m);
        assert!(out.contains("Custom concise override."));
    }
}
