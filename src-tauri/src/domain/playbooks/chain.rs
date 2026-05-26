//! 链 Playbook 的构造(Wave 2 / Codex C 实现)。
//!
//! 任务规格见 orchestrator 下发的提示与 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 第 11 节。
//! 需实现:`chain_canonical_id`、`chain_composite_version`、`build_chain_playbook`
//! (`kind="chain"`,`params` 存序列化的 `Vec<ChainStep>`,指纹经
//! `Fingerprint::from_invocation`),以及对应单元测试。

use super::types::{Fingerprint, Health, Playbook, PlaybookVerification, Provenance};
use crate::domain::operators::ChainStep;
use sha2::{Digest, Sha256};

pub fn chain_canonical_id(steps: &[ChainStep]) -> String {
    let aliases = steps
        .iter()
        .map(|step| step.alias.as_str())
        .collect::<Vec<_>>()
        .join(">");

    format!("chain:{}", aliases)
}

pub fn chain_composite_version(operator_versions: &[(String, String)]) -> String {
    let mut versions = operator_versions.to_vec();
    versions.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let canonical = versions
        .iter()
        .map(|(alias, version)| format!("{}={}", alias, version))
        .collect::<Vec<_>>()
        .join("\n");

    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub fn build_chain_playbook(
    playbook_id: impl Into<String>,
    title: impl Into<String>,
    steps: &[ChainStep],
    operator_versions: &[(String, String)],
    expected_output_keys: Vec<String>,
    env_signature: Option<String>,
    provenance: Provenance,
) -> Result<Playbook, String> {
    let params =
        serde_json::to_value(steps).map_err(|err| format!("serialize chain steps: {}", err))?;
    let canonical_id = chain_canonical_id(steps);
    let operator_version = chain_composite_version(operator_versions);
    let fingerprint = Fingerprint::from_invocation(
        &canonical_id,
        &operator_version,
        &params,
        env_signature.clone(),
    );

    Ok(Playbook {
        playbook_id: playbook_id.into(),
        title: title.into(),
        fingerprint,
        kind: "chain".into(),
        canonical_id,
        operator_version,
        params,
        inputs: serde_json::Value::Null,
        verification: PlaybookVerification {
            expected_status: "succeeded".into(),
            expected_output_keys,
        },
        provenance,
        health: Health::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::store::JsonFilePlaybookStore;
    use super::super::types::PlaybookStore;
    use super::*;
    use serde_json::{json, Value as JsonValue};
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("chain-playbook-store-test-{}", Uuid::new_v4()));
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

    fn chain_step(value: JsonValue) -> ChainStep {
        serde_json::from_value::<ChainStep>(value).expect("valid chain step")
    }

    fn sample_steps() -> Vec<ChainStep> {
        vec![
            chain_step(json!({
                "alias": "load",
                "label": "Load source data",
                "arguments": {
                    "path": "input.csv"
                },
                "dependsOn": []
            })),
            chain_step(json!({
                "alias": "summarize",
                "arguments": {
                    "method": "mean"
                },
                "inheritPrevOutputAs": "inputDir",
                "dependsOn": ["load"]
            })),
        ]
    }

    fn reversed_sample_steps() -> Vec<ChainStep> {
        vec![
            chain_step(json!({
                "alias": "summarize",
                "arguments": {
                    "method": "mean"
                },
                "inheritPrevOutputAs": "inputDir",
                "dependsOn": ["load"]
            })),
            chain_step(json!({
                "alias": "load",
                "label": "Load source data",
                "arguments": {
                    "path": "input.csv"
                },
                "dependsOn": []
            })),
        ]
    }

    fn sample_versions() -> Vec<(String, String)> {
        vec![
            ("load".to_string(), "1.0.0".to_string()),
            ("summarize".to_string(), "2.0.0".to_string()),
        ]
    }

    fn sample_provenance() -> Provenance {
        Provenance {
            distilled_from: vec!["execution-record-1".to_string()],
            proposal_id: Some("proposal-1".to_string()),
            created_at: "2026-05-25T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn chain_canonical_id_is_ordered_and_deterministic() {
        let steps = sample_steps();
        let reordered = reversed_sample_steps();

        assert_eq!(chain_canonical_id(&steps), "chain:load>summarize");
        assert_eq!(chain_canonical_id(&steps), chain_canonical_id(&steps));
        assert_ne!(chain_canonical_id(&steps), chain_canonical_id(&reordered));
    }

    #[test]
    fn chain_composite_version_sorts_by_alias_before_hashing() {
        let versions = sample_versions();
        let reordered = vec![
            ("summarize".to_string(), "2.0.0".to_string()),
            ("load".to_string(), "1.0.0".to_string()),
        ];
        let changed = vec![
            ("summarize".to_string(), "2.1.0".to_string()),
            ("load".to_string(), "1.0.0".to_string()),
        ];

        let first = chain_composite_version(&versions);

        assert!(first.starts_with("sha256:"));
        assert_eq!(first.len(), "sha256:".len() + 64);
        assert_eq!(first, chain_composite_version(&versions));
        assert_eq!(first, chain_composite_version(&reordered));
        assert_ne!(first, chain_composite_version(&changed));
    }

    #[test]
    fn build_chain_playbook_serializes_steps_and_sets_fingerprint_fields() {
        let steps = sample_steps();
        let operator_versions = sample_versions();
        let playbook = build_chain_playbook(
            "playbook-chain-1",
            "Demo Chain",
            &steps,
            &operator_versions,
            vec!["summary".to_string(), "report".to_string()],
            Some("darwin-arm64".to_string()),
            sample_provenance(),
        )
        .expect("build chain playbook");

        let expected_canonical_id = chain_canonical_id(&steps);
        let expected_operator_version = chain_composite_version(&operator_versions);
        let expected_fingerprint = Fingerprint::from_invocation(
            &expected_canonical_id,
            &expected_operator_version,
            &playbook.params,
            Some("darwin-arm64".to_string()),
        );
        let restored_steps: Vec<ChainStep> =
            serde_json::from_value(playbook.params.clone()).expect("deserialize chain params");

        assert_eq!(playbook.kind, "chain");
        assert_eq!(playbook.canonical_id, expected_canonical_id);
        assert_eq!(playbook.operator_version, expected_operator_version);
        assert_eq!(playbook.fingerprint, expected_fingerprint);
        assert_eq!(
            serde_json::to_value(&restored_steps).expect("serialize restored steps"),
            serde_json::to_value(&steps).expect("serialize original steps")
        );
        assert_eq!(playbook.inputs, serde_json::Value::Null);
        assert_eq!(playbook.verification.expected_status, "succeeded");
        assert_eq!(
            playbook.verification.expected_output_keys,
            vec!["summary".to_string(), "report".to_string()]
        );
    }

    #[test]
    fn chain_playbook_round_trips_through_json_file_store_by_fingerprint() {
        let temp = TestDir::new();
        let steps = sample_steps();
        let operator_versions = sample_versions();
        let playbook = build_chain_playbook(
            "playbook-chain-store",
            "Stored Chain",
            &steps,
            &operator_versions,
            vec!["summary".to_string()],
            Some("linux-x86_64".to_string()),
            sample_provenance(),
        )
        .expect("build chain playbook");
        let fingerprint = playbook.fingerprint.clone();
        let mut store = JsonFilePlaybookStore::new(temp.path());

        store.save(playbook.clone()).expect("save chain playbook");

        assert_eq!(store.find_by_fingerprint(&fingerprint), Some(playbook));
    }
}
