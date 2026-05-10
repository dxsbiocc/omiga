# Learning Proposal Flow

Omiga 的自进化闭环不应该把执行路径直接暴露给普通用户。当前第一版采用
**proposal-first** 策略：后台从 `ExecutionRecord` 中识别可固化经验，写入项目级
学习建议；用户只需要看到“是否保存/稍后/忽略”的确认，而不是完整 trace。

## 存储位置

- 数据库来源：`.omiga/execution/executions.sqlite`
- 学习建议存储：`.omiga/learning/proposals.json`

学习建议是项目级、可审计、可撤销的 JSON 记录。保存建议不会自动修改
Operator、Template、Skill 或归档结果目录。

## 工具入口

### `learning_proposal_list`

列出学习建议；当 `refresh=true` 时，会扫描最近的执行记录并持久化新的建议。

参数：

- `refresh`: 是否从最近 `ExecutionRecord` 生成新建议。
- `limit`: `refresh=true` 时扫描的记录数，默认 100，最大 200。
- `includeDecided`: 是否包含已批准、已应用、已忽略或暂缓的建议。

当前自动生成两类用户可理解的建议：

1. `reusable_choice`：执行中捕获了 `user_preflight` 参数选择，可考虑固化为项目偏好、
   模板默认值候选或示例参数。
2. `archive_result`：根执行成功且有 run/provenance/output，可考虑封存为项目结果记录。

### `learning_proposal_decide`

记录用户对某条建议的决定。

参数：

- `proposalId`: `learning_proposal_list` 返回的建议 ID。
- `decision`: `approve`、`dismiss`、`snooze` 或 `mark_applied`。
- `note`: 可选的人类备注。

`approve` 代表用户确认“值得固化”；真正落到 Template、项目偏好或归档目录仍属于后续
apply 流程，避免后台静默改写核心实现。

### `learning_proposal_apply`

将已批准的建议写入项目级固化记录。默认要求建议已经 `approve`，除非显式传入
`allowUnapproved=true`。

当前 apply 仍保持保守边界：它只写入可审计的学习记录，不静默改写 Operator、Template、
Skill，也不移动或删除产物文件。

写入位置：

- `.omiga/learning/applied.json`：统一 apply 记录，描述本次固化的来源和目标。
- `.omiga/learning/preference-candidates.json`：参数/工作流偏好候选；后续可提升为项目偏好
  或 Template 默认值。
- `.omiga/learning/archive-markers.json`：结果封存标记；记录 runDir、provenance 和产物路径，
  后续可由归档 agent 执行真实搬运/复制/清理。

## 轻量用户确认

Chat 窗口在空闲状态会调用 `learning_proposal_next(refresh=false)` 做一次低成本检查；若已存在
pending proposal，则弹出简洁确认框。UI 不在空闲轮询中扫描 ExecutionRecord 或生成新 proposal，
proposal 生成仍由 agent/工具显式触发：

- **保存**：调用 `learning_proposal_respond(action=approve_apply)`，内部串联 approve + apply，
  成功后提示“已保存为项目学习记录”。
- **稍后**：标记为 `snoozed`，避免继续打断当前会话。
- **忽略**：标记为 `dismissed`。

该确认框只显示 `title`、`userMessage` 和按钮，不展示执行 trace。关闭弹窗只在当前会话内
抑制重复提醒，不改变 proposal 状态。

## 产品原则

- 面向用户：提示“发现可固化经验，是否保存？”，而不是展示 trace 细节。
- 面向 agent：保留 `sourceRecordIds`、`evidence`、`recommendationActions`，便于自主学习
  agent 后续判断如何应用。
- 避免噪音：第一版不从失败记录、child cleanup 或纯 trace 中生成用户弹窗建议。
- 保持边界：本机制只产生与记录建议，不改变已有内置工具函数和插件实现。

## 后续提升

1. 学习 agent 周期性查看 `.omiga/learning/proposals.json`，自动给出合并、忽略或 apply
   建议。
2. 将 preference candidates 升级为真实项目偏好或 Template 默认值时，继续要求可审计确认。
