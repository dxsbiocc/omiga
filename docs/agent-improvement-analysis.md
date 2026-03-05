# Omiga Agent 提升空间分析

> 基于 pi-mono、OpenManus、pc-agent-loop (GenericAgent) 的对比分析
> 日期：2026-03-04

---

## 一、当前状态总览

### 1.1 Omiga 现有能力

| 模块 | 状态 | 代码位置 |
|------|------|----------|
| Agent 核心循环 | ✅ `think()` → `act()` | `omiga/agent/session.py` |
| 会话管理 | ✅ 树状结构 + 持久化 | `omiga/session/manager.py` |
| 上下文压缩 | ✅ 自动阈值触发 | `omiga/session/compaction.py` |
| 事件系统 | ✅ 完整流式事件 | `omiga/events/agent_events.py` |
| 错误分类 | ✅ 9 种异常类型 | `omiga/agent/exceptions.py` |
| 工具注册 | ✅ 基础注册表 | `omiga/tools/registry.py` |
| 多通道支持 | ✅ Telegram/飞书/QQ 等 | `omiga/channels/` |
| 记忆系统 | ✅ 三层 SOP 管理 | `omiga/memory/` |

### 1.2 与参考项目对比

| 特性 | Omiga | pi-mono | OpenManus | pc-agent-loop |
|------|-------|---------|-----------|---------------|
| **Agent 架构** | 容器 + 进程内混合 | 进程内 Session | ReAct 模式 | 5 SOP 自进化 |
| **会话树** | ✅ | ✅ | ❌ | ⚠️ 简单历史 |
| **上下文压缩** | ✅ | ✅ | ⚠️ 简单 | ❌ |
| **流式事件** | ✅ | ✅ | ⚠️ Generator | ❌ |
| **错误分类** | ✅ | ✅ | ✅ | ⚠️ 基础 |
| **工具系统** | ⚠️ 基础 | ✅ 扩展注册 | ✅ 丰富内置 | ✅ 自发现 |
| **自进化机制** | ⚠️ SOP 系统 | ❌ | ❌ | ✅ 核心特性 |
| **多通道** | ✅ 核心特性 | ❌ | ❌ | ⚠️ 部分 |
| **代码规模** | ~10K 行 | ~50K 行 | ~20K 行 | ~3.3K 行 |

---

## 二、核心差距分析

### 2.1 哲学层面差异

#### pc-agent-loop 的"种子哲学"

```
大多数 Agent 框架是成品，而 pc-agent-loop 是种子。

5 个核心 SOP 定义思考/记忆/操作方式，之后每个新能力
都由 Agent 自己发现和记录：
1. 用户请求新任务
2. Agent 自行解决（安装依赖、写脚本、测试）
3. 将过程保存为新 SOP
4. 下次直接回忆执行
```

**Omiga 现状**：SOP 系统类似，但缺少"自发现"机制

#### OpenManus 的 ReAct 抽象

```python
class ReActAgent(BaseAgent, ABC):
    @abstractmethod
    async def think(self) -> bool:
        """处理当前状态，决定下一步行动"""

    @abstractmethod
    async def act(self) -> str:
        """执行决定的行动"""
```

**Omiga 现状**：`AgentSession.run()` 实现了类似循环，但抽象层次较低

#### pi-mono 的扩展系统

```typescript
interface Extension {
    name: string;
    version: string;
    onActivate?(ctx: ExtensionContext): Promise<void>;
    onEvent?(event: ExtensionEvent): Promise<void>;
    getTools?(): RegisteredTool[];
    getCommands?(): SlashCommandInfo[];
}
```

**Omiga 现状**：无正式扩展系统（Skill 系统是硬编码的）

---

### 2.2 架构差距

#### 差距 1：缺少统一的 Agent 基类

**OpenManus 的分层设计**：
```
BaseAgent (基础)
    └── ReActAgent (think/act 抽象)
        └── ToolCallAgent (工具调用支持)
            ├── Manus (通用助手)
            ├── BrowserAgent (浏览器专家)
            ├── MCPAgent (MCP 专家)
            └── SWEAgent (软件工程专家)
```

**Omiga 现状**：只有 `AgentSession` 单一类，无继承体系

**建议**：
```python
class BaseAgent(ABC):
    """基础 Agent"""

class ReActAgent(BaseAgent):
    """ReAct 模式抽象"""
    async def think(self) -> bool: ...
    async def act(self) -> str: ...

class ToolCallAgent(ReActAgent):
    """支持工具调用的 Agent"""

class ContainerAgent(ToolCallAgent):
    """容器隔离的 Agent (Omiga 特色)"""
```

---

#### 差距 2：工具系统不够丰富

**OpenManus 内置工具**：
- `CreateChatCompletion` - LLM 调用
- `Terminate` - 终止执行
- `BrowserTool` - 浏览器控制
- `FileTool` - 文件操作
- `ShellTool` - Shell 命令
- `MCPTool` - MCP 协议

**Omiga 现状**：
- 基础工具注册表
- Skills 硬编码在 `omiga/skills/`

**建议**：
1. 增加更多内置工具（浏览器、MCP、视觉等）
2. 支持工具流式输出（`onUpdate` 回调）
3. 工具自动发现机制

---

#### 差距 3：缺少 Memory 抽象

**OpenManus 的 Memory**：
```python
class Memory(BaseModel):
    messages: List[Message] = Field(default_factory=list)
    max_messages: int = Field(default=100)

    def add_message(self, message: Message) -> None: ...
    def get_recent_messages(self, n: int) -> List[Message]: ...
    def to_dict_list(self) -> List[dict]: ...
```

**Omiga 现状**：
- `AgentSession.messages` 是简单列表
- `SessionManager` 管理持久化但非内存抽象

**建议**：
```python
class Memory(BaseModel):
    """Agent 工作记忆"""
    messages: List[Message] = Field(default_factory=list)
    max_messages: int = 100

    # 工作记忆管理
    def add_message(self, message: Message) -> None: ...
    def get_recent_messages(self, n: int) -> List[Message]: ...

    # 与长期记忆交互
    def sync_to_long_term(self, manager: MemoryManager) -> None: ...
```

---

#### 差距 4：缺少 Flow/Workflow 支持

**OpenManus 的 Flow 系统**：
```python
class Flow:
    """多 Agent 协作流程"""
    async def run(self):
        # 编排多个 Agent
        pass
```

**pc-agent-loop 的 SOP 系统**：
```
SOP-0: 任务分解
SOP-1: 工具使用
SOP-2: 环境探索
SOP-3: 记忆存储
SOP-4: 自我反思
```

**Omiga 现状**：
- 三层记忆系统（L1 反应/L2 技能/L3 专家）
- 缺少明确的流程编排

**建议**：
```python
class AgentFlow:
    """Agent 流程编排"""

class SOPExecutor:
    """SOP 执行器"""
    async def execute(self, sop_id: str, params: dict) -> str: ...
```

---

#### 差距 5：UI/可视化缺失

**pi-mono 的 UI 集成**：
```typescript
interface ExtensionContext {
    ui: ExtensionUIContext;
    // setStatus("git-status", "Loading...")
}
```

**pc-agent-loop**：
- Streamlit UI
- WebView 窗口
- 实时状态显示

**Omiga 现状**：
- 简单 Web 界面 (`omiga/web/`)
- 无实时状态显示

**建议**：
1. 增加 Agent 状态实时推送
2. 可视化会话树
3. 工具执行追踪 UI

---

### 2.3 功能差距

| 功能 | Omiga | 参考项目 | 优先级 |
|------|-------|----------|--------|
| **多模态支持** | ❌ | OpenManus(base64_image) | 中 |
| **浏览器注入** | ❌ | pc-agent-loop | 低 |
| **手机 ADB 控制** | ❌ | pc-agent-loop | 低 |
| **键盘/鼠标控制** | ❌ | pc-agent-loop | 低 |
| **屏幕视觉** | ❌ | pc-agent-loop | 低 |
| **MCP 协议** | ❌ | OpenManus | 中 |
| **专家 Agent** | ❌ | OpenManus | 中 |
| **Flow 编排** | ❌ | OpenManus | 低 |

---

## 三、提升方案

### 3.1 短期提升（1-2 周）

#### 任务 1：Memory 抽象层

```python
# omiga/memory/agent_memory.py
from pydantic import BaseModel, Field

class AgentMemory(BaseModel):
    """Agent working memory"""
    messages: List[Message] = Field(default_factory=list)
    max_messages: int = 100
    working_context: Dict[str, Any] = Field(default_factory=dict)

    def add_message(self, message: Message) -> None:
        self.messages.append(message)
        if len(self.messages) > self.max_messages:
            self.messages = self.messages[-self.max_messages:]

    def get_recent_messages(self, n: int) -> List[Message]:
        return self.messages[-n:]

    def get_context(self, key: str, default=None) -> Any:
        return self.working_context.get(key, default)

    def set_context(self, key: str, value: Any) -> None:
        self.working_context[key] = value
```

**收益**：
- 统一记忆管理
- 支持工作上下文
- 更容易与长期记忆集成

---

#### 任务 2：Agent 分层抽象

```python
# omiga/agent/base.py
from abc import ABC, abstractmethod

class BaseAgent(ABC, BaseModel):
    """Base Agent class"""
    name: str
    description: Optional[str] = None
    state: AgentState = AgentState.IDLE

    @abstractmethod
    async def think(self) -> bool:
        """Process current state and decide next action"""
        pass

    @abstractmethod
    async def act(self) -> str:
        """Execute decided actions"""
        pass

    async def step(self) -> str:
        """Execute a single step"""
        should_act = await self.think()
        if not should_act:
            return "Thinking complete"
        return await self.act()
```

```python
# omiga/agent/toolcall.py
class ToolCallAgent(BaseAgent):
    """Agent with tool call support"""
    available_tools: ToolCollection = Field(default_factory=ToolCollection)
    tool_calls: List[ToolCall] = Field(default_factory=list)

    async def think(self) -> bool:
        # LLM call with tool options
        response = await self.llm.ask_tool(
            messages=self.memory.messages,
            tools=self.available_tools.to_params(),
        )
        self.tool_calls = response.tool_calls or []
        return bool(self.tool_calls)

    async def act(self) -> str:
        # Execute tool calls
        results = []
        for call in self.tool_calls:
            result = await self.execute_tool(call)
            results.append(result)
        return "\n".join(results)
```

**收益**：
- 清晰的职责分离
- 易于扩展专家 Agent
- 与 OpenManus 兼容

---

#### 任务 3：增强工具系统

```python
# omiga/tools/base.py
class Tool(ABC):
    name: str
    description: str
    parameters: Dict = {}  # JSON Schema

    # 新增：流式更新回调
    on_update: Optional[Callable[[str], Awaitable[None]]] = None

    @abstractmethod
    async def execute(self, **kwargs) -> ToolResult:
        pass

    async def execute_with_streaming(self, **kwargs) -> ToolResult:
        """支持流式输出的执行"""
        return await self.execute(**kwargs)
```

**收益**：
- 流式工具输出
- 更好的用户体验
- 支持复杂工具（如浏览器操作）

---

### 3.2 中期提升（2-4 周）

#### 任务 4：专家 Agent 系统

```python
# omiga/agent/experts.py

class BrowserExpert(ToolCallAgent):
    """Browser automation expert"""
    name: str = "browser_expert"
    description: str = "Expert at web browsing and automation"

    available_tools: ToolCollection = Field(
        default_factory=lambda: ToolCollection(
            BrowserNavigateTool(),
            BrowserClickTool(),
            BrowserFillTool(),
            BrowserScreenshotTool(),
        )
    )


class CodingExpert(ToolCallAgent):
    """Coding expert"""
    name: str = "coding_expert"
    description: str = "Expert at writing and reviewing code"

    available_tools: ToolCollection = Field(
        default_factory=lambda: ToolCollection(
            ReadFileTool(),
            WriteFileTool(),
            EditFileTool(),
            RunTestsTool(),
        )
    )


class AnalysisExpert(ToolCallAgent):
    """Data analysis expert"""
    name: str = "analysis_expert"
    description: str = "Expert at data analysis and visualization"
```

**收益**：
- 专业化能力
- 更好的任务分配
- 可扩展到任意领域

---

#### 任务 5：Flow 编排系统

```python
# omiga/flow/base.py
class AgentFlow(BaseModel):
    """Agent workflow orchestrator"""
    name: str
    steps: List[FlowStep] = Field(default_factory=list)

    async def run(self, input_data: dict) -> dict:
        results = {}
        for step in self.steps:
            result = await step.execute(results)
            results[step.name] = result
        return results


class FlowStep(BaseModel):
    """A step in a flow"""
    name: str
    agent: Optional[BaseAgent] = None
    task: Optional[str] = None
    condition: Optional[Callable[[dict], bool]] = None

    async def execute(self, context: dict) -> Any:
        if self.condition and not self.condition(context):
            return None

        if self.agent:
            return await self.agent.run(self.task.format(**context))
        return None
```

**示例 Flow**：
```python
# 数据分析流程
analysis_flow = AgentFlow(
    name="data_analysis",
    steps=[
        FlowStep(name="fetch", agent=data_expert, task="Fetch stock data"),
        FlowStep(name="analyze", agent=analysis_expert, task="Analyze trends"),
        FlowStep(name="report", agent=coding_expert, task="Generate report"),
    ]
)
```

**收益**：
- 复杂任务编排
- 可复用工作流
- 支持条件分支

---

#### 任务 6：自进化 SOP 机制（pc-agent-loop 启发）

```python
# omiga/memory/sop_self_learning.py

class SOPLearner:
    """Self-learning SOP generator"""

    async def learn_from_execution(
        self,
        task: str,
        steps_taken: List[StepRecord],
        success: bool,
    ) -> Optional[SOP]:
        """从执行历史学习新 SOP"""
        if not success:
            return None

        # 分析执行轨迹
        pattern = self.extract_pattern(steps_taken)

        # 生成 SOP
        sop = SOP(
            id=f"learned_{task_hash(task)}",
            trigger_pattern=task,
            steps=pattern,
            learned_at=datetime.now(timezone.utc).isoformat(),
            execution_count=1,
        )

        # 存储到 L2 技能记忆
        await self.memory_manager.store_learned_sop(sop)

        return sop
```

**收益**：
- 真正的自进化
- 越用越聪明
- 保留 Omiga 记忆系统特色

---

### 3.3 长期提升（1-2 月）

#### 任务 7：扩展系统（pi-mono 启发）

```python
# omiga/extensions/base.py

class ExtensionContext(BaseModel):
    """Extension execution context"""
    fs: FileSystem
    shell: ShellExecutor
    event_bus: AgentEventBus
    tool_registry: ToolRegistry
    ui: Optional[UIContext] = None


class Extension(ABC):
    """Extension base class"""
    name: str
    version: str

    async def on_activate(self, ctx: ExtensionContext) -> None:
        """Called when extension is activated"""
        pass

    async def on_deactivate(self) -> None:
        """Called when extension is deactivated"""
        pass

    async def on_event(self, event: AgentEvent, ctx: ExtensionContext) -> None:
        """Called on every agent event"""
        pass

    def get_tools(self) -> List[Tool]:
        """Return tools provided by this extension"""
        return []

    def get_commands(self) -> List[SlashCommandInfo]:
        """Return slash commands provided by this extension"""
        return []
```

**示例扩展**：
```python
# 示例：Git 状态扩展
class GitStatusExtension(Extension):
    name = "git-status"
    version = "1.0.0"

    async def on_event(self, event: AgentEvent, ctx: ExtensionContext) -> None:
        if event.type == AgentEventType.AGENT_START:
            # 更新 git 状态
            result = await ctx.shell.exec("git status --porcelain")
            has_changes = bool(result.stdout.strip())
            ctx.ui.set_status("git-status", "●" if has_changes else "✓")
```

**收益**：
- 真正的插件系统
- 社区可扩展
- 保持核心精简

---

#### 任务 8：UI/可视化增强

**建议实现**：
1. WebSocket 实时推送 Agent 状态
2. 会话树可视化
3. 工具执行追踪
4. Token 使用统计

```python
# omiga/web/realtime.py

class RealtimeAgent:
    """Real-time Agent status broadcasting"""

    def __init__(self, event_bus: AgentEventBus):
        self._subscribers: Set[WebSocket] = set()
        event_bus.subscribe("*", self._broadcast_event)

    async def _broadcast_event(self, event: AgentEvent) -> None:
        """Broadcast event to all subscribers"""
        if self._subscribers:
            message = json.dumps(event.to_dict())
            await asyncio.gather(
                *[ws.send_text(message) for ws in self._subscribers],
                return_exceptions=True,
            )
```

---

## 四、差异化优势

在吸收参考项目优点的同时，Omiga 应保持以下差异化优势：

### 4.1 核心优势

| 优势 | 描述 | 如何加强 |
|------|------|----------|
| **多通道支持** | Telegram/飞书/QQ/WhatsApp | 增加更多通道 |
| **群组管理** | 多群组独立会话 | 群组级专家 Agent |
| **三层记忆** | L1 反应/L2 技能/L3 专家 | 自进化 SOP |
| **容器隔离** | Docker 安全执行 | 保持并优化 |

### 4.2 独特定位

```
Omiga = 多通道消息 Agent + 三层记忆系统 + 容器安全 + 自进化 SOP

不是单纯的代码助手，而是：
- 社交机器人（多通道）
- 个人助理（记忆系统）
- 安全执行（容器）
- 持续学习（SOP 自进化）
```

---

## 五、实施路线图

### Phase 5: Memory 抽象（1 周）
- [ ] 实现 `AgentMemory` 类
- [ ] 集成到 `AgentSession`
- [ ] 更新测试

### Phase 6: Agent 分层（1-2 周）
- [ ] 实现 `BaseAgent` 抽象
- [ ] 实现 `ToolCallAgent`
- [ ] 重构现有 `AgentSession`

### Phase 7: 工具增强（1 周）
- [ ] 增加流式输出支持
- [ ] 添加更多内置工具
- [ ] 工具自动发现

### Phase 8: 专家 Agent（2 周）
- [ ] 实现 BrowserExpert
- [ ] 实现 CodingExpert
- [ ] 实现 AnalysisExpert

### Phase 9: Flow 编排（2 周）
- [ ] 实现 `AgentFlow`
- [ ] 创建示例工作流
- [ ] 集成到主循环

### Phase 10: 自进化 SOP（2 周）
- [ ] 实现 `SOPLearner`
- [ ] 执行轨迹追踪
- [ ] 自动 SOP 生成

### Phase 11: 扩展系统（3-4 周）
- [ ] 实现 `Extension` 基类
- [ ] 实现 `ExtensionRunner`
- [ ] 创建示例扩展

---

## 六、代码规模估算

| 阶段 | 新增代码 | 修改代码 | 测试代码 |
|------|----------|----------|----------|
| Phase 5 | ~200 行 | ~100 行 | ~100 行 |
| Phase 6 | ~400 行 | ~200 行 | ~200 行 |
| Phase 7 | ~300 行 | ~100 行 | ~150 行 |
| Phase 8 | ~500 行 | ~50 行 | ~250 行 |
| Phase 9 | ~400 行 | ~100 行 | ~200 行 |
| Phase 10 | ~300 行 | ~200 行 | ~150 行 |
| Phase 11 | ~600 行 | ~300 行 | ~300 行 |
| **总计** | ~2,700 行 | ~1,050 行 | ~1,350 行 |

---

## 七、总结

### 7.1 学习要点

| 项目 | 核心思想 | Omiga 采纳 |
|------|----------|-----------|
| **pi-mono** | 扩展系统、事件流、会话树 | 扩展系统 |
| **OpenManus** | ReAct 抽象、专家 Agent、Flow | Agent 分层、专家系统 |
| **pc-agent-loop** | 种子哲学、SOP 自进化 | 自进化 SOP |

### 7.2 保持特色

1. **多通道优先** - 不仅是代码助手，更是社交机器人
2. **三层记忆** - L1/L2/L3 记忆管理
3. **容器安全** - Docker 隔离执行
4. **渐进式复杂** - 核心精简，扩展丰富

### 7.3 最终目标

```
Omiga 应该成为：
- 最易用的多通道 Agent 框架
- 最具成长性的个人助理系统
- 最安全的代码执行环境
- 越用越聪明的终身伴侣
```
