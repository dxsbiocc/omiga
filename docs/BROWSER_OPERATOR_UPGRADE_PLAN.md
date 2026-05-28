# Browser Operator Upgrade Plan

## Goal

Add a Codex-like, Omiga-native browser automation surface without routing through MCP by default. The model should receive stable first-party tools (`browser_open`, `browser_snapshot`, `browser_click`, `browser_fill`, `browser_screenshot`, `browser_close`) while Omiga owns lifecycle, permissions, telemetry, and UI state.

## Current Architecture Anchors

- Tool schema assembly: `src-tauri/src/commands/chat/mod.rs`
- Tool execution dispatcher: `src-tauri/src/commands/chat/tool_exec.rs`
- Native tool registry: `src-tauri/src/domain/tools/mod.rs`
- Existing facade precedent: `src-tauri/src/domain/computer_use/mod.rs`
- App/session runtime state: `src-tauri/src/app_state.rs`, `src-tauri/src/domain/chat_state.rs`
- Composer state/payload: `src/state/chatComposerStore.ts`, `src/state/sessionStore.ts`, `src/components/Chat/index.tsx`

## Target Architecture

```text
Omiga Agent
  -> browser_* facade tool schema
  -> chat/tool_exec.rs browser branch
  -> domain/browser_operator BrowserOperatorManager
  -> JSONL sidecar process
  -> browser-use Browser/BrowserSession/Actor over CDP
  -> Chrome/Chromium
```

The facade is the product contract. `browser-use` is only one backend adapter, so Omiga can later swap in Playwright/CDP or a Chrome extension without changing model-visible tools.

## MVP Scope

### Model-facing tools

1. `browser_open({ url, sessionId? })`
2. `browser_snapshot({ sessionId? })`
3. `browser_click({ target, sessionId? })`
4. `browser_fill({ target, value, sessionId? })`
5. `browser_screenshot({ sessionId?, fullPage? })`
6. `browser_close({ sessionId? })`

### Composer gate

Add `browserUseMode: "off" | "task" | "session"` parallel to `computerUseMode`.

- `off`: browser tools hidden/unavailable.
- `task`: browser tools exposed for this turn and reset after send.
- `session`: browser tools exposed for future turns in this session.

### Sidecar protocol

Line-delimited JSON over stdio:

```json
{"id":"1","method":"open","params":{"sessionId":"...","url":"https://example.com"}}
{"id":"1","ok":true,"result":{"url":"https://example.com","title":"Example"}}
```

Required methods: `health`, `open`, `snapshot`, `click`, `fill`, `screenshot`, `close`.

## Implementation Workstreams

### Backend facade owner

- Add `src-tauri/src/domain/browser_operator/mod.rs`.
- Add mode parser, facade tool enum, schemas, request preparation, response shaping, and redaction helpers.
- Add BrowserOperatorManager or equivalent stateful process/session adapter.
- Wire `browserUseMode` into `SendMessageRequest` in `chat/mod.rs`.
- Extend schema assembly when browser mode is enabled.
- Add `tool_exec.rs` dispatch branch before MCP/native fallback.

### Sidecar owner

- Add `src-tauri/browser-operator/browser_operator.py`.
- Implement robust JSONL loop and `--self-test`.
- Use defensive browser-use imports; return structured `browser_use_unavailable` instead of crashing.
- Honor `OMIGA_BROWSER_OPERATOR_CDP_URL` and `OMIGA_BROWSER_OPERATOR_HEADLESS`.

### Frontend owner

- Add `browserUseMode` to composer store and send payload.
- Reset `task` mode after sending; keep `session` mode for resume.
- Add minimal UI control separate from Computer Use.

### Test owner

- Rust unit tests for parser/schema/redaction.
- Python sidecar `py_compile` and `--self-test`.
- Frontend store/payload tests where existing harness allows.
- Full `bun run test` / targeted tests and Rust `cargo test` for browser_operator.

## Safety Requirements

- Do not expose browser tools unless `browserUseMode` is enabled.
- Do not log or echo `browser_fill.value` in tool result display/model output.
- Return structured errors; never panic on missing Python/browser-use.
- Add later allow/block domain policy before enabling remote/high-risk default use.
- Treat form submission and third-party communication as permission-sensitive; MVP should not add an explicit submit tool.

## Acceptance Criteria

- `browser_*` schemas appear only when Browser mode is enabled.
- Disabled mode returns a clear error if browser tools are called.
- `browser_fill` redacts value in UI/model-facing output.
- Sidecar can run `--self-test` without browser-use installed.
- Rust and frontend targeted tests pass.
- Existing user changes outside Browser Operator scope remain untouched.

## Implementation Status on `feature/browser-operator-facade`

- Branch created: `feature/browser-operator-facade`.
- Backend MVP implemented:
  - `browserUseMode` is parsed by `send_message`.
  - `browser_*` schemas are injected only when enabled.
  - `tool_exec.rs` dispatches `browser_*` through `BrowserOperatorManager`, not MCP.
  - Subagents receive `browser_use_enabled: false` for MVP containment.
- Sidecar MVP implemented:
  - `src-tauri/browser-operator/browser_operator.py`
  - `src-tauri/browser-operator/install_backend.py`
  - JSONL methods: `health`, `open`, `snapshot`, `click`, `fill`, `screenshot`, `close`.
  - Graceful `browser_use_unavailable` errors when `browser-use` is absent.
  - Managed install target: `~/.omiga/browser-operator/.venv`; Rust auto-detects it.
  - Tauri backend management commands: `browser_operator_backend_status`, `browser_operator_install_backend`.
- Frontend MVP implemented:
  - Composer/browser mode state and menu.
  - Browser backend is not installed by default; selecting `task`/`session` checks backend status and opens an on-demand install prompt when the managed backend is absent.
  - `task` mode resets after send; `session` mode is preserved for resume.
  - Tool cards redact `browser_fill.value`.
- Permission MVP implemented:
  - Browser tools are classified by risk.
  - `browser_fill` uses single-use approval and escalates probable secrets.
  - `browser_open` approval is host-scoped.
- Verified with targeted Rust, Python, TypeScript, and frontend test commands listed in the final report.

## Supervision Notes

This branch was created from a dirty working tree with existing unrelated modifications. All Browser Operator work must remain small, reviewable, and avoid reverting those files unless the change is explicitly related.
