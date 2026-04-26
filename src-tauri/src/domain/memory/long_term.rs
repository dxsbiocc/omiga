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
    /// End-of-session snapshot: goal + key decisions, lower promotion threshold.
    SessionSummary,
}

impl std::fmt::Display for LongTermMemoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TaskConclusion => write!(f, "task_conclusion"),
            Self::ProjectExperience => write!(f, "project_experience"),
            Self::ResearchInsight => write!(f, "research_insight"),
            Self::MethodLesson => write!(f, "method_lesson"),
            Self::SessionSummary => write!(f, "session_summary"),
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
        let base_score = score_terms_against_text(&search_text, &query_terms);
        if base_score <= 0.0 {
            continue;
        }
        // Blend TF-IDF with memory quality: confidence (0-1) and stability (0-1)
        let quality = (entry.confidence as f64 * 0.6 + entry.stability as f64 * 0.4).clamp(0.3, 1.0);
        // Boost recently reused memories
        let recency = recency_bonus(entry.last_reused_at.as_deref());
        let score = base_score * quality + recency;
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

/// Returns a small additive score bonus for recently reused memories.
/// +0.15 if reused within 7 days, +0.08 if within 30 days, 0 otherwise.
fn recency_bonus(last_reused_at: Option<&str>) -> f64 {
    let Some(ts) = last_reused_at else { return 0.0 };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else { return 0.0 };
    let days = chrono::Utc::now()
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_days();
    if days < 7 { 0.15 } else if days < 30 { 0.08 } else { 0.0 }
}

/// Count entries that are stale: not reused in >90 days AND stability < 0.4.
pub async fn count_stale_entries(root: &Path) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(90);
    list_entries(root)
        .await
        .into_iter()
        .filter(|(_, entry)| {
            let old = entry
                .last_reused_at
                .as_deref()
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc) < cutoff)
                .unwrap_or(true);
            old && entry.stability < 0.4
        })
        .count()
}

/// Create and persist a session-level summary in long-term memory.
///
/// Lower threshold than `promote_from_working_memory`: requires only that the
/// session had a goal or active topic, and at least one high-confidence decision.
pub async fn create_session_summary(
    root: &Path,
    session_id: &str,
    state: &WorkingMemoryState,
) -> Option<LongTermMemoryEntry> {
    let topic = state
        .active_topic
        .as_deref()
        .or(state.session_goal.as_deref())?;
    if topic.trim().is_empty() {
        return None;
    }

    let mut summary_parts = Vec::new();
    if let Some(goal) = &state.session_goal {
        summary_parts.push(format!("Goal: {}", truncate_chars(goal, 160)));
    }
    for decision in state
        .decisions
        .iter()
        .filter(|d| {
            d.status == crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active
                && d.confidence >= 0.70
        })
        .take(3)
    {
        summary_parts.push(format!("→ {}", truncate_chars(&decision.text, 140)));
    }
    for fact in state
        .working_facts
        .iter()
        .filter(|f| {
            f.status == crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active
                && f.confidence >= 0.75
        })
        .take(2)
    {
        summary_parts.push(format!("Fact: {}", truncate_chars(&fact.text, 140)));
    }

    if summary_parts.len() < 2 {
        return None;
    }

    let entry = LongTermMemoryEntry {
        topic: truncate_chars(topic, 120),
        summary: summary_parts.join(" | "),
        kind: LongTermMemoryKind::SessionSummary,
        entities: derive_query_terms(topic).into_iter().take(5).collect(),
        source_sessions: vec![session_id.to_string()],
        source_artifacts: vec![],
        confidence: 0.70,
        stability: 0.55,
        created_at: chrono::Utc::now().to_rfc3339(),
        last_reused_at: None,
    };

    let _ = upsert_entry(root, entry.clone()).await;
    Some(entry)
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
    if item.text.chars().count() < 24 {
        return false;
    }
    if looks_transient_for_long_term(&item.text) {
        return false;
    }

    // SessionSummary has a lighter threshold — no touch_count requirement.
    if *kind == LongTermMemoryKind::SessionSummary {
        return item.confidence >= 0.65 && !item.source_message_ids.is_empty();
    }

    if item.confidence < 0.82 {
        return false;
    }
    if item.source_message_ids.len() < 2 {
        return false;
    }

    let min_touch_count = match kind {
        LongTermMemoryKind::ResearchInsight
        | LongTermMemoryKind::MethodLesson
        | LongTermMemoryKind::ProjectExperience => 2,
        LongTermMemoryKind::SessionSummary => 1,
        LongTermMemoryKind::TaskConclusion => 3,
    };
    item.touch_count >= min_touch_count
}

fn infer_long_term_kind(kind: Option<&str>) -> LongTermMemoryKind {
    match kind {
        Some("research_insight") => LongTermMemoryKind::ResearchInsight,
        Some("method_lesson") => LongTermMemoryKind::MethodLesson,
        Some("project_experience") => LongTermMemoryKind::ProjectExperience,
        Some("session_summary") => LongTermMemoryKind::SessionSummary,
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

    #[tokio::test]
    async fn search_entries_blends_quality_into_score() {
        let temp = tempfile::tempdir().unwrap();
        // High confidence/stability entry
        let high_quality = LongTermMemoryEntry {
            topic: "memory search ranking".to_string(),
            summary: "Higher confidence entries should rank above weak ones.".to_string(),
            kind: LongTermMemoryKind::ResearchInsight,
            entities: vec!["memory".to_string(), "search".to_string()],
            source_sessions: vec!["s1".to_string()],
            source_artifacts: vec![],
            confidence: 0.95,
            stability: 0.9,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_reused_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        // Low confidence/stability entry with same topic
        let low_quality = LongTermMemoryEntry {
            topic: "memory search weak".to_string(),
            summary: "Memory search result from uncertain source.".to_string(),
            kind: LongTermMemoryKind::TaskConclusion,
            entities: vec!["memory".to_string(), "search".to_string()],
            source_sessions: vec!["s2".to_string()],
            source_artifacts: vec![],
            confidence: 0.3,
            stability: 0.2,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_reused_at: None,
        };

        upsert_entry(temp.path(), high_quality).await.unwrap();
        upsert_entry(temp.path(), low_quality).await.unwrap();

        let matches = search_entries(temp.path(), "memory search", 5, false).await;
        assert_eq!(matches.len(), 2);
        // High quality + recency bonus should outrank low quality
        assert!(
            matches[0].score > matches[1].score,
            "high-quality entry should rank first, got scores {} vs {}",
            matches[0].score,
            matches[1].score
        );
    }

    #[tokio::test]
    async fn count_stale_entries_detects_old_weak_memories() {
        let temp = tempfile::tempdir().unwrap();
        // Stale: old reuse, low stability
        let stale = LongTermMemoryEntry {
            topic: "old unused insight".to_string(),
            summary: "An old fact no longer relevant.".to_string(),
            kind: LongTermMemoryKind::TaskConclusion,
            entities: vec![],
            source_sessions: vec!["s1".to_string()],
            source_artifacts: vec![],
            confidence: 0.5,
            stability: 0.2,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            last_reused_at: Some("2024-01-01T00:00:00Z".to_string()),
        };
        // Fresh: recently reused, high stability
        let fresh = LongTermMemoryEntry {
            topic: "active project convention".to_string(),
            summary: "Always run tests before pushing.".to_string(),
            kind: LongTermMemoryKind::ProjectExperience,
            entities: vec![],
            source_sessions: vec!["s2".to_string()],
            source_artifacts: vec![],
            confidence: 0.9,
            stability: 0.8,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_reused_at: Some(chrono::Utc::now().to_rfc3339()),
        };

        upsert_entry(temp.path(), stale).await.unwrap();
        upsert_entry(temp.path(), fresh).await.unwrap();

        let stale_count = count_stale_entries(temp.path()).await;
        assert_eq!(stale_count, 1, "only the old weak entry should be stale");
    }

    #[tokio::test]
    async fn create_session_summary_requires_goal_and_decisions() {
        let temp = tempfile::tempdir().unwrap();

        // Empty state: no summary
        let empty = WorkingMemoryState::default();
        let result = create_session_summary(temp.path(), "sess-empty", &empty).await;
        assert!(result.is_none(), "empty state should not produce a summary");

        // State with goal but no decisions: still no summary (needs ≥2 parts)
        let goal_only = WorkingMemoryState {
            session_goal: Some("优化记忆系统".to_string()),
            ..Default::default()
        };
        let result = create_session_summary(temp.path(), "sess-goal", &goal_only).await;
        assert!(result.is_none(), "goal alone should not produce a summary");

        // State with goal + high-confidence decision: summary created
        let rich = WorkingMemoryState {
            session_goal: Some("优化记忆系统分层检索".to_string()),
            active_topic: Some("memory recall".to_string()),
            decisions: vec![crate::domain::memory::working_memory::WorkingMemoryItem {
                text: "recall should blend TF-IDF with confidence and recency".to_string(),
                source_message_ids: vec!["m1".to_string()],
                confidence: 0.85,
                last_touched_turn: 3,
                expires_after_turns: 24,
                status: crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active,
                kind: None,
                touch_count: 2,
            }],
            ..Default::default()
        };
        let result = create_session_summary(temp.path(), "sess-rich", &rich).await;
        assert!(result.is_some(), "rich state should produce a session summary");
        let entry = result.unwrap();
        assert_eq!(entry.kind, LongTermMemoryKind::SessionSummary);
        assert!(entry.summary.contains("Goal:"));
    }
}
