# Claude Code TS 与 Omiga Rust Agent 系统对比

## 架构差异概览

| 特性 | Claude Code (TypeScript) | Omiga (Rust) |
|------|--------------------------|--------------|
| **调度方式** | 用户显式指定 Agent | 自动选择 + 智能调度 |
| **任务分解** | 无自动分解 | 自动分解复杂任务 |
| **多 Agent 编排** | 不支持 | 完整编排系统 |
| **Agent 类型** | 5 个内置 + 自定义 | 4 个内置 + 可扩展 |
| **后台执行** | 支持 | 支持 |
| **工具过滤** | 精细控制 | 多层次过滤 |

---

## TS 版本核心机制

### 1. Agent 定义

```typescript
// src/tools/AgentTool/loadAgentsDir.ts
export type AgentDefinition = {
  agentType: string
  whenToUse: string
  initialPrompt?: string
  systemPrompt?: string
  model?: ModelSpec  // 模型配置
  tools?: string[]   // 允许的工具
  disallowedTools?: string[]  // 禁止的工具
  permissionMode?: PermissionMode
  mcpServers?: MCPServerSpec[]  // MCP 服务器
  memoryScope?: string
  maxTurns?: number
  isolation?: 'worktree' | 'bubble'
  background?: boolean
  omitClaudeMd?: boolean
  color?: string
}
```

### 2. Agent 工具调用

```typescript
// Agent 由用户显式指定
Agent({
  "description": "搜索代码",
  "prompt": "找到所有模型文件",
  "subagent_type": "Explore"  // <-- 显式指定
})
```

### 3. 自定义 Agent 加载

```typescript
// src/tools/AgentTool/loadAgentsDir.ts

// 从 .claude/agents/*.md 自动加载自定义 Agent
export const getAgentDefinitionsWithOverrides = memoize(
  async (cwd: string): Promise<AgentDefinitionsResult> => {
    // 1. 加载 markdown 文件
    const markdownFiles = await loadMarkdownFilesForSubdir('agents', cwd)
    
    // 2. 解析每个 markdown 文件
    const customAgents = markdownFiles
      .map(({ filePath, frontmatter, content, source }) => {
        return parseAgentFromMarkdown(
          filePath,
          baseDir,
          frontmatter,
          content,
          source,  // 'userSettings' | 'projectSettings' | 'policySettings'
        )
      })
      .filter(agent => agent !== null)

    // 3. 加载插件 Agent
    const pluginAgents = await loadPluginAgents()
    
    // 4. 获取内置 Agent
    const builtInAgents = getBuiltInAgents()

    // 5. 合并所有 Agent（后加载的覆盖先加载的）
    const allAgents = [...builtInAgents, ...pluginAgents, ...customAgents]
    const activeAgents = getActiveAgentsFromList(allAgents)

    return { activeAgents, allAgents }
  }
)

// Agent markdown 文件格式示例:
// .claude/agents/my-custom-agent.md
// ---
// name: my-custom-agent
// description: 用于特定任务的自定义 Agent
// model: haiku
// tools: ["Read", "Write", "Bash"]
// ---
// 
// 系统提示词内容...
```

### 4. 工具过滤

```typescript
// src/tools/AgentTool/agentToolUtils.ts
export function filterToolsForAgent({
  tools,
  isBuiltIn,
  isAsync = false,
  permissionMode,
}: {
  tools: Tools
  isBuiltIn: boolean
  isAsync?: boolean
  permissionMode?: PermissionMode
}): Tools {
  return tools.filter(tool => {
    // MCP 工具始终允许
    if (tool.name.startsWith('mcp__')) {
      return true
    }
    
    // 全局禁止的工具
    if (ALL_AGENT_DISALLOWED_TOOLS.has(tool.name)) {
      return false
    }
    
    // 自定义 Agent 额外限制
    if (!isBuiltIn && CUSTOM_AGENT_DISALLOWED_TOOLS.has(tool.name)) {
      return false
    }
    
    // 异步 Agent 限制
    if (isAsync && !ASYNC_AGENT_ALLOWED_TOOLS.has(tool.name)) {
      return false
    }
    
    return true
  })
}
```

### 5. Agent 执行

```typescript
// src/tools/AgentTool/runAgent.ts
export async function* runAgent({
  agentDefinition,     // Agent 定义
  promptMessages,      // 提示消息
  toolUseContext,      // 工具上下文
  availableTools,      // 可用工具
  model,               // 模型覆盖
  maxTurns,            // 最大轮数
  ...
}): AsyncGenerator<Message, void> {
  // 1. 解析模型
  const resolvedAgentModel = getAgentModel(
    agentDefinition.model,
    toolUseContext.options.mainLoopModel,
    model,
    permissionMode,
  )
  
  // 2. 初始化 MCP 服务器
  const { clients, tools, cleanup } = await initializeAgentMcpServers(...)
  
  // 3. 构建系统提示词
  const systemPrompt = buildSystemPrompt(...)
  
  // 4. 运行 query 循环
  for await (const message of query(...)) {
    yield message
  }
}
```

### 6. 后台 Agent 生命周期

```typescript
// src/tools/AgentTool/agentToolUtils.ts
export async function runAsyncAgentLifecycle({
  taskId,
  makeStream,
  metadata,
  ...
}: {
  taskId: string
  makeStream: () => AsyncGenerator<Message, void>
  ...
}): Promise<void> {
  try {
    // 1. 启动 Agent 流
    for await (const message of makeStream()) {
      // 更新进度
      updateAsyncAgentProgress(taskId, progress, setAppState)
    }
    
    // 2. 标记完成
    completeAsyncAgent(agentResult, setAppState)
    
    // 3. 发送通知
    enqueueAgentNotification({
      taskId,
      status: 'completed',
      ...
    })
  } catch (error) {
    // 处理错误
    failAsyncAgent(taskId, error, setAppState)
  }
}
```

### 7. 内置 Agent 注册

```typescript
// src/tools/AgentTool/builtInAgents.ts
export function getBuiltInAgents(): AgentDefinition[] {
  const agents: AgentDefinition[] = [
    GENERAL_PURPOSE_AGENT,
    STATUSLINE_SETUP_AGENT,
  ]

  if (areExplorePlanAgentsEnabled()) {
    agents.push(EXPLORE_AGENT, PLAN_AGENT)
  }

  if (isNonSdkEntrypoint) {
    agents.push(CLAUDE_CODE_GUIDE_AGENT)
  }

  if (feature('VERIFICATION_AGENT')) {
    agents.push(VERIFICATION_AGENT)
  }

  return agents
}
```

---

## Rust 版本核心机制

### 1. Agent 定义 (Trait)

```rust
// domain/agents/definition.rs
pub trait AgentDefinition: Send + Sync {
    fn agent_type(&self) -> &str;
    fn when_to_use(&self) -> &str;
    fn system_prompt(&self, ctx: &ToolContext) -> String;
    fn allowed_tools(&self) -> Option<Vec<String>>;
    fn disallowed_tools(&self) -> Option<Vec<String>>;
    fn model(&self) -> Option<&str>;
    fn background(&self) -> bool;
    fn omit_claude_md(&self) -> bool;
}
```

### 2. 智能 Agent 选择

```rust
// domain/agents/scheduler/selector.rs
pub fn select_agent_for_task(task_description: &str) -> String {
    let selector = AgentSelector::new();
    selector.select(task_description, ".")
}

// 使用示例
let agent = select_agent_for_task("搜索所有模型文件");
// 返回: "Explore"
```

### 3. 自动任务调度

```rust
// domain/agents/scheduler/mod.rs
pub async fn auto_schedule(
    user_request: impl Into<String>,
    project_root: impl Into<String>,
) -> Result<SchedulingResult, String> {
    let scheduler = AgentScheduler::new();
    let request = SchedulingRequest::new(user_request)
        .with_project_root(project_root);
    
    scheduler.schedule(request).await
}

// 使用示例
let result = auto_schedule("实现用户认证系统", "/project").await?;
// 自动分解为: Explore → Plan → General-Purpose → Verification
```

### 4. 任务分解与编排

```rust
// domain/agents/scheduler/planner.rs
pub fn rule_based_decomposition(&self, request: &str) -> Vec<SubTask> {
    // 模式 1: 探索 → 设计 → 实现 → 验证
    if self.has_pattern(&lower, &["find", "search"], &["design", "implement"]) {
        vec![
            SubTask::new("explore", "...").with_agent("Explore"),
            SubTask::new("design", "...").with_agent("Plan").with_dependencies(vec!["explore"]),
            SubTask::new("implement", "...").with_agent("general-purpose").with_dependencies(vec!["design"]),
            SubTask::new("verify", "...").with_agent("verification").with_dependencies(vec!["implement"]),
        ]
    }
    // ...
}
```

### 5. 调度策略

```rust
// domain/agents/scheduler/strategy.rs
pub enum SchedulingStrategy {
    Auto,              // 自动选择
    Single,            // 单 Agent
    Sequential,        // 顺序执行
    Parallel,          // 并行执行
    Phased,            // 分阶段
    Competitive,       // 竞争执行
    VerificationFirst, // 验证优先
}
```

---

## 关键差异对比

### Agent 选择

| | TS 版本 | Rust 版本 |
|--|---------|-----------|
| **方式** | 用户显式指定 | 自动智能选择 |
| **调用** | `subagent_type: "Explore"` | 自动推断 |
| **灵活性** | 低 | 高 |
| **示例** | 用户必须知道用 Explore | 系统根据"搜索"关键词自动选 Explore |

### 自定义 Agent 加载

| | TS 版本 | Rust 版本 |
|--|---------|-----------|
| **方式** | ✅ 从 `.md` 文件自动加载 | ❌ 当前不支持 |
| **格式** | YAML frontmatter + Markdown | 代码中定义 |
| **位置** | `~/.claude/agents/`, `.claude/agents/` | `domain/agents/builtins/` |
| **热加载** | ✅ 支持 | ❌ 编译时确定 |
| **示例** | 创建 `my-agent.md` 自动识别 | 需修改代码并重新编译 |

### 任务分解

| | TS 版本 | Rust 版本 |
|--|---------|-----------|
| **支持** | ❌ 不支持 | ✅ 自动分解 |
| **粒度** | 单任务 | 多子任务 |
| **依赖** | 无 | 有向无环图 |
| **示例** | 一个 Agent 完成所有 | 分解为探索→设计→实现→验证 |

### 多 Agent 编排

| | TS 版本 | Rust 版本 |
|--|---------|-----------|
| **并行** | ❌ 不支持 | ✅ 支持 |
| **依赖管理** | ❌ 无 | ✅ DAG 调度 |
| **结果汇总** | 单结果 | 多结果合并 |
| **执行顺序** | 串行 | 自动优化 |

### 工具过滤

| | TS 版本 | Rust 版本 |
|--|---------|-----------|
| **层级** | 单层 | 多层 |
| **配置** | Agent 定义中 | Agent + 全局 + 动态 |
| **灵活性** | 静态 | 动态组合 |

---

## TS 版本调度流程

```
用户调用 Agent Tool
       ↓
解析参数 (description, prompt, subagent_type)
       ↓
加载 Agent 定义 (通过 subagent_type 查找)
       ↓
过滤可用工具
       ↓
构建系统提示词
       ↓
执行 Agent (runAgent)
       ↓
返回结果给父 Agent
```

---

## Rust 版本调度流程

```
用户请求
       ↓
[调度器] 分析复杂度
       ↓
[规划器] 决定是否需要分解
       ↓
是 → 分解为子任务
       ↓
[选择器] 为每个子任务选择最佳 Agent
       ↓
[编排器] 构建执行计划 (DAG)
       ↓
[执行器] 并行/顺序执行
       ↓
汇总结果
```

---

## TS 版本的优势

1. **简单直接** - 用户完全控制，可预测
2. **精细控制** - 每个 Agent 调用都可精确配置
3. **自定义 Agent** - 从 `.claude/agents/*.md` 自动加载
4. **成熟稳定** - 经过大量实际使用验证

## Rust 版本的优势

1. **智能自动** - 减少用户决策负担
2. **高效执行** - 自动并行化独立任务
3. **复杂任务** - 能处理需要多步骤的复杂请求
4. **可扩展** - 调度策略可配置

---

## 迁移建议

### 场景 1: 保持 TS 行为

如果希望保持与 TS 版本相同的行为（用户显式指定 Agent）：

```rust
// 在 run_subagent_session 中
let agent_type = args.subagent_type.as_deref()
    .unwrap_or("general-purpose");  // 直接使用用户指定的
```

### 场景 2: 混合模式（推荐）

用户可指定，也可让系统自动选择：

```rust
let agent_type = if let Some(user_choice) = args.subagent_type.as_deref() {
    user_choice.to_string()  // 用户指定优先
} else {
    select_agent_for_task(&args.prompt)  // 自动选择
};
```

### 场景 3: 完整自动调度

对于复杂任务，使用完整调度系统：

```rust
// 添加 /auto 命令
if user_input.starts_with("/auto ") {
    let request = SchedulingRequest::new(&user_input[6..]);
    let result = scheduler.schedule(request).await?;
    
    // 显示计划，等待确认
    if result.requires_confirmation {
        show_confirmation_dialog(&result.confirmation_message.unwrap());
    }
    
    // 执行
    scheduler.execute_plan(&result.plan, &request, &app).await?;
}
```

---

## 如何实现 TS 版本的自定义 Agent 功能

TS 版本的自定义 Agent 加载是一个强大功能，允许用户在不修改代码的情况下创建新 Agent。要在 Rust 版本中实现类似功能：

### 方案 1: 运行时解析（推荐）

```rust
// domain/agents/loader.rs
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct AgentFrontmatter {
    name: String,
    description: String,
    model: Option<String>,
    tools: Option<Vec<String>>,
    #[serde(default)]
    background: bool,
}

pub async fn load_agents_from_dir(dir: &Path) -> Vec<Box<dyn AgentDefinition>> {
    let mut agents: Vec<Box<dyn AgentDefinition>> = Vec::new();
    
    if let Ok(entries) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    // 解析 frontmatter
                    if let Some((frontmatter_str, body)) = content.split_once("---\n").map(|(_, rest)| {
                        rest.split_once("---\n").unwrap_or(("", rest))
                    }) {
                        if let Ok(frontmatter) = serde_yaml::from_str::<AgentFrontmatter>(frontmatter_str) {
                            // 创建动态 Agent
                            agents.push(Box::new(DynamicAgent {
                                agent_type: frontmatter.name,
                                when_to_use: frontmatter.description,
                                system_prompt_text: body.trim().to_string(),
                                model: frontmatter.model,
                                // ...
                            }));
                        }
                    }
                }
            }
        }
    }
    
    agents
}

// 动态 Agent 结构
pub struct DynamicAgent {
    agent_type: String,
    when_to_use: String,
    system_prompt_text: String,
    model: Option<String>,
    // ...
}

impl AgentDefinition for DynamicAgent {
    fn agent_type(&self) -> &str { &self.agent_type }
    fn when_to_use(&self) -> &str { &self.when_to_use }
    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        self.system_prompt_text.clone()
    }
    // ...
}
```

### 方案 2: WASM 插件系统

更高级的方案是使用 WASM 插件，允许用任何语言编写 Agent：

```rust
// 加载 WASM 插件
pub async fn load_wasm_agents(dir: &Path) -> Vec<Box<dyn AgentDefinition>> {
    // 使用 wasmtime 或 wasmer 加载 WASM 插件
    // ...
}
```

### 方案 3: 配置热重载

结合文件监控实现热重载：

```rust
use notify::{Watcher, RecursiveMode};

pub async fn watch_agent_dir(dir: &Path) {
    let (tx, rx) = std::sync::mpsc::channel();
    
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    }).unwrap();
    
    watcher.watch(dir, RecursiveMode::NonRecursive).unwrap();
    
    // 当文件变化时重新加载
    while let Ok(event) = rx.recv() {
        println!("Agent 文件变化: {:?}", event);
        // 重新加载 Agent 定义
    }
}
```

---

## 代码量对比

| 模块 | TS 版本 | Rust 版本 |
|------|---------|-----------|
| Agent 定义 | ~100 行 | ~185 行 |
| 工具过滤 | ~150 行 | ~200 行 |
| Agent 执行 | ~500 行 | ~400 行 |
| 后台任务 | ~200 行 | ~345 行 |
| **调度系统** | ❌ 无 | **~1500 行** |
| **总计** | ~950 行 | ~2630 行 |

---

## 总结

| 维度 | 评价 |
|------|------|
| **TS 版本** | 简单直接，适合明确知道自己需要什么的用户 |
| **Rust 版本** | 智能自动，适合处理复杂任务和新手用户 |
| **建议** | 在 Omiga 中支持两种模式，让用户选择 |
