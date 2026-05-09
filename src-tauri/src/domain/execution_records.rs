//! Project-scoped execution records for Operator / Template runs.
//!
//! This is intentionally a small persistence substrate for later
//! crystallization/optimization work. Recording must never make a successful
//! runtime fail, so production write points should call the best-effort helper.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

const EXECUTION_DB_RELATIVE_PATH: &str = ".omiga/execution/executions.sqlite";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecordInput {
    pub kind: String,
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub canonical_id: Option<String>,
    #[serde(default)]
    pub provider_plugin: Option<String>,
    pub status: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub parent_execution_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub input_hash: Option<String>,
    #[serde(default)]
    pub param_hash: Option<String>,
    #[serde(default)]
    pub output_summary_json: Option<JsonValue>,
    #[serde(default)]
    pub runtime_json: Option<JsonValue>,
    #[serde(default)]
    pub metadata_json: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub id: String,
    pub kind: String,
    pub unit_id: Option<String>,
    pub canonical_id: Option<String>,
    pub provider_plugin: Option<String>,
    pub status: String,
    pub session_id: Option<String>,
    pub parent_execution_id: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub input_hash: Option<String>,
    pub param_hash: Option<String>,
    pub output_summary_json: Option<String>,
    pub runtime_json: Option<String>,
    pub metadata_json: Option<String>,
}

pub fn execution_db_path(project_root: &Path) -> PathBuf {
    project_root.join(EXECUTION_DB_RELATIVE_PATH)
}

pub async fn record_execution(
    project_root: &Path,
    record: ExecutionRecordInput,
) -> Result<String, String> {
    record_execution_with_id(project_root, new_execution_record_id(), record).await
}

pub async fn record_execution_with_id(
    project_root: &Path,
    id: String,
    record: ExecutionRecordInput,
) -> Result<String, String> {
    let pool = open_execution_db(project_root)
        .await
        .map_err(|err| format!("open execution db: {err}"))?;
    insert_execution_record(&pool, id, record).await
}

pub async fn record_execution_best_effort(project_root: &Path, record: ExecutionRecordInput) {
    if let Err(err) = record_execution(project_root, record).await {
        tracing::warn!("execution record write failed: {err}");
    }
}

pub async fn update_execution_record(
    project_root: &Path,
    id: &str,
    record: ExecutionRecordInput,
) -> Result<(), String> {
    let pool = open_execution_db(project_root)
        .await
        .map_err(|err| format!("open execution db: {err}"))?;
    update_execution_record_row(&pool, id, record).await
}

pub async fn update_execution_record_best_effort(
    project_root: &Path,
    id: &str,
    record: ExecutionRecordInput,
) {
    if let Err(err) = update_execution_record(project_root, id, record).await {
        tracing::warn!("execution record update failed: {err}");
    }
}

pub async fn list_recent_execution_records(
    project_root: &Path,
    limit: usize,
) -> Result<Vec<ExecutionRecord>, String> {
    if !execution_db_path(project_root).is_file() {
        return Ok(Vec::new());
    }
    let pool = open_execution_db(project_root)
        .await
        .map_err(|err| format!("open execution db: {err}"))?;
    let limit = i64::try_from(limit.clamp(1, 200)).unwrap_or(50);
    sqlx::query_as::<_, ExecutionRecord>(
        r#"
        SELECT
          id, kind, unit_id, canonical_id, provider_plugin, status, session_id,
          parent_execution_id, started_at, ended_at, input_hash, param_hash,
          output_summary_json, runtime_json, metadata_json
        FROM executions
        ORDER BY COALESCE(ended_at, started_at, id) DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(&pool)
    .await
    .map_err(|err| format!("list execution records: {err}"))
}

pub async fn list_child_execution_records(
    project_root: &Path,
    parent_execution_id: &str,
    limit: usize,
) -> Result<Vec<ExecutionRecord>, String> {
    if !execution_db_path(project_root).is_file() {
        return Ok(Vec::new());
    }
    let pool = open_execution_db(project_root)
        .await
        .map_err(|err| format!("open execution db: {err}"))?;
    let limit = i64::try_from(limit.clamp(1, 200)).unwrap_or(50);
    sqlx::query_as::<_, ExecutionRecord>(
        r#"
        SELECT
          id, kind, unit_id, canonical_id, provider_plugin, status, session_id,
          parent_execution_id, started_at, ended_at, input_hash, param_hash,
          output_summary_json, runtime_json, metadata_json
        FROM executions
        WHERE parent_execution_id = ?
        ORDER BY COALESCE(ended_at, started_at, id) DESC
        LIMIT ?
        "#,
    )
    .bind(parent_execution_id)
    .bind(limit)
    .fetch_all(&pool)
    .await
    .map_err(|err| format!("list child execution records: {err}"))
}

pub fn hash_json(value: &JsonValue) -> Option<String> {
    let canonical = serde_json::to_vec(value).ok()?;
    Some(hash_bytes(&canonical))
}

pub fn hash_execution_map<T: Serialize>(value: &T) -> Option<String> {
    let json = serde_json::to_value(value).ok()?;
    hash_json(&json)
}

fn hash_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn new_execution_record_id() -> String {
    format!("execrec_{}", uuid::Uuid::new_v4().simple())
}

async fn open_execution_db(project_root: &Path) -> Result<SqlitePool, sqlx::Error> {
    let db_path = execution_db_path(project_root);
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(sqlx::Error::Io)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .pragma("busy_timeout", "5000")
        .pragma("foreign_keys", "ON");

    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await?;
    ensure_schema(&pool).await?;
    Ok(pool)
}

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS executions (
          id TEXT PRIMARY KEY,
          kind TEXT NOT NULL,
          unit_id TEXT,
          canonical_id TEXT,
          provider_plugin TEXT,
          status TEXT NOT NULL,
          session_id TEXT,
          parent_execution_id TEXT,
          started_at TEXT,
          ended_at TEXT,
          input_hash TEXT,
          param_hash TEXT,
          output_summary_json TEXT,
          runtime_json TEXT,
          metadata_json TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_executions_recent
        ON executions(COALESCE(ended_at, started_at, id) DESC)
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_executions_unit
        ON executions(kind, canonical_id, status)
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_execution_record(
    pool: &SqlitePool,
    id: String,
    record: ExecutionRecordInput,
) -> Result<String, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let started_at = record.started_at.unwrap_or_else(|| now.clone());
    let ended_at = record.ended_at;
    let output_summary_json = optional_json_string(record.output_summary_json)?;
    let runtime_json = optional_json_string(record.runtime_json)?;
    let metadata_json = optional_json_string(record.metadata_json)?;

    sqlx::query(
        r#"
        INSERT INTO executions (
          id, kind, unit_id, canonical_id, provider_plugin, status, session_id,
          parent_execution_id, started_at, ended_at, input_hash, param_hash,
          output_summary_json, runtime_json, metadata_json
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(record.kind)
    .bind(record.unit_id)
    .bind(record.canonical_id)
    .bind(record.provider_plugin)
    .bind(record.status)
    .bind(record.session_id)
    .bind(record.parent_execution_id)
    .bind(started_at)
    .bind(ended_at)
    .bind(record.input_hash)
    .bind(record.param_hash)
    .bind(output_summary_json)
    .bind(runtime_json)
    .bind(metadata_json)
    .execute(pool)
    .await
    .map_err(|err| format!("insert execution record: {err}"))?;

    Ok(id)
}

async fn update_execution_record_row(
    pool: &SqlitePool,
    id: &str,
    record: ExecutionRecordInput,
) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let started_at = record.started_at.unwrap_or_else(|| now.clone());
    let ended_at = record.ended_at;
    let output_summary_json = optional_json_string(record.output_summary_json)?;
    let runtime_json = optional_json_string(record.runtime_json)?;
    let metadata_json = optional_json_string(record.metadata_json)?;

    let result = sqlx::query(
        r#"
        UPDATE executions
        SET
          kind = ?,
          unit_id = ?,
          canonical_id = ?,
          provider_plugin = ?,
          status = ?,
          session_id = ?,
          parent_execution_id = ?,
          started_at = ?,
          ended_at = ?,
          input_hash = ?,
          param_hash = ?,
          output_summary_json = ?,
          runtime_json = ?,
          metadata_json = ?
        WHERE id = ?
        "#,
    )
    .bind(record.kind)
    .bind(record.unit_id)
    .bind(record.canonical_id)
    .bind(record.provider_plugin)
    .bind(record.status)
    .bind(record.session_id)
    .bind(record.parent_execution_id)
    .bind(started_at)
    .bind(ended_at)
    .bind(record.input_hash)
    .bind(record.param_hash)
    .bind(output_summary_json)
    .bind(runtime_json)
    .bind(metadata_json)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|err| format!("update execution record: {err}"))?;

    if result.rows_affected() == 0 {
        return Err(format!("execution record `{id}` does not exist"));
    }
    Ok(())
}

fn optional_json_string(value: Option<JsonValue>) -> Result<Option<String>, String> {
    value
        .map(|value| serde_json::to_string(&value).map_err(|err| err.to_string()))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn execution_record_round_trip_uses_project_scoped_sqlite() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("write_text_report".to_string()),
                canonical_id: Some(
                    "operator-smoke@omiga-curated/operator/write_text_report".to_string(),
                ),
                provider_plugin: Some("operator-smoke@omiga-curated".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: Some("2026-05-09T00:00:01Z".to_string()),
                input_hash: Some("sha256:input".to_string()),
                param_hash: Some("sha256:param".to_string()),
                output_summary_json: Some(json!({"outputCount": 1})),
                runtime_json: Some(json!({"surface": "local"})),
                metadata_json: Some(json!({"runId": "oprun_test"})),
            },
        )
        .await
        .expect("record");

        assert!(execution_db_path(tmp.path()).is_file());
        let rows = list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(
            rows[0].canonical_id.as_deref(),
            Some("operator-smoke@omiga-curated/operator/write_text_report")
        );
        assert_eq!(rows[0].status, "succeeded");
        assert!(rows[0]
            .output_summary_json
            .as_deref()
            .unwrap_or_default()
            .contains("outputCount"));
    }

    #[tokio::test]
    async fn execution_record_parent_child_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let parent_id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("template_a".to_string()),
                canonical_id: Some("provider/template/template_a".to_string()),
                provider_plugin: Some("provider".to_string()),
                status: "running".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: None,
                input_hash: None,
                param_hash: None,
                output_summary_json: Some(json!({"status": "running"})),
                runtime_json: None,
                metadata_json: None,
            },
        )
        .await
        .expect("parent");
        let before_update = list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("before update");
        assert_eq!(before_update[0].status, "running");
        assert!(before_update[0].ended_at.is_none());
        let child_id = record_execution(
            tmp.path(),
            ExecutionRecordInput {
                kind: "operator".to_string(),
                unit_id: Some("operator_a".to_string()),
                canonical_id: Some("provider/operator/operator_a".to_string()),
                provider_plugin: Some("provider".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: Some(parent_id.clone()),
                started_at: Some("2026-05-09T00:00:01Z".to_string()),
                ended_at: Some("2026-05-09T00:00:02Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: Some(json!({"status": "succeeded"})),
                runtime_json: None,
                metadata_json: None,
            },
        )
        .await
        .expect("child");

        update_execution_record(
            tmp.path(),
            &parent_id,
            ExecutionRecordInput {
                kind: "template".to_string(),
                unit_id: Some("template_a".to_string()),
                canonical_id: Some("provider/template/template_a".to_string()),
                provider_plugin: Some("provider".to_string()),
                status: "succeeded".to_string(),
                session_id: Some("session-1".to_string()),
                parent_execution_id: None,
                started_at: Some("2026-05-09T00:00:00Z".to_string()),
                ended_at: Some("2026-05-09T00:00:03Z".to_string()),
                input_hash: None,
                param_hash: None,
                output_summary_json: Some(json!({"status": "succeeded"})),
                runtime_json: None,
                metadata_json: Some(json!({"child": child_id})),
            },
        )
        .await
        .expect("update parent");

        let children = list_child_execution_records(tmp.path(), &parent_id, 10)
            .await
            .expect("children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child_id);
        assert_eq!(
            children[0].parent_execution_id.as_deref(),
            Some(parent_id.as_str())
        );

        let records = list_recent_execution_records(tmp.path(), 10)
            .await
            .expect("records");
        let parent = records
            .iter()
            .find(|record| record.id == parent_id)
            .expect("updated parent");
        assert_eq!(parent.status, "succeeded");
        assert!(parent
            .metadata_json
            .as_deref()
            .unwrap_or_default()
            .contains(&child_id));
    }

    #[test]
    fn hash_json_is_stable_for_identical_values() {
        let left = hash_json(&json!({"a": 1, "b": ["x"]})).expect("left");
        let right = hash_json(&json!({"a": 1, "b": ["x"]})).expect("right");
        assert_eq!(left, right);
        assert!(left.starts_with("sha256:"));
    }
}
