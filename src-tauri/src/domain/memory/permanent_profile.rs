use serde::{Deserialize, Serialize};

const DEFAULT_MAX_PROMPT_TOKENS: usize = 1_500;
const APPROX_CHARS_PER_TOKEN: usize = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermanentProfile {
    pub agent_identity: Vec<String>,
    pub persona: Vec<String>,
    pub boundaries: Vec<String>,
    pub stable_user_profile: Vec<String>,
    pub style_preferences: Vec<String>,
    pub taboos: Vec<String>,
    pub workflow_preferences: Vec<String>,
    pub environment_constraints: Vec<String>,
    pub stable_project_conventions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermanentProfileStatus {
    pub enabled: bool,
    pub item_count: usize,
    pub injected_char_count: usize,
}

impl PermanentProfile {
    pub fn compile(soul_md: Option<&str>, user_md: Option<&str>, memory_md: Option<&str>) -> Self {
        let mut soul = soul_md.map(parse_markdown_sections).unwrap_or_default();
        let mut user = user_md.map(parse_markdown_sections).unwrap_or_default();
        let mut memory = memory_md.map(parse_markdown_sections).unwrap_or_default();

        let mut profile = Self::default();

        profile
            .agent_identity
            .extend(take_section_lines(&mut soul, &["基本身份"]));

        profile.persona.extend(take_section_lines(
            &mut soul,
            &["风格与语气（vibe）", "风格与语气"],
        ));
        profile.persona.extend(take_section_lines(
            &mut soul,
            &["核心准则（core truths）", "核心准则"],
        ));

        profile
            .boundaries
            .extend(take_section_lines(&mut soul, &["边界"]));

        profile
            .stable_user_profile
            .extend(take_section_lines(&mut user, &["基本信息"]));
        profile
            .stable_user_profile
            .extend(filtered_user_background_lines_from_lines(
                take_section_lines(&mut user, &["背景与偏好"]),
            ));

        let communication_preferences = take_section_lines(&mut user, &["沟通偏好"]);
        for line in communication_preferences {
            if line.contains("不希望") || line.contains("不要") || line.contains("禁") {
                push_unique(&mut profile.taboos, line);
            } else {
                push_unique(&mut profile.style_preferences, line);
            }
        }

        profile
            .environment_constraints
            .extend(take_section_lines(&mut memory, &["环境与工具"]));

        profile
            .stable_project_conventions
            .extend(take_section_lines(&mut memory, &["仓库与工作区习惯"]));
        profile
            .stable_project_conventions
            .extend(take_section_lines(&mut memory, &["反复出现的约定"]));

        profile
            .workflow_preferences
            .extend(filtered_pitfall_lines_from_lines(take_section_lines(
                &mut memory,
                &["踩坑记录"],
            )));

        for line in drain_remaining_lines(&mut soul) {
            classify_soul_fallback(&mut profile, line);
        }
        for line in drain_remaining_lines(&mut user) {
            classify_user_fallback(&mut profile, line);
        }
        for line in drain_remaining_lines(&mut memory) {
            classify_memory_fallback(&mut profile, line);
        }

        profile.normalize();
        profile
    }

    pub fn item_count(&self) -> usize {
        [
            self.agent_identity.len(),
            self.persona.len(),
            self.boundaries.len(),
            self.stable_user_profile.len(),
            self.style_preferences.len(),
            self.taboos.len(),
            self.workflow_preferences.len(),
            self.environment_constraints.len(),
            self.stable_project_conventions.len(),
        ]
        .into_iter()
        .sum()
    }

    pub fn status(&self) -> PermanentProfileStatus {
        let rendered = self.render_for_system_prompt(DEFAULT_MAX_PROMPT_TOKENS);
        PermanentProfileStatus {
            enabled: self.item_count() > 0,
            item_count: self.item_count(),
            injected_char_count: rendered.as_ref().map(|s| s.chars().count()).unwrap_or(0),
        }
    }

    pub fn render_for_system_prompt(&self, max_tokens: usize) -> Option<String> {
        let max_chars = max_tokens.saturating_mul(APPROX_CHARS_PER_TOKEN).max(1_200);
        let mut out = String::from(
            "## Permanent Profile (compiled from ~/.omiga/SOUL.md, USER.md, MEMORY.md)\n\n",
        );
        let mut has_any = false;

        let prioritized_sections = [
            ("Agent Identity", &self.agent_identity),
            ("Persona", &self.persona),
            ("Boundaries", &self.boundaries),
            ("Taboos", &self.taboos),
            ("Environment Constraints", &self.environment_constraints),
            ("Stable User Profile", &self.stable_user_profile),
            ("Style Preferences", &self.style_preferences),
            ("Workflow Preferences", &self.workflow_preferences),
            (
                "Stable Project Conventions",
                &self.stable_project_conventions,
            ),
        ];

        for (heading, items) in prioritized_sections {
            if items.is_empty() {
                continue;
            }
            let section = render_section(heading, items);
            if out.chars().count().saturating_add(section.chars().count()) > max_chars {
                break;
            }
            out.push_str(&section);
            has_any = true;
        }

        has_any.then_some(out.trim().to_string())
    }

    fn normalize(&mut self) {
        dedupe_in_place(&mut self.agent_identity);
        dedupe_in_place(&mut self.persona);
        dedupe_in_place(&mut self.boundaries);
        dedupe_in_place(&mut self.stable_user_profile);
        dedupe_in_place(&mut self.style_preferences);
        dedupe_in_place(&mut self.taboos);
        dedupe_in_place(&mut self.workflow_preferences);
        dedupe_in_place(&mut self.environment_constraints);
        dedupe_in_place(&mut self.stable_project_conventions);
    }
}

fn render_section(heading: &str, items: &[String]) -> String {
    let mut out = format!("### {heading}\n");
    for item in items {
        out.push_str("- ");
        out.push_str(item);
        out.push('\n');
    }
    out.push('\n');
    out
}

fn filtered_user_background_lines_from_lines(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            !lower.contains("当前")
                && !lower.contains("这周")
                && !lower.contains("最近")
                && !lower.contains("ongoing")
        })
        .collect()
}

fn filtered_pitfall_lines_from_lines(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            if line.starts_with("容易") || line.starts_with("不要") {
                line
            } else {
                format!("Watch out: {line}")
            }
        })
        .collect()
}

fn parse_markdown_sections(raw: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current_heading: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            continue;
        }
        if trimmed.starts_with("## ") {
            flush_current_section(&mut sections, &mut current_heading, &mut current_lines);
            current_heading = Some(trimmed.trim_start_matches("## ").trim().to_lowercase());
            continue;
        }
        current_lines.push(line.to_string());
    }

    flush_current_section(&mut sections, &mut current_heading, &mut current_lines);

    sections
}

fn bullet_like_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('>'))
        .filter(|line| !line.starts_with("<!--"))
        .filter(|line| !line.starts_with('|'))
        .filter(|line| !line.starts_with('#'))
        .filter(|line| !line.starts_with("```"))
        .map(|line| {
            line.trim_start_matches("- ")
                .trim_start_matches("* ")
                .trim_start_matches("1. ")
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .filter(|line| !line.contains("<!--"))
        .filter(|line| line != "---")
        .collect()
}

fn push_unique(target: &mut Vec<String>, value: String) {
    let normalized = normalize_line(&value);
    if normalized.is_empty() {
        return;
    }
    if target
        .iter()
        .any(|existing| normalize_line(existing) == normalized)
    {
        return;
    }
    target.push(value.trim().to_string());
}

fn dedupe_in_place(values: &mut Vec<String>) {
    let mut deduped = Vec::new();
    for value in values.drain(..) {
        push_unique(&mut deduped, value);
    }
    *values = deduped;
}

fn normalize_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

fn take_section_lines(
    sections: &mut std::collections::HashMap<String, String>,
    headings: &[&str],
) -> Vec<String> {
    let mut lines = Vec::new();
    for heading in headings {
        if let Some(content) = sections.remove(&heading.to_lowercase()) {
            lines.extend(bullet_like_lines(&content));
        }
    }
    lines
}

fn drain_remaining_lines(sections: &mut std::collections::HashMap<String, String>) -> Vec<String> {
    let mut entries: Vec<(String, String)> = sections.drain().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = Vec::new();
    for (_heading, content) in entries {
        lines.extend(bullet_like_lines(&content));
    }
    lines
}

fn flush_current_section(
    sections: &mut std::collections::HashMap<String, String>,
    current_heading: &mut Option<String>,
    current_lines: &mut Vec<String>,
) {
    let content = current_lines.join("\n").trim().to_string();
    current_lines.clear();
    if content.is_empty() {
        return;
    }
    let key = current_heading
        .take()
        .unwrap_or_else(|| "__root__".to_string());
    sections
        .entry(key)
        .and_modify(|existing| {
            if !existing.is_empty() {
                existing.push('\n');
            }
            existing.push_str(&content);
        })
        .or_insert(content);
}

fn classify_soul_fallback(profile: &mut PermanentProfile, line: String) {
    let lower = line.to_lowercase();
    if looks_like_boundary(&lower) {
        push_unique(&mut profile.boundaries, line.clone());
        if looks_like_taboo(&lower) {
            push_unique(&mut profile.taboos, line);
        }
    } else if looks_like_style_preference(&lower) {
        push_unique(&mut profile.persona, line.clone());
        push_unique(&mut profile.style_preferences, line);
    } else {
        push_unique(&mut profile.persona, line);
    }
}

fn classify_user_fallback(profile: &mut PermanentProfile, line: String) {
    let lower = line.to_lowercase();
    if looks_like_transient_user_focus(&lower) {
        return;
    }
    if looks_like_taboo(&lower) {
        push_unique(&mut profile.taboos, line);
    } else if looks_like_style_preference(&lower) {
        push_unique(&mut profile.style_preferences, line);
    } else {
        push_unique(&mut profile.stable_user_profile, line);
    }
}

fn classify_memory_fallback(profile: &mut PermanentProfile, line: String) {
    let lower = line.to_lowercase();
    if looks_like_environment_constraint(&lower) {
        push_unique(&mut profile.environment_constraints, line);
    } else if looks_like_watchout(&lower) {
        push_unique(&mut profile.workflow_preferences, line);
    } else {
        push_unique(&mut profile.stable_project_conventions, line);
    }
}

fn looks_like_boundary(line: &str) -> bool {
    looks_like_taboo(line)
        || line.contains("边界")
        || line.contains("boundary")
        || line.contains("必须先确认")
}

fn looks_like_taboo(line: &str) -> bool {
    [
        "不希望",
        "不要",
        "禁止",
        "不得",
        "must not",
        "never",
        "avoid",
    ]
    .iter()
    .any(|needle| line.contains(needle))
}

fn looks_like_style_preference(line: &str) -> bool {
    [
        "风格", "语气", "语言", "回答", "输出", "长度", "tone", "style", "response",
    ]
    .iter()
    .any(|needle| line.contains(needle))
}

fn looks_like_environment_constraint(line: &str) -> bool {
    [
        "os",
        "shell",
        "python",
        "node",
        "rust",
        "toolchain",
        "路径",
        "环境",
        "工具",
        "zsh",
        "bash",
    ]
    .iter()
    .any(|needle| line.contains(needle))
}

fn looks_like_watchout(line: &str) -> bool {
    [
        "踩坑",
        "容易",
        "注意",
        "watch out",
        "pitfall",
        "warning",
        "不要",
    ]
    .iter()
    .any(|needle| line.contains(needle))
}

fn looks_like_transient_user_focus(line: &str) -> bool {
    ["当前", "这周", "最近", "ongoing", "right now", "currently"]
        .iter()
        .any(|needle| line.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_filters_transient_user_focus() {
        let soul =
            "## 基本身份\n- 名字：Nova\n\n## 风格与语气\n- 直接简洁\n\n## 边界\n- 不主动删除文件";
        let user = "## 基本信息\n- 角色：研究员\n\n## 当前焦点\n- 这周正在写 rebuttal\n\n## 沟通偏好\n- 回答风格：先给结论\n- 不希望出现的行为：不要过度解释";
        let memory =
            "## 环境与工具\n- OS / Shell: macOS / zsh\n\n## 反复出现的约定\n- 提交前跑测试";

        let profile = PermanentProfile::compile(Some(soul), Some(user), Some(memory));

        assert!(profile
            .stable_user_profile
            .iter()
            .all(|line| !line.contains("rebuttal")));
        assert!(profile
            .taboos
            .iter()
            .any(|line| line.contains("不要过度解释")));
        assert!(profile
            .environment_constraints
            .iter()
            .any(|line| line.contains("macOS")));
    }

    #[test]
    fn compile_keeps_free_form_and_unknown_sections() {
        let soul = "# SOUL\n\n我希望助手保持审慎，不要给未经验证的结论。\n\n## 自定义段落\n- 回答时先给结论";
        let user = "# USER\n\n我是生物信息研究员。\n\n## 其他说明\n- 请默认使用中文";
        let memory = "# MEMORY\n\nConda 环境叫 bioinfo\n\n## 老习惯\n- 提交前跑测试";

        let profile = PermanentProfile::compile(Some(soul), Some(user), Some(memory));
        let rendered = profile.render_for_system_prompt(1_500).unwrap();

        assert!(rendered.contains("未经验证"));
        assert!(rendered.contains("默认使用中文"));
        assert!(rendered.contains("Conda 环境叫 bioinfo"));
    }

    #[test]
    fn render_respects_priority_budget() {
        let profile = PermanentProfile {
            agent_identity: vec!["Agent identity".to_string()],
            persona: vec!["Persona".to_string()],
            boundaries: vec!["Boundary".to_string()],
            stable_project_conventions: vec!["Convention".repeat(400)],
            ..Default::default()
        };

        let rendered = profile.render_for_system_prompt(60).unwrap();

        assert!(rendered.contains("Agent Identity"));
        assert!(rendered.contains("Persona"));
        assert!(!rendered.contains("ConventionConvention"));
    }
}
