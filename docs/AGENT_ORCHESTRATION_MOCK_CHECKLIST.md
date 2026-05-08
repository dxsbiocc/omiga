# Agent 编排 Mock 场景验收清单 v1

> 目标：在**不依赖真实 LLM provider** 的前提下，验证当前 Agent 编排的主工作区可见性、控制动作、事件流、phase history、trace panel 是否工作正常。  
> 说明：这是 **Mock 验收层**，不是最终真实运行态验收。通过后，应继续执行真实 provider 环境下的端到端验收。

---

## 1. 使用入口

位置：

- `Settings -> Agent 编排 -> Mock 场景验收辅助`

可注入场景：

- `Mock /schedule`
- `Mock /team`
- `Mock /autopilot`

注入后，任务区应自动刷新当前会话的：

- Orchestration Dashboard
- Phase History / Runtime Trace
- 编排时间线
- Trace Panel
- Transcript drill-down

---

## 2. 场景 A：Mock `/schedule`

### 预期注入结果

- 生成 `schedule_plan_created`
- 至少 1 个 worker 完成事件
- 至少 1 个 reviewer verdict 事件
- 至少 2 个 background tasks

### UI 验收点

- [ ] Dashboard 显示当前存在 worker / reviewer 相关状态
- [ ] 时间线出现 `调度计划已生成`
- [ ] 时间线出现 worker 完成类事件
- [ ] 时间线出现 reviewer verdict 事件
- [ ] Trace Panel 中可筛选到 `schedule_plan_created`
- [ ] 点击 schedule 事件可跳到调度计划 tab
- [ ] 点击 reviewer / worker 事件可打开 transcript

### 当前代码/测试证据

- `seeds_mock_schedule_scenario`
- `workflowCommands.test.ts`

### 当前结论

- **结构性通过**

---

## 3. 场景 B：Mock `/team`

### 预期注入结果

- Team state 存在
- 事件链中至少包含：
  - `mode_requested`
  - `phase_changed -> executing`
  - `verification_started`
  - `fix_started`
  - `synthesizing_started`
- 至少存在 executor / verification background tasks

### UI 验收点

- [ ] Dashboard 显示当前 mode = team（或 team 相关摘要）
- [ ] Phase History 中可看到 team phase 路径
- [ ] 时间线可看到 verifying / fixing / synthesizing 相关事件
- [ ] Trace Panel 可筛选出 `verification_started`
- [ ] Trace Panel 可筛选出 `fix_started`
- [ ] Trace Panel 可筛选出 `synthesizing_started`
- [ ] 点击 worker / reviewer 事件能打开 transcript

### 当前代码/测试证据

- `seeds_mock_team_scenario`
- `team_orchestrator` 相关测试
- `detects_parallel_analysis`

### 当前结论

- **结构性通过**

---

## 4. 场景 C：Mock `/autopilot`

### 预期注入结果

- Autopilot state 存在
- phase 至少覆盖：
  - `intake`
  - `design`
  - `plan`
  - `implementation`
  - `qa`
  - `validation`
- 至少 1 个 reviewer verdict 事件
- QA cycle 状态存在

### UI 验收点

- [ ] Dashboard 显示当前 mode = autopilot
- [ ] Dashboard 显示 QA cycle 信息
- [ ] Phase History 显示 autopilot phase 路径
- [ ] 时间线出现 validation 相关事件
- [ ] Trace Panel 可筛选出 `phase_changed`
- [ ] reviewer 事件可 drill-down 到 transcript

### 当前代码/测试证据

- `seeds_mock_autopilot_scenario`
- `orchestrator_updates_phase_and_qa_limit`
- `validation_lane_includes_reviewer_family`
- `qa_cycles_increment_only_on_entering_qa`
- `qa_limit_reached_after_exceeding_max_cycles`

### 当前结论

- **结构性通过**

---

## 5. 当前 Mock 验收结论

### 当前 Mock 层能证明什么

Mock harness 现在已经能够证明：

1. 前端主工作区可以消费 orchestration runtime 数据
2. Dashboard / Timeline / Phase History / Trace Panel 已形成一条可见性链
3. Transcript drill-down 链路可从：
   - dashboard
   - timeline
   - trace panel
   等入口打开
4. 三条主链路在“结构上”都可以被注入、观测、检查

### 当前 Mock 层不能证明什么

Mock harness **不能证明**：

1. 真正 LLM planner 生成的计划是否合理
2. 真正 worker / reviewer 的自然语言输出是否稳定
3. 真实 provider 下流式执行是否完整闭环
4. `/schedule` / `/team` / `/autopilot` 在真实模型环境下是否全部端到端通过

---

## 6. Mock Harness 生命周期建议

当前 mock 场景注入器是为了帮助**收敛与验收**，不是永久产品功能。

建议：

- 在真实运行态三场景全部通过之前，**保留**
- 在真实端到端验收全部通过后，评估是否：
  - 保留为开发者调试工具
  - 或移出 Settings 主入口
  - 或删除前端入口，仅保留测试 helper

也就是说：

> **验收通过后记得删除不必要代码**  
> 当前建议删除优先级最高的是“面向普通用户暴露的 mock launcher UI”，而不是底层测试能力。

---

## 7. 下一步

完成 Mock 验收后，下一步应进入：

### 真实运行态验收

优先准备：

- 一个可用的 provider 配置
- 或一个本地兼容的 mock LLM endpoint

然后正式跑：

- `/schedule`
- `/team`
- `/autopilot`

并记录：

- 实际 phase
- 实际 orchestration events
- 实际 UI
- 是否通过
- 卡点在哪里

