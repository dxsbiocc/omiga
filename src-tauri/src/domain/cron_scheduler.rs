//! Cron job scheduling engine.
//!
//! Polls the `cron_jobs` table every 30 seconds and fires a Tauri event
//! `cron-job-fired` for each job whose cron expression matches the current
//! minute. The frontend listens for this event and shows a notification.

use chrono::{Datelike, Timelike, Utc};
use sqlx::SqlitePool;
use tauri::Emitter;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobFiredPayload {
    pub id: String,
    pub task: String,
    pub fired_at: String,
}

/// Start the background scheduling loop. Non-blocking — spawns a tokio task.
pub fn start_cron_scheduler(pool: SqlitePool, app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(e) = tick(&pool, &app_handle).await {
                tracing::warn!(target: "omiga::cron", "scheduler tick error: {e}");
            }
        }
    });
}

async fn tick(pool: &SqlitePool, app: &tauri::AppHandle) -> Result<(), sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, schedule, task_description FROM cron_jobs WHERE enabled = 1",
    )
    .fetch_all(pool)
    .await?;

    let now = Utc::now();

    for (id, schedule, task) in rows {
        if matches_now(&schedule, &now) {
            let payload = CronJobFiredPayload {
                id: id.clone(),
                task: task.clone(),
                fired_at: now.to_rfc3339(),
            };
            if let Err(e) = app.emit("cron-job-fired", &payload) {
                tracing::warn!(target: "omiga::cron", "emit failed for {id}: {e}");
            }
            tracing::info!(target: "omiga::cron", "fired job id={id} task={task}");
        }
    }
    Ok(())
}

/// Check whether a 5-field cron expression matches the current UTC minute.
/// Supports: `*`, literal numbers, `*/N` (step), `N-M` (range), `N,M,...` (list).
fn matches_now(schedule: &str, now: &chrono::DateTime<Utc>) -> bool {
    let parts: Vec<&str> = schedule.split_whitespace().collect();
    if parts.len() < 5 {
        return false;
    }
    // Fields: minute hour day-of-month month day-of-week
    let values = [
        now.minute(),
        now.hour(),
        now.day(),
        now.month(),
        now.weekday().num_days_from_sunday(),
    ];
    parts[..5]
        .iter()
        .zip(values.iter())
        .all(|(expr, &val)| field_matches(expr, val))
}

fn field_matches(expr: &str, value: u32) -> bool {
    if expr == "*" {
        return true;
    }
    // step: */N
    if let Some(step_str) = expr.strip_prefix("*/") {
        return step_str
            .parse::<u32>()
            .ok()
            .filter(|&s| s > 0)
            .map(|s| value % s == 0)
            .unwrap_or(false);
    }
    // list: N,M,...
    if expr.contains(',') {
        return expr.split(',').any(|part| {
            part.trim().parse::<u32>().ok() == Some(value) || range_matches(part.trim(), value)
        });
    }
    // range: N-M
    if range_matches(expr, value) {
        return true;
    }
    // literal
    expr.parse::<u32>().ok() == Some(value)
}

fn range_matches(expr: &str, value: u32) -> bool {
    if let Some((s, e)) = expr.split_once('-') {
        if let (Ok(start), Ok(end)) = (s.parse::<u32>(), e.parse::<u32>()) {
            return value >= start && value <= end;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_any() {
        assert!(field_matches("*", 0));
        assert!(field_matches("*", 59));
    }

    #[test]
    fn literal_matches_exact() {
        assert!(field_matches("9", 9));
        assert!(!field_matches("9", 10));
    }

    #[test]
    fn step_matches() {
        assert!(field_matches("*/15", 0));
        assert!(field_matches("*/15", 15));
        assert!(field_matches("*/15", 30));
        assert!(field_matches("*/15", 45));
        assert!(!field_matches("*/15", 7));
    }

    #[test]
    fn range_matches_bounds() {
        assert!(field_matches("9-17", 9));
        assert!(field_matches("9-17", 17));
        assert!(field_matches("9-17", 12));
        assert!(!field_matches("9-17", 8));
        assert!(!field_matches("9-17", 18));
    }

    #[test]
    fn list_matches() {
        assert!(field_matches("1,3,5", 1));
        assert!(field_matches("1,3,5", 5));
        assert!(!field_matches("1,3,5", 2));
    }

    #[test]
    fn cron_daily_8am() {
        // "0 8 * * *" — every day at 08:00
        use chrono::TimeZone;
        let t = Utc.with_ymd_and_hms(2026, 5, 16, 8, 0, 0).unwrap();
        assert!(matches_now("0 8 * * *", &t));
        let t2 = Utc.with_ymd_and_hms(2026, 5, 16, 8, 1, 0).unwrap();
        assert!(!matches_now("0 8 * * *", &t2));
    }

    #[test]
    fn cron_weekday() {
        // "0 9 * * 1" — every Monday at 09:00 (1 = Monday in Sun-indexed)
        use chrono::TimeZone;
        // 2026-05-18 is a Monday
        let mon = Utc.with_ymd_and_hms(2026, 5, 18, 9, 0, 0).unwrap();
        assert!(matches_now("0 9 * * 1", &mon));
        // 2026-05-19 is a Tuesday
        let tue = Utc.with_ymd_and_hms(2026, 5, 19, 9, 0, 0).unwrap();
        assert!(!matches_now("0 9 * * 1", &tue));
    }
}
