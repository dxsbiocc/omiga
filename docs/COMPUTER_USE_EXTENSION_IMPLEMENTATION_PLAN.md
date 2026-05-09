# Computer Use Extension 分阶段实现计划

Status: V10 MVP implemented through guarded `computer_*` facade, target-window revalidation, user-facing controls, real macOS sidecar backend, and enforced allowed-app policy
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
| Phase 11+ | Pending | live target thumbnail/history browser、deeper AX tree/OCR、full release QA |

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
  sees: computer_observe / computer_set_target / computer_click / computer_type / computer_stop
        ↓
Omiga core facade
  gate → permission → audit → target-window validation → stop/cancel checks
        ↓
Internal MCP route
  server: computer
  tools: observe / set_target / click / click_element / type_text / stop / validate_target
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
mcp__computer__type_text
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
- OCR。
- Full AX tree。
- 视觉模型 image input。
- 通用拖拽。
- 菜单栏通用点击。
- 通用 AppleScript/shell 执行。
- 完整 run-history 浏览器。
- 独立剪贴板工具，除非 `computer_type` 内部需要临时剪贴板粘贴。
