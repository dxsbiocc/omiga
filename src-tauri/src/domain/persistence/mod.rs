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

    Ok(())
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
    /// `reasoning_content` (extended-thinking chain-of-thought) is excluded here — it can be
    /// several KB per assistant message and the frontend `RawMessage` interface does not use it
    /// on load.  Sending it over the Tauri WebView bridge was the primary cause of the
    /// ~200 ms session-switch IPC latency.  Use `get_message_reasoning` if you need it later.
    pub async fn get_session_messages_paged(
        &self,
        session_id: &str,
        limit: i64,
        _offset: i64,
    ) -> Result<Vec<MessageRecord>, sqlx::Error> {
        sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, role, content, tool_calls, tool_call_id,
                   token_usage_json, NULL as reasoning_content, follow_up_suggestions_json, turn_summary, created_at
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
    /// `reasoning_content` excluded for the same reason as `get_session_messages_paged`.
    pub async fn get_messages_before(
        &self,
        session_id: &str,
        before_id: &str,
        limit: i64,
    ) -> Result<Vec<MessageRecord>, sqlx::Error> {
        sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, role, content, tool_calls, tool_call_id,
                   token_usage_json, NULL as reasoning_content, follow_up_suggestions_json, turn_summary, created_at
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
}
