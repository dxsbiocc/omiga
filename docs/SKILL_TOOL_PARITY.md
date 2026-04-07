# Skill 工具：Claude Code (`SkillTool`) 与 Omiga 对照表

主仓库参考：`src/tools/SkillTool/SkillTool.ts`、`src/skills/loadSkillsDir.ts`、`src/utils/argumentSubstitution.ts`。

| 能力 | Claude Code (`SkillTool`) | Omiga |
|------|---------------------------|--------|
| **工具名** | `Skill`（`SKILL_TOOL_NAME`） | 接受 `skill` 或 `Skill`（不区分大小写） |
| **输入** | `skill: string`, `args?: string` | 相同；`arguments` 作为 `args` 的别名 |
| **名称规范化** | trim、去掉前导 `/` | `normalize_skill_name` 同上 |
| **技能来源** | 本地 `SKILL.md`、插件、bundled、MCP prompts（`loadedFrom === mcp`）等 | **仅** 用户目录与项目下 `SKILL.md`（`load_skills_for_project` 搜索顺序与 TS 文档一致） |
| **发现列表** | `getSkillToolCommands` + 预算截断 | `format_skills_system_section`（描述截断 250 字符） |
| **校验** | `validateInput`：存在、`prompt` 类型、`disableModelInvocation` 等 | 存在性、`disable-model-invocation` 为真时拒绝 |
| **内联执行** | `processPromptSlashCommand` → `newMessages` 注入对话 + `contextModifier`（allowedTools、model、effort） | **不注入新消息**；在**一条 tool 结果**中返回：`Launching skill:` + **JSON 元数据** + `---` + 展开后的全文（含 `Base directory…`） |
| **allowed-tools** | 写入权限上下文 `alwaysAllowRules` | 仅出现在返回的 JSON 中；**不自动收窄会话工具**（见 `_omiga` 说明） |
| **model / effort / agent** | 可覆盖主循环模型、effort、fork agent 类型 | 元数据在 JSON 中透出；**不自动改 Omiga 会话模型** |
| **Fork（`context: fork`）** | `runAgent` 子代理、`executeForkedSkill` | **不跑子代理**；`status: fork_unsupported`，仍附带完整 skill 正文供当前会话使用 |
| **远程 / canonical 技能** | Ant + `EXPERIMENTAL_SKILL_SEARCH` | **未实现** |
| **参数替换** | `substituteArguments` + `shell-quote` 解析 | `$ARGUMENTS`、`${ARGUMENTS}`、`$ARGUMENTS[n]`、`$0`…、`$name`（frontmatter `arguments`）；使用 **`shell-words` 解析** `args` |
| **目录占位符** | `${CLAUDE_SKILL_DIR}` 等 | `${CLAUDE_SKILL_DIR}`、`${OMIGA_SKILL_DIR}` → 技能目录 |
| **权限 / Ask** | `checkPermissions`、规则 allow/deny | **未实现**（会话层无同款规则引擎） |
| **遥测** | `tengu_skill_tool_invocation` 等 | 无 |
| **Hooks / shell 块** | `hooks`、`executeShellCommandsInPrompt` 等 | **未执行** shell 或 hooks |

## 返回 JSON 字段（Omiga 内联 / fork 分支）

内联：`status: "inline"`。Fork 配置：`status: "fork_unsupported"`，并带 `_omiga` 说明。

公共字段示例：`success`、`commandName`、`allowedTools`、`model`、`effort`、`agent`、`userInvocable`、`_omiga`。

## 相关代码路径（Omiga）

- `omiga/src-tauri/src/domain/skills/mod.rs` — 加载、解析、替换、`invoke_skill` / `invoke_skill_detailed`
- `omiga/src-tauri/src/domain/tools/skill_invoke.rs` — LLM 可见的 `skill` 工具 schema
- `omiga/src-tauri/src/commands/chat.rs` — `skill` / `Skill` 工具执行与流式结果
