# Computer Use Extension 分阶段实现计划

Status: V39 optional native visual OCR observe added；Python remains the user-facing Computer Use runtime；Rust sidecar is retained only as an internal experimental feature flag
Created: 2026-05-09
Scope: Omiga local-first desktop app；先做 macOS-only tracer bullet，再扩展真实本机自动化。

## Implementation progress

| Phase | Status | Evidence |
| --- | --- | --- |
| Phase 1 — 插件/MCP 基础设施修正 | Done | `computer-use` bundled plugin scaffold；plugin `.mcp.json` relative cwd rebasing test passed |
| Phase 2 — raw Computer MCP 过滤与防绕过 | Done | `mcp__computer__*` schema filtering + execution rejection tests passed |
| Phase 3 — Computer Use gate 与前端开启入口 | Done | Composer `Computer Use` Off/Task/Session toggle；`send_message.computerUseMode` 前后端透传；task 发送后自动回到 off；`bun run build` passed |
| Phase 4 — `computer_*` core facade 与 mock MCP 插件 | Done | `computer_*` schemas only inject when mode enabled；facade maps to internal `mcp__computer__*` backend；mock MCP sidecar smoke test passed |
| Phase 5 — 权限、限速、stop 与审计日志 | Done | `computer_*` 权限分级；observe/action budget；stop 阻断；project-local audit log；secret redaction tests passed |
| Phase 6 — 目标窗口锁定与动作前 revalidate | Done | action schema requires latest `observationId` + `targetWindowId`；core checks stale/missing observation, locked window, TTL, and click bounds；backend `validate_target` runs before actions |
| Phase 7 — Settings 与运行记录 UI | MVP Done | Settings → Computer Use tab persists allowed apps/log retention/screenshot preference；shows audit summary；clears project-local runs；composer menu can stop active core run；`computer_*` tool card hides typed text |
| Phase 8 — 真实 macOS backend | MVP Done | plugin sidecar now defaults to macOS real backend；observe uses fixed `osascript` + `screencapture`；validate rechecks frontmost target；click/type use fixed Accessibility actions；mock mode remains for deterministic tests |
| Phase 9 — 发布前硬化 | MVP Done | User-facing Computer Use docs added；target-lock schema regression tests；backend validation blocks unsafe/occluded targets；existing Rust/frontend Computer Use tests pass |
| Phase 10 — allowedApps 强制执行与权限状态检测 | MVP Done | Settings allowlist is injected into backend calls；sidecar blocks disallowed apps before screenshot/action；core turns disallowed targets into `app_not_allowed`；Settings shows Accessibility/Screen Recording status |
| Phase 11 — 高优先级安全修复 | Done | set_target activation precheck；permission status explicit trigger；UI stop best-effort reaches backend；missing target identity fail-closed |
| Phase 12 — Operation Flow 与 retention | Done | run.json auditVersion=2 flow aggregation；runs/tmp retention prune；Settings audit summary shows flows/actions/pruned bytes |
| Phase 13 — `computer_type` 剪贴板风险收敛 | Done | direct-keystroke first；probable secrets disable clipboard fallback；backend result reports method/clipboardFallbackUsed without echoing text |
| Phase 14 — Rust sidecar MVP 与 Python fallback | Done | `computer-use-sidecar` MCP stdio bin；Rust mock backend mirrors safety semantics；wrappers can opt into Rust sidecar |
| Phase 15 — Rust real observe/validate/set_target | Done | `OMIGA_COMPUTER_USE_BACKEND=mock|real|auto`；Rust macOS real mode supports observe/validate/set_target/stop；click/type explicitly unsupported in Rust real mode |
| Phase 16 — Rust observe screenshot parity | Done | Rust macOS real observe captures screenshots under `/tmp/omiga-computer-use/<runId>/` when enabled；screenshot permission/display failures stay structured and non-fatal |
| Phase 17 — Rust click/click_element parity | Done | Rust macOS real click/click_element revalidate target immediately before action；left-click only；element clicks resolve from same-process observation cache；stop blocks follow-up actions |
| Phase 18 — Rust type_text parity | Done | Rust macOS real type_text validates target first；direct-keystroke first；ordinary text clipboard fallback only when allowed；probable secrets disable clipboard fallback |
| Phase 19 — Rust sidecar install/status path | Done | `scripts/install-computer-use-sidecar.sh` builds/copies or reports Rust sidecar install status；default runtime remains Python；installed binary ignored from git |
| Phase 20 — Settings backend diagnostics | Done | `computer_use_backend_status` reports the user-facing Python backend and wrapper status；Settings displays it without permission probes or backend startup |
| Phase 21 — QA matrix 与 smoke 脚本 | Done | `docs/COMPUTER_USE_QA_MATRIX.md` documents scripted/manual/security coverage；`scripts/computer-use-smoke.py` runs Rust/Python mock and real-safe sidecar probes |
| Phase 22 — Release checklist aggregation | Done | `scripts/computer-use-release-check.py` aggregates low-risk release checks, mock smoke, install status, diff/rustfmt hygiene, and optional build/cargo/real-safe probes with JSON/Markdown reports |
| Phase 23 — Rust sidecar artifact integrity | Done | release check can verify installed bundled sidecar existence/executable bit/SHA-256/source-hash match and run the installed binary through Rust mock MCP smoke；install status reports source/destination SHA-256 |
| Phase 24 — Result-first evidence UI | Done | Settings no longer emphasizes flows/actions or detailed screenshots；it shows result-record counts, evidence size, retention, and the local evidence path only |
| Phase 25 — Result status signals | Done | audit summary aggregates result status counts；Settings shows OK/Needs attention/Blocked/Stopped without exposing action timelines or payloads |
| Phase 26 — Packaging config release check | Done | release check verifies Tauri resources, marketplace entry, plugin manifest, MCP config, wrappers, executable bits, and reports generated packaging artifacts |
| Phase 27 — Final packaging hygiene gate | Done | release check supports `--fail-on-generated-artifacts` to block final packaging when bundled plugin resources contain `__pycache__` or `.pyc` files |
| Phase 28 — Generated artifact cleanup | Done | removed generated `__pycache__`/`.pyc` from the bundled computer-use plugin and verified `--fail-on-generated-artifacts` passes |
| Phase 29 — Python-first runtime / Rust internal feature | Done | Settings shows Python backend only；wrappers require `OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1` before honoring Rust sidecar opt-in；release gate defaults to Python mock |
| Phase 30 — Real observe strict QA gate | Done | `computer-use-smoke.py` and release-check support `--require-real-observe` so permission-ready QA machines can fail the gate unless real macOS observe succeeds；default real-safe probes still validate fail-closed behavior |
| Phase 31 — Real AX element observation | Done | User-facing Python macOS backend now returns bounded real Accessibility elements from the frontmost window and click_element resolves returned element ids；`computer_click` schema no longer advertises unsupported right/middle buttons |
| Phase 32 — Controlled positive dialog E2E | Done | Smoke/release gate can open a temporary macOS dialog, type a unique token, click the observed OK element, and verify the returned text for Python user runtime plus optional Rust parity |
| Phase 33 — Fixed non-text key action | Done | `computer_key` facade maps to backend `key_press`; Python and Rust sidecars validate the locked target then emit fixed key-code snippets for Enter/Tab/Escape/arrows/paging/delete/home/end/space；mock and real dialog key E2E suites cover the path |
| Phase 34 — Fixed scroll-wheel action | Done | `computer_scroll` facade maps to backend `scroll`; Python and Rust sidecars validate the locked target then emit fixed CoreGraphics scroll-wheel events at the target center；mock and TextEdit scroll E2E suites verify the path |
| Phase 35 — Distribution hardening preflight | Done | release-check can write/verify checksum manifests for packaged Computer Use artifacts and run macOS signing/notarization preflight for codesign/notarytool/stapler plus Tauri plist/entitlements config |
| Phase 36 — Fixed shortcut action | Done | `computer_shortcut` facade maps to backend `shortcut`; Python and Rust sidecars validate the locked target then emit only whitelisted shortcuts (`select_all`, `undo`, `redo`)；mock blocks unsupported combos and TextEdit shortcut E2E verifies positive select-all replacement |
| Phase 37 — Fixed drag gesture | Done | `computer_drag` facade maps to backend `drag`; Python and Rust sidecars validate the locked target, reject non-left buttons/out-of-window endpoints, then emit a fixed left-button mouse down/drag/up gesture；mock and TextEdit window-move E2E verify the path |
| Phase 38 — Semantic AX observe metadata | Done | Python and Rust observe now preserve bounded AX depth/count while adding semantic hints per element (`kind`, `parentId`, `depth`, `roleDescription`, bounded text preview, focus/enabled/selected/expanded flags, `interactable`)；mock/real-safe smoke verifies active semantic metadata |
| Phase 39 — Optional visual OCR observe | Done | `extractVisualText=true` makes Python and Rust observe capture a screenshot, run native macOS Vision OCR through a fixed helper, return bounded `visualText` boxes/errors, and verify the path with TextEdit OCR E2E；default observe remains non-OCR |
| Phase 40+ | Deferred | actual codesign/notarization submission、full semantic AX tree/richer drag-and-drop/visual model image input only when tied to concrete release/user value |

## 0. 已锁定产品与架构决策

### 0.1 产品边界

Computer Use 是 Omiga 的一等本机自动化能力，但 OS-specific backend 必须作为可选插件提供。

已确认：

- 通用 MCP 工具继续使用现有 `mcp__server__tool` 命名。
- Computer Use 对模型暴露稳定的一等工具：`computer_*`。
- `computer_*` 是 Omiga core facade，负责 gate、权限、审计、stop、目标窗口校验。
- 实际后端由可选插件 `computer-use` 提供。
- 插件名为 `computer-use`，不加 `macos` 后缀；macOS 只是第一版 target。
- 插件安装/启用不等于当前任务可使用。
- 只有当前任务/会话显式开启 Computer Use 后才向模型暴露 `computer_*`。
- 第一版先做 tracer bullet，不直接做完整 macOS 自动化。

### 0.2 总体架构

```text
LLM
  sees: computer_observe / computer_set_target / computer_click / computer_drag / computer_type / computer_key / computer_scroll / computer_shortcut / computer_stop
        ↓
Omiga core facade
  gate → permission → audit → target-window validation → stop/cancel checks
        ↓
Internal MCP route
  server: computer
  tools: observe / set_target / click / click_element / drag / type_text / key_press / scroll / shortcut / stop / validate_target
        ↓
computer-use plugin sidecar
        ↓
macOS APIs / Accessibility / screenshot / input backend
```

### 0.3 命名原则

| 场景 | 命名 |
| --- | --- |
| 通用动态 MCP 工具 | `mcp__server__tool` |
| Computer Use 产品能力 | `computer_*` |
| 插件 ID | `computer-use` |
| MCP server key | `computer` |
| 平台产物 | `bin/computer-use` wrapper → `bin/darwin-arm64/computer-use` 或 `bin/darwin-x64/computer-use` |

`mcp__computer__*` 是内部路由，不应作为模型主工具面。直接调用 raw `mcp__computer__*` 应被拒绝，提示使用 `computer_*` facade。

---

# 1. 分阶段实施路线图

## Phase 1 — 插件/MCP 基础设施修正

### 目标

先修好 Computer Use 所依赖的插件扩展点，避免后续能力绑定到错误路径或错误命名。

### 范围

1. 确认 bundled plugin 使用当前目录结构：

```text
src-tauri/bundled_plugins/plugins/computer-use/
  plugin.json
  .mcp.json
  bin/
    computer-use
    darwin-arm64/computer-use
    darwin-x64/computer-use
```

2. 支持 plugin `.mcp.json` 中的相对路径以 plugin root 为基准解析，而不是 project root。MVP 先让 `.mcp.json` 指向 `./bin/computer-use` wrapper，wrapper 再选择当前 macOS 架构产物。
3. 为 `computer-use` 插件增加 marketplace entry。
4. 先不实现真实 macOS backend，只保证插件可被发现、安装、启用。

### 主要文件

```text
src-tauri/src/domain/plugins.rs
src-tauri/src/domain/mcp/config.rs
src-tauri/src/domain/mcp/client.rs
src-tauri/bundled_plugins/marketplace.json
src-tauri/bundled_plugins/plugins/computer-use/plugin.json
src-tauri/bundled_plugins/plugins/computer-use/.mcp.json
```

### 交付物

- `computer-use` 插件出现在插件市场。
- 插件可安装/启用/禁用。
- 插件 MCP server `computer` 可被 merged MCP config 发现。
- 相对 command/cwd 解析正确。

V4 status: implemented.

### 验收标准

- 安装并启用 `computer-use` 后，`merged_mcp_servers()` 包含 `computer`。
- `./bin/.../computer-use` 相对插件目录解析。
- 插件禁用后，`computer` MCP server 不再生效。

---

## Phase 2 — raw Computer MCP 过滤与防绕过

### 目标

保留通用 MCP 机制，但阻止 Computer Use backend 的 raw MCP 工具绕过 core facade。

### 范围

1. 识别 reserved MCP tools：

```text
mcp__computer__observe
mcp__computer__click
mcp__computer__click_element
mcp__computer__drag
mcp__computer__type_text
mcp__computer__key_press
mcp__computer__scroll
mcp__computer__shortcut
mcp__computer__stop
mcp__computer__validate_target
```

2. 从模型可见 MCP tool list 中过滤 `mcp__computer__*`。
3. 如果执行层收到 raw `mcp__computer__*`，直接拒绝，不做静默重定向。
4. 错误提示应说明：Computer Use 必须通过 `computer_*` 工具调用。

### 主要文件

```text
src-tauri/src/domain/mcp/tool_pool.rs
src-tauri/src/domain/mcp/names.rs
src-tauri/src/commands/chat/mod.rs
src-tauri/src/commands/chat/tool_exec.rs
```

### 交付物

- `mcp__computer__*` 不再进入模型工具列表。
- raw `mcp__computer__*` 调用被统一拒绝。

V4 status: implemented.

### 验收标准

- MCP server `computer` 启用时，模型仍看不到 `mcp__computer__click`。
- 直接构造 raw tool call 调用 `mcp__computer__click` 返回错误。
- 其他普通 MCP 工具不受影响。

---

## Phase 3 — Computer Use gate 与前端开启入口

### 目标

插件启用只是安装能力；当前任务/会话显式开启后才暴露 `computer_*`。

### 范围

1. 新增模式：

```ts
type ComputerUseMode = "off" | "task" | "session";
```

2. Chat composer 增加入口：

```text
Computer Use: Off / This task / This session
```

3. 自然语言触发只弹确认，不直接开启。
4. `send_message` 请求携带 Computer Use mode。
5. 后端根据 mode 决定是否注入 `computer_*` tool schemas。

### 主要文件

```text
src/state/computerUseStore.ts
src/components/Chat/ComputerUseToggle.tsx
src/components/Chat/ChatComposer.tsx
src-tauri/src/commands/chat/commands.rs
src-tauri/src/commands/chat/mod.rs
src-tauri/src/domain/tools/mod.rs
```

### 交付物

- UI 可选择 Off / This task / This session。
- mode=off 时不暴露 `computer_*`。
- mode=task/session 时才暴露 `computer_*`。

V6 status: request gate, UI mode plumbing, and model-visible `computer_*`
schema injection are implemented. Schemas are injected only when Computer Use
mode is `task` or `session`.

### 验收标准

- 插件启用但 mode=off：模型工具列表不包含 `computer_*`。
- mode=task：当前消息包含 `computer_*`。
- task 结束后恢复 off 或不继续暴露。
- mode=session：同会话后续消息继续暴露，直到用户关闭。

---

## Phase 4 — `computer_*` core facade 与 mock MCP 插件

### 目标

先跑通完整调用链，不碰真实 macOS API。

V6 status: mock MCP sidecar is implemented under `computer-use/bin/*`; core
facade tools are implemented and route through the guarded MCP bridge.

### 范围

1. 增加 model-visible tools：

```text
computer_observe
computer_set_target
computer_click
computer_click_element
computer_type
computer_key
computer_scroll
computer_stop
```

2. Facade 内部路由到 MCP server `computer`。
3. 新增 mock `computer-use` MCP sidecar 或 mock binary path。
4. mock backend 返回固定 observation 和动作结果。

### 主要文件

```text
src-tauri/src/domain/computer_use/mod.rs
src-tauri/src/domain/computer_use/types.rs
src-tauri/src/domain/computer_use/mcp_bridge.rs
src-tauri/src/domain/tools/computer_observe.rs
src-tauri/src/domain/tools/computer_set_target.rs
src-tauri/src/domain/tools/computer_click.rs
src-tauri/src/domain/tools/computer_click_element.rs
src-tauri/src/domain/tools/computer_type.rs
src-tauri/src/domain/tools/computer_stop.rs
src-tauri/src/domain/tools/mod.rs
src-tauri/bundled_plugins/plugins/computer-use/bin/.../computer-use
```

### Mock observation 示例

```json
{
  "observationId": "obs_mock_1",
  "screenshotPath": null,
  "screenSize": [1400, 900],
  "frontmostApp": "Omiga",
  "activeWindowTitle": "Mock Window",
  "target": {
    "bundleId": "com.omiga.desktop",
    "pid": 1,
    "windowId": 1,
    "bounds": [0, 0, 1400, 900]
  },
  "targetVisible": true,
  "occluded": false,
  "safeToAct": true,
  "elements": [
    {
      "id": "button-save",
      "role": "button",
      "label": "Save",
      "bounds": [100, 100, 80, 32]
    }
  ]
}
```

### 交付物

- `computer_observe` 可返回 mock observation。
- `computer_click_element` 可通过 facade 调到 mock MCP backend。
- `computer_type` 可通过 facade 调到 mock MCP backend。

### 验收标准

- 完整链路：LLM tool call → core facade → MCP bridge → mock sidecar → tool result。
- raw MCP 仍被拒绝。
- 没有真实 macOS 权限也能跑通 tracer bullet。

---

## Phase 5 — 权限、限速、stop 与审计日志

### 目标

把 Computer Use 的高风险特性纳入 Omiga 现有权限与审计系统。

V5 status: implemented as tracer-bullet policy in `domain::computer_use` plus
PermissionManager risk classification. Audit logs are currently project-local
under `<project>/.omiga/computer-use/runs/...` to keep development/test runs
self-contained; this can be made user-level/configurable in Phase 7 Settings.

### 范围

1. 权限风险等级：

```text
computer_observe        medium / privacy
computer_set_target     medium / system
computer_click          medium / system
computer_click_element  medium / system
computer_type           high / privacy
computer_stop           safe
```

2. 自动化限速：

```text
必须先 observe
最多 5 个动作后重新 observe
最多 15 个动作后汇报/确认继续
默认运行 5 分钟超时
高风险动作单独确认
```

3. stop：

- core 设置 run stop token。
- 后续动作拒绝。
- 尝试调用 backend `stop`。

4. 审计日志：

```text
<project>/.omiga/computer-use/runs/{sessionId}/{runId}/
  run.json
  actions.jsonl
  observations/
    obs_xxx.json
    obs_xxx.png
```

5. 敏感内容脱敏：

```text
sk-...
ghp_...
AKIA...
-----BEGIN PRIVATE KEY-----
password=
token=
api_key=
```

### 主要文件

```text
src-tauri/src/domain/permissions/tool_rules.rs
src-tauri/src/domain/permissions/manager.rs
src-tauri/src/domain/computer_use/policy.rs
src-tauri/src/domain/computer_use/audit.rs
src-tauri/src/domain/computer_use/session.rs
src-tauri/src/commands/computer_use.rs
src/components/permissions/PermissionPromptBar.tsx
```

### 交付物

- Computer Use 工具进入权限系统。
- action log 写入本地。
- 输入文本日志脱敏。
- stop 阻断后续动作。

### 验收标准

- `computer_type` 触发 high risk 权限提示。
- 疑似 secret 输入必须单独确认。
- 审计日志不保存完整敏感文本。
- stop 后再 click/type 返回 stopped error。

---

## Phase 6 — 目标窗口锁定与动作前 revalidate

### 目标

解决“截图后被其他软件覆盖/抢焦点，旧坐标点错窗口”的核心安全问题。

V6 status: implemented in the core policy path. `computer_click`,
`computer_click_element`, and `computer_type` must include the latest
`observationId` and `targetWindowId`; stale/missing observations, target-window
mismatch, expired observations, and out-of-bounds clicks are rejected before
backend execution. Actions that pass core policy call backend
`mcp__computer__validate_target` before the actual click/type backend tool.

### 范围

1. Computer Use session 绑定目标：

```rust
ComputerUseSession {
  run_id,
  target_app,
  target_bundle_id,
  target_pid,
  target_window_id,
  target_bounds,
  last_observation_id,
  action_count,
  started_at,
  stop_token
}
```

2. 动作入参必须包含：

```json
{
  "observationId": "obs_123",
  "targetWindowId": 67890
}
```

3. 动作前校验：

```text
observation 是否存在且未过期
当前 gate 是否仍开启
run 是否未 stop
frontmost app/window 是否仍为目标
window bounds 是否未变化
目标是否未被遮挡
点击点是否在目标窗口内
```

4. 校验失败时不执行动作，返回：

```json
{
  "ok": false,
  "requiresObserve": true,
  "reason": "target_window_changed"
}
```

5. 跨 App 流程必须显式调用 `computer_set_target`。

### 主要文件

```text
src-tauri/src/domain/computer_use/session.rs
src-tauri/src/domain/computer_use/policy.rs
src-tauri/src/domain/computer_use/mcp_bridge.rs
src-tauri/src/domain/tools/computer_click.rs
src-tauri/src/domain/tools/computer_click_element.rs
src-tauri/src/domain/tools/computer_type.rs
```

### 交付物

- target window 状态在 core 中持有。
- click/type 之前强制 validate_target。
- target 改变时动作被拒绝。

### 验收标准

- observe 后模拟 target 变更，click 被拒绝。
- observe 过期后 click 被拒绝。
- click 不带 observationId 被拒绝。
- set_target 不在 allowlist 时需授权。

---

## Phase 7 — Settings 与运行记录 UI

### 目标

让用户可理解、配置、清理 Computer Use，而不是隐式运行。

V7 status: MVP implemented. Settings now has a Computer Use tab for plugin
entry, platform status, allowed-app/log-retention/screenshot preferences, audit
summary, and run cleanup. The composer Computer Use menu has Settings and Stop
actions; Stop marks the active core run as stopped so subsequent local UI
actions are rejected. `ToolCallCard` renders `computer_*` summaries and hides
the full `computer_type.text` payload.

### 范围

1. Settings 增加 Computer Use 配置区：

```text
启用/禁用插件
当前平台支持状态
macOS 权限状态
allowedApps
日志保留天数
是否保存截图
清理运行记录
```

2. Chat UI 显示：

```text
当前 Computer Use mode
当前目标 App/window
最近 observation 缩略图或元数据
Stop 按钮
```

3. ToolCallCard 对 `computer_*` 做专门摘要。

### 主要文件

```text
src/components/Settings/ComputerUseSettingsTab.tsx
src/components/Settings/index.tsx
src/state/computerUseStore.ts
src/components/Chat/ComputerUseToggle.tsx
src/components/Chat/ToolCallCard.tsx
src/components/Chat/AssistantTraceItem.tsx
```

### 交付物

- 用户可配置 allowedApps。
- 用户可清理本地 Computer Use 数据。
- Chat 中可停止当前 Computer Use run。

### 验收标准

- allowedApps 持久化。
- 日志保留设置持久化。
- Stop 按钮对 active run 生效。
- computer tool card 不泄露完整输入文本。

---

## Phase 8 — 真实 macOS backend

### 目标

替换 mock sidecar，实现真实 macOS observe/click/type。

V8 status: MVP implemented in the optional `computer-use` plugin sidecar.
`bin/computer-use` still dispatches by macOS architecture, but both arch
wrappers now run a shared `computer-use-macos.py` backend. The sidecar defaults
to a real macOS backend on Darwin and keeps
`OMIGA_COMPUTER_USE_BACKEND=mock` for CI/smoke tests. Real mode uses only fixed
internal `osascript` snippets plus `screencapture`/`pbcopy`/`pbpaste`; it does
not execute model-provided scripts.

MVP limitations: AX tree is shallow (`active-window` element only), occlusion
detection is conservative (frontmost target mismatch blocks actions), left-click
only, and `computer_type` uses controlled clipboard paste with restore.

### 范围

Backend tool 实现：

```text
observe:
  screenshot
  frontmost app/window
  active window bounds/title
  focused element
  shallow AX tree

set_target:
  activate app
  bind target window

validate_target:
  verify frontmost app/window/bounds/occlusion

click / click_element:
  revalidate target
  perform click

type_text:
  revalidate target
  type text or controlled clipboard paste

stop:
  halt backend pending work
```

候选依赖放在 sidecar，尽量不放 main Omiga core：

```text
enigo
xcap
arboard
objc2 / core-foundation / accessibility
```

### 主要文件

```text
src-tauri/bundled_plugins/plugins/computer-use/bin source or sidecar crate
src-tauri/Cargo.toml 或独立 sidecar Cargo.toml
src-tauri/Info.plist
src-tauri/Entitlements.plist
```

macOS 权限：

```text
Accessibility
Screen Recording
Apple Events only for fixed internal allowlisted calls, not model-provided scripts
```

### 交付物

- macOS 真机可截图、读取前台窗口信息、点击、输入。
- AX metadata 可返回 shallow elements。
- 目标窗口被覆盖/失焦时不执行动作。

### 验收标准

- 在 Omiga 自身窗口执行 observe/click/type smoke test。
- 切换到其他 App 后旧 click 被拒绝。
- 未授权 Screen Recording/Accessibility 时返回可操作错误提示。

---

## Phase 9 — 发布前硬化

### 目标

完成测试、文档和安全硬化，准备进入可用版本。

V9 status: MVP hardening implemented. Added `docs/COMPUTER_USE_EXTENSION.md`
with install/enable, macOS permission, safety-boundary, audit-cleanup, and MVP
limitation guidance. Added regression tests that action schemas require
`observationId` + `targetWindowId`, and backend validation blocks unsafe or
occluded targets. Full release QA, live thumbnails, and deeper AX/OCR remain
deferred.

### 范围

1. 增加自动化测试：

```text
computer gate schema injection
raw mcp__computer__* filtering
raw mcp__computer__* execution rejection
permission risk classification
target validation transitions
audit redaction
stop token behavior
frontend toggle/settings/tool-card tests
```

2. 增加文档：

```text
用户如何安装/启用 Computer Use
macOS 权限说明
安全边界说明
哪些动作不会自动执行
如何清理本地日志
```

3. 安全硬化：

```text
默认只允许 Omiga
疑似敏感输入二次确认
不保存完整 secret
禁止 raw backend path
动作限速
运行超时
```

### 验收标准

- `bun run test` 通过。
- `bun run build` 通过。
- Rust 单测通过。
- Computer Use smoke test 通过。
- 文档说明安全边界。

---

## Phase 10 — allowedApps 强制执行与权限状态检测

### 目标

让 Settings 中的安全配置进入实际执行路径，并让用户看到 macOS 权限状态。

V10 status: MVP implemented. `omiga.computer_use.settings.v1` is parsed by
core, injected into backend calls as `allowedApps` and `saveScreenshot`, and
the macOS sidecar blocks disallowed apps before screenshot/action. Core also
sanitizes disallowed backend targets into a structured `app_not_allowed` result
with `requiresSettingsChange`. Settings → Computer Use now shows
Accessibility and Screen Recording status.

### 范围

1. allowedApps 强制执行：

```text
Settings allowedApps
  ↓
core loads settings
  ↓
backend args include allowedApps/saveScreenshot
  ↓
sidecar observe/validate/action blocks target not in allowlist
  ↓
core converts disallowed targets to app_not_allowed
```

2. macOS 权限状态检测：

```text
Accessibility: fixed System Events probe
Screen Recording: fixed screencapture probe
```

3. saveScreenshots 配置进入 sidecar；默认不保存 screenshot。

### 验收标准

- allowed app 可 observe/action。
- blocked app 返回 `app_not_allowed`。
- `saveScreenshots=false` 时 observe 不主动保存 screenshot。
- Settings 显示 Accessibility / Screen Recording 状态。

---

## Phase 11 — 安全审查修复：预激活 allowlist、显式权限探测、停止同步

### 目标

修复安全审查中优先级最高的一组问题：不能为了“检查目标”先激活未授权 App，Settings 不能在打开页面时自动触发截图权限探测，用户点击 Stop 时需要同时通知后端 sidecar。

V11 status: implemented. `computer_set_target` now checks the exact activation
identifier against `allowedApps` before running AppleScript activation. Settings
only calls `computer_use_permission_status` after the user clicks “检测权限”.
`computer_use_stop_active_run` marks the core run stopped and then best-effort
calls backend `mcp__computer__stop` with the current runId.

### 范围

1. set_target 预检：

```text
model args(appName/bundleId)
  ↓
sidecar checks exact activation identifier against allowedApps
  ↓
blocked app returns app_not_allowed before activate
  ↓
successful activation still observe/revalidates actual target
```

2. Settings 权限探测显式触发：

```text
open Settings → load settings/audit only
click 检测权限 → Accessibility probe + one-time Screen Recording probe
```

3. Stop 同步：

```text
composer Stop
  ↓
core stop_active_run(sessionId)
  ↓
best-effort backend mcp__computer__stop(runId)
  ↓
UI mode off even if backend plugin is unavailable
```

4. Core allowlist fail-closed：

```text
observe/set_target/validate_target result missing target identity
  ↓
blocked as target_identity_missing

post-action click/type result missing target identity
  ↓
allowed only because validate_target already enforced target identity
```

### 验收标准

- `computer_set_target` 对 blocked app 不会先激活再拦截。
- Settings 首次加载不会调用 `screencapture`。
- Stop 后 core 与 backend 都收到停止意图；backend 不可用时不影响本地阻断。
- validation 返回 `currentTarget` 时仍执行 allowlist；缺少 target identity 时 fail-closed。

---

## Phase 12 — Operation Flow 与保留期清理

### 目标

避免 Computer Use 每次运行长期堆积不可管理的本地文件；把逐条动作记录提升为 run 级 Operation Flow 摘要，同时让 Settings 中的“日志保留天数”真正进入清理路径。

V12 status: implemented. Each run still keeps redacted `actions.jsonl` for
short-term forensic detail, but `run.json` is now upgraded to `auditVersion: 2`
and contains a compact `flow` summary: status, action counters, tool counters,
last tool, last error, last target, first/last action timestamps. Retention is
applied from `logRetentionDays` during Computer Use execution and Settings audit
refresh. Old run directories and old `/tmp/omiga-computer-use/*` screenshot
directories are pruned.

### 范围

1. Operation Flow 摘要：

```text
record_facade_result / record_policy_rejection
  ↓
append actions.jsonl
  ↓
upsert run.json.flow
```

`run.json.flow` 包含：

```text
flowVersion
status active | stopped | blocked | needs_attention
actionCount / okActionCount / failedActionCount
policyRejectionCount
toolCounts
target
lastTool / lastError
firstActionAt / lastActionAt
```

2. 保留期清理：

```text
Settings logRetentionDays
  ↓
core ComputerUseSettings
  ↓
prune old .omiga/computer-use/runs/**/run.json directories
  ↓
prune old /tmp/omiga-computer-use/<runId> screenshot dirs
```

3. Settings 展示：

```text
Result records / Evidence size / Retention / Evidence path
清理了多少旧 run、临时截图目录、释放多少空间
不展开截图、逐步操作流程或输入内容
```

### 验收标准

- 每个新 run 的 `run.json` 包含 `flow` 摘要，不再只有静态元数据。
- `logRetentionDays` 会删除超过保留期的 run 目录。
- Settings 刷新记录时会应用保留期并展示清理结果。
- Computer Use 执行路径会 best-effort 应用保留期，失败不影响工具执行。
- 临时截图目录按保留期清理，不再无限增长。

---

## Phase 13 — `computer_type` 剪贴板风险降低

### 目标

降低本机输入文本时被剪贴板历史工具、同步剪贴板或其它本机观察者记录的风险。剪贴板不再是默认路径，而是普通文本的 fallback；疑似 secret/token/password 禁用剪贴板 fallback。

V13 status: implemented. macOS sidecar now tries fixed System Events direct
keystrokes first, including newline/tab handling. Clipboard paste is only used
when direct typing is unsupported or fails and `allowClipboardFallback=true`.
Core injects `allowClipboardFallback=false` for probable secrets before calling
the backend. Settings and docs now state the direct-type-first behavior and
clipboard-history risk.

### 范围

1. sidecar 输入路径：

```text
computer_type
  ↓
validate target
  ↓
direct System Events keystrokes
  ↓
if ordinary text only: controlled clipboard paste fallback
  ↓
restore previous clipboard
```

2. secret 保护：

```text
core detects probable secret/token/password
  ↓
backend arg allowClipboardFallback=false
  ↓
direct typing fails/unsupported
  ↓
return safe error instead of placing text on clipboard
```

3. 输出与审计：

```text
typed text remains hidden/redacted
backend result exposes method + clipboardFallbackUsed only
```

### 验收标准

- 普通短文本使用 direct keystroke。
- 普通长文本/特殊文本可 fallback 到受控剪贴板。
- 疑似 secret 禁用剪贴板 fallback。
- backend result 不回显 typed text。
- Settings/docs 告知剪贴板 fallback 风险。

---

## Phase 14 — Rust sidecar MVP 与 Python fallback

### 目标

降低正式发布对系统 Python 的长期依赖。先不一次性迁移真实 macOS AX/CoreGraphics 操作，而是把 MCP stdio 协议和 mock backend 用 Rust 落地，证明 Computer Use backend 可以在同一工具协议下由 Rust 进程承载。

V14 status: implemented. A new Cargo bin `computer-use-sidecar` implements the
Computer Use MCP server protocol, `tools/list`, and a Rust mock backend with
the same safety semantics as the Python mock: allowedApps checks,
set_target activation-identifier precheck, stop state, direct-type-first
`type_text`, and disabled clipboard fallback for sensitive input. The bundled
platform wrappers continue to use the Python macOS real backend by default, but
can opt into an installed Rust sidecar via `OMIGA_COMPUTER_USE_SIDECAR=rust`.

### 范围

1. Rust sidecar MVP：

```text
src-tauri/src/bin/computer-use-sidecar.rs
  ↓
MCP Content-Length stdio loop
  ↓
initialize / tools/list / tools/call
  ↓
mock observe/set_target/validate/click/type/stop
```

2. 包装器策略：

```text
default
  ↓
Python macOS real backend remains active

OMIGA_COMPUTER_USE_SIDECAR=rust + bin/computer-use-sidecar installed
  ↓
Rust sidecar handles backend
```

3. 不进入本阶段：

```text
真实 macOS observe/click/type 的 Rust AX/CoreGraphics 移植
codesign/notarization packaging
自动复制 Rust sidecar release binary 到 plugin/bin
```

### 验收标准

- `cargo build --bin computer-use-sidecar` 通过。
- Rust sidecar 能完成 initialize/tools/list/tools/call。
- Rust mock observe 返回 target identity。
- Rust mock set_target 能拦截 appName/bundleId smuggle。
- Rust mock type_text 保持 direct-first / secret 禁用 fallback。
- 默认 Python real backend 行为不变。

---

## Phase 15 — Rust sidecar real observe/validate/set_target

### 目标

继续降低长期 Python 依赖，但不为了“统一语言”牺牲安全边界：Rust sidecar 先接管真实 macOS 的只读/低风险路径，让真实目标识别、目标校验、目标激活在 Rust MCP 进程内跑通；click/type 等高风险动作仍留在 Python real backend，直到 Rust 侧完成同等目标锁定、权限、审计和输入安全语义。

V15 status: implemented. `computer-use-sidecar` now supports
`OMIGA_COMPUTER_USE_BACKEND=mock|real|auto`. Explicit `mock` keeps deterministic
test behavior. `real` uses macOS System Events for `observe`, `validate_target`,
and `set_target`. `auto` uses the real backend on macOS and mock elsewhere.
Rust real mode refuses `click`, `click_element`, and `type_text` with a
structured `unsupported_real_tool` result instead of mixing real observation
with mock actions.

### 范围

1. Backend mode：

```text
OMIGA_COMPUTER_USE_BACKEND=mock
  → Rust mock backend

OMIGA_COMPUTER_USE_BACKEND=real
  → Rust macOS metadata/target backend

OMIGA_COMPUTER_USE_BACKEND=auto
  → macOS real observe/validate/set_target；非 macOS mock
```

2. Rust real macOS capabilities：

```text
observe
  → query frontmost app/window metadata through fixed osascript
  → enforce allowedApps before returning safe target
  → screenshot intentionally unsupported in this phase

validate_target
  → re-query frontmost target
  → compare targetWindowId
  → reject coordinate outside target bounds

set_target
  → precheck exact appName/bundleId against allowedApps
  → activate by fixed AppleScript only after precheck
  → observe activated target
```

3. 不进入本阶段：

```text
Rust screenshot capture
Rust click/click_element
Rust type_text direct/clipboard implementation
AX tree/OCR
release packaging/codesign automation
```

### 验收标准

- `cargo build --manifest-path src-tauri/Cargo.toml --bin computer-use-sidecar` 通过。
- Rust mock mode 仍可完成 initialize/tools/list/tools/call。
- Rust real mode 在缺少 macOS Accessibility 权限时返回结构化 `macos_permission_or_window_query_failed`，不执行动作。
- Rust real mode `click`/`click_element`/`type_text` 返回 `unsupported_real_tool`，不 fallback 到 mock。
- 默认 Python real backend 行为不变。

---

## Phase 16 — Rust observe screenshot parity

### 目标

补齐 Rust real `observe` 与 Python backend 的截图能力，让 Rust sidecar 在只读观察阶段已经可以产生真实截图文件，同时保持截图失败非致命：窗口 metadata 可用时仍返回 observation，截图权限或显示捕获错误放入结构化字段，绝不继续执行 click/type。

V16 status: implemented. Rust real `observe` now reads `runId`, writes
`screencapture -x -t png` output to `/tmp/omiga-computer-use/<runId>/<observationId>.png`,
and returns `screenshotPath` when capture succeeds. If macOS blocks screen
capture or display capture fails, the result remains `ok=true` for metadata but
includes `screenshotError` and `screenshotRequiresPermission`.

### 范围

1. 截图落点对齐 Python backend：

```text
computer_observe(saveScreenshot=true)
  ↓
Rust real observe
  ↓
/tmp/omiga-computer-use/<runId>/<observationId>.png
  ↓
same retention root used by core cleanup
```

2. 结构化截图失败：

```text
frontmost target metadata ok
  ↓
screencapture fails
  ↓
return ok=true observation
  + screenshotPath=null
  + screenshotError
  + screenshotRequiresPermission
```

3. 安全保持：

```text
allowedApps check happens before screenshot
Rust real click/type remain unsupported_real_tool
runId / observationId path components are sanitized
```

### 验收标准

- Rust real `observe(saveScreenshot=true)` 成功时返回存在的 `screenshotPath`。
- 截图失败时返回结构化错误，metadata observation 不被误判为 action-safe failure。
- 截图目录继续落在 `/tmp/omiga-computer-use`，可被现有 retention 清理。
- Rust mock mode 行为不变。
- Rust real mode 仍不支持 click/type，避免高风险动作提前迁移。

---

## Phase 17 — Rust click/click_element parity

### 目标

把 Rust real backend 的点击动作推进到 Python backend 同等安全边界，但仍不迁移文本输入。点击是本机可见副作用动作，因此必须坚持“动作前重新验证”：即使 core 已经做过 observation TTL、targetWindowId、bounds、action budget 检查，sidecar 仍要重新读取当前前台目标并确认 targetWindowId 与坐标范围。

V17 status: implemented. Rust real mode now supports `click` and
`click_element`. `click` accepts left-click only, requires numeric `x/y`, calls
`validate_target` immediately before running the fixed System Events click, and
returns structured permission/safety errors. `click_element` resolves element
center from the same sidecar process's observation cache, then delegates to the
same validated click path. `type_text` remains `unsupported_real_tool`.

### 范围

1. `click` 安全路径：

```text
computer_click args
  ↓
button must be left/empty
  ↓
x/y must be numeric
  ↓
Rust real validate_target
    - re-query frontmost app/window
    - allowedApps enforced by observe
    - targetWindowId must match
    - coordinate must be inside current target bounds
  ↓
fixed AppleScript click at {round(x), round(y)}
```

2. `click_element` 安全路径：

```text
computer_click_element(args observationId, elementId)
  ↓
lookup elements from same-process observation cache
  ↓
compute element center
  ↓
delegate to click path
```

3. 不进入本阶段：

```text
Rust type_text
right/middle/double click
drag gestures
deep AX element tree
cross-process persisted observation cache
```

### 验收标准

- Rust real `click` 对窗口外坐标返回 `point_outside_target_window` 且不点击。
- Rust real `click` 对非 left button 返回 `unsupported_button`。
- Rust real `click_element` 能从当前进程 observation cache 解析 `active-window`。
- Accessibility 权限不足时点击返回结构化失败，后续 `stop` 仍能阻断同 run 动作。
- Rust real `type_text` 继续返回 `unsupported_real_tool`。
- Rust mock mode 行为不变。

---

## Phase 18 — Rust type_text parity

### 目标

把 Rust real backend 的文本输入迁移到与 Python backend 一致的安全语义：动作前重新验证目标，优先使用固定 System Events direct keystroke；只有普通文本且显式允许时才使用受控剪贴板 fallback；疑似 secret/token/password 不进入剪贴板。

V18 status: implemented. Rust real `type_text` now calls `validate_target`
before any input side effect, tries direct keystrokes for supported text, and
falls back to command-v clipboard paste only when `allowClipboardFallback=true`
and the text does not look sensitive. The result exposes method,
`clipboardFallbackUsed`, and character count only; it does not echo typed text.

### 范围

1. 输入路径：

```text
computer_type args
  ↓
Rust real validate_target
  ↓
direct System Events keystrokes
  ↓
if unsupported/failed and ordinary text fallback allowed
  → save current clipboard
  → pbcopy text
  → command-v
  → restore previous clipboard
```

2. secret/clipboard policy：

```text
probable secret OR allowClipboardFallback=false
  ↓
direct typing unsupported/failed
  ↓
return safe error
  ↓
do not place text on clipboard
```

3. 输出与审计：

```text
typed text never returned
typedChars only
method = direct_keystroke | controlled_clipboard_paste
clipboardFallbackUsed = boolean
```

### 验收标准

- Rust real `type_text` 对错误 `targetWindowId` 先返回 `target_window_changed`，不输入。
- Rust real `type_text` 对 long secret 返回 `direct_type_unsupported_text`，`clipboardFallbackUsed=false`。
- Rust real `type_text` stop 后返回 `run_stopped`。
- Rust mock mode direct-first / secret-no-clipboard 行为不变。
- 不回显 typed text。

---

## Phase 19 — Rust sidecar install/status path

### 目标

把 Rust sidecar 从“源码里能编译”推进到“开发/发布前可安装、可诊断”。本阶段不改变默认运行时：Python backend 仍是默认路径；Rust sidecar 仍需用户/开发者通过 `OMIGA_COMPUTER_USE_SIDECAR=rust` 显式 opt-in。

V19 status: implemented. Added `scripts/install-computer-use-sidecar.sh` with a
status mode and install mode. The script can build `computer-use-sidecar` in
debug or release profile, copy an existing binary, and install it into the
bundled plugin as `bin/computer-use-sidecar`. `.gitignore` ignores the installed
local binary so release/dev installs do not accidentally dirty source control.

### 范围

1. Status diagnostics：

```text
scripts/install-computer-use-sidecar.sh --status
  ↓
repo root
Cargo manifest
profile
target output path
plugin destination path
installed/executable status
runtime opt-in env vars
```

2. Install path：

```text
scripts/install-computer-use-sidecar.sh --profile release
  ↓
cargo build --manifest-path src-tauri/Cargo.toml --bin computer-use-sidecar --release
  ↓
copy target/release/computer-use-sidecar
  ↓
src-tauri/bundled_plugins/plugins/computer-use/bin/computer-use-sidecar
  ↓
chmod 755
```

3. Test/CI-friendly overrides：

```text
--no-build
--binary PATH
--plugin-dir PATH
CARGO_TARGET_DIR=...
```

4. 安全边界：

```text
default runtime = Python backend
Rust runtime requires OMIGA_COMPUTER_USE_SIDECAR=rust
Rust backend mode remains OMIGA_COMPUTER_USE_BACKEND=mock|real|auto
installed binary ignored from git
```

### 验收标准

- `--status` 无副作用输出当前 install/build 状态。
- `--no-build --binary ... --plugin-dir ...` 可复制到临时插件目录并保持 executable。
- 临时安装出的 sidecar 可完成 MCP `initialize`。
- 默认 wrapper 行为不变：未设置 `OMIGA_COMPUTER_USE_SIDECAR=rust` 时仍走 Python backend。
- source control 不追踪本地安装的 `bin/computer-use-sidecar`。

---

## Phase 20 — Settings backend diagnostics

### 目标

把 backend 安装/可执行状态提升到应用内可见，但保持诊断只读：不做权限探测、不截图、不启动后端进程。V29 收敛后，此页面只展示用户可见的 Python backend 与 wrapper 状态；Rust sidecar 状态保留在内部开发脚本中，不进入 Settings。

V20 status: implemented. Added Tauri command `computer_use_backend_status`
and Settings → Computer Use backend diagnostics. The command inspects only
filesystem paths. It reports the Python runtime, wrapper path, Python backend
path, and installed/executable booleans.

### 范围

1. 只读 Tauri command：

```text
computer_use_backend_status
  ↓
inspect bundled computer-use plugin paths
  ↓
return status JSON
```

2. Settings 展示：

```text
Runtime: Python
Wrapper: ready | not executable | missing
Python backend: executable | not executable | missing
Python backend path
```

3. 明确不做：

```text
no osascript
no screencapture
no backend process startup
no settings mutation
```

### 验收标准

- Settings 打开可自动加载 backend status，不触发权限探测。
- 用户可点击“刷新后端状态”手动刷新。
- Settings 不显示 Rust runtime/sidecar/mode/path。
- 默认 runtime 仍显示 Python。
- 命令注册到 Tauri invoke handler。

---

## Phase 21 — QA matrix 与 smoke 脚本

### 目标

把之前分散在对话和手工命令里的 Computer Use 验证固化下来，形成可重复的 smoke 脚本和发布前 QA matrix。重点覆盖“无副作用 mock”与“real-safe 阻断路径”，不扩大 OS 动作能力。

V21 status: implemented. Added `scripts/computer-use-smoke.py` and
`docs/COMPUTER_USE_QA_MATRIX.md`. The smoke runner speaks MCP
Content-Length stdio directly and can validate Rust/Python mock sidecars plus
Rust/Python real-safe paths. The QA matrix documents scripted checks, manual app
checks, security regressions, and release checklist.

### 范围

1. Scripted smoke：

```text
scripts/computer-use-smoke.py --suite mock
  → rust-mock
  → python-mock

scripts/computer-use-smoke.py --suite rust-real-safe
scripts/computer-use-smoke.py --suite python-real-safe
scripts/computer-use-smoke.py --suite all-safe
```

2. Mock coverage：

```text
initialize / tools/list
observe target identity
set_target app smuggle blocked
short type_text direct-keystroke
long password secret blocks clipboard fallback
typed text not echoed
```

3. Real-safe coverage：

```text
observe current target or structured permission failure
out-of-bounds click blocked before action
wrong targetWindowId type_text blocked before input
stop blocks follow-up same-run observe/action
```

4. QA matrix doc：

```text
scripted coverage
manual app QA
security regression checks
release checklist
```

### 验收标准

- `scripts/computer-use-smoke.py --suite mock` passes with a built Rust sidecar.
- `scripts/computer-use-smoke.py --suite rust-real-safe` exercises only safe validation failures.
- QA matrix clearly separates deterministic mock, real-safe, manual, and security checks.
- Smoke script does not introduce third-party dependencies.
- Computer Use main docs link to the QA matrix and smoke command.

---

## Phase 22 — Release checklist aggregation

### 目标

把 Phase 21 的 QA matrix 进一步收敛成一个可重复执行的 release gate，避免每次发布前靠人工复制多条命令。默认路径只执行低风险检查：脚本语法、Python syntax、安装状态、Rust sidecar 格式、Computer Use 相关 diff/text hygiene，以及 Rust/Python mock MCP smoke；真实 macOS backend、安全失败探测、完整前端 build 和 cargo lib test 都必须显式 opt in。

V22 status: implemented. Added `scripts/computer-use-release-check.py`. The
aggregator produces JSON by default, can write Markdown with `--format
markdown --output PATH`, and exposes opt-in flags for `--include-build`,
`--include-cargo-test`, and `--include-real-safe`.

### 范围

1. Default low-risk gate：

```text
required-files
computer-use-text-hygiene
python-syntax-check
install-script-syntax
install-script-status
rustfmt-sidecar
git-diff-check
mcp-mock-smoke
```

2. Optional gates：

```text
--include-build       → bun run build
--include-cargo-test  → cargo test --manifest-path src-tauri/Cargo.toml computer_use --lib
--include-real-safe   → rust-real-safe + python-real-safe MCP probes
--format markdown     → human-readable release artifact
```

3. Safety policy：

```text
default run does not start the real macOS backend
default run does not click, type, capture screenshots, or request permissions
real-safe probes remain explicit because they may observe the current frontmost app
```

### 验收标准

- `scripts/computer-use-release-check.py --rust-bin /path/to/computer-use-sidecar` passes when a Rust sidecar binary is supplied.
- JSON report includes per-check command, exit code, duration, stdout/stderr tail, and overall `ok`.
- Markdown report can be written for release notes/PR evidence.
- QA matrix and user-facing Computer Use docs point to the aggregate command.
- No third-party dependency is introduced.

---

## Phase 23 — Rust sidecar artifact integrity

### 目标

补齐 Rust opt-in 发布路径中的“安装产物是否就是刚构建/指定的产物”验证。保持默认 runtime 为 Python，不改变插件启用语义，也不把 sidecar 二进制纳入源码跟踪；只在显式 release/package gate 中检查已安装的 bundled sidecar。

V23 status: implemented. `scripts/install-computer-use-sidecar.sh --status`
now reports source/destination SHA-256 values when binaries exist. The release
aggregator accepts `--verify-installed-sidecar`, `--installed-sidecar`, and
`--expected-sidecar-sha256`. When `--rust-bin` is supplied together with
`--verify-installed-sidecar`, the installed bundled binary must be executable
and hash-match the source binary. Unless `--skip-smoke` is used, the installed
binary also runs the Rust mock MCP smoke suite.

### 范围

1. Installed artifact check：

```text
installed sidecar exists
installed sidecar is executable
installed sha256 is reported
optional expected sha256 matches
optional --rust-bin source sha256 matches installed sha256
```

2. Installed executable smoke：

```text
scripts/computer-use-release-check.py \
  --rust-bin /path/to/computer-use-sidecar \
  --verify-installed-sidecar
  → mcp-installed-rust-mock-smoke
```

3. Install script status：

```text
scripts/install-computer-use-sidecar.sh --status
  → source_sha256=...
  → dest_sha256=...
```

### 验收标准

- A temp plugin install using `--no-build --binary ... --plugin-dir /private/tmp/...` can be verified without dirtying the repo.
- `--verify-installed-sidecar` fails closed if the installed binary is missing, non-executable, hash-mismatched, or does not pass Rust mock MCP smoke.
- The default release check remains low-risk and does not require an installed Rust sidecar.
- QA matrix and Computer Use docs include the installed-artifact verification path.
- No new dependency is introduced.

---

## Phase 24 — Result-first evidence UI

### 目标

修正下一阶段方向：不做 detailed screenshot/history browser，也不把 Operation Flow 作为普通用户 UI。用户关注“任务有没有得到结果”，不是逐步过程；过程只作为本地 evidence path 保留，供高级审计/排障时手动查看。

V24 status: implemented. Settings → Computer Use now frames the audit area as
“结果留痕”: it shows result-record count, evidence size, retention, and the
local evidence path. It explicitly states that the UI does not render saved
screenshots, step-by-step operation flows, or typed content. Backend audit data
and retention behavior remain available internally, but the default UI no
longer emphasizes `Flows`/`Actions` process metrics.

### 范围

1. UI 表达：

```text
本项目结果留痕
  → Result records
  → Evidence size
  → Retention
  → Evidence path
```

2. 不展示：

```text
saved screenshots gallery
step-by-step operation flow viewer
typed text / action payloads
per-action timeline
```

3. 保留：

```text
本地 run/evidence path
retention cleanup
secret redaction
manual forensic path for advanced debugging
```

### 验收标准

- Settings UI 不再突出 `Flows`/`Actions` 过程指标。
- Settings UI 明确说明不展示截图、逐步操作流程或输入内容。
- 仍保留 evidence path，支持高级用户手动排障。
- 清理/刷新按钮文案与“结果留痕”一致。
- 不改变 Computer Use backend 行为、不扩大电脑操作能力。

---

## Phase 25 — Result status signals

### 目标

在不展示详细操作过程的前提下，给用户一个“结果是否可靠”的快速判断。聚合 run-level flow 的状态，只输出结果信号，不展示 action timeline、截图、输入内容或 raw payload。

V25 status: implemented. `ComputerUseAuditSummary` now includes exclusive
result buckets: `resultOkCount`, `resultBlockedCount`,
`resultNeedsAttentionCount`, `resultStoppedCount`, and `resultUnknownCount`.
Settings displays OK / Needs attention / Blocked / Stopped chips next to result
records and evidence path.

### 范围

1. Backend 聚合：

```text
run.json.flow.status + counters
  → ok / blocked / needs_attention / stopped / unknown
  → ComputerUseAuditSummary result*Count
```

2. UI 信号：

```text
Result records
OK
Needs attention
Blocked
Stopped
Evidence size / Retention / Evidence path
```

3. 不做：

```text
per-action details
screenshot gallery
typed content display
process timeline
new Computer Use capabilities
```

### 验收标准

- Result status counts are derived from existing run-level flow data.
- Counts are exclusive and do not require reading or rendering action payloads.
- Settings shows compact status signals only.
- Existing evidence path and retention remain intact.
- No new dependency is introduced.

---

## Phase 26 — Packaging config release check

### 目标

进入稳定化阶段后，不继续扩展 Computer Use 功能，而是把发布前最容易遗漏的 packaging 配置纳入现有 release gate。目标是确认 bundled plugin 会被 Tauri 打包、marketplace 指向正确 plugin、plugin manifest 与 `.mcp.json` 指向稳定 wrapper、平台 wrapper 有可执行位并通过 shell syntax check。

V26 status: implemented. `scripts/computer-use-release-check.py` now runs
`computer-use-packaging-config` by default. The check validates Tauri
`bundle.resources`, bundled marketplace entry, `plugin.json`, `.mcp.json`,
wrapper files, executable bits, and wrapper shell syntax. It also reports
generated packaging artifacts such as `__pycache__`/`.pyc` under the bundled
plugin so final packaging can clean them intentionally. Smoke tests now set
`PYTHONDONTWRITEBYTECODE=1` for backend processes to avoid creating new Python
bytecode inside plugin resources.

### 范围

1. Packaging config：

```text
src-tauri/tauri.conf.json bundle.resources includes bundled_plugins
src-tauri/bundled_plugins/marketplace.json has computer-use entry
plugin.json name=mcpServers/interface are valid
.mcp.json mcpServers.computer.command=./bin/computer-use
bin wrappers exist, are executable, and pass sh -n
```

2. Artifact hygiene visibility：

```text
release report includes generatedPackagingArtifacts
smoke backend processes set PYTHONDONTWRITEBYTECODE=1
```

3. 不做：

```text
codesign/notarization keychain automation
actual bundle build/sign/upload
new Computer Use runtime behavior
```

### 验收标准

- Default release check includes packaging config validation.
- Packaging validation fails closed on missing marketplace/plugin/MCP/wrapper config.
- Generated plugin artifacts are visible in the report for final release cleanup.
- Smoke tests avoid creating new Python bytecode under plugin resources.
- No UI or runtime capability expansion.

---

## Phase 27 — Final packaging hygiene gate

### 目标

把 Phase 26 中“报告 generated packaging artifacts”升级为显式 final packaging gate，但仍然不自动删除文件，避免误删其它 session 的产物。最终打包前由发布流程清理 `__pycache__` / `.pyc` 后再启用该 gate。

V27 status: implemented. `scripts/computer-use-release-check.py` accepts
`--fail-on-generated-artifacts`. Default release checks continue to report
generated artifacts without failing; final packaging checks can opt in to fail
closed when bundled plugin resources contain `__pycache__` or `.pyc` files.

### 范围

1. Final hygiene gate：

```text
scripts/computer-use-release-check.py \
  --fail-on-generated-artifacts
```

2. 失败条件：

```text
src-tauri/bundled_plugins/plugins/computer-use/**/__pycache__
src-tauri/bundled_plugins/plugins/computer-use/**/*.pyc
```

3. 不做：

```text
automatic rm/rmtree
bundle build/sign/upload
runtime behavior changes
new UI
```

### 验收标准

- Default release gate remains useful during development and still reports generated artifacts.
- Final packaging gate can fail closed on generated plugin artifacts.
- Smoke tests do not create new `.pyc` files because backend env sets `PYTHONDONTWRITEBYTECODE=1`.
- No destructive cleanup is performed automatically.

---

## Phase 28 — Generated artifact cleanup

### 目标

执行最终打包卫生检查前的最小清理：只删除 Computer Use bundled plugin 目录下已确认的 Python bytecode 生成物，不触碰源码、不做 bundle/sign/upload、不新增功能。

V28 status: completed. Removed:

```text
src-tauri/bundled_plugins/plugins/computer-use/bin/__pycache__/computer-use-macos.cpython-312.pyc
src-tauri/bundled_plugins/plugins/computer-use/bin/__pycache__
```

After cleanup, the final packaging hygiene gate passes with
`--fail-on-generated-artifacts`.

### 范围

1. 删除对象：

```text
computer-use bundled plugin 内的 __pycache__ / .pyc
```

2. 验证：

```text
scripts/computer-use-release-check.py \
  --rust-bin /path/to/computer-use-sidecar \
  --fail-on-generated-artifacts
```

3. 不做：

```text
删除其它 session 文件
删除源码或配置
实际 Tauri bundle
codesign/notarization
新功能
```

### 验收标准

- Computer Use plugin resources no longer contain `__pycache__` or `.pyc`.
- Final packaging hygiene gate passes.
- Default release gate still passes.
- No runtime behavior changes.

---

## Phase 29 — Python-first runtime / Rust internal feature

### 目标

按产品收敛决策：先使用 Python backend 作为用户可见运行时；Rust sidecar 继续保留，但只作为内部实验 feature，不在 Settings 或普通 release gate 中暴露给用户。

V29 status: implemented. Platform wrappers only honor
`OMIGA_COMPUTER_USE_SIDECAR=rust` when the internal developer flag
`OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1` is also set. Settings backend
diagnostics now serializes and shows the Python runtime/backend only; it does
not return Rust sidecar path, mode, or opt-in state to the frontend. The
default release gate uses Python mock smoke; Rust sidecar smoke/integrity
checks require explicit internal flags such as `--include-rust-sidecar`.

### 范围

1. Runtime selection：

```text
default → Python backend
OMIGA_COMPUTER_USE_SIDECAR=rust alone → ignored by wrapper and hidden from Settings status
OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1 + OMIGA_COMPUTER_USE_SIDECAR=rust → internal Rust path
```

2. 用户界面：

```text
Settings → Computer Use
  → Runtime: Python
  → Wrapper status
  → Python backend status/path
  → no Rust sidecar chip/path/mode
```

3. Release gate：

```text
default release check → Python mock
internal Rust check → --include-rust-sidecar --rust-bin ...
```

### 验收标准

- 用户设置页不显示 Rust runtime/sidecar/mode。
- `computer_use_backend_status` 不向前端返回 Rust sidecar 路径或模式。
- 普通用户默认 wrapper 始终走 Python backend；只有内部 feature flag 双条件才会改走 Rust。
- Rust sidecar 只能通过内部 developer feature flag 进入。
- 默认 release gate 不要求 Rust binary。
- 不删除 Rust sidecar 源码或内部验证脚本。

---

## Phase 33 — Fixed non-text key action

### 目标

补齐低风险、常用的真实键盘动作：在已有 observe/target-lock/revalidate/stop/audit
边界内，允许模型通过 `computer_key` 触发固定白名单非文本按键，而不是开放任意
快捷键、AppleScript 或 shell。

V33 status: implemented. The core exposes `computer_key`, maps it to backend
`key_press`, and requires `observationId` plus `targetWindowId` like click/type.
Python and Rust sidecars revalidate the target immediately before the action,
then run fixed System Events key-code snippets for Enter/Return, Tab, Escape,
Backspace/Delete, arrows, Page Up/Down, Home/End, and Space. Mock smoke covers
schema/backend parity, and real key E2E verifies submitting a temporary dialog
via Enter.

### 范围

```text
computer_key
  → core facade policy
  → mcp__computer__key_press
  → backend validate_target
  → fixed key code only
```

不做：

```text
arbitrary shortcut strings
command/option/control modifiers
model-provided AppleScript
global app switching shortcuts
```

### 验收标准

- `computer_key` schema is model-visible only when Computer Use is enabled.
- Action schema requires latest `observationId` and `targetWindowId`.
- Unsupported keys fail closed with `unsupported_key`.
- Python and Rust mock suites report `keyPressAction=key_press`.
- Permission-ready macOS QA can run `--include-real-key-e2e`.

---

## Phase 34 — Fixed scroll-wheel action

### 目标

补齐真实滚轮动作，但继续保持低权限、低自由度：模型只能请求固定方向与有界
amount，core 仍要求最新 observe 与 target lock，backend 在动作前重新验证前台
窗口，然后把滚轮事件投递到验证目标中心。

V34 status: implemented. The core exposes `computer_scroll`, maps it to backend
`scroll`, and requires `observationId` plus `targetWindowId`. Python and Rust
sidecars validate the current target, derive the target center from validated
bounds, then post a fixed CoreGraphics scroll-wheel event for up/down/left/right.
Mock smoke covers backend parity, and real scroll E2E opens a temporary long
TextEdit document, scrolls down, and verifies the vertical scroll indicator
moved.

### 范围

```text
computer_scroll
  → core facade policy
  → mcp__computer__scroll
  → backend validate_target
  → fixed CoreGraphics scroll-wheel event at target center
```

不做：

```text
arbitrary pointer gestures
drag gestures
free-form mouse movement
model-provided scripts
```

### 验收标准

- `computer_scroll` schema is model-visible only when Computer Use is enabled.
- Action schema requires latest `observationId` and `targetWindowId`.
- Unsupported directions fail closed with `unsupported_scroll_direction`.
- Python and Rust mock suites report `scrollAction=scroll`.
- Permission-ready macOS QA can run `--include-real-scroll-e2e`.

---

## Phase 35 — Distribution hardening preflight

### 目标

把“准备分发”从人工检查推进到可重复门禁：发布前能生成/校验 Computer Use
packaging artifact checksum manifest，并能在 macOS packaging machine 上确认
签名/公证工具链与 Tauri macOS plist 配置就绪。

V35 status: implemented. `scripts/computer-use-release-check.py` now supports
`--write-artifact-manifest` and `--verify-artifact-manifest` for deterministic
path/size/executable/SHA-256 verification across plugin manifest, MCP config,
wrappers, Python backend, installed internal Rust sidecar when present, Tauri
config, and marketplace entry. It also supports `--include-signing-preflight`,
which checks local `codesign`, `xcrun notarytool`, `xcrun stapler`, Tauri
`bundle.macOS.entitlements` / `infoPlist` references, and parseable
`Entitlements.plist` / `Info.plist`. Optional flags can require a local
codesign identity and verify a notarytool keychain profile.

### 范围

```text
artifact manifest
  → path
  → sizeBytes
  → executable
  → sha256

signing preflight
  → codesign available
  → xcrun notarytool available
  → xcrun stapler available
  → Tauri macOS plist refs exist
  → Entitlements.plist / Info.plist parse
```

不做：

```text
actual codesign
actual notarization submit
upload/release publishing
credential creation
```

### 验收标准

- A generated artifact manifest verifies cleanly in the same checkout.
- Manifest verification fails closed on size/hash/executable drift.
- macOS preflight reports tools and plist/entitlement keys.
- Generated plugin packaging artifacts remain blocked by
  `--fail-on-generated-artifacts`.

---

## Phase 39 — Optional visual OCR observe

### 目标

在不改变默认隐私姿态的前提下，补齐一个真实的视觉文本观察能力：只有调用方显式传入
`extractVisualText=true` 时，observe 才捕获一次截图并用 macOS Vision 做 OCR。OCR
结果作为独立、受限的 `visualText` boxes 返回，不替代 AX 元素、不驱动动作，也不让模型执行任意脚本。

V39 status: implemented. Python user-facing backend and internal Rust sidecar
share the same fixed Swift/Vision OCR helper. `observe(extractVisualText=true)`
captures a screenshot after allowlist checks, returns bounded visual text boxes
with `text`, `confidence`, `bounds`, and `source`, and reports
`visualTextError` / `visualTextRequiresPermission` when Screen Recording, Swift,
or Vision is unavailable. Mock mode returns deterministic visual text metadata.
Smoke/release gates include `python-real-visual-text` against a temporary
dialog and optional Rust parity via `--include-real-visual-text`.

### 范围

```text
computer_observe(extractVisualText=true)
  → backend allowlist check
  → screenshot capture
  → fixed macOS Vision OCR helper
  → bounded visualText[] result
  → structured visualTextError on failure
```

不做：

```text
default OCR on every observe
model-provided OCR scripts
visual model image input
OCR-driven autonomous clicking
full OCR/AX semantic tree fusion
Windows/Linux OCR backend
```

### 验收标准

- 默认 `computer_observe` 不触发 OCR，也不因 OCR 失败影响普通观察。
- `extractVisualText=true` 会在 allowlist 通过后捕获截图并返回 bounded
  `visualText` boxes 或结构化 `visualTextError`。
- Mock smoke 覆盖 visual text response shape。
- Permission-ready macOS QA can run `--include-real-visual-text` against a
  temporary dialog and verify visible target text is recognized.

---

# 2. Tracer-bullet 最小交付定义

第一条垂直切片完成时必须满足：

1. `computer-use` 插件可安装/启用。
2. 插件启用但 Computer Use mode=off 时，不暴露 `computer_*`。
3. Computer Use mode=task/session 时，暴露：

```text
computer_observe
computer_click
computer_click_element
computer_type
computer_key
computer_stop
```

4. `computer_observe` 调 mock MCP backend 并返回固定 observation。
5. `computer_click` / `computer_type` 经过 core policy 后再调 backend。
6. raw `mcp__computer__*` 被拒绝。
7. stop 后后续动作被拒绝。
8. 本地审计日志写入，敏感输入脱敏。

---

# 3. Deferred work

以下内容不进入 tracer bullet：

- 完整平台 target resolver。
- Windows/Linux backend。
- Full AX tree。
- 视觉模型 image input。
- 通用拖拽。
- 菜单栏通用点击。
- 通用 AppleScript/shell 执行。
- 完整 run-history 浏览器。
- 独立剪贴板工具，除非 `computer_type` 内部需要临时剪贴板粘贴。
