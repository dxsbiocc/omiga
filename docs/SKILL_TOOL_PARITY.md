# Skill 工具：Claude Code (`SkillTool`) 与 Omiga 对照表

主仓库参考：`src/tools/SkillTool/SkillTool.ts`、`src/skills/loadSkillsDir.ts`、`src/utils/argumentSubstitution.ts`。

| 能力 | Claude Code (`SkillTool`) | Omiga |
|------|---------------------------|--------|
| **工具名** | `Skill`（`SKILL_TOOL_NAME`） | 接受 `skill` 或 `Skill`（不区分大小写） |
| **输入** | `skill: string`, `args?: string` | 相同；`arguments` 作为 `args` 的别名 |
| **名称规范化** | trim、去掉前导 `/` | `normalize_skill_name` 同上 |
| **技能来源** | 本地 `SKILL.md`、插件、bundled、MCP prompts（`loadedFrom === mcp`）等 | 用户目录 → 已启用 Omiga-native 插件 skill roots → 项目目录；bundled / MCP prompt / 远程 canonical 技能仍未实现 |
| **发现列表** | `getSkillToolCommands` + 预算截断 | `format_skills_system_section`（描述截断 250 字符） |
| **校验** | `validateInput`：存在、`prompt` 类型、`disableModelInvocation` 等 | 存在性、`disable-model-invocation` 为真时拒绝 |
| **内联执行** | `processPromptSlashCommand` → `newMessages` 注入对话 + `contextModifier`（allowedTools、model、effort） | **不注入新消息**；在**一条 tool 结果**中返回：`Launching skill:` + **JSON 元数据** + `---` + 展开后的全文（含 `Base directory…`） |
| **allowed-tools** | 写入权限上下文 `alwaysAllowRules` | 内联时返回 JSON 元数据；chat 批处理会解析 `allowedTools` 并限制同一顺序批次里的后续工具，但仍不是 Claude Code 的会话级权限上下文。Fork 时用于过滤子代理可见工具 schema。 |
| **model / effort / agent** | 可覆盖主循环模型、effort、fork agent 类型 | 元数据在 JSON 中透出；当前不会自动改 Omiga 主会话模型，fork 路径也主要继承现有 agent runtime 配置，尚未做到与 Claude Code 完全等价。 |
| **Fork（`context: fork`）** | `runAgent` 子代理、`executeForkedSkill` | `invoke_skill_detailed` 返回 `status: "needs_fork"`；chat 层在有 agent runtime 时路由到 forked sub-agent，缺 runtime 时回退 inline。 |
| **远程 / canonical 技能** | Ant + `EXPERIMENTAL_SKILL_SEARCH` | **未实现** |
| **参数替换** | `substituteArguments` + `shell-quote` 解析 | `$ARGUMENTS`、`${ARGUMENTS}`、`$ARGUMENTS[n]`、`$0`…、`$name`（frontmatter `arguments`）；使用 **`shell-words` 解析** `args` |
| **目录占位符** | `${CLAUDE_SKILL_DIR}` 等 | `${CLAUDE_SKILL_DIR}`、`${OMIGA_SKILL_DIR}` → 技能目录 |
| **权限 / Ask** | `checkPermissions`、规则 allow/deny | **未实现**（会话层无同款规则引擎） |
| **遥测** | `tengu_skill_tool_invocation` 等 | 无 |
| **Hooks / shell 块** | `hooks`、`executeShellCommandsInPrompt` 等 | **未执行** shell 或 hooks |

## 返回 JSON 字段（Omiga 内联 / fork 分支）

内联：`status: "inline"`。Fork 配置：`status: "needs_fork"`；chat 执行层成功派生后会返回
`status: "forked"` 的工具结果，缺少 agent runtime 时回退为 inline。

公共字段示例：`success`、`commandName`、`allowedTools`、`model`、`effort`、`agent`、`userInvocable`、`_omiga`。

## 当前剩余差异

- Plugin skill roots 已纳入扫描和缓存戳计算，但 bundled / MCP prompt / 远程 canonical 技能仍未接入。
- Inline skill 的 `allowed-tools` 是批处理级约束，不是持久会话权限策略，也不会写入 `alwaysAllowRules`。
- Fork skill 已有子代理路径，但 `model` / `effort` / `agent` override 仍未与 Claude Code 完全等价。
- Inline 与 fork 都会暴露 frontmatter 元数据；不要把该元数据等同于已经完整应用的运行时配置。

## 相关代码路径（Omiga）

- `omiga/src-tauri/src/domain/skills/mod.rs` — 加载、解析、替换、`invoke_skill` / `invoke_skill_detailed`
- `omiga/src-tauri/src/domain/tools/skill_invoke.rs` — LLM 可见的 `skill` 工具 schema
- `omiga/src-tauri/src/commands/chat/tool_exec.rs` — `skill` / `Skill` 工具执行、inline allowed-tools 批处理约束、fork 路由
- `omiga/src-tauri/src/commands/chat/subagent.rs` — forked skill 子代理执行
