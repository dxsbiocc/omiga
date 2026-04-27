//! Source Registry — Track web pages, PDFs, and literature used in responses.
//!
//! Prevents repeatedly fetching the same URL, enables citation lookup, and
//! provides a retrievable log of which external sources informed which sessions.
//!
//! ## Storage
//!
//! Each source is stored as `sources/{url_hash}.json` under the project's
//! long-term memory directory.  The hash is the first 16 hex chars of SHA-256
//! of the canonical URL (scheme + host + path, lowercased, query stripped).
//!
//! ## Design decisions
//! - URL is the primary key (deduped by canonical form).
//! - `gist` is a short (≤300 chars) summary of what was useful in the page.
//! - `query_context` records the search queries that led to this URL being used,
//!   enabling future recall to match it to similar queries.
//! - `expires_at` is set to 90 days from last access; extended on re-access.
//!   Expired entries are excluded from search and pruned probabilistically.

/// Default TTL for source entries: 90 days from last access.
const SOURCE_TTL_DAYS: i64 = 90;

use crate::domain::pageindex::{derive_query_terms, score_terms_against_text};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEntry {
    /// Original URL as accessed.
    pub url: String,
    /// Canonical URL used as storage key (scheme+host+path, lowercase, no query).
    pub canonical_url: String,
    /// Page title extracted from content (if available).
    pub title: Option<String>,
    /// Hostname/domain for display.
    pub domain: String,
    /// Short summary of what was useful in this source (≤300 chars).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gist: Option<String>,
    /// ISO-8601 timestamp of first access.
    pub accessed_at: String,
    /// ISO-8601 timestamp of most recent use.
    pub last_used_at: String,
    /// Total number of times this source was accessed across all sessions.
    #[serde(default)]
    pub use_count: u32,
    /// Session IDs that used this source.
    #[serde(default)]
    pub sessions: Vec<String>,
    /// Search queries that led to this URL being fetched.
    #[serde(default)]
    pub query_context: Vec<String>,
    /// ISO-8601 expiry timestamp. Entries past this date are excluded from search.
    /// Extended by SOURCE_TTL_DAYS on every re-access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SourceMatch {
    pub url: String,
    pub title: Option<String>,
    pub domain: String,
    pub gist: Option<String>,
    pub score: f64,
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Returns the `sources/` subdirectory under `lt_root`.
pub fn sources_dir(lt_root: &Path) -> PathBuf {
    lt_root.join("sources")
}

fn entry_path(lt_root: &Path, canonical_url: &str) -> PathBuf {
    sources_dir(lt_root).join(format!("{}.json", url_hash(canonical_url)))
}

// ── Core operations ───────────────────────────────────────────────────────────

/// Create or update a source entry (deduped by canonical URL).
/// Re-access extends `expires_at` by SOURCE_TTL_DAYS from now.
pub async fn upsert_source(lt_root: &Path, mut entry: SourceEntry) {
    let dir = sources_dir(lt_root);
    if let Err(e) = fs::create_dir_all(&dir).await {
        tracing::warn!("source_registry: mkdir failed: {}", e);
        return;
    }
    // Always set expiry to TTL from now (extended on every re-access).
    entry.expires_at = Some(
        (chrono::Utc::now() + chrono::Duration::days(SOURCE_TTL_DAYS)).to_rfc3339(),
    );

    let path = entry_path(lt_root, &entry.canonical_url);
    // Merge with existing if present.
    if let Ok(raw) = fs::read_to_string(&path).await {
        if let Ok(existing) = serde_json::from_str::<SourceEntry>(&raw) {
            // Keep earliest access time.
            entry.accessed_at = existing.accessed_at;
            entry.use_count += existing.use_count;
            // Merge sessions (dedup).
            for s in existing.sessions {
                if !entry.sessions.contains(&s) {
                    entry.sessions.push(s);
                }
            }
            // Merge query_context (dedup, cap at 10).
            for q in existing.query_context {
                if !entry.query_context.contains(&q) && entry.query_context.len() < 10 {
                    entry.query_context.push(q);
                }
            }
            // Prefer longer gist.
            if entry.gist.as_ref().map(|g| g.len()).unwrap_or(0)
                < existing.gist.as_ref().map(|g| g.len()).unwrap_or(0)
            {
                entry.gist = existing.gist;
            }
        }
    }

    match serde_json::to_string_pretty(&entry) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json).await {
                tracing::warn!("source_registry: write failed: {}", e);
            }
        }
        Err(e) => tracing::warn!("source_registry: serialize failed: {}", e),
    }
}

/// Returns `true` if the entry has passed its `expires_at` timestamp.
fn is_expired(entry: &SourceEntry) -> bool {
    entry.expires_at.as_deref().map(|exp| {
        chrono::DateTime::parse_from_rfc3339(exp)
            .map(|dt| dt < chrono::Utc::now())
            .unwrap_or(false)
    }).unwrap_or(false)
}

/// List all source entries including expired ones (used for admin/stats).
pub async fn list_sources(lt_root: &Path) -> Vec<SourceEntry> {
    let dir = sources_dir(lt_root);
    if !dir.is_dir() {
        return vec![];
    }
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return vec![];
    };
    let mut out = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&p).await {
            if let Ok(src) = serde_json::from_str::<SourceEntry>(&raw) {
                out.push(src);
            }
        }
    }
    out
}

/// List active (non-expired) source entries.
pub async fn list_active_sources(lt_root: &Path) -> Vec<SourceEntry> {
    list_sources(lt_root).await.into_iter().filter(|e| !is_expired(e)).collect()
}

/// Count non-expired source entries.
pub async fn count_sources(lt_root: &Path) -> usize {
    list_active_sources(lt_root).await.len()
}

/// Count expired source entries.
pub async fn count_stale_sources(lt_root: &Path) -> usize {
    list_sources(lt_root).await.into_iter().filter(|e| is_expired(e)).count()
}

/// Delete expired source entries from disk.
/// Returns the number of entries removed.
pub async fn prune_stale_sources(lt_root: &Path, dry_run: bool) -> usize {
    let dir = sources_dir(lt_root);
    if !dir.is_dir() {
        return 0;
    }
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return 0;
    };
    let mut removed = 0usize;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let expired = if let Ok(raw) = fs::read_to_string(&p).await {
            serde_json::from_str::<SourceEntry>(&raw)
                .map(|src| is_expired(&src))
                .unwrap_or(false)
        } else {
            false
        };
        if expired {
            removed += 1;
            if !dry_run {
                let _ = fs::remove_file(&p).await;
                tracing::debug!("source_registry: pruned expired entry {:?}", p);
            }
        }
    }
    if removed > 0 {
        tracing::info!("source_registry: pruned {} expired source entries (dry_run={})", removed, dry_run);
    }
    removed
}

/// Search sources by keyword (matches URL, title, gist, and query_context).
pub async fn search_sources(lt_root: &Path, query: &str, limit: usize) -> Vec<SourceMatch> {
    let terms = derive_query_terms(query);
    if terms.is_empty() {
        return vec![];
    }
    let mut matches: Vec<(SourceEntry, f64)> = list_active_sources(lt_root)
        .await
        .into_iter()
        .filter_map(|entry| {
            let text = format!(
                "{} {} {} {}",
                entry.url,
                entry.title.as_deref().unwrap_or(""),
                entry.gist.as_deref().unwrap_or(""),
                entry.query_context.join(" ")
            );
            let score = score_terms_against_text(&text, &terms);
            if score > 0.0 {
                Some((entry, score))
            } else {
                None
            }
        })
        .collect();

    matches.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.0.use_count.cmp(&a.0.use_count))
    });
    matches.truncate(limit);

    matches
        .into_iter()
        .map(|(e, score)| SourceMatch {
            url: e.url,
            title: e.title,
            domain: e.domain,
            gist: e.gist,
            score,
        })
        .collect()
}

// ── Builder helpers ───────────────────────────────────────────────────────────

/// Build a `SourceEntry` from a fetched URL and its content.
pub fn entry_from_fetch(
    url: &str,
    content: &str,
    session_id: Option<&str>,
    query_context: Option<&str>,
) -> SourceEntry {
    let canonical = canonicalize_url(url);
    let domain = extract_domain(url);
    let title = extract_title(content);
    let gist = extract_gist(content, 300);
    let now = chrono::Utc::now().to_rfc3339();

    let expires_at = (chrono::Utc::now() + chrono::Duration::days(SOURCE_TTL_DAYS)).to_rfc3339();
    SourceEntry {
        url: url.to_string(),
        canonical_url: canonical,
        title,
        domain,
        gist: Some(gist),
        accessed_at: now.clone(),
        last_used_at: now,
        use_count: 1,
        sessions: session_id.map(|s| vec![s.to_string()]).unwrap_or_default(),
        query_context: query_context
            .filter(|q| !q.trim().is_empty())
            .map(|q| vec![q.to_string()])
            .unwrap_or_default(),
        expires_at: Some(expires_at),
    }
}

/// Build lightweight entries from web_search result text (extracts URLs from output).
pub fn entries_from_search_output(
    output: &str,
    session_id: Option<&str>,
    query: &str,
) -> Vec<SourceEntry> {
    extract_urls_from_text(output)
        .into_iter()
        .map(|url| {
            let canonical = canonicalize_url(&url);
            let domain = extract_domain(&url);
            let now = chrono::Utc::now().to_rfc3339();
            let expires_at = (chrono::Utc::now() + chrono::Duration::days(SOURCE_TTL_DAYS)).to_rfc3339();
            SourceEntry {
                url: url.clone(),
                canonical_url: canonical,
                title: None,
                domain,
                gist: None,
                accessed_at: now.clone(),
                last_used_at: now,
                use_count: 1,
                sessions: session_id.map(|s| vec![s.to_string()]).unwrap_or_default(),
                query_context: if query.trim().is_empty() {
                    vec![]
                } else {
                    vec![query.to_string()]
                },
                expires_at: Some(expires_at),
            }
        })
        .collect()
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn url_hash(canonical: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(canonical.as_bytes());
    hex::encode(&h.finalize()[..8])
}

fn canonicalize_url(url: &str) -> String {
    // Keep scheme + host + path, drop query string and fragment.
    if let Some(no_query) = url.split('?').next() {
        if let Some(no_frag) = no_query.split('#').next() {
            return no_frag.to_lowercase();
        }
    }
    url.to_lowercase()
}

fn extract_domain(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn extract_title(content: &str) -> Option<String> {
    // Try "# Title" markdown heading first.
    for line in content.lines().take(15) {
        let t = line.trim();
        if let Some(heading) = t.strip_prefix("# ") {
            let heading = heading.trim();
            if !heading.is_empty() && heading.len() < 200 {
                return Some(heading.to_string());
            }
        }
        // HTML <title>
        let lower = t.to_lowercase();
        if lower.starts_with("<title>") {
            if let Some(end) = lower.find("</title>") {
                let title = &t[7..end].trim();
                if !title.is_empty() {
                    return Some(title.to_string());
                }
            }
        }
    }
    None
}

fn extract_gist(content: &str, max_chars: usize) -> String {
    // Skip blank lines and headings; take the first prose paragraph.
    let text: String = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("```"))
        .take(5)
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() <= max_chars {
        text
    } else {
        let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn extract_urls_from_text(text: &str) -> Vec<String> {
    // Simple regex-free extraction: find tokens that look like https?:// URLs.
    let mut urls = Vec::new();
    for word in text.split_whitespace() {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '/' && c != '.' && c != '-' && c != '_' && c != '?'  && c != '=' && c != '&' && c != '#');
        if (clean.starts_with("https://") || clean.starts_with("http://"))
            && clean.len() > 12
            && !urls.contains(&clean.to_string())
        {
            urls.push(clean.to_string());
        }
        if urls.len() >= 10 {
            break;
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_strips_query_and_fragment() {
        assert_eq!(
            canonicalize_url("https://example.com/path?foo=bar#section"),
            "https://example.com/path"
        );
        assert_eq!(
            canonicalize_url("HTTP://EXAMPLE.COM/Page"),
            "http://example.com/page"
        );
    }

    #[test]
    fn extract_domain_works() {
        assert_eq!(extract_domain("https://pubmed.ncbi.nlm.nih.gov/12345"), "pubmed.ncbi.nlm.nih.gov");
    }

    #[test]
    fn extract_title_prefers_markdown_heading() {
        let content = "# My Research Paper\n\nSome abstract text here.";
        assert_eq!(extract_title(content), Some("My Research Paper".to_string()));
    }

    #[test]
    fn extract_gist_truncates() {
        let content = "x".repeat(500);
        let gist = extract_gist(&content, 300);
        assert!(gist.chars().count() <= 300);
        assert!(gist.ends_with('…'));
    }

    #[test]
    fn extract_urls_finds_https_links() {
        let text = "Results from https://example.com/paper and https://arxiv.org/abs/1234";
        let urls = extract_urls_from_text(text);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("example.com"));
    }

    #[tokio::test]
    async fn upsert_and_search_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let entry = entry_from_fetch(
            "https://pubmed.ncbi.nlm.nih.gov/12345",
            "# Circadian redox rhythms\n\nNRF2 and glutathione interact with clock genes.",
            Some("sess-1"),
            Some("circadian redox"),
        );
        upsert_source(temp.path(), entry).await;

        let results = search_sources(temp.path(), "circadian NRF2", 5).await;
        assert!(!results.is_empty());
        assert!(results[0].url.contains("pubmed"));
    }

    #[tokio::test]
    async fn upsert_merges_use_count_and_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let e1 = entry_from_fetch("https://example.com/page", "content one", Some("s1"), None);
        let e2 = entry_from_fetch("https://example.com/page?foo=bar", "content two", Some("s2"), None);
        upsert_source(temp.path(), e1).await;
        upsert_source(temp.path(), e2).await;

        let all = list_sources(temp.path()).await;
        // same canonical URL → merged into one entry
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].use_count, 2);
        assert_eq!(all[0].sessions.len(), 2);
    }

    #[tokio::test]
    async fn entry_from_fetch_sets_expires_at() {
        let entry = entry_from_fetch("https://example.com/ttl-test", "content", Some("s1"), None);
        let exp = entry.expires_at.as_deref().expect("expires_at must be set");
        let exp_dt = chrono::DateTime::parse_from_rfc3339(exp).expect("valid RFC-3339");
        let now = chrono::Utc::now();
        // Should be ~90 days in the future (allow ±1 minute for test timing).
        let diff_days = (exp_dt.timestamp() - now.timestamp()) / 86400;
        assert!(
            diff_days >= 89 && diff_days <= 91,
            "expires_at should be ~90 days from now, got {} days",
            diff_days
        );
    }

    #[tokio::test]
    async fn upsert_extends_ttl_on_reaccess() {
        let temp = tempfile::tempdir().unwrap();
        // First access.
        let e1 = entry_from_fetch("https://example.com/reaccess", "first access", Some("s1"), None);
        upsert_source(temp.path(), e1).await;
        let before = list_sources(temp.path()).await;
        let first_exp = before[0].expires_at.clone().unwrap();

        // Re-access: should reset TTL to 90 days from now.
        let e2 = entry_from_fetch("https://example.com/reaccess", "second access", Some("s2"), None);
        upsert_source(temp.path(), e2).await;
        let after = list_sources(temp.path()).await;
        let second_exp = after[0].expires_at.clone().unwrap();

        // Both are ~90 days out so they should be very close.
        assert!(second_exp >= first_exp, "TTL must not shrink on re-access");
    }

    #[tokio::test]
    async fn search_excludes_expired_entries() {
        let temp = tempfile::tempdir().unwrap();

        // Insert an expired entry directly.
        let expired = SourceEntry {
            url: "https://example.com/expired-source".to_string(),
            canonical_url: "https://example.com/expired-source".to_string(),
            title: Some("Expired Research Paper".to_string()),
            domain: "example.com".to_string(),
            gist: Some("circadian redox clock expired paper".to_string()),
            accessed_at: "2024-01-01T00:00:00Z".to_string(),
            last_used_at: "2024-01-01T00:00:00Z".to_string(),
            use_count: 1,
            sessions: vec![],
            query_context: vec!["circadian".to_string()],
            expires_at: Some("2024-01-02T00:00:00Z".to_string()), // past
        };
        let path = sources_dir(temp.path());
        fs::create_dir_all(&path).await.unwrap();
        fs::write(
            path.join(format!("{}.json", "expired00deadbeef")),
            serde_json::to_string_pretty(&expired).unwrap(),
        ).await.unwrap();

        // Insert a valid (non-expired) entry.
        let valid = entry_from_fetch(
            "https://pubmed.ncbi.nlm.nih.gov/valid-paper",
            "# Valid Circadian Paper\n\ncircadian redox NRF2 glutathione",
            Some("s1"),
            Some("circadian"),
        );
        upsert_source(temp.path(), valid).await;

        let results = search_sources(temp.path(), "circadian redox", 10).await;
        // Expired entry must not appear in search results.
        assert!(
            results.iter().all(|r| !r.url.contains("expired")),
            "expired source must not appear in search results: {:?}",
            results.iter().map(|r| &r.url).collect::<Vec<_>>()
        );
        // Valid entry must appear.
        assert!(
            results.iter().any(|r| r.url.contains("valid-paper")),
            "valid source must appear in search results"
        );
    }

    #[tokio::test]
    async fn count_sources_excludes_expired() {
        let temp = tempfile::tempdir().unwrap();

        // One expired, one active.
        let expired = SourceEntry {
            url: "https://example.com/old".to_string(),
            canonical_url: "https://example.com/old".to_string(),
            title: None,
            domain: "example.com".to_string(),
            gist: None,
            accessed_at: "2024-01-01T00:00:00Z".to_string(),
            last_used_at: "2024-01-01T00:00:00Z".to_string(),
            use_count: 1,
            sessions: vec![],
            query_context: vec![],
            expires_at: Some("2024-01-02T00:00:00Z".to_string()),
        };
        let src_dir = sources_dir(temp.path());
        fs::create_dir_all(&src_dir).await.unwrap();
        fs::write(
            src_dir.join("expiredaaaa0000.json"),
            serde_json::to_string_pretty(&expired).unwrap(),
        ).await.unwrap();
        let active = entry_from_fetch("https://example.com/active", "active content", None, None);
        upsert_source(temp.path(), active).await;

        assert_eq!(list_sources(temp.path()).await.len(), 2, "list_sources shows all");
        assert_eq!(count_sources(temp.path()).await, 1, "count_sources excludes expired");
        assert_eq!(count_stale_sources(temp.path()).await, 1, "one stale entry");
    }

    #[tokio::test]
    async fn prune_stale_sources_dry_run_and_real() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = sources_dir(temp.path());
        fs::create_dir_all(&src_dir).await.unwrap();

        let expired = SourceEntry {
            url: "https://example.com/prune-me".to_string(),
            canonical_url: "https://example.com/prune-me".to_string(),
            title: None,
            domain: "example.com".to_string(),
            gist: None,
            accessed_at: "2024-01-01T00:00:00Z".to_string(),
            last_used_at: "2024-01-01T00:00:00Z".to_string(),
            use_count: 1,
            sessions: vec![],
            query_context: vec![],
            expires_at: Some("2024-01-02T00:00:00Z".to_string()),
        };
        let keeper = entry_from_fetch("https://example.com/keeper", "kept content", None, None);
        fs::write(
            src_dir.join("pruneme0deadbeef.json"),
            serde_json::to_string_pretty(&expired).unwrap(),
        ).await.unwrap();
        upsert_source(temp.path(), keeper).await;

        assert_eq!(list_sources(temp.path()).await.len(), 2);

        // Dry run: reports removal but does not delete.
        let dry = prune_stale_sources(temp.path(), true).await;
        assert_eq!(dry, 1, "dry run should report 1 stale source");
        assert_eq!(list_sources(temp.path()).await.len(), 2, "dry run must not delete");

        // Real run: deletes the expired entry.
        let removed = prune_stale_sources(temp.path(), false).await;
        assert_eq!(removed, 1);
        let remaining = list_sources(temp.path()).await;
        assert_eq!(remaining.len(), 1, "only keeper should remain");
        assert!(remaining[0].url.contains("keeper"));
    }
}
