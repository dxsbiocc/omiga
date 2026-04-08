# Agent 调度系统使用指南

## 概述

Agent 调度系统提供了自动 Agent 选择、任务分解和多 Agent 编排功能。让模型能够智能地根据任务类型选择合适的 Agent，并自动管理执行流程。

---

## 核心组件

```
scheduler/
├── mod.rs           # 调度器主体
├── selector.rs      # Agent 选择器
├── planner.rs       # 任务规划器
├── orchestrator.rs  # Agent 编排器
└── strategy.rs      # 调度策略
```

---

## 快速开始

### 1. 自动调度（推荐）

```rust
use crate::domain::agents::scheduler::{auto_schedule, SchedulingRequest};

// 最简单的使用方式
let result = auto_schedule(
    "设计并实现一个用户认证系统",
    "/path/to/project"
).await?;

println!("选中的 Agent: {:?}", result.selected_agents);
println!("执行计划: {:?}", result.plan);
```

### 2. 使用特定策略

```rust
use crate::domain::agents::scheduler::{
    schedule_with_strategy, 
    SchedulingStrategy 
};

let result = schedule_with_strategy(
    "重构代码库",
    "/path/to/project",
    SchedulingStrategy::Phased  // 分阶段执行
).await?;
```

### 3. 完整配置

```rust
use crate::domain::agents::scheduler::{
    AgentScheduler,
    SchedulingRequest,
    SchedulingStrategy,
};

let scheduler = AgentScheduler::new();

let request = SchedulingRequest::new("实现新功能")
    .with_description("添加用户管理模块")
    .with_project_root("/path/to/project")
    .with_strategy(SchedulingStrategy::Phased)
    .with_parallel(true)
    .with_max_agents(5)
    .with_auto_decompose(true);

let result = scheduler.schedule(request).await?;

// 如果需要确认
if result.requires_confirmation {
    println!("{}", result.confirmation_message.unwrap());
    // 等待用户确认...
}

// 执行计划
let execution_result = scheduler.execute_plan(
    &result.plan, 
    &request,
    &app_handle
).await?;
```

---

## Agent 选择器

### 基于关键词的自动选择

```rust
use crate::domain::agents::scheduler::{
    select_agent_for_task,
    select_agents_with_scores
};

// 快速选择
let agent = select_agent_for_task("搜索所有模型文件");
// 返回: "Explore"

// 获取所有候选及分数
let matches = select_agents_with_scores("设计 API 架构");
for m in matches {
    println!("{}: {}分 - {}", m.agent_type, m.score, m.reason);
}
// 输出:
// Plan: 85分 - 任务需要架构设计和规划
// general-purpose: 60分 - 通用任务执行
// ...
```

### 选择规则

| 关键词 | 选择的 Agent |
|--------|-------------|
| find, search, locate | Explore |
| design, architecture, plan | Plan |
| verify, test, check | Verification |
| edit, modify, change | general-purpose |

---

## 任务规划器

### 自动分解任务

```rust
use crate::domain::agents::scheduler::{
    TaskPlanner,
    SchedulingRequest
};

let planner = TaskPlanner::new();
let request = SchedulingRequest::new("搜索代码并设计新功能");

let plan = planner.decompose(&request).await?;

for subtask in &plan.subtasks {
    println!("{}: {} [{}]", 
        subtask.id,
        subtask.description,
        subtask.agent_type
    );
}
```

### 内置分解模式

#### 模式 1: 探索 → 设计 → 实现 → 验证

```
输入: "实现用户认证系统"

分解:
1. [Explore] 探索现有代码结构
2. [Plan] 设计认证架构（依赖: 1）
3. [general-purpose] 实现代码（依赖: 2）
4. [verification] 验证实现（依赖: 3）
```

#### 模式 2: 设计 → 实现 → 验证

```
输入: "设计并实现 API"

分解:
1. [Plan] 设计 API
2. [general-purpose] 实现 API（依赖: 1）
3. [verification] 验证（依赖: 2）
```

#### 模式 3: 并行验证

```
输入: "验证代码库质量"

分解:
1. [Explore] 探索代码库
2. [verification] 验证代码（依赖: 1）
```

---

## 调度策略

### 可用策略

| 策略 | 说明 | 适用场景 |
|------|------|---------|
| `Auto` | 自动选择最佳策略 | 通用 |
| `Single` | 单 Agent 执行 | 简单任务 |
| `Sequential` | 顺序执行 | 有依赖的任务 |
| `Parallel` | 并行执行 | 独立子任务 |
| `Phased` | 分阶段执行 | 复杂功能开发 |
| `Competitive` | 竞争执行 | 需要最佳方案 |
| `VerificationFirst` | 先验证后执行 | 重构优化 |

### 策略选择建议

```rust
use crate::domain::agents::scheduler::strategy::{
    ComplexityEvaluator,
    SchedulingStrategy
};

// 评估复杂度
let complexity = ComplexityEvaluator::evaluate(request);
let description = ComplexityEvaluator::complexity_description(complexity);
let recommended = ComplexityEvaluator::recommend_strategy(complexity);

println!("复杂度: {} ({}/10)", description, complexity);
println!("推荐策略: {:?}", recommended);
```

### 复杂度评分

| 分数 | 复杂度 | 推荐策略 |
|------|--------|---------|
| 0-2 | 简单 | Single |
| 3-4 | 中等 | Sequential |
| 5-6 | 复杂 | Phased |
| 7-8 | 很复杂 | Parallel |
| 9-10 | 非常复杂 | Competitive |

---

## 编排器

### 执行计划

```rust
use crate::domain::agents::scheduler::{
    AgentOrchestrator,
    TaskPlan
};

let orchestrator = AgentOrchestrator::new();

// 执行完整计划
let result = orchestrator.execute(
    &plan,
    &request,
    &app_handle
).await?;

// 查看结果
for (task_id, subtask_result) in &result.subtask_results {
    println!("{}: {:?}", task_id, subtask_result.status);
}
```

### 带确认的执行

```rust
let result = orchestrator.execute_with_confirmation(
    &plan,
    &request,
    &app_handle,
    |summary| {
        println!("{}", summary);
        // 询问用户...
        true // 或 false 取消
    }
).await?;
```

### 执行日志

```rust
for entry in &result.execution_log {
    println!("[{}] {:?}: {}", 
        entry.timestamp,
        entry.level,
        entry.message
    );
}
```

---

## 完整示例

```rust
use crate::domain::agents::scheduler::*;

async fn example(app: tauri::AppHandle) -> Result<(), String> {
    // 1. 创建调度请求
    let request = SchedulingRequest::new("重构认证模块")
        .with_description("优化现有认证代码的性能和可维护性")
        .with_project_root("/workspace/my-project")
        .with_strategy(SchedulingStrategy::VerificationFirst)
        .with_parallel(false)  // 顺序执行更安全
        .with_max_agents(4);

    // 2. 创建调度器
    let scheduler = AgentScheduler::new();

    // 3. 执行调度（分析和分解）
    let schedule_result = scheduler.schedule(request.clone()).await?;

    println!("将使用以下 Agent: {:?}", schedule_result.selected_agents);
    println!("预估时间: {} 分钟", schedule_result.estimated_duration_secs / 60);

    // 4. 如果需要确认，显示详情
    if schedule_result.requires_confirmation {
        if let Some(msg) = schedule_result.confirmation_message {
            println!("{}", msg);
            // 在实际应用中，这里会弹出确认对话框
        }
    }

    // 5. 执行计划
    let execution_result = scheduler.execute_plan(
        &schedule_result.plan,
        &request,
        &app
    ).await?;

    // 6. 处理结果
    match execution_result.status {
        ExecutionStatus::Completed => {
            println!("执行成功!");
            println!("{}", execution_result.final_summary);
        }
        ExecutionStatus::Failed => {
            println!("执行失败!");
            for (id, result) in &execution_result.subtask_results {
                if result.status == ExecutionStatus::Failed {
                    println!("任务 {} 失败: {:?}", id, result.error);
                }
            }
        }
        _ => {}
    }

    Ok(())
}
```

---

## 高级用法

### 自定义任务分解

```rust
use crate::domain::agents::scheduler::planner::{TaskPlan, SubTask};

let mut plan = TaskPlan::new("自定义任务");

plan.add_subtask(
    SubTask::new("step1", "第一步：探索代码")
        .with_agent("Explore")
);

plan.add_subtask(
    SubTask::new("step2", "第二步：设计方案")
        .with_agent("Plan")
        .with_dependencies(vec!["step1".to_string()])
        .critical()  // 关键任务
);

plan.add_subtask(
    SubTask::new("step3", "第三步：并行实现 A")
        .with_agent("general-purpose")
        .with_dependencies(vec!["step2".to_string()])
);

plan.add_subtask(
    SubTask::new("step4", "第四步：并行实现 B")
        .with_agent("general-purpose")
        .with_dependencies(vec!["step2".to_string()])
);

// 获取并行执行组
let groups = plan.get_parallel_groups();
// 组 1: [step1]
// 组 2: [step2]
// 组 3: [step3, step4]  ← 这两个可以并行
```

### 竞争执行模式

```rust
// 启动多个 Agent 解决同一问题，选择最佳结果
let request = SchedulingRequest::new("优化算法性能")
    .with_strategy(SchedulingStrategy::Competitive);

let result = schedule_with_strategy(
    "优化排序算法",
    "/project",
    SchedulingStrategy::Competitive
).await?;

// 系统会启动 3 个不同的 Agent 同时处理
// 然后比较结果选择最佳方案
```

---

## 与 Chat 系统集成

### 在工具调用中使用

```rust
// commands/chat.rs

// 当 Agent 工具被调用时，自动选择 Agent
let router = crate::domain::agents::get_agent_router();
let selector = crate::domain::agents::scheduler::AgentSelector::new();

// 如果用户没有指定 subagent_type，自动选择
let agent_type = args.subagent_type.as_deref()
    .unwrap_or_else(|| {
        selector.select(&args.prompt, &project_root)
    });
```

### 智能任务路由

```rust
// 根据任务复杂度自动决定是否分解
if scheduler.should_decompose(&user_input) {
    // 复杂任务：分解并编排多个 Agent
    let result = scheduler.schedule(
        SchedulingRequest::new(&user_input)
            .with_auto_decompose(true)
    ).await?;
    
    // 执行计划...
} else {
    // 简单任务：直接执行单个 Agent
    let agent = select_agent_for_task(&user_input);
    // 执行单个 Agent...
}
```

---

## 最佳实践

### 1. 策略选择

- **简单查询**（< 100 字）：使用 `Single` 或 `Auto`
- **探索任务**：使用 `Sequential`（探索→分析）
- **功能开发**：使用 `Phased`（探索→设计→实现→验证）
- **性能优化**：使用 `VerificationFirst`
- **不确定时**：使用 `Auto`，让系统决定

### 2. 并行设置

```rust
// 依赖关系明确的任务：顺序执行
.with_parallel(false)

// 独立子任务：并行执行
.with_parallel(true)

// 控制并发数
.with_max_agents(3)  // 最多同时运行 3 个 Agent
```

### 3. 确认机制

```rust
// 复杂任务要求确认
let request = SchedulingRequest::new("大规模重构")
    .with_strategy(SchedulingStrategy::Phased);

let result = scheduler.schedule(request).await?;

if result.requires_confirmation {
    // 显示 confirmation_message 给用户
    // 等待用户确认后再执行
}
```

---

## API 参考

### 核心函数

| 函数 | 说明 |
|------|------|
| `auto_schedule` | 自动调度入口 |
| `schedule_with_strategy` | 指定策略调度 |
| `select_agent_for_task` | 快速选择 Agent |
| `ComplexityEvaluator::evaluate` | 评估复杂度 |

### 核心结构

| 结构 | 说明 |
|------|------|
| `AgentScheduler` | 主调度器 |
| `SchedulingRequest` | 调度请求 |
| `SchedulingResult` | 调度结果 |
| `TaskPlan` | 执行计划 |
| `AgentOrchestrator` | 编排器 |

---

## 故障排除

### 问题：选择的 Agent 不合适

**解决**：使用 `select_agents_with_scores` 查看所有候选，手动覆盖选择。

### 问题：任务分解不合理

**解决**：手动创建 `TaskPlan`，自定义子任务和依赖关系。

### 问题：执行时间过长

**解决**：
1. 启用并行执行 `.with_parallel(true)`
2. 增加 Agent 数量 `.with_max_agents(10)`
3. 使用更快的 Agent 类型（如 Explore 使用 haiku）

---

## 未来扩展

- [ ] 基于 LLM 的智能任务分解
- [ ] 自适应策略选择（根据历史表现）
- [ ] Agent 间直接通信
- [ ] 动态重排执行顺序
- [ ] 学习用户偏好
