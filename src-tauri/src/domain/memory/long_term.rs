use super::{config::permanent_long_term_path, working_memory::WorkingMemoryState, MemoryConfig};
use crate::domain::pageindex::{derive_query_terms, score_terms_against_text};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryKind {
    TaskConclusion,
    ProjectExperience,
    ResearchInsight,
    MethodLesson,
}

impl std::fmt::Display for LongTermMemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TaskConclusion => write!(f, "task_conclusion"),
            Self::ProjectExperience => write!(f, "project_experience"),
            Self::ResearchInsight => write!(f, "research_insight"),
            Self::MethodLesson => write!(f, "method_lesson"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LongTermMemoryEntry {
    pub topic: String,
    pub summary: String,
    pub kind: LongTermMemoryKind,
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub source_sessions: Vec<String>,
    #[serde(default)]
    pub source_artifacts: Vec<String>,
    pub confidence: f32,
    pub stability: f32,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reused_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermStatus {
    pub project_entry_count: usize,
    pub global_entry_count: usize,
}

#[derive(Debug, Clone)]
pub struct LongTermMatch {
    pub title: String,
    pub path: String,
    pub excerpt: String,
    pub score: f64,
    pub global: bool,
}

pub fn long_term_path(config: &MemoryConfig, project_root: &Path) -> PathBuf {
    config.long_term_path(project_root)
}

pub async fn ensure_dirs(config: &MemoryConfig, project_root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(long_term_path(config, project_root)).await?;
    fs::create_dir_all(permanent_long_term_path()).await?;
    Ok(())
}

pub async fn count_entries(root: &Path) -> usize {
    list_entries(root).await.len()
}

pub async fn list_entries(root: &Path) -> Vec<(PathBuf, LongTermMemoryEntry)> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    let Ok(mut entries) = fs::read_dir(root).await else {
        return out;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path).await else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<LongTermMemoryEntry>(&raw) else {
            continue;
        };
        out.push((path, parsed));
    }
    out
}

pub async fn promote_from_working_memory(
    config: &MemoryConfig,
    project_root: &Path,
    session_id: &str,
    state: &WorkingMemoryState,
) -> usize {
    let root = long_term_path(config, project_root);
    let candidates = build_promotion_candidates(session_id, state);
    let count = candidates.len();
    for candidate in candidates {
        let _ = upsert_entry(&root, candidate).await;
    }
    count
}

pub fn promotion_candidate_count(session_id: &str, state: &WorkingMemoryState) -> usize {
    build_promotion_candidates(session_id, state).len()
}

pub async fn search_entries(
    root: &Path,
    query: &str,
    limit: usize,
    global: bool,
) -> Vec<LongTermMatch> {
    let query_terms = derive_query_terms(query);
    if query_terms.is_empty() || !root.is_dir() {
        return vec![];
    }
    let mut matches = Vec::new();
    for (path, entry) in list_entries(root).await {
        let search_text = format!(
            "{}\n{}\n{}",
            entry.topic,
            entry.summary,
            entry.entities.join(" ")
        );
        let score = score_terms_against_text(&search_text, &query_terms);
        if score <= 0.0 {
            continue;
        }
        matches.push(LongTermMatch {
            title: entry.topic.clone(),
            path: path.to_string_lossy().to_string(),
            excerpt: entry.summary.clone(),
            score,
            global,
        });
    }
    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    matches.truncate(limit);
    matches
}

async fn upsert_entry(root: &Path, entry: LongTermMemoryEntry) -> Result<(), std::io::Error> {
    fs::create_dir_all(root).await?;
    let path = root.join(format!("{}.json", slugify(&entry.topic)));
    let existing = if path.exists() {
        fs::read_to_string(&path)
            .await
            .ok()
            .and_then(|raw| serde_json::from_str::<LongTermMemoryEntry>(&raw).ok())
    } else {
        None
    };

    let merged = merge_entry(existing, entry);
    let json = serde_json::to_string_pretty(&merged)
        .map_err(|e| std::io::Error::other(format!("serialize long-term entry: {e}")))?;
    fs::write(path, json).await
}

fn merge_entry(
    existing: Option<LongTermMemoryEntry>,
    incoming: LongTermMemoryEntry,
) -> LongTermMemoryEntry {
    let Some(mut existing) = existing else {
        return incoming;
    };
    if existing.summary.chars().count() < incoming.summary.chars().count() {
        existing.summary = incoming.summary;
    }
    existing.confidence = existing.confidence.max(incoming.confidence);
    existing.stability = existing.stability.max(incoming.stability);
    existing.last_reused_at = incoming.last_reused_at;

    for entity in incoming.entities {
        if !existing.entities.contains(&entity) {
            existing.entities.push(entity);
        }
    }
    for session in incoming.source_sessions {
        if !existing.source_sessions.contains(&session) {
            existing.source_sessions.push(session);
        }
    }
    for artifact in incoming.source_artifacts {
        if !existing.source_artifacts.contains(&artifact) {
            existing.source_artifacts.push(artifact);
        }
    }
    existing
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

fn build_promotion_candidates(
    session_id: &str,
    state: &WorkingMemoryState,
) -> Vec<LongTermMemoryEntry> {
    let mut candidates = Vec::new();

    for item in state.decisions.iter().chain(state.working_facts.iter()) {
        let kind = infer_long_term_kind(item.kind.as_deref());
        if !should_promote_item(item, &kind) {
            continue;
        }
        candidates.push(LongTermMemoryEntry {
            topic: truncate_chars(&item.text, 120),
            summary: truncate_chars(&item.text, 280),
            kind,
            entities: derive_query_terms(&item.text).into_iter().take(5).collect(),
            source_sessions: vec![session_id.to_string()],
            source_artifacts: item.source_message_ids.clone(),
            confidence: item.confidence.clamp(0.0, 1.0),
            stability: ((item.touch_count as f32) / 4.0).clamp(0.45, 1.0),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_reused_at: Some(chrono::Utc::now().to_rfc3339()),
        });
    }

    candidates
}

fn should_promote_item(
    item: &crate::domain::memory::working_memory::WorkingMemoryItem,
    kind: &LongTermMemoryKind,
) -> bool {
    if item.status != crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active {
        return false;
    }
    if item.confidence < 0.82 {
        return false;
    }
    if item.source_message_ids.len() < 2 {
        return false;
    }
    if item.text.chars().count() < 24 {
        return false;
    }
    if looks_transient_for_long_term(&item.text) {
        return false;
    }

    let min_touch_count = match kind {
        LongTermMemoryKind::ResearchInsight
        | LongTermMemoryKind::MethodLesson
        | LongTermMemoryKind::ProjectExperience => 2,
        LongTermMemoryKind::TaskConclusion => 3,
    };
    item.touch_count >= min_touch_count
}

fn infer_long_term_kind(kind: Option<&str>) -> LongTermMemoryKind {
    match kind {
        Some("research_insight") => LongTermMemoryKind::ResearchInsight,
        Some("method_lesson") => LongTermMemoryKind::MethodLesson,
        Some("project_experience") => LongTermMemoryKind::ProjectExperience,
        _ => LongTermMemoryKind::TaskConclusion,
    }
}

fn looks_transient_for_long_term(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "这周",
        "今天",
        "当前",
        "目前",
        "暂时",
        "稍后",
        "this week",
        "today",
        "current",
        "temporary",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_entries_matches_topic_and_summary() {
        let temp = tempfile::tempdir().unwrap();
        let entry = LongTermMemoryEntry {
            topic: "recall ordering".to_string(),
            summary: "Recall should search working memory before wiki.".to_string(),
            kind: LongTermMemoryKind::TaskConclusion,
            entities: vec!["recall".to_string(), "wiki".to_string()],
            source_sessions: vec!["s1".to_string()],
            source_artifacts: vec![],
            confidence: 0.9,
            stability: 0.8,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_reused_at: None,
        };

        upsert_entry(temp.path(), entry).await.unwrap();
        let matches = search_entries(temp.path(), "recall wiki order", 5, false).await;

        assert_eq!(matches.len(), 1);
        assert!(matches[0].excerpt.contains("working memory"));
    }

    #[test]
    fn promotion_candidates_require_stronger_signal() {
        let weak_state = WorkingMemoryState {
            decisions: vec![crate::domain::memory::working_memory::WorkingMemoryItem {
                text: "这个项目暂时用 qwen 做 baseline".to_string(),
                source_message_ids: vec!["m1".to_string()],
                confidence: 0.9,
                last_touched_turn: 4,
                expires_after_turns: 24,
                status: crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active,
                kind: Some("project_experience".to_string()),
                touch_count: 2,
            }],
            ..WorkingMemoryState::default()
        };
        assert!(build_promotion_candidates("s1", &weak_state).is_empty());

        let strong_state = WorkingMemoryState {
            decisions: vec![crate::domain::memory::working_memory::WorkingMemoryItem {
                text: "Recall 结果应优先展示 working memory 和 long-term 再展示 wiki".to_string(),
                source_message_ids: vec!["m1".to_string(), "m2".to_string()],
                confidence: 0.9,
                last_touched_turn: 6,
                expires_after_turns: 24,
                status: crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active,
                kind: Some("project_experience".to_string()),
                touch_count: 3,
            }],
            ..WorkingMemoryState::default()
        };
        assert_eq!(build_promotion_candidates("s1", &strong_state).len(), 1);
    }
}
