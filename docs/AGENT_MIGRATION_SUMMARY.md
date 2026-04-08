# Omiga Agent 系统迁移总结

## 已完成的工作

### 1. 核心模块创建 ✅

```
omiga/src-tauri/src/domain/agents/
├── mod.rs              # 模块导出
├── constants.rs        # Agent 常量（名称、颜色等）
├── definition.rs       # AgentDefinition trait
├── router.rs           # Agent 路由器
├── integration.rs      # 与 Chat 系统集成
└── builtins/
    ├── mod.rs          # 内置 Agent 注册
    ├── explore.rs      # Explore Agent（代码探索）
    ├── plan.rs         # Plan Agent（架构设计）
    └── general.rs      # General-Purpose Agent（通用任务）
```

### 2. 内置 Agent 功能

| Agent | 类型 | 特点 | 用途 |
|-------|------|------|------|
| **Explore** | 只读 | 禁止文件修改、轻量级模型 | 快速代码库探索 |
| **Plan** | 只读 | 禁止文件修改、结构化输出 | 架构设计和实现规划 |
| **general-purpose** | 通用 | 可使用所有工具（除 Agent 递归） | 通用研究任务 |

### 3. Agent 路由系统

```rust
use crate::domain::agents::{AgentRouter, get_agent_router};

// 使用全局路由器
let router = get_agent_router();

// 根据 subagent_type 选择 Agent
let agent = router.select_agent(Some("Explore"));

// 获取 Agent 系统提示词
let prompt = agent.system_prompt(&tool_context);

// 获取工具限制
let disallowed = agent.disallowed_tools();
```

---

## 下一步集成（推荐顺序）

### Step 1: 修改 `run_subagent_session` 集成 Agent 路由

文件: `omiga/src-tauri/src/commands/chat.rs`

```rust
async fn run_subagent_session(
    // ... 现有参数 ...
    args: &crate::domain::tools::agent::AgentArgs,
    // ...
) -> Result<String, String> {
    // 添加 Agent 路由
    let router = crate::domain::agents::get_agent_router();
    let agent_config = crate::domain::agents::prepare_agent_session_config(
        router,
        args.subagent_type.as_deref(),
        &runtime.llm_config.model,
        parent_in_plan,
        runtime.allow_nested_agent,
    );
    
    // 使用 agent_config.system_prompt 作为基础系统提示词
    // 使用 agent_config.model 作为子 Agent 模型
    // 使用 agent_config.disallowed_tools 过滤工具列表
    
    // ... 其余逻辑 ...
}
```

### Step 2: 添加更多内置 Agent（可选）

创建以下文件：
- `domain/agents/builtins/verification.rs` - 代码验证 Agent
- `domain/agents/builtins/guide.rs` - Claude Code 使用指南 Agent
- `domain/agents/builtins/statusline.rs` - 状态栏配置 Agent

### Step 3: 前端 Agent 状态管理

创建 `omiga/src/state/agentStore.ts`：

```typescript
interface AgentState {
  activeAgents: Map<string, AgentInstance>;
  spawnAgent: (config: AgentSpawnConfig) => Promise<string>;
  killAgent: (agentId: string) => Promise<void>;
}

export const useAgentStore = create<AgentState>((set, get) => ({
  // ... 实现 ...
}));
```

### Step 4: 支持后台异步 Agent

需要修改：
1. `run_subagent_session` 支持 `run_in_background: true`
2. 创建后台任务管理系统
3. Agent 完成通知机制

### Step 5: Fork Subagent（高级）

实现轻量级 Fork 模式：
- 继承父会话完整上下文
- 共享 prompt cache
- 异步执行 + 通知机制

---

## 使用示例

### 在 Chat 中使用 Agent

```rust
// 用户发送消息包含 Agent 工具调用
Agent({
    "description": "搜索代码库",
    "prompt": "找到所有使用 User 模型的文件",
    "subagent_type": "Explore"  // 指定使用 Explore Agent
})
```

### 创建自定义 Agent（用户）

用户可以在 `.claude/agents/my-agent.md` 创建：

```markdown
---
name: my-agent
description: 专门用于处理 XXX 任务
model: sonnet
tools: [Read, Edit, Bash]
color: blue
---

你是专门用于 XXX 的 Agent...
```

### 创建自定义 Agent（代码）

```rust
use crate::domain::agents::definition::{AgentDefinition, AgentSource};

pub struct MyCustomAgent;

impl AgentDefinition for MyCustomAgent {
    fn agent_type(&self) -> &str { "my-custom" }
    fn when_to_use(&self) -> &str { "用于特定任务..." }
    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        "你是专门的 Agent...".to_string()
    }
    fn source(&self) -> AgentSource { AgentSource::BuiltIn }
}

// 注册到路由器
router.register(Box::new(MyCustomAgent));
```

---

## 与现有记忆系统对比（供后续参考）

### Claude Code 记忆系统
- **Agent Memory**: 每个 Agent 可以有独立记忆
- **记忆范围**: user / project / local 三级
- **持久化**: 文件系统存储
- **快照**: 支持记忆快照和恢复

### Omiga Unified Memory（现有）
- **Explicit Memory**: 用户管理的 Wiki 文档
- **Implicit Memory**: 自动索引的代码文件
- **PageIndex**: 结构化的文档索引
- **Chat Indexer**: 对话历史索引

### 集成建议
暂时保持独立，后续考虑：
1. Agent 可以查询 Unified Memory（只读）
2. 特定 Agent 可以有独立的记忆空间
3. 不替换现有记忆系统，而是作为补充

---

## 编译检查

```bash
cd omiga/src-tauri
cargo check
```

预期修复：
- 可能需要调整 `definition.rs` 中的类型匹配
- 确保 `ToolContext` 的导入路径正确

---

## 测试建议

1. **单元测试**: 测试 Agent 路由逻辑
2. **集成测试**: 测试完整子 Agent 执行流程
3. **手动测试**: 验证 Explore/Plan Agent 的行为

```rust
#[test]
fn test_agent_router() {
    let router = AgentRouter::new();
    
    // 测试默认 Agent
    let agent = router.select_agent(None);
    assert_eq!(agent.agent_type(), "general-purpose");
    
    // 测试 Explore Agent
    let agent = router.select_agent(Some("Explore"));
    assert_eq!(agent.agent_type(), "Explore");
    assert!(agent.disallowed_tools().is_some());
}
```
