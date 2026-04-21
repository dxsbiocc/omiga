//! Runtime overlay for agent prompts.
//!
//! OMX-style orchestration works best when each turn receives a compact view of
//! the current project runtime: active long-running modes, recent context
//! snapshots, and a lightweight codebase map. Omiga already persists several
//! pieces of this data on disk; this module composes them into a bounded prompt
//! section that can be injected into main-agent and sub-agent system prompts.

use std::path::Path;

const MAX_OVERLAY_CHARS: usize = 3_500;
const MAX_ACTIVE_RALPH: usize = 2;
const MAX_ACTIVE_AUTOPILOT: usize = 2;
const MAX_ACTIVE_TEAM: usize = 2;
const MAX_SNAPSHOTS: usize = 3;
const MAX_TOP_LEVEL_ENTRIES: usize = 8;
const MAX_NOTEPAD_CHARS: usize = 500;
const MAX_PROJECT_MEMORY_CHARS: usize = 700;

fn truncate_overlay(mut text: String) -> String {
    if text.len() <= MAX_OVERLAY_CHARS {
        return text;
    }

    let notice = "\n\n[Omiga runtime overlay truncated]";
    let max_body = MAX_OVERLAY_CHARS.saturating_sub(notice.len());
    let mut end = max_body.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(notice);
    text
}

fn summarize_top_level_entries(project_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(project_root) else {
        return vec![];
    };

    let mut out: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_hidden = name.starts_with('.');
            let is_noisy = matches!(
                name.as_str(),
                "node_modules" | "target" | "dist" | "build" | "__pycache__"
            );
            if is_hidden || is_noisy {
                return None;
            }
            let kind = entry.file_type().ok()?;
            Some(if kind.is_dir() {
                format!("{name}/")
            } else {
                name
            })
        })
        .collect();

    out.sort();
    out.truncate(MAX_TOP_LEVEL_ENTRIES);
    out
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

fn summarize_notepad(project_root: &Path) -> Option<String> {
    let candidates = [
        project_root.join(".omiga").join("notepad.md"),
        project_root.join(".omiga").join("notes.md"),
    ];
    for path in candidates {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let single_line = trimmed
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(6)
            .collect::<Vec<_>>()
            .join(" | ");
        return Some(truncate_chars(&single_line, MAX_NOTEPAD_CHARS));
    }
    None
}

fn summarize_project_memory_json(project_root: &Path) -> Option<String> {
    let candidates = [
        project_root.join(".omiga").join("project-memory.json"),
        project_root.join(".omiga").join("project_memory.json"),
    ];
    for path in candidates {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let mut parts: Vec<String> = Vec::new();
        for key in [
            "techStack",
            "conventions",
            "structure",
            "notes",
            "directives",
        ] {
            if let Some(value) = json.get(key) {
                let rendered = match value {
                    serde_json::Value::String(s) => s.trim().to_string(),
                    serde_json::Value::Array(arr) => arr
                        .iter()
                        .take(3)
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                        .join("; "),
                    serde_json::Value::Object(map) => {
                        map.keys().take(4).cloned().collect::<Vec<_>>().join(", ")
                    }
                    _ => String::new(),
                };
                if !rendered.is_empty() {
                    parts.push(format!("{key}: {rendered}"));
                }
            }
        }
        if !parts.is_empty() {
            return Some(truncate_chars(&parts.join(" | "), MAX_PROJECT_MEMORY_CHARS));
        }
    }
    None
}

fn summarize_registry_backed_memory(project_root: &Path) -> Option<String> {
    let registry_path = crate::domain::memory::registry::registry_file_path();
    let Ok(raw) = std::fs::read_to_string(registry_path) else {
        return None;
    };
    let Ok(registry) =
        serde_json::from_str::<crate::domain::memory::registry::MemoryRegistry>(&raw)
    else {
        return None;
    };
    let canonical = std::fs::canonicalize(project_root)
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string();
    let entry = registry.projects.get(&canonical)?;
    let wiki_dir = std::path::PathBuf::from(&entry.wiki_path);
    let wiki_pages = std::fs::read_dir(&wiki_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.ends_with(".md"))
                .take(4)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut parts = vec![format!("wiki_path: {}", entry.wiki_path)];
    if !entry.implicit_path.is_empty() {
        parts.push(format!("implicit_path: {}", entry.implicit_path));
    }
    if !wiki_pages.is_empty() {
        parts.push(format!("wiki_pages: {}", wiki_pages.join(", ")));
    }
    Some(truncate_chars(&parts.join(" | "), MAX_PROJECT_MEMORY_CHARS))
}

fn summarize_project_memory(project_root: &Path) -> Option<String> {
    summarize_project_memory_json(project_root)
        .or_else(|| summarize_registry_backed_memory(project_root))
}

fn count_project_skill_dirs(project_root: &Path) -> usize {
    let root = project_root.join(".omiga").join("skills");
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };

    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let ty = entry.file_type().ok()?;
            if !ty.is_dir() {
                return None;
            }
            let path = entry.path();
            if path.join("SKILL.md").is_file() {
                return Some(1usize);
            }
            let nested = std::fs::read_dir(path).ok()?;
            let count = nested
                .filter_map(|child| child.ok())
                .filter_map(|child| {
                    let ty = child.file_type().ok()?;
                    if ty.is_dir() && child.path().join("SKILL.md").is_file() {
                        Some(1usize)
                    } else {
                        None
                    }
                })
                .sum::<usize>();
            Some(count)
        })
        .sum()
}

fn count_project_agent_overrides(project_root: &Path) -> usize {
    let root = project_root.join(".omiga").join("agents");
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };

    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let ty = entry.file_type().ok()?;
            if ty.is_file() && name.ends_with(".md") {
                Some(1usize)
            } else {
                None
            }
        })
        .sum()
}

/// Build a compact OMX-style runtime overlay section for the current project.
///
/// The overlay intentionally focuses on project-local runtime data that is not
/// already covered by the main memory/navigation sections:
/// - active Ralph / Team sessions
/// - recent context snapshots
/// - lightweight top-level codebase map
/// - project-local skill / agent override counts
pub async fn build_runtime_overlay(project_root: &Path) -> Option<String> {
    let (ralph_states, autopilot_states, team_states, snapshots) = tokio::join!(
        crate::domain::ralph_state::list_states(project_root),
        crate::domain::autopilot_state::list_states(project_root),
        crate::domain::team_state::list_states(project_root),
        crate::domain::context_snapshot::list_snapshots(project_root),
    );

    let active_ralph: Vec<_> = ralph_states
        .into_iter()
        .filter(|s| !matches!(s.phase, crate::domain::ralph_state::RalphPhase::Complete))
        .take(MAX_ACTIVE_RALPH)
        .collect();
    let active_team: Vec<_> = team_states
        .into_iter()
        .filter(|s| !matches!(s.phase, crate::domain::team_state::TeamPhase::Complete))
        .take(MAX_ACTIVE_TEAM)
        .collect();
    let active_autopilot: Vec<_> = autopilot_states
        .into_iter()
        .filter(|s| {
            !matches!(
                s.phase,
                crate::domain::autopilot_state::AutopilotPhase::Complete
            )
        })
        .take(MAX_ACTIVE_AUTOPILOT)
        .collect();
    let recent_snapshots: Vec<_> = snapshots.into_iter().take(MAX_SNAPSHOTS).collect();

    let top_level = summarize_top_level_entries(project_root);
    let project_skill_count = count_project_skill_dirs(project_root);
    let project_agent_override_count = count_project_agent_overrides(project_root);
    let notepad_summary = summarize_notepad(project_root);
    let project_memory_summary = summarize_project_memory(project_root);

    let mut lines = vec!["## Omiga Runtime Overlay".to_string()];
    lines.push(format!("- Project root: `{}`", project_root.display()));

    if !top_level.is_empty() {
        lines.push(format!(
            "- Top-level workspace map: {}",
            top_level.join(", ")
        ));
    }

    if project_skill_count > 0 || project_agent_override_count > 0 {
        lines.push(format!(
            "- Project-local orchestration assets: {} skill(s), {} custom agent override(s)",
            project_skill_count, project_agent_override_count
        ));
    }
    if let Some(notepad) = &notepad_summary {
        lines.push(format!("- Notepad summary: {}", notepad));
    }
    if let Some(memory) = &project_memory_summary {
        lines.push(format!("- Project memory summary: {}", memory));
    }

    if !active_ralph.is_empty() {
        lines.push(String::new());
        lines.push("### Active Ralph sessions".to_string());
        for state in &active_ralph {
            lines.push(format!(
                "- `{}` · phase=`{}` · iteration={} · pending_todos={} · goal={}",
                state.session_id,
                state.phase,
                state.iteration,
                state.todos_pending.len(),
                state.goal,
            ));
        }
    }

    if !active_team.is_empty() {
        lines.push(String::new());
        lines.push("### Active Team sessions".to_string());
        for state in &active_team {
            lines.push(format!(
                "- `{}` · phase=`{}` · subtasks={} · running={} · failed={} · goal={}",
                state.session_id,
                state.phase,
                state.subtasks.len(),
                state.running_count(),
                state.failed_count(),
                state.goal,
            ));
        }
    }

    if !active_autopilot.is_empty() {
        lines.push(String::new());
        lines.push("### Active Autopilot sessions".to_string());
        for state in &active_autopilot {
            lines.push(format!(
                "- `{}` · phase=`{}` · qa_cycles={}/{} · pending_todos={} · goal={}",
                state.session_id,
                state.phase,
                state.qa_cycles,
                state.max_qa_cycles,
                state.todos_pending.len(),
                state.goal,
            ));
        }
    }

    if !recent_snapshots.is_empty() {
        lines.push(String::new());
        lines.push("### Recent context snapshots".to_string());
        for snap in &recent_snapshots {
            lines.push(format!(
                "- `{}` · modified={} · path=`{}`",
                snap.name, snap.modified_at, snap.path
            ));
        }
    }

    if lines.len() == 1 {
        return None;
    }

    Some(truncate_overlay(lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn build_overlay_contains_active_modes_and_snapshots() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".omiga").join("context")).unwrap();
        std::fs::create_dir_all(dir.path().join(".omiga").join("skills").join("plan")).unwrap();
        std::fs::create_dir_all(dir.path().join(".omiga").join("memory")).unwrap();
        std::fs::write(
            dir.path()
                .join(".omiga")
                .join("skills")
                .join("plan")
                .join("SKILL.md"),
            "---\nname: plan\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".omiga").join("notepad.md"),
            "priority: preserve runtime state\nnext: validate reviewer outputs\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".omiga").join("project-memory.json"),
            serde_json::json!({
                "techStack": ["rust", "tauri", "react"],
                "conventions": ["small diffs", "verify before complete"],
                "notes": "keep orchestration evidence concise"
            })
            .to_string(),
        )
        .unwrap();

        let ralph = crate::domain::ralph_state::RalphState {
            version: 1,
            session_id: "ralph-test".to_string(),
            goal: "Investigate prompt runtime".to_string(),
            phase: crate::domain::ralph_state::RalphPhase::Executing,
            iteration: 2,
            consecutive_errors: 0,
            project_root: dir.path().display().to_string(),
            env: None,
            todos_completed: vec!["map files".to_string()],
            todos_pending: vec!["write overlay".to_string()],
            last_error: None,
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::ralph_state::write_state(dir.path(), &ralph)
            .await
            .unwrap();

        let team = crate::domain::team_state::TeamState {
            version: 1,
            session_id: "team-test".to_string(),
            goal: "Parallelize checks".to_string(),
            phase: crate::domain::team_state::TeamPhase::Executing,
            project_root: dir.path().display().to_string(),
            subtasks: vec![crate::domain::team_state::TeamSubtaskState {
                id: "t1".to_string(),
                description: "Run validation".to_string(),
                agent_type: "executor".to_string(),
                status: "running".to_string(),
                attempt: 0,
                max_retries: 2,
                error: None,
                bg_task_id: None,
            }],
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::team_state::write_state(dir.path(), &team)
            .await
            .unwrap();

        let autopilot = crate::domain::autopilot_state::AutopilotState {
            version: 1,
            session_id: "auto-test".to_string(),
            goal: "Build end-to-end feature".to_string(),
            phase: crate::domain::autopilot_state::AutopilotPhase::Implementation,
            project_root: dir.path().display().to_string(),
            qa_cycles: 1,
            max_qa_cycles: 5,
            env: None,
            todos_completed: vec!["spec".to_string()],
            todos_pending: vec!["qa".to_string()],
            last_error: None,
            started_at: Utc::now(),
            updated_at: Utc::now(),
        };
        crate::domain::autopilot_state::write_state(dir.path(), &autopilot)
            .await
            .unwrap();

        std::fs::write(
            dir.path()
                .join(".omiga")
                .join("context")
                .join("snapshot-a.md"),
            "# snapshot",
        )
        .unwrap();

        let overlay = build_runtime_overlay(dir.path()).await.unwrap();
        assert!(overlay.contains("Omiga Runtime Overlay"));
        assert!(overlay.contains("Active Ralph sessions"));
        assert!(overlay.contains("ralph-test"));
        assert!(overlay.contains("Active Team sessions"));
        assert!(overlay.contains("team-test"));
        assert!(overlay.contains("Active Autopilot sessions"));
        assert!(overlay.contains("auto-test"));
        assert!(overlay.contains("Recent context snapshots"));
        assert!(overlay.contains("snapshot-a"));
        assert!(overlay.contains("Top-level workspace map"));
        assert!(overlay.contains("Project-local orchestration assets"));
        assert!(overlay.contains("Notepad summary"));
        assert!(overlay.contains("Project memory summary"));
    }

    #[test]
    fn truncate_overlay_respects_limit() {
        let long = "a".repeat(MAX_OVERLAY_CHARS + 128);
        let truncated = truncate_overlay(long);
        assert!(truncated.len() <= MAX_OVERLAY_CHARS + 64);
        assert!(truncated.contains("truncated"));
    }
}
