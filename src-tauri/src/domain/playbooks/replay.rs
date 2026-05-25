//! 重放解析 + health 反馈 + 探索阀门(Wave 2 / Codex D 实现)。
//!
//! 任务规格见 orchestrator 下发的提示与 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 第 11 节。
//! 需实现:`resolve_for_replay`(返回 `types::ReplayResolution`)、`record_replay_outcome`
//! (health 回写 + 成功率阈值 auto-demote)、`should_explore`(探索阀门),及对应单元测试。
//! `ReplayResolution` 枚举与阈值常量已在 `types.rs` 定义,直接使用。
