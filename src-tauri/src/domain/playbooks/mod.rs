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
//! `fingerprint` 的指纹构造 / 匹配方法是 `Fingerprint` 的固有方法,随类型自动可用;
//! `store` 的具体实现 `JsonFilePlaybookStore` 在此统一重导出。

pub mod fingerprint;
pub mod store;
pub mod types;

pub use store::JsonFilePlaybookStore;
pub use types::{
    Fingerprint, Health, Playbook, PlaybookStatus, PlaybookStore, PlaybookVerification, Provenance,
};
