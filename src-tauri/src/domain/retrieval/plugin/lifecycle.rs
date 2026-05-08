use super::manifest::PluginRetrievalRuntime;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::time::Instant;

pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 60_000;
pub const DEFAULT_INITIALIZATION_TIMEOUT_MS: u64 = 60_000;
pub const DEFAULT_IDLE_TTL_MS: u64 = 120_000;
pub const DEFAULT_CANCEL_GRACE_MS: u64 = 500;
pub const DEFAULT_KILL_GRACE_MS: u64 = 200;
pub const DEFAULT_QUARANTINE_AFTER_FAILURES: u32 = 3;
pub const DEFAULT_QUARANTINE_BACKOFF_MS: u64 = 30_000;
pub const DEFAULT_MAX_QUARANTINE_MS: u64 = 5 * 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLifecyclePolicy {
    pub request_timeout: Duration,
    pub initialization_timeout: Duration,
    pub idle_ttl: Duration,
    pub cancel_grace: Duration,
    pub kill_grace: Duration,
    pub quarantine_after_failures: u32,
    pub quarantine_backoff: Duration,
    pub max_quarantine: Duration,
}

impl PluginLifecyclePolicy {
    pub fn from_runtime(runtime: &PluginRetrievalRuntime) -> Self {
        let request_timeout = duration_ms(runtime.request_timeout_ms, DEFAULT_REQUEST_TIMEOUT_MS);
        Self {
            request_timeout,
            initialization_timeout: request_timeout
                .max(Duration::from_millis(DEFAULT_INITIALIZATION_TIMEOUT_MS)),
            idle_ttl: duration_ms(runtime.idle_ttl_ms, DEFAULT_IDLE_TTL_MS),
            cancel_grace: duration_ms(runtime.cancel_grace_ms, DEFAULT_CANCEL_GRACE_MS),
            kill_grace: Duration::from_millis(DEFAULT_KILL_GRACE_MS),
            quarantine_after_failures: DEFAULT_QUARANTINE_AFTER_FAILURES,
            quarantine_backoff: Duration::from_millis(DEFAULT_QUARANTINE_BACKOFF_MS),
            max_quarantine: Duration::from_millis(DEFAULT_MAX_QUARANTINE_MS),
        }
    }
}

fn duration_ms(value: Option<u64>, default_ms: u64) -> Duration {
    Duration::from_millis(value.unwrap_or(default_ms).max(1))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginLifecycleKey {
    pub plugin_id: String,
    pub category: String,
    pub source_id: String,
}

impl PluginLifecycleKey {
    pub fn new(
        plugin_id: impl Into<String>,
        category: impl Into<String>,
        source_id: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            category: category.into(),
            source_id: source_id.into(),
        }
    }

    pub fn display(&self) -> String {
        format!(
            "{}.{} via {}",
            self.category, self.source_id, self.plugin_id
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginQuarantineStatus {
    pub consecutive_failures: u32,
    pub remaining: Duration,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginLifecycleRouteState {
    Healthy,
    Degraded,
    Quarantined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginLifecycleRouteStatus {
    pub plugin_id: String,
    pub category: String,
    pub source_id: String,
    pub route: String,
    pub state: PluginLifecycleRouteState,
    pub quarantined: bool,
    pub consecutive_failures: u32,
    pub remaining_ms: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginLifecycleState {
    inner: Arc<Mutex<HashMap<PluginLifecycleKey, PluginSourceLifecycle>>>,
}

#[derive(Debug, Clone)]
struct PluginSourceLifecycle {
    consecutive_failures: u32,
    quarantined_until: Option<Instant>,
    last_error: Option<String>,
}

impl PluginLifecycleState {
    pub fn global() -> Self {
        static GLOBAL: OnceLock<PluginLifecycleState> = OnceLock::new();
        GLOBAL.get_or_init(PluginLifecycleState::default).clone()
    }

    pub fn quarantine_status(
        &self,
        key: &PluginLifecycleKey,
        now: Instant,
    ) -> Option<PluginQuarantineStatus> {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        let state = guard.get_mut(key)?;
        match state.quarantined_until {
            Some(until) if until > now => Some(PluginQuarantineStatus {
                consecutive_failures: state.consecutive_failures,
                remaining: until.saturating_duration_since(now),
                last_error: state.last_error.clone(),
            }),
            Some(_) => {
                state.quarantined_until = None;
                None
            }
            None => None,
        }
    }

    pub fn record_success(&self, key: &PluginLifecycleKey) {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        guard.remove(key);
    }

    pub fn route_status(
        &self,
        key: &PluginLifecycleKey,
        now: Instant,
    ) -> PluginLifecycleRouteStatus {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        match guard.get_mut(key) {
            Some(state) => route_status_from_state(key, state, now),
            None => PluginLifecycleRouteStatus::healthy(key),
        }
    }

    pub fn route_statuses<I>(&self, keys: I, now: Instant) -> Vec<PluginLifecycleRouteStatus>
    where
        I: IntoIterator<Item = PluginLifecycleKey>,
    {
        let mut seen = HashSet::new();
        let mut statuses = keys
            .into_iter()
            .filter(|key| seen.insert(key.clone()))
            .map(|key| self.route_status(&key, now))
            .collect::<Vec<_>>();
        statuses.sort_by(|left, right| {
            (&left.plugin_id, &left.category, &left.source_id).cmp(&(
                &right.plugin_id,
                &right.category,
                &right.source_id,
            ))
        });
        statuses
    }

    pub fn record_failure(
        &self,
        key: PluginLifecycleKey,
        policy: &PluginLifecyclePolicy,
        now: Instant,
        error: impl Into<String>,
    ) -> Option<PluginQuarantineStatus> {
        let mut guard = self.inner.lock().unwrap_or_else(|err| err.into_inner());
        let state = guard.entry(key).or_insert_with(|| PluginSourceLifecycle {
            consecutive_failures: 0,
            quarantined_until: None,
            last_error: None,
        });
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_error = Some(compact_error(error.into()));

        if state.consecutive_failures < policy.quarantine_after_failures.max(1) {
            return None;
        }

        let duration = quarantine_duration(policy, state.consecutive_failures);
        let until = now + duration;
        state.quarantined_until = Some(until);
        Some(PluginQuarantineStatus {
            consecutive_failures: state.consecutive_failures,
            remaining: duration,
            last_error: state.last_error.clone(),
        })
    }
}

impl PluginLifecycleRouteStatus {
    fn healthy(key: &PluginLifecycleKey) -> Self {
        Self {
            plugin_id: key.plugin_id.clone(),
            category: key.category.clone(),
            source_id: key.source_id.clone(),
            route: key.display(),
            state: PluginLifecycleRouteState::Healthy,
            quarantined: false,
            consecutive_failures: 0,
            remaining_ms: 0,
            last_error: None,
        }
    }
}

fn route_status_from_state(
    key: &PluginLifecycleKey,
    source: &mut PluginSourceLifecycle,
    now: Instant,
) -> PluginLifecycleRouteStatus {
    let remaining = match source.quarantined_until {
        Some(until) if until > now => until.saturating_duration_since(now),
        Some(_) => {
            source.quarantined_until = None;
            Duration::ZERO
        }
        None => Duration::ZERO,
    };
    let quarantined = remaining > Duration::ZERO;
    let route_state = if quarantined {
        PluginLifecycleRouteState::Quarantined
    } else if source.consecutive_failures > 0 {
        PluginLifecycleRouteState::Degraded
    } else {
        PluginLifecycleRouteState::Healthy
    };
    PluginLifecycleRouteStatus {
        plugin_id: key.plugin_id.clone(),
        category: key.category.clone(),
        source_id: key.source_id.clone(),
        route: key.display(),
        state: route_state,
        quarantined,
        consecutive_failures: source.consecutive_failures,
        remaining_ms: duration_millis(remaining),
        last_error: source.last_error.clone(),
    }
}

fn quarantine_duration(policy: &PluginLifecyclePolicy, consecutive_failures: u32) -> Duration {
    let threshold = policy.quarantine_after_failures.max(1);
    let exponent = consecutive_failures.saturating_sub(threshold).min(8);
    policy
        .quarantine_backoff
        .saturating_mul(1_u32 << exponent)
        .min(policy.max_quarantine)
        .max(Duration::from_millis(1))
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn compact_error(error: String) -> String {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn runtime() -> PluginRetrievalRuntime {
        PluginRetrievalRuntime {
            command: PathBuf::from("/tmp/mock_plugin"),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: PathBuf::from("/tmp"),
            idle_ttl_ms: None,
            request_timeout_ms: None,
            cancel_grace_ms: None,
            concurrency: 1,
        }
    }

    #[test]
    fn default_policy_uses_bounded_lifecycle_durations() {
        let policy = PluginLifecyclePolicy::from_runtime(&runtime());

        assert_eq!(
            policy.request_timeout,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS)
        );
        assert_eq!(
            policy.initialization_timeout,
            Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS)
        );
        assert_eq!(policy.idle_ttl, Duration::from_millis(DEFAULT_IDLE_TTL_MS));
        assert_eq!(
            policy.cancel_grace,
            Duration::from_millis(DEFAULT_CANCEL_GRACE_MS)
        );
        assert_eq!(
            policy.kill_grace,
            Duration::from_millis(DEFAULT_KILL_GRACE_MS)
        );
        assert_eq!(
            policy.quarantine_after_failures,
            DEFAULT_QUARANTINE_AFTER_FAILURES
        );
        assert_eq!(
            policy.quarantine_backoff,
            Duration::from_millis(DEFAULT_QUARANTINE_BACKOFF_MS)
        );
    }

    #[test]
    fn manifest_runtime_overrides_policy_durations() {
        let mut runtime = runtime();
        runtime.request_timeout_ms = Some(2_000);
        runtime.idle_ttl_ms = Some(3_000);
        runtime.cancel_grace_ms = Some(750);

        let policy = PluginLifecyclePolicy::from_runtime(&runtime);

        assert_eq!(policy.request_timeout, Duration::from_millis(2_000));
        assert_eq!(
            policy.initialization_timeout,
            Duration::from_millis(DEFAULT_INITIALIZATION_TIMEOUT_MS)
        );
        assert_eq!(policy.idle_ttl, Duration::from_millis(3_000));
        assert_eq!(policy.cancel_grace, Duration::from_millis(750));
    }

    #[test]
    fn zero_manifest_durations_are_clamped() {
        let mut runtime = runtime();
        runtime.request_timeout_ms = Some(0);
        runtime.idle_ttl_ms = Some(0);
        runtime.cancel_grace_ms = Some(0);

        let policy = PluginLifecyclePolicy::from_runtime(&runtime);

        assert_eq!(policy.request_timeout, Duration::from_millis(1));
        assert_eq!(
            policy.initialization_timeout,
            Duration::from_millis(DEFAULT_INITIALIZATION_TIMEOUT_MS)
        );
        assert_eq!(policy.idle_ttl, Duration::from_millis(1));
        assert_eq!(policy.cancel_grace, Duration::from_millis(1));
    }

    #[test]
    fn failure_tracking_quarantines_after_threshold() {
        let state = PluginLifecycleState::default();
        let key = PluginLifecycleKey::new("plugin", "dataset", "mock");
        let mut policy = PluginLifecyclePolicy::from_runtime(&runtime());
        policy.quarantine_after_failures = 2;
        policy.quarantine_backoff = Duration::from_secs(10);
        let now = Instant::now();

        assert!(state
            .record_failure(key.clone(), &policy, now, "first failure")
            .is_none());
        let status = state
            .record_failure(key.clone(), &policy, now, "second failure")
            .unwrap();

        assert_eq!(status.consecutive_failures, 2);
        assert_eq!(status.remaining, Duration::from_secs(10));
        assert!(state.quarantine_status(&key, now).is_some());
        assert!(state
            .quarantine_status(&key, now + Duration::from_secs(11))
            .is_none());
    }

    #[test]
    fn success_clears_failure_state() {
        let state = PluginLifecycleState::default();
        let key = PluginLifecycleKey::new("plugin", "dataset", "mock");
        let mut policy = PluginLifecyclePolicy::from_runtime(&runtime());
        policy.quarantine_after_failures = 1;
        let now = Instant::now();

        assert!(state
            .record_failure(key.clone(), &policy, now, "failure")
            .is_some());
        state.record_success(&key);

        assert!(state.quarantine_status(&key, now).is_none());
        assert!(state
            .record_failure(key, &policy, now, "fresh failure")
            .is_some());
    }

    #[test]
    fn route_statuses_report_healthy_degraded_and_quarantined_routes() {
        let state = PluginLifecycleState::default();
        let healthy = PluginLifecycleKey::new("plugin", "dataset", "healthy");
        let degraded = PluginLifecycleKey::new("plugin", "dataset", "degraded");
        let quarantined = PluginLifecycleKey::new("plugin", "dataset", "quarantined");
        let mut policy = PluginLifecyclePolicy::from_runtime(&runtime());
        policy.quarantine_after_failures = 2;
        policy.quarantine_backoff = Duration::from_secs(10);
        let now = Instant::now();

        state.record_failure(degraded.clone(), &policy, now, "transient failure");
        state.record_failure(quarantined.clone(), &policy, now, "first failure");
        state.record_failure(quarantined.clone(), &policy, now, "second failure");

        let statuses = state.route_statuses(
            vec![healthy.clone(), degraded.clone(), quarantined.clone()],
            now,
        );

        let healthy_status = statuses
            .iter()
            .find(|status| status.source_id == "healthy")
            .unwrap();
        assert_eq!(healthy_status.state, PluginLifecycleRouteState::Healthy);
        assert_eq!(healthy_status.consecutive_failures, 0);

        let degraded_status = statuses
            .iter()
            .find(|status| status.source_id == "degraded")
            .unwrap();
        assert_eq!(degraded_status.state, PluginLifecycleRouteState::Degraded);
        assert_eq!(degraded_status.consecutive_failures, 1);
        assert_eq!(
            degraded_status.last_error.as_deref(),
            Some("transient failure")
        );

        let quarantine_status = statuses
            .iter()
            .find(|status| status.source_id == "quarantined")
            .unwrap();
        assert_eq!(
            quarantine_status.state,
            PluginLifecycleRouteState::Quarantined
        );
        assert!(quarantine_status.quarantined);
        assert_eq!(quarantine_status.consecutive_failures, 2);
        assert_eq!(quarantine_status.remaining_ms, 10_000);
        assert_eq!(
            quarantine_status.last_error.as_deref(),
            Some("second failure")
        );
    }
}
