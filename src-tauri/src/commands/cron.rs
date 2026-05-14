//! Tauri commands for managing cron jobs from the frontend.

use crate::app_state::OmigaAppState;
use crate::commands::CommandResult;
use crate::errors::AppError;
use serde::Serialize;
use sqlx::FromRow;
use std::str::FromStr;
use tauri::State;

/// Internal row type for sqlx deserialization.
#[derive(Debug, FromRow)]
struct CronJobRow {
    id: String,
    schedule: String,
    task_description: String,
    session_id: Option<String>,
    created_at: String,
}

/// Summary of a cron job returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobSummary {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub session_id: Option<String>,
    pub created_at: String,
}

impl From<CronJobRow> for CronJobSummary {
    fn from(r: CronJobRow) -> Self {
        CronJobSummary {
            id: r.id,
            schedule: r.schedule,
            task: r.task_description,
            session_id: r.session_id,
            created_at: r.created_at,
        }
    }
}

fn db_err(e: sqlx::Error) -> AppError {
    AppError::Persistence(e.to_string())
}

/// List all enabled cron jobs, newest first.
#[tauri::command]
pub async fn list_cron_jobs(
    state: State<'_, OmigaAppState>,
) -> CommandResult<Vec<CronJobSummary>> {
    let pool = state.repo.pool();
    let rows = sqlx::query_as::<_, CronJobRow>(
        r#"
        SELECT id, schedule, task_description, session_id, created_at
        FROM cron_jobs
        WHERE enabled = 1
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    Ok(rows.into_iter().map(CronJobSummary::from).collect())
}

/// Create a new enabled cron job. Returns the created job summary.
#[tauri::command]
pub async fn create_cron_job(
    state: State<'_, OmigaAppState>,
    schedule: String,
    task: String,
) -> CommandResult<CronJobSummary> {
    if schedule.trim().is_empty() {
        return Err(AppError::Unknown("schedule must not be empty".into()));
    }
    if task.trim().is_empty() {
        return Err(AppError::Unknown("task must not be empty".into()));
    }
    cron::Schedule::from_str(&schedule)
        .map_err(|e| AppError::Unknown(format!("invalid cron expression: {e}")))?;

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let pool = state.repo.pool();
    sqlx::query(
        "INSERT INTO cron_jobs (id, schedule, task_description, enabled, created_at) VALUES (?, ?, ?, 1, ?)",
    )
    .bind(&id)
    .bind(&schedule)
    .bind(&task)
    .bind(&created_at)
    .execute(pool)
    .await
    .map_err(db_err)?;

    Ok(CronJobSummary {
        id,
        schedule,
        task,
        session_id: None,
        created_at,
    })
}

/// Soft-delete a cron job by id. Returns true if a row was updated.
#[tauri::command]
pub async fn delete_cron_job(
    state: State<'_, OmigaAppState>,
    id: String,
) -> CommandResult<bool> {
    let pool = state.repo.pool();
    let result = sqlx::query(
        "UPDATE cron_jobs SET enabled = 0 WHERE id = ? AND enabled = 1",
    )
    .bind(&id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(result.rows_affected() > 0)
}
