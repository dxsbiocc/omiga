//! Session persistence layer
//!
//! Uses SQLite for storing:
//! - Session metadata
//! - Message history
//! - Settings/preferences
//! - Session-scoped tool state (`todo_write`, V2 tasks)
//! - Session-scoped working memory scratchpad

use crate::domain::agents::background::{BackgroundAgentStatus, BackgroundAgentTask};
use crate::domain::session::{sanitize_background_sidechain_message, AgentTask, Message, TodoItem};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
};
use std::path::Path;
use std::time::Duration;

const MESSAGE_FTS_SCHEMA_KEY: &str = "messages_fts_schema_version";
const MESSAGE_FTS_SCHEMA_VERSION: &str = "1";
const MESSAGE_FTS_MATCH_LIMIT: i64 = 500;
const MESSAGE_TRIGRAM_FTS_SCHEMA_KEY: &str = "messages_trigram_fts_schema_version";
const MESSAGE_TRIGRAM_FTS_SCHEMA_VERSION: &str = "1";
const SESSION_FTS_SCHEMA_KEY: &str = "sessions_fts_schema_version";
const SESSION_FTS_SCHEMA_VERSION: &str = "1";
const SESSION_FTS_MATCH_LIMIT: i64 = 500;
const SESSION_TRIGRAM_FTS_SCHEMA_KEY: &str = "sessions_trigram_fts_schema_version";
const SESSION_TRIGRAM_FTS_SCHEMA_VERSION: &str = "1";

/// Initialize the database.
///
/// Connection-level pragmas applied to **every** connection in the pool:
///
/// | Pragma | Value | Rationale |
/// |--------|-------|-----------|
/// | `journal_mode` | WAL | Concurrent readers never block a writer |
/// | `synchronous` | NORMAL | One fsync per WAL checkpoint, not per commit |
/// | `busy_timeout` | 5000 | Retry for 5 s instead of failing with SQLITE_BUSY |
/// | `cache_size` | -32000 | 32 MB page cache per connection (negative = KiB) |
/// | `temp_store` | MEMORY | Sort/index temporaries in RAM, not a temp file |
/// | `mmap_size` | 256 MiB | Memory-map reads — avoids read() syscalls for hot pages |
/// | `foreign_keys` | ON | Enforce referential integrity |
/// | `wal_autocheckpoint` | 1000 | Bound WAL size to ~4 MB before auto-checkpoint |
pub async fn init_db(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .pragma("busy_timeout", "5000")
        .pragma("cache_size", "-32000")
        .pragma("temp_store", "MEMORY")
        .pragma("mmap_size", "268435456")
        .pragma("foreign_keys", "ON")
        .pragma("wal_autocheckpoint", "1000");

    let pool = SqlitePoolOptions::new()
        // 1 write + up to 3 concurrent readers under WAL.
        .max_connections(4)
        // Keep 2 warm connections so the first query after idle startup
        // doesn't pay the connection-creation latency.
        .min_connections(2)
        // Surface pool exhaustion quickly rather than hanging silently.
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(options)
        .await?;

    // Run migrations
    run_migrations(&pool).await?;

    // Update query-planner statistics after migrations (no-op on a fresh DB).
    // This is cheap and helps the planner pick optimal indexes on startup.
    let _ = sqlx::query("PRAGMA optimize").execute(&pool).await;

    Ok(pool)
}

/// Database migrations
async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            project_path TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            tool_calls TEXT,
            tool_call_id TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Composite index: covers the WHERE session_id = ? ORDER BY created_at ASC, id ASC query
    // so SQLite can satisfy the entire query from the index without a separate sort pass.
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_messages_session_created
        ON messages(session_id, created_at ASC, id ASC)
        "#,
    )
    .execute(pool)
    .await?;
    // Keep old single-column index for any queries that filter by session_id alone.
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)
        "#,
    )
    .execute(pool)
    .await?;

    // Index to satisfy ORDER BY s.updated_at DESC in list_sessions without a sort pass.
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC)
        "#,
    )
    .execute(pool)
    .await?;

    // Migration: assistant token usage JSON (reload in UI)
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN token_usage_json TEXT")
        .execute(pool)
        .await;

    // Migration: Moonshot/Kimi thinking replay text for assistant rows
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN reasoning_content TEXT")
        .execute(pool)
        .await;

    // Migration: per-session active provider (omiga.yaml entry name), independent of global default
    let _ = sqlx::query("ALTER TABLE sessions ADD COLUMN active_provider_entry_name TEXT")
        .execute(pool)
        .await;

    // Migration: follow-up suggestions JSON (persist LLM-generated next step suggestions)
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN follow_up_suggestions_json TEXT")
        .execute(pool)
        .await;

    // Migration: optional assistant turn summary (persist recap text shown in UI)
    let _ = sqlx::query("ALTER TABLE messages ADD COLUMN turn_summary TEXT")
        .execute(pool)
        .await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Migration: Add conversation_rounds table for round state persistence
    // Note: message_id is the assistant message ID - we don't use FK constraint
    // because the message doesn't exist yet when round is created
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversation_rounds (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('running', 'partial', 'cancelled', 'completed')),
            user_message_id TEXT,
            assistant_message_id TEXT,
            cancelled_at TEXT,
            completed_at TEXT,
            error_message TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_constraint_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            round_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            constraint_id TEXT,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS orchestration_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            round_id TEXT,
            message_id TEXT,
            mode TEXT,
            event_type TEXT NOT NULL,
            phase TEXT,
            task_id TEXT,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_orchestration_events_session
        ON orchestration_events(session_id, created_at DESC)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_runtime_constraint_events_round
        ON runtime_constraint_events(round_id, created_at ASC)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_runtime_constraint_events_session
        ON runtime_constraint_events(session_id, created_at DESC)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_rounds_session_id ON conversation_rounds(session_id)
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_rounds_status ON conversation_rounds(status)
        "#,
    )
    .execute(pool)
    .await?;

    // Migration: Remove message_id FK constraint from existing table
    // Check if the old table with FK constraint exists by trying to insert a non-existent message_id
    // If it fails, we need to recreate the table without the FK constraint
    let needs_migration = sqlx::query(
        r#"
        SELECT COUNT(*) FROM sqlite_master
        WHERE type = 'table' AND name = 'conversation_rounds'
        AND sql LIKE '%REFERENCES messages%'
        "#,
    )
    .fetch_one(pool)
    .await
    .map(|row: sqlx::sqlite::SqliteRow| row.get::<i64, _>(0))
    .unwrap_or(0)
        > 0;

    if needs_migration {
        tracing::info!("Migrating conversation_rounds table to remove message_id FK constraint");

        // Backup old data
        let _ = sqlx::query(
            r#"
            ALTER TABLE conversation_rounds RENAME TO conversation_rounds_old
            "#,
        )
        .execute(pool)
        .await;

        // Create new table without FK constraint
        sqlx::query(
            r#"
            CREATE TABLE conversation_rounds (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('running', 'partial', 'cancelled', 'completed')),
                user_message_id TEXT,
                assistant_message_id TEXT,
                cancelled_at TEXT,
                completed_at TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Copy data back
        let _ = sqlx::query(
            r#"
            INSERT INTO conversation_rounds SELECT * FROM conversation_rounds_old
            "#,
        )
        .execute(pool)
        .await;

        // Recreate indexes
        sqlx::query(
            r#"
            CREATE INDEX idx_rounds_session_id ON conversation_rounds(session_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX idx_rounds_status ON conversation_rounds(status)
            "#,
        )
        .execute(pool)
        .await?;

        // Drop old table
        let _ = sqlx::query("DROP TABLE conversation_rounds_old")
            .execute(pool)
            .await;

        tracing::info!("Migration completed successfully");
    }

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session_tool_state (
            session_id TEXT PRIMARY KEY,
            todos_json TEXT NOT NULL DEFAULT '[]',
            agent_tasks_json TEXT NOT NULL DEFAULT '[]',
            updated_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session_working_memory (
            session_id TEXT PRIMARY KEY,
            state_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Background Agent tasks (Rust authority + survive restart; memory cache in BackgroundAgentManager)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS background_agent_tasks (
            task_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            round_id TEXT,
            plan_id TEXT,
            agent_type TEXT NOT NULL,
            description TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,
            result_summary TEXT,
            error_message TEXT,
            output_path TEXT,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    ensure_column_exists(pool, "background_agent_tasks", "round_id", "TEXT").await?;
    ensure_column_exists(pool, "background_agent_tasks", "plan_id", "TEXT").await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_background_agent_tasks_session
        ON background_agent_tasks(session_id)
        "#,
    )
    .execute(pool)
    .await?;

    // Sidechain transcript for background Agent tasks (teammate view; not main session messages)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS background_agent_messages (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (task_id) REFERENCES background_agent_tasks(task_id) ON DELETE CASCADE,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_background_agent_messages_task_seq
        ON background_agent_messages(task_id, seq)
        "#,
    )
    .execute(pool)
    .await?;

    if let Err(err) = ensure_messages_fts(pool).await {
        tracing::warn!(
            target: "omiga::persistence",
            error = %err,
            "FTS5 message index unavailable; session search will use scan fallback"
        );
    }
    if let Err(err) = ensure_messages_trigram_fts(pool).await {
        tracing::warn!(
            target: "omiga::persistence",
            error = %err,
            "FTS5 trigram message index unavailable; literal session search will use scan fallback"
        );
    }
    if let Err(err) = ensure_sessions_fts(pool).await {
        tracing::warn!(
            target: "omiga::persistence",
            error = %err,
            "FTS5 session metadata index unavailable; session search will use scan fallback"
        );
    }
    if let Err(err) = ensure_sessions_trigram_fts(pool).await {
        tracing::warn!(
            target: "omiga::persistence",
            error = %err,
            "FTS5 trigram session metadata index unavailable; literal session search will use scan fallback"
        );
    }

    Ok(())
}

async fn ensure_messages_fts(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let existed = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table' AND name = 'messages_fts'
        "#,
    )
    .fetch_one(pool)
    .await?
        > 0;

    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
        USING fts5(
            content,
            content='messages',
            content_rowid='rowid',
            tokenize='unicode61'
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS messages_fts_ai
        AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, content)
            VALUES (new.rowid, new.content);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS messages_fts_ad
        AFTER DELETE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content)
            VALUES ('delete', old.rowid, old.content);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS messages_fts_au
        AFTER UPDATE OF content ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content)
            VALUES ('delete', old.rowid, old.content);
            INSERT INTO messages_fts(rowid, content)
            VALUES (new.rowid, new.content);
        END
        "#,
    )
    .execute(pool)
    .await?;

    let current_version =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(MESSAGE_FTS_SCHEMA_KEY)
            .fetch_optional(pool)
            .await?;

    if !existed || current_version.as_deref() != Some(MESSAGE_FTS_SCHEMA_VERSION) {
        sqlx::query("INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')")
            .execute(pool)
            .await?;

        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(MESSAGE_FTS_SCHEMA_KEY)
        .bind(MESSAGE_FTS_SCHEMA_VERSION)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn ensure_messages_trigram_fts(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let existed = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table' AND name = 'messages_trigram_fts'
        "#,
    )
    .fetch_one(pool)
    .await?
        > 0;

    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS messages_trigram_fts
        USING fts5(
            content,
            content='messages',
            content_rowid='rowid',
            tokenize='trigram'
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS messages_trigram_fts_ai
        AFTER INSERT ON messages BEGIN
            INSERT INTO messages_trigram_fts(rowid, content)
            VALUES (new.rowid, new.content);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS messages_trigram_fts_ad
        AFTER DELETE ON messages BEGIN
            INSERT INTO messages_trigram_fts(messages_trigram_fts, rowid, content)
            VALUES ('delete', old.rowid, old.content);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS messages_trigram_fts_au
        AFTER UPDATE OF content ON messages BEGIN
            INSERT INTO messages_trigram_fts(messages_trigram_fts, rowid, content)
            VALUES ('delete', old.rowid, old.content);
            INSERT INTO messages_trigram_fts(rowid, content)
            VALUES (new.rowid, new.content);
        END
        "#,
    )
    .execute(pool)
    .await?;

    let current_version =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(MESSAGE_TRIGRAM_FTS_SCHEMA_KEY)
            .fetch_optional(pool)
            .await?;

    if !existed || current_version.as_deref() != Some(MESSAGE_TRIGRAM_FTS_SCHEMA_VERSION) {
        sqlx::query("INSERT INTO messages_trigram_fts(messages_trigram_fts) VALUES ('rebuild')")
            .execute(pool)
            .await?;

        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(MESSAGE_TRIGRAM_FTS_SCHEMA_KEY)
        .bind(MESSAGE_TRIGRAM_FTS_SCHEMA_VERSION)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn ensure_sessions_fts(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let existed = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table' AND name = 'sessions_fts'
        "#,
    )
    .fetch_one(pool)
    .await?
        > 0;

    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts
        USING fts5(
            name,
            project_path,
            content='sessions',
            content_rowid='rowid',
            tokenize='unicode61'
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS sessions_fts_ai
        AFTER INSERT ON sessions BEGIN
            INSERT INTO sessions_fts(rowid, name, project_path)
            VALUES (new.rowid, new.name, new.project_path);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS sessions_fts_ad
        AFTER DELETE ON sessions BEGIN
            INSERT INTO sessions_fts(sessions_fts, rowid, name, project_path)
            VALUES ('delete', old.rowid, old.name, old.project_path);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS sessions_fts_au
        AFTER UPDATE OF name, project_path ON sessions BEGIN
            INSERT INTO sessions_fts(sessions_fts, rowid, name, project_path)
            VALUES ('delete', old.rowid, old.name, old.project_path);
            INSERT INTO sessions_fts(rowid, name, project_path)
            VALUES (new.rowid, new.name, new.project_path);
        END
        "#,
    )
    .execute(pool)
    .await?;

    let current_version =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(SESSION_FTS_SCHEMA_KEY)
            .fetch_optional(pool)
            .await?;

    if !existed || current_version.as_deref() != Some(SESSION_FTS_SCHEMA_VERSION) {
        sqlx::query("INSERT INTO sessions_fts(sessions_fts) VALUES ('rebuild')")
            .execute(pool)
            .await?;

        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(SESSION_FTS_SCHEMA_KEY)
        .bind(SESSION_FTS_SCHEMA_VERSION)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn ensure_sessions_trigram_fts(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let existed = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM sqlite_master
        WHERE type = 'table' AND name = 'sessions_trigram_fts'
        "#,
    )
    .fetch_one(pool)
    .await?
        > 0;

    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS sessions_trigram_fts
        USING fts5(
            name,
            project_path,
            content='sessions',
            content_rowid='rowid',
            tokenize='trigram'
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS sessions_trigram_fts_ai
        AFTER INSERT ON sessions BEGIN
            INSERT INTO sessions_trigram_fts(rowid, name, project_path)
            VALUES (new.rowid, new.name, new.project_path);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS sessions_trigram_fts_ad
        AFTER DELETE ON sessions BEGIN
            INSERT INTO sessions_trigram_fts(sessions_trigram_fts, rowid, name, project_path)
            VALUES ('delete', old.rowid, old.name, old.project_path);
        END
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TRIGGER IF NOT EXISTS sessions_trigram_fts_au
        AFTER UPDATE OF name, project_path ON sessions BEGIN
            INSERT INTO sessions_trigram_fts(sessions_trigram_fts, rowid, name, project_path)
            VALUES ('delete', old.rowid, old.name, old.project_path);
            INSERT INTO sessions_trigram_fts(rowid, name, project_path)
            VALUES (new.rowid, new.name, new.project_path);
        END
        "#,
    )
    .execute(pool)
    .await?;

    let current_version =
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(SESSION_TRIGRAM_FTS_SCHEMA_KEY)
            .fetch_optional(pool)
            .await?;

    if !existed || current_version.as_deref() != Some(SESSION_TRIGRAM_FTS_SCHEMA_VERSION) {
        sqlx::query("INSERT INTO sessions_trigram_fts(sessions_trigram_fts) VALUES ('rebuild')")
            .execute(pool)
            .await?;

        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(SESSION_TRIGRAM_FTS_SCHEMA_KEY)
        .bind(SESSION_TRIGRAM_FTS_SCHEMA_VERSION)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

fn fts5_query_for_session_search(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .take(12)
        .map(|term| {
            let term = term.chars().take(48).collect::<String>();
            format!("{term}*")
        })
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn fts5_trigram_query_for_session_search(query: &str) -> Option<String> {
    let q = query.trim();
    if q.chars().count() < 3 {
        return None;
    }

    let escaped = q.replace('"', "\"\"");
    Some(format!("\"{escaped}\""))
}

fn query_requires_literal_scan(query: &str) -> bool {
    query
        .chars()
        .any(|ch| !ch.is_ascii() || (!ch.is_alphanumeric() && !ch.is_whitespace()))
}

fn merge_session_search_results(
    target: &mut Vec<SessionSearchResult>,
    rows: Vec<SessionSearchResult>,
) {
    for row in rows {
        if let Some(existing) = target.iter_mut().find(|item| item.id == row.id) {
            if existing.match_snippet.is_none() {
                existing.match_snippet = row.match_snippet;
            }
        } else {
            target.push(row);
        }
    }
}

fn sort_and_limit_session_search_results(rows: &mut Vec<SessionSearchResult>, limit: i64) {
    rows.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    rows.truncate(limit as usize);
}

async fn ensure_column_exists(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    column_def: &str,
) -> Result<(), sqlx::Error> {
    let pragma = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;
    let exists = rows.iter().any(|row| {
        row.try_get::<String, _>("name")
            .map(|name| name == column)
            .unwrap_or(false)
    });
    if exists {
        return Ok(());
    }

    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {column_def}");
    sqlx::query(&alter).execute(pool).await?;
    Ok(())
}

fn background_agent_status_db(s: &BackgroundAgentStatus) -> &'static str {
    match s {
        BackgroundAgentStatus::Pending => "pending",
        BackgroundAgentStatus::Running => "running",
        BackgroundAgentStatus::Completed => "completed",
        BackgroundAgentStatus::Failed => "failed",
        BackgroundAgentStatus::Cancelled => "cancelled",
    }
}

fn background_agent_status_from_db(s: &str) -> BackgroundAgentStatus {
    match s {
        "running" => BackgroundAgentStatus::Running,
        "completed" => BackgroundAgentStatus::Completed,
        "failed" => BackgroundAgentStatus::Failed,
        "cancelled" => BackgroundAgentStatus::Cancelled,
        _ => BackgroundAgentStatus::Pending,
    }
}

#[derive(Debug, sqlx::FromRow)]
struct BackgroundAgentTaskRow {
    task_id: String,
    session_id: String,
    message_id: String,
    round_id: Option<String>,
    plan_id: Option<String>,
    agent_type: String,
    description: String,
    status: String,
    created_at: i64,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    result_summary: Option<String>,
    error_message: Option<String>,
    output_path: Option<String>,
}

fn row_to_background_task(row: BackgroundAgentTaskRow) -> BackgroundAgentTask {
    BackgroundAgentTask {
        task_id: row.task_id,
        agent_type: row.agent_type,
        description: row.description,
        status: background_agent_status_from_db(&row.status),
        created_at: row.created_at as u64,
        started_at: row.started_at.map(|u| u as u64),
        completed_at: row.completed_at.map(|u| u as u64),
        result_summary: row.result_summary,
        error_message: row.error_message,
        output_path: row.output_path,
        session_id: row.session_id,
        message_id: row.message_id,
        round_id: row.round_id,
        plan_id: row.plan_id,
    }
}

/// Session repository
pub struct SessionRepository {
    pool: SqlitePool,
}

impl SessionRepository {
    /// Create a new repository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// List all sessions with message counts
    pub async fn list_sessions(&self) -> Result<Vec<SessionWithCount>, sqlx::Error> {
        // Correlated subquery is faster than LEFT JOIN + GROUP BY for SQLite when sessions
        // are few but messages are many: the index idx_messages_session_id satisfies each
        // COUNT(*) without a sort pass, and we avoid producing a large intermediate result.
        let sessions = sqlx::query_as::<_, SessionWithCount>(
            r#"
            SELECT
                s.id,
                s.name,
                s.project_path,
                s.created_at,
                s.updated_at,
                (SELECT COUNT(*) FROM messages WHERE session_id = s.id) as message_count
            FROM sessions s
            ORDER BY s.updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(sessions)
    }

    /// Search sessions by title, project path, or message body.
    pub async fn search_sessions(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<SessionSearchResult>, sqlx::Error> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 100);

        let mut fts_results = Vec::new();
        let mut fts_failed = false;

        if !query_requires_literal_scan(q) {
            if let Some(fts_query) = fts5_query_for_session_search(q) {
                match self.search_sessions_fts(&fts_query, limit).await {
                    Ok(rows) => merge_session_search_results(&mut fts_results, rows),
                    Err(err) => {
                        fts_failed = true;
                        tracing::debug!(
                            target: "omiga::persistence",
                            error = %err,
                            "FTS5 session search failed; falling back to literal scan"
                        );
                    }
                }
            }
        }

        if let Some(trigram_query) = fts5_trigram_query_for_session_search(q) {
            match self
                .search_sessions_trigram_fts(q, &trigram_query, limit)
                .await
            {
                Ok(rows) => merge_session_search_results(&mut fts_results, rows),
                Err(err) => {
                    fts_failed = true;
                    tracing::debug!(
                        target: "omiga::persistence",
                        error = %err,
                        "FTS5 trigram session search failed; falling back to literal scan"
                    );
                }
            }
        }

        if !fts_results.is_empty() {
            sort_and_limit_session_search_results(&mut fts_results, limit);
            return Ok(fts_results);
        }

        if fts_failed {
            tracing::debug!(
                target: "omiga::persistence",
                "FTS session search produced no usable results after an error; falling back to literal scan"
            );
        }
        self.search_sessions_scan(q, limit).await
    }

    async fn search_sessions_fts(
        &self,
        fts_query: &str,
        limit: i64,
    ) -> Result<Vec<SessionSearchResult>, sqlx::Error> {
        sqlx::query_as::<_, SessionSearchResult>(
            r#"
            WITH
            message_hits AS (
                SELECT
                    m.session_id AS id,
                    snippet(messages_fts, 0, '', '', '…', 32) AS match_snippet,
                    0 AS source_order
                FROM messages_fts
                JOIN messages m ON m.rowid = messages_fts.rowid
                WHERE messages_fts MATCH ?
                ORDER BY bm25(messages_fts), m.created_at ASC, m.id ASC
                LIMIT ?
            ),
            session_hits AS (
                SELECT
                    s.id AS id,
                    NULL AS match_snippet,
                    1 AS source_order
                FROM sessions_fts
                JOIN sessions s ON s.rowid = sessions_fts.rowid
                WHERE sessions_fts MATCH ?
                ORDER BY bm25(sessions_fts), s.updated_at DESC
                LIMIT ?
            ),
            hits AS (
                SELECT id, match_snippet, source_order FROM message_hits
                UNION ALL
                SELECT id, match_snippet, source_order FROM session_hits
            ),
            picked AS (
                SELECT
                    h.id,
                    (
                        SELECT h2.match_snippet
                        FROM hits h2
                        WHERE h2.id = h.id
                          AND h2.match_snippet IS NOT NULL
                        ORDER BY h2.source_order
                        LIMIT 1
                    ) AS match_snippet
                FROM hits h
                GROUP BY h.id
            )
            SELECT
                s.id,
                s.name,
                s.project_path,
                s.created_at,
                s.updated_at,
                (SELECT COUNT(*) FROM messages WHERE session_id = s.id) as message_count,
                picked.match_snippet
            FROM picked
            JOIN sessions s ON s.id = picked.id
            ORDER BY s.updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(fts_query)
        .bind(MESSAGE_FTS_MATCH_LIMIT)
        .bind(fts_query)
        .bind(SESSION_FTS_MATCH_LIMIT)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    async fn search_sessions_trigram_fts(
        &self,
        query: &str,
        trigram_query: &str,
        limit: i64,
    ) -> Result<Vec<SessionSearchResult>, sqlx::Error> {
        let q = query.trim().to_lowercase();
        sqlx::query_as::<_, SessionSearchResult>(
            r#"
            WITH
            message_hits AS (
                SELECT
                    m.session_id AS id,
                    snippet(messages_trigram_fts, 0, '', '', '…', 32) AS match_snippet,
                    0 AS source_order
                FROM messages_trigram_fts
                JOIN messages m ON m.rowid = messages_trigram_fts.rowid
                WHERE messages_trigram_fts MATCH ?
                  AND instr(lower(COALESCE(m.content, '')), ?) > 0
                ORDER BY bm25(messages_trigram_fts), m.created_at ASC, m.id ASC
                LIMIT ?
            ),
            session_hits AS (
                SELECT
                    s.id AS id,
                    NULL AS match_snippet,
                    1 AS source_order
                FROM sessions_trigram_fts
                JOIN sessions s ON s.rowid = sessions_trigram_fts.rowid
                WHERE sessions_trigram_fts MATCH ?
                  AND (
                    instr(lower(s.name), ?) > 0
                    OR instr(lower(s.project_path), ?) > 0
                  )
                ORDER BY bm25(sessions_trigram_fts), s.updated_at DESC
                LIMIT ?
            ),
            hits AS (
                SELECT id, match_snippet, source_order FROM message_hits
                UNION ALL
                SELECT id, match_snippet, source_order FROM session_hits
            ),
            picked AS (
                SELECT
                    h.id,
                    (
                        SELECT h2.match_snippet
                        FROM hits h2
                        WHERE h2.id = h.id
                          AND h2.match_snippet IS NOT NULL
                        ORDER BY h2.source_order
                        LIMIT 1
                    ) AS match_snippet
                FROM hits h
                GROUP BY h.id
            )
            SELECT
                s.id,
                s.name,
                s.project_path,
                s.created_at,
                s.updated_at,
                (SELECT COUNT(*) FROM messages WHERE session_id = s.id) as message_count,
                picked.match_snippet
            FROM picked
            JOIN sessions s ON s.id = picked.id
            ORDER BY s.updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(trigram_query)
        .bind(&q)
        .bind(MESSAGE_FTS_MATCH_LIMIT)
        .bind(trigram_query)
        .bind(&q)
        .bind(&q)
        .bind(SESSION_FTS_MATCH_LIMIT)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    async fn search_sessions_scan(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<SessionSearchResult>, sqlx::Error> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }

        sqlx::query_as::<_, SessionSearchResult>(
            r#"
            SELECT
                s.id,
                s.name,
                s.project_path,
                s.created_at,
                s.updated_at,
                (SELECT COUNT(*) FROM messages WHERE session_id = s.id) as message_count,
                (
                    SELECT
                        CASE
                            WHEN instr(lower(COALESCE(m.content, '')), ?) > 80
                            THEN '…' || substr(m.content, instr(lower(COALESCE(m.content, '')), ?) - 80, 240)
                            ELSE substr(m.content, 1, 240)
                        END
                    FROM messages m
                    WHERE m.session_id = s.id
                      AND instr(lower(COALESCE(m.content, '')), ?) > 0
                    ORDER BY m.created_at ASC, m.id ASC
                    LIMIT 1
                ) as match_snippet
            FROM sessions s
            WHERE instr(lower(s.name), ?) > 0
               OR instr(lower(s.project_path), ?) > 0
               OR EXISTS (
                    SELECT 1
                    FROM messages m
                    WHERE m.session_id = s.id
                      AND instr(lower(COALESCE(m.content, '')), ?) > 0
               )
            ORDER BY s.updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(&q)
        .bind(&q)
        .bind(&q)
        .bind(&q)
        .bind(&q)
        .bind(&q)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    /// Get session metadata only (no messages) — used by the paginated `load_session` path.
    pub async fn get_session_meta(&self, id: &str) -> Result<Option<SessionRecord>, sqlx::Error> {
        sqlx::query_as::<_, SessionRecord>(
            "SELECT id, name, project_path, created_at, updated_at, active_provider_entry_name FROM sessions WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn find_latest_session_id_for_project(
        &self,
        project_path: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT id
            FROM sessions
            WHERE project_path = ?
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(project_path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
    }

    /// Return at most `limit` messages for `session_id`, ordered newest-first.
    /// The caller takes `limit` rows and checks `len > limit - 1` to detect older pages.
    ///
    /// Include reasoning only for assistant rows that own tool calls: the frontend uses that
    /// text to rebuild the ReAct fold "Thoughts" preface after refresh. Keep non-tool
    /// reasoning out of the paged load path to avoid reintroducing large IPC payloads.
    pub async fn get_session_messages_paged(
        &self,
        session_id: &str,
        limit: i64,
        _offset: i64,
    ) -> Result<Vec<MessageRecord>, sqlx::Error> {
        sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, role, content, tool_calls, tool_call_id,
                   token_usage_json,
                   CASE
                       WHEN role = 'assistant'
                            AND reasoning_content IS NOT NULL
                            AND trim(reasoning_content) <> ''
                            AND tool_calls IS NOT NULL
                            AND trim(tool_calls) <> ''
                            AND trim(tool_calls) <> '[]'
                       THEN reasoning_content
                       ELSE NULL
                   END as reasoning_content,
                   follow_up_suggestions_json, turn_summary, created_at
            FROM messages
            WHERE session_id = ?
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    /// Return at most `limit` messages older than `before_id`, newest-first.
    /// The caller reverses the result to get chronological order.
    /// `reasoning_content` is included only for tool-call assistant rows, matching
    /// `get_session_messages_paged`.
    pub async fn get_messages_before(
        &self,
        session_id: &str,
        before_id: &str,
        limit: i64,
    ) -> Result<Vec<MessageRecord>, sqlx::Error> {
        sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, role, content, tool_calls, tool_call_id,
                   token_usage_json,
                   CASE
                       WHEN role = 'assistant'
                            AND reasoning_content IS NOT NULL
                            AND trim(reasoning_content) <> ''
                            AND tool_calls IS NOT NULL
                            AND trim(tool_calls) <> ''
                            AND trim(tool_calls) <> '[]'
                       THEN reasoning_content
                       ELSE NULL
                   END as reasoning_content,
                   follow_up_suggestions_json, turn_summary, created_at
            FROM messages
            WHERE session_id = ?
              AND (created_at, id) < (
                  SELECT created_at, id FROM messages WHERE id = ?
              )
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(before_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    /// Get a session by ID with all messages (legacy — used by save_session, etc.)
    pub async fn get_session(&self, id: &str) -> Result<Option<SessionWithMessages>, sqlx::Error> {
        // Get session metadata
        let session = sqlx::query_as::<_, SessionRecord>(
            "SELECT id, name, project_path, created_at, updated_at, active_provider_entry_name FROM sessions WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(session) = session else {
            return Ok(None);
        };

        let active_provider_entry_name = session.active_provider_entry_name.clone();

        // Get all messages for this session
        let messages = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, role, content, tool_calls, tool_call_id, token_usage_json, reasoning_content, follow_up_suggestions_json, turn_summary, created_at
            FROM messages
            WHERE session_id = ?
            ORDER BY created_at ASC, id ASC
            "#
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        Ok(Some(SessionWithMessages {
            id: session.id,
            name: session.name,
            project_path: session.project_path,
            created_at: session.created_at,
            updated_at: session.updated_at,
            active_provider_entry_name,
            messages,
        }))
    }

    /// Create a new session
    pub async fn create_session(
        &self,
        id: &str,
        name: &str,
        project_path: &str,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO sessions (id, name, project_path, created_at, updated_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(id)
        .bind(name)
        .bind(project_path)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update session timestamp
    pub async fn touch_session(&self, id: &str) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Delete a session
    pub async fn delete_session(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Rename a session
    pub async fn rename_session(&self, id: &str, name: &str) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query("UPDATE sessions SET name = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Update project (working) path for a session
    pub async fn update_session_project_path(
        &self,
        id: &str,
        project_path: &str,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query("UPDATE sessions SET project_path = ?, updated_at = ? WHERE id = ?")
            .bind(project_path)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Persist which `omiga.yaml` provider entry this session uses (quick-switch), or `null` to mean "yaml default".
    pub async fn set_session_active_provider(
        &self,
        session_id: &str,
        provider_entry_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "UPDATE sessions SET active_provider_entry_name = ?, updated_at = ? WHERE id = ?",
        )
        .bind(provider_entry_name)
        .bind(&now)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Save a message
    pub async fn save_message(&self, message: NewMessageRecord<'_>) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, token_usage_json, reasoning_content, follow_up_suggestions_json, turn_summary, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(message.id)
        .bind(message.session_id)
        .bind(message.role)
        .bind(message.content)
        .bind(message.tool_calls)
        .bind(message.tool_call_id)
        .bind(message.token_usage_json)
        .bind(message.reasoning_content)
        .bind(message.follow_up_suggestions_json)
        .bind(message.turn_summary)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Persist multiple tool-result rows in a **single transaction**.
    ///
    /// Replaces the previous pattern of N individual `save_message` calls (each an
    /// autocommit) with one `BEGIN` / N `INSERT` / `COMMIT`, reducing fsync overhead
    /// from O(N) to O(1) under `synchronous = NORMAL`.
    pub async fn save_tool_results_batch(
        &self,
        session_id: &str,
        results: &[(String, String, Option<String>)], // (tool_use_id, output, id_override)
    ) -> Result<(), sqlx::Error> {
        if results.is_empty() {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        for (tool_use_id, output, id_override) in results {
            let msg_id = id_override
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            sqlx::query(
                r#"
                INSERT INTO messages
                    (id, session_id, role, content, tool_calls, tool_call_id,
                     token_usage_json, reasoning_content, follow_up_suggestions_json, turn_summary, created_at)
                VALUES (?, ?, 'tool', ?, NULL, ?, NULL, NULL, NULL, NULL, ?)
                "#,
            )
            .bind(&msg_id)
            .bind(session_id)
            .bind(output)
            .bind(tool_use_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Update token usage on an existing assistant message row (after turn completes).
    pub async fn update_message_token_usage(
        &self,
        id: &str,
        token_usage_json: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE messages SET token_usage_json = ? WHERE id = ?")
            .bind(token_usage_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update follow-up suggestions on an existing assistant message row (after turn completes).
    pub async fn update_message_follow_up_suggestions(
        &self,
        id: &str,
        follow_up_suggestions_json: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE messages SET follow_up_suggestions_json = ? WHERE id = ?")
            .bind(follow_up_suggestions_json)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update turn summary on an existing assistant message row.
    pub async fn update_message_turn_summary(
        &self,
        id: &str,
        turn_summary: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE messages SET turn_summary = ? WHERE id = ?")
            .bind(turn_summary)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete all messages for a session (useful for clearing history)
    pub async fn clear_messages(&self, session_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Update plain-text `content` on an existing message row (e.g. retry with edited user text).
    pub async fn update_message_content(
        &self,
        message_id: &str,
        content: &str,
    ) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("UPDATE messages SET content = ? WHERE id = ?")
            .bind(content)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected())
    }

    /// Delete all messages strictly **after** `anchor_message_id` in transcript order, and any
    /// `conversation_rounds` rows that reference those message ids. Used when retrying from a user row.
    pub async fn delete_messages_after_anchor(
        &self,
        session_id: &str,
        anchor_message_id: &str,
    ) -> Result<(), sqlx::Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT id FROM messages WHERE session_id = ? ORDER BY created_at ASC, id ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        let ids: Vec<String> = rows.into_iter().map(|r| r.0).collect();
        let Some(pos) = ids.iter().position(|id| id == anchor_message_id) else {
            return Ok(());
        };
        let to_delete: Vec<String> = ids[(pos + 1)..].to_vec();
        if to_delete.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        for mid in &to_delete {
            sqlx::query(
                r#"DELETE FROM conversation_rounds WHERE session_id = ? AND (
                    message_id = ? OR user_message_id = ? OR assistant_message_id = ?
                )"#,
            )
            .bind(session_id)
            .bind(mid)
            .bind(mid)
            .bind(mid)
            .execute(&mut *tx)
            .await?;
        }
        for mid in &to_delete {
            sqlx::query("DELETE FROM messages WHERE id = ?")
                .bind(mid)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Save setting
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(value)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get setting
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let result: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.map(|r| r.0))
    }

    /// Delete setting
    pub async fn delete_setting(&self, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // === Conversation Round State Operations ===

    /// Create a new conversation round
    pub async fn create_round(
        &self,
        id: &str,
        session_id: &str,
        message_id: &str,
        user_message_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO conversation_rounds (id, session_id, message_id, status, user_message_id, created_at, updated_at)
            VALUES (?, ?, ?, 'running', ?, ?, ?)
            "#
        )
        .bind(id)
        .bind(session_id)
        .bind(message_id)
        .bind(user_message_id)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn append_runtime_constraint_event(
        &self,
        session_id: &str,
        round_id: &str,
        message_id: &str,
        event_type: &str,
        constraint_id: Option<&str>,
        payload_json: &str,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO runtime_constraint_events
                (id, session_id, round_id, message_id, event_type, constraint_id, payload_json, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(round_id)
        .bind(message_id)
        .bind(event_type)
        .bind(constraint_id)
        .bind(payload_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn append_orchestration_event(
        &self,
        event: NewOrchestrationEventRecord<'_>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO orchestration_events
                (id, session_id, round_id, message_id, mode, event_type, phase, task_id, payload_json, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(event.session_id)
        .bind(event.round_id)
        .bind(event.message_id)
        .bind(event.mode)
        .bind(event.event_type)
        .bind(event.phase)
        .bind(event.task_id)
        .bind(event.payload_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_orchestration_events_for_session(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<OrchestrationEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, OrchestrationEventRecord>(
            r#"
            SELECT id, session_id, round_id, message_id, mode, event_type, phase, task_id, payload_json, created_at
            FROM orchestration_events
            WHERE session_id = ?
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn list_runtime_constraint_events_for_round(
        &self,
        round_id: &str,
    ) -> Result<Vec<RuntimeConstraintEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, RuntimeConstraintEventRecord>(
            r#"
            SELECT id, session_id, round_id, message_id, event_type, constraint_id, payload_json, created_at
            FROM runtime_constraint_events
            WHERE round_id = ?
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(round_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn list_runtime_constraint_rounds_for_session(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<RuntimeConstraintRoundTraceRecord>, sqlx::Error> {
        sqlx::query_as::<_, RuntimeConstraintRoundTraceRecord>(
            r#"
            SELECT
                round_id,
                session_id,
                MIN(message_id) AS message_id,
                COUNT(*) AS event_count,
                MIN(created_at) AS first_event_at,
                MAX(created_at) AS last_event_at
            FROM runtime_constraint_events
            WHERE session_id = ?
            GROUP BY round_id, session_id
            ORDER BY last_event_at DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    /// Get round by message ID
    pub async fn get_round_by_message_id(
        &self,
        message_id: &str,
    ) -> Result<Option<ConversationRoundRecord>, sqlx::Error> {
        let round = sqlx::query_as::<_, ConversationRoundRecord>(
            r#"
            SELECT id, session_id, message_id, status, user_message_id, assistant_message_id,
                   cancelled_at, completed_at, error_message, created_at, updated_at
            FROM conversation_rounds
            WHERE message_id = ?
            "#,
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(round)
    }

    /// Get active (running/partial) rounds for a session
    pub async fn get_active_rounds(
        &self,
        session_id: &str,
    ) -> Result<Vec<ConversationRoundRecord>, sqlx::Error> {
        let rounds = sqlx::query_as::<_, ConversationRoundRecord>(
            r#"
            SELECT id, session_id, message_id, status, user_message_id, assistant_message_id,
                   cancelled_at, completed_at, error_message, created_at, updated_at
            FROM conversation_rounds
            WHERE session_id = ? AND status IN ('running', 'partial')
            ORDER BY created_at DESC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rounds)
    }

    /// Update round status to partial (received partial response)
    pub async fn mark_round_partial(
        &self,
        round_id: &str,
        assistant_message_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE conversation_rounds
            SET status = 'partial', assistant_message_id = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(assistant_message_id)
        .bind(&now)
        .bind(round_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update round status to cancelled
    pub async fn cancel_round(
        &self,
        round_id: &str,
        error_message: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let cancelled_at = now.clone();

        sqlx::query(
            r#"
            UPDATE conversation_rounds
            SET status = 'cancelled', cancelled_at = ?, error_message = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&cancelled_at)
        .bind(error_message)
        .bind(&now)
        .bind(round_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update round status to completed
    pub async fn complete_round(
        &self,
        round_id: &str,
        assistant_message_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let completed_at = now.clone();

        sqlx::query(
            r#"
            UPDATE conversation_rounds
            SET status = 'completed', assistant_message_id = ?, completed_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(assistant_message_id)
        .bind(&completed_at)
        .bind(&now)
        .bind(round_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Load persisted `todo_write` + V2 task list for a session (empty if missing).
    pub async fn get_session_tool_state(
        &self,
        session_id: &str,
    ) -> Result<(Vec<TodoItem>, Vec<AgentTask>), sqlx::Error> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT todos_json, agent_tasks_json FROM session_tool_state WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((todos_s, tasks_s)) = row else {
            return Ok((vec![], vec![]));
        };

        let todos: Vec<TodoItem> = serde_json::from_str(&todos_s).unwrap_or_default();
        let tasks: Vec<AgentTask> = serde_json::from_str(&tasks_s).unwrap_or_default();
        Ok((todos, tasks))
    }

    /// Load session working memory scratchpad JSON.
    pub async fn get_session_working_memory(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::domain::memory::working_memory::WorkingMemoryState>, sqlx::Error>
    {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT state_json FROM session_working_memory WHERE session_id = ?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.and_then(|(json,)| {
            serde_json::from_str::<crate::domain::memory::working_memory::WorkingMemoryState>(&json)
                .ok()
        }))
    }

    /// Upsert one background agent task row (authoritative store; memory cache may overlay).
    pub async fn upsert_background_agent_task(
        &self,
        task: &BackgroundAgentTask,
    ) -> Result<(), sqlx::Error> {
        let status = background_agent_status_db(&task.status);
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO background_agent_tasks (
                task_id, session_id, message_id, round_id, plan_id, agent_type, description, status,
                created_at, started_at, completed_at, result_summary, error_message, output_path, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(task_id) DO UPDATE SET
                session_id = excluded.session_id,
                message_id = excluded.message_id,
                round_id = excluded.round_id,
                plan_id = excluded.plan_id,
                agent_type = excluded.agent_type,
                description = excluded.description,
                status = excluded.status,
                started_at = excluded.started_at,
                completed_at = excluded.completed_at,
                result_summary = excluded.result_summary,
                error_message = excluded.error_message,
                output_path = excluded.output_path,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&task.task_id)
        .bind(&task.session_id)
        .bind(&task.message_id)
        .bind(task.round_id.as_deref())
        .bind(task.plan_id.as_deref())
        .bind(&task.agent_type)
        .bind(&task.description)
        .bind(status)
        .bind(task.created_at as i64)
        .bind(task.started_at.map(|u| u as i64))
        .bind(task.completed_at.map(|u| u as i64))
        .bind(task.result_summary.as_deref())
        .bind(task.error_message.as_deref())
        .bind(task.output_path.as_deref())
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// List background agent tasks for a session (newest first by `created_at`).
    pub async fn list_background_agent_tasks_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<BackgroundAgentTask>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BackgroundAgentTaskRow>(
            r#"
            SELECT task_id, session_id, message_id, round_id, plan_id, agent_type, description, status,
                   created_at, started_at, completed_at, result_summary, error_message, output_path
            FROM background_agent_tasks
            WHERE session_id = ?
            ORDER BY created_at DESC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_background_task).collect())
    }

    /// Single background agent task by id (for cancel / reconcile after restart).
    pub async fn get_background_agent_task_by_id(
        &self,
        task_id: &str,
    ) -> Result<Option<BackgroundAgentTask>, sqlx::Error> {
        let row = sqlx::query_as::<_, BackgroundAgentTaskRow>(
            r#"
            SELECT task_id, session_id, message_id, round_id, plan_id, agent_type, description, status,
                   created_at, started_at, completed_at, result_summary, error_message, output_path
            FROM background_agent_tasks
            WHERE task_id = ?
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(row_to_background_task))
    }

    /// Append one message to a background task sidechain transcript (ordered by `seq`).
    pub async fn append_background_agent_message(
        &self,
        task_id: &str,
        session_id: &str,
        message: &Message,
    ) -> Result<(), sqlx::Error> {
        let message = sanitize_background_sidechain_message(message);
        let payload_json = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string());
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let mut tx = self.pool.begin().await?;
        let max_seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), 0) FROM background_agent_messages WHERE task_id = ?",
        )
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
        let next_seq = max_seq + 1;

        sqlx::query(
            r#"
            INSERT INTO background_agent_messages (id, task_id, session_id, seq, payload_json, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(task_id)
        .bind(session_id)
        .bind(next_seq)
        .bind(&payload_json)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Append multiple messages to a background task sidechain transcript in one transaction.
    pub async fn append_background_agent_messages_batch(
        &self,
        task_id: &str,
        session_id: &str,
        messages: &[Message],
    ) -> Result<(), sqlx::Error> {
        if messages.is_empty() {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let base_seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), 0) FROM background_agent_messages WHERE task_id = ?",
        )
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;

        for (i, message) in messages.iter().enumerate() {
            let message = sanitize_background_sidechain_message(message);
            let payload_json = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string());
            let id = uuid::Uuid::new_v4().to_string();
            let seq = base_seq + 1 + i as i64;
            sqlx::query(
                r#"
                INSERT INTO background_agent_messages (id, task_id, session_id, seq, payload_json, created_at)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(task_id)
            .bind(session_id)
            .bind(seq)
            .bind(&payload_json)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Load sidechain transcript for one background task (chronological).
    pub async fn list_background_agent_messages_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<Message>, sqlx::Error> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT payload_json FROM background_agent_messages WHERE task_id = ? ORDER BY seq ASC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for payload_json in rows {
            match serde_json::from_str::<Message>(&payload_json) {
                Ok(m) => out.push(m),
                Err(e) => {
                    tracing::warn!(
                        target: "omiga::persistence",
                        "skip bad background_agent_messages row for {}: {}",
                        task_id,
                        e
                    );
                }
            }
        }
        Ok(out)
    }

    /// Persist session tool state (best-effort JSON).
    pub async fn upsert_session_tool_state(
        &self,
        session_id: &str,
        todos: &[TodoItem],
        agent_tasks: &[AgentTask],
    ) -> Result<(), sqlx::Error> {
        let todos_json = serde_json::to_string(todos).unwrap_or_else(|_| "[]".to_string());
        let agent_tasks_json =
            serde_json::to_string(agent_tasks).unwrap_or_else(|_| "[]".to_string());
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO session_tool_state (session_id, todos_json, agent_tasks_json, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(session_id) DO UPDATE SET
                todos_json = excluded.todos_json,
                agent_tasks_json = excluded.agent_tasks_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(session_id)
        .bind(&todos_json)
        .bind(&agent_tasks_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Persist session working memory scratchpad (best-effort JSON).
    pub async fn upsert_session_working_memory(
        &self,
        session_id: &str,
        state: &crate::domain::memory::working_memory::WorkingMemoryState,
    ) -> Result<(), sqlx::Error> {
        let state_json = serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string());
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO session_working_memory (session_id, state_json, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(session_id) DO UPDATE SET
                state_json = excluded.state_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(session_id)
        .bind(&state_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all rounds for a session (for history/replay)
    pub async fn get_session_rounds(
        &self,
        session_id: &str,
    ) -> Result<Vec<ConversationRoundRecord>, sqlx::Error> {
        let rounds = sqlx::query_as::<_, ConversationRoundRecord>(
            r#"
            SELECT id, session_id, message_id, status, user_message_id, assistant_message_id,
                   cancelled_at, completed_at, error_message, created_at, updated_at
            FROM conversation_rounds
            WHERE session_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rounds)
    }
}

/// Session database record
#[derive(Debug, sqlx::FromRow)]
pub struct SessionRecord {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub active_provider_entry_name: Option<String>,
}

/// Session with message count
#[derive(Debug, sqlx::FromRow)]
pub struct SessionWithCount {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i64,
}

/// Session search result with an optional message-body snippet.
#[derive(Debug, sqlx::FromRow)]
pub struct SessionSearchResult {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i64,
    pub match_snippet: Option<String>,
}

/// Message database record
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub token_usage_json: Option<String>,
    pub reasoning_content: Option<String>,
    pub follow_up_suggestions_json: Option<String>,
    pub turn_summary: Option<String>,
    pub created_at: String,
}

/// Borrowed message insert payload.
#[derive(Debug, Clone, Copy)]
pub struct NewMessageRecord<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub role: &'a str,
    pub content: &'a str,
    pub tool_calls: Option<&'a str>,
    pub tool_call_id: Option<&'a str>,
    pub token_usage_json: Option<&'a str>,
    pub reasoning_content: Option<&'a str>,
    pub follow_up_suggestions_json: Option<&'a str>,
    pub turn_summary: Option<&'a str>,
}

/// Session with all messages
#[derive(Debug, Clone)]
pub struct SessionWithMessages {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub created_at: String,
    pub updated_at: String,
    /// `omiga.yaml` provider map key; `None` means "use saved default_provider" for this session.
    pub active_provider_entry_name: Option<String>,
    pub messages: Vec<MessageRecord>,
}

/// Conversation round status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStatus {
    Running,
    Partial,
    Cancelled,
    Completed,
}

impl std::fmt::Display for RoundStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoundStatus::Running => write!(f, "running"),
            RoundStatus::Partial => write!(f, "partial"),
            RoundStatus::Cancelled => write!(f, "cancelled"),
            RoundStatus::Completed => write!(f, "completed"),
        }
    }
}

/// Conversation round database record
#[derive(Debug, sqlx::FromRow)]
pub struct ConversationRoundRecord {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub status: String,
    pub user_message_id: Option<String>,
    pub assistant_message_id: Option<String>,
    pub cancelled_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ConversationRoundRecord {
    /// Get round status as enum
    pub fn status_enum(&self) -> RoundStatus {
        match self.status.as_str() {
            "partial" => RoundStatus::Partial,
            "cancelled" => RoundStatus::Cancelled,
            "completed" => RoundStatus::Completed,
            _ => RoundStatus::Running,
        }
    }

    /// Check if round is active (running or partial)
    pub fn is_active(&self) -> bool {
        matches!(
            self.status_enum(),
            RoundStatus::Running | RoundStatus::Partial
        )
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RuntimeConstraintEventRecord {
    pub id: String,
    pub session_id: String,
    pub round_id: String,
    pub message_id: String,
    pub event_type: String,
    pub constraint_id: Option<String>,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RuntimeConstraintRoundTraceRecord {
    pub round_id: String,
    pub session_id: String,
    pub message_id: String,
    pub event_count: i64,
    pub first_event_at: String,
    pub last_event_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrchestrationEventRecord {
    pub id: String,
    pub session_id: String,
    pub round_id: Option<String>,
    pub message_id: Option<String>,
    pub mode: Option<String>,
    pub event_type: String,
    pub phase: Option<String>,
    pub task_id: Option<String>,
    pub payload_json: String,
    pub created_at: String,
}

/// Borrowed orchestration-event insert payload.
#[derive(Debug, Clone, Copy)]
pub struct NewOrchestrationEventRecord<'a> {
    pub session_id: &'a str,
    pub round_id: Option<&'a str>,
    pub message_id: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub event_type: &'a str,
    pub phase: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub payload_json: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persists_and_lists_orchestration_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        repo.create_session("session-1", "Scenario A", "/tmp/project")
            .await
            .expect("create session");

        repo.append_orchestration_event(NewOrchestrationEventRecord {
            session_id: "session-1",
            round_id: Some("round-1"),
            message_id: Some("message-1"),
            mode: Some("schedule"),
            event_type: "schedule_plan_created",
            phase: None,
            task_id: None,
            payload_json: r#"{"planId":"plan-1","taskCount":3}"#,
        })
        .await
        .expect("append event");

        repo.append_orchestration_event(NewOrchestrationEventRecord {
            session_id: "session-1",
            round_id: Some("round-1"),
            message_id: Some("message-1"),
            mode: Some("team"),
            event_type: "phase_changed",
            phase: Some("executing"),
            task_id: None,
            payload_json: r#"{"goal":"fix export flow"}"#,
        })
        .await
        .expect("append phase event");

        let events = repo
            .list_orchestration_events_for_session("session-1", 10)
            .await
            .expect("list events");

        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .any(|e| e.event_type == "schedule_plan_created"));
        assert!(events
            .iter()
            .any(|e| e.event_type == "phase_changed" && e.phase.as_deref() == Some("executing")));
    }

    #[tokio::test]
    async fn paged_messages_restore_tool_call_reasoning_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        repo.create_session("session-1", "Reasoning", "/tmp/project")
            .await
            .expect("create session");

        repo.save_message(NewMessageRecord {
            id: "assistant-tool",
            session_id: "session-1",
            role: "assistant",
            content: "",
            tool_calls: Some(r#"[{"id":"call-1","name":"bash","arguments":"{}"}]"#),
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: Some("I should inspect the CAS1-specific file first."),
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .expect("save tool assistant");

        repo.save_message(NewMessageRecord {
            id: "assistant-plain",
            session_id: "session-1",
            role: "assistant",
            content: "final answer",
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: Some("This non-tool reasoning should stay out of paged loads."),
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .expect("save plain assistant");

        let messages = repo
            .get_session_messages_paged("session-1", 10, 0)
            .await
            .expect("load paged messages");

        let tool_owner = messages
            .iter()
            .find(|msg| msg.id == "assistant-tool")
            .expect("tool assistant row");
        assert_eq!(
            tool_owner.reasoning_content.as_deref(),
            Some("I should inspect the CAS1-specific file first.")
        );

        let plain = messages
            .iter()
            .find(|msg| msg.id == "assistant-plain")
            .expect("plain assistant row");
        assert!(plain.reasoning_content.is_none());
    }

    #[tokio::test]
    async fn search_sessions_matches_message_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        repo.create_session("session-1", "Unrelated title", "/tmp/project-a")
            .await
            .expect("create session 1");
        repo.create_session("session-2", "QSDB title", "/tmp/project-b")
            .await
            .expect("create session 2");

        repo.save_message(NewMessageRecord {
            id: "message-1",
            session_id: "session-1",
            role: "user",
            content: "Please extract QS core gene sequence from the uploaded FASTA.",
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: None,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .expect("save matching message");

        repo.save_message(NewMessageRecord {
            id: "message-2",
            session_id: "session-2",
            role: "user",
            content: "No special body text here.",
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: None,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .expect("save nonmatching message");

        let body_rows = repo
            .search_sessions("core gene", 10)
            .await
            .expect("search body content");
        assert_eq!(body_rows.len(), 1);
        assert_eq!(body_rows[0].id, "session-1");
        assert!(body_rows[0]
            .match_snippet
            .as_deref()
            .expect("body match snippet")
            .contains("core gene"));

        let title_rows = repo
            .search_sessions("qsdb", 10)
            .await
            .expect("search title");
        assert_eq!(title_rows.len(), 1);
        assert_eq!(title_rows[0].id, "session-2");
    }

    #[tokio::test]
    async fn message_fts_index_tracks_message_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool.clone());

        repo.create_session("session-1", "FTS", "/tmp/project")
            .await
            .expect("create session");
        repo.save_message(NewMessageRecord {
            id: "message-1",
            session_id: "session-1",
            role: "user",
            content: "alpha omega marker",
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: None,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .expect("save message");

        let omega_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH ?")
                .bind("omega*")
                .fetch_one(&pool)
                .await
                .expect("query inserted fts row");
        assert_eq!(omega_count, 1);

        repo.update_message_content("message-1", "beta marker")
            .await
            .expect("update message content");

        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH ?")
                .bind("omega*")
                .fetch_one(&pool)
                .await
                .expect("query removed fts row");
        let beta_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH ?")
                .bind("beta*")
                .fetch_one(&pool)
                .await
                .expect("query updated fts row");
        assert_eq!(old_count, 0);
        assert_eq!(beta_count, 1);

        repo.clear_messages("session-1")
            .await
            .expect("delete messages");
        let deleted_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH ?")
                .bind("beta*")
                .fetch_one(&pool)
                .await
                .expect("query deleted fts row");
        assert_eq!(deleted_count, 0);
    }

    #[tokio::test]
    async fn session_fts_index_tracks_metadata_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool.clone());

        repo.create_session("session-1", "Alpha Signal", "/tmp/project")
            .await
            .expect("create session");

        let alpha_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH ?")
                .bind("alpha*")
                .fetch_one(&pool)
                .await
                .expect("query inserted session fts row");
        assert_eq!(alpha_count, 1);

        repo.rename_session("session-1", "Beta Signal")
            .await
            .expect("rename session");

        let old_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH ?")
                .bind("alpha*")
                .fetch_one(&pool)
                .await
                .expect("query removed session fts row");
        let beta_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH ?")
                .bind("beta*")
                .fetch_one(&pool)
                .await
                .expect("query renamed session fts row");
        assert_eq!(old_count, 0);
        assert_eq!(beta_count, 1);

        repo.update_session_project_path("session-1", "/tmp/gamma-project")
            .await
            .expect("update project path");

        let search_rows = repo
            .search_sessions("gamma", 10)
            .await
            .expect("search project path through fts");
        assert_eq!(search_rows.len(), 1);
        assert_eq!(search_rows[0].id, "session-1");

        repo.delete_session("session-1")
            .await
            .expect("delete session");
        let deleted_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH ?")
                .bind("beta*")
                .fetch_one(&pool)
                .await
                .expect("query deleted session fts row");
        assert_eq!(deleted_count, 0);
    }

    #[tokio::test]
    async fn trigram_fts_accelerates_literal_session_search() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool.clone());

        let has_trigram: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'table'
              AND name IN ('messages_trigram_fts', 'sessions_trigram_fts')
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("check trigram fts tables");

        // Older SQLite builds may not provide the FTS5 trigram tokenizer. Runtime code
        // intentionally falls back to scan in that case, so keep this test portable.
        if has_trigram < 2 {
            return;
        }

        repo.create_session("session-1", "English title", "/tmp/project")
            .await
            .expect("create session 1");
        repo.create_session("session-2", "Path title", "/Users/dengxsh/Downloads/Work")
            .await
            .expect("create session 2");

        repo.save_message(NewMessageRecord {
            id: "message-1",
            session_id: "session-1",
            role: "user",
            content: "实现聊天记录搜索功能，展示 session 内容片段。",
            tool_calls: None,
            tool_call_id: None,
            token_usage_json: None,
            reasoning_content: None,
            follow_up_suggestions_json: None,
            turn_summary: None,
        })
        .await
        .expect("save chinese message");

        let message_trigram_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages_trigram_fts WHERE messages_trigram_fts MATCH ?",
        )
        .bind("\"聊天记录\"")
        .fetch_one(&pool)
        .await
        .expect("query message trigram fts");
        assert_eq!(message_trigram_count, 1);

        let body_rows = repo
            .search_sessions("聊天记录", 10)
            .await
            .expect("search chinese message through trigram fts");
        assert_eq!(body_rows.len(), 1);
        assert_eq!(body_rows[0].id, "session-1");

        let session_trigram_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions_trigram_fts WHERE sessions_trigram_fts MATCH ?",
        )
        .bind("\"Downloads/Work\"")
        .fetch_one(&pool)
        .await
        .expect("query session trigram fts");
        assert_eq!(session_trigram_count, 1);

        let path_rows = repo
            .search_sessions("Downloads/Work", 10)
            .await
            .expect("search path through trigram fts");
        assert_eq!(path_rows.len(), 1);
        assert_eq!(path_rows[0].id, "session-2");
    }

    #[tokio::test]
    async fn session_search_preserves_substring_semantics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("omiga-test.db");
        let pool = init_db(&db_path).await.expect("init db");
        let repo = SessionRepository::new(pool);

        repo.create_session("session-1", "GitHub Connector", "/tmp/project")
            .await
            .expect("create session 1");
        repo.create_session("session-2", "OpenAI Tools", "/tmp/project")
            .await
            .expect("create session 2");

        let hub_rows = repo
            .search_sessions("Hub", 10)
            .await
            .expect("search ascii substring");
        assert_eq!(hub_rows.len(), 1);
        assert_eq!(hub_rows[0].id, "session-1");

        let short_rows = repo
            .search_sessions("AI", 10)
            .await
            .expect("search short ascii substring");
        assert_eq!(short_rows.len(), 1);
        assert_eq!(short_rows[0].id, "session-2");
    }
}
