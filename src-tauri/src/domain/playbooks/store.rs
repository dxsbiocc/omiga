//! Playbook 持久化与 O(1) 指纹查找(Wave 1 / Codex B 实现)。
//!
//! 任务规格见 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 与 orchestrator 下发的提示。
//! 需实现:`JsonFilePlaybookStore`(每 Playbook 一个 `<id>.json`,参照
//! `research_system/stores.rs::JsonFileTaskGraphStore`)+ `PlaybookStore` trait 全部方法。
//! 硬性要求:`find_by_fingerprint` 必须经内存索引 `index_key -> playbook_id` 做 O(1) 查表,
//! 禁止线性扫描;仅返回 `status == Active`。附完整单元测试(含 round-trip、索引命中、
//! 版本变更失配、delete 清索引)。

use super::types::{Fingerprint, Playbook, PlaybookStatus, PlaybookStore};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct JsonFilePlaybookStore {
    dir: PathBuf,
    index: HashMap<String, String>,
}

impl JsonFilePlaybookStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref().to_path_buf();
        let mut index = HashMap::new();

        for playbook in read_json_dir_lossy(&dir) {
            index.insert(playbook.fingerprint.index_key(), playbook.playbook_id);
        }

        Self { dir, index }
    }
}

impl PlaybookStore for JsonFilePlaybookStore {
    fn save(&mut self, playbook: Playbook) -> Result<(), String> {
        write_json_record(&self.dir, &playbook.playbook_id, &playbook)?;

        self.index
            .retain(|_, indexed_id| indexed_id != &playbook.playbook_id);
        self.index.insert(
            playbook.fingerprint.index_key(),
            playbook.playbook_id.clone(),
        );

        Ok(())
    }

    fn get(&self, playbook_id: &str) -> Option<Playbook> {
        read_json_record(
            self.dir
                .join(format!("{}.json", sanitize_playbook_id(playbook_id))),
        )
        .ok()
    }

    fn find_by_fingerprint(&self, fingerprint: &Fingerprint) -> Option<Playbook> {
        let index_key = fingerprint.index_key();
        let playbook_id = self.index.get(&index_key)?;
        let playbook = self.get(playbook_id)?;

        if playbook.fingerprint.index_key() == index_key
            && playbook.health.status == PlaybookStatus::Active
        {
            Some(playbook)
        } else {
            None
        }
    }

    fn list(&self) -> Vec<Playbook> {
        read_json_dir_lossy(&self.dir)
    }

    fn delete(&mut self, playbook_id: &str) -> Result<(), String> {
        let path = self
            .dir
            .join(format!("{}.json", sanitize_playbook_id(playbook_id)));

        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                return Err(format!("delete playbook '{}': {}", path.display(), err));
            }
        }

        self.index
            .retain(|_, indexed_id| indexed_id.as_str() != playbook_id);
        Ok(())
    }
}

fn write_json_record<T: Serialize>(dir: &Path, id: &str, value: &T) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|err| format!("create playbook directory '{}': {}", dir.display(), err))?;
    let json = serde_json::to_string_pretty(value)
        .map_err(|err| format!("serialize playbook '{}': {}", id, err))?;
    let path = dir.join(format!("{}.json", sanitize_playbook_id(id)));
    fs::write(&path, json).map_err(|err| format!("write playbook '{}': {}", path.display(), err))
}

fn sanitize_playbook_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn read_json_record<T: DeserializeOwned>(path: PathBuf) -> Result<T, String> {
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("read playbook '{}': {}", path.display(), err))?;
    serde_json::from_str(&raw)
        .map_err(|err| format!("parse playbook '{}': {}", path.display(), err))
}

fn read_json_dir_lossy(dir: &Path) -> Vec<Playbook> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|path| read_json_record(path).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        Fingerprint, Health, Playbook, PlaybookStatus, PlaybookStore, PlaybookVerification,
        Provenance,
    };
    use super::JsonFilePlaybookStore;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("json-file-playbook-store-test-{}", Uuid::new_v4()));
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

    fn test_playbook(
        playbook_id: &str,
        fingerprint: Fingerprint,
        status: PlaybookStatus,
    ) -> Playbook {
        Playbook {
            playbook_id: playbook_id.to_string(),
            title: "Demo Playbook".to_string(),
            canonical_id: fingerprint.canonical_id.clone(),
            operator_version: fingerprint.operator_version.clone(),
            fingerprint,
            kind: "template".to_string(),
            params: json!({
                "topic": "playbook-store",
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
            health: Health {
                hit_count: 0,
                success_count: 0,
                last_verified_at: None,
                status,
            },
        }
    }

    #[test]
    fn save_get_round_trip() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let playbook = test_playbook(
            "playbook-round-trip",
            test_fingerprint("1.0.0"),
            PlaybookStatus::Active,
        );

        store.save(playbook.clone()).expect("save playbook");

        assert_eq!(store.get("playbook-round-trip"), Some(playbook));
    }

    #[test]
    fn save_get_delete_sanitize_playbook_id_only_for_file_paths() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let unique = Uuid::new_v4();
        let playbook_id = format!("../escape-{unique}");
        let playbook = test_playbook(
            &playbook_id,
            test_fingerprint("1.0.0"),
            PlaybookStatus::Active,
        );
        let escaped_path = temp
            .path()
            .parent()
            .expect("temp path has parent")
            .join(format!("escape-{unique}.json"));
        let sanitized_path =
            temp.path().join(format!("{}.json", super::sanitize_playbook_id(&playbook_id)));

        let _ = fs::remove_file(&escaped_path);

        store.save(playbook.clone()).expect("save playbook");

        let sanitized_exists_after_save = sanitized_path.exists();
        let escaped_exists_after_save = escaped_path.exists();
        let saved_playbook = store.get(&playbook_id);
        let _ = fs::remove_file(&escaped_path);

        assert!(sanitized_exists_after_save);
        assert!(!escaped_exists_after_save);
        assert_eq!(saved_playbook, Some(playbook));

        store.delete(&playbook_id).expect("delete playbook");

        assert!(!sanitized_path.exists());
        assert!(!escaped_path.exists());
    }

    #[test]
    fn find_by_fingerprint_hits_active_playbook() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let fingerprint = test_fingerprint("1.0.0");
        let playbook = test_playbook(
            "playbook-active",
            fingerprint.clone(),
            PlaybookStatus::Active,
        );

        store.save(playbook.clone()).expect("save playbook");

        assert_eq!(store.find_by_fingerprint(&fingerprint), Some(playbook));
    }

    #[test]
    fn find_by_fingerprint_misses_when_operator_version_changes() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let stored_fingerprint = test_fingerprint("1.0.0");
        let changed_fingerprint = test_fingerprint("2.0.0");
        let playbook = test_playbook(
            "playbook-versioned",
            stored_fingerprint,
            PlaybookStatus::Active,
        );

        store.save(playbook).expect("save playbook");

        assert_eq!(store.find_by_fingerprint(&changed_fingerprint), None);
    }

    #[test]
    fn find_by_fingerprint_only_returns_active_playbooks() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let fingerprint = test_fingerprint("1.0.0");
        let playbook = test_playbook(
            "playbook-quarantined",
            fingerprint.clone(),
            PlaybookStatus::Quarantined,
        );

        store.save(playbook.clone()).expect("save playbook");

        assert_eq!(store.find_by_fingerprint(&fingerprint), None);
        assert_eq!(store.get("playbook-quarantined"), Some(playbook));
    }

    #[test]
    fn delete_removes_file_and_index_entry() {
        let temp = TestDir::new();
        let mut store = JsonFilePlaybookStore::new(temp.path());
        let fingerprint = test_fingerprint("1.0.0");
        let playbook = test_playbook(
            "playbook-delete",
            fingerprint.clone(),
            PlaybookStatus::Active,
        );

        store.save(playbook).expect("save playbook");
        store.delete("playbook-delete").expect("delete playbook");
        store
            .delete("playbook-delete")
            .expect("delete remains idempotent");

        assert_eq!(store.get("playbook-delete"), None);
        assert_eq!(store.find_by_fingerprint(&fingerprint), None);
    }

    #[test]
    fn new_rebuilds_index_from_existing_json_files() {
        let temp = TestDir::new();
        let fingerprint = test_fingerprint("1.0.0");
        let playbook = test_playbook(
            "playbook-reindexed",
            fingerprint.clone(),
            PlaybookStatus::Active,
        );
        fs::create_dir_all(temp.path()).expect("create temp playbook dir");
        let json = serde_json::to_string_pretty(&playbook).expect("serialize playbook");
        fs::write(temp.path().join("playbook-reindexed.json"), json).expect("write playbook");

        let store = JsonFilePlaybookStore::new(temp.path());

        assert_eq!(store.find_by_fingerprint(&fingerprint), Some(playbook));
    }
}
