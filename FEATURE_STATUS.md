# Omiga Feature Status

Last reviewed: 2026-04-25
Source of truth: this file tracks current project capability status, evidence, gaps, and next actions. When a feature's implementation status changes, update this file in the same branch.

## Legend

- ✅ Implemented and covered by repeatable verification
- 🚧 Implemented or wired structurally, but incomplete or missing repeatable runtime validation
- 🔬 Experimental / test harness / development-only
- ⚠️ Risk accepted temporarily; needs hardening before production-ready use
- ❌ Not implemented
- ➖ Not applicable to Omiga's desktop-first product direction

## Update Policy

1. Any PR or branch that changes user-visible behavior should update the relevant row.
2. Status changes must cite evidence: file, test, command, or doc.
3. Do not mark a feature ✅ unless it has a repeatable test or deterministic validation path.
4. Experimental features must say whether they are user-facing or developer-only.
5. Security-sensitive features must reference `docs/SECURITY_MODEL.md` when their trust boundary changes.

## Product Surface

| Area | Status | Evidence | Gap | Priority | Next action |
| --- | --- | --- | --- | --- | --- |
| Desktop app shell | 🚧 | Tauri config defines app bundle/window and build hooks in `src-tauri/tauri.conf.json`; React/Vite build scripts exist in `package.json`. | CI was absent before this baseline; CSP is currently documented as a hardening gap. | P0 | Keep build/test in CI green before broad UI changes. |
| Chat and session persistence | 🚧 | Session/message persistence code exists under `src-tauri/src/domain/persistence/mod.rs`; frontend chat UI exists under `src/components/Chat/`. | Needs repeatable full-flow tests that include provider/tool failure paths. | P0 | Add mock-provider E2E after CI baseline. |
| Provider configuration | 🚧 | Multi-provider config layer exists under `src-tauri/src/llm/`; `config.example.yaml` documents DeepSeek/custom/OpenAI-compatible paths. | No failover/circuit breaker/cost guard yet. | P1 | Add provider health/failover design and tests. |
| Agent orchestration commands | 🚧 | Existing validation doc identifies `/schedule`, `/team`, and `/autopilot` as MVP flows with structural event logging; real-provider planner harnesses pass via `./scripts/real-llm-validation.sh all`; CI-safe mock LLM planner harness exists in `src-tauri/tests/mock_llm_runtime_harness.rs`; TaskStatus projection + SSR render tests cover schedule/team/autopilot timeline, trace visibility, and trace callback wiring. | Full browser-rendered UI/tool-execution E2E is still missing. | P0 | Add browser E2E around live TaskStatus rendering and transcript drill-down. |
| Research system | 🔬 | `README.md` and `docs/architecture.md` describe the Research System MVP and Rust tests exist under `src-tauri/tests/research_*.rs`. | Default runner is mock/deterministic; provider-backed path is future work. | P1 | Keep mock tests green; plan provider-backed runner separately. |
| Task dashboard / orchestration timeline | 🚧 | Validation docs report dashboard/timeline/trace surfaces are structurally in place. | Needs E2E assertions against actual events and UI rendering. | P1 | Add browser or component E2E after mock LLM harness. |

## Tools and Skills

| Area | Status | Evidence | Gap | Priority | Next action |
| --- | --- | --- | --- | --- | --- |
| Built-in tool set | 🚧 | `docs/TOOLS_PARITY.md` maps core tools and known missing tool families. | Some Claude Code tools are intentionally absent or not yet prioritized. | P1 | Convert parity notes into targeted issue-sized rows before implementing more tools. |
| MCP tools/resources | 🚧 | `docs/TOOLS_PARITY.md` documents MCP discovery, tool naming, resource read/list behavior. | Needs failure-mode and long-running MCP regression tests. | P1 | Add MCP smoke tests with a local fixture server. |
| Skill invocation | 🚧 | `docs/SKILL_TOOL_PARITY.md` documents inline skill execution and unsupported fork/canonical behavior. | Forked skill execution, remote/canonical skills, hooks, and permission narrowing are not production-complete. | P1 | Decide whether to implement skill fork or explicitly de-scope in UI/docs. |
| Tool permissions deny rules | 🚧 | Permission deny parity is documented in `docs/TOOLS_PARITY.md`; runtime manager exists in `src-tauri/src/domain/permissions/`. | Durable audit and full skill permission behavior are incomplete. | P0 | Persist permission denials/approvals after schema plan + tests. |

## Security and Trust Boundaries

| Area | Status | Evidence | Gap | Priority | Next action |
| --- | --- | --- | --- | --- | --- |
| Permission manager | 🚧 | `src-tauri/src/domain/permissions/manager.rs` keeps rules, approvals, windows, denials, and recent denial records. | Recent denials are in-memory; durable audit trail is not yet authoritative. | P0 | Add persisted permission audit records with tests. |
| Bash command safety | 🚧 | `src-tauri/src/domain/tools/bash.rs` blocks some dangerous commands and sleep anti-patterns. | No comprehensive command policy matrix or persisted audit. | P0 | Add policy table and regression tests for destructive patterns. |
| Web fetch/search safety | 🚧 | `src-tauri/src/domain/tools/web_safety.rs` blocks private/loopback/internal targets and secret-like URLs. | Needs documented allow/deny policy and broader SSRF tests. | P0 | Expand security tests and document defaults. |
| Secrets management | ⚠️ | Provider secrets are configuration-driven across settings/config paths. | No IronClaw-style encrypted secret store/leak scanner boundary is documented as complete. | P1 | Design encrypted secret store and leak scan separately. |
| Tauri CSP | ⚠️ | `src-tauri/tauri.conf.json` currently has `csp: null`. | Needs compatibility-tested CSP for React/MUI/Monaco. | P1 | Add CSP only with app startup/build verification. |

## Execution Backends

| Area | Status | Evidence | Gap | Priority | Next action |
| --- | --- | --- | --- | --- | --- |
| Local execution | 🚧 | `src-tauri/src/execution/local.rs` implements local command environment behavior. | Needs clearer product safety defaults and tests against secret/env leakage. | P0 | Add local execution safety regression tests. |
| Docker execution | 🚧 | `src-tauri/src/execution/docker.rs` implements container execution with security args. | Needs repeatable Docker integration tests, cleanup guarantees, and network/resource policy validation. | P1 | Harden as first non-local backend. |
| SSH execution | 🚧 | `src-tauri/src/execution/ssh.rs` implements remote command/sync path. | Needs end-to-end fixture or mocked command tests for rsync/SSH boundaries. | P1 | Add config validation and path-safety tests. |
| Modal/Daytona/Singularity | 🔬 | Registered in `src-tauri/src/execution/mod.rs`; Modal/Daytona files identify unimplemented SDK/API portions. | User-facing UI must not imply production support. | P2 | Mark experimental/unavailable until implemented. |

## Engineering and Release Quality

| Area | Status | Evidence | Gap | Priority | Next action |
| --- | --- | --- | --- | --- | --- |
| Frontend unit tests | 🚧 | Vitest configured and frontend tests exist under `src/**/*.test.ts`. | CI gate was absent before this baseline. | P0 | Keep `npm test` in CI. |
| Rust unit/integration tests | 🚧 | Rust integration tests exist under `src-tauri/tests/`; dev-deps include `wiremock`/`tempfile`. | CI/fmt gate was absent before this baseline; strict clippy currently fails on existing warning/error debt. | P0 | Keep cargo fmt/test and non-blocking advisory clippy in CI; retire clippy debt before enabling clippy as a blocking gate. |
| Runtime validation | 🚧 | Real-provider ignored tests exist under `src-tauri/tests/real_*_harness.rs`; `scripts/real-llm-validation.sh all` passed on 2026-04-25 using `~/.omiga/omiga.yaml`; `scripts/mock-llm-validation.sh` wraps the CI-safe mock LLM planner harness; `orchestrationProjection.test.ts`, `OrchestrationTimelineList.test.tsx`, and `OrchestrationTraceList.test.tsx` protect headless projection/rendering/callback wiring. | No browser-rendered TaskStatus E2E yet; real-provider path requires secrets/network and is manual. | P0 | Add browser E2E for visible TaskStatus panels and transcript/payload interactions. |
| Coverage | ❌ | No committed coverage workflow/helper existed before this baseline. | Coverage remains optional and local until CI coverage is designed. | P2 | Use `scripts/coverage.sh` locally; add CI coverage later. |
| Release automation | ❌ | Tauri bundle config exists; no release workflow is currently present. | No signing/notarization/release artifact workflow. | P3 | Add after CI/E2E stabilize. |
