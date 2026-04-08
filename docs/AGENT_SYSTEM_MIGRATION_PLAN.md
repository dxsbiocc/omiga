# Omiga Agent/Subagent 系统迁移计划

## 1. 现状分析

### Claude Code Agent 系统核心功能
- **6 个内置 Agent**: Explore, Plan, general-purpose, verification, claude-code-guide, statusline-setup
- **Agent 路由系统**: 根据 `subagent_type` 路由到不同 Agent
- **Fork Subagent**: 继承父上下文的轻量级子进程
- **后台异步 Agent**: `run_in_background` 支持
- **Agent 团队**: 多 Agent 协作（KAIROS feature）
- **隔离模式**: worktree/remote 隔离

### Omiga 现状
- 已有基础 Agent Tool 定义（`domain/tools/agent.rs`）
- 已有 Task 系统（`domain/session/agent_task.rs`）
- 缺少内置 Agent 定义系统
- 缺少 Agent 路由逻辑
- 缺少 Fork/后台 Agent 支持

---

## 2. 迁移模块清单

### Phase 1: 核心 Agent 定义系统 ⭐ 高优先级

| 模块 | Claude Code 源文件 | Omiga 目标路径 | 说明 |
|------|-------------------|---------------|------|
| Agent 常量 | `tools/AgentTool/constants.ts` | `domain/agents/constants.rs` | Agent 名称常量 |
| Agent 类型定义 | `tools/AgentTool/loadAgentsDir.ts` (lines 106-165) | `domain/agents/definition.rs` | AgentDefinition trait |
| 内置 Agent 注册 | `tools/AgentTool/builtInAgents.ts` | `domain/agents/builtins/mod.rs` | 注册所有内置 Agent |
| Explore Agent | `tools/AgentTool/built-in/exploreAgent.ts` | `domain/agents/builtins/explore.rs` | 代码探索 Agent |
| Plan Agent | `tools/AgentTool/built-in/planAgent.ts` | `domain/agents/builtins/plan.rs` | 架构设计 Agent |
| General-Purpose | `tools/AgentTool/built-in/generalPurposeAgent.ts` | `domain/agents/builtins/general.rs` | 通用 Agent |

### Phase 2: Agent 路由与执行

| 模块 | 源文件 | 目标路径 | 说明 |
|------|--------|---------|------|
| Agent 路由逻辑 | `tools/AgentTool/AgentTool.tsx` (lines 318-356) | `domain/agents/router.rs` | subagent_type 路由 |
| Agent 执行器 | `tools/AgentTool/runAgent.ts` | `domain/agents/executor.rs` | Agent 执行逻辑 |
| Agent 提示生成 | `tools/AgentTool/prompt.ts` | `domain/agents/prompt.rs` | Agent 系统提示生成 |

### Phase 3: Fork Subagent 系统

| 模块 | 源文件 | 目标路径 | 说明 |
|------|--------|---------|------|
| Fork 子进程 | `tools/AgentTool/forkSubagent.ts` | `domain/agents/fork.rs` | Fork 模式实现 |
| 消息构建 | `tools/AgentTool/forkSubagent.ts` (buildForkedMessages) | `domain/agents/fork.rs` | 继承父消息 |

### Phase 4: 后台异步 Agent

| 模块 | 源文件 | 目标路径 | 说明 |
|------|--------|---------|------|
| 后台任务管理 | `tasks/LocalAgentTask/` | `domain/agents/background.rs` | 异步 Agent 任务 |
| Agent 生命周期 | `tools/AgentTool/agentToolUtils.ts` | `domain/agents/lifecycle.rs` | Agent 状态管理 |

### Phase 5: Agent 团队（可选）

| 模块 | 源文件 | 目标路径 | 说明 |
|------|--------|---------|------|
| 团队成员 | `tools/shared/spawnMultiAgent.ts` | `domain/agents/team.rs` | 多 Agent 团队 |
| SendMessage | `tools/SendMessageTool/` | `domain/tools/send_message.rs` | Agent 间通信 |

### Phase 6: 前端状态管理

| 模块 | 源文件 | 目标路径 | 说明 |
|------|--------|---------|------|
| Agent Store | - | `src/state/agentStore.ts` | 前端 Agent 状态 |
| Agent UI 组件 | - | `src/components/Agent/` | Agent 显示组件 |

---

## 3. 详细设计

### 3.1 Agent 定义 Trait (Rust)

```rust
// domain/agents/definition.rs
pub trait AgentDefinition: Send + Sync {
    fn agent_type(&self) -> &str;
    fn when_to_use(&self) -> &str;
    fn system_prompt(&self, ctx: &ToolContext) -> String;
    fn allowed_tools(&self) -> Option<&[String]>;
    fn disallowed_tools(&self) -> Option<&[String]>;
    fn model(&self) -> Option<&str>; // None = inherit
    fn color(&self) -> Option<&str>;
    fn permission_mode(&self) -> Option<PermissionMode>;
    fn background(&self) -> bool;
    fn omit_claude_md(&self) -> bool;
}
```

### 3.2 内置 Agent 结构

```rust
// domain/agents/builtins/explore.rs
pub struct ExploreAgent;

impl AgentDefinition for ExploreAgent {
    fn agent_type(&self) -> &str { "Explore" }
    
    fn when_to_use(&self) -> &str {
        "Fast agent specialized for exploring codebases..."
    }
    
    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        include_str!("explore_prompt.md")
    }
    
    fn disallowed_tools(&self) -> Option<&[String]> {
        Some(&["Agent", "ExitPlanMode", "FileEdit", "FileWrite", "NotebookEdit"])
    }
    
    fn model(&self) -> Option<&str> { Some("haiku") }
    fn omit_claude_md(&self) -> bool { true }
}
```

### 3.3 Agent 路由器

```rust
// domain/agents/router.rs
pub struct AgentRouter {
    agents: HashMap<String, Box<dyn AgentDefinition>>,
}

impl AgentRouter {
    pub fn new() -> Self {
        let mut router = Self {
            agents: HashMap::new(),
        };
        router.register_builtin_agents();
        router
    }
    
    pub fn select_agent(&self, subagent_type: Option<&str>) -> &dyn AgentDefinition {
        match subagent_type {
            Some(agent_type) => self.agents.get(agent_type)
                .map(|a| a.as_ref())
                .unwrap_or_else(|| self.agents.get("general-purpose").unwrap().as_ref()),
            None => self.agents.get("general-purpose").unwrap().as_ref(),
        }
    }
}
```

### 3.4 前端 Agent Store (TypeScript)

```typescript
// src/state/agentStore.ts
interface AgentState {
  // 当前运行的 Agents
  activeAgents: Map<string, AgentInstance>;
  
  // Agent 定义列表
  agentDefinitions: AgentDefinition[];
  
  // 后台任务
  backgroundJobs: BackgroundAgentJob[];
  
  // Actions
  spawnAgent: (config: AgentSpawnConfig) => Promise<string>;
  killAgent: (agentId: string) => Promise<void>;
  sendMessageToAgent: (agentId: string, message: string) => Promise<void>;
}

interface AgentInstance {
  id: string;
  agentType: string;
  description: string;
  status: 'running' | 'completed' | 'error';
  messages: Message[];
  isBackground: boolean;
  parentSessionId?: string;
}
```

---

## 4. 迁移步骤

### Step 1: 创建基础模块
```bash
# 创建目录结构
mkdir -p omiga/src-tauri/src/domain/agents/builtins
mkdir -p omiga/src/state
mkdir -p omiga/src/components/Agent
```

### Step 2: 迁移 Agent 定义 (Rust)
1. 创建 `domain/agents/definition.rs` - Agent trait
2. 创建 `domain/agents/constants.rs` - 常量
3. 创建 `domain/agents/builtins/mod.rs` - 注册器
4. 迁移 Explore/Plan/General 三个核心 Agent

### Step 3: 集成到 Chat 系统
1. 修改 `commands/chat.rs` - 使用 AgentRouter
2. 在 `run_subagent_session` 中根据 `subagent_type` 选择 Agent
3. 应用 Agent 的工具限制

### Step 4: 前端状态管理
1. 创建 `src/state/agentStore.ts`
2. 添加 Agent 面板组件
3. 显示运行中的 Agents

### Step 5: 高级功能
1. Fork subagent 支持
2. 后台异步 Agent
3. Agent 团队（可选）

---

## 5. 与现有记忆系统对比（Phase 6）

### Claude Code 记忆系统
- **Agent Memory**: 每个 Agent 可以有独立的记忆
- **记忆范围**: user / project / local 三级
- **持久化**: 文件系统存储
- **快照**: 支持记忆快照和恢复

### Omiga 现有记忆系统 (Unified Memory)
- **Explicit Memory**: 用户管理的 Wiki 文档
- **Implicit Memory**: 自动索引的代码文件
- **PageIndex**: 结构化的文档索引
- **Chat Indexer**: 对话历史索引

### 集成方案
暂时保持独立，后续考虑：
1. Agent 可以访问 Unified Memory 的查询接口
2. 考虑为特定 Agent 添加独立的记忆空间
3. 不替换现有记忆系统，而是作为补充

---

## 6. 文件清单

### 需要创建的文件

```
omiga/src-tauri/src/domain/agents/
├── mod.rs              # 模块导出
├── definition.rs       # AgentDefinition trait
├── constants.rs        # 常量定义
├── router.rs           # Agent 路由
├── executor.rs         # Agent 执行器
├── prompt.rs           # 提示生成
├── fork.rs             # Fork subagent
├── background.rs       # 后台任务
├── lifecycle.rs        # 生命周期管理
└── builtins/
    ├── mod.rs          # 内置 Agent 注册
    ├── explore.rs      # Explore Agent
    ├── plan.rs         # Plan Agent
    ├── general.rs      # General-Purpose Agent
    ├── verification.rs # Verification Agent (可选)
    ├── guide.rs        # Claude Code Guide (可选)
    └── statusline.rs   # Statusline Setup (可选)

omiga/src/state/
└── agentStore.ts       # 前端 Agent 状态

omiga/src/components/Agent/
├── index.tsx           # Agent 面板
├── AgentList.tsx       # Agent 列表
└── AgentDetail.tsx     # Agent 详情
```

---

## 7. 预估工作量

| Phase | 模块 | 预估时间 |
|-------|------|---------|
| Phase 1 | 核心 Agent 定义系统 | 1-2 天 |
| Phase 2 | Agent 路由与执行 | 1-2 天 |
| Phase 3 | Fork Subagent | 1 天 |
| Phase 4 | 后台异步 Agent | 1-2 天 |
| Phase 5 | Agent 团队 | 2-3 天 |
| Phase 6 | 前端集成 | 1-2 天 |

**总计**: 7-12 天（1-2 周）

建议先实现 Phase 1-2（核心功能），再根据需要添加高级功能。
