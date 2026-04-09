# 内置工具：`getAllBaseTools()`（Claude Code）与 Omiga 对照

主仓库权威列表：`src/tools.ts` → `getAllBaseTools()`。Omiga 侧注册：`omiga/src-tauri/src/domain/tools/mod.rs` → `all_tool_schemas(include_skill)`（并在 `Tool` enum / `from_json_str` 中解析）。

**约定**

- **同名 / 同能力**：Rust 工具名一般为 snake_case（如 `todo_write`），与 TS `name` 对齐或兼容多种别名（见各工具 `from_json_str`）。
- **条件工具**：TS 大量依赖 `process.env`、Ant 构建、`isTodoV2Enabled()` 等；Omiga 采用固定子集 + 文档说明差异。
- **MCP 动态工具**：TS 在 `assembleToolPool` 合并 MCP。Omiga 在 **`send_message`** 时对合并后的 `mcp.json` 服务器并行执行 **`tools/list`**，将工具以 **`mcp__{normalize(server)}__{normalize(tool)}`** 命名并入 LLM 工具表（与 `buildMcpToolName` 一致）；执行时走 **`tools/call`**（`domain/mcp_tool_pool.rs`、`mcp_tool_dispatch.rs`、`mcp_names.rs`）。内置工具同名优先，不覆盖。另：**`resources/list`** / **`resources/read`** 仍由 `list_mcp_resources` / `read_mcp_resource` 提供。

## 核心工具（两边均作为“默认池”的一部分）

| TS（`getAllBaseTools`） | Omiga（`all_tool_schemas`） | 备注 |
|-------------------------|-----------------------------|------|
| `AgentTool` | `agent` | |
| `TaskOutputTool` | `task_output` | |
| `BashTool` | `bash` | |
| `GlobTool` | `glob` | TS 在 embedded search 模式下可能省略 Glob/Grep；Omiga **始终**注册 |
| `GrepTool` | `ripgrep` | 同上（对外名称；实现为 ripgrep 语义） |
| `ExitPlanModeV2Tool` | `exit_plan_mode` | |
| `FileReadTool` | `file_read` | |
| `FileEditTool` | `file_edit` | |
| `FileWriteTool` | `file_write` | |
| `NotebookEditTool` | `notebook_edit` | |
| `WebFetchTool` | `web_fetch` | |
| `TodoWriteTool` | `todo_write` | 会话级列表；Omiga 持久化见下 |
| `WebSearchTool` | `web_search` | |
| `TaskStopTool` | `task_stop` | |
| `AskUserQuestionTool` | `ask_user_question` | |
| `SkillTool` | `skill` | 仅当项目存在 `SKILL.md` 时 `include_skill == true` |
| `EnterPlanModeTool` | `enter_plan_mode` | |
| `TaskCreateTool` … `TaskListTool` | `task_create`, `task_get`, `task_update`, `task_list` | TS 受 `isTodoV2Enabled()` 控制；Omiga **始终**注册 |
| `getSendMessageTool()` | `send_user_message` | |
| `SleepTool` | `sleep` | |
| `ListMcpResourcesTool` | `list_mcp_resources` | Omiga：合并 `mcp.json`；无 `server` 则并行拉取；每条资源带 `server`；失败项在 `_errors` |
| `ReadMcpResourceTool` | `read_mcp_resource` | Omiga：MCP 返回 `{ contents: [...] }`；`blob` 落盘到会话 `tool-results`，`blobSavedTo` + 说明文案同 TS；`http(s)` 亦包成 `contents` |
| `ToolSearchTool` | `tool_search` | TS 可能按功能开关省略；Omiga **始终**注册 |

## Claude Code 有、Omiga **未**实现的工具（节选）

以下出现在 `getAllBaseTools()` 或其条件分支中，当前 **无** 对等的 Rust `Tool`：

| 类别 | 示例（TS） |
|------|------------|
| Ant / 内部 | `ConfigTool`, `TungstenTool`, `REPLTool` |
| 协作 / 产品 | `SuggestBackgroundPRTool`, `WebBrowserTool`, `ListPeersTool`, `getTeamCreateTool` / `getTeamDeleteTool` |
| 计划 / 验证 | `VerifyPlanExecutionTool`, `WorkflowTool`, `BriefTool` |
| 开发与诊断 | `OverflowTestTool`, `CtxInspectTool`, `TerminalCaptureTool`, `LSPTool`, `TestingPermissionTool` |
| Git / 工作区 | `EnterWorktreeTool`, `ExitWorktreeTool` |
| Cron / 远程 / 通知 | `cronTools`, `RemoteTriggerTool`, `MonitorTool`, `PushNotificationTool`, `SubscribePRTool` |
| Shell 变体 | `getPowerShellTool()` |
| 其它 | `SnipTool`, `SendUserFileTool` |

若需 parity，按功能逐工具补 `domain/tools/*.rs` 并在 `Tool` / `all_tool_schemas` 中注册。

## 会话状态：`todo_write` 与 V2 `agent_tasks`

| 能力 | Claude Code | Omiga |
|------|-------------|--------|
| 存储 | 运行时 / 会话状态（依实现） | `SessionRuntimeState` 内 `Arc<Mutex<Vec<...>>>` |
| 跨 `send_message` 持久化 | 依 TS 存储 | SQLite 表 `session_tool_state`：`todos_json`、`agent_tasks_json`；每轮工具执行后 `upsert`，加载会话时 `get_session_tool_state` |

相关代码：`omiga/src-tauri/src/domain/persistence/mod.rs`，`omiga/src-tauri/src/commands/chat.rs`（`persist_session_tool_state`）。

## MCP 配置发现与 resources

| 能力 | Claude Code | Omiga |
|------|-------------|--------|
| 服务列表来源 | MCP 宿主连接的服务器 | `~/.cursor/mcp.json` 与 `<project>/.cursor/mcp.json` 顶层 `mcpServers` 键名（`domain/mcp_discovery.rs`） |
| `resources/list` / `read` | 长连 MCP 客户端 | 每次工具调用内 **按需连接**（stdio 或 `url` HTTP），超时同 `ToolContext`（上限 120s） |

## 权限拒绝规则（`permissions.deny`）

| 能力 | Claude Code | Omiga |
|------|-------------|--------|
| 配置来源 | `ToolPermissionContext.alwaysDenyRules`（多路 settings 合并） | `~/.claude/settings.json`、 `~/.claude/settings.local.json`、`<project>/.claude/settings.json`、`<project>/.claude/settings.local.json` 中的 `permissions.deny` 字符串数组（合并）；另 **`<project>/.omiga/permissions.json`** 顶层 **`deny`** 数组（Omiga「设置 → 权限」写入，与上述 **合并**，`append_omiga_project_permissions`） |
| 工具列表过滤 | `filterToolsByDenyRules` → `getDenyRuleForTool` / `toolMatchesRule`（仅无 `ruleContent` 的整条规则） | `domain/tool_permission_rules.rs`：`filter_tool_schemas_by_deny_rules` 作用于内置 + MCP `ToolSchema` |
| MCP 服务级规则 | `mcp__server` 或 `mcp__server__*` 匹配该服务器全部工具 | `mcp_info_from_string`（与 TS `mcpInfoFromString` 一致） |
| 执行期拦截 | 同上（模型侧已过滤；运行时再拦一层） | `execute_tool_calls` 对任意工具名检查；**Tauri `execute_tool` IPC** 同样 `matching_deny_entry`（内置 `Tool` 枚举；无 MCP `mcp__*`） |
| 内置别名 | `getToolNameForPermissionCheck` + `normalizeLegacyToolName` | `canonical_permission_tool_name`：Claude 风格名（如 `Bash`、`Read`）与 Omiga 线名（`bash`、`file_read`）对齐 |

相关代码：`omiga/src-tauri/src/domain/tool_permission_rules.rs`、`mcp_names.rs`、`commands/chat.rs`、`commands/tools.rs`（`execute_tool`）、`commands/permissions.rs`（`get_omiga_permission_denies` / `save_omiga_permission_denies`，仅读写 `.omiga/permissions.json`）。

### 配置示例与调试

在 **`~/.claude/settings.json`** 或 **`<project>/.claude/settings.local.json`** 中写入与 Claude Code 相同的 `permissions` 字段，例如：

```json
{
  "permissions": {
    "deny": ["Bash", "Read", "mcp__user-Figma", "mcp__plugin-playwright__*"]
  }
}
```

说明：

- **`Bash`**、`**Read`** 等会与 Omiga 内置名（`bash`、`file_read`）对齐后再判断是否拒绝。
- **`mcp__server`**：拒绝该 MCP 服务器下全部工具；**`mcp__server__*`** 同上（显式通配）。
- 带参数的细粒度规则（如 **`Bash(rm:*)`**）不参与「整工具」列表过滤，与 Claude Code 的 `toolMatchesRule` 一致。

调试日志（需 **debug** 级别）：

- 目标 **`omiga::permissions`**：合并后的规则条数、每个设置文件加载的条数、从 LLM 工具表中剔除的工具名及命中的规则与文件路径、执行期拦截同上。
- 启动 Tauri 时可在环境中设置 `RUST_LOG=omiga::permissions=debug`（或 `RUST_LOG=debug`）查看。

配置校验（**warn**）：

- 设置文件 JSON 解析失败。
- **`permissions.deny`** 中空字符串。
- 规则含 **`(`** 但字符串不以 **`)`** 结尾（可能被误解析为单个工具名）。
- 单条规则过长（> 2048 字符）。

## 相关路径

- TS：`src/tools.ts`（`getAllBaseTools` / `getTools` / `assembleToolPool`）
- Omiga：`omiga/src-tauri/src/domain/tools/mod.rs`、`omiga/src-tauri/src/commands/chat.rs`
- Skill 细表：`docs/SKILL_TOOL_PARITY.md`
