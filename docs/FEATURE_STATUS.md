# Feature Status

Last updated: 2026-05-16

This file is the release-facing source of truth for README capability claims. Use
these states consistently:

- `stable`: implemented, documented, and covered by normal validation.
- `beta`: implemented but still needs stronger UX, evidence, or hardening before
  stronger public claims.
- `experimental`: available or wired in part, but not production-safe for broad
  use.
- `planned`: roadmap item with no reliable user-facing implementation yet.
- `deprecated`: retained only for compatibility.

## Summary

| Capability | Status | Evidence | Next hardening step |
| --- | --- | --- | --- |
| Local-first desktop workspace | stable | `README.md`, `src-tauri/src/lib.rs`, `src-tauri/src/domain/persistence/mod.rs`, `src-tauri/src/domain/session/config.rs` | Keep platform support labels accurate before each release. |
| Provider-based LLM runtime | stable | `src-tauri/src/llm/mod.rs`, `src-tauri/src/llm/config.rs`, `src-tauri/src/commands/chat/settings.rs`, `src-tauri/src/commands/chat/provider.rs` | Add first-run diagnostics for missing keys and disabled providers. |
| Auditable tool execution | beta | `src-tauri/src/domain/audit.rs`, `src-tauri/src/commands/chat/tool_exec.rs`, `src-tauri/src/commands/permissions.rs`, `src-tauri/src/domain/persistence/mod.rs` | Expand durable permission audit UI and include approval/denial restart tests in release validation. |
| Multi-agent workflows | beta | `src-tauri/src/domain/agents/scheduler/*`, `src-tauri/src/domain/agents/background.rs`, `src-tauri/src/commands/chat/subagent.rs`, `src-tauri/src/domain/persistence/mod.rs` | Add clearer task lifecycle UI and multi-agent worktree validation. |
| Repository-aware context and worktrees | stable | `src-tauri/src/domain/tools/enter_worktree.rs`, `src-tauri/src/domain/tools/exit_worktree.rs`, `src-tauri/src/domain/session/config.rs`, `src/components/CodeWorkspace/*` | Add task-to-worktree status in command-center views. |
| Search and retrieval | stable | `src-tauri/src/domain/retrieval/*`, `src-tauri/src/domain/retrieval_registry.rs`, `src-tauri/src/domain/tools/search/*`, `docs/retrieval-plugin-protocol.md` | Add provider fallback observability in Settings. |
| Persistent memory | stable | `src-tauri/src/commands/memory/mod.rs`, `src-tauri/src/domain/memory/*`, `src-tauri/src/domain/pageindex/*`, `src/hooks/useUnifiedMemory.ts` | Add recall explanation and retrieval regression fixtures. |
| Operator system | beta | `docs/OPERATOR_PLUGIN_MANIFEST.md`, `docs/PLUGIN_DEVELOPER_GUIDE.md`, `src-tauri/src/domain/operators/mod.rs`, `src-tauri/src/commands/plugins.rs`, `src-tauri/src/commands/chat/tool_exec.rs` | Ship curated operator catalog and creation wizard before stronger marketplace claims. |
| Local IPC bridge | stable | `src-tauri/src/bridge/*`, `src-tauri/src/commands/bridge.rs`, `README.md` | Add bridge smoke tests for editor/CLI clients. |
| Execution environments | experimental | `src-tauri/src/commands/execution_envs.rs`, `src-tauri/src/domain/tools/environment/*`, `src-tauri/src/execution/*`, `docs/SECURITY_MODEL.md` | Keep incomplete backends visibly marked experimental/unavailable in UI and docs. |
| Release validation paths | stable | `package.json`, `src-tauri/Cargo.toml`, `scripts/mock-llm-validation.sh`, `scripts/real-llm-validation.sh`, `README.md` | Add first-run safe-demo and permission-audit restart checks. |

## Release Claim Rules

- README must not describe `experimental` features as production-safe.
- Any new stable claim needs at least one implementation link and one verification
  path.
- Security-sensitive features require a `docs/SECURITY_MODEL.md` update when the
  trust boundary changes.
- New operator, retrieval, execution backend, or computer-use claims must identify
  whether they run locally, via SSH/container, or through an external provider.

## Current Gaps

| Gap | Priority | Owner lane | Notes |
| --- | --- | --- | --- |
| Durable permission audit UI is not complete | P0 | Trust hardening | Backend SQLite records now persist project-scoped approve/deny events with argument redaction; Settings UI has compact decision/tool filtering but still needs pagination and richer facets. |
| First-run setup is split across onboarding and provider settings | P1 | Daily loop UX | `get_setup_status`, `OnboardingWizard`, and `SetupGuideDialog` exist, but the app needs a single polished path. |
| Operator creation UX is not yet a first-class wizard | P1 | Domain workflow platform | Manifest/runtime support exists; user-script wizard should remain a beta claim until shipped. |
| Execution backends vary in maturity | P1 | Trust hardening | Modal/Daytona and other non-local surfaces must stay experimental until validation exists. |
| Memory recall quality is not measured by a fixture suite | P2 | Memory/retrieval quality | Add precision/recall fixtures before marketing quality improvements. |
