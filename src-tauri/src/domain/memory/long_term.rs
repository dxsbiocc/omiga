use super::{config::permanent_long_term_path, working_memory::WorkingMemoryState, MemoryConfig};
use crate::domain::pageindex::{derive_query_terms, score_terms_against_text};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Lifecycle status of a long-term memory entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    /// Entry is live and used in retrieval (default).
    #[default]
    Active,
    /// Intentionally hidden from retrieval but kept for audit.
    Archived,
    /// Replaced by a newer, higher-quality entry on the same topic.
    Superseded,
}

/// Governs auto-deletion policy for a long-term memory entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    /// Never auto-deleted (core facts, project conventions).
    Permanent,
    /// Default — pruned when stability < 0.4 and not reused in 90 days.
    #[default]
    LongTerm,
    /// Lighter entry; survives a few sessions but pruned sooner (30 days).
    Session,
    /// Can be deleted after a short TTL.
    Ephemeral,
}

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
#[serde(default)]
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
    /// Subjective importance of this entry (0–1). Higher = ranked first during retrieval.
    #[serde(default = "default_importance")]
    pub importance: f32,
    /// Predicted probability this entry will be reused in a future similar task (0–1).
    #[serde(default = "default_reuse_probability")]
    pub reuse_probability: f32,
    /// Controls auto-deletion behaviour.
    #[serde(default)]
    pub retention_class: RetentionClass,
    /// Lifecycle status — only Active entries appear in search results.
    #[serde(default)]
    pub status: EntryStatus,
    /// ISO-8601 expiry timestamp for Ephemeral entries; None = no TTL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reused_at: Option<String>,
}

fn default_importance() -> f32 {
    0.5
}
fn default_reuse_probability() -> f32 {
    0.5
}

impl Default for LongTermMemoryEntry {
    fn default() -> Self {
        Self {
            topic: String::new(),
            summary: String::new(),
            kind: LongTermMemoryKind::TaskConclusion,
            entities: vec![],
            source_sessions: vec![],
            source_artifacts: vec![],
            confidence: 0.5,
            stability: 0.5,
            importance: 0.5,
            reuse_probability: 0.5,
            retention_class: RetentionClass::LongTerm,
            status: EntryStatus::Active,
            expires_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_reused_at: None,
        }
    }
}

/// Write Gate constants — prevent unbounded memory growth.
const MAX_ENTRIES_PER_TOPIC: usize = 5;
const GLOBAL_SOFT_CAP: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermStatus {
    pub project_entry_count: usize,
    pub global_entry_count: usize,
    /// Entries not reused in >90 days with stability < 0.4 (across project + global).
    pub stale_entry_count: usize,
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
        let target_root = if should_store_candidate_globally(&candidate, state) {
            permanent_long_term_path()
        } else {
            root.clone()
        };
        let _ = upsert_entry(&target_root, candidate).await;
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
    let now = chrono::Utc::now();
    for (path, entry) in list_entries(root).await {
        // Skip non-active entries (archived / superseded).
        if entry.status != EntryStatus::Active {
            continue;
        }
        // Honour TTL expiry for Ephemeral entries.
        if let Some(ref exp) = entry.expires_at {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(exp) {
                if dt.with_timezone(&chrono::Utc) < now {
                    continue;
                }
            }
        }
        // Field-weighted scoring: topic matches are most important, entities are
        // high-precision tags, summary contains the bulk of the substance.
        let topic_score = score_terms_against_text(&entry.topic, &query_terms) * 3.0;
        let summary_score = score_terms_against_text(&entry.summary, &query_terms) * 1.5;
        let entity_score = score_terms_against_text(&entry.entities.join(" "), &query_terms) * 2.0;

        // Recency bonus: full +0.5 within 7 days, linear decay to 0 at 90 days.
        // Falls back to last_reused_at, then created_at.
        let recency_ts = entry
            .last_reused_at
            .as_deref()
            .or(Some(entry.created_at.as_str()));
        let recency_bonus_new = recency_ts
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| {
                let days_old = now
                    .signed_duration_since(dt.with_timezone(&chrono::Utc))
                    .num_days()
                    .max(0) as f64;
                (1.0 - (days_old / 90.0)).max(0.0) * 0.5
            })
            .unwrap_or(0.0);

        let field_score = topic_score + summary_score + entity_score;
        if field_score <= 0.0 {
            continue;
        }
        // Blend field-weighted TF-IDF score with quality signals.
        let quality = (entry.confidence as f64 * 0.35
            + entry.stability as f64 * 0.25
            + entry.importance as f64 * 0.25
            + entry.reuse_probability as f64 * 0.15)
            .clamp(0.3, 1.0);
        let score = field_score * quality + recency_bonus_new;
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

/// Count entries that are stale: not reused in >90 days AND stability < 0.4.
pub async fn count_stale_entries(root: &Path) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(90);
    list_entries(root)
        .await
        .into_iter()
        .filter(|(_, entry)| is_stale(entry, cutoff))
        .count()
}

/// Delete stale entries and return the number removed.
///
/// An entry is stale if it has not been reused in >90 days AND has stability < 0.4.
/// Pass `dry_run = true` to count without deleting.
pub async fn prune_stale_entries(root: &Path, dry_run: bool) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(90);
    let mut removed = 0;
    for (path, entry) in list_entries(root).await {
        if !is_stale(&entry, cutoff) {
            continue;
        }
        if dry_run {
            removed += 1;
            continue;
        }
        if let Err(e) = fs::remove_file(&path).await {
            tracing::warn!("prune_stale_entries: failed to remove {:?}: {}", path, e);
        } else {
            tracing::info!("prune_stale_entries: removed {:?}", path);
            removed += 1;
        }
    }
    removed
}

fn is_stale(entry: &LongTermMemoryEntry, cutoff: chrono::DateTime<chrono::Utc>) -> bool {
    // Non-active entries are handled separately (archive/supersede), not stale.
    if entry.status != EntryStatus::Active {
        return false;
    }
    // Permanent entries are never stale.
    if entry.retention_class == RetentionClass::Permanent {
        return false;
    }
    // Ephemeral entries: check hard TTL first.
    if entry.retention_class == RetentionClass::Ephemeral {
        if let Some(ref exp) = entry.expires_at {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(exp) {
                return dt.with_timezone(&chrono::Utc) < chrono::Utc::now();
            }
        }
        // Ephemeral without explicit expiry: 30-day window.
        let eph_cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        let old = entry
            .last_reused_at
            .as_deref()
            .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc) < eph_cutoff)
            .unwrap_or(true);
        return old;
    }
    // Session-class entries use a shorter 30-day window.
    let effective_cutoff = if entry.retention_class == RetentionClass::Session {
        chrono::Utc::now() - chrono::Duration::days(30)
    } else {
        cutoff
    };
    let old = entry
        .last_reused_at
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc) < effective_cutoff)
        .unwrap_or(true);
    old && entry.stability < 0.4
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
                && !is_unapproved_user_preference(d)
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
                && !is_unapproved_user_preference(f)
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
        confidence: 0.70,
        stability: 0.55,
        importance: 0.60,
        reuse_probability: 0.55,
        retention_class: RetentionClass::Session,
        status: EntryStatus::Active,
        expires_at: Some((chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339()),
        ..Default::default()
    };

    let _ = upsert_entry(root, entry.clone()).await;
    Some(entry)
}

async fn upsert_entry(root: &Path, entry: LongTermMemoryEntry) -> Result<(), std::io::Error> {
    fs::create_dir_all(root).await?;

    let slug = slugify(&entry.topic);

    // ── Write Gate ──────────────────────────────────────────────────────────
    // Global cap: cheap metadata-only file count first; only do the expensive
    // full content scan when we're actually at or near the cap.
    let total_files = count_json_files(root).await;
    if total_files >= GLOBAL_SOFT_CAP {
        let all_entries = list_entries(root).await;
        let total_active = all_entries
            .iter()
            .filter(|(_, e)| e.status == EntryStatus::Active)
            .count();
        if total_active >= GLOBAL_SOFT_CAP {
            let weakest = all_entries
                .iter()
                .filter(|(_, e)| {
                    e.status == EntryStatus::Active
                        && e.retention_class != RetentionClass::Permanent
                })
                .min_by(|(_, a), (_, b)| {
                    quality_score(a)
                        .partial_cmp(&quality_score(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some((evict_path, evicted)) = weakest {
                let _ = fs::remove_file(evict_path).await;
                tracing::info!(
                    target: "omiga::memory::write_gate",
                    cap = GLOBAL_SOFT_CAP,
                    total_active,
                    evicted_topic = %evicted.topic,
                    evicted_score = quality_score(evicted),
                    "write_gate: global cap reached — evicted weakest entry"
                );
            }
        }
    }

    // Per-topic cap: only load files matching the topic family prefix (typically 1-5
    // files), not the entire directory. No full content scan unless cap is hit.
    let family_prefix = family_slug(&slug);
    let topic_entries = list_entries_with_prefix(root, &family_prefix).await;
    let active_topic: Vec<_> = topic_entries
        .iter()
        .filter(|(_, e)| e.status == EntryStatus::Active)
        .collect();

    if active_topic.len() >= MAX_ENTRIES_PER_TOPIC {
        let weakest = active_topic.iter().min_by(|(_, a), (_, b)| {
            quality_score(a)
                .partial_cmp(&quality_score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some((supersede_path, old)) = weakest {
            let mut updated = (*old).clone();
            updated.status = EntryStatus::Superseded;
            if let Ok(json) = serde_json::to_string_pretty(&updated) {
                let _ = fs::write(supersede_path, json).await;
            }
            tracing::debug!(
                target: "omiga::memory::write_gate",
                family = %family_prefix,
                new_topic = %entry.topic,
                superseded_topic = %old.topic,
                superseded_score = quality_score(old),
                "write_gate: per-topic cap reached — superseded weakest entry"
            );
        }
    }
    // ── End Write Gate ───────────────────────────────────────────────────────

    let path = root.join(format!("{}.json", slug));
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

/// Composite quality score used for write-gate eviction (higher = keep).
fn quality_score(e: &LongTermMemoryEntry) -> f32 {
    e.confidence * 0.5 + e.stability * 0.3 + e.importance * 0.2
}

/// Topic-family prefix: first 2 hyphen-components of the slug.
fn family_slug(slug: &str) -> String {
    slug.splitn(3, '-').take(2).collect::<Vec<_>>().join("-")
}

/// Count JSON files without reading their content — O(n) metadata scan.
async fn count_json_files(root: &Path) -> usize {
    let Ok(mut dir) = fs::read_dir(root).await else {
        return 0;
    };
    let mut count = 0usize;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            count += 1;
        }
    }
    count
}

/// Load only entries whose filename starts with `prefix` — avoids full-directory scan.
async fn list_entries_with_prefix(
    root: &Path,
    prefix: &str,
) -> Vec<(PathBuf, LongTermMemoryEntry)> {
    let Ok(mut dir) = fs::read_dir(root).await else {
        return vec![];
    };
    let mut out = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with(prefix) || !name.ends_with(".json") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path).await {
            if let Ok(parsed) = serde_json::from_str::<LongTermMemoryEntry>(&raw) {
                out.push((path, parsed));
            }
        }
    }
    out
}

fn merge_entry(
    existing: Option<LongTermMemoryEntry>,
    incoming: LongTermMemoryEntry,
) -> LongTermMemoryEntry {
    let Some(mut existing) = existing else {
        return incoming;
    };
    // Take the richer summary.
    if existing.summary.chars().count() < incoming.summary.chars().count() {
        existing.summary = incoming.summary;
    }
    // Quality signals: take max (new evidence can only raise, not lower).
    existing.confidence = existing.confidence.max(incoming.confidence);
    existing.stability = existing.stability.max(incoming.stability);
    existing.importance = existing.importance.max(incoming.importance);
    existing.reuse_probability = existing.reuse_probability.max(incoming.reuse_probability);
    // Timestamps: keep earliest access, update latest reuse.
    existing.last_reused_at = incoming.last_reused_at;
    // Re-activate if incoming is Active (allows un-archiving via re-promotion).
    if incoming.status == EntryStatus::Active {
        existing.status = EntryStatus::Active;
    }
    // Extend TTL if incoming has a later expiry.
    if let Some(new_exp) = incoming.expires_at {
        existing.expires_at = Some(match &existing.expires_at {
            Some(old_exp) => {
                if new_exp > *old_exp {
                    new_exp
                } else {
                    old_exp.clone()
                }
            }
            None => new_exp,
        });
    }
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

pub fn slugify_pub(value: &str) -> String {
    slugify(value)
}

pub fn truncate_pub(text: &str, max_chars: usize) -> String {
    truncate_chars(text, max_chars)
}

pub async fn upsert_entry_pub(root: &std::path::Path, entry: LongTermMemoryEntry) {
    let _ = upsert_entry(root, entry).await;
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
            // Reject log: surface borderline rejects for observability.
            if item.confidence >= 0.70
                && item.status
                    == crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active
            {
                tracing::debug!(
                    target: "omiga::memory::write_gate",
                    topic = %truncate_chars(&item.text, 60),
                    confidence = item.confidence,
                    touch_count = item.touch_count,
                    "write_gate: rejected promotion candidate"
                );
            }
            continue;
        }
        let (retention, ttl_days) = match kind {
            LongTermMemoryKind::ResearchInsight
            | LongTermMemoryKind::MethodLesson
            | LongTermMemoryKind::ProjectExperience
            | LongTermMemoryKind::TaskConclusion => (RetentionClass::LongTerm, None),
            LongTermMemoryKind::SessionSummary => (RetentionClass::Session, Some(30u32)),
        };
        let expires_at = ttl_days
            .map(|days| (chrono::Utc::now() + chrono::Duration::days(days as i64)).to_rfc3339());
        let touch_ratio = (item.touch_count as f32 / 6.0).min(0.3);
        candidates.push(LongTermMemoryEntry {
            topic: truncate_chars(&item.text, 120),
            summary: truncate_chars(&item.text, 280),
            kind,
            entities: derive_query_terms(&item.text).into_iter().take(5).collect(),
            source_sessions: vec![session_id.to_string()],
            source_artifacts: item.source_message_ids.clone(),
            confidence: item.confidence.clamp(0.0, 1.0),
            stability: ((item.touch_count as f32) / 4.0).clamp(0.45, 1.0),
            importance: (item.confidence * 0.7 + touch_ratio).clamp(0.0, 1.0),
            reuse_probability: (item.confidence * 0.6 + touch_ratio * 1.5).clamp(0.0, 1.0),
            retention_class: retention,
            status: EntryStatus::Active,
            expires_at,
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
    let approved_user_preference = is_approved_user_preference(item);
    if item.text.chars().count() < 24 && !approved_user_preference {
        return false;
    }
    if looks_transient_for_long_term(&item.text) {
        return false;
    }
    if is_unapproved_user_preference(item) {
        return false;
    }
    if approved_user_preference {
        return item.confidence >= 0.82 && !item.source_message_ids.is_empty();
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
        Some("approved_user_preference") | Some("confirmed_user_preference") => {
            LongTermMemoryKind::ProjectExperience
        }
        Some("session_summary") => LongTermMemoryKind::SessionSummary,
        _ => LongTermMemoryKind::TaskConclusion,
    }
}

fn is_unapproved_user_preference(
    item: &crate::domain::memory::working_memory::WorkingMemoryItem,
) -> bool {
    if is_approved_user_preference(item) {
        return false;
    }
    crate::domain::memory::working_memory::is_user_preference_candidate_text(&item.text)
}

fn is_approved_user_preference(
    item: &crate::domain::memory::working_memory::WorkingMemoryItem,
) -> bool {
    matches!(
        item.kind.as_deref(),
        Some("approved_user_preference") | Some("confirmed_user_preference")
    )
}

fn should_store_candidate_globally(
    entry: &LongTermMemoryEntry,
    state: &WorkingMemoryState,
) -> bool {
    let entry_key = slugify(&entry.topic);
    state.candidate_memories.iter().any(|candidate| {
        candidate.status == crate::domain::memory::working_memory::CandidateMemoryStatus::Approved
            && candidate.scope
                == crate::domain::memory::working_memory::CandidateMemoryScope::Global
            && slugify(&candidate.text) == entry_key
    })
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
            confidence: 0.9,
            stability: 0.8,
            ..Default::default()
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

    #[test]
    fn promotion_candidates_hold_user_preferences_for_confirmation() {
        let preference_state = WorkingMemoryState {
            decisions: vec![crate::domain::memory::working_memory::WorkingMemoryItem {
                text: "以后所有项目默认优先用 Rscript 脚本而不是 notebook".to_string(),
                source_message_ids: vec!["m1".to_string(), "m2".to_string()],
                confidence: 0.95,
                last_touched_turn: 6,
                expires_after_turns: 24,
                status: crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active,
                kind: Some("project_experience".to_string()),
                touch_count: 4,
            }],
            ..WorkingMemoryState::default()
        };

        assert!(
            build_promotion_candidates("s1", &preference_state).is_empty(),
            "unapproved user preferences should stay as candidates, not auto-promote"
        );
    }

    #[test]
    fn promotion_candidates_allow_approved_user_preferences() {
        let approved_state = WorkingMemoryState {
            working_facts: vec![crate::domain::memory::working_memory::WorkingMemoryItem {
                text: "请记住：以后所有项目默认先分析再行动。".to_string(),
                source_message_ids: vec!["m1".to_string(), "m2".to_string()],
                confidence: 0.96,
                last_touched_turn: 6,
                expires_after_turns: 64,
                status: crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active,
                kind: Some("approved_user_preference".to_string()),
                touch_count: 3,
            }],
            ..WorkingMemoryState::default()
        };

        let candidates = build_promotion_candidates("s1", &approved_state);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, LongTermMemoryKind::ProjectExperience);
    }

    #[test]
    fn approved_global_candidate_targets_global_long_term_memory() {
        let text = "请记住：以后所有项目默认先分析再行动。";
        let state = WorkingMemoryState {
            candidate_memories: vec![crate::domain::memory::working_memory::CandidateMemory {
                text: text.to_string(),
                source_message_ids: vec!["m1".to_string()],
                confidence: 0.96,
                reason: "explicit_remember_request".to_string(),
                scope: crate::domain::memory::working_memory::CandidateMemoryScope::Global,
                status: crate::domain::memory::working_memory::CandidateMemoryStatus::Approved,
                created_at: chrono::Utc::now().to_rfc3339(),
                last_seen_at: chrono::Utc::now().to_rfc3339(),
                touch_count: 1,
            }],
            ..WorkingMemoryState::default()
        };
        let entry = LongTermMemoryEntry {
            topic: text.to_string(),
            summary: text.to_string(),
            kind: LongTermMemoryKind::ProjectExperience,
            ..Default::default()
        };

        assert!(should_store_candidate_globally(&entry, &state));
    }

    #[tokio::test]
    async fn session_summary_ignores_unapproved_user_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let state = WorkingMemoryState {
            session_goal: Some("更新用户长期偏好".to_string()),
            active_topic: Some("用户偏好".to_string()),
            decisions: vec![crate::domain::memory::working_memory::WorkingMemoryItem {
                text: "以后所有项目默认优先用 Rscript 脚本而不是 notebook".to_string(),
                source_message_ids: vec!["m1".to_string(), "m2".to_string()],
                confidence: 0.95,
                last_touched_turn: 6,
                expires_after_turns: 24,
                status: crate::domain::memory::working_memory::WorkingMemoryItemStatus::Active,
                kind: Some("project_experience".to_string()),
                touch_count: 4,
            }],
            ..WorkingMemoryState::default()
        };

        assert!(
            create_session_summary(temp.path(), "s1", &state).await.is_none(),
            "preference-only turns should not archive unapproved preferences as long-term summaries"
        );
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
            confidence: 0.95,
            stability: 0.9,
            importance: 0.9,
            last_reused_at: Some(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        };
        let low_quality = LongTermMemoryEntry {
            topic: "memory search weak".to_string(),
            summary: "Memory search result from uncertain source.".to_string(),
            kind: LongTermMemoryKind::TaskConclusion,
            entities: vec!["memory".to_string(), "search".to_string()],
            source_sessions: vec!["s2".to_string()],
            confidence: 0.3,
            stability: 0.2,
            importance: 0.2,
            ..Default::default()
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
            source_sessions: vec!["s1".to_string()],
            confidence: 0.5,
            stability: 0.2,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            last_reused_at: Some("2024-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let fresh = LongTermMemoryEntry {
            topic: "active project convention".to_string(),
            summary: "Always run tests before pushing.".to_string(),
            kind: LongTermMemoryKind::ProjectExperience,
            source_sessions: vec!["s2".to_string()],
            confidence: 0.9,
            stability: 0.8,
            last_reused_at: Some(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        };

        upsert_entry(temp.path(), stale).await.unwrap();
        upsert_entry(temp.path(), fresh).await.unwrap();

        let stale_count = count_stale_entries(temp.path()).await;
        assert_eq!(stale_count, 1, "only the old weak entry should be stale");
    }

    #[tokio::test]
    async fn prune_stale_entries_removes_only_weak_old_entries() {
        let temp = tempfile::tempdir().unwrap();

        let stale = LongTermMemoryEntry {
            topic: "stale-fact".to_string(),
            summary: "Outdated and weak fact.".to_string(),
            kind: LongTermMemoryKind::TaskConclusion,
            source_sessions: vec!["s1".to_string()],
            confidence: 0.4,
            stability: 0.15,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            last_reused_at: Some("2024-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        let keeper = LongTermMemoryEntry {
            topic: "keeper-fact".to_string(),
            summary: "Strong, recently used insight.".to_string(),
            kind: LongTermMemoryKind::ResearchInsight,
            source_sessions: vec!["s2".to_string()],
            confidence: 0.9,
            stability: 0.85,
            last_reused_at: Some(chrono::Utc::now().to_rfc3339()),
            ..Default::default()
        };

        upsert_entry(temp.path(), stale).await.unwrap();
        upsert_entry(temp.path(), keeper).await.unwrap();

        assert_eq!(count_entries(temp.path()).await, 2);

        let dry_removed = prune_stale_entries(temp.path(), true).await;
        assert_eq!(dry_removed, 1, "dry run should report 1 stale entry");
        assert_eq!(
            count_entries(temp.path()).await,
            2,
            "dry run must not delete anything"
        );

        let removed = prune_stale_entries(temp.path(), false).await;
        assert_eq!(removed, 1);
        assert_eq!(
            count_entries(temp.path()).await,
            1,
            "only the keeper should remain"
        );
        let remaining = list_entries(temp.path()).await;
        assert_eq!(remaining[0].1.topic, "keeper-fact");
    }

    #[tokio::test]
    async fn write_gate_supersedes_weakest_entry_when_topic_cap_reached() {
        let temp = tempfile::tempdir().unwrap();
        // Write MAX_ENTRIES_PER_TOPIC + 1 entries with the same slug *prefix*
        // but distinct topic names — simulating a family of related conclusions.
        for i in 0..=MAX_ENTRIES_PER_TOPIC {
            let entry = LongTermMemoryEntry {
                topic: format!("recall ordering variant {}", i),
                summary: format!("recall ordering fact variant {}", i),
                confidence: 0.5 + i as f32 * 0.05,
                stability: 0.5,
                ..Default::default()
            };
            upsert_entry(temp.path(), entry).await.unwrap();
        }
        let all = list_entries(temp.path()).await;
        // After the cap is reached, at least one entry must be Superseded.
        let superseded = all
            .iter()
            .filter(|(_, e)| e.status == EntryStatus::Superseded)
            .count();
        assert!(
            superseded >= 1,
            "at least one entry should be superseded when per-topic cap is exceeded; all statuses: {:?}",
            all.iter().map(|(_, e)| (&e.topic, &e.status)).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn search_excludes_superseded_and_archived_entries() {
        let temp = tempfile::tempdir().unwrap();
        let active = LongTermMemoryEntry {
            topic: "active-entry".to_string(),
            summary: "active memory fact about recall ordering".to_string(),
            status: EntryStatus::Active,
            ..Default::default()
        };
        let superseded = LongTermMemoryEntry {
            topic: "superseded-entry".to_string(),
            summary: "superseded memory fact about recall ordering".to_string(),
            status: EntryStatus::Superseded,
            ..Default::default()
        };
        upsert_entry(temp.path(), active).await.unwrap();
        // Write superseded directly (bypassing gate) by slug name
        let path = temp.path().join("superseded-entry.json");
        let json = serde_json::to_string_pretty(&superseded).unwrap();
        tokio::fs::write(path, json).await.unwrap();

        let matches = search_entries(temp.path(), "recall ordering", 10, false).await;
        assert_eq!(
            matches.len(),
            1,
            "superseded entry must not appear in search"
        );
        assert_eq!(matches[0].title, "active-entry");
    }

    #[tokio::test]
    async fn ephemeral_entry_expires_correctly() {
        let temp = tempfile::tempdir().unwrap();
        // Already expired
        let expired = LongTermMemoryEntry {
            topic: "ephemeral-old".to_string(),
            summary: "ephemeral memory fact about recall ordering".to_string(),
            retention_class: RetentionClass::Ephemeral,
            expires_at: Some("2024-01-01T00:00:00Z".to_string()),
            ..Default::default()
        };
        // Not yet expired
        let valid = LongTermMemoryEntry {
            topic: "ephemeral-valid".to_string(),
            summary: "ephemeral memory valid about recall ordering".to_string(),
            retention_class: RetentionClass::Ephemeral,
            expires_at: Some((chrono::Utc::now() + chrono::Duration::days(10)).to_rfc3339()),
            ..Default::default()
        };
        let path_exp = temp.path().join("ephemeral-old.json");
        let path_val = temp.path().join("ephemeral-valid.json");
        tokio::fs::write(path_exp, serde_json::to_string_pretty(&expired).unwrap())
            .await
            .unwrap();
        tokio::fs::write(path_val, serde_json::to_string_pretty(&valid).unwrap())
            .await
            .unwrap();

        let results = search_entries(temp.path(), "recall ordering", 10, false).await;
        assert_eq!(results.len(), 1, "expired Ephemeral must not appear");
        assert_eq!(results[0].title, "ephemeral-valid");
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
        assert!(
            result.is_some(),
            "rich state should produce a session summary"
        );
        let entry = result.unwrap();
        assert_eq!(entry.kind, LongTermMemoryKind::SessionSummary);
        assert!(entry.summary.contains("Goal:"));
    }

    #[test]
    fn quality_score_ranks_high_confidence_above_low() {
        let high = LongTermMemoryEntry {
            confidence: 0.9,
            stability: 0.8,
            importance: 0.9,
            ..Default::default()
        };
        let low = LongTermMemoryEntry {
            confidence: 0.3,
            stability: 0.2,
            importance: 0.2,
            ..Default::default()
        };
        assert!(
            quality_score(&high) > quality_score(&low),
            "high-quality entry must score above low-quality entry"
        );
    }

    #[test]
    fn quality_score_does_not_evict_permanent_entries() {
        // Permanent retention is excluded from global-cap eviction; ensure quality
        // score is computed correctly (eviction code filters on retention_class separately).
        let permanent = LongTermMemoryEntry {
            retention_class: RetentionClass::Permanent,
            confidence: 0.1,
            stability: 0.1,
            importance: 0.1,
            ..Default::default()
        };
        // Just verify it doesn't panic and returns a float.
        let score = quality_score(&permanent);
        assert!((0.0..=1.0).contains(&score));
    }

    #[test]
    fn merge_entry_takes_max_quality_scores() {
        let existing = LongTermMemoryEntry {
            topic: "existing".to_string(),
            summary: "short".to_string(),
            confidence: 0.5,
            stability: 0.4,
            importance: 0.3,
            reuse_probability: 0.3,
            ..Default::default()
        };
        let incoming = LongTermMemoryEntry {
            topic: "existing".to_string(),
            summary: "a longer and richer summary with more detail".to_string(),
            confidence: 0.8,
            stability: 0.2,
            importance: 0.9,
            reuse_probability: 0.7,
            ..Default::default()
        };
        let merged = merge_entry(Some(existing), incoming);
        // confidence: max(0.5, 0.8) = 0.8
        assert!(
            (merged.confidence - 0.8).abs() < 1e-6,
            "confidence should be max"
        );
        // stability: max(0.4, 0.2) = 0.4
        assert!(
            (merged.stability - 0.4).abs() < 1e-6,
            "stability should be max"
        );
        // importance: max(0.3, 0.9) = 0.9
        assert!(
            (merged.importance - 0.9).abs() < 1e-6,
            "importance should be max"
        );
        // reuse_probability: max(0.3, 0.7) = 0.7
        assert!((merged.reuse_probability - 0.7).abs() < 1e-6);
        // summary: richer incoming wins
        assert!(
            merged.summary.contains("longer"),
            "richer summary should win"
        );
    }

    #[test]
    fn merge_entry_reactivates_archived_when_incoming_is_active() {
        let archived = LongTermMemoryEntry {
            topic: "archived".to_string(),
            summary: "archived fact".to_string(),
            status: EntryStatus::Archived,
            ..Default::default()
        };
        let incoming = LongTermMemoryEntry {
            topic: "archived".to_string(),
            summary: "re-promoted fact".to_string(),
            status: EntryStatus::Active,
            ..Default::default()
        };
        let merged = merge_entry(Some(archived), incoming);
        assert_eq!(
            merged.status,
            EntryStatus::Active,
            "Active incoming should reactivate Archived existing"
        );
    }

    #[test]
    fn merge_entry_extends_ttl_to_later_expiry() {
        let near_future = (chrono::Utc::now() + chrono::Duration::days(5)).to_rfc3339();
        let far_future = (chrono::Utc::now() + chrono::Duration::days(60)).to_rfc3339();
        let existing = LongTermMemoryEntry {
            topic: "expiry-test".to_string(),
            summary: "expiry test fact".to_string(),
            expires_at: Some(near_future.clone()),
            ..Default::default()
        };
        let incoming = LongTermMemoryEntry {
            topic: "expiry-test".to_string(),
            summary: "expiry test fact extended".to_string(),
            expires_at: Some(far_future.clone()),
            ..Default::default()
        };
        let merged = merge_entry(Some(existing), incoming);
        assert_eq!(
            merged.expires_at.as_deref(),
            Some(far_future.as_str()),
            "TTL should extend to the later expiry"
        );
    }

    #[test]
    fn merge_entry_deduplicates_entities_and_sessions() {
        let existing = LongTermMemoryEntry {
            topic: "dedup-test".to_string(),
            summary: "fact".to_string(),
            entities: vec!["rust".to_string(), "memory".to_string()],
            source_sessions: vec!["sess-1".to_string()],
            ..Default::default()
        };
        let incoming = LongTermMemoryEntry {
            topic: "dedup-test".to_string(),
            summary: "fact updated".to_string(),
            entities: vec!["memory".to_string(), "tokio".to_string()],
            source_sessions: vec!["sess-1".to_string(), "sess-2".to_string()],
            ..Default::default()
        };
        let merged = merge_entry(Some(existing), incoming);
        // "memory" must not be duplicated
        let memory_count = merged
            .entities
            .iter()
            .filter(|e| e.as_str() == "memory")
            .count();
        assert_eq!(memory_count, 1, "entities must not be duplicated");
        assert_eq!(merged.entities.len(), 3, "rust + memory + tokio = 3");
        let sess1_count = merged
            .source_sessions
            .iter()
            .filter(|s| s.as_str() == "sess-1")
            .count();
        assert_eq!(sess1_count, 1, "sessions must not be duplicated");
        assert_eq!(merged.source_sessions.len(), 2);
    }

    #[tokio::test]
    async fn field_weighted_scoring_ranks_topic_match_higher() {
        let temp = tempfile::tempdir().unwrap();

        // Entry A: query term appears in the topic (high-weight field).
        let topic_match = LongTermMemoryEntry {
            topic: "authentication scheme".to_string(),
            summary: "Describes how user sessions are established.".to_string(),
            kind: LongTermMemoryKind::ProjectExperience,
            entities: vec!["auth".to_string()],
            source_sessions: vec!["s1".to_string()],
            confidence: 0.8,
            stability: 0.7,
            importance: 0.7,
            reuse_probability: 0.7,
            ..Default::default()
        };
        // Entry B: query term appears only in the summary (lower-weight field).
        // Same quality scores to isolate field-weight effect.
        let summary_match = LongTermMemoryEntry {
            topic: "session management overview".to_string(),
            summary: "The authentication scheme uses JWT tokens for stateless login.".to_string(),
            kind: LongTermMemoryKind::ResearchInsight,
            entities: vec!["jwt".to_string()],
            source_sessions: vec!["s2".to_string()],
            confidence: 0.8,
            stability: 0.7,
            importance: 0.7,
            reuse_probability: 0.7,
            ..Default::default()
        };

        // Write directly so we bypass the write-gate slug collision.
        let path_a = temp.path().join("authentication-scheme.json");
        let path_b = temp.path().join("session-management-overview.json");
        tokio::fs::write(&path_a, serde_json::to_string_pretty(&topic_match).unwrap())
            .await
            .unwrap();
        tokio::fs::write(
            &path_b,
            serde_json::to_string_pretty(&summary_match).unwrap(),
        )
        .await
        .unwrap();

        let results = search_entries(temp.path(), "authentication scheme", 5, false).await;
        assert_eq!(results.len(), 2, "both entries should match");
        assert_eq!(
            results[0].title, "authentication scheme",
            "topic match should rank first (got title '{}' with score {:.4} vs '{}' with score {:.4})",
            results[0].title, results[0].score,
            results[1].title, results[1].score,
        );
        assert!(
            results[0].score > results[1].score,
            "topic match score ({:.4}) must exceed summary-only match score ({:.4})",
            results[0].score,
            results[1].score
        );
    }

    #[test]
    fn family_slug_uses_first_two_hyphen_components() {
        assert_eq!(family_slug("recall-ordering-variant-2"), "recall-ordering");
        assert_eq!(family_slug("single"), "single");
        assert_eq!(family_slug("two-parts"), "two-parts");
        assert_eq!(family_slug("a-b-c-d"), "a-b");
    }
}
