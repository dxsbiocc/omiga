//! 重放解析 + health 反馈 + 探索阀门(Wave 2 / Codex D 实现)。
//!
//! 任务规格见 orchestrator 下发的提示与 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 第 11 节。
//! 需实现:`resolve_for_replay`(返回 `types::ReplayResolution`)、`record_replay_outcome`
//! (health 回写 + 成功率阈值 auto-demote)、`should_explore`(探索阀门),及对应单元测试。
//! `ReplayResolution` 枚举与阈值常量已在 `types.rs` 定义,直接使用。

use super::types::{
    Fingerprint, Health, Playbook, PlaybookStatus, PlaybookStore, ReplayResolution,
    DEMOTE_MIN_ATTEMPTS, DEMOTE_SUCCESS_RATE,
};

pub fn resolve_for_replay(
    store: &dyn PlaybookStore,
    playbook_id: &str,
    current_fingerprint: &Fingerprint,
) -> ReplayResolution {
    let Some(playbook) = store.get(playbook_id) else {
        return ReplayResolution::NotFound;
    };

    if playbook.health.status != PlaybookStatus::Active {
        return ReplayResolution::Inactive;
    }

    if playbook.fingerprint.matches(current_fingerprint) {
        ReplayResolution::Ready(Box::new(playbook))
    } else {
        ReplayResolution::Invalidated
    }
}

pub fn record_replay_outcome(
    store: &mut dyn PlaybookStore,
    playbook_id: &str,
    verified: bool,
    now: &str,
) -> Result<PlaybookStatus, String> {
    let Some(playbook) = store.get(playbook_id) else {
        return Err(format!("playbook not found: {}", playbook_id));
    };

    let previous_health = playbook.health.clone();
    let hit_count = previous_health.hit_count + 1;
    let success_count = previous_health.success_count + u64::from(verified);
    let last_verified_at = if verified {
        Some(now.to_string())
    } else {
        previous_health.last_verified_at.clone()
    };
    let success_rate = success_count as f64 / hit_count as f64;
    let status = if previous_health.status == PlaybookStatus::Quarantined
        || (hit_count >= DEMOTE_MIN_ATTEMPTS && success_rate < DEMOTE_SUCCESS_RATE)
    {
        PlaybookStatus::Quarantined
    } else {
        previous_health.status
    };

    let updated = Playbook {
        health: Health {
            hit_count,
            success_count,
            last_verified_at,
            status,
        },
        ..playbook
    };

    store.save(updated)?;
    Ok(status)
}

pub fn should_explore(playbook: &Playbook, epsilon: f64, roll: f64) -> bool {
    playbook.health.status == PlaybookStatus::Stale || roll < epsilon
}

#[cfg(test)]
mod tests {
    use super::super::store::JsonFilePlaybookStore;
    use super::super::types::{
        Fingerprint, Health, Playbook, PlaybookStatus, PlaybookStore, PlaybookVerification,
        Provenance, ReplayResolution,
    };
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("playbook-replay-test-{}", Uuid::new_v4()));
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn test_fingerprint(operator_version: &str) -> Fingerprint {
        Fingerprint {
            canonical_id: "template/demo".to_string(),
            operator_version: operator_version.to_string(),
            param_schema_hash: "param-hash-a".to_string(),
            env_signature: Some("test-env".to_string()),
        }
    }

    fn test_playbook(playbook_id: &str, fingerprint: Fingerprint, health: Health) -> Playbook {
        Playbook {
            playbook_id: playbook_id.to_string(),
            title: "Demo Playbook".to_string(),
            canonical_id: fingerprint.canonical_id.clone(),
            operator_version: fingerprint.operator_version.clone(),
            fingerprint,
            kind: "template".to_string(),
            params: json!({
                "topic": "playbook-replay",
                "limit": 3,
            }),
            inputs: json!({
                "source": "unit-test",
            }),
            verification: PlaybookVerification {
                expected_status: "succeeded".to_string(),
                expected_output_keys: vec!["result".to_string()],
            },
            provenance: Provenance {
                distilled_from: vec!["execrec_1".to_string()],
                proposal_id: Some("proposal_1".to_string()),
                created_at: "2026-05-25T00:00:00Z".to_string(),
            },
            health,
        }
    }

    fn health(status: PlaybookStatus) -> Health {
        Health {
            hit_count: 0,
            success_count: 0,
            last_verified_at: None,
            status,
        }
    }

    #[test]
    fn resolve_for_replay_returns_not_found_for_empty_store() {
        let temp = TestDir::new();
        let store = JsonFilePlaybookStore::new(temp.path());

        let resolution = resolve_for_replay(&store, "missing", &test_fingerprint("1.0.0"));

        assert!(matches!(resolution, ReplayResolution::NotFound));
    }

    #[test]
    fn resolve_for_replay_returns_inactive_for_quarantined_playbook() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let fingerprint = test_fingerprint("1.0.0");
        store
            .save(test_playbook(
                "playbook-quarantined",
                fingerprint.clone(),
                health(PlaybookStatus::Quarantined),
            ))
            .expect("save playbook");

        let resolution = resolve_for_replay(&store, "playbook-quarantined", &fingerprint);

        assert!(matches!(resolution, ReplayResolution::Inactive));
    }

    #[test]
    fn resolve_for_replay_returns_ready_when_fingerprint_matches() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let fingerprint = test_fingerprint("1.0.0");
        let playbook = test_playbook(
            "playbook-ready",
            fingerprint.clone(),
            health(PlaybookStatus::Active),
        );
        store.save(playbook.clone()).expect("save playbook");

        let resolution = resolve_for_replay(&store, "playbook-ready", &fingerprint);

        match resolution {
            ReplayResolution::Ready(resolved) => assert_eq!(*resolved, playbook),
            _ => panic!("expected ready resolution"),
        }
    }

    #[test]
    fn resolve_for_replay_returns_invalidated_when_operator_version_changes() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        store
            .save(test_playbook(
                "playbook-invalidated",
                test_fingerprint("1.0.0"),
                health(PlaybookStatus::Active),
            ))
            .expect("save playbook");

        let resolution =
            resolve_for_replay(&store, "playbook-invalidated", &test_fingerprint("2.0.0"));

        assert!(matches!(resolution, ReplayResolution::Invalidated));
    }

    #[test]
    fn record_replay_outcome_updates_health_for_verified_replay() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        store
            .save(test_playbook(
                "playbook-verified",
                test_fingerprint("1.0.0"),
                health(PlaybookStatus::Active),
            ))
            .expect("save playbook");

        let status = record_replay_outcome(
            &mut store,
            "playbook-verified",
            true,
            "2026-05-25T00:00:00Z",
        )
        .expect("record replay outcome");
        let updated = store.get("playbook-verified").expect("get playbook");

        assert_eq!(status, PlaybookStatus::Active);
        assert_eq!(updated.health.hit_count, 1);
        assert_eq!(updated.health.success_count, 1);
        assert_eq!(
            updated.health.last_verified_at,
            Some("2026-05-25T00:00:00Z".to_string())
        );
        assert_eq!(updated.health.status, PlaybookStatus::Active);
    }

    #[test]
    fn record_replay_outcome_quarantines_after_low_success_rate() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        store
            .save(test_playbook(
                "playbook-demote",
                test_fingerprint("1.0.0"),
                health(PlaybookStatus::Active),
            ))
            .expect("save playbook");

        for attempt in 0..3 {
            record_replay_outcome(
                &mut store,
                "playbook-demote",
                false,
                &format!("2026-05-25T00:00:0{}Z", attempt),
            )
            .expect("record replay outcome");
        }

        let updated = store.get("playbook-demote").expect("get playbook");

        assert_eq!(updated.health.hit_count, 3);
        assert_eq!(updated.health.success_count, 0);
        assert_eq!(updated.health.last_verified_at, None);
        assert_eq!(updated.health.status, PlaybookStatus::Quarantined);
    }

    #[test]
    fn should_explore_returns_true_when_roll_is_less_than_epsilon() {
        let playbook = test_playbook(
            "playbook-explore-roll",
            test_fingerprint("1.0.0"),
            health(PlaybookStatus::Active),
        );

        assert!(should_explore(&playbook, 0.1, 0.09));
    }

    #[test]
    fn should_explore_returns_false_for_active_when_roll_reaches_epsilon() {
        let playbook = test_playbook(
            "playbook-explore-active",
            test_fingerprint("1.0.0"),
            health(PlaybookStatus::Active),
        );

        assert!(!should_explore(&playbook, 0.1, 0.1));
    }

    #[test]
    fn should_explore_returns_true_for_stale_playbook() {
        let playbook = test_playbook(
            "playbook-explore-stale",
            test_fingerprint("1.0.0"),
            health(PlaybookStatus::Stale),
        );

        assert!(should_explore(&playbook, 0.1, 0.99));
    }
}
