//! Project-scoped skill CRUD under `<project>/.omiga/skills/<name>/` or
//! `<project>/.omiga/skills/<category>/<name>/` when `category` is set on **create**.
//! User-level `~/.omiga/skills` skills are read-only here — copy into the project to edit.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use serde::Deserialize;
use serde::Serialize;

use super::{
    find_skill_entry,
    fuzzy_match::fuzzy_find_and_replace,
    invalidate_skill_cache, load_skills_for_project, normalize_skill_name, parse_frontmatter,
    resolve_skill_entry,
    skill_guard::{check_content, scan_content},
    SkillCacheMap, SkillEntry, SkillSource,
};

const MAX_SKILL_MD_BYTES: usize = 1_048_576; // 1 MiB
const MAX_AUX_FILE_BYTES: usize = 2_048_576; // 2 MiB

// ---------------------------------------------------------------------------
// Atomic write helper — temp file in same dir + rename (crash-safe on POSIX).
// ---------------------------------------------------------------------------

/// Write `content` to `path` atomically: create a sibling `.tmp.<uuid>` file,
/// flush it, then rename into place. Guarantees that readers never see a
/// partial write even if the process is killed mid-write.
async fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("atomic_write: no parent directory for {}", path.display()))?;

    // Unique temp name inside the same directory so rename stays on-device.
    let tmp = dir.join(format!(".tmp.{}.write", uuid::Uuid::new_v4().simple()));

    tokio::fs::write(&tmp, content)
        .await
        .map_err(|e| format!("atomic_write: write temp: {e}"))?;

    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        // Best-effort cleanup before surfacing the error.
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("atomic_write: rename into place: {e}"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Create,
    Patch,
    Edit,
    Delete,
    #[serde(rename = "write_file")]
    WriteFile,
    #[serde(rename = "remove_file")]
    RemoveFile,
}

#[derive(Debug, Deserialize)]
struct SkillManageArgs {
    action: Action,
    /// Skill logical name (matches `name` in frontmatter or directory name).
    name: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    old_string: Option<String>,
    #[serde(default)]
    new_string: Option<String>,
    /// Relative to skill directory (e.g. `references/notes.md`)
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    file_content: Option<String>,
    /// When true, `patch` replaces every occurrence of `old_string` (otherwise exactly one match).
    #[serde(default)]
    replace_all: bool,
    /// Optional one-level folder under `.omiga/skills/` — **create** only. Results in
    /// `.omiga/skills/<category>/<name>/`. Omit for flat `skills/<name>/`.
    #[serde(default)]
    category: Option<String>,
}

#[derive(Serialize)]
struct SkillManageOk {
    success: bool,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// Security-scan warnings (caution-level findings). The write was still allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<String>,
}

fn sanitize_skill_dir_name(raw: &str) -> Result<String, String> {
    let n = normalize_skill_name(raw).to_lowercase();
    if n.is_empty() {
        return Err("skill_manage: `name` is empty".to_string());
    }
    if n.len() > 64 {
        return Err("skill_manage: `name` exceeds 64 characters".to_string());
    }
    for ch in n.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-' {
            continue;
        }
        return Err(format!(
            "skill_manage: invalid character in `name` ({ch:?}) — use a-z, 0-9, _, -"
        ));
    }
    Ok(n)
}

/// Optional category segment: same rules as `name` directory; empty / missing → flat layout.
fn sanitize_category_segment(raw: &Option<String>) -> Result<Option<String>, String> {
    let Some(s) = raw.as_ref().map(|x| x.trim()).filter(|x| !x.is_empty()) else {
        return Ok(None);
    };
    Ok(Some(sanitize_skill_dir_name(s)?))
}

fn project_skills_root(project_root: &Path) -> PathBuf {
    project_root.join(".omiga").join("skills")
}

fn resolve_project_skill_dir(project_root: &Path, dir_name: &str) -> PathBuf {
    project_skills_root(project_root).join(dir_name)
}

/// Returns the skill entry if it exists **under the project** `.omiga/skills` tree.
async fn resolve_project_skill_entry<'a>(
    project_root: &Path,
    skills: &'a [SkillEntry],
    name_arg: &str,
) -> Result<&'a SkillEntry, String> {
    let n = normalize_skill_name(name_arg);
    let e = resolve_skill_entry(skills, &n)
        .ok_or_else(|| format!("skill_manage: unknown skill `{n}`"))?;
    if e.source != SkillSource::OmigaProject {
        return Err(format!(
            "skill_manage: skill `{n}` lives under ~/.omiga/skills (user install). \
             Copy or recreate it under {} to manage it here.",
            project_skills_root(project_root).display()
        ));
    }
    Ok(e)
}

fn ensure_relative_safe(rel: &str) -> Result<PathBuf, String> {
    let t = rel.trim();
    if t.is_empty() {
        return Err("skill_manage: `file_path` is empty".to_string());
    }
    if t.contains("..") || t.starts_with('/') || t.starts_with('\\') {
        return Err(format!(
            "skill_manage: `file_path` must be relative with no `..`: {t:?}"
        ));
    }
    Ok(PathBuf::from(t))
}

/// Run `skill_manage` and invalidate the skill cache on success.
pub async fn execute_skill_manage(
    project_root: &Path,
    arguments_json: &str,
    skill_cache: &Arc<StdMutex<SkillCacheMap>>,
) -> Result<serde_json::Value, String> {
    let args: SkillManageArgs = serde_json::from_str(arguments_json)
        .map_err(|e| format!("skill_manage: invalid JSON: {e}"))?;

    let dir_key = sanitize_skill_dir_name(&args.name)?;
    let category_seg = sanitize_category_segment(&args.category)?;
    if category_seg.is_some() && !matches!(args.action, Action::Create) {
        return Err("skill_manage: `category` is only valid for action `create`".to_string());
    }
    let skills = load_skills_for_project(project_root).await;

    let ok = match args.action {
        Action::Create => {
            let content = args
                .content
                .ok_or_else(|| "skill_manage: `content` is required for create".to_string())?;
            if content.len() > MAX_SKILL_MD_BYTES {
                return Err(format!(
                    "skill_manage: SKILL.md content exceeds {} bytes",
                    MAX_SKILL_MD_BYTES
                ));
            }
            let (fm, _) = parse_frontmatter(&content)
                .map_err(|e| format!("skill_manage: invalid SKILL.md (frontmatter): {e}"))?;
            let n = fm.name.as_deref().unwrap_or("").trim();
            let d = fm.description.as_deref().unwrap_or("").trim();
            if n.is_empty() || d.is_empty() {
                return Err(
                    "skill_manage: create requires YAML frontmatter with non-empty `name` and `description`"
                        .to_string(),
                );
            }
            if let Some(existing) = find_skill_entry(&skills, n) {
                if existing.source == SkillSource::OmigaProject {
                    return Err(format!(
                        "skill_manage: a project skill named `{}` already exists",
                        existing.name
                    ));
                }
            }
            // Security scan before writing
            let scan = scan_content(&dir_key, &content);
            let warnings = check_content(&scan)
                .map_err(|e| format!("skill_manage: security scan blocked create: {e}"))?;

            let skill_dir = match &category_seg {
                None => resolve_project_skill_dir(project_root, &dir_key),
                Some(cat) => project_skills_root(project_root).join(cat).join(&dir_key),
            };
            if tokio::fs::metadata(&skill_dir).await.is_ok() {
                return Err(format!(
                    "skill_manage: skill directory already exists: {}",
                    skill_dir.display()
                ));
            }
            tokio::fs::create_dir_all(&skill_dir)
                .await
                .map_err(|e| format!("skill_manage: mkdir: {e}"))?;
            let path = skill_dir.join("SKILL.md");
            atomic_write(&path, content.as_bytes())
                .await
                .map_err(|e| format!("skill_manage: {e}"))?;
            let msg = match &category_seg {
                None => format!("Created skill `{dir_key}`"),
                Some(cat) => format!("Created skill `{dir_key}` under category `{cat}`"),
            };
            SkillManageOk {
                success: true,
                action: "create".to_string(),
                path: Some(path.to_string_lossy().to_string()),
                message: Some(msg),
                warnings,
            }
        }
        Action::Patch => {
            let old_s = args
                .old_string
                .ok_or_else(|| "skill_manage: `old_string` is required for patch".to_string())?;
            let new_s = args
                .new_string
                .ok_or_else(|| "skill_manage: `new_string` is required for patch".to_string())?;
            if old_s.is_empty() {
                return Err("skill_manage: `old_string` must not be empty".to_string());
            }
            let entry = resolve_project_skill_entry(project_root, &skills, &args.name).await?;

            let rel = args
                .file_path
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let is_skill_md =
                rel.is_none() || rel.is_some_and(|r| r.eq_ignore_ascii_case("SKILL.md"));

            let path = if is_skill_md {
                entry.skill_dir.join("SKILL.md")
            } else {
                let sub = rel.ok_or_else(|| {
                    "skill_manage: `file_path` is required when patching a file other than SKILL.md"
                        .to_string()
                })?;
                let rel_pb = ensure_relative_safe(sub)?;
                entry.skill_dir.join(rel_pb)
            };

            if !path.starts_with(&entry.skill_dir) {
                return Err("skill_manage: invalid patch path".to_string());
            }

            let max_bytes = if is_skill_md {
                MAX_SKILL_MD_BYTES
            } else {
                MAX_AUX_FILE_BYTES
            };

            let raw = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("skill_manage: read file: {e}"))?;

            let (patched, _count) = fuzzy_find_and_replace(&raw, &old_s, &new_s, args.replace_all)
                .map_err(|e| {
                    format!(
                        "skill_manage: patch `{}`: {e}",
                        path.file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    )
                })?;

            if patched.len() > max_bytes {
                return Err(format!(
                    "skill_manage: patched file would exceed size limit ({max_bytes} bytes)"
                ));
            }
            if is_skill_md {
                let _ = parse_frontmatter(&patched)
                    .map_err(|e| format!("skill_manage: patched SKILL.md invalid: {e}"))?;
                // Security scan the patched SKILL.md
                let scan = scan_content(&args.name, &patched);
                let warnings = check_content(&scan)
                    .map_err(|e| format!("skill_manage: security scan blocked patch: {e}"))?;
                atomic_write(&path, patched.as_bytes())
                    .await
                    .map_err(|e| format!("skill_manage: {e}"))?;
                SkillManageOk {
                    success: true,
                    action: "patch".to_string(),
                    path: Some(path.to_string_lossy().to_string()),
                    message: Some("Patched SKILL.md".to_string()),
                    warnings,
                }
            } else {
                atomic_write(&path, patched.as_bytes())
                    .await
                    .map_err(|e| format!("skill_manage: {e}"))?;
                SkillManageOk {
                    success: true,
                    action: "patch".to_string(),
                    path: Some(path.to_string_lossy().to_string()),
                    message: Some(format!("Patched {}", rel.unwrap_or(""))),
                    warnings: None,
                }
            }
        }
        Action::Edit => {
            let content = args
                .content
                .ok_or_else(|| "skill_manage: `content` is required for edit".to_string())?;
            if content.len() > MAX_SKILL_MD_BYTES {
                return Err("skill_manage: content exceeds size limit".to_string());
            }
            let (fm, _) = parse_frontmatter(&content)
                .map_err(|e| format!("skill_manage: invalid SKILL.md: {e}"))?;
            let n = fm.name.as_deref().unwrap_or("").trim();
            let d = fm.description.as_deref().unwrap_or("").trim();
            if n.is_empty() || d.is_empty() {
                return Err(
                    "skill_manage: edit requires YAML frontmatter with non-empty `name` and `description`"
                        .to_string(),
                );
            }
            // Security scan before writing
            let scan = scan_content(&args.name, &content);
            let warnings = check_content(&scan)
                .map_err(|e| format!("skill_manage: security scan blocked edit: {e}"))?;

            let entry = resolve_project_skill_entry(project_root, &skills, &args.name).await?;
            let path = entry.skill_dir.join("SKILL.md");
            atomic_write(&path, content.as_bytes())
                .await
                .map_err(|e| format!("skill_manage: {e}"))?;
            SkillManageOk {
                success: true,
                action: "edit".to_string(),
                path: Some(path.to_string_lossy().to_string()),
                message: Some("Replaced SKILL.md".to_string()),
                warnings,
            }
        }
        Action::Delete => {
            let entry = resolve_project_skill_entry(project_root, &skills, &args.name).await?;
            let dir = entry.skill_dir.clone();
            tokio::fs::remove_dir_all(&dir)
                .await
                .map_err(|e| format!("skill_manage: remove_dir_all: {e}"))?;
            SkillManageOk {
                success: true,
                action: "delete".to_string(),
                path: Some(dir.to_string_lossy().to_string()),
                message: Some(format!("Removed skill directory for `{}`", entry.name)),
                warnings: None,
            }
        }
        Action::WriteFile => {
            let rel = args.file_path.ok_or_else(|| {
                "skill_manage: `file_path` is required for write_file".to_string()
            })?;
            let fc = args.file_content.ok_or_else(|| {
                "skill_manage: `file_content` is required for write_file".to_string()
            })?;
            if fc.len() > MAX_AUX_FILE_BYTES {
                return Err("skill_manage: file_content exceeds size limit".to_string());
            }
            if rel.trim().eq_ignore_ascii_case("SKILL.md") {
                return Err(
                    "skill_manage: use `patch` or `edit` to change SKILL.md, not write_file"
                        .to_string(),
                );
            }
            let rel_pb = ensure_relative_safe(&rel)?;
            let entry = resolve_project_skill_entry(project_root, &skills, &args.name).await?;
            let dest = entry.skill_dir.join(&rel_pb);
            if !dest.starts_with(&entry.skill_dir) {
                return Err("skill_manage: invalid write_file path".to_string());
            }
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("skill_manage: mkdir: {e}"))?;
            }
            atomic_write(&dest, fc.as_bytes())
                .await
                .map_err(|e| format!("skill_manage: {e}"))?;
            SkillManageOk {
                success: true,
                action: "write_file".to_string(),
                path: Some(dest.to_string_lossy().to_string()),
                message: None,
                warnings: None,
            }
        }
        Action::RemoveFile => {
            let rel = args.file_path.ok_or_else(|| {
                "skill_manage: `file_path` is required for remove_file".to_string()
            })?;
            if rel.trim().eq_ignore_ascii_case("SKILL.md") {
                return Err("skill_manage: refusing to remove SKILL.md — use `delete` to remove the whole skill".to_string());
            }
            let rel_pb = ensure_relative_safe(&rel)?;
            let entry = resolve_project_skill_entry(project_root, &skills, &args.name).await?;
            let dest = entry.skill_dir.join(&rel_pb);
            if !dest.starts_with(&entry.skill_dir) {
                return Err("skill_manage: invalid remove_file path".to_string());
            }
            tokio::fs::remove_file(&dest)
                .await
                .map_err(|e| format!("skill_manage: remove_file: {e}"))?;
            SkillManageOk {
                success: true,
                action: "remove_file".to_string(),
                path: Some(dest.to_string_lossy().to_string()),
                message: None,
                warnings: None,
            }
        }
    };

    invalidate_skill_cache(project_root, skill_cache);

    serde_json::to_value(&ok).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_patch_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let cache = Arc::new(StdMutex::new(SkillCacheMap::new()));

        let raw = r#"---
name: demo-skill
description: Test
---
Hello WORLD
"#;
        let j = serde_json::json!({
            "action": "create",
            "name": "demo-skill",
            "content": raw,
        });
        execute_skill_manage(root, &j.to_string(), &cache)
            .await
            .expect("create");

        let j2 = serde_json::json!({
            "action": "patch",
            "name": "demo-skill",
            "old_string": "WORLD",
            "new_string": "Omiga",
        });
        execute_skill_manage(root, &j2.to_string(), &cache)
            .await
            .expect("patch");

        let p = resolve_project_skill_dir(root, "demo-skill").join("SKILL.md");
        let s = tokio::fs::read_to_string(&p).await.unwrap();
        assert!(s.contains("Hello Omiga"));

        let j3 = serde_json::json!({
            "action": "delete",
            "name": "demo-skill",
        });
        execute_skill_manage(root, &j3.to_string(), &cache)
            .await
            .expect("delete");
        assert!(tokio::fs::metadata(p).await.is_err());
    }

    #[tokio::test]
    async fn create_rejects_missing_description() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let cache = Arc::new(StdMutex::new(SkillCacheMap::new()));
        let raw = r#"---
name: x
---
body
"#;
        let j = serde_json::json!({
            "action": "create",
            "name": "x-skill",
            "content": raw,
        });
        let err = execute_skill_manage(root, &j.to_string(), &cache)
            .await
            .unwrap_err();
        assert!(err.contains("description"));
    }

    #[tokio::test]
    async fn patch_replace_all_and_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let cache = Arc::new(StdMutex::new(SkillCacheMap::new()));

        let raw = r#"---
name: demo-skill
description: Test
---
FOO FOO
"#;
        let j = serde_json::json!({
            "action": "create",
            "name": "demo-skill",
            "content": raw,
        });
        execute_skill_manage(root, &j.to_string(), &cache)
            .await
            .expect("create");

        let j_rep = serde_json::json!({
            "action": "patch",
            "name": "demo-skill",
            "old_string": "FOO",
            "new_string": "BAR",
            "replace_all": true,
        });
        execute_skill_manage(root, &j_rep.to_string(), &cache)
            .await
            .expect("replace_all patch");
        let p = resolve_project_skill_dir(root, "demo-skill").join("SKILL.md");
        let s = tokio::fs::read_to_string(&p).await.unwrap();
        assert!(s.contains("BAR BAR"));

        let j_w = serde_json::json!({
            "action": "write_file",
            "name": "demo-skill",
            "file_path": "references/n.txt",
            "file_content": "alpha beta alpha",
        });
        execute_skill_manage(root, &j_w.to_string(), &cache)
            .await
            .expect("write_file");

        let j_fp = serde_json::json!({
            "action": "patch",
            "name": "demo-skill",
            "file_path": "references/n.txt",
            "old_string": "alpha",
            "new_string": "gamma",
            "replace_all": true,
        });
        execute_skill_manage(root, &j_fp.to_string(), &cache)
            .await
            .expect("patch file_path");
        let p2 = resolve_project_skill_dir(root, "demo-skill").join("references/n.txt");
        let s2 = tokio::fs::read_to_string(&p2).await.unwrap();
        assert_eq!(s2, "gamma beta gamma");
    }

    #[tokio::test]
    async fn create_writes_category_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let cache = Arc::new(StdMutex::new(SkillCacheMap::new()));
        let raw = r#"---
name: cat-skill
description: In a category
---
Hi
"#;
        let j = serde_json::json!({
            "action": "create",
            "name": "cat-skill",
            "category": "bio",
            "content": raw,
        });
        execute_skill_manage(root, &j.to_string(), &cache)
            .await
            .expect("create");
        let p = root
            .join(".omiga")
            .join("skills")
            .join("bio")
            .join("cat-skill")
            .join("SKILL.md");
        assert!(tokio::fs::metadata(&p).await.is_ok());
        let s = tokio::fs::read_to_string(&p).await.unwrap();
        assert!(s.contains("In a category"));
    }

    #[tokio::test]
    async fn category_only_allowed_on_create() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let cache = Arc::new(StdMutex::new(SkillCacheMap::new()));
        let raw = r#"---
name: x-skill
description: y
---
z
"#;
        let j = serde_json::json!({
            "action": "create",
            "name": "x-skill",
            "content": raw,
        });
        execute_skill_manage(root, &j.to_string(), &cache)
            .await
            .expect("create");
        let err = execute_skill_manage(
            root,
            &serde_json::json!({
                "action": "patch",
                "name": "x-skill",
                "category": "bio",
                "old_string": "z",
                "new_string": "w",
            })
            .to_string(),
            &cache,
        )
        .await
        .unwrap_err();
        assert!(err.contains("category") && err.contains("create"));
    }

    #[tokio::test]
    async fn create_rejects_duplicate_project_skill_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let cache = Arc::new(StdMutex::new(SkillCacheMap::new()));
        let raw = r#"---
name: dup
description: First
---
A
"#;
        execute_skill_manage(
            root,
            &serde_json::json!({
                "action": "create",
                "name": "dup",
                "content": raw,
            })
            .to_string(),
            &cache,
        )
        .await
        .expect("first");
        let raw2 = r#"---
name: dup
description: Second
---
B
"#;
        let err = execute_skill_manage(
            root,
            &serde_json::json!({
                "action": "create",
                "name": "dup-other",
                "category": "x",
                "content": raw2,
            })
            .to_string(),
            &cache,
        )
        .await
        .unwrap_err();
        assert!(err.contains("already exists"), "{}", err);
    }
}
