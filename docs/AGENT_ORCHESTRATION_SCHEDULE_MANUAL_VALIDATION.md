# `/schedule` 真实完整执行链手动验收指引 v1

> 目标：在**当前真实 DeepSeek provider 已可用**的前提下，手动跑通一条 `/schedule` 完整执行链，收集第一份“从命令到完成”的真实运行证据。  
> 范围：只聚焦 `/schedule`，不在本轮同时验证 `/team` 和 `/autopilot`，避免分散注意力。

---

## 1. 为什么需要这份指引

当前已经确认：

- 真实 provider 冒烟通过
- `/schedule` 真实计划生成通过
- `/team` 真实计划生成通过
- `/autopilot` 真实计划生成通过

当前尚未完成的关键缺口是：

> **真实完整执行链证据**

也就是还缺一份“从用户输入 `/schedule ...` 到 worker 执行、reviewer 结论、最终 assistant summary 全部完成”的运行记录。

---

## 2. 本轮验收目标

本轮只验证一条主链路：

### `/schedule`

需要证明以下 5 件事同时成立：

1. 输入框命令入口可用
2. scheduler 生成真实多步骤计划
3. background workers 真正启动并结束
4. reviewer verdict 真正出现
5. 最终 assistant summary 真正落到会话里

---

## 3. 验收前检查

在开始之前，先确认以下条件：

- [ ] 当前 Omiga 已打开并能正常聊天
- [ ] DeepSeek 为当前 active/default provider
- [ ] 当前 session 已选择工作目录
- [ ] 任务区可见
- [ ] TaskStatus 中 Dashboard / Timeline / Trace Panel 正常显示
- [ ] “正在生成下一步建议…”不会无限卡住（本轮已修）

如果这些前提不满足，先不要开始 `/schedule` 验收。

---

## 4. 推荐验收输入

建议直接使用这一条：

```text
/schedule 把登录流程重构为 token refresh + error boundary，并补充验证；先规划、再执行、再验证，尽量给出清晰的分步结果
```

为什么选这个输入：

- 足够复杂，应该触发多步骤计划
- 同时包含：
  - 实现任务
  - 验证任务
- 适合观察 scheduler / worker / reviewer / summary 是否都出现

---

## 5. 操作步骤

### Step 1：记录初始状态

发送前，记录：

- 当前 session 名称
- 当前 provider / model
- 任务区是否为空
- 当前 Trace Panel 是否为空或仅有历史事件

建议截图：

- 输入框
- 任务区顶部

---

### Step 2：发送 `/schedule`

把推荐输入发出去。

期望立即观察到：

- 当前消息保留 `/schedule ...` 原始文本
- 任务区 / 聊天流里出现调度计划卡片
- Timeline / Trace 中出现：
  - `schedule_plan_created`

若这一步失败，则本轮验收直接判定为失败。

---

### Step 3：观察 worker 启动

期望在数秒内观察到：

- 任务区右下角出现：
  - `N 个后台任务运行中`
- hover 能看到具体任务列表
- Timeline 出现：
  - `worker_started`
- Trace Panel 中能筛选到：
  - `worker_started`

建议截图：

- Dashboard
- 右下角后台任务提示
- Timeline / Trace Panel

---

### Step 4：观察 reviewer 结果

期望在 worker 执行后观察到：

- reviewer verdict 出现在：
  - Scheduler plan
  - Dashboard blocker / reviewer 区
  - Timeline
  - Trace Panel

至少应出现一种：

- PASS
- PARTIAL
- FAIL / REJECT

并能够点击打开对应 transcript。

建议截图：

- reviewer 详细结论
- reviewer transcript drawer

---

### Step 5：观察最终 summary

期望在最后观察到：

- assistant 有最终汇总消息
- 当前 round 进入 completed / terminal 状态
- 任务区不再停留在“永远运行中”
- Timeline 中已形成完整链：
  - `schedule_plan_created`
  - `worker_*`
  - `reviewer_verdict`
  - （如有）post-turn 元信息

建议截图：

- 最终 assistant 回复
- Timeline 最终状态

---

## 6. 通过标准

### 必须全部满足，才算 `/schedule` 完整执行链通过

- [ ] 输入框 `/schedule` 正常触发
- [ ] scheduler 生成多步骤计划
- [ ] 至少 1 个 worker 启动
- [ ] 至少 1 个 worker 完成或失败
- [ ] 至少 1 个 reviewer verdict 出现
- [ ] 能从 worker/reviewer drill-down 到 transcript
- [ ] assistant 最终 summary 出现
- [ ] 当前 round 进入 terminal 状态

只要其中任一项不满足，本轮就应标为：

- **部分通过**
或
- **未通过**

---

## 7. 结果记录模板

请按下面模板记录结果：

### 基本信息

- 日期：
- provider：
- model：
- session：
- 工作目录：

### 输入

```text
/schedule ...
```

### 实际结果

- scheduler 计划：
  - [ ] 有
  - [ ] 无
  - task 数量：

- worker：
  - 启动数量：
  - 完成数量：
  - 失败数量：

- reviewer：
  - verdict：
  - severity：

- transcript drill-down：
  - [ ] worker 可打开
  - [ ] reviewer 可打开

- final summary：
  - [ ] 有
  - [ ] 无

- round terminal：
  - [ ] completed
  - [ ] partial
  - [ ] failed
  - [ ] 仍卡住

### 关键截图

- [ ] 输入后首屏
- [ ] 调度计划
- [ ] worker 运行中
- [ ] reviewer 结果
- [ ] transcript drawer
- [ ] final summary

### 最终判定

- [ ] 通过
- [ ] 部分通过
- [ ] 未通过

### 卡点说明

- ...

---

## 8. 如果失败，优先排查顺序

如果本轮失败，建议按这个顺序排查：

1. 是否真的生成了多步骤计划
2. worker 是否真的启动
3. suggestions/summary 是否卡在 post-turn 阶段
4. transcript 是否有内容但 UI 没显示
5. reviewer 是否有执行但没被前端正确消费

---

## 9. 本轮完成后的下一步

如果 `/schedule` 完整执行链通过，则下一步应进入：

- `/team` 完整执行链验收

如果 `/schedule` 仍未通过，则先不要同时推进 `/team` 和 `/autopilot` 完整执行链，避免问题扩散。

