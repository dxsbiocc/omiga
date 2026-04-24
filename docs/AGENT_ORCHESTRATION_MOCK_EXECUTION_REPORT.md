# Agent 编排 Mock 场景验收执行 v1

> 日期：2026-04-22  
> 范围：基于 `run_mock_orchestration_scenario` 的 A/B/C 三个 mock 场景执行结果  
> 目的：验证在**不依赖真实 LLM provider** 的前提下，当前 Agent 编排的状态面板、事件流、phase history 和 trace 机制是否成立。

---

## 1. 本次实际执行的验证

### 后端 Mock Harness 场景测试

已执行：

- `cargo test --manifest-path src-tauri/Cargo.toml seeds_mock_schedule_scenario --quiet`
- `cargo test --manifest-path src-tauri/Cargo.toml seeds_mock_team_scenario --quiet`
- `cargo test --manifest-path src-tauri/Cargo.toml seeds_mock_autopilot_scenario --quiet`

结果：

- **全部通过**

覆盖含义：

- A：`schedule` mock 数据可注入
- B：`team` mock 数据可注入
- C：`autopilot` mock 数据可注入

注入内容包括：

- orchestration events
- background tasks
- reviewer verdict
- team / autopilot state

### 前端入口解析测试

已执行：

- `npm test -- src/utils/workflowCommands.test.ts`

结果：

- **4 tests 全部通过**

覆盖含义：

- `/schedule`
- `/team`
- `/autopilot`

这三个 workflow command 在主输入框语义层成立。

---

## 2. 场景 A：Mock `/schedule`

### 已验证通过

- [x] 可注入 `schedule_plan_created`
- [x] 可注入 worker 完成事件
- [x] 可注入 reviewer verdict 事件
- [x] 至少存在 2 个 background tasks
- [x] 前端已具备 Timeline / Trace Panel 对 `schedule_plan_created` 的映射逻辑
- [x] 前端已具备从 schedule 事件跳转到调度计划 tab 的逻辑

### 结论

- **通过（结构性 / mock 层）**

### 当前仍未验证

- [ ] 真实 UI 手动截图确认
- [ ] 真实 provider 环境下 planner 输出是否合理
- [ ] 真实 worker / reviewer 执行链路是否稳定

---

## 3. 场景 B：Mock `/team`

### 已验证通过

- [x] Team mock 场景可注入 Team state
- [x] 可注入 `verification_started`
- [x] 可注入 `fix_started`
- [x] 可注入 `synthesizing_started`
- [x] 至少存在 executor / verification background tasks
- [x] Team phase 闭环在运行时结构上成立：
  - planning
  - executing
  - verifying
  - fixing
  - synthesizing
- [x] 前端具备 team 相关 trace 消费逻辑

### 结论

- **通过（结构性 / mock 层）**

### 当前仍未验证

- [ ] 真实 UI 手动截图确认
- [ ] 真实 provider 环境下 verify → fix → re-verify → synthesize 的完整运行证据

---

## 4. 场景 C：Mock `/autopilot`

### 已验证通过

- [x] Autopilot mock 场景可注入 Autopilot state
- [x] phase 覆盖至：
  - intake
  - design
  - plan
  - implementation
  - qa
  - validation
- [x] reviewer verdict 可注入
- [x] QA cycle 数据存在
- [x] 前端 phase history 可消费 autopilot phase
- [x] Trace Panel 可消费 autopilot 相关事件

### 结论

- **通过（结构性 / mock 层）**

### 当前仍未验证

- [ ] 真实 UI 手动截图确认
- [ ] 真实 provider 环境下 implementation → qa → validation → complete/stop 的运行证据

---

## 5. 总结矩阵

| 场景 | Mock 注入 | 事件流 | 状态面板 | Trace 消费 | 真实 provider 运行 |
|---|---|---:|---:|---:|---:|
| A `/schedule` | 通过 | 通过 | 通过（结构） | 通过（结构） | 未验证 |
| B `/team` | 通过 | 通过 | 通过（结构） | 通过（结构） | 未验证 |
| C `/autopilot` | 通过 | 通过 | 通过（结构） | 通过（结构） | 未验证 |

### 当前总评

- **Mock 场景验收：通过**
- **真实运行态验收：尚未开始**

---

## 6. 这次 Mock 验收到底证明了什么

这次执行已经可以证明：

1. 主聊天工作流的三种命令入口存在且受测试保护
2. 当前任务区对 orchestration runtime 数据的消费链路成立
3. Dashboard / Phase History / Timeline / Trace Panel / Transcript 之间的联动机制成立
4. 后端 orchestration event log 足以支撑当前前端可观测面

也就是说：

> **Agent 编排的“结构闭环”已成立。**

---

## 7. 当前仍然缺什么

当前唯一没有被真正验证通过的是：

> **真实 provider 环境下的端到端执行闭环**

也就是还缺：

- 实际 LLM planner
- 实际 worker / reviewer 执行
- 实际 summary / synthesis
- 实际失败与重试路径

---

## 8. 建议的下一步

下一步不建议继续扩展 UI，而是进入：

### 真实运行态验收准备

二选一：

1. 配置一个真实 provider  
2. 提供一个本地兼容 mock LLM endpoint

然后正式执行：

- `/schedule`
- `/team`
- `/autopilot`

的端到端验收。

---

## 9. 删除不必要代码的建议

当前 mock harness 仍有价值，因为它已经成为：

- 本地可重复验收辅助层
- UI/trace 回归测试基础

### 验收完全通过后，优先考虑删除/收敛：

1. 面向普通用户暴露的 mock UI 入口  
   - `MockScenarioLauncher`
   - Settings 中的 mock 场景入口

2. 如需保留 mock 能力，建议迁移到：
   - 开发者调试模式
   - 内部 devtools
   - 测试专用入口

### 当前不建议删除：

- mock seed helper
- orchestration event log
- trace panel

因为这些已经服务于真实收敛与回归，不属于多余代码。

