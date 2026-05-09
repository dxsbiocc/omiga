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

## 产品原则

- 面向用户：提示“发现可固化经验，是否保存？”，而不是展示 trace 细节。
- 面向 agent：保留 `sourceRecordIds`、`evidence`、`recommendationActions`，便于自主学习
  agent 后续判断如何应用。
- 避免噪音：第一版不从失败记录、child cleanup 或纯 trace 中生成用户弹窗建议。
- 保持边界：本机制只产生与记录建议，不改变已有内置工具函数和插件实现。

## 后续提升

1. 将 `approved` 建议接入真正的 apply 流程：
   - 保存为项目偏好；
   - 写入 Template 默认值候选/示例；
   - 封存成功结果目录。
2. 前端接入轻量弹窗/通知，只展示 `userMessage` 和确认按钮。
3. 学习 agent 周期性查看 `.omiga/learning/proposals.json`，自动给出合并、忽略或 apply
   建议。
