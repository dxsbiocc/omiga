//! Playbook 持久化与 O(1) 指纹查找(Wave 1 / Codex B 实现)。
//!
//! 任务规格见 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 与 orchestrator 下发的提示。
//! 需实现:`JsonFilePlaybookStore`(每 Playbook 一个 `<id>.json`,参照
//! `research_system/stores.rs::JsonFileTaskGraphStore`)+ `PlaybookStore` trait 全部方法。
//! 硬性要求:`find_by_fingerprint` 必须经内存索引 `index_key -> playbook_id` 做 O(1) 查表,
//! 禁止线性扫描;仅返回 `status == Active`。附完整单元测试(含 round-trip、索引命中、
//! 版本变更失配、delete 清索引)。
