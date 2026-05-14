//! Skills — load `skill-name/SKILL.md` from disk (parity with `src/skills/loadSkillsDir.ts`
//! + `SkillTool` inline execution path).
//!
//! Layouts supported under each skills root:
//! - **Flat:** `<root>/<skill-name>/SKILL.md`
//! - **One-level category (Hermes-style):** `<root>/<category>/<skill-name>/SKILL.md` when `<category>/` has no `SKILL.md`.
//!
//! Search order (later overrides earlier on same skill name):
//! 1. `~/.omiga/skills` — user-level.
//! 2. `<project>/.omiga/skills` — project-level.
//!
//! `~/.claude/skills` is **not** read at runtime. Use Settings → Skills → import buttons to copy
//! skills into an Omiga directory.

use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tracing;

pub mod fuzzy_match;
pub mod skill_config;
pub mod skill_guard;
pub mod skill_manage;
pub mod skill_view;

pub use skill_config::{
    list_skill_config_vars, project_config_path, set_config_var, user_config_path,
};
pub use skill_manage::execute_skill_manage;
pub use skill_view::execute_skill_view;

const MAX_LISTING_DESC_CHARS: usize = 250;

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
        "的", "了", "和", "是", "在", "我", "有", "与", "或", "为", "将", "请", "帮", "怎么",
        "如何", "一个", "这个", "可以", "什么", "需要", "如果",
    ];
    let (latin_re, han_re) = task_token_regexes();
    let mut out = Vec::new();
    let lower = text.to_lowercase();
    for m in latin_re.find_iter(&lower) {
        let t = m.as_str();
        if !STOP.contains(&t) {
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

// ── Metadata structs (Feature 2: config vars + Feature 3: conditions) ──────

/// Raw config var entry from `metadata.omiga.config[]`.
#[derive(Debug, Deserialize, Default, Clone)]
struct RawConfigVar {
    key: Option<String>,
    description: Option<String>,
    default: Option<String>,
    prompt: Option<String>,
}

/// Conditions / toolset requirements from `metadata.omiga.*`.
#[derive(Debug, Deserialize, Default, Clone, Serialize)]
pub struct SkillConditions {
    /// Skill only activates when ALL of these toolsets are available.
    #[serde(default)]
    pub requires_toolsets: Vec<String>,
    /// Skill activates as a fallback when any of these toolsets is unavailable.
    #[serde(default)]
    pub fallback_for_toolsets: Vec<String>,
    /// Skill only activates when ALL of these tools are available.
    #[serde(default)]
    pub requires_tools: Vec<String>,
    /// Skill activates as a fallback when any of these tools is unavailable.
    #[serde(default)]
    pub fallback_for_tools: Vec<String>,
}

impl SkillConditions {
    pub fn is_empty(&self) -> bool {
        self.requires_toolsets.is_empty()
            && self.fallback_for_toolsets.is_empty()
            && self.requires_tools.is_empty()
            && self.fallback_for_tools.is_empty()
    }
}

/// `metadata.omiga` (or `metadata.hermes` for cross-compatibility) block.
#[derive(Debug, Deserialize, Default, Clone)]
struct SkillMetadataNamespace {
    #[serde(default)]
    config: Vec<RawConfigVar>,
    #[serde(default)]
    requires_toolsets: Vec<String>,
    #[serde(default)]
    fallback_for_toolsets: Vec<String>,
    #[serde(default)]
    requires_tools: Vec<String>,
    #[serde(default)]
    fallback_for_tools: Vec<String>,
}

/// Top-level `metadata:` block in SKILL.md frontmatter.
#[derive(Debug, Deserialize, Default, Clone)]
struct SkillMetadata {
    /// Primary namespace for Omiga-specific declarations.
    #[serde(default)]
    omiga: SkillMetadataNamespace,
    /// Secondary namespace for Hermes cross-compatibility.
    #[serde(default)]
    hermes: SkillMetadataNamespace,
}

impl SkillMetadata {
    /// Merged config vars (omiga takes precedence; hermes fills gaps).
    fn config_vars(&self) -> Vec<skill_config::ConfigVar> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for ns in [&self.omiga, &self.hermes] {
            for raw in &ns.config {
                let Some(key) = raw.key.as_ref().filter(|k| !k.trim().is_empty()) else {
                    continue;
                };
                let key = key.trim().to_string();
                if seen.contains(&key) {
                    continue;
                }
                let Some(desc) = raw.description.as_ref().filter(|d| !d.trim().is_empty()) else {
                    continue;
                };
                seen.insert(key.clone());
                result.push(skill_config::ConfigVar {
                    key,
                    description: desc.trim().to_string(),
                    default: raw.default.clone(),
                    prompt: raw.prompt.clone(),
                });
            }
        }
        result
    }

    /// Merged conditions (omiga takes precedence; hermes supplements).
    fn conditions(&self) -> SkillConditions {
        fn merge_vecs(a: &[String], b: &[String]) -> Vec<String> {
            let mut seen = std::collections::HashSet::new();
            a.iter()
                .chain(b.iter())
                .filter(|s| seen.insert(s.to_lowercase()))
                .cloned()
                .collect()
        }
        SkillConditions {
            requires_toolsets: merge_vecs(
                &self.omiga.requires_toolsets,
                &self.hermes.requires_toolsets,
            ),
            fallback_for_toolsets: merge_vecs(
                &self.omiga.fallback_for_toolsets,
                &self.hermes.fallback_for_toolsets,
            ),
            requires_tools: merge_vecs(&self.omiga.requires_tools, &self.hermes.requires_tools),
            fallback_for_tools: merge_vecs(
                &self.omiga.fallback_for_tools,
                &self.hermes.fallback_for_tools,
            ),
        }
    }
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
    #[serde(
        default,
        rename = "disable-model-invocation",
        alias = "disable_model_invocation"
    )]
    disable_model_invocation: bool,
    #[serde(
        default = "default_user_invocable",
        rename = "user-invocable",
        alias = "user_invocable"
    )]
    user_invocable: bool,
    /// Declared argument names for `$foo` substitution (`arguments` in TS frontmatter).
    arguments: Option<YamlStringOrList>,
    /// Search / filter tags: YAML list, single string, or comma-separated (e.g. `pdb, structure`).
    #[serde(default)]
    tags: Option<YamlStringOrList>,
    agent: Option<String>,
    effort: Option<String>,
    /// Extended metadata block: config var declarations + conditional activation.
    #[serde(default)]
    metadata: Option<SkillMetadata>,
}

/// Result of invoking the `skill` tool (TS inline / fork metadata + body for the model).
#[derive(Debug, Clone, Serialize)]
pub struct SkillInvokeOutput {
    pub success: bool,
    pub command_name: String,
    /// `inline` | `needs_fork` | `fork_unsupported` (legacy).
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
    /// Raw skill body (with substitutions applied), present only when `status == "needs_fork"`.
    /// The caller should use this as the `skill_content` argument to `run_skill_forked`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_body: Option<String>,
}

/// Where a skill was discovered from (for UI source labeling).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    /// Reserved for legacy UI / older catalog payloads — runtime discovery does not use `~/.claude/skills`.
    ClaudeUser,
    /// `~/.omiga/skills` (user-level).
    OmigaUser,
    /// `<project>/.omiga/skills` (project-level).
    OmigaProject,
    /// Skill provided by an enabled Omiga-native plugin.
    OmigaPlugin,
}

/// One discovered skill (directory name = fallback id).
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    /// Declared in frontmatter `tags` for search and UI (deduped, order preserved).
    pub tags: Vec<String>,
    pub skill_dir: PathBuf,
    /// Where this skill was discovered from.
    pub source: SkillSource,
    pub allowed_tools: Vec<String>,
    /// Conditional activation metadata (Feature 3).
    pub conditions: SkillConditions,
    /// Config vars declared in frontmatter (Feature 2).
    pub config_vars: Vec<skill_config::ConfigVar>,
}

/// Returns `true` when the skill's requirements are satisfied by the given available tool names.
///
/// - `requires_tools`: all must be in `available_tools`.
/// - `requires_toolsets`: same check (toolset name treated as a tool identifier).
/// - `fallback_for_*`: skill is a fallback; matches when the listed tool/toolset is absent.
///   In that context, the caller should pass only the *missing* tools to this function.
pub fn skill_matches_conditions(skill: &SkillEntry, available_tools: &[&str]) -> bool {
    if skill.conditions.is_empty() {
        return true; // No restrictions — always usable.
    }
    let has = |name: &str| available_tools.iter().any(|t| t.eq_ignore_ascii_case(name));

    // requires_tools: all must be present.
    if !skill.conditions.requires_tools.is_empty()
        && !skill.conditions.requires_tools.iter().all(|t| has(t))
    {
        return false;
    }
    // requires_toolsets: all must be present.
    if !skill.conditions.requires_toolsets.is_empty()
        && !skill.conditions.requires_toolsets.iter().all(|t| has(t))
    {
        return false;
    }
    true
}

fn parse_skill_tags(v: &Option<YamlStringOrList>) -> Vec<String> {
    let raw = yaml_string_or_list_to_strings(v, true);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in raw {
        let t = s.trim();
        if t.is_empty() {
            continue;
        }
        let key = t.to_lowercase();
        if seen.insert(key) {
            out.push(t.to_string());
        }
    }
    out
}

fn skill_task_score(skill: &SkillEntry, tokens: &[String]) -> i32 {
    let name = skill.name.to_lowercase();
    let desc = skill.description.to_lowercase();
    let w = skill.when_to_use.as_deref().unwrap_or("").to_lowercase();
    let tags = skill.tags.join(" ").to_lowercase();
    let blob = format!("{name} {desc} {w} {tags}");
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

fn yaml_string_or_list_to_strings(
    v: &Option<YamlStringOrList>,
    split_for_tools: bool,
) -> Vec<String> {
    let Some(v) = v else {
        return vec![];
    };
    match v {
        YamlStringOrList::Strings(s) => s
            .iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
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
            line.chars()
                .take(MAX_LISTING_DESC_CHARS - 1)
                .collect::<String>()
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

async fn read_skill_entry(
    skill_dir: &Path,
    dir_name: &str,
    source: SkillSource,
) -> Option<SkillEntry> {
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
    let allowed_tools = yaml_string_or_list_to_strings(&fm.allowed_tools, true);
    let tags = parse_skill_tags(&fm.tags);
    let (conditions, config_vars) = if let Some(meta) = &fm.metadata {
        (meta.conditions(), meta.config_vars())
    } else {
        (SkillConditions::default(), vec![])
    };
    Some(SkillEntry {
        name,
        description,
        when_to_use: fm.when_to_use.filter(|s| !s.is_empty()),
        tags,
        skill_dir: skill_dir.to_path_buf(),
        source,
        allowed_tools,
        conditions,
        config_vars,
    })
}

/// One-level **category** folders (Hermes-style): `<base>/<category>/<skill-name>/SKILL.md`.
/// Top-level `<base>/<skill-name>/SKILL.md` remains supported.
async fn collect_skills_dir(
    base: &Path,
    map: &mut HashMap<String, SkillEntry>,
    source: SkillSource,
) {
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
        let skill_md = path.join("SKILL.md");
        if tokio::fs::metadata(&skill_md).await.is_ok() {
            if let Some(sk) = read_skill_entry(&path, &dir_name, source.clone()).await {
                map.insert(sk.name.clone(), sk);
            }
        } else {
            // Category folder: scan immediate children for `*/SKILL.md`.
            let mut sub = match tokio::fs::read_dir(&path).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            loop {
                let se = match sub.next_entry().await {
                    Ok(Some(e)) => e,
                    Ok(None) => break,
                    Err(_) => break,
                };
                let sub_path = se.path();
                let Ok(sub_meta) = tokio::fs::metadata(&sub_path).await else {
                    continue;
                };
                if !sub_meta.is_dir() {
                    continue;
                }
                let leaf = se.file_name().to_string_lossy().to_string();
                if let Some(sk) = read_skill_entry(&sub_path, &leaf, source.clone()).await {
                    map.insert(sk.name.clone(), sk);
                }
            }
        }
    }
}

/// User-level Omiga skills: `~/.omiga/skills`.
fn user_skills_dir_omiga() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".omiga").join("skills"))
}

/// Load all skills for a project root (absolute or relative cwd).
///
/// Merge order (later overrides earlier):
/// 1. `~/.omiga/skills` — user-level.
/// 2. Enabled Omiga-native plugin skill roots.
/// 3. `<project>/.omiga/skills` — project-level.
pub async fn load_skills_for_project(project_root: &Path) -> Vec<SkillEntry> {
    let mut map: HashMap<String, SkillEntry> = HashMap::new();

    if let Some(omiga_user) = user_skills_dir_omiga() {
        collect_skills_dir(&omiga_user, &mut map, SkillSource::OmigaUser).await;
    }

    for plugin_skills in crate::domain::plugins::enabled_plugin_skill_roots() {
        collect_skills_dir(&plugin_skills, &mut map, SkillSource::OmigaPlugin).await;
    }

    let omiga = project_root.join(".omiga").join("skills");
    collect_skills_dir(&omiga, &mut map, SkillSource::OmigaProject).await;

    let mut list: Vec<SkillEntry> = map.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

// ── Skill cache ──────────────────────────────────────────────────────────────────────────────────

/// XOR of (dir-name-hash + mtime_secs) for every `SKILL.md` found across the search dirs.
/// Zero means no skills exist. Computed with stat-only syscalls — no file content reads.
type DirStamp = u64;

pub struct SkillCacheSlot {
    pub stamp: DirStamp,
    /// `None` = stamp known but entries not yet loaded (set by `skills_any_exist`).
    /// `Some(v)` = fully loaded; may be empty if no skills exist.
    pub entries: Option<Vec<SkillEntry>>,
}

/// Process-level skill cache keyed by project root.
pub type SkillCacheMap = HashMap<PathBuf, SkillCacheSlot>;

/// Drop cached skill entries for `project_root` so the next `load_skills_cached` rescans disk.
pub fn invalidate_skill_cache(project_root: &Path, cache: &Arc<StdMutex<SkillCacheMap>>) {
    let mut guard = cache.lock().expect("skill cache poisoned");
    guard.remove(&project_root.to_path_buf());
}

fn skill_base_dirs(project_root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(2);
    if let Some(p) = user_skills_dir_omiga() {
        dirs.push(p);
    }
    dirs.extend(crate::domain::plugins::enabled_plugin_skill_roots());
    dirs.push(project_root.join(".omiga").join("skills"));
    dirs
}

fn xor_stamp_component(mtime_secs: u64, label: &str, stamp: &mut u64) {
    let path_hash = label
        .bytes()
        .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
    *stamp ^= mtime_secs.wrapping_add(path_hash);
}

async fn stamp_skill_md(skill_md: &Path, label: &str, stamp: &mut u64) {
    let Ok(meta) = tokio::fs::metadata(skill_md).await else {
        return;
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    xor_stamp_component(mtime, label, stamp);
}

/// Includes flat `dir/SKILL.md` and one-level nested `category/dir/SKILL.md`.
async fn compute_stamp_for_skills_root(base: &Path, stamp: &mut u64) {
    let mut rd = match tokio::fs::read_dir(base).await {
        Ok(r) => r,
        Err(_) => return,
    };
    loop {
        let entry = match rd.next_entry().await {
            Ok(Some(e)) => e,
            _ => break,
        };
        let path = entry.path();
        let Ok(meta) = tokio::fs::metadata(&path).await else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let top = entry.file_name().to_string_lossy().to_string();
        let skill_md = path.join("SKILL.md");
        if tokio::fs::metadata(&skill_md).await.is_ok() {
            stamp_skill_md(&skill_md, &top, stamp).await;
        } else {
            let mut sub = match tokio::fs::read_dir(&path).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            loop {
                let se = match sub.next_entry().await {
                    Ok(Some(e)) => e,
                    Ok(None) => break,
                    Err(_) => break,
                };
                let sub_path = se.path();
                let Ok(sm) = tokio::fs::metadata(&sub_path).await else {
                    continue;
                };
                if !sm.is_dir() {
                    continue;
                }
                let leaf = se.file_name().to_string_lossy().to_string();
                let nested = sub_path.join("SKILL.md");
                if tokio::fs::metadata(&nested).await.is_ok() {
                    let label = format!("{top}/{leaf}");
                    stamp_skill_md(&nested, &label, stamp).await;
                }
            }
        }
    }
}

async fn compute_stamp(base_dirs: &[PathBuf]) -> DirStamp {
    let mut stamp: u64 = 0;
    for base in base_dirs {
        compute_stamp_for_skills_root(base, &mut stamp).await;
    }
    stamp
}

/// Load skills with a process-level mtime-stamp cache.
///
/// Hot path (stamp unchanged): only stat calls, no file reads.
/// Cold path (stamp changed or first call): full disk scan, cache updated.
pub async fn load_skills_cached(
    project_root: &Path,
    cache: &Arc<StdMutex<SkillCacheMap>>,
) -> Vec<SkillEntry> {
    let bases = skill_base_dirs(project_root);
    let stamp = compute_stamp(&bases).await;
    let key = project_root.to_path_buf();

    {
        let guard = cache.lock().expect("skill cache poisoned");
        if let Some(slot) = guard.get(&key) {
            // Only use the cached entries when they have been fully loaded (Some).
            // A slot with entries=None was written by skills_any_exist and has no entry data.
            if slot.stamp == stamp {
                if let Some(ref v) = slot.entries {
                    return v.clone();
                }
            }
        }
    }

    let entries = load_skills_for_project(project_root).await;
    {
        let mut guard = cache.lock().expect("skill cache poisoned");
        guard.insert(
            key,
            SkillCacheSlot {
                stamp,
                entries: Some(entries.clone()),
            },
        );
    }
    entries
}

/// Check whether any skills exist, using the process cache for subsequent calls.
///
/// On a cache hit the check is free (zero I/O). On a miss the stamp is computed
/// (stat-only) and stored; entry metadata is loaded lazily on the first `list_skills` call.
pub async fn skills_any_exist(project_root: &Path, cache: &Arc<StdMutex<SkillCacheMap>>) -> bool {
    let key = project_root.to_path_buf();

    {
        let guard = cache.lock().expect("skill cache poisoned");
        if let Some(slot) = guard.get(&key) {
            return slot.stamp != 0;
        }
    }

    let bases = skill_base_dirs(project_root);
    let stamp = compute_stamp(&bases).await;
    {
        let mut guard = cache.lock().expect("skill cache poisoned");
        // Use or_insert so a concurrent writer (same key, same stamp) wins — result is identical.
        guard.entry(key).or_insert(SkillCacheSlot {
            stamp,
            entries: None,
        });
    }
    stamp != 0
}

/// Try to load a single skill entry by direct path probe (O(1)) instead of scanning all dirs.
///
/// Returns `(entry, raw_skill_md_content)` so the caller can reuse the already-read file
/// content without a second `read_to_string` call.  Returns `None` when the skill's frontmatter
/// `name` differs from its directory name (caller should fall back to a full scan).
async fn try_load_skill_direct(
    project_root: &Path,
    dir_name: &str,
) -> Option<(SkillEntry, String)> {
    async fn probe(
        skill_dir: &Path,
        dir_name: &str,
        source: SkillSource,
    ) -> Option<(SkillEntry, String)> {
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
        let allowed_tools = yaml_string_or_list_to_strings(&fm.allowed_tools, true);
        let tags = parse_skill_tags(&fm.tags);
        let (conditions, config_vars) = if let Some(meta) = &fm.metadata {
            (meta.conditions(), meta.config_vars())
        } else {
            (SkillConditions::default(), vec![])
        };
        let entry = SkillEntry {
            name,
            description,
            when_to_use: fm.when_to_use.filter(|s| !s.is_empty()),
            tags,
            skill_dir: skill_dir.to_path_buf(),
            source,
            allowed_tools,
            conditions,
            config_vars,
        };
        Some((entry, raw))
    }

    let project_dir = project_root.join(".omiga").join("skills").join(dir_name);
    if let Some(r) = probe(&project_dir, dir_name, SkillSource::OmigaProject).await {
        return Some(r);
    }
    if let Some(base) = user_skills_dir_omiga() {
        if let Some(r) = probe(&base.join(dir_name), dir_name, SkillSource::OmigaUser).await {
            return Some(r);
        }
    }
    None
}

/// Normalize skill name: trim and strip a leading `/` (TS `SkillTool.validateInput`).
pub fn normalize_skill_name(raw: &str) -> String {
    let t = raw.trim();
    t.strip_prefix('/').unwrap_or(t).trim().to_string()
}

/// Find a skill by resolved `name` or by directory basename (TS `findCommand` parity for file skills).
pub fn resolve_skill_entry<'a>(
    skills: &'a [SkillEntry],
    normalized: &str,
) -> Option<&'a SkillEntry> {
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
pub fn resolve_skill_display_name(
    skills: &[SkillEntry],
    raw_skill_argument: &str,
) -> Option<String> {
    let n = normalize_skill_name(raw_skill_argument);
    resolve_skill_entry(skills, &n).map(|e| e.name.clone())
}

/// Short system-prompt note: no skill list is inlined — models discover via `list_skills`
/// (which uses the in-process cache after first load).
#[must_use]
pub fn format_skills_discovery_system_section() -> String {
    "## Skills (on-demand)\n\
     Skill metadata is **not** inlined here. Call `list_skills` when you need names and \
     fields (`description`, `when_to_use`, `tags`, `source`); optional `query` filters. The tool uses the \
     same cached scan as the rest of the app after the first load. Use `skill_view` / `skill` as appropriate.\n"
        .to_string()
}

/// Upper bound for the skill index block (names + truncated descriptions) in the system prompt.
const SKILL_INDEX_BODY_MAX_CHARS: usize = 16_000;

/// Category label for grouping in [`format_skills_index_system_section`] (Hermes-style paths).
fn skill_index_category(skill: &SkillEntry, project_root: &Path) -> String {
    if let Some(ref user_base) = user_skills_dir_omiga() {
        if skill.skill_dir.starts_with(user_base) {
            let rel = skill
                .skill_dir
                .strip_prefix(user_base)
                .unwrap_or_else(|_| Path::new(""));
            let parts: Vec<&str> = rel
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();
            if parts.len() >= 2 {
                return parts[0].to_string();
            }
            return "user".to_string();
        }
    }
    let proj_base = project_root.join(".omiga").join("skills");
    if skill.skill_dir.starts_with(&proj_base) {
        let rel = skill
            .skill_dir
            .strip_prefix(&proj_base)
            .unwrap_or_else(|_| Path::new(""));
        let parts: Vec<&str> = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        if parts.len() >= 2 {
            return parts[0].to_string();
        }
        return "project".to_string();
    }
    "skills".to_string()
}

fn truncate_desc_chars(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return "…".to_string();
    }
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    format!(
        "{}…",
        s.chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
    )
}

/// Inject **names and short descriptions** for all discovered skills into the system prompt (Hermes-style
/// index). Full `SKILL.md` bodies are still loaded on demand via `skill_view` or `skill`.
///
/// When `skills` is empty, falls back to [`format_skills_discovery_system_section`].
#[must_use]
pub fn format_skills_index_system_section(project_root: &Path, skills: &[SkillEntry]) -> String {
    if skills.is_empty() {
        return format_skills_discovery_system_section();
    }

    let preamble = "## Skills (available)\n\n\
        Scan the skills below. If one matches the user\u{2019}s task, call `skill` with that name, \
        or use `skill_view` / `list_skills` for details. Full instructions are **not** inlined \
        here \u{2014} load them with `skill_view` or `skill`.\n\n\
        <available_skills>\n";

    let mut by_cat: BTreeMap<String, Vec<&SkillEntry>> = BTreeMap::new();
    for s in skills {
        let cat = skill_index_category(s, project_root);
        by_cat.entry(cat).or_default().push(s);
    }
    for v in by_cat.values_mut() {
        v.sort_by(|a, b| a.name.cmp(&b.name));
    }

    let mut desc_limit = MAX_LISTING_DESC_CHARS;
    let mut body = String::new();
    loop {
        body.clear();
        for (cat, entries) in &by_cat {
            body.push_str(&format!("  {}:\n", cat));
            for s in entries {
                let desc = truncate_desc_chars(&s.description, desc_limit);
                body.push_str(&format!("    - {}: {}\n", s.name, desc));
            }
        }
        if body.len() <= SKILL_INDEX_BODY_MAX_CHARS {
            break;
        }
        if desc_limit > 48 {
            desc_limit = (desc_limit * 2 / 3).max(48);
            continue;
        }
        // Still too large: hard-truncate (very many skills).
        let note = "\n  … (index truncated; call `list_skills` for the full catalog.)\n";
        let cap = SKILL_INDEX_BODY_MAX_CHARS.saturating_sub(note.len());
        if body.len() > cap {
            let mut t = body.chars().take(cap).collect::<String>();
            // Avoid cutting mid-line: snap to last newline if possible
            if let Some(pos) = t.rfind('\n') {
                if pos > cap * 3 / 4 {
                    t.truncate(pos + 1);
                }
            }
            body = t;
        }
        body.push_str(note);
        break;
    }

    let footer = "</available_skills>\n";
    let mut out = String::with_capacity(preamble.len() + body.len() + footer.len());
    out.push_str(preamble);
    out.push_str(&body);
    out.push_str(footer);
    out
}

/// JSON for `list_skills` tool: metadata only, no full SKILL.md body.
///
/// When `query` is set, filters by substring. When `task_rank_context` is set, entries are ordered by
/// keyword overlap with that text (higher first), then by name. With no `query` and no task context,
/// order follows the loaded skill list.
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
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        source: SkillSource,
        /// Conditional activation fields — omitted from JSON when empty.
        #[serde(skip_serializing_if = "SkillConditions::is_empty")]
        conditions: SkillConditions,
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
                let tags_joined = e.tags.join(" ").to_lowercase();
                name.contains(qq.as_str())
                    || desc.contains(qq.as_str())
                    || w.contains(qq.as_str())
                    || tags_joined.contains(qq.as_str())
                    || e.tags.iter().any(|t| t.eq_ignore_ascii_case(qq))
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
            when_to_use: e.when_to_use.as_ref().map(|w| truncate_listing(w)),
            tags: e.tags.clone(),
            source: e.source.clone(),
            conditions: e.conditions.clone(),
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
///
/// When `preloaded_skills` is provided the list is used directly (avoids a redundant disk scan).
/// Otherwise skills are loaded from disk under `project_root` (Omiga skill dirs only).
pub async fn invoke_skill_detailed(
    project_root: &Path,
    raw_skill_name: &str,
    args: &str,
) -> Result<SkillInvokeOutput, String> {
    invoke_skill_detailed_with_cache(project_root, raw_skill_name, args, None).await
}

/// Like [`invoke_skill_detailed`] but accepts an already-loaded skill list to avoid a redundant
/// `load_skills_for_project` call when the caller already has the list.
pub async fn invoke_skill_detailed_with_cache(
    project_root: &Path,
    raw_skill_name: &str,
    args: &str,
    preloaded_skills: Option<&[SkillEntry]>,
) -> Result<SkillInvokeOutput, String> {
    // ========== Validation Phase (aligned with SkillTool.validateInput in TS) ==========
    let normalized = normalize_skill_name(raw_skill_name);
    if normalized.is_empty() {
        return Err("Error code 1: Invalid skill format: empty name".to_string());
    }

    // Check for leading slash (accepted but stripped) - telemetry only
    let had_leading_slash = raw_skill_name.trim().starts_with('/');
    if had_leading_slash {
        tracing::debug!(skill = %normalized, "Skill name had leading slash, stripped");
    }

    // Log skill invocation start (aligned with SkillTool telemetry)
    let start_time = std::time::Instant::now();
    tracing::info!(
        skill = %normalized,
        args_len = args.len(),
        had_leading_slash,
        "SkillTool invoking skill (start)"
    );

    // Resolve skill entry + raw SKILL.md content (three paths, cheapest first):
    // 1. Preloaded list — no I/O; entry found, file read separately below.
    // 2. Direct path probe — reads exactly one SKILL.md and returns entry + content together.
    // 3. Full scan fallback — only when `name` in frontmatter differs from directory name.
    let direct_entry;
    let owned_full;
    let direct_raw: String;
    let (entry, raw_opt): (&SkillEntry, Option<&str>) = if let Some(s) = preloaded_skills {
        let e = resolve_skill_entry(s, &normalized)
            .ok_or_else(|| format!("Error code 2: Unknown skill: {normalized}"))?;
        (e, None)
    } else if let Some((e, r)) = try_load_skill_direct(project_root, &normalized).await {
        direct_entry = e;
        direct_raw = r;
        (&direct_entry, Some(direct_raw.as_str()))
    } else {
        owned_full = load_skills_for_project(project_root).await;
        let e = resolve_skill_entry(&owned_full, &normalized)
            .ok_or_else(|| format!("Error code 2: Unknown skill: {normalized}"))?;
        (e, None)
    };

    // Use the already-read content from the direct probe; otherwise read from disk.
    let owned_raw: String;
    let raw: &str = if let Some(r) = raw_opt {
        r
    } else {
        let path = entry.skill_dir.join("SKILL.md");
        owned_raw = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("read SKILL.md: {e}"))?;
        &owned_raw
    };
    let (fm, body) = parse_frontmatter(raw)?;

    // Check disable-model-invocation (Error code 4 in SkillTool)
    if fm.disable_model_invocation {
        return Err(format!(
            "Error code 4: Skill {normalized} cannot be used with the skill tool due to disable-model-invocation"
        ));
    }

    let command_name = entry.name.clone();
    let source = &entry.source;
    let allowed_tools = yaml_string_or_list_to_strings(&fm.allowed_tools, true);
    let arg_names = yaml_string_or_list_to_strings(&fm.arguments, false);

    let dir_str = entry.skill_dir.to_string_lossy().to_string();
    let is_fork = fm
        .context
        .as_deref()
        .map(|c| c.eq_ignore_ascii_case("fork"))
        .unwrap_or(false);

    tracing::info!(
        skill = %command_name,
        source = ?source,
        is_fork,
        has_allowed_tools = !allowed_tools.is_empty(),
        has_model_override = fm.model.is_some(),
        "Skill resolved, preparing execution"
    );

    let mut md = format!("Base directory for this skill: {dir_str}\n\n{body}");
    md = md.replace("${CLAUDE_SKILL_DIR}", &dir_str);
    md = md.replace("${OMIGA_SKILL_DIR}", &dir_str);
    md = substitute_arguments(md, args, true, &arg_names);

    // Inject config values if the skill declares any.
    let config_vars_fm = fm
        .metadata
        .as_ref()
        .map(|m| m.config_vars())
        .unwrap_or_default();
    if !config_vars_fm.is_empty() {
        let resolved = skill_config::resolve_config_vars(&config_vars_fm, project_root);
        if let Some(block) = skill_config::format_config_injection(&resolved) {
            md = format!("{block}\n\n{md}");
        }
    }

    let mut body_for_model = String::new();
    body_for_model.push_str(&format!("Launching skill: {command_name}\n\n"));

    let mut fork_skill_body: Option<String> = None;
    let status = if is_fork {
        // Signal to the chat layer that this skill should be executed as a forked sub-agent.
        // We store the processed skill body so the caller can pass it to `run_skill_forked`
        // without re-reading and re-substituting.
        let meta = serde_json::json!({
            "success": true,
            "commandName": command_name,
            "status": "needs_fork",
            "allowedTools": if allowed_tools.is_empty() { serde_json::Value::Null } else { serde_json::to_value(&allowed_tools).unwrap() },
            "model": fm.model,
            "effort": fm.effort,
            "agent": fm.agent,
            "userInvocable": fm.user_invocable,
        });
        body_for_model.push_str(&serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?);
        body_for_model.push_str("\n\n---\n\n");
        body_for_model.push_str(&md);
        fork_skill_body = Some(md.clone());
        "needs_fork"
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

    let duration_ms = start_time.elapsed().as_millis();
    tracing::info!(
        skill = %command_name,
        status,
        duration_ms,
        result_len = body_for_model.len(),
        "SkillTool skill invocation completed"
    );

    Ok(SkillInvokeOutput {
        success: true,
        command_name: command_name.clone(),
        status: status.to_string(),
        allowed_tools: allowed_tools.clone(),
        model: fm.model.clone(),
        effort: fm.effort.clone(),
        agent: fm.agent.clone(),
        formatted_tool_result: body_for_model,
        skill_body: fork_skill_body,
    })
}

/// Invoke a skill and return the formatted tool result string.
/// Pass `preloaded_skills` to skip the internal directory scan when the caller already has the list.
pub async fn invoke_skill_with_cache(
    project_root: &Path,
    raw_skill_name: &str,
    args: &str,
    preloaded_skills: &[SkillEntry],
) -> Result<String, String> {
    let out = invoke_skill_detailed_with_cache(
        project_root,
        raw_skill_name,
        args,
        Some(preloaded_skills),
    )
    .await?;
    Ok(out.formatted_tool_result)
}

/// Find a skill entry by name (exact match, case-insensitive).
#[must_use]
pub fn find_skill_entry<'a>(skills: &'a [SkillEntry], name: &str) -> Option<&'a SkillEntry> {
    let normalized = name.trim().to_lowercase();
    skills
        .iter()
        .find(|e| e.name.trim().to_lowercase() == normalized)
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
        let s = substitute_arguments("Hi $ARGUMENTS and $0 end".to_string(), "a b", false, &[]);
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
        tokio::fs::create_dir_all(&skill_dir).await.expect("mkdir");
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
        let out = invoke_skill_detailed(dir.path(), "/demo", "hello")
            .await
            .expect("invoke");
        assert_eq!(out.status, "inline");
        assert!(out.allowed_tools.contains(&"bash".to_string()));
        assert!(out.formatted_tool_result.contains("Launching skill: demo"));
        assert!(out.formatted_tool_result.contains("Line hello"));
    }

    #[test]
    fn list_skills_json_orders_by_task_when_context_set() {
        let skills = vec![
            SkillEntry {
                name: "alpha-help".to_string(),
                description: "generic".to_string(),
                when_to_use: None,
                tags: vec![],
                skill_dir: PathBuf::from("/tmp/a"),
                source: SkillSource::OmigaProject,
                allowed_tools: vec![],
                conditions: SkillConditions::default(),
                config_vars: vec![],
            },
            SkillEntry {
                name: "postgres-patterns".to_string(),
                description: "SQL tips".to_string(),
                when_to_use: Some("database".to_string()),
                tags: vec!["sql".to_string(), "postgres".to_string()],
                skill_dir: PathBuf::from("/tmp/b"),
                source: SkillSource::OmigaProject,
                allowed_tools: vec![],
                conditions: SkillConditions::default(),
                config_vars: vec![],
            },
        ];
        let json = list_skills_metadata_json(&skills, None, Some("postgres tuning"));
        let pg = json.find("postgres-patterns").expect("postgres in json");
        let al = json.find("alpha-help").expect("alpha in json");
        assert!(pg < al);
        assert!(
            json.contains("\"tags\"") && json.contains("postgres"),
            "expected tags in JSON: {json}"
        );
    }

    #[test]
    fn list_skills_query_matches_tags() {
        let skills = vec![SkillEntry {
            name: "t".to_string(),
            description: "d".to_string(),
            when_to_use: None,
            tags: vec!["alphafold".to_string()],
            skill_dir: PathBuf::from("/tmp/t"),
            source: SkillSource::OmigaProject,
            allowed_tools: vec![],
            conditions: SkillConditions::default(),
            config_vars: vec![],
        }];
        let json = list_skills_metadata_json(&skills, Some("alphafold"), None);
        assert!(json.contains("\"count\": 1"), "{json}");
        let empty = list_skills_metadata_json(&skills, Some("nomatch-xyz"), None);
        assert!(empty.contains("\"count\": 0"), "{empty}");
    }

    #[tokio::test]
    async fn load_skills_includes_tags_from_frontmatter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join(".omiga").join("skills").join("tagged");
        tokio::fs::create_dir_all(&skill_dir).await.expect("mkdir");
        let raw = r#"---
name: tagged
description: Has tags
tags:
  - pdb
  - alphafold
---
body
"#;
        tokio::fs::write(skill_dir.join("SKILL.md"), raw)
            .await
            .expect("write");
        let skills = load_skills_for_project(dir.path()).await;
        let sk = skills
            .iter()
            .find(|s| s.name == "tagged")
            .expect("tagged skill");
        assert_eq!(sk.tags, vec!["pdb", "alphafold"]);
    }

    #[tokio::test]
    async fn load_skills_includes_category_nested_skill() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cat = dir.path().join(".omiga").join("skills").join("gstack");
        let skill_dir = cat.join("nested-demo");
        tokio::fs::create_dir_all(&skill_dir).await.expect("mkdir");
        let raw = r#"---
name: nested-demo
description: From category folder
---
ok
"#;
        tokio::fs::write(skill_dir.join("SKILL.md"), raw)
            .await
            .expect("write");
        let skills = load_skills_for_project(dir.path()).await;
        let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"nested-demo"),
            "expected nested-demo, got {names:?}"
        );
    }
}
