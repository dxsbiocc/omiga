//! Message routing — keyword detection and skill routing

pub mod keyword_detector;

pub use keyword_detector::{detect_skill_route, SkillRoute};

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
        let body = if raw.starts_with("---") {
            if let Some(end) = raw[3..].find("\n---") {
                raw[3 + end + 4..].trim_start().to_string()
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
