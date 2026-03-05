# Omiga Agent 核心功能差距分析与提升方案

> 基于与 pc-agent-loop、OpenManus、pi-mono、opencode-dev 的对比分析

---

## 执行摘要

Omiga 当前架构在**消息处理**和**记忆系统**方面表现良好，但在**Agent 核心循环**、**工具调用抽象**、**会话管理**三个关键领域存在显著差距。

**关键差距**：
1. 缺少 ReAct 模式的核心 `think()` → `act()` 循环
2. 工具/Skill 抽象层级混乱（Tool vs Skill 职责不清）
3. 会话管理依赖外部容器，缺乏内部状态追踪
4. 缺少流式输出处理和实时反馈机制

---

## 1. Agent 核心架构对比

### 1.1 当前 Omiga 架构

```
┌─────────────────────────────────────────────────────┐
│  processing.py: process_group_messages()            │
│    ↓                                                 │
│  agent.py: run_agent()                              │
│    ↓                                                 │
│  container/runner.py: run_container_agent()         │
│    ↓                                                 │
│  [外部 Docker 容器：omiga-py-agent]                  │
│    ↓                                                 │
│  ContainerOutput(result/error/execution_log)        │
└─────────────────────────────────────────────────────┘
```

**问题**：
- Agent 逻辑完全外包给容器，主进程无法干预执行过程
- 缺少中间的 `think/act` 循环，无法实现逐步推理
- 工具调用追踪在容器内，SOP 生成依赖事后日志分析

---

### 1.2 OpenManus 架构（参考）

```
┌─────────────────────────────────────────────────────┐
│  BaseAgent.run()                                    │
│    ↓                                                 │
│  ReActAgent.step()                                  │
│    ├─→ think() → LLM 决定下一步动作                   │
│    └─→ act() → 执行工具调用                          │
│         ↓                                            │
│  ToolCollection.execute()                           │
│    ↓                                                │
│  工具结果直接返回到内存                              │
└─────────────────────────────────────────────────────┘
```

**优势**：
- 清晰的 `think()` → `act()` 分离
- 工具调用在进程内，可实时追踪
- 支持 `max_steps` 控制和 `is_stuck()` 检测

---

### 1.3 pc-agent-loop 架构（参考）

```
┌─────────────────────────────────────────────────────┐
│  agent_runner_loop()                                │
│    ↓                                                 │
│  handler.dispatch(tool_name, args, response)        │
│    ├─→ tool_before_callback()                       │
│    ├─→ do_<tool_name>()                             │
│    └─→ tool_after_callback()                        │
│         ↓                                            │
│  StepOutcome(data, next_prompt, should_exit)        │
└─────────────────────────────────────────────────────┘
```

**优势**：
- 生成器模式支持流式输出 (`yield`)
- 工具调用前后钩子（便于 AOP 切面）
- `StepOutcome` 统一返回格式

---

### 1.4 差距总结

| 特性 | Omiga | OpenManus | pc-agent-loop |
|------|-------|-----------|---------------|
| **Think/Act 分离** | ❌ | ✅ ReActAgent | ✅ handler.dispatch |
| **进程内工具调用** | ❌ (容器外) | ✅ | ✅ |
| **流式输出** | ❌ | ⚠️ 部分 | ✅ Generator |
| **步骤追踪** | ❌ (事后日志) | ✅ 每 step 记录 | ✅ 每 turn 记录 |
| **提前终止** | ❌ | ✅ `should_exit` | ✅ `should_exit` |

---

## 2. 工具/Skill 抽象对比

### 2.1 当前 Omiga 问题

**双重抽象混乱**：
```
tools/base.py: Tool (基类)
  ├─→ ReadFileTool, WriteFileTool, ExecuteCommandTool...

skills/base.py: Skill (基类)
  ├─→ SystemSkill, ExcelSkill, PdfSkill...
```

**职责不清**：
- `Tool`：低级原语（文件读写、命令执行）
- `Skill`：高级业务逻辑（但实现中仍调用 Tool）
- 两者都有 `execute()` 方法，调用链路不清晰

**缺少注册机制**：
- Tool 有 `ToolRegistry`，但 Skill 没有对应的注册表
- LLM 工具调用配置分散在各处

---

### 2.2 OpenManus ToolCollection（参考）

```python
class ToolCollection:
    def __init__(self, *tools: BaseTool):
        self.tool_map: dict[str, BaseTool] = {}
        for tool in tools:
            self.tool_map[tool.name] = tool

    def to_params(self) -> list[dict]:
        """转换为 LLM function calling schema"""
        return [tool.to_params() for tool in self.tool_map.values()]

    async def execute(self, name: str, **kwargs) -> ToolResult:
        """执行工具并返回结果"""
        return await self.tool_map[name].execute(**kwargs)
```

**优势**：
- 统一注册和管理
- `to_params()` 自动生成 LLM schema
- `execute()` 统一入口

---

### 2.3 提升方案

**合并 Tool/Skill 为统一抽象**：

```python
class Skill(ABC):
    """统一技能抽象（替代 Tool 和 Skill）"""

    name: str
    description: str
    parameters: dict  # JSON schema

    @abstractmethod
    async def execute(self, context: SkillContext, **kwargs) -> SkillResult:
        pass

    def to_llm_tool(self) -> dict:
        """转换为 LLM 工具格式"""
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters
            }
        }

class SkillRegistry:
    """技能注册表（统一管理）"""

    def __init__(self):
        self._skills: dict[str, Skill] = {}

    def register(self, skill: Skill) -> None:
        self._skills[skill.name] = skill

    def get(self, name: str) -> Optional[Skill]:
        return self._skills.get(name)

    def to_llm_tools(self) -> list[dict]:
        """转换为 LLM 工具列表"""
        return [skill.to_llm_tool() for skill in self._skills.values()]
```

---

## 3. 会话管理对比

### 3.1 当前 Omiga

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
- `session_id` 仅用于容器，主进程不追踪会话内容
- 会话历史存储在容器内，主进程无法访问
- 会话损坏检测依赖错误消息匹配（`is_session_corruption_error`）

---

### 3.2 pi-mono AgentSession（参考）

```typescript
class AgentSession {
    private state: AgentState = AgentState.IDLE
    private messages: Message[] = []
    private tools: ToolRegistry
    private events: EventEmitter

    async run(prompt: string): Promise<SessionResult> {
        this.state = AgentState.RUNNING
        this.events.emit('session:start')

        while (!this.isFinished()) {
            const step = await this.think()
            if (step.action) {
                const result = await this.act(step.action)
                this.messages.push(result.toMessage())
            }
        }

        this.events.emit('session:end')
        return this.buildResult()
    }

    subscribe(listener: (event: AgentEvent) => void): () => void {
        // 事件订阅
    }
}
```

**优势**：
- 会话状态在主进程中管理
- 消息历史实时可访问
- 事件订阅机制解耦

---

### 3.3 提升方案

**在 Omiga 中引入内部会话追踪**：

```python
# 新增：omiga/session.py
class AgentSession:
    """Agent 会话管理（进程内）"""

    def __init__(self, group_folder: str):
        self.group_folder = group_folder
        self.state = AgentState.IDLE
        self.messages: list[Message] = []
        self.tool_calls: list[ToolCallRecord] = []
        self.step_count = 0
        self.max_steps = 20

    def add_message(self, message: Message) -> None:
        self.messages.append(message)

    def record_tool_call(self, record: ToolCallRecord) -> None:
        self.tool_calls.append(record)

    def is_finished(self) -> bool:
        return self.state in (AgentState.FINISHED, AgentState.ERROR)

    def is_stuck(self, threshold: int = 2) -> bool:
        """检测重复响应"""
        if len(self.messages) < 2:
            return False
        last = self.messages[-1].content
        count = sum(1 for m in self.messages[:-1] if m.content == last)
        return count >= threshold


# 修改：omiga/state.py
_sessions: dict[str, AgentSession] = {}  # group_folder -> AgentSession

def get_session(group_folder: str) -> Optional[AgentSession]:
    return _sessions.get(group_folder)

def create_session(group_folder: str) -> AgentSession:
    session = AgentSession(group_folder)
    _sessions[group_folder] = session
    return session
```

---

## 4. 流式输出与实时反馈

### 4.1 当前 Omiga

```python
# processing.py
async def _on_output(result: ContainerOutput) -> None:
    if result.result:
        # 一次性接收完整输出
        text = format_outbound(result.result)
        await channel.send_message(chat_jid, text)
```

**问题**：
- 输出是批量的，无法实时反馈
- 长任务（如多工具调用）用户等待时间长
- 无法显示 "正在思考..." 或 "正在调用工具..." 等中间状态

---

### 4.2 pc-agent-loop 生成器模式（参考）

```python
def agent_runner_loop(...):
    for turn in range(max_turns):
        yield f"**LLM Running (Turn {turn+1}) ...**\n\n"

        response = yield from client.chat(...)

        if tool_name != 'no_tool':
            yield f"🛠️ **正在调用工具:** `{tool_name}`\n"

        gen = handler.dispatch(tool_name, args, response)
        outcome = yield from gen
        yield '`````\n' + outcome.data + '\n`````\n'
```

**优势**：
- 实时输出每个步骤
- 用户可以中途看到进展
- 支持 verbose 模式开关

---

### 4.3 提升方案

**在容器 runner 中添加流式回调**：

```python
# 新增：omiga/container/runner.py
class StreamEvent:
    """流式事件类型"""
    THINKING_START = "thinking_start"
    THINKING_END = "thinking_end"
    TOOL_CALL_START = "tool_call_start"
    TOOL_CALL_END = "tool_call_end"
    OUTPUT_CHUNK = "output_chunk"

@dataclass
class StreamOutput:
    event_type: str
    data: Any = None
    timestamp: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())

# 修改：ContainerInput
@dataclass
class ContainerInput:
    prompt: str
    session_id: Optional[str]
    group_folder: str
    chat_jid: str
    is_main: bool
    assistant_name: str
    stream_callback: Optional[Callable[[StreamOutput], Awaitable[None]]] = None  # 新增
```

---

## 5. 错误处理与恢复

### 5.1 当前 Omiga

```python
# agent.py
if output.status == "error":
    if attempt == 0 and is_session_corruption_error(output.error or ""):
        # 清除 session 并重试
        state._sessions.pop(group.folder, None)
        await set_session(group.folder, "")
        continue
```

**问题**：
- 错误分类粗糙（只有 success/error）
- 重试策略单一（仅 session corruption）
- 缺少细粒度错误恢复（如 token 超限、工具调用失败）

---

### 5.2 OpenManus 细粒度错误处理（参考）

```python
try:
    response = await self.llm.ask_tool(...)
except ValueError:
    raise
except Exception as e:
    # 检查是否为 TokenLimitExceeded
    if hasattr(e, "__cause__") and isinstance(e.__cause__, TokenLimitExceeded):
        logger.error(f"Token limit error: {e.__cause__}")
        self.memory.add_message(Message.assistant_message("Token limit reached"))
        self.state = AgentState.FINISHED
        return False
    raise
```

---

### 5.3 提升方案

**定义细粒度错误类型**：

```python
# 新增：omiga/exceptions.py
class OmigaError(Exception):
    """Base error class"""
    pass

class SessionCorruptionError(OmigaError):
    """Session history corrupted"""
    pass

class TokenLimitExceeded(OmigaError):
    """LLM token limit exceeded"""
    pass

class ToolExecutionError(OmigaError):
    """Tool execution failed"""
    def __init__(self, tool_name: str, error: str):
        self.tool_name = tool_name
        self.error = error
        super().__init__(f"Tool {tool_name} failed: {error}")

class StuckDetectedError(OmigaError):
    """Agent stuck in loop"""
    pass

# 使用示例
try:
    output = await run_container_agent(...)
except SessionCorruptionError:
    # 清除会话并重试
    await clear_session(group.folder)
    output = await run_container_agent(...)
except TokenLimitExceeded:
    # 通知用户上下文过长
    await send_error_message("上下文过长，请开始新对话")
except ToolExecutionError as e:
    # 记录具体工具错误
    logger.error(f"Tool {e.tool_name} failed: {e.error}")
```

---

## 6. 提升优先级与实施路线图

### Phase 1: 核心架构重构（2-3 周）

| 任务 | 文件 | 优先级 | 工作量 |
|------|------|--------|--------|
| 1.1 创建 `AgentSession` 类 | `omiga/session.py` | 高 | 2 天 |
| 1.2 实现 `think()` → `act()` 循环 | `omiga/agent_loop.py` | 高 | 3 天 |
| 1.3 合并 Tool/Skill 为统一抽象 | `omiga/skills/base.py` | 高 | 3 天 |
| 1.4 创建 `SkillRegistry` | `omiga/skills/registry.py` | 中 | 1 天 |

### Phase 2: 会话与错误管理（1-2 周）

| 任务 | 文件 | 优先级 | 工作量 |
|------|------|--------|--------|
| 2.1 细粒度错误类型定义 | `omiga/exceptions.py` | 高 | 1 天 |
| 2.2 会话状态追踪集成 | `omiga/state.py` | 高 | 2 天 |
| 2.3 错误恢复策略 | `omiga/agent.py` | 中 | 2 天 |

### Phase 3: 流式输出（1 周）

| 任务 | 文件 | 优先级 | 工作量 |
|------|------|--------|--------|
| 3.1 定义 `StreamEvent` 和 `StreamOutput` | `omiga/container/runner.py` | 中 | 2 天 |
| 3.2 实现流式回调 | `omiga/processing.py` | 中 | 2 天 |

### Phase 4: 高级功能（可选）

| 任务 | 说明 | 优先级 |
|------|------|--------|
| 4.1 工具调用钩子 | tool_before_callback / tool_after_callback | 低 |
| 4.2 多 Agent 协作 | sub-agent 调度 | 低 |
| 4.3 计划模式 | plan-mode 先规划后执行 | 低 |

---

## 7. 关键设计决策

### 决策 1：是否保留容器架构？

**推荐**：保留容器作为执行沙箱，但将 `think()` 循环移到主进程。

**理由**：
- 容器提供隔离环境（依赖、文件系统）
- `think()` 在主进程可实现更好的追踪和调试
- 工具调用可配置在容器内或进程内

### 决策 2：Tool/Skill 合并策略？

**推荐**：统一为 `Skill` 抽象，内部可组合使用低级 `Tool`。

**理由**：
- 减少概念混乱
- 技能可测试性更好
- LLM 工具注册更清晰

### 决策 3：流式输出实现方式？

**推荐**：生成器模式 + 回调函数。

**理由**：
- 与 pc-agent-loop 一致
- 支持实时反馈
- 可开关 verbose 模式

---

## 8. 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| **架构重构影响现有功能** | 高 | 分阶段实施，每阶段保留回滚 |
| **容器内外逻辑分离复杂** | 中 | 明确边界：think 在内，act 可在外 |
| **流式输出性能开销** | 低 | 支持开关，默认关闭 |
| **Skill 迁移工作量大** | 中 | 渐进式迁移，保持向后兼容 |

---

## 9. 验收标准

### Phase 1 验收
- [ ] `AgentSession` 类实现并通过单元测试
- [ ] `think()` → `act()` 循环可执行简单任务
- [ ] `Skill` 统一抽象可替代现有 Tool/Skill
- [ ] 现有测试全部通过

### Phase 2 验收
- [ ] 细粒度错误类型可正确抛出和捕获
- [ ] 会话状态在主进程中可追踪
- [ ] 错误恢复策略生效（如 session corruption 自动恢复）

### Phase 3 验收
- [ ] 流式输出可实时显示思考过程
- [ ] 工具调用可触发回调通知
- [ ] verbose 模式可开关

---

## 10. 结论

Omiga 当前架构在**消息处理**和**记忆系统**方面已经相当成熟，但**Agent 核心功能**与参考项目相比存在明显差距：

1. **最紧急**：缺少 `think()` → `act()` 循环，导致无法实现逐步推理和实时反馈
2. **最混乱**：Tool/Skill 双重抽象，职责不清
3. **最薄弱**：会话管理依赖外部容器，缺少内部追踪

通过实施上述 Phase 1-3，Omiga 可以达到与 OpenManus 和 pc-agent-loop 相当的 Agent 核心能力，同时保留现有的消息处理和记忆系统优势。
