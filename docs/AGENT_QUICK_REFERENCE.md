# Agent 系统快速参考

## 可用的 Agent

| Agent | `subagent_type` | 用途 | 模型 | 禁止工具 |
|-------|-----------------|------|------|---------|
| General-Purpose | (不传) 或 `"general-purpose"` | 通用任务 | inherit | Agent |
| Explore | `"Explore"` | 代码探索 | haiku | Agent, FileEdit, FileWrite, NotebookEdit |
| Plan | `"Plan"` | 架构设计 | inherit | Agent, FileEdit, FileWrite, NotebookEdit |
| Verification | `"verification"` | 代码验证 | inherit | Agent, FileEdit, FileWrite, NotebookEdit, ExitPlanMode |

## 使用示例

### 探索代码库
```json
{
  "description": "搜索代码结构",
  "prompt": "找到所有使用 User 模型的文件",
  "subagent_type": "Explore"
}
```

### 设计架构
```json
{
  "description": "设计认证系统",
  "prompt": "设计一个基于 JWT 的认证系统，包括登录、注册、刷新令牌",
  "subagent_type": "Plan"
}
```

### 验证代码
```json
{
  "description": "验证实现",
  "prompt": "验证 auth.rs 中的 JWT 实现是否正确",
  "subagent_type": "verification"
}
```

### 通用任务（默认）
```json
{
  "description": "研究代码库",
  "prompt": "分析这个项目的架构和主要模块"
}
```

## 模型选择优先级

1. **用户指定** (`args.model`) - 最高优先级
2. **Agent 配置** - 如果模型不是 "inherit"
3. **继承父模型** - 默认行为

```json
// 使用 haiku 模型（覆盖 Explore 默认）
{
  "description": "搜索",
  "prompt": "找到所有测试文件",
  "subagent_type": "Explore",
  "model": "haiku"
}

// 使用 sonnet 模型（覆盖 Plan 默认的 inherit）
{
  "description": "复杂设计",
  "prompt": "设计微服务架构",
  "subagent_type": "Plan",
  "model": "sonnet"
}
```

## 工具过滤流程

```
1. 全局过滤 (SubagentFilterOptions)
   - ALL_AGENT_DISALLOWED_TOOLS
   - allow_nested_agent

2. Agent 级别过滤
   - allowed_tools (白名单，如果指定)
   - disallowed_tools (黑名单)
```

## 特殊功能

### omit_claude_md
Explore 和 Plan Agent 自动省略 CLAUDE.md：
- 更快启动
- 适合只读任务

### 计划模式支持
当父会话在计划模式时：
- `ExitPlanMode` 可用
- Agent 会在提示词中说明

### 嵌套 Agent
默认禁止，防止无限递归：
- 子 Agent 无法调用 Agent 工具
- 除非 `USER_TYPE=ant` 环境变量设置

## 代码集成点

### 添加新 Agent

1. 创建文件 `domain/agents/builtins/my_agent.rs`:
```rust
use crate::domain::agents::definition::{AgentDefinition, AgentSource};
use crate::domain::tools::ToolContext;

pub struct MyAgent;

impl AgentDefinition for MyAgent {
    fn agent_type(&self) -> &str { "my-agent" }
    fn when_to_use(&self) -> &str { "用于..." }
    fn system_prompt(&self, _ctx: &ToolContext) -> String { "...".to_string() }
    fn source(&self) -> AgentSource { AgentSource::BuiltIn }
    fn disallowed_tools(&self) -> Option<Vec<String>> {
        Some(vec!["Agent".to_string(), "FileEdit".to_string()])
    }
    fn model(&self) -> Option<&str> { Some("inherit") }
}
```

2. 在 `builtins/mod.rs` 注册:
```rust
pub mod my_agent;
router.register(Box::new(my_agent::MyAgent));
```

## 调试

### 查看 Agent 选择
```rust
let router = crate::domain::agents::get_agent_router();
let agent = router.select_agent(Some("Explore"));
println!("Selected agent: {}", agent.agent_type());
```

### 查看工具过滤
```rust
println!("Allowed tools: {:?}", agent.allowed_tools());
println!("Disallowed tools: {:?}", agent.disallowed_tools());
```

### 查看系统提示词
```rust
let tool_ctx = ToolContext::new(&project_root);
let prompt = agent.system_prompt(&tool_ctx);
println!("System prompt: {}", prompt);
```

## 常见问题

### Q: Agent 为什么无法修改文件？
A: Explore 和 Plan Agent 禁止文件修改工具。如需修改，使用默认 Agent（不传 `subagent_type`）。

### Q: 如何让 Agent 使用特定模型？
A: 在调用时指定 `model` 参数，或在 Agent 定义中设置。

### Q: Verification Agent 为什么报错？
A: 当前后台 Agent 未完全实现。需要实现异步任务系统后才能使用。

### Q: 如何禁用嵌套 Agent？
A: 嵌套 Agent 默认禁用，除非设置 `USER_TYPE=ant` 环境变量。

## 文件位置

| 文件 | 路径 |
|------|------|
| Agent 定义 | `domain/agents/definition.rs` |
| 路由系统 | `domain/agents/router.rs` |
| Chat 集成 | `domain/agents/integration.rs` |
| 内置 Agent | `domain/agents/builtins/*.rs` |
| 使用位置 | `commands/chat.rs` (`run_subagent_session`) |
