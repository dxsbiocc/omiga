# Omiga Agent 响应内核 — Engineering Review Plan

Generated: 2026-04-02  
Scope: `omiga/src-tauri` 聊天持久化与工具输出，`omiga/src` 前端消息展示；参考 `claude-code-main` 根目录下 `src/query.ts`、`src/utils/toolResultStorage.ts`、`src/utils/messages.ts`（`handleMessageFromStream`）。

## Step 0: Scope Challenge

### What already exists (reuse, do not duplicate poorly)

| Layer | TypeScript (参考) | Omiga (当前) |
|--------|-------------------|--------------|
| 大工具结果落盘 | `maybePersistLargeToolResult` → `persistToolResult`；MCP 用 `ENABLE_MCP_LARGE_OUTPUT_FILES` 控制是否落盘；失败时 MCP 用错误文案回退 | `constants/tool_limits.rs` 已与 TS 数值对齐；流式预览用 `PREVIEW_SIZE_BYTES`；**未实现** 完整 `persistToolResult` + `OMIGA_ENABLE_LARGE_TOOL_OUTPUT_FILES` 接线（见上节 TS 为准） |
| 消息流 | `Message[]` 含 assistant blocks（text + tool_use）+ user tool_result；`useLogMessages` 写 transcript | `SessionCodec::to_api_messages` 正确；**内存** `Session::to_api_messages` 仍错误（assistant 无 tool、tool 用 text），但 `chat.rs` 已用 `SessionCodec` |
| 持久化 | JSONL 多行，含完整 message 图 | SQLite `save_message(role, content, tool_calls, tool_call_id)` **schema 支持** `tool_calls`，但 **写入路径未用满** |

### 最小改动集（目标）

1. **持久化与 `queryLoop` 语义对齐**：每一轮 assistant（含 tool_calls）与每条 tool 结果、以及 **后续** assistant 回复，**必须**落库；重载 `load_session` 后前端能还原完整链。
2. **大工具输出**：对齐 TS：超阈值 → 写 `tool-results/`，消息体保留预览 + 路径（模型读文件用 `read_file`）。
3. **前端**：`sessionStore` 已能映射 `tool_calls` / `tool` + `output`；需保证后端 **总是** 返回与线上一致的结构（含 `round_id` 可选）。

### 复杂度检查

- 主要改动集中在 `omiga/src-tauri/src/commands/chat.rs` 与少量 `Session`/`execute_tool_calls` 辅助；**不**引入新服务类，优先函数内聚 + 已有 `SessionCodec`。
- 若单文件超过 ~400 行新增，再抽 `persist_round.rs` 模块。

### Search / Layer

- 大结果落盘：优先复刻 TS 行为（Layer 1），**不要** invent 新格式；路径与标签可对齐 `PERSISTED_OUTPUT_TAG` 或 Omiga 约定一种稳定前缀便于前端解析。
- 多轮工具循环：`chat.rs` 当前只执行 **一轮** tool → 一次 `stream_llm_response`；若模型在 follow-up 中再次 `tool_use`，需 **循环** 直到无工具或上限（与 `queryLoop` 一致）。此为 **范围内** 若当前产品承诺「多轮 agent」。

### Completeness

- **推荐完整方案**：所有 assistant 轮次 + tool_calls JSON + tool 行 + 大结果文件引用 + 回归测试（见下）。Completeness: **9/10**。
- 捷径：只修「第二轮 assistant 落库」不修 tool_calls 与多轮循环（**7/10**，重载仍缺工具参数展示）。

---

## 原则：大内容不进 agent 全文，先落盘再按需注入

与 TS 一致：**禁止**把超大工具输出整段塞进发给模型的 `tool_result` / user 消息。

1. **落盘**：完整内容写到会话目录（如 `tool-results/{tool_use_id}.txt`），与 `persistToolResult` + `buildLargeToolResultMessage` 同思路。
2. **注入模型的文本**：只放 **短指令**（路径、格式、如何用 `offset`/`limit`/搜索/`jq` 分块读），见 `src/utils/mcpOutputStorage.ts` 的 `getLargeOutputInstructions`。
3. **模型侧**：用 **Read**（或等价）按块读取，只把**与当前任务相关**的片段纳入推理；全量「读完」的要求写在 TS 指令里是为了审计类任务，一般任务可先搜索再读对应段。

Omiga 已移植：`omiga/src-tauri/src/utils/large_output_instructions.rs` → `get_large_output_instructions`（与 TS 文案对齐，便于行为一致）。

---

## TypeScript 为准：大结果、落盘失败、沙箱 / 禁用写文件

处理方式以主仓库 TS 为准；Omiga 只增加 **环境开关** 与 **失败回退**，不发明新语义。

### 权威源码

| 行为 | TS 位置 |
|------|---------|
| 阈值、预览字节数、`MAX_TOOL_RESULT_BYTES` | `src/constants/toolLimits.ts`，`src/utils/toolResultStorage.ts`（`maybePersistLargeToolResult`、`generatePreview`、`buildLargeToolResultMessage`） |
| 禁用「大结果写文件」→ 仅截断 | `src/services/mcp/client.ts` → `processMCPResult`：`isEnvDefinedFalsy(process.env.ENABLE_MCP_LARGE_OUTPUT_FILES)` 为真时走 `truncateMcpContentIfNeeded` |
| 写文件失败 | 同文件：`isPersistError(persistResult)` → 返回 **明确错误文案** + 建议分页/过滤，**不**把整段超大正文塞回模型 |
| 通用 `maybePersistLargeToolResult` | `persistToolResult` 失败时 TS 返回 **原 block**（可能仍很大）；**Omiga 实施时优先采用 MCP 路径的「错误 + 截断提示」**，避免沙箱拒写盘时仍把整段送进上下文 |

### Omiga 对齐（已实现 / 计划用）

- **常量**：`omiga/src-tauri/src/constants/tool_limits.rs` 与 `toolLimits.ts` 数值一致。
- **环境变量**：`OMIGA_ENABLE_LARGE_TOOL_OUTPUT_FILES` 语义对齐 `ENABLE_MCP_LARGE_OUTPUT_FILES`（`isEnvDefinedFalsy`）：未设置 = 允许尝试落盘；`0` / `false` / `no` / `off` = **不写大结果文件**，只走内存截断（与 MCP 在 env 关闭时一致）。
- **落盘失败**：使用 `large_output_persist_failed_message`（文案风格对齐 MCP `persist_failed` 分支），不要静默回传完整超大字符串。
- **沙箱 / CI**：若会话目录不可写，可设置 `OMIGA_ENABLE_LARGE_TOOL_OUTPUT_FILES=0`，避免无意义的 write 重试；或依赖「persist Err → MCP 式回退」。

### 计划调整说明

- 不再要求「必须写文件」作为唯一路径；**与 TS 一样**，允许「仅截断」分支。
- 评测或 Cursor 沙箱：在文档/CI 中注明上述 env，避免与「禁止写文件」冲突。

---

## Root Cause（已验证）

### 1. 首轮 assistant 未写入 `tool_calls`

```323:328:omiga/src-tauri/src/commands/chat.rs
        // Save assistant message to database
        let assistant_msg_id = uuid::Uuid::new_v4().to_string();
        {
            let repo = repo_clone.lock().await;
            if let Err(e) = repo
                .save_message(&assistant_msg_id, &session_id_clone, "assistant", &assistant_text, None, None)
```

`completed_tool_calls` 已收集，但 **`None` 未传 tool_calls**。`SessionCodec::record_to_message` 读库时 `tool_calls` 为空 → 前端无法还原「哪些工具被调用」。

### 2. 内存 `Session::add_assistant_message` 永远 `tool_calls: None`

```38:44:omiga/src-tauri/src/domain/session/mod.rs
    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::Assistant {
            content: content.into(),
            tool_calls: None,
        });
```

与 DB 不一致，且后续 `SessionCodec::to_api_messages` 依赖 domain 中的 `tool_calls`。

### 3. Follow-up `stream_llm_response` 结果未落库

```430:439:omiga/src-tauri/src/commands/chat.rs
            // Stream the follow-up response
            let _ = stream_llm_response(
                client.as_ref(),
                &app_clone,
                &message_id_clone,
                &updated_llm_messages,
                &tools,
                &pending_tools_clone,
            )
            .await;
```

返回值丢弃，**无 `save_message`**。重载后用户只能看到：user → assistant(首轮) → tool(s)，**缺少最终总结 assistant**（与「只能看到最终总结」的体感可能相反：实际是 **中间有、最终无**；若 UI 只高亮最后一条，则表现为「只剩工具链/半截」）。无论哪种，** transcript 不完整**。

### 4. 多轮工具循环

若 follow-up 中再次产生 `tool_calls`，当前 `stream_llm_response` 只跑 **一次**，未实现 `while` 工具循环（与 `query.ts` 的 `queryLoop` 不一致）。

### 5. 大输出

`execute_tool_calls` 中 `MAX_OUTPUT: 10000` 仅用于发往事件的 `display_output`；**入库**为全文。应对齐 TS：**超阈值** 写文件，DB 存预览 + 路径，避免 SQLite 行过大与模型上下文浪费。

---

## Architecture（ASCII）

### 目标数据流（与 TS 对齐）

```
User send
  → persist user row
  → loop:
       stream assistant
       → persist assistant (content + tool_calls JSON if any)
       → if no tools: break
       → for each tool: execute → maybe persist large → persist tool row
       → build messages for API → next stream iteration
  → emit Complete
```

### 当前 Omiga（缺陷）

```
stream once → save assistant (no tool_calls!)
  → tools → save tool rows
  → stream_llm_response → **no save**
```

---

## Opinionated Recommendations

1. **RECOMMENDED**: 在 `chat.rs` 中抽取 `persist_assistant(repo, session_id, text, tool_calls_opt)` 与 `run_tool_loop(...)`，内部 `while` 直到无工具或上限（如 25 轮，对齐 TS 常量若存在）。
2. **RECOMMENDED**: 扩展 `Session`：`add_assistant_message_with_tools` 或 `add_assistant_message(content, Option<Vec<ToolCall>>)`，保证内存与 DB 一致。
3. **RECOMMENDED**: 大输出：新建 `omiga/src-tauri/src/utils/tool_result_files.rs`（或复用 `get_project_dir()`/`sessionId`），阈值使用 **`crate::constants::tool_limits::DEFAULT_MAX_RESULT_SIZE_CHARS`**（与 TS `src/constants/toolLimits.ts` 一致，50_000），环境变量可覆盖。
4. **Settings parity**: Omiga `constants/tool_limits.rs` mirrors TS `toolLimits.ts` + `PREVIEW_SIZE_BYTES` / `TOOL_DISPLAY_MAX_INPUT_CHARS`（见 `chat.rs` 流式 tool 预览截断）。
5. **DRY**: 删除或修正 `Session::to_api_messages` 若仍有调用方；**统一** `SessionCodec::to_api_messages` 为唯一 API 消息构建器。

---

## Test Review（覆盖图摘要）

### CODE PATH COVERAGE（计划实施后）

| Path | 测试 |
|------|------|
| 首轮 assistant + tool_calls 落库 | 集成：`send_message` → DB 行 `assistant` 含 `tool_calls` JSON |
| tool 行落库 + 重载顺序 | `load_session` 消息顺序与 `message_id` 时间序一致 |
| 第二轮 assistant 落库 | 集成：mock LLM 先返回 tool，再返回纯文本；断言最后一行 `assistant` |
| 大工具输出 | 单元：超阈值写入 `tool-results/`，DB 内容为预览 + 路径 |
| 取消 | 已有 `cancel_stream` 路径保持；不写入 completed assistant |

**COVERAGE 目标**: 新增 3～5 个 Rust 集成/单测（`omiga/src-tauri/tests/` 已有 `session_flow_integration_tests.rs` 可扩展）。

---

## Performance

- 大结果写文件：O(n) 一次写盘；避免把 1MB+ 塞进单行 SQLite。
- 循环轮次：设硬上限防止无限工具循环。

---

## NOT in scope

- 主仓库 Ink/React `REPL.tsx` 全量移植。
- Claude Code `compaction` / `marble-origami` 与 omiga 对齐（可后续单独计划）。
- 修改 `node_modules` 或 SDK。

---

## What already exists（实现时复用）

- `SessionCodec::message_to_record` / `db_to_domain`（`omiga/src-tauri/src/domain/session_codec.rs`）。
- `repo.save_message(..., tool_calls, tool_call_id)`。
- TS 参考：`src/utils/toolResultStorage.ts`（阈值、预览、`getToolResultsDir` 模式）。

---

## 并行化

- **Lane A**: `chat.rs` 持久化 + 工具循环。
- **Lane B**: `tool_result_files` + `execute_tool_calls` 大输出。
- 合并前需同一 PR 或顺序合并（共享 `chat.rs` 时串行）。

---

## Failure modes

| 失败 | 检测 | 用户可见 |
|------|------|----------|
| 磁盘满，大结果落盘失败 | `persist` Err | 回退到截断 + 错误串（对齐 TS） |
| 第二轮 assistant 未保存 | 集成测试 | 重载丢最终回复 |
| tool_calls JSON 损坏 | `record_to_message` 解析失败 | 日志 + 降级为纯文本 assistant |

---

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | — | — |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | — | — |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | **draft** | 见上文 Root Cause 与 Recommendations |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | — | — |

**VERDICT:** 计划文档已就绪；实施前建议用户确认 **多轮工具循环** 上限与 **50k** 阈值是否与产品一致。

---

## Completion Summary

- Step 0: Scope Challenge — **scope 已收紧为 omiga 内核 + 对齐 TS 持久化语义**
- Architecture Review: **issues** — 首轮缺 tool_calls、follow-up 未落库、内存 Session 不一致、缺工具循环
- Code Quality Review: **DRY** — 统一 `SessionCodec` 为唯一 API 视图构建
- Test Review: **gaps** — 第二轮 assistant、tool_calls 往返、大文件
- Performance Review: **1 issue** — 大结果勿进 SQLite 行
- NOT in scope: **已写**
- What already exists: **已写**
- Lake Score: **9/10**（完整持久化 + 文件溢出 + 测试）

**STATUS:** DONE（计划交付）。实施阶段需 **代码** 修改 `chat.rs` / `session/mod.rs` / 新工具文件模块。

---

## Next Steps（实现顺序）

1. 修复 `save_message` 传入 `tool_calls`；`add_assistant_message` 支持 tool_calls。
2. `stream_llm_response`（或统一循环）在流结束后 **persist** assistant；若仍有工具则继续循环。
3. 引入 `maybe_persist_large_tool_output`（对齐 TS 常量）。
4. 扩展集成测试与 `load_session` 前端冒烟。
