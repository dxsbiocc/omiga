//! Playbook 固化系统(Task Graph Crystallization)。
//!
//! 把"被验证过的执行轨迹"晋升为"可检索、可重放、带指纹"的确定性流程。
//! 设计文档:`docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md`。
//!
//! 模块划分(Phase 0 地基):
//! - [`types`]:冻结的数据契约(orchestrator 维护)。
//! - [`fingerprint`]:指纹构造与精确匹配(Wave 1 / Codex A)。
//! - [`store`]:Playbook 持久化与 O(1) 指纹查找(Wave 1 / Codex B)。
//!
//! 注:`fingerprint` / `store` 内部条目的 `pub use` 重导出在 Wave 1 合并时由
//! orchestrator 统一补齐,避免并行实现期间争抢本文件造成冲突。

pub mod fingerprint;
pub mod store;
pub mod types;

pub use types::{
    Fingerprint, Health, Playbook, PlaybookStatus, PlaybookStore, PlaybookVerification, Provenance,
};
