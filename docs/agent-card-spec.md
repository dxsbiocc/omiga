# Agent Card Spec

Agent Card 是带 YAML front matter 的 Markdown 文件，运行时由 registry 读取。

## 文件结构

```md
---
id: seeker.web_research
name: Seeker
version: 1.0.0
category: retrieval
description: 检索、筛选、摘录和归纳外部资料
use_when:
  - 需要外部资料、论文、网页、新闻、文档检索
avoid_when:
  - 已有完整上下文，不需要检索
capabilities:
  - web_search
  - evidence_extraction
tools:
  allowed:
    - web_search
  forbidden:
    - shell
permissions:
  read:
    - web
  write:
    - evidence_store
  execute: []
  external_side_effect: []
  human_approval_required: false
memory_scope:
  read:
    - task_context
  write:
    - evidence_store
context_policy:
  max_input_tokens: 6000
  include:
    - global_context
    - task_spec
  exclude:
    - full_conversation_history
  summarization_required: true
input_schema:
  type: object
output_schema:
  type: object
handoff_targets:
  - analyzer.data
failure_modes:
  - 来源质量不足
success_criteria:
  - 每个关键结论至少有来源
evals:
  - source_quality_eval
enabled: true
---

你是 Seeker，负责检索、证据筛选和来源归纳。
只返回和任务目标相关的证据。
```

## 核心字段

- `id`: 稳定唯一标识，例如 `reporter.final`
- `name`: 展示名称
- `version`: 版本号
- `category`: 路由与 mock runner 分类
- `description`: 简要说明
- `use_when`: 适用场景
- `avoid_when`: 不适用场景
- `capabilities`: 可搜索能力标签
- `handoff_targets`: 允许的下游目标
- `failure_modes`: 预期失败方式
- `success_criteria`: 此 card 的完成标准
- `evals`: 对应评估钩子
- `enabled`: 是否启用

## Tools / Permissions / Context

`tools` 表示 agent 可以请求哪些工具：

- `allowed`
- `forbidden`

`permissions` 表示 executor 最多允许这个 agent 请求哪些作用域：

- `read`
- `write`
- `execute`
- `external_side_effect`
- `human_approval_required`

`context_policy` 用来约束 `ContextAssembler`：

- `max_input_tokens`
- `include`
- `exclude`
- `summarization_required`

## Input / Output Schema

MVP 里 `input_schema` 和 `output_schema` 仍是轻量 JSON/YAML 结构，便于后续逐步升级为更严格的 schema validation。

## 版本化策略

- registry 按 `id` 维护多个版本
- 新注册的更高版本会成为 active 版本
- 可以 disable 某个 agent，而不删除历史
- Creator proposal 在显式审批前不能成为 production card

## 安全约束

- 不要默认给 specialist card “创建子 Agent”的能力
- 不要把高风险权限只写在正文说明里，必须写进 front matter
- `use_when` 尽量窄，`avoid_when` 尽量明确
- 高风险 write / execute / external side effect 应通过 `human_approval_required` 明示
