//! Message routing — keyword detection and skill routing

pub mod keyword_detector;

pub use keyword_detector::{detect_skill_route, parse_direct_skill_command, SkillRoute};

/// Load the SKILL.md body for the given skill name by searching:
///   1. `<project>/.omiga/skills/<name>/SKILL.md`
///   2. `~/.omiga/skills/<name>/SKILL.md`
///
/// Returns the body text (frontmatter stripped) with `$ARGUMENTS` substituted,
/// or `None` if the skill file cannot be found or read.
pub async fn load_skill_body(
    skill_name: &str,
    args: &str,
    project_root: &std::path::Path,
) -> Option<String> {
    let cfg = crate::domain::integrations_config::load_integrations_config(project_root);
    let all_skills = crate::domain::skills::load_skills_for_project(project_root).await;
    let skills = crate::domain::integrations_config::filter_skill_entries(all_skills.clone(), &cfg);
    let normalized = crate::domain::skills::normalize_skill_name(skill_name);
    let exists_unfiltered =
        crate::domain::skills::resolve_skill_entry(&all_skills, &normalized).is_some();
    let exists_enabled = crate::domain::skills::resolve_skill_entry(&skills, &normalized).is_some();

    if exists_enabled {
        return match crate::domain::skills::invoke_skill_detailed_with_cache(
            project_root,
            skill_name,
            args,
            Some(&skills),
        )
        .await
        {
            Ok(out) => Some(out.formatted_tool_result),
            Err(error) => {
                tracing::warn!(
                    skill = %normalized,
                    error = %error,
                    "Direct skill route rejected by SkillTool validation"
                );
                None
            }
        };
    }

    if exists_unfiltered {
        return None;
    }

    let candidates: Vec<std::path::PathBuf> = {
        let mut v = vec![project_root
            .join(".omiga")
            .join("skills")
            .join(skill_name)
            .join("SKILL.md")];
        if let Some(home) = dirs::home_dir() {
            v.push(
                home.join(".omiga")
                    .join("skills")
                    .join(skill_name)
                    .join("SKILL.md"),
            );
        }
        v
    };

    for path in &candidates {
        let Ok(raw) = tokio::fs::read_to_string(path).await else {
            continue;
        };

        // Strip YAML frontmatter (--- ... ---) to get the body
        let body = if let Some(stripped) = raw.strip_prefix("---") {
            if let Some(end) = stripped.find("\n---") {
                stripped[end + 4..].trim_start().to_string()
            } else {
                raw.clone()
            }
        } else {
            raw.clone()
        };

        // Substitute $ARGUMENTS placeholder
        let body = body.replace("$ARGUMENTS", args);

        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let dir_str = dir.to_string_lossy();
        let body = body
            .replace("${CLAUDE_SKILL_DIR}", &dir_str)
            .replace("${OMIGA_SKILL_DIR}", &dir_str);

        return Some(body);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::load_skill_body;

    #[tokio::test]
    async fn direct_skill_route_does_not_fallback_when_skill_tool_validation_rejects() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join(".omiga").join("skills").join("restricted");
        tokio::fs::create_dir_all(&skill_dir).await.expect("mkdir");
        tokio::fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: restricted
description: Cannot be injected through direct routing
disable-model-invocation: true
---
Sensitive $ARGUMENTS
"#,
        )
        .await
        .expect("write skill");

        let body = load_skill_body("restricted", "payload", dir.path()).await;
        assert!(
            body.is_none(),
            "direct $skill routes must preserve SkillTool validation errors"
        );
    }
}
