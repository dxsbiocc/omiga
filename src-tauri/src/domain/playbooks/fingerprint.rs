//! 指纹构造与精确匹配(Wave 1 / Codex A 实现)。
//!
//! `Fingerprint::index_key` 定义在 `types.rs`;本模块只负责从调用现场构造指纹,
//! 以及用稳定索引键做精确匹配判定。

use super::types::Fingerprint;
use serde_json::Value as JsonValue;

impl Fingerprint {
    pub fn from_invocation(
        canonical_id: impl Into<String>,
        operator_version: impl Into<String>,
        params: &JsonValue,
        env_signature: Option<String>,
    ) -> Self {
        let param_schema_hash =
            crate::domain::execution_records::hash_execution_map(params).unwrap_or_default();

        Self {
            canonical_id: canonical_id.into(),
            operator_version: operator_version.into(),
            param_schema_hash,
            env_signature,
        }
    }

    pub fn matches(&self, other: &Fingerprint) -> bool {
        self.index_key() == other.index_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_params() -> JsonValue {
        json!({
            "threshold": 0.05,
            "genes": ["TP53", "BRCA1"],
            "options": {
                "normalize": true,
                "method": "bh"
            }
        })
    }

    fn sample_fingerprint() -> Fingerprint {
        Fingerprint::from_invocation(
            "plugin/template/differential-expression",
            "1.0.0",
            &sample_params(),
            Some("linux-x86_64".to_string()),
        )
    }

    #[test]
    fn index_key_is_deterministic_for_same_input() {
        let first = sample_fingerprint();
        let second = sample_fingerprint();

        assert_eq!(first.index_key(), second.index_key());
        assert_eq!(first.index_key(), first.index_key());
    }

    #[test]
    fn param_key_order_does_not_change_fingerprint() {
        let params_a = json!({
            "alpha": 1,
            "beta": {
                "enabled": true,
                "limit": 10
            },
            "gamma": ["x", "y"]
        });
        let params_b = json!({
            "gamma": ["x", "y"],
            "beta": {
                "limit": 10,
                "enabled": true
            },
            "alpha": 1
        });

        let first = Fingerprint::from_invocation("plugin/template/order", "1.0.0", &params_a, None);
        let second =
            Fingerprint::from_invocation("plugin/template/order", "1.0.0", &params_b, None);

        assert_eq!(first.param_schema_hash, second.param_schema_hash);
        assert_eq!(first.index_key(), second.index_key());
        assert!(first.matches(&second));
    }

    #[test]
    fn operator_version_changes_index_key_and_match_result() {
        let params = sample_params();
        let first =
            Fingerprint::from_invocation("plugin/template/versioned", "1.0.0", &params, None);
        let second =
            Fingerprint::from_invocation("plugin/template/versioned", "1.0.1", &params, None);

        assert_ne!(first.index_key(), second.index_key());
        assert!(!first.matches(&second));
    }

    #[test]
    fn param_value_changes_index_key() {
        let first_params = json!({
            "threshold": 0.05,
            "method": "bh"
        });
        let second_params = json!({
            "threshold": 0.10,
            "method": "bh"
        });

        let first =
            Fingerprint::from_invocation("plugin/template/params", "1.0.0", &first_params, None);
        let second =
            Fingerprint::from_invocation("plugin/template/params", "1.0.0", &second_params, None);

        assert_ne!(first.param_schema_hash, second.param_schema_hash);
        assert_ne!(first.index_key(), second.index_key());
    }

    #[test]
    fn env_signature_presence_changes_index_key() {
        let params = sample_params();
        let without_env =
            Fingerprint::from_invocation("plugin/template/env", "1.0.0", &params, None);
        let with_env = Fingerprint::from_invocation(
            "plugin/template/env",
            "1.0.0",
            &params,
            Some("linux-x86_64".to_string()),
        );

        assert_ne!(without_env.index_key(), with_env.index_key());
    }

    #[test]
    fn matches_is_reflexive_and_symmetric_for_equal_fingerprints() {
        let first = sample_fingerprint();
        let second = sample_fingerprint();

        assert!(first.matches(&first));
        assert!(first.matches(&second));
        assert!(second.matches(&first));
    }
}
