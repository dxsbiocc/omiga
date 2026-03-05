# Omiga 与 pi-mono 项目对比分析

> pi-mono 是一个成熟的 TypeScript 编码 Agent 项目，具有完整的会话管理、扩展系统、上下文压缩等机制。本文档对比 Omiga 与 pi-mono 的核心功能，识别差距和提升方向。

---

## 1. 架构对比总览

| 维度 | Omiga (Python) | pi-mono (TypeScript) | 差距评估 |
|------|----------------|---------------------|----------|
| **Agent 核心** | 容器外包 (`run_container_agent`) | 进程内 (`AgentSession.prompt()`) | 🔴 大 |
| **会话管理** | 简单 session_id 字典 | 完整 SessionManager + 树状结构 | 🔴 大 |
| **事件系统** | 简单事件总线 (新增) | 完整 AgentEvent 类型 + 订阅机制 | 🟡 中 |
| **扩展机制** | 无 | 完整 Extension 系统 (工具/命令/UI) | 🔴 大 |
| **上下文压缩** | 无 | 自动压缩 + 分支摘要 | 🔴 大 |
| **工具注册** | ToolRegistry (基础) | 完整 Tool 系统 + 扩展工具注册 | 🟡 中 |
| **流式输出** | 无 | 完整流式事件 (message_start/update/end) | 🟡 中 |
| **错误恢复** | 基础重试 | 自动重试 + 错误分类 | 🟡 中 |

---

## 2. Agent 核心架构对比

### 2.1 Omiga 架构

```
┌────────────────────────────────────────┐
│ processing.py: process_group_messages() │
│   ↓                                     │
│ agent.py: run_agent()                   │
│   ↓                                     │
│ container/runner.py                     │
│   ↓                                     │
│ [Docker 容器: omiga-py-agent]           │
│   ↓                                     │
│ ContainerOutput                         │
└────────────────────────────────────────┘
```

**问题**：
- Agent 逻辑完全外包，主进程无法干预
- 缺少 `think()` → `act()` 循环
- 工具调用追踪在容器内

### 2.2 pi-mono AgentSession 架构

```typescript
class AgentSession {
  readonly agent: Agent;
  readonly sessionManager: SessionManager;

  // 核心方法
  async prompt(options: PromptOptions): Promise<void> {
    // 1. 构建系统提示
    const systemPrompt = buildSystemPrompt(...)

    // 2. 触发 before_agent_start 事件
    const event = await this._extensionRunner.emitBeforeAgentStart(...)

    // 3. 调用 LLM
    const stream = this.agent.stream({ messages, systemPrompt, tools })

    // 4. 处理流式事件
    for await (const event of stream) {
      this._handleAgentEvent(event)
    }

    // 5. 检查自动重试/压缩
    await this._checkCompaction(lastMessage)
  }

  // 事件订阅
  subscribe(listener: AgentSessionEventListener): () => void {
    this._eventListeners.push(listener)
    return () => { /* unsubscribe */ }
  }
}
```

**优势**：
- 完整的 Agent 生命周期管理
- 事件驱动架构（`before_agent_start`、`tool_call`、`message_end`）
- 自动重试和上下文压缩
- 扩展点丰富（Extension 系统）

---

## 3. 会话管理对比

### 3.1 Omiga 会话管理

```python
# state.py
_sessions: dict[str, str] = {}  # group_folder -> session_id

# agent.py
session_id = state._sessions.get(group.folder)
container_input = ContainerInput(
    prompt=prompt,
    session_id=session_id,
    ...
)
```

**问题**：
- 只存储 session_id，不管理会话内容
- 会话历史在容器内，主进程无法访问
- 无会话树/分支概念

### 3.2 pi-mono SessionManager

```typescript
// session-manager.ts
interface SessionEntry {
  type: string;
  id: string;
  parentId: string | null;  // 树状结构
  timestamp: string;
}

type SessionEntry =
  | SessionMessageEntry      // LLM 消息
  | ThinkingLevelChangeEntry // 思考级别变更
  | ModelChangeEntry         // 模型切换
  | CompactionEntry          // 压缩摘要
  | BranchSummaryEntry       // 分支摘要
  | CustomEntry              // 自定义数据
  | CustomMessageEntry       // 自定义消息 (进入上下文)
  | LabelEntry               // 用户标签

class SessionManager {
  // 会话树操作
  getTree(sessionId: string): SessionTree;
  navigateToEntry(sessionId: string, entryId: string): void;
  forkFromEntry(sessionId: string, entryId: string): string;

  // 条目操作
  appendMessage(message: AgentMessage): void;
  appendCustomMessage(customType: string, data: T): void;
  getEntries(): SessionEntry[];

  // 压缩管理
  compact(options: CompactOptions): Promise<CompactionResult>;
}
```

**优势**：
- 完整的会话树结构（支持分支/合并）
- 多样化的条目类型（消息、压缩、分支摘要等）
- 会话导航和分支操作
- 自定义条目支持扩展

---

## 4. 事件系统对比

### 4.1 Omiga 事件系统（Phase 1 新增）

```python
# omiga/memory/events.py
class MemoryEventType(str, Enum):
    TOOL_CALL_START = "tool_call_start"
    SOP_GENERATED = "sop_generated"
    ...

class MemoryEventBus:
    def subscribe(event_type, callback): ...
    def publish(event): ...
```

**局限**：
- 仅覆盖记忆相关事件
- 缺少 Agent 生命周期事件
- 无流式事件支持

### 4.2 pi-mono AgentEvent 系统

```typescript
// @mariozechner/pi-agent-core
type AgentEvent =
  | { type: "agent_start" }
  | { type: "agent_end" }
  | { type: "message_start"; message: AgentMessage }
  | { type: "message_update"; delta: string }
  | { type: "message_end"; message: AgentMessage }
  | { type: "tool_call_start"; toolName: string; args: any }
  | { type: "tool_call_update"; delta: string }
  | { type: "tool_call_end"; toolName: string; result: any }
  | { type: "error"; error: Error }

// AgentSession 订阅处理
this._unsubscribeAgent = this.agent.subscribe(this._handleAgentEvent)

private _handleAgentEvent = async (event: AgentEvent) => {
  // 1. 触发扩展事件
  await this._emitExtensionEvent(event)

  // 2. 通知所有监听器
  this._emit(event)

  // 3. 会话持久化
  if (event.type === "message_end") {
    this.sessionManager.appendMessage(event.message)
  }

  // 4. 检查自动重试/压缩
  if (event.type === "agent_end") {
    await this._checkCompaction(lastMessage)
  }
}
```

**优势**：
- 完整的流式事件（start/update/end）
- 事件驱动的状态更新
- 扩展事件钩子
- 自动持久化

---

## 5. 扩展系统对比

### 5.1 Omiga 扩展机制

**现状**：无正式扩展系统

### 5.2 pi-mono Extension 系统

```typescript
// extensions/types.ts
interface Extension {
  name: string;
  version: string;

  // 生命周期钩子
  onActivate?(ctx: ExtensionContext): Promise<void>;
  onDeactivate?(): Promise<void>;

  // 事件钩子
  onEvent?(event: ExtensionEvent): Promise<void>;

  // 工具注册
  getTools?(): RegisteredTool[];

  // 命令注册
  getCommands?(): SlashCommandInfo[];

  // 快捷键注册
  getKeybindings?(): KeybindingsConfig;
}

interface ExtensionContext {
  ui: ExtensionUIContext;       // UI 交互
  runner: ExtensionRunner;      // 扩展运行器
  eventBus: EventBus;           // 事件总线
  fs: {                         // 文件系统
    workspaceDir: string;
    readTextFile(path: string): Promise<string>;
    writeTextFile(path: string, content: string): Promise<void>;
  };
  shell: {                      // Shell 执行
    exec(command: string, opts: ExecOptions): Promise<ExecResult>;
  };
  modelRegistry: ModelRegistry; // 模型注册表
}

// 扩展事件类型
type ExtensionEvent =
  | BeforeAgentStartEvent    // agent_start 前
  | ToolCallEvent            // 工具调用
  | ToolResultEvent          // 工具结果
  | ContextEvent             // 上下文事件
  | InputEvent               // 用户输入
  | UserBashEvent            // Bash 执行
```

**示例扩展**：
```typescript
// Git 状态扩展
const gitStatusExtension: Extension = {
  name: "git-status",
  version: "1.0.0",

  async onActivate(ctx) {
    ctx.ui.setStatus("git-status", "Loading...")
  },

  async onEvent(event) {
    if (event.type === "before_agent_start") {
      // 每次 agent 运行前更新 git 状态
      const status = await ctx.shell.exec("git status --porcelain")
      ctx.ui.setStatus("git-status", status.stdout ? "●" : "✓")
    }
  },

  getTools() {
    return [{
      name: "git_status",
      description: "Get git status",
      async execute() {
        return await ctx.shell.exec("git status")
      }
    }]
  }
}
```

---

## 6. 上下文压缩对比

### 6.1 Omiga 上下文管理

**现状**：无上下文压缩机制

### 6.2 pi-mono Compaction 系统

```typescript
// compaction/compaction.ts
interface CompactionResult {
  summary: string;           // 压缩摘要
  firstKeptEntryId: string;  // 保留的第一个条目 ID
  tokensBefore: number;      // 压缩前 token 数
  fileOperations?: {         // 文件操作追踪
    readFiles: string[];
    modifiedFiles: string[];
  };
}

// 自动压缩触发
async _checkCompaction(msg: AssistantMessage): Promise<void> {
  const threshold = this.settingsManager.get("context.compactionThreshold")
  const currentTokens = calculateContextTokens(this.sessionManager.getEntries())

  if (currentTokens > threshold) {
    this._emit({
      type: "auto_compaction_start",
      reason: "threshold"
    })

    const result = await compact(
      this.sessionManager.getEntries(),
      this.agent.model,
      { maxTokens: threshold * 0.5 }
    )

    this.sessionManager.saveCompaction(result)
  }
}

// 压缩算法
async function compact(
  entries: SessionEntry[],
  model: Model<any>,
  options: CompactOptions
): Promise<CompactionResult> {
  // 1. 序列化对话
  const serialized = serializeConversation(entries)

  // 2. 调用 LLM 生成摘要
  const response = await completeSimple({
    model,
    prompt: `Summarize this conversation:\n${serialized}`,
    maxTokens: options.maxTokens
  })

  // 3. 提取文件操作
  const fileOps = extractFileOperations(entries)

  return {
    summary: response.content,
    firstKeptEntryId: entries[thresholdIndex].id,
    tokensBefore: currentTokens,
    details: {
      readFiles: Array.from(fileOps.read),
      modifiedFiles: Array.from(fileOps.edited)
    }
  }
}
```

**优势**：
- 自动触发压缩（阈值/溢出）
- 保留关键文件操作历史
- 支持分支摘要

---

## 7. 工具系统对比

### 7.1 Omiga 工具系统

```python
# tools/base.py
class Tool(ABC):
    name: str
    description: str

    @abstractmethod
    async def execute(self, **kwargs) -> ToolResult:
        pass

# tools/registry.py
class ToolRegistry:
    def __init__(self):
        self._tools: dict[str, Tool] = {}

    def register(self, tool: Tool): ...
    async def execute_tool(self, name: str, **kwargs) -> ToolResult: ...
```

### 7.2 pi-mono 工具系统

```typescript
// core/tools/index.ts
type AgentTool = {
  name: string;
  description: string;
  parameters: JsonSchema;
  execute: (args: any) => Promise<AgentToolResult>;
  onUpdate?: AgentToolUpdateCallback;  // 流式更新
}

// 内置工具创建
function createAllTools(ctx: ToolContext): Record<string, AgentTool> {
  return {
    read: createReadTool(ctx),
    write: createWriteTool(ctx),
    edit: createEditTool(ctx),
    bash: createBashTool(ctx),
    grep: createGrepTool(ctx),
    find: createFindTool(ctx),
    ls: createLsTool(ctx),
  }
}

// 扩展工具注册
interface Extension {
  getTools?(): RegisteredTool[];
}

interface RegisteredTool {
  tool: AgentTool;
  display?: {
    hidden?: boolean;        // 隐藏于 LLM
    readOnly?: boolean;      // 只读标记
  };
}
```

---

## 8. 错误处理对比

### 8.1 Omiga 错误处理

```python
# agent.py
if output.status == "error":
    if attempt == 0 and is_session_corruption_error(output.error):
        # 清除 session 重试
        state._sessions.pop(group.folder, None)
        continue
```

**问题**：错误分类粗糙，仅 success/error 两种状态

### 8.2 pi-mono 错误分类

```typescript
// @mariozechner/pi-ai
type StopReason =
  | "stop"           // 正常完成
  | "length"         // token 超限
  | "toolCalls"      // 工具调用
  | "error"          // 通用错误
  | "overloaded"     // 服务端过载
  | "rate_limit"     // 速率限制
  | "server_error"   // 服务器错误
  | "auth_error"     // 认证错误

// 自动重试逻辑
if (this._isRetryableError(msg)) {
  const didRetry = await this._handleRetryableError(msg)
  if (didRetry) return
}

_isRetryableError(msg: AssistantMessage): boolean {
  return msg.stopReason === "overloaded"
      || msg.stopReason === "rate_limit"
      || msg.stopReason === "server_error"
}
```

---

## 9. Omiga 提升方案

基于 pi-mono 的最佳实践，以下是 Omiga 的优先提升项：

### Phase 1: 会话管理增强（2-3 周）

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 1.1 定义 `SessionEntry` 类型 | 支持消息/压缩/分支等类型 | 高 |
| 1.2 实现 `SessionManager` | 会话树操作 + 持久化 | 高 |
| 1.3 会话树 API | `getTree()`, `navigateTo()`, `forkFrom()` | 中 |

### Phase 2: 事件系统扩展（1-2 周）

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 2.1 定义 `AgentEvent` 类型 | 完整的流式事件 | 高 |
| 2.2 事件处理集成 | 在 `agent.py` 中处理事件 | 高 |
| 2.3 自动重试机制 | 基于错误分类的重试 | 中 |

### Phase 3: 上下文压缩（2 周）

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 3.1 实现 `compact()` 函数 | 对话序列化 + LLM 摘要 | 高 |
| 3.2 自动压缩触发 | 阈值检测 | 中 |
| 3.3 文件操作追踪 | 记录读/写文件历史 | 低 |

### Phase 4: 扩展系统基础（3-4 周）

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 4.1 定义 `Extension` 接口 | 生命周期钩子 + 事件钩子 | 中 |
| 4.2 实现 `ExtensionRunner` | 扩展加载 + 事件分发 | 中 |
| 4.3 扩展工具注册 | 支持扩展注册 LLM 工具 | 低 |

---

## 10. 关键设计决策

### 决策 1：是否保留容器架构？

**推荐**：保留容器作为执行沙箱，但将 Agent 核心循环移到主进程。

```python
# 推荐的混合架构
class AgentSession:
    async def prompt(self, prompt: str) -> str:
        # 1. Think 在主进程
        response = await self.llm.ask(prompt, tools=...)

        # 2. Act 可在容器内
        for tool_call in response.tool_calls:
            result = await self._execute_in_container(tool_call)

        return response.content
```

### 决策 2：会话持久化格式？

**推荐**：采用类似 pi-mono 的行式格式（每行一个 JSON 条目）

```
# session.jsonl
{"type":"header","id":"abc123","timestamp":"...","cwd":"/app"}
{"type":"message","id":"entry1","message":{"role":"user","content":"..."}}
{"type":"message","id":"entry2","message":{"role":"assistant","content":"..."}}
{"type":"compaction","id":"entry3","summary":"...","firstKeptEntryId":"entry2"}
```

### 决策 3：扩展系统实现语言？

**推荐**：Python 扩展（与 Skill 系统整合）

```python
class Extension(ABC):
    name: str
    version: str

    async def on_activate(self, ctx: ExtensionContext): ...
    async def on_event(self, event: ExtensionEvent): ...

    def get_tools(self) -> list[RegisteredTool]: ...
    def get_commands(self) -> list[SlashCommandInfo]: ...
```

---

## 11. 总结

### pi-mono 核心优势

1. **完整的 Agent 生命周期管理** (`AgentSession.prompt()`)
2. **丰富的会话模型** (树状结构、分支、压缩)
3. **强大的扩展系统** (工具/命令/UI/快捷键)
4. **自动上下文压缩** (阈值触发 + 文件追踪)
5. **细粒度的错误分类** (支持自动重试)

### Omiga 差异化优势

1. **多通道支持** (Telegram/WhatsApp/Discord/飞书/QQ)
2. **群组管理** (多群组独立会话)
3. **三层记忆系统** (L1/L2/L3 SOP 管理)
4. **Docker 隔离** (容器化执行环境)

### 最佳整合方案

保留 Omiga 的多通道和记忆系统优势，引入 pi-mono 的以下机制：

1. `AgentSession` 类（进程内 Agent 管理）
2. `SessionManager`（会话树 + 持久化）
3. 完整事件系统（流式事件 + 自动持久化）
4. 上下文压缩（自动触发 + 文件追踪）
