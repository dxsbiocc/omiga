# Agent Runtime 优化目标（对标 codex-rs）

> 依据：与 `codex-main/codex-rs` 核心运行时的对比分析（2026-07-07）。
> 原则：**禁止占位/伪实现**。每项完成的定义 = 真实可运行 + `cargo check`/测试通过 + review 无 stub。
> 参考实现均位于 `/Users/dengxsh/Downloads/Work/Agent/codex-main/codex-rs`。

## 状态总览

| # | 目标 | 优先级 | 状态 |
|---|------|--------|------|
| G1 | Prompt 缓存（Anthropic cache_control + OpenAI prompt_cache_key） | P0 | ✅ 已实现（Anthropic 默认生效；OpenAI key 已在 chat 层注入 session.id） |
| G2 | 真实 usage 驱动的上下文压缩 | P0 | ✅ 已实现（11 测试通过；cache_read 口径列为后续） |
| G3 | 流中断重试/恢复 | P1 | ✅ 已实现（保守重试：仅首个可见 chunk 前静默重试） |
| G4 | 持久化 exec 会话（unified-exec 式进程复用 + 有界输出缓冲） | P1 | ✅ 已实现（4 测试通过） |
| G5 | 工具并行性声明化（替换名称白名单启发式） | P1 | ✅ 已实现（5 测试通过，删除 2 处旧启发式） |
| G6 | 本地沙箱（macOS seatbelt，Linux landlock 后续） | P2 | ✅ 已实现（6 测试，含真实 seatbelt 拦截；后台 shell 与 Linux 列为后续） |
| G7 | 流式 emit 合帧节流 | P2 | ✅ 已实现（30ms/2KB 合帧，工具事件前与结束强制 flush） |
| G8 | 热路径巨石文件拆分（chat/mod.rs 5618 行、tool_exec.rs 3108 行） | P2 | ✅ 已实现（5 提交:3320601/aa4e0b9/ed36cb3/1b98ca5/1cde0fe;详见下方 G8 完成记录） |
| G9 | OTel 级性能埋点 | P2 | ✅ 已实现（turn 级指标：connect/ttft/stream 时长、工具数、token/cache 用量、重试数，经 tracing 结构化事件 `llm_turn_metrics` 输出；3 测试。完整 OTel exporter 列为后续） |
| G10 | apply_patch 多文件原子补丁工具 | P1 | ✅ 已实现（9 测试，先算后写原子回滚） |
| G11 | hooks 生命周期系统（PreToolUse/PostToolUse 等） | P1 | ✅ 已实现（6 测试，.omiga/hooks.toml，无配置零开销） |
| G12 | 沙箱增强（Linux landlock + 后台 shell 沙箱化 + 提权审批） | P2 | ✅ 已实现（后台沙箱化+SANDBOX_DENIED 信号+landlock 诚实降级骨架） |
| G13 | 细粒度网络策略（按域名 allow/deny，替代总开关） | P2 | ✅ 已实现（NetworkPolicy allow/deny list，域名过滤局限已注明） |
| G14 | 缓存/压缩收尾（cache_read 计入压缩口径 + Anthropic cache usage 持久化） | P2 | ✅ 已实现（cache 落库+按 provider 语义纳入压缩口径） |

> 第二阶段（G10–G14）已完成并通过 review。review 中发现并修复了 G12/G13 沙箱测试的进程级 env 竞争（libc `setenv`/`getenv` 非线程安全）——通过将 `from_env` 拆为纯 `from_parts` 核心 + 薄壳，测试改测纯核心，彻底消除 flake（0/12 稳定）；两个真实 `sandbox-exec` 集成测试 gate 为 `--ignored` 按需运行。
> G1–G14 全部完成。G9 已以 tracing 结构化事件形式落地（`domain/telemetry` + `turn.rs` 全链路埋点）；接完整 OTel exporter 列为后续增强。

## G8 完成记录（2026-07-08）

分 5 个提交落地,每个提交独立过 codex review(全部 PASS)+ 主会话机械比对(逐行集合比对 + 早退路径对账 + 锁作用域核对),每轮 `cargo test --lib` 1279 passed 0 failed:
- **G8a**(3320601):tool_exec.rs 3302 行 → tool_exec/ 14 文件模块树(orchestrate/dispatch/concurrency/normalize + handlers/ 按工具族分 9 文件,最大 564 行),调用方零 diff。
- **G8b**(aa4e0b9):chat/mod.rs helper 层 → 8 个子模块(agent_runtime/attachments/compaction_input/composer_route/fallback_messages/llm_bridge/runtime_constraints/tool_output,最大 447 行)。
- **G8c-1**(ed36cb3):send 管道迁入 send_pipeline.rs;TurnSpawnContext 结构体消灭全部 12 个 `*_for_spawn` 逐字段 clone;mod.rs → 167 行命令壳。
- **G8c-2**(1b98ca5):send_message_impl 2030 → 350 行 + 12 个阶段函数;8 处早退路径经 ?/Option/枚举等价透传逐一对账。
- **G8c-3**(1cde0fe):run_turn_spawn 1719 → 360 行编排层 + 16 个阶段函数(最大 227 行)。

执行事故与审查拦截记录(供后续任务借鉴):
1. codex 插件共享 app-server 管道多次僵死(review 线程启动后无输出),已改用 `codex exec` 直连方式执行与审查。
2. G8c-2 的 codex 执行因 model capacity 中途崩溃,留下 `.pipe(|_| unreachable!())` 占位(PrepareTurnRuntimeInput 漏 pending_tools 字段),由主会话对照 HEAD 修复并全量验证。
3. G8c-3 中 codex 用"喂测试注释"使 include_str! 结构性测试窗口坍缩到编排层调用点(死锁防护断言实际检查的是注释),主会话审查发现后将测试重新锚定到 compact_tool_loop_history 函数体,断言加强为禁止函数内任何 `.read().await`。

整合 review(d6c0cb7..1cde0fe 全跨度,PASS)遗留 3 条 LOW 后续项:
- Skill DTO(SkillToolArgs 等)应从 composer_route.rs 移回 tool_exec 内部;
- provider.rs 的 handle_skill_config 应并入 tool_exec/handlers/(skill_config_ops);
- 新 helper 模块的 `use super::*` 应逐步改为显式 import,mod.rs 只留对外 re-export。

## codex-rs 功能差距复查（2026-07-08,G8 完成后）

已对齐:prompt 缓存、usage 压缩、流重试、unified-exec 会话、工具并行声明、seatbelt 沙箱、emit 合帧、apply_patch、hooks、域名网络策略、turn 遥测、热路径模块化。
仍存差距(按对产品价值排序,候选第三阶段):
| 候选 | codex-rs 参考 | omiga 现状 | 建议优先级 |
|---|---|---|---|
| 提权审批完整流(沙箱拒绝 → 单次提权请求 → UI 审批 → 重跑) | shell-escalation/ | 仅有 SANDBOX_DENIED 信号,无审批闭环 | P1 |
| Linux landlock 真实现 | linux-sandbox/ | 116 行诚实降级骨架 | P2(视 Linux 用户量) |
| OTel exporter(现有 tracing 事件外接 OTLP) | otel/ | llm_turn_metrics tracing 事件 | P2 |
| 网络代理式策略(强制所有子进程流量过代理) | network-proxy/ | seatbelt 域名过滤,子进程可绕过 | P2 |
| Windows 沙箱 | windows-sandbox-rs/ | 空白(裸执行) | P3(视 Windows 用户量) |
| bash 命令安全解析(执行前静态分析危险命令) | shell-command/ | 无 | P3 |
非目标(产品形态差异,不追):app-server 系列、cloud-tasks、tui、realtime-webrtc、chatgpt 集成。
> 修复记录（2026-07-07）：`research plan/run` 从产品 CLI 隐藏时误删了内部执行路径，导致 goals 引擎 6 个测试失败。已恢复为 `cli::execute_research_request` 内部入口（CLI 门禁保持不变），goals 循环改走该入口。

> G1–G7 已完成并通过 review（详见下方各节 + 状态）。以下为第二阶段：补足与 codex 的**功能性差距**（非纯性能）。

---

## 第二阶段：功能差距清单（对照 codex-rs）

### G10 apply_patch 多文件原子补丁工具（P1，功能差距最大）
**现状**：omiga 只有单文件 `file/edit.rs`（逐次 Edit）与 `write.rs`。跨多文件修改要多次调用，且无原子性、无统一 context 匹配。
**codex**：`apply-patch/`（1719 行）提供 `*** Begin Patch / *** Update File / *** Add File / *** Delete File` 语法，一次调用原子地跨多文件打补丁，带上下文行模糊匹配与冲突检测。
**目标**：新增 `apply_patch` 工具，支持 add/update/delete、`@@` context hunk、模糊匹配容错；失败时整体回滚不留半成品。参考 codex `apply-patch/src/lib.rs`（解析器 + 应用器）。
**验收**：单测覆盖 add/update/delete/多文件/context 漂移匹配/冲突拒绝；集成测试证明原子回滚。

### G11 hooks 生命周期系统（P1，扩展性差距）
**现状**：omiga 无任何 hook 机制。用户全局 rules 明确依赖 PreToolUse/PostToolUse/Stop 钩子（格式化、校验、通知），当前无法满足。
**codex**：`hooks/`（engine + registry + schema + events + output_spill）提供声明式 hook 配置、事件分发、输出溢出处理。
**目标**：最小可用 hook 引擎——从配置加载 hook 声明，在工具执行前后（PreToolUse/PostToolUse）触发外部命令，PreToolUse 可否决/改参，PostToolUse 可注入反馈。参考 codex `hooks/src/{engine,registry,schema}`。
**验收**：单测：PreToolUse 否决阻止执行、PostToolUse 反馈注入、匹配规则（按工具名/参数）。

### G12 沙箱增强（P2，G6 延伸）
- **Linux landlock**：G6 只做了 macOS seatbelt，Linux 目前裸执行。参考 codex `linux-sandbox/`。
- **后台 shell 沙箱化**：G6 的 `run_bash_command`（background-shell 路径）目前 `sandbox_disabled=true`，需接入。
- **提权审批**：当前只有 `dangerously_disable_sandbox` 全开/全关。参考 codex `shell-escalation/`，加"沙箱拒绝→请求单次提权→审批"流。

### G13 细粒度网络策略（P2）
**现状**：G6 沙箱只有 `OMIGA_SANDBOX_NETWORK=deny` 总开关。
**codex**：`network-proxy/` + `network_policy_decision.rs` 支持按域名/端口 allow/deny。
**目标**：沙箱网络策略支持域名白/黑名单，seatbelt policy 生成对应规则。

### G14 缓存/压缩收尾（P2，G1/G2 尾巴）
- G2 的 `last_turn_input_tokens` 口径当前只用 `MessageTokenUsage.input`，未含 Anthropic `cache_read_input_tokens`。
- 需把 G1 已解析出的 cache_read/cache_creation 持久化进 `MessageTokenUsage`，再让 G2 压缩口径纳入完整前缀规模。
**验收**：cache usage 落库 + 压缩基线纳入 cache_read 后行为正确。

### G8 拆分巨石文件（P2，技术债）
`commands/chat/mod.rs`（5618 行）与 `tool_exec.rs`（3108 行）按路由/编排/持久化分层拆分，消除 `*_for_spawn` 逐字段 clone。风险高，需在功能项稳定后单独迭代。

### G9 OTel 遥测（P2）
turn 级 span（连接耗时、首 token、工具耗时、压缩耗时）。参考 codex `otel/`。

---

## G1 Prompt 缓存（P0）

**现状**：`src/llm/`、`src/api/` 无任何 `cache_control` / `prompt_cache_key`。长会话每轮全量重算历史 token。

**目标**：
- Anthropic 原生路径（`src/api/mod.rs` 的 `Request` 结构）：system prompt 与 tools 定义打 `cache_control: {type: "ephemeral"}` 断点；消息历史尾部打滚动断点（最多 4 个断点，遵循 Anthropic 限制）。
- OpenAI 兼容路径（`src/llm/openai.rs`）：请求体带 `prompt_cache_key`（按 session id 稳定取值），保证消息前缀稳定（system/tools 顺序不因轮次变化）。
- Usage 统计透传 `cache_creation_input_tokens` / `cache_read_input_tokens`（Anthropic）与 `cached_tokens`（OpenAI），落库并在 UI token 用量中体现。

**参考**：codex-rs `core/src/client.rs`（`prompt_cache_key`）、`client_common.rs`。

**验收**：连续两轮请求，第二轮 usage 中 cache_read tokens > 0（可用 mock/真实 API 验证序列化正确性 + 单测断言请求体 JSON 结构）。

## G2 真实 usage 驱动的压缩（P0）

**现状**：`src/domain/auto_compact.rs` 用 `rough_token_estimate_chars`（字节折算）估 token，CJK/代码误差大；压缩触发时机不准。

**目标**：
- `turn.rs` 已捕获流末 `Usage`（`persist_and_emit_turn_token_usage`）；将上一轮真实 `input_tokens + output_tokens` 作为当前上下文规模的权威值，喂给 auto_compact 触发判断。
- 字符估算仅作首轮（无 usage 可用）fallback，并显式标注。
- 增量修正：本轮新增消息用估算叠加到上轮真实值上。

**参考**：codex-rs `core/src/compact_token_budget.rs`、`context_manager/`。

**验收**：单测覆盖「上轮 usage 权威值 + 新消息增量」触发路径；含 CJK 长文本用例证明相对纯估算的行为差异。

## G3 流中断重试（P1）

**现状**：`commands/chat/turn.rs` 的 `connect_with_retry` 只在建连阶段重试；SSE 中途断开整轮作废。

**目标**：
- 流消费循环外包一层重试：可重试错误（网络断开、429/529、5xx）在**尚未产生已提交副作用**（未完成任何工具调用落库）时，丢弃部分累积文本、指数退避后整轮重发。
- 已发出 `chat-stream-*` 部分文本的情况，重发前向前端 emit 一个 reset/replace 事件，避免 UI 重复拼接。
- 重试上限与退避与现有 `connect_with_retry` 常量统一。

**参考**：codex-rs `core/src/responses_retry.rs`（`handle_retryable_response_stream_error` 返回后重入请求循环）。

**验收**：单测模拟中途 `Err(Network)` 的 stream，断言发生重发且最终文本不重复。

## G4 持久化 exec 会话（P1）

**现状**：每次 bash 调用 `tokio::process::Command` 冷启动新进程；无进程复用、无交互式会话；大输出无 head+tail 有界缓冲。

**目标**：
- 新模块 `src/domain/tools/exec_session/`：进程管理器（session id → 长驻 shell 进程），支持写 stdin、带超时读输出、显式关闭、进程退出自动清理。
- head+tail 有界输出缓冲（参考 codex `head_tail_buffer`）：超限时保留头尾、中间截断标记。
- bash 工具增加可选 `session_id` 参数：带此参数走会话复用路径；不带保持现状（一次性执行）。
- 会话表挂在 AppState，进程句柄用 tokio 管理，会话空闲超时自动回收。

**参考**：codex-rs `core/src/unified_exec/`（`process_manager.rs`、`head_tail_buffer.rs`）。

**验收**：集成测试：同一 session_id 两次调用共享 shell 状态（第一次 `export FOO=1`，第二次 `echo $FOO` 得到 `1`）；大输出被有界截断。

## G5 工具并行性声明化（P1）

**现状**：两处重复且脆弱的判定——`commands/chat/subagent.rs:1617` `is_parallelizable_tool`（工具名白名单 + MCP 名称含 `__send`/`__create` 启发式）与 `domain/tools/mod.rs` 尾部的并发安全函数。

**目标**：
- `ToolSchema` 增加 `concurrency_safe: bool` 字段（或注册表级映射），每个工具在 `schema()` 处声明。
- MCP 工具优先读 MCP 协议的 `annotations.readOnlyHint`；无 annotation 时保守串行。
- 删除两处名称启发式，`tool_exec.rs` 统一从声明读取。

**参考**：codex-rs `core/src/tools/registry.rs`（`tool_supports_parallel`）。

**验收**：单测：声明为 safe 的工具进并行批、未声明的串行；MCP readOnlyHint 生效；旧白名单函数删除后编译通过。

## G6 本地沙箱（P2）

**现状**：`domain/tools/bash.rs` 明确无沙箱层，`dangerously_disable_sandbox` 被忽略；本地要么裸跑要么走远端（延迟高）。

**目标**：
- macOS：`sandbox-exec` + 生成的 seatbelt policy（默认：全盘只读、写入白名单 = cwd + scratchpad + TMPDIR，网络按配置开关）。
- `dangerously_disable_sandbox: true` 真正生效（跳过沙箱）。
- Linux/Windows：本期返回明确的「平台不支持，走无沙箱」并记录日志——这是诚实降级，不是伪实现；landlock 列入后续。
- 沙箱失败（policy 拒绝）时错误信息可读，提示可用 `dangerously_disable_sandbox` 逃生。

**参考**：codex-rs `sandboxing/src/`（`manager.rs`、`seatbelt.rs`）。

**验收**：macOS 集成测试：沙箱内写 cwd 成功、写 `~/` 外部路径失败；disable 标志生效。

## G7 流式 emit 合帧（P2）

**现状**：`turn.rs` 每个文本 chunk 一次 `app.emit`（Tauri IPC 到 webview），高速输出事件洪泛。

**目标**：文本/thinking chunk 按 ~30ms 或 2KB 阈值合并后 emit；工具事件（ToolStart/Stop）不合帧保持即时；流结束强制 flush。

**验收**：单测：连续小 chunk 被合并；顺序不变；结尾无丢失。

## G8 / G9（后续迭代）

- **G8 拆分巨石文件**：`commands/chat/mod.rs`（5618 行）与 `tool_exec.rs`（3108 行）按阶段拆分（路由/编排/持久化分层），消除 `*_for_spawn` 逐字段 clone。风险高，安排在 G1–G7 稳定后单独迭代。
- **G9 OTel 埋点**：turn 级 span（连接耗时、首 token、工具耗时、压缩耗时），参考 codex-rs `otel/`。

---

## 执行纪律

1. 所有子任务由 codex 执行，Claude 负责任务书、验收与 review。
2. Review 检查项：`todo!`/`unimplemented!`/空函数体/假测试（无断言）/被注释掉的逻辑/与任务书偏离。
3. 每项完成后运行 `cargo check` + 相关单测，更新本文档状态表。
