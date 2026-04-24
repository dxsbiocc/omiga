# Agent 编排真实运行态验收执行 v1（进行中）

> 日期：2026-04-22  
> Provider：DeepSeek  
> Model：`deepseek-reasoner`

---

## 1. 本次已执行内容

### 1.1 真实 provider 普通聊天冒烟

执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test real_runtime_smoke provider_chat_smoke -- --ignored --nocapture
```

结果：

- **通过**

结论：

- Omiga 当前真实配置可被后端读取
- `create_client(...)`
- `send_message_streaming(...)`

链路已在真实 DeepSeek provider 下跑通。

---

### 1.2 真实 `/schedule` harness

执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test real_schedule_harness real_schedule_builds_multi_step_plan -- --ignored --nocapture
```

实际输出：

- provider = `deepseek`
- model = `deepseek-reasoner`
- `tasks=1`
- `agents=["verification"]`
- `strategy=Auto`

结果：

- **失败**

失败原因：

- 期望生成多步骤编排计划
- 实际只返回了 1 个子任务

---

## 2. 当前结论

### 已通过

- 真实 provider 读取成功
- 普通聊天流式调用成功
- 真实 `/schedule` harness 已通过（可生成多步骤 phased plan）
- 真实 `/team` harness 已通过（可生成 Team 模式计划并包含 `team-verify`）
- 真实 `/autopilot` harness 已通过（可生成 phased plan 且包含 reviewer-family augmentation）

### 未通过

- team 的完整 execute → verify → fix → synthesize 真实执行链尚未直接跑通
- autopilot 的完整 implementation → QA → validation → complete/stop 真实执行链尚未直接跑通

这说明：

> 当前真正的主线阻塞已经从“provider 是否可用”，收敛成  
> **“剩余真实运行态场景（尤其 `/autopilot`，以及 team 的完整 execute→verify→fix→synthesize 执行链）尚未被直接跑通。”**

---

## 3. 已完成的真实运行态结果

### 3.1 普通聊天冒烟

- provider = `deepseek`
- model = `deepseek-reasoner`
- **通过**

### 3.2 `/schedule` 真实计划生成

初次运行结果：

- `tasks=1`
- `strategy=Auto`
- **失败**

修复：

- 将 `/schedule` 的强制策略从 `Auto` 收敛为 `Phased`

修复后重新运行结果：

- `tasks=4`
- `agents=["Explore", "Plan", "executor", "verification"]`
- `strategy=Phased`
- **通过**

### 3.3 `/team` 真实计划生成

执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test real_team_harness real_team_builds_team_plan -- --ignored --nocapture
```

实际输出：

- provider = `deepseek`
- model = `deepseek-reasoner`
- `tasks=7`
- `strategy=Team`
- 包含 `team-verify`

结果：

- **通过**

### 3.4 `/autopilot` 真实计划生成

执行：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test real_autopilot_harness real_autopilot_builds_phased_plan -- --ignored --nocapture
```

实际输出：

- provider = `deepseek`
- model = `deepseek-reasoner`
- `tasks=9`
- `strategy=Phased`
- reviewer_agents 包含：
  - `api-reviewer`
  - `code-reviewer`
  - `critic`
  - `quality-reviewer`
  - `security-reviewer`
  - `verification`

结果：

- **通过**

### 3.5 全命令面真实执行 harness 尝试

尝试方向：

- 使用 Tauri `send_message` 命令面做真正的 `/schedule` 命令级 harness
- 使用 Tauri `run_agent_schedule` 命令面做真正的 `/schedule` 完整执行链 harness

当前结果：

- **未落地**

原因：

- 当前 crate 依赖未启用 `tauri` 的 `test` feature，因此 integration test 中无法使用 `tauri::test::*`
- 即便启用后，`generate_handler!` 也需要命令宏在当前 crate 作用域中可见，现有 integration-test 结构不满足
- 尝试改用真实 `tauri::Builder::default()` + `run_agent_schedule` 直接调用时，在 macOS 上触发：
  - `EventLoop must be created on the main thread`

结论：

> 当前“真实计划生成 harness”已通过，  
> 但“真实命令级 harness / 完整执行链 harness”仍缺一个可用的 Tauri command-test 基础设施。

---

## 4. 当前最可能的问题方向

### 方向 A：`/schedule` 的约束不够强

当前真实 harness 是：

- `SchedulingStrategy::Auto`
- `auto_decompose = true`

这意味着 planner 仍然可能选择：

- single
- auto

从而不给出多任务结果。

### 方向 B：DeepSeek 对当前 planner prompt 的响应不稳定

当前 planner prompt 可能对 `deepseek-reasoner` 不够收敛，导致：

- 虽然用户意图是多步骤
- 但 planner 仍然返回 single / trivial output

### 方向 C：`/schedule` 命令的产品语义没有真正“强制多步骤编排”

从产品预期看，用户输入 `/schedule` 通常是在说：

> “请给我一个编排计划”

但当前实际实现更接近：

> “把这个请求送进 scheduler，允许它自行决定是否只做 single”

这两者不是一回事。

---

## 5. 下一步建议

当前最有价值的下一步不是继续扩 UI，而是：

### 优先推进真实执行链验证（而非仅计划生成）

因为当前已经通过：

- 普通聊天冒烟
- `/schedule` 真实计划生成
- `/team` 真实计划生成
- `/autopilot` 真实计划生成

当前最需要补齐的是：

- team 的真实 execute → verify → fix → synthesize 执行链
- autopilot 的真实 implementation → QA → validation → complete/stop 执行链
- 以及更薄的一层：可重复的 Tauri command 级真实验收 harness

在这两条真实执行链通过之前，不要急着宣布真实运行态编排已完成。
