//! Dossier — Continuously-updated structured brief for high-frequency topics/projects.
//!
//! Instead of accumulating many scattered long-term entries for the same topic,
//! a dossier consolidates the evolving state into a single, structured document
//! with `current_beliefs`, `decisions`, `open_questions`, and `next_steps`.
//!
//! Stored as `long_term/dossier_{slug}.json` alongside regular entries.

use super::long_term::{slugify_pub, LongTermMemoryEntry, LongTermMemoryKind, RetentionClass};
use crate::domain::pageindex::derive_query_terms;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

const MAX_BELIEFS: usize = 8;
const MAX_DECISIONS: usize = 10;
const MAX_QUESTIONS: usize = 6;
const MAX_NEXT_STEPS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Dossier {
    pub title: String,
    pub brief: String,
    pub current_beliefs: Vec<String>,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub next_steps: Vec<String>,
    pub updated_at: String,
}

impl Dossier {
    /// Merge incoming session data into this dossier (deduplicates by normalized text).
    pub fn merge(
        &mut self,
        decisions: &[String],
        beliefs: &[String],
        open_questions: &[String],
        next_steps: &[String],
    ) {
        for item in decisions {
            push_unique(&mut self.decisions, item, MAX_DECISIONS);
        }
        for item in beliefs {
            push_unique(&mut self.current_beliefs, item, MAX_BELIEFS);
        }
        for item in open_questions {
            push_unique(&mut self.open_questions, item, MAX_QUESTIONS);
        }
        // next_steps are replaced, not merged — they reflect the latest plan.
        self.next_steps.clear();
        for item in next_steps.iter().take(MAX_NEXT_STEPS) {
            self.next_steps.push(item.clone());
        }
        self.updated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Render as a compact Hot-memory section injected into every turn's system prompt.
    pub fn render_for_hot_memory(&self) -> String {
        let mut out = String::from("## Project Brief\n\n");
        out.push_str(&format!("**{}**: {}\n\n", self.title, self.brief));
        if !self.current_beliefs.is_empty() {
            out.push_str("**Current beliefs:**\n");
            for b in self.current_beliefs.iter().take(5) {
                out.push_str(&format!("- {}\n", b));
            }
            out.push('\n');
        }
        if !self.decisions.is_empty() {
            out.push_str("**Decisions:**\n");
            for d in self.decisions.iter().take(5) {
                out.push_str(&format!("- {}\n", d));
            }
            out.push('\n');
        }
        if !self.open_questions.is_empty() {
            out.push_str("**Open questions:**\n");
            for q in self.open_questions.iter().take(3) {
                out.push_str(&format!("- {}\n", q));
            }
            out.push('\n');
        }
        if !self.next_steps.is_empty() {
            out.push_str("**Next steps:**\n");
            for s in self.next_steps.iter().take(3) {
                out.push_str(&format!("- {}\n", s));
            }
        }
        out.trim().to_string()
    }

    /// Render as a `LongTermMemoryEntry` for unified retrieval.
    pub fn as_long_term_entry(&self) -> LongTermMemoryEntry {
        let summary = format!(
            "{} | Beliefs: {} | Decisions: {} | Questions: {}",
            self.brief,
            self.current_beliefs.join("; "),
            self.decisions.join("; "),
            self.open_questions.join("; "),
        );
        LongTermMemoryEntry {
            topic: self.title.clone(),
            summary: truncate_chars(&summary, 500),
            kind: LongTermMemoryKind::ProjectExperience,
            entities: derive_query_terms(&self.title).into_iter().take(5).collect(),
            source_sessions: vec![],
            confidence: 0.85,
            stability: 0.80,
            importance: 0.80,
            reuse_probability: 0.85,
            retention_class: RetentionClass::Permanent,
            last_reused_at: Some(self.updated_at.clone()),
            ..Default::default()
        }
    }
}

/// Load an existing dossier from disk, or return a new blank one.
pub async fn load_dossier(root: &Path, slug: &str) -> Dossier {
    let path = root.join(format!("dossier_{}.json", slug));
    if !path.exists() {
        return Dossier::default();
    }
    match fs::read_to_string(&path).await {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => Dossier::default(),
    }
}

/// Persist a dossier to disk.
pub async fn save_dossier(root: &Path, slug: &str, dossier: &Dossier) -> Result<(), std::io::Error> {
    fs::create_dir_all(root).await?;
    let path = root.join(format!("dossier_{}.json", slug));
    let json = serde_json::to_string_pretty(dossier)
        .map_err(|e| std::io::Error::other(format!("serialize dossier: {e}")))?;
    fs::write(path, json).await
}

/// Update or create a project dossier based on the latest working memory state.
///
/// Called after `create_session_summary` when the topic already has ≥ 2 long-term entries.
pub async fn update_project_dossier(
    lt_root: &Path,
    topic: &str,
    decisions: Vec<String>,
    beliefs: Vec<String>,
    open_questions: Vec<String>,
    next_steps: Vec<String>,
) {
    let slug = slugify_pub(topic);
    let mut dossier = load_dossier(lt_root, &slug).await;
    if dossier.title.is_empty() {
        dossier.title = topic.to_string();
        dossier.brief = truncate_chars(topic, 200);
        dossier.updated_at = chrono::Utc::now().to_rfc3339();
    }
    dossier.merge(&decisions, &beliefs, &open_questions, &next_steps);
    if let Err(e) = save_dossier(lt_root, &slug, &dossier).await {
        tracing::warn!("update_project_dossier: failed to save dossier for '{}': {}", topic, e);
    }
}

fn push_unique(list: &mut Vec<String>, item: &str, max: usize) {
    let normalized = normalize(item);
    if normalized.is_empty() {
        return;
    }
    if list.iter().any(|existing| normalize(existing) == normalized) {
        return;
    }
    if list.len() >= max {
        list.remove(0); // evict oldest
    }
    list.push(item.trim().to_string());
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_deduplicates_and_caps_decisions() {
        let mut d = Dossier::default();
        d.title = "test topic".to_string();
        let items: Vec<String> = (0..12).map(|i| format!("decision {}", i)).collect();
        d.merge(&items, &[], &[], &[]);
        assert!(d.decisions.len() <= MAX_DECISIONS);
    }

    #[test]
    fn merge_replaces_next_steps() {
        let mut d = Dossier::default();
        d.next_steps = vec!["old step".to_string()];
        d.merge(&[], &[], &[], &["new step A".to_string(), "new step B".to_string()]);
        assert_eq!(d.next_steps, vec!["new step A", "new step B"]);
    }

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let d = Dossier {
            title: "My Project".to_string(),
            brief: "A test project".to_string(),
            current_beliefs: vec!["belief one".to_string()],
            decisions: vec!["use Rust".to_string()],
            open_questions: vec!["what about perf?".to_string()],
            next_steps: vec!["write tests".to_string()],
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        save_dossier(temp.path(), "my-project", &d).await.unwrap();
        let loaded = load_dossier(temp.path(), "my-project").await;
        assert_eq!(loaded.title, "My Project");
        assert_eq!(loaded.decisions, d.decisions);
    }

    #[test]
    fn render_for_hot_memory_contains_required_sections() {
        let d = Dossier {
            title: "Omiga".to_string(),
            brief: "AI coding workbench".to_string(),
            current_beliefs: vec!["Rust backend is the right call".to_string()],
            decisions: vec!["Use Tauri for desktop".to_string()],
            open_questions: vec!["How to scale memory?".to_string()],
            next_steps: vec!["Write tests".to_string()],
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let rendered = d.render_for_hot_memory();
        assert!(rendered.contains("## Project Brief"), "must have ## Project Brief header");
        assert!(rendered.contains("Omiga"), "must include project title");
        assert!(rendered.contains("AI coding workbench"), "must include brief");
        assert!(rendered.contains("**Current beliefs:**"), "must have beliefs section");
        assert!(rendered.contains("Rust backend is the right call"));
        assert!(rendered.contains("**Decisions:**"), "must have decisions section");
        assert!(rendered.contains("Use Tauri for desktop"));
        assert!(rendered.contains("**Open questions:**"), "must have questions section");
        assert!(rendered.contains("**Next steps:**"), "must have next steps section");
    }

    #[test]
    fn render_for_hot_memory_empty_dossier_is_minimal() {
        let d = Dossier {
            title: "Empty".to_string(),
            brief: "minimal".to_string(),
            ..Default::default()
        };
        let rendered = d.render_for_hot_memory();
        assert!(rendered.contains("## Project Brief"));
        // No sections for empty lists.
        assert!(!rendered.contains("**Current beliefs:**"));
        assert!(!rendered.contains("**Decisions:**"));
        assert!(!rendered.contains("**Open questions:**"));
        assert!(!rendered.contains("**Next steps:**"));
    }

    #[test]
    fn render_for_hot_memory_caps_at_5_beliefs_and_decisions() {
        let d = Dossier {
            title: "Capped".to_string(),
            brief: "brief".to_string(),
            current_beliefs: (0..10).map(|i| format!("belief {}", i)).collect(),
            decisions: (0..10).map(|i| format!("decision {}", i)).collect(),
            ..Default::default()
        };
        let rendered = d.render_for_hot_memory();
        // take(5) is applied — only first 5 should appear.
        assert!(rendered.contains("belief 4"), "belief 4 should appear (5th item)");
        assert!(!rendered.contains("belief 5"), "belief 5 should be truncated");
        assert!(rendered.contains("decision 4"));
        assert!(!rendered.contains("decision 5"));
    }

    #[test]
    fn merge_deduplicates_beliefs_and_questions() {
        let mut d = Dossier {
            title: "dup-test".to_string(),
            current_beliefs: vec!["belief A".to_string()],
            open_questions: vec!["question A".to_string()],
            ..Default::default()
        };
        // Merging same items should not duplicate.
        d.merge(
            &[],
            &["belief A".to_string(), "belief B".to_string()],
            &["question A".to_string(), "question B".to_string()],
            &[],
        );
        let belief_a_count = d.current_beliefs.iter().filter(|b| b.as_str() == "belief A").count();
        assert_eq!(belief_a_count, 1, "belief A must not be duplicated");
        assert!(d.current_beliefs.contains(&"belief B".to_string()));
        let q_a_count = d.open_questions.iter().filter(|q| q.as_str() == "question A").count();
        assert_eq!(q_a_count, 1, "question A must not be duplicated");
    }
}
