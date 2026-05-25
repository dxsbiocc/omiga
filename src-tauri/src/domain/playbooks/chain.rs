//! 链 Playbook 的构造(Wave 2 / Codex C 实现)。
//!
//! 任务规格见 orchestrator 下发的提示与 `docs/PLAYBOOK_CRYSTALLIZATION_DESIGN.md` 第 11 节。
//! 需实现:`chain_canonical_id`、`chain_composite_version`、`build_chain_playbook`
//! (`kind="chain"`,`params` 存序列化的 `Vec<ChainStep>`,指纹经
//! `Fingerprint::from_invocation`),以及对应单元测试。
