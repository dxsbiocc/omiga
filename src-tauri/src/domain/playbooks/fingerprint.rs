//! 指纹构造与精确匹配(Wave 1 / Codex A 实现)。
//!
//! 任务规格见 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 与 orchestrator 下发的提示。
//! 需实现:`Fingerprint::from_parts(...)`(复用 `execution_records::hash_execution_map`
//! 计算 `param_schema_hash`)、`Fingerprint::matches(&self, other)` 精确匹配,以及
//! 覆盖确定性 / 版本敏感 / 参数敏感的单元测试。`Fingerprint::index_key` 已在
//! `types.rs` 实现,勿重复。
