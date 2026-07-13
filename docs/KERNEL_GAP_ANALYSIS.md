# omiga 内核 vs codex core 深度对比与完善路线（2026-07-12）

对照 codex-rs 内核（agent-graph-store/thread-store/execpolicy/rmcp-client/skills/sandboxing/process-hardening/
code-mode 等）与 omiga 现状（N0-N13 后），按 agent 编排、MCP、skill、权限、进程加固、memory、code-mode 七维度。
**结论先行**：omiga 在广度（内置专家 agent、memory、per-tool 权限）领先，在架构统一性（agent 状态）、
深度策略引擎（execpolicy）、服务端 MCP、进程加固上落后。

---

## 1. Agent 编排 —— omiga 广度领先，架构统一性落后

**codex**：`multi_agents`(v1/v2) 作为模型可调的一级工具 spawn/协调子 agent；`agent_jobs` 后台作业；
`agent-graph-store` + `thread-store` 提供**持久化的 agent/thread DAG**；`agent-identity`；`collaboration-mode-templates`。
架构精简统一：单一持久图存储。

**omiga**：`agents/` 下 coordinator/router/model_router/intent_classifier/scheduler/registry/subagent_tool_filter +
personality/hot_reload/prompt_loader，且 `builtins/` 有 15+ 内置专家 agent（architect/code_reviewer/critic/
debugger/deep_research/executor/explore/data_analysis/...）。但状态基座**碎片化**：
blackboard / team_state / ralph_state / autopilot_state / orchestration / research_system 六个并存的状态子系统。

- **领先**：内置专家 agent 阵容、intent_classifier（意图分类路由）、model_router（按任务选模型）、personality、hot_reload。
- **落后**：六个重叠状态基座 vs codex 单一 agent-graph-store；**无 thread 持久化**（跨会话 agent 图不落盘）。
- **完善路线 K1（P1，最大架构债）**：把六个状态子系统盘点去重 → 收敛成统一的持久化 agent/thread store
  （承接 N6-item5 盘点）。先出对比文档定边界，再分阶段迁移。这是 omiga 内核最值得投入的一项。

## 2. MCP —— 客户端成熟，缺服务端 surface

**codex**：`rmcp-client`(官方 Rust MCP SDK,18 文件) 消费 MCP；`codex-mcp` + `mcp-server` 让 **codex 自身作为 MCP server**
暴露给其它 agent；`connectors`。双向。

**omiga**：`domain/mcp/` 有 client/connection_manager/tool_pool/discovery/oauth/tool_dispatch/resource_output —
**客户端侧非常完整**（连接池、OAuth、发现、资源）。但**只有客户端**，无"omiga 作为 MCP server"暴露面。

- **领先/持平**：客户端能力（tool_pool 池化、oauth、connection_manager）不输 codex。
- **落后**：无服务端 surface——omiga 不能把自己的 operator/工具经 MCP 暴露给外部 agent 编排。
- **完善路线 K3（P2）**：加 MCP server surface，把 operator_execute / unit 工具经 MCP 暴露，
  让 omiga 能作为子 agent 挂到 codex/其它 runtime 下。中等工作量，扩展互操作性。

## 3. Skill —— 大致持平，各有侧重

**codex**：`skills/` = loader + injection + config_rules + mention_counts（按提及次数排序）+ invocation_utils。
**omiga**：`domain/skills/` = fuzzy_match（模糊匹配）+ skill_config + skill_guard（**权限门控**）+ skill_manage + skill_view。

- omiga 独有 skill_guard（skill 接权限系统）、fuzzy_match；codex 独有 mention_counts（用量排序）+ injection 调优。
- **完善路线（P3，小）**：可借 codex 的 mention_counts 思路给 skill 加用量感知排序。低优先。

## 4. 权限控制 —— omiga 广（全工具面），codex 深（shell 策略引擎）

**codex**：`execpolicy` = **策略引擎**（parser + rule + policy + decision + **amend**）——一套 shell 命令的
allow/deny/amend 规则 DSL，`amend.rs` 能程序化追加 allow-prefix 规则、把危险命令改写成安全形式。
配 `shell-escalation`（G15 对等）、`process-hardening`。

**omiga**：`domain/permissions/` = patterns + risk_assessment + tool_rules + workspace_guard + compat——
**per-tool 权限**（不止 shell，覆盖全 native 工具）+ risk 分级 + 工作区边界（G12/G15 成果）。
N8a 危险分类器是模式匹配（Block/Warn/Safe），非策略引擎。

- **领先**：per-tool 权限面比 codex 宽（codex execpolicy 主要针对 shell）；risk_assessment + workspace_guard。
- **落后**：无 execpolicy 式的**可配置策略 DSL** + **amend 自动改写**；N8a 是硬编码模式，规则不可持久化/编辑。
- **完善路线 K2（P2，承接 N8/N10）**：把 N8a 分类器 + risk_assessment 演进成 execpolicy 式策略引擎——
  可配置的命令规则（allow/deny/warn/amend）、持久化用户规则、危险命令 amend（改写成沙箱内安全形式）。
  与 N8 审批接线（高危→G15）合流。

## 5. 进程加固 —— omiga 完全缺失

**codex**：`process-hardening` crate——对 codex 进程自身降权/加固（no_new_privs、rlimit、drop privileges）。
**omiga**：**无任何对等**（grep setuid/no_new_privs/rlimit/prctl 零命中）。

- **完善路线 K4（P3，小而实的安全项）**：启动时做进程加固——Unix 下设 no_new_privs、合理 rlimit
  （限制 fd/内存/进程数）、可选降权。小改动、纯安全收益，防子进程提权/资源耗尽扩散到主进程。

## 6. Memory —— omiga 显著领先

**codex**：`memories` crate 较薄。
**omiga**：`domain/memory/` = chat_indexer + dossier + long_term + permanent_profile + source_registry + migration——
**远比 codex 发达**的记忆系统。omiga 优势，无需补。

## 7. code-mode —— omiga 缺失（可能非目标）

**codex**：`code-mode`（cell_actor + remote_session）——把工具调用作为代码 cell 执行的新范式。
**omiga**：无。这是大范式转变，与 omiga 的 operator/unit 声明式模型定位不同，**建议列为非目标或远期**。

---

## 完善路线优先级汇总

| # | 项 | 维度 | 优先级 | 工作量 | 价值 |
|---|---|---|---|---|---|
| **K1** | agent 状态六基座去重 → 统一持久化 agent/thread store | 编排 | **P1** | 大（先盘点后分阶段） | 最大架构债，跨会话持久化 |
| **K2** | 权限策略引擎（execpolicy 式 DSL + amend），并 N8 审批合流 | 权限 | P2 | 中 | 对齐 codex 最深的差距 |
| **K3** | MCP server surface（暴露 operator/工具经 MCP） | MCP | P2 | 中 | 互操作性，omiga 可作子 agent |
| **K4** | 进程加固（no_new_privs/rlimit/降权） | 安全 | P3 | 小 | 小而实的安全兜底 |
| **K5** | skill 用量感知排序（借 mention_counts） | skill | P3 | 小 | 边际优化 |
| K6 | code-mode | 范式 | 非目标/远期 | 大 | 与声明式定位冲突 |

**建议起点**：K4（进程加固，小而独立、纯安全收益，可先做验证流程）或 K1 的盘点阶段（agent 状态去重，最大债但需先出文档定边界）。
K2 承接已有的 N8/权限工作，衔接自然。
