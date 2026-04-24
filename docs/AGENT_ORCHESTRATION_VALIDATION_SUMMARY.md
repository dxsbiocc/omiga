# Agent 编排结构性验收总结 + 运行阻塞清单

> 状态：收敛模式 / 验收导向  
> 日期：2026-04-22  
> 范围：`/schedule`、`/team`、`/autopilot` 三条主链路的结构性验收结果

---

## 1. 目的

这份文档不是继续扩展功能的愿望清单，而是当前 Agent 编排 MVP 的**收敛基线**。

目标是回答三个问题：

1. 当前三条主编排链路哪些已经结构性成立？
2. 哪些仍然没有真实端到端通过？
3. 阻塞“宣布完成”的核心问题到底是功能缺口，还是运行环境缺口？

结论先行：

- **主链路结构大体成立**
- **任务区 dashboard / timeline / phase history / trace panel 已形成可观测面**
- **真实端到端验收仍被 LLM 运行环境缺失阻断**

换句话说，当前问题已不再是“功能没搭起来”，而是“没有可重复的运行态验收环境”。

---

## 2. 当前 MVP 目标

MVP 只围绕三条主链路：

- `/schedule`
- `/team`
- `/autopilot`

判定完成的标准不是继续增加局部 UI，而是：

1. 主输入框能稳定启动编排
2. 当前会话能看到统一状态总览
3. 当前会话能执行关键控制动作
4. 后端存在真实 orchestration event log
5. 三条场景链路能被验证通过

---

## 3. 场景 A：`/schedule`

### 3.1 已通过项

- `/schedule <task>` workflow command 解析已存在并有前端测试保护
- `send_message` 支持 `workflowCommand` / `routingContent`
- scheduler 路径可被强制触发
- `schedule_plan_created` 事件可写入后端 orchestration event log
- TaskStatus Timeline / Trace Panel 已可消费该事件

### 3.2 当前证据

- `src/utils/workflowCommands.ts`
- `src/utils/workflowCommands.test.ts`
- `src-tauri/src/commands/chat/mod.rs`
- `src-tauri/src/domain/persistence/mod.rs`

### 3.3 当前结论

- **结构性通过**
- **真实端到端未完成**

### 3.4 阻塞点

- 当前本机没有任何可用 LLM 配置
- 无法真实触发 planner / worker / reviewer 执行闭环

---

## 4. 场景 B：`/team`

### 4.1 已通过项

- `/team` workflow command 存在
- keyword detector 能识别 team route
- TeamOrchestrator 具备：
  - begin
  - complete
  - fail
  - suggested_strategy
  - current_execution_lane
- Team phase 结构完整：
  - planning
  - executing
  - verifying
  - fixing
  - synthesizing
  - complete / failed
- 事件 taxonomy 已覆盖：
  - `phase_changed`
  - `verification_started`
  - `fix_started`
  - `synthesizing_started`
  - `worker_*`
  - `reviewer_verdict`
  - `cancel_requested`
  - `cancel_completed`

### 4.2 当前证据

- `src-tauri/src/domain/orchestration/team.rs`
- `src-tauri/src/domain/agents/scheduler/orchestrator.rs`
- `src-tauri/src/commands/ralph.rs`
- `cargo test --manifest-path src-tauri/Cargo.toml team_orchestrator --quiet`
- `cargo test --manifest-path src-tauri/Cargo.toml detects_parallel_analysis --quiet`

### 4.3 当前结论

- **结构性通过到较高程度**
- Team 模式已不是概念存在，而是完整闭环结构

### 4.4 阻塞点

- 还没有在真实 LLM 环境中跑出一次：
  - verifying 失败
  - fixing
  - re-verifying
  - synthesizing
  - complete / failed

---

## 5. 场景 C：`/autopilot`

### 5.1 已通过项

- `/autopilot` workflow command 存在
- keyword detector 能识别 autopilot route
- AutopilotOrchestrator 具备：
  - begin
  - set_phase
  - complete
  - fail
  - suggested_strategy
  - current_execution_lane
- phase 体系完整：
  - intake
  - interview
  - expansion
  - design
  - plan
  - implementation
  - qa
  - validation
  - complete
- QA cycle 逻辑存在且有测试保护：
  - 仅在重新进入 QA 时递增
  - 超限停止逻辑存在
- validation lane 已包含 reviewer family
- event taxonomy 已覆盖：
  - `resume_requested`
  - `mode_requested`
  - `phase_changed`
  - `mode_completed`
  - `mode_failed`

### 5.2 当前证据

- `src-tauri/src/domain/orchestration/autopilot.rs`
- `src-tauri/src/domain/autopilot_state.rs`
- `src-tauri/src/commands/chat/mod.rs`
- `src-tauri/src/commands/chat/orchestration.rs`
- `cargo test --manifest-path src-tauri/Cargo.toml orchestrator_updates_phase_and_qa_limit --quiet`
- `cargo test --manifest-path src-tauri/Cargo.toml validation_lane_includes_reviewer_family --quiet`
- `cargo test --manifest-path src-tauri/Cargo.toml qa_cycles_increment_only_on_entering_qa --quiet`
- `cargo test --manifest-path src-tauri/Cargo.toml qa_limit_reached_after_exceeding_max_cycles --quiet`

### 5.3 当前结论

- **结构性通过到较高程度**
- Autopilot 已具备真实模式的核心状态机，不只是 prompt skill

### 5.4 阻塞点

- 当前本机缺少 LLM 运行环境，无法真实跑完：
  - implementation
  - qa
  - validation
  - complete / stop

---

## 6. 当前统一阻塞

这是现在最重要的判断：

### 当前阻塞“宣布 Agent 编排 MVP 完成”的核心原因：

**不是功能不够多，而是缺少真实运行态验收环境。**

### 具体表现

- 未配置任何可用 provider API key
- 未发现 `omiga.yaml`
- 未发现 `src-tauri/.env`
- 未发现 `~/.config/omiga/config.yaml`

因此当前能做的是：

- 结构性验收
- 单测 / 构建验证
- 真实事件写入验证

但不能做的是：

- 真正的 LLM 驱动端到端验收

---

## 7. 当前已完成的系统性成果

尽管还未完成最终运行态验收，当前系统已经具备这些关键能力：

### 7.1 主入口

- 主聊天输入框支持：
  - `/schedule`
  - `/team`
  - `/autopilot`

### 7.2 当前态总览

- TaskStatus 顶部提供：
  - Orchestration Dashboard
  - Phase History / Runtime Trace
  - Timeline
  - Trace Panel

### 7.3 控制动作

- cancel 当前编排
- inspect 当前计划 / 模式详情
- resume 当前模式
- 打开 blocker / worker transcript

### 7.4 真实后端事件流

- `orchestration_events` 已落地
- Timeline / Phase History / Trace Panel 已开始优先消费真实事件

---

## 8. 现在不建议继续优先做的事

为了避免再次偏离主目标，以下内容建议暂时降级：

- reviewer 细节进一步抛光
- 更多 chip / badge / tooltip 优化
- 更多非验收驱动的 panel 视觉增强
- 新增不服务于主场景通过的外围功能

---

## 9. 下一步决策点

现在项目需要做一个明确决策：

### 路线 A：补真实 LLM 验收环境

目标：
- 配置一个可用 provider
- 真正执行三条主场景
- 拿到运行态证据

优点：
- 直接验证真实产品链路

缺点：
- 依赖外部环境

### 路线 B：补 Mock / Fake orchestration harness

目标：
- 通过 fake planner / fake worker / fake reviewer
- 构造可重复本地端到端测试

优点：
- 可重复、稳定
- 不依赖外部模型

缺点：
- 仍不等于真实 LLM 路径

---

## 10. 建议

我建议下一步优先做：

### **路线 B：先补 Mock orchestration harness**

原因：

1. 当前已经证明结构性链路大体成立
2. 最大阻塞是缺乏可重复运行环境
3. 如果先补 mock harness，就能把 MVP 验收真正产品化，而不是每次靠手工和外部环境

然后再在合适时候补：

### **路线 A：真实 provider 运行态验收**

---

## 11. 当前结论（最终）

### 当前 omiga Agent 编排状态：

- **功能结构已基本成型**
- **统一 runtime 可观测性已开始建立**
- **三条主场景已具备结构性通过证据**
- **真正未完成的核心点是：没有可重复的运行态验收环境**

因此接下来最重要的工作，不是继续无边界加功能，而是：

> **把 Agent 编排从“能开发”切换成“能验收”。**

---

## 12. 当前新增 UI / 交互问题待办

以下问题已在近期真实运行与截图检查中确认，需继续收敛：

- [ ] 右侧任务区顶部文案/信息密度仍偏高，需要继续压缩
- [ ] 聊天区执行动态仍偏弱，需要继续补轻量执行回执
- [ ] `/schedule` 当前仍更像“计划展示 + 工具调用”，动态执行主视图需要继续强化
- [ ] 建议按钮应只显示标题，完整文本通过 hover/tooltip 查看
- [ ] Agent / worker 标签仍有过长风险，需要进一步短标签化
- [ ] 任务区与聊天区之间“谁是主编排视图”还要继续统一
- [ ] 验收全部通过后，移除或隐藏普通用户可见的 mock UI 入口
