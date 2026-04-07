//! Session persistence layer
//!
//! Uses SQLite for storing:
//! - Session metadata
//! - Message history
//! - Settings/preferences
//! - Session-scoped tool state (`todo_write`, V2 tasks)

use crate::domain::session::{AgentTask, TodoItem};
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::path::Path;

/// Initialize the database
pub async fn init_db(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(options).await?;

    // Run migrations
    run_migrations(&pool).await?;

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

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id)
        "#,
    )
    .execute(pool)
    .await?;

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
    .unwrap_or(0) > 0;

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
        let _ = sqlx::query("DROP TABLE conversation_rounds_old").execute(pool).await;

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

    Ok(())
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
        let sessions = sqlx::query_as::<_, SessionWithCount>(
            r#"
            SELECT
                s.id,
                s.name,
                s.project_path,
                s.created_at,
                s.updated_at,
                COUNT(m.id) as message_count
            FROM sessions s
            LEFT JOIN messages m ON s.id = m.session_id
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(sessions)
    }

    /// Get a session by ID with all messages
    pub async fn get_session(&self, id: &str) -> Result<Option<SessionWithMessages>, sqlx::Error> {
        // Get session metadata
        let session = sqlx::query_as::<_, SessionRecord>(
            "SELECT id, name, project_path, created_at, updated_at FROM sessions WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(session) = session else {
            return Ok(None);
        };

        // Get all messages for this session
        let messages = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, session_id, role, content, tool_calls, tool_call_id, created_at
            FROM messages
            WHERE session_id = ?
            ORDER BY created_at ASC
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

    /// Save a message
    pub async fn save_message(
        &self,
        id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        tool_calls: Option<&str>,
        tool_call_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO messages (id, session_id, role, content, tool_calls, tool_call_id, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(tool_calls)
        .bind(tool_call_id)
        .bind(&now)
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
            "#
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
            "#
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
            "#
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
            "#
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
            "#
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
            "#
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
            "#
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
#[derive(Debug, sqlx::FromRow)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub created_at: String,
}

/// Session with all messages
#[derive(Debug)]
pub struct SessionWithMessages {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub created_at: String,
    pub updated_at: String,
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
        matches!(self.status_enum(), RoundStatus::Running | RoundStatus::Partial)
    }
}
