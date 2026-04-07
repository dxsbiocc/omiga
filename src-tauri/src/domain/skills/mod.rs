//! Skills — load `skill-name/SKILL.md` from disk (parity with `src/skills/loadSkillsDir.ts`
//! + `SkillTool` inline execution path).
//!
//! Search order (later overrides earlier on same skill name):
//! 1. `CLAUDE_CONFIG_DIR/skills` or `~/.claude/skills` — **only when** the app setting
//!    `loadClaudeUserSkills` is enabled (default: off).
//! 2. `~/.omiga/skills` — user-level Omiga path (overrides Claude on clash).
//! 3. `<project>/.omiga/skills` — project-level.

/// SQLite `settings` key: when truthy, include `~/.claude/skills` in discovery. Default **off**.
pub const SETTING_KEY_LOAD_CLAUDE_USER_SKILLS: &str = "loadClaudeUserSkills";

/// Parse stored value for [`SETTING_KEY_LOAD_CLAUDE_USER_SKILLS`]. Missing or invalid → `false`.
pub fn parse_load_claude_user_skills_setting(value: Option<&str>) -> bool {
    match value {
        Some(s) => {
            let t = s.trim().to_ascii_lowercase();
            t == "true" || t == "1" || t == "yes"
        }
        None => false,
    }
}

use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_LISTING_DESC_CHARS: usize = 250;

/// Max skills to show in the auto task-ranked system-prompt section.
const TASK_SKILL_TOP_K: usize = 8;
/// When no token matches, show this many skills (sorted by name) as a neutral fallback.
const TASK_FALLBACK_K: usize = 4;

static TASK_TOKEN_REGEXES: OnceLock<(Regex, Regex)> = OnceLock::new();

fn task_token_regexes() -> &'static (Regex, Regex) {
    TASK_TOKEN_REGEXES.get_or_init(|| {
        (
            Regex::new(r"\b[a-zA-Z][a-zA-Z0-9_-]{2,}\b").expect("latin task token regex"),
            Regex::new(r"\p{Han}{2,}").expect("Han task token regex"),
        )
    })
}

fn extract_task_tokens(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all", "can", "her", "was", "one", "our",
        "out", "day", "get", "has", "him", "his", "how", "its", "may", "new", "now", "old", "see",
        "two", "who", "way", "use", "that", "this", "with", "from", "have", "been", "than", "will",
        "your", "what", "when", "which", "while", "about", "into", "just", "more", "some", "such",
        "only", "also", "very", "here", "there", "they", "them", "then", "http", "https", "www",
        "的", "了", "和", "是", "在", "我", "有", "与", "或", "为", "将", "请", "帮", "怎么", "如何",
        "一个", "这个", "可以", "什么", "需要", "如果",
    ];
    let (latin_re, han_re) = task_token_regexes();
    let mut out = Vec::new();
    let lower = text.to_lowercase();
    for m in latin_re.find_iter(&lower) {
        let t = m.as_str();
        if !STOP.iter().any(|&s| s == t) {
            out.push(t.to_string());
        }
    }
    for m in han_re.find_iter(text) {
        let t = m.as_str();
        if t.chars().count() >= 2 {
            out.push(t.to_string());
        }
    }
    let trimmed = text.trim();
    if trimmed.len() >= 4 && trimmed.len() <= 120 {
        out.push(trimmed.to_lowercase());
    }
    out.sort();
    out.dedup();
    out
}

/// YAML `allowed-tools` / `arguments`: string or list of strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum YamlStringOrList {
    Strings(Vec<String>),
    One(String),
}

fn default_user_invocable() -> bool {
    true
}

/// Parsed YAML frontmatter — aligned with `parseSkillFrontmatterFields` in `loadSkillsDir.ts`.
#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "when_to_use", alias = "when-to-use")]
    when_to_use: Option<String>,
    #[serde(default, alias = "allowed-tools")]
    allowed_tools: Option<YamlStringOrList>,
    model: Option<String>,
    /// `inline` or `fork` (see `PromptCommand.context` in `src/types/command.ts`).
    #[serde(default)]
    context: Option<String>,
    #[serde(default, rename = "disable-model-invocation", alias = "disable_model_invocation")]
    disable_model_invocation: bool,
    #[serde(default = "default_user_invocable", rename = "user-invocable", alias = "user_invocable")]
    user_invocable: bool,
    /// Declared argument names for `$foo` substitution (`arguments` in TS frontmatter).
    arguments: Option<YamlStringOrList>,
    agent: Option<String>,
    effort: Option<String>,
}

/// Result of invoking the `skill` tool (TS inline / fork metadata + body for the model).
#[derive(Debug, Clone, Serialize)]
pub struct SkillInvokeOutput {
    pub success: bool,
    pub command_name: String,
    /// `inline` | `fork_unsupported` (forked sub-agent not implemented in Omiga).
    pub status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Full text passed to the model as the tool result (header + JSON + body).
    pub formatted_tool_result: String,
}

/// Where a skill was discovered from (for UI source labeling).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    /// `~/.claude/skills` or `$CLAUDE_CONFIG_DIR/skills` (user-level; no copy, read in place).
    ClaudeUser,
    /// `~/.omiga/skills` (user-level; overrides same-named skill from Claude path).
    OmigaUser,
    /// `<project>/.omiga/skills` (project-level).
    OmigaProject,
}

/// One discovered skill (directory name = fallback id).
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub skill_dir: PathBuf,
    /// Where this skill was discovered from.
    pub source: SkillSource,
}

fn skill_task_score(skill: &SkillEntry, tokens: &[String]) -> i32 {
    let name = skill.name.to_lowercase();
    let desc = skill.description.to_lowercase();
    let w = skill.when_to_use.as_deref().unwrap_or("").to_lowercase();
    let blob = format!("{name} {desc} {w}");
    let mut score = 0i32;
    for t in tokens {
        if t.is_empty() {
            continue;
        }
        let tl = t.to_lowercase();
        if name.contains(&tl) {
            score += 5;
        } else if blob.contains(&tl) {
            score += 2;
        }
    }
    score
}

/// Split `---\n yaml \n---\n body` from markdown.
fn parse_frontmatter(raw: &str) -> Result<(SkillFrontmatter, String), String> {
    let content = raw.trim_start();
    if !content.starts_with("---") {
        return Ok((SkillFrontmatter::default(), content.to_string()));
    }
    let rest = content[3..].trim_start_matches(['\n', '\r']);
    let end = rest
        .find("\n---\n")
        .or_else(|| rest.find("\n---\r\n"))
        .ok_or_else(|| "unclosed YAML frontmatter in SKILL.md".to_string())?;
    let yaml_str = &rest[..end];
    let after_close = &rest[end..];
    let body = after_close
        .strip_prefix("\n---\n")
        .or_else(|| after_close.strip_prefix("\n---\r\n"))
        .ok_or_else(|| "invalid SKILL.md frontmatter closing delimiter".to_string())?
        .trim_start();
    let fm: SkillFrontmatter =
        serde_yaml::from_str(yaml_str).map_err(|e| format!("SKILL.md frontmatter: {e}"))?;
    Ok((fm, body.to_string()))
}

fn yaml_string_or_list_to_strings(v: &Option<YamlStringOrList>, split_for_tools: bool) -> Vec<String> {
    let Some(v) = v else {
        return vec![];
    };
    match v {
        YamlStringOrList::Strings(s) => s.iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        YamlStringOrList::One(s) => {
            if split_for_tools {
                s.split(|c: char| c == ',' || c.is_whitespace())
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            } else {
                s.split_whitespace()
                    .map(|x| x.to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            }
        }
    }
}

/// Parse shell-like argument tokens (aligned with `parseArguments` in `argumentSubstitution.ts`).
fn parse_arguments(args: &str) -> Vec<String> {
    if args.trim().is_empty() {
        return vec![];
    }
    shell_words::split(args).unwrap_or_else(|_| args.split_whitespace().map(String::from).collect())
}

/// Substitute `$ARGUMENTS`, `$0`, `$ARGUMENTS[n]`, and `$name` (aligned with `substituteArguments`).
fn substitute_arguments(
    mut content: String,
    args: &str,
    append_if_no_placeholder: bool,
    argument_names: &[String],
) -> String {
    let original = content.clone();
    let parsed = parse_arguments(args);

    for (i, name) in argument_names.iter().enumerate() {
        if name.trim().is_empty() || name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(re) = Regex::new(&format!(r"\${}(?![\[\w])", regex::escape(name))) else {
            continue;
        };
        let repl = parsed.get(i).map(String::as_str).unwrap_or("");
        content = re.replace_all(&content, repl).to_string();
    }

    let re_idx = Regex::new(r"\$ARGUMENTS\[(\d+)\]").unwrap();
    content = re_idx
        .replace_all(&content, |caps: &regex::Captures| {
            let index: usize = caps[1].parse().unwrap_or(0);
            parsed.get(index).cloned().unwrap_or_default()
        })
        .to_string();

    // Word-boundary after digits matches TS `(?!\w)` after the index without regex look-around.
    let re_shorthand = Regex::new(r"\$(\d+)\b").unwrap();
    content = re_shorthand
        .replace_all(&content, |caps: &regex::Captures| {
            let index: usize = caps[1].parse().unwrap_or(0);
            parsed.get(index).cloned().unwrap_or_default()
        })
        .to_string();

    content = content.replace("${ARGUMENTS}", args);
    content = content.replace("$ARGUMENTS", args);

    if content == original && append_if_no_placeholder && !args.trim().is_empty() {
        content.push_str(&format!("\n\nARGUMENTS: {args}"));
    }
    content
}

fn fallback_description(body: &str) -> String {
    let line = body
        .trim()
        .lines()
        .next()
        .unwrap_or("Skill")
        .trim()
        .to_string();
    if line.len() > MAX_LISTING_DESC_CHARS {
        format!(
            "{}…",
            line.chars().take(MAX_LISTING_DESC_CHARS - 1).collect::<String>()
        )
    } else {
        line
    }
}

fn truncate_listing(s: &str) -> String {
    if s.len() <= MAX_LISTING_DESC_CHARS {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars()
                .take(MAX_LISTING_DESC_CHARS - 1)
                .collect::<String>()
        )
    }
}

async fn read_skill_entry(skill_dir: &Path, dir_name: &str, source: SkillSource) -> Option<SkillEntry> {
    let path = skill_dir.join("SKILL.md");
    let raw = tokio::fs::read_to_string(&path).await.ok()?;
    let (fm, body) = parse_frontmatter(&raw).ok()?;
    let name = fm
        .name
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| dir_name.to_string());
    let description = fm
        .description
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_description(&body));
    Some(SkillEntry {
        name,
        description,
        when_to_use: fm.when_to_use.filter(|s| !s.is_empty()),
        skill_dir: skill_dir.to_path_buf(),
        source,
    })
}

async fn collect_skills_dir(base: &Path, map: &mut HashMap<String, SkillEntry>, source: SkillSource) {
    let mut rd = match tokio::fs::read_dir(base).await {
        Ok(r) => r,
        Err(_) => return,
    };

    loop {
        let entry = match rd.next_entry().await {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(_) => break,
        };
        let path = entry.path();
        let Ok(meta) = tokio::fs::metadata(&path).await else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if let Some(sk) = read_skill_entry(&path, &dir_name, source.clone()).await {
            map.insert(sk.name.clone(), sk);
        }
    }
}

/// `CLAUDE_CONFIG_DIR` or `~/.claude` (Claude Code home).
fn claude_config_home_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
}

fn user_skills_dir_claude() -> Option<PathBuf> {
    claude_config_home_dir().map(|d| d.join("skills"))
}

/// User-level Omiga skills: `~/.omiga/skills`.
fn user_skills_dir_omiga() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".omiga").join("skills"))
}

/// Load all skills for a project root (absolute or relative cwd).
///
/// Merge order (later overrides earlier):
/// 1. `~/.claude/skills` — only if `include_claude_user_skills` is true (see
///    [`SETTING_KEY_LOAD_CLAUDE_USER_SKILLS`]).
/// 2. `~/.omiga/skills` — user-level Omiga path (wins over Claude on name clash).
/// 3. `<project>/.omiga/skills` — project-level.
pub async fn load_skills_for_project(
    project_root: &Path,
    include_claude_user_skills: bool,
) -> Vec<SkillEntry> {
    let mut map: HashMap<String, SkillEntry> = HashMap::new();

    if include_claude_user_skills {
        if let Some(claude_user) = user_skills_dir_claude() {
            collect_skills_dir(&claude_user, &mut map, SkillSource::ClaudeUser).await;
        }
    }

    if let Some(omiga_user) = user_skills_dir_omiga() {
        collect_skills_dir(&omiga_user, &mut map, SkillSource::OmigaUser).await;
    }

    let omiga = project_root.join(".omiga").join("skills");
    collect_skills_dir(&omiga, &mut map, SkillSource::OmigaProject).await;

    let mut list: Vec<SkillEntry> = map.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// Normalize skill name: trim and strip a leading `/` (TS `SkillTool.validateInput`).
pub fn normalize_skill_name(raw: &str) -> String {
    let t = raw.trim();
    t.strip_prefix('/').unwrap_or(t).trim().to_string()
}

/// Find a skill by resolved `name` or by directory basename (TS `findCommand` parity for file skills).
pub fn resolve_skill_entry<'a>(skills: &'a [SkillEntry], normalized: &str) -> Option<&'a SkillEntry> {
    if normalized.is_empty() {
        return None;
    }
    skills.iter().find(|s| s.name == normalized).or_else(|| {
        skills.iter().find(|s| {
            s.skill_dir
                .file_name()
                .map(|f| f.to_string_lossy() == normalized)
                .unwrap_or(false)
        })
    })
}

/// Resolved canonical skill name for invoke / permissions (`SkillEntry.name`).
#[must_use]
pub fn resolve_skill_display_name(skills: &[SkillEntry], raw_skill_argument: &str) -> Option<String> {
    let n = normalize_skill_name(raw_skill_argument);
    resolve_skill_entry(skills, &n).map(|e| e.name.clone())
}

/// System prompt fragment: on-demand skills + pointer to task-ranked section below.
#[must_use]
pub fn format_skills_discovery_system_section() -> String {
    "## Skills (on-demand)\n\
     The full catalog is **not** inlined here. A short **task-ranked** block is appended below when \
     skills exist (keywords from the current user message or sub-agent task). Call `list_skills` for \
     all names and short metadata (optional `query`). Use `skill` to load full `SKILL.md` when a \
     skill fits the task.\n"
        .to_string()
}

/// Per-turn task text → top skills by simple keyword overlap (name weighted higher than description).
#[must_use]
pub fn format_skills_task_relevance_section(
    skills: &[SkillEntry],
    task_text: &str,
) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let text = task_text.trim();
    if text.is_empty() {
        return None;
    }
    let tokens = extract_task_tokens(text);
    let mut scored: Vec<(i32, &SkillEntry)> = skills
        .iter()
        .map(|s| (skill_task_score(s, &tokens), s))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    let ranked: Vec<&SkillEntry> = if scored.first().map(|(s, _)| *s).unwrap_or(0) > 0 {
        scored
            .into_iter()
            .take(TASK_SKILL_TOP_K)
            .map(|(_, s)| s)
            .collect()
    } else {
        let mut v: Vec<&SkillEntry> = skills.iter().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v.into_iter().take(TASK_FALLBACK_K).collect()
    };

    let mut out = String::from(
        "## Skills likely relevant to this task\n\
         Auto-ranked from keyword overlap with the **current task text** (skill name matches weigh \
         more than description). Prefer these when choosing `skill`; use `list_skills` for the full catalog.\n\n",
    );
    for sk in ranked {
        let desc = truncate_listing(&sk.description);
        match &sk.when_to_use {
            Some(w) => {
                let w = truncate_listing(w);
                out.push_str(&format!("- `{}` — {} — {}\n", sk.name, desc, w));
            }
            None => {
                out.push_str(&format!("- `{}` — {}\n", sk.name, desc));
            }
        }
    }
    Some(out)
}

/// JSON for `list_skills` tool: metadata only, no full SKILL.md body.
///
/// When `query` is set, filters by substring. When `task_rank_context` is set, matching entries are
/// ordered by the same keyword overlap score as the system-prompt task section (higher first), then
/// by name. With no `query` and no task context, order follows the loaded skill list.
#[must_use]
pub fn list_skills_metadata_json(
    skills: &[SkillEntry],
    query: Option<&str>,
    task_rank_context: Option<&str>,
) -> String {
    #[derive(Serialize)]
    struct SkillMeta {
        name: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        when_to_use: Option<String>,
        source: SkillSource,
    }

    let q = query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());

    let mut matched: Vec<&SkillEntry> = skills
        .iter()
        .filter(|e| {
            if let Some(ref qq) = q {
                let name = e.name.to_lowercase();
                let desc = e.description.to_lowercase();
                let w = e.when_to_use.as_deref().unwrap_or("").to_lowercase();
                name.contains(qq.as_str())
                    || desc.contains(qq.as_str())
                    || w.contains(qq.as_str())
            } else {
                true
            }
        })
        .collect();

    let task = task_rank_context.map(str::trim).filter(|s| !s.is_empty());
    if let Some(t) = task {
        let tokens = extract_task_tokens(t);
        matched.sort_by(|a, b| {
            let sa = skill_task_score(a, &tokens);
            let sb = skill_task_score(b, &tokens);
            sb.cmp(&sa).then_with(|| a.name.cmp(&b.name))
        });
    }

    let filtered: Vec<SkillMeta> = matched
        .into_iter()
        .map(|e| SkillMeta {
            name: e.name.clone(),
            description: truncate_listing(&e.description),
            when_to_use: e
                .when_to_use
                .as_ref()
                .map(|w| truncate_listing(w)),
            source: e.source.clone(),
        })
        .collect();

    let count = filtered.len();
    serde_json::to_string_pretty(&serde_json::json!({
        "skills": filtered,
        "count": count,
    }))
    .unwrap_or_else(|_| "{\"skills\":[],\"count\":0}".to_string())
}

/// Full skill invocation — TS `SkillTool.call` inline path + fork notice when `context: fork`.
pub async fn invoke_skill_detailed(
    project_root: &Path,
    raw_skill_name: &str,
    args: &str,
    include_claude_user_skills: bool,
) -> Result<SkillInvokeOutput, String> {
    let normalized = normalize_skill_name(raw_skill_name);
    if normalized.is_empty() {
        return Err("Invalid skill format: empty name".to_string());
    }

    let skills = load_skills_for_project(project_root, include_claude_user_skills).await;
    let entry = resolve_skill_entry(&skills, &normalized)
        .ok_or_else(|| format!("Unknown skill: {normalized}"))?;

    let path = entry.skill_dir.join("SKILL.md");
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read SKILL.md: {e}"))?;
    let (fm, body) = parse_frontmatter(&raw)?;

    if fm.disable_model_invocation {
        return Err(format!(
            "Skill {normalized} cannot be used with the skill tool due to disable-model-invocation"
        ));
    }

    let command_name = entry.name.clone();
    let allowed_tools = yaml_string_or_list_to_strings(&fm.allowed_tools, true);
    let arg_names = yaml_string_or_list_to_strings(&fm.arguments, false);

    let dir_str = entry.skill_dir.to_string_lossy().to_string();
    let is_fork = fm
        .context
        .as_deref()
        .map(|c| c.eq_ignore_ascii_case("fork"))
        .unwrap_or(false);

    let mut md = format!("Base directory for this skill: {dir_str}\n\n{body}");
    md = md.replace("${CLAUDE_SKILL_DIR}", &dir_str);
    md = md.replace("${OMIGA_SKILL_DIR}", &dir_str);
    md = substitute_arguments(md, args, true, &arg_names);

    let mut body_for_model = String::new();
    body_for_model.push_str(&format!("Launching skill: {command_name}\n\n"));

    let status = if is_fork {
        let fork_note = "This skill is configured with `context: fork` (sub-agent in Claude Code). Omiga does not spawn forked agents yet — follow the skill text in this session.";
        let meta = serde_json::json!({
            "success": true,
            "commandName": command_name,
            "status": "fork_unsupported",
            "allowedTools": if allowed_tools.is_empty() { serde_json::Value::Null } else { serde_json::to_value(&allowed_tools).unwrap() },
            "model": fm.model,
            "effort": fm.effort,
            "agent": fm.agent,
            "userInvocable": fm.user_invocable,
            "_omiga": fork_note
        });
        body_for_model.push_str(&serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?);
        body_for_model.push_str("\n\n---\n\n");
        body_for_model.push_str("## Forked skill note (Omiga)\n\n");
        body_for_model.push_str(fork_note);
        body_for_model.push_str("\n\n---\n\n");
        body_for_model.push_str(&md);
        "fork_unsupported"
    } else {
        let meta = serde_json::json!({
            "success": true,
            "commandName": command_name,
            "status": "inline",
            "allowedTools": if allowed_tools.is_empty() { serde_json::Value::Null } else { serde_json::to_value(&allowed_tools).unwrap() },
            "model": fm.model,
            "effort": fm.effort,
            "agent": fm.agent,
            "userInvocable": fm.user_invocable,
            "_omiga": "Skill content is inlined below; Omiga does not apply separate tool allowlists or model overrides — configure the session in the app if needed."
        });
        body_for_model.push_str(&serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?);
        body_for_model.push_str("\n\n---\n\n");
        body_for_model.push_str(&md);
        "inline"
    };

    Ok(SkillInvokeOutput {
        success: true,
        command_name: command_name.clone(),
        status: status.to_string(),
        allowed_tools: allowed_tools.clone(),
        model: fm.model.clone(),
        effort: fm.effort.clone(),
        agent: fm.agent.clone(),
        formatted_tool_result: body_for_model,
    })
}

/// Resolve skill and return the formatted tool result string (what the model receives).
pub async fn invoke_skill(
    project_root: &Path,
    raw_skill_name: &str,
    args: &str,
    include_claude_user_skills: bool,
) -> Result<String, String> {
    let out = invoke_skill_detailed(
        project_root,
        raw_skill_name,
        args,
        include_claude_user_skills,
    )
    .await?;
    Ok(out.formatted_tool_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_basic() {
        let raw = "---\ndescription: Hello\n---\n\nBody here.";
        let (fm, body) = parse_frontmatter(raw).unwrap();
        assert_eq!(fm.description.as_deref(), Some("Hello"));
        assert!(body.contains("Body here"));
    }

    #[test]
    fn frontmatter_empty() {
        let raw = "No frontmatter";
        let (_fm, body) = parse_frontmatter(raw).unwrap();
        assert!(body.starts_with("No frontmatter"));
    }

    #[test]
    fn normalize_skill_name_strips_slash() {
        assert_eq!(normalize_skill_name(" /foo "), "foo");
    }

    #[test]
    fn substitute_arguments_basic() {
        let s = substitute_arguments(
            "Hi $ARGUMENTS and $0 end".to_string(),
            "a b",
            false,
            &[],
        );
        assert!(s.contains("a b"));
    }

    #[test]
    fn parse_arguments_quoted() {
        let v = parse_arguments(r#"one "two three""#);
        assert_eq!(v, vec!["one", "two three"]);
    }

    #[tokio::test]
    async fn invoke_skill_inline_project_skill() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join(".omiga").join("skills").join("demo");
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .expect("mkdir");
        let raw = r#"---
name: demo
allowed-tools:
  - bash
  - file_read
---
Line $ARGUMENTS
"#;
        tokio::fs::write(skill_dir.join("SKILL.md"), raw)
            .await
            .expect("write");
        let out = invoke_skill_detailed(dir.path(), "/demo", "hello", false)
            .await
            .expect("invoke");
        assert_eq!(out.status, "inline");
        assert!(out.allowed_tools.contains(&"bash".to_string()));
        assert!(out.formatted_tool_result.contains("Launching skill: demo"));
        assert!(out.formatted_tool_result.contains("Line hello"));
    }

    #[test]
    fn task_relevance_ranks_postgres_above_unrelated() {
        let skills = vec![
            SkillEntry {
                name: "alpha-help".to_string(),
                description: "generic".to_string(),
                when_to_use: None,
                skill_dir: PathBuf::from("/tmp/a"),
                source: SkillSource::OmigaProject,
            },
            SkillEntry {
                name: "postgres-patterns".to_string(),
                description: "SQL tips".to_string(),
                when_to_use: Some("database work".to_string()),
                skill_dir: PathBuf::from("/tmp/b"),
                source: SkillSource::OmigaProject,
            },
        ];
        let sec =
            format_skills_task_relevance_section(&skills, "optimize my postgres query").expect("sec");
        let pg = sec.find("postgres-patterns").expect("postgres");
        let al = sec.find("alpha-help").expect("alpha");
        assert!(pg < al);
    }

    #[test]
    fn list_skills_json_orders_by_task_when_context_set() {
        let skills = vec![
            SkillEntry {
                name: "alpha-help".to_string(),
                description: "generic".to_string(),
                when_to_use: None,
                skill_dir: PathBuf::from("/tmp/a"),
                source: SkillSource::OmigaProject,
            },
            SkillEntry {
                name: "postgres-patterns".to_string(),
                description: "SQL tips".to_string(),
                when_to_use: Some("database".to_string()),
                skill_dir: PathBuf::from("/tmp/b"),
                source: SkillSource::OmigaProject,
            },
        ];
        let json = list_skills_metadata_json(&skills, None, Some("postgres tuning"));
        let pg = json.find("postgres-patterns").expect("postgres in json");
        let al = json.find("alpha-help").expect("alpha in json");
        assert!(pg < al);
    }
}
