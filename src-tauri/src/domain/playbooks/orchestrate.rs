//! L2 重放编排(orchestrator 实现)。
//!
//! 把 Wave 1/2 的纯逻辑(指纹失效检测、health 回写)与**注入式链执行**拼成完整的
//! "凭引用重放"闭环:解析失效 → 执行 → 验证 → 回写 health(含 auto-demote)。
//!
//! 关键设计:对结果类型 `R` 泛型化、链执行经闭包 `run_chain` 注入。这样:
//! 1. 不依赖 command 层的 `OperatorChainResult`(避免 domain→command 分层倒置);
//! 2. 可用 mock runner 完整单测,无需 Tauri `State`;
//! 3. 真正的 tauri 命令只是把真实链执行器注入本函数的薄包装。

use super::chain::chain_composite_version;
use super::replay::{record_replay_outcome, resolve_for_replay};
use super::types::{Fingerprint, PlaybookStatus, ReplayResolution};
use crate::domain::operators::ChainStep;

/// 重放编排结果。
#[derive(Debug, PartialEq, Eq)]
pub enum ReplayOutcome<R> {
    /// 指纹仍一致且 Active —— 已重放;`verified` 为验证结论,`status` 为回写后的状态。
    Replayed {
        result: R,
        verified: bool,
        status: PlaybookStatus,
    },
    /// 算子版本或环境漂移 —— 已失效,**未执行**,调用方应回退正常链路规划。
    Invalidated,
    /// 找不到该 playbook。
    NotFound,
    /// 存在但非 Active(Stale / Quarantined)。
    Inactive,
}

/// 凭 `playbook_id` 重放一条链 Playbook。
///
/// 流程:① 用**当前**算子版本和环境重算指纹 → `resolve_for_replay` 判定;② 仅在 `Ready` 时把
/// 存储的 `params` 反序列化为 `Vec<ChainStep>` 并经注入的 `run_chain` 执行;③ 用
/// `verify` 判定本次是否通过验证;④ `record_replay_outcome` 回写 health(可能触发
/// auto-demote)。失效 / 未找到 / 非 Active 时**绝不执行**,直接返回对应结果让调用方回退。
///
/// `run_chain`:注入的链执行器(真实命令层提供,测试可注入 mock)。
/// `verify`:从执行结果和存储的验证契约判定"是否通过验证"。验证在重放路径**强制**执行。
pub async fn execute_replay<R, F, Fut>(
    store: &mut dyn super::types::PlaybookStore,
    playbook_id: &str,
    current_operator_versions: &[(String, String)],
    current_env_signature: Option<String>,
    now: &str,
    run_chain: F,
    verify: impl Fn(&R, &super::types::PlaybookVerification) -> bool,
) -> Result<ReplayOutcome<R>, String>
where
    F: FnOnce(Vec<crate::domain::operators::ChainStep>) -> Fut,
    Fut: std::future::Future<Output = R>,
{
    // 取存储的 playbook 以重建"当前指纹"(canonical_id / params 来自存储,
    // operator_version / env 用**当前**输入重算 —— 漂移即在此体现为指纹失配)。
    let Some(stored) = store.get(playbook_id) else {
        return Ok(ReplayOutcome::NotFound);
    };
    let current_fingerprint = Fingerprint::from_invocation(
        &stored.canonical_id,
        &chain_composite_version(current_operator_versions),
        &stored.params,
        current_env_signature,
    );

    match resolve_for_replay(&*store, playbook_id, &current_fingerprint) {
        ReplayResolution::NotFound => Ok(ReplayOutcome::NotFound),
        ReplayResolution::Inactive => Ok(ReplayOutcome::Inactive),
        ReplayResolution::Invalidated => Ok(ReplayOutcome::Invalidated),
        ReplayResolution::Ready(ready) => {
            let steps: Vec<ChainStep> = serde_json::from_value(ready.params.clone())
                .map_err(|err| format!("deserialize chain steps for replay: {err}"))?;
            let result = run_chain(steps).await;
            let verified = verify(&result, &ready.verification);
            let status = record_replay_outcome(store, playbook_id, verified, now)?;
            Ok(ReplayOutcome::Replayed {
                result,
                verified,
                status,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::chain::build_chain_playbook;
    use super::super::store::JsonFilePlaybookStore;
    use super::super::types::{
        Playbook, PlaybookStatus, PlaybookStore, PlaybookVerification, Provenance,
    };
    use super::*;
    use crate::domain::operators::ChainStep;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            Self {
                path: std::env::temp_dir()
                    .join(format!("playbook-orchestrate-test-{}", Uuid::new_v4())),
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn sample_steps() -> Vec<ChainStep> {
        vec![
            serde_json::from_value(json!({
                "alias": "load",
                "arguments": { "path": "in.csv" },
                "dependsOn": []
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "alias": "summarize",
                "arguments": { "method": "mean" },
                "inheritPrevOutputAs": "inputDir",
                "dependsOn": ["load"]
            }))
            .unwrap(),
        ]
    }

    fn versions(summarize: &str) -> Vec<(String, String)> {
        vec![
            ("load".to_string(), "1.0.0".to_string()),
            ("summarize".to_string(), summarize.to_string()),
        ]
    }

    fn provenance() -> Provenance {
        Provenance {
            distilled_from: vec!["execrec-1".to_string()],
            proposal_id: None,
            created_at: "2026-05-25T00:00:00Z".to_string(),
        }
    }

    fn save_chain_playbook(store: &mut JsonFilePlaybookStore, id: &str, vers: &[(String, String)]) {
        let pb = build_chain_playbook(
            id,
            "Demo Chain",
            &sample_steps(),
            vers,
            vec!["summary".to_string()],
            Some("test-env".to_string()),
            provenance(),
        )
        .expect("build chain playbook");
        store.save(pb).expect("save playbook");
    }

    #[tokio::test]
    async fn ready_replays_and_records_success() {
        let dir = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(&dir.path);
        save_chain_playbook(&mut store, "pb-ready", &versions("2.0.0"));

        let outcome = execute_replay(
            &mut store,
            "pb-ready",
            &versions("2.0.0"), // 与存储版本一致 → 指纹匹配
            Some("test-env".to_string()),
            "2026-05-25T01:00:00Z",
            |steps| async move {
                // 注入的链执行器收到反序列化回来的完整 steps
                assert_eq!(steps.len(), 2);
                true // 模拟链执行成功
            },
            |ok: &bool, _verification: &PlaybookVerification| *ok,
        )
        .await
        .expect("execute replay");

        match outcome {
            ReplayOutcome::Replayed {
                result,
                verified,
                status,
            } => {
                assert!(result);
                assert!(verified);
                assert_eq!(status, PlaybookStatus::Active);
            }
            other => panic!("expected Replayed, got {other:?}"),
        }

        let after = store.get("pb-ready").expect("reload");
        assert_eq!(after.health.hit_count, 1);
        assert_eq!(after.health.success_count, 1);
        assert_eq!(
            after.health.last_verified_at,
            Some("2026-05-25T01:00:00Z".to_string())
        );
    }

    #[tokio::test]
    async fn invalidated_when_operator_version_drifts_does_not_execute() {
        let dir = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(&dir.path);
        save_chain_playbook(&mut store, "pb-drift", &versions("2.0.0"));

        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        let outcome = execute_replay(
            &mut store,
            "pb-drift",
            &versions("2.1.0"), // 版本漂移 → 指纹失配
            Some("test-env".to_string()),
            "2026-05-25T01:00:00Z",
            |_steps| async move {
                ran_clone.store(true, Ordering::SeqCst);
                true
            },
            |ok: &bool, _verification: &PlaybookVerification| *ok,
        )
        .await
        .expect("execute replay");

        assert_eq!(outcome, ReplayOutcome::Invalidated);
        assert!(!ran.load(Ordering::SeqCst), "失效时绝不执行链");
        // 未执行 → health 不变
        let after = store.get("pb-drift").expect("reload");
        assert_eq!(after.health.hit_count, 0);
    }

    #[tokio::test]
    async fn invalidated_when_env_signature_drifts_does_not_execute() {
        let dir = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(&dir.path);
        save_chain_playbook(&mut store, "pb-env-drift", &versions("2.0.0"));

        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        let outcome = execute_replay(
            &mut store,
            "pb-env-drift",
            &versions("2.0.0"),
            Some("other-env".to_string()), // 环境漂移 → 指纹失配
            "2026-05-25T01:00:00Z",
            |_steps| async move {
                ran_clone.store(true, Ordering::SeqCst);
                true
            },
            |ok: &bool, _verification: &PlaybookVerification| *ok,
        )
        .await
        .expect("execute replay");

        assert_eq!(outcome, ReplayOutcome::Invalidated);
        assert!(!ran.load(Ordering::SeqCst), "环境失效时绝不执行链");
        let after = store.get("pb-env-drift").expect("reload");
        assert_eq!(after.health.hit_count, 0);
    }

    #[tokio::test]
    async fn not_found_for_missing_playbook() {
        let dir = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(&dir.path);

        let outcome = execute_replay(
            &mut store,
            "missing",
            &versions("2.0.0"),
            Some("test-env".to_string()),
            "2026-05-25T01:00:00Z",
            |_steps| async move { true },
            |ok: &bool, _verification: &PlaybookVerification| *ok,
        )
        .await
        .expect("execute replay");

        assert_eq!(outcome, ReplayOutcome::NotFound);
    }

    #[tokio::test]
    async fn inactive_for_quarantined_playbook_does_not_execute() {
        let dir = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(&dir.path);
        // 构造一个 Quarantined 的链 playbook
        let pb = build_chain_playbook(
            "pb-quarantined",
            "Demo Chain",
            &sample_steps(),
            &versions("2.0.0"),
            vec!["summary".to_string()],
            Some("test-env".to_string()),
            provenance(),
        )
        .expect("build");
        let quarantined = Playbook {
            health: super::super::types::Health {
                status: PlaybookStatus::Quarantined,
                ..pb.health.clone()
            },
            ..pb
        };
        store.save(quarantined).expect("save");

        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();
        let outcome = execute_replay(
            &mut store,
            "pb-quarantined",
            &versions("2.0.0"),
            Some("test-env".to_string()),
            "2026-05-25T01:00:00Z",
            |_steps| async move {
                ran_clone.store(true, Ordering::SeqCst);
                true
            },
            |ok: &bool, _verification: &PlaybookVerification| *ok,
        )
        .await
        .expect("execute replay");

        assert_eq!(outcome, ReplayOutcome::Inactive);
        assert!(!ran.load(Ordering::SeqCst), "非 Active 时绝不执行链");
    }
}
