//! Hermes-style `skill_view`: load full `SKILL.md` or a file under the skill directory (read-only).
//! Does **not** run skill substitution / inline workflow — use `skill` to execute.

use std::path::Path;

use serde::Serialize;

use super::{normalize_skill_name, parse_frontmatter, resolve_skill_entry, SkillEntry};

const MAX_READ_BYTES: usize = 2_048_576;

#[derive(Serialize)]
struct SkillMeta {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    when_to_use: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    source: super::SkillSource,
}

#[derive(Serialize)]
struct SkillViewOk {
    success: bool,
    skill: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
    /// Raw file text (full SKILL.md or requested file)
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    skill_metadata: Option<SkillMeta>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    linked_files: Vec<String>,
    hint: &'static str,
}

fn list_linked_files(skill_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(skill_dir) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                if matches!(
                    name.as_str(),
                    "references" | "templates" | "scripts" | "assets"
                ) {
                    if let Ok(w) = walk_rel(skill_dir, &p, 80) {
                        out.extend(w);
                    }
                }
            } else if name != "SKILL.md" {
                out.push(name);
            }
        }
    }
    out.sort();
    out.truncate(80);
    out
}

fn walk_rel(base: &Path, dir: &Path, budget: usize) -> Result<Vec<String>, ()> {
    let mut v = Vec::new();
    let mut remaining = budget;
    let rd = std::fs::read_dir(dir).map_err(|_| ())?;
    for e in rd.flatten() {
        if remaining == 0 {
            break;
        }
        let p = e.path();
        let rel = p.strip_prefix(base).map_err(|_| ())?;
        if p.is_dir() {
            if let Ok(sub) = walk_rel(base, &p, remaining) {
                remaining = remaining.saturating_sub(sub.len());
                v.extend(sub);
            }
        } else {
            v.push(rel.to_string_lossy().replace('\\', "/"));
            remaining -= 1;
        }
    }
    Ok(v)
}

/// Hermes-aligned progressive disclosure: full `SKILL.md` or `references/foo.md` without executing the skill.
/// Pass the same **integration-filtered** skill list as `list_skills` so disabled skills are not readable.
pub async fn execute_skill_view(
    skills: &[SkillEntry],
    skill_name: &str,
    file_path: Option<&str>,
) -> Result<serde_json::Value, String> {
    let n = normalize_skill_name(skill_name);
    if n.is_empty() {
        return Err("skill_view: `skill` is empty".to_string());
    }

    let entry = resolve_skill_entry(skills, &n)
        .ok_or_else(|| format!("skill_view: unknown skill `{n}`"))?;

    let skill_dir = &entry.skill_dir;

    if let Some(rel) = file_path.map(str::trim).filter(|s| !s.is_empty()) {
        if !rel.eq_ignore_ascii_case("SKILL.md") {
            if rel.contains("..") || rel.starts_with('/') || rel.starts_with('\\') {
                return Err("skill_view: `file_path` must be relative with no `..`".to_string());
            }
            let path = skill_dir.join(rel);
            if !path.starts_with(skill_dir) {
                return Err("skill_view: invalid path".to_string());
            }
            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| format!("skill_view: {e}"))?;
            if meta.len() as usize > MAX_READ_BYTES {
                return Err(format!(
                    "skill_view: file too large (max {} bytes)",
                    MAX_READ_BYTES
                ));
            }
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("skill_view: read: {e}"))?;
            return serde_json::to_value(SkillViewOk {
                success: true,
                skill: entry.name.clone(),
                file_path: Some(rel.to_string()),
                content,
                skill_metadata: None,
                linked_files: vec![],
                hint: "Use `skill` to execute this skill with arguments when you need the workflow, not just the text.",
            })
            .map_err(|e| e.to_string());
        }
    }

    let path = skill_dir.join("SKILL.md");
    let raw = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("skill_view: read SKILL.md: {e}"))?;
    let _ = parse_frontmatter(&raw).map_err(|e| format!("skill_view: invalid SKILL.md: {e}"))?;

    let linked_files = list_linked_files(skill_dir);

    let skill_metadata = SkillMeta {
        name: entry.name.clone(),
        description: entry.description.clone(),
        when_to_use: entry.when_to_use.clone(),
        tags: entry.tags.clone(),
        source: entry.source.clone(),
    };

    serde_json::to_value(SkillViewOk {
        success: true,
        skill: entry.name.clone(),
        file_path: None,
        content: raw,
        skill_metadata: Some(skill_metadata),
        linked_files,
        hint: "For a reference file, call skill_view with `file_path`. To run the workflow, use `skill`.",
    })
    .map_err(|e| e.to_string())
}
