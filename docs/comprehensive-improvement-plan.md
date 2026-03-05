# Omiga Agent 功能提升综合方案

> 综合对比 OpenManus、pc-agent-loop、pi-mono、opencode-dev 四个项目后的整体提升方案。

---

## 执行摘要

Omiga 当前在**多通道消息处理**和**三层记忆系统**方面表现良好，但在**Agent 核心架构**、**会话管理**、**扩展系统**三个方面存在显著差距。

**核心差距**（按优先级）：
1. 🔴 **Agent 核心循环缺失** - 无 `think()` → `act()` 循环
2. 🔴 **会话管理薄弱** - 仅存储 session_id，无树状结构/分支
3. 🔴 **上下文压缩缺失** - 无自动压缩机制
4. 🟡 **事件系统不完整** - 缺少 Agent 生命周期事件
5. 🟡 **扩展系统空白** - 无 Extension 机制

---

## 1. 四项目核心特性对比

| 特性 | Omiga | OpenManus | pc-agent-loop | pi-mono |
|------|-------|-----------|---------------|---------|
| **Agent 循环** | ❌ 容器外包 | ✅ ReAct | ✅ dispatch | ✅ Session |
| **会话树** | ❌ | ❌ | ❌ | ✅ 完整 |
| **上下文压缩** | ❌ | ⚠️ 简单 | ❌ | ✅ 自动 |
| **事件系统** | ⚠️ 基础 | ⚠️ 简单 | ⚠️ Generator | ✅ 完整 |
| **扩展系统** | ❌ | ❌ | ❌ | ✅ 完整 |
| **流式输出** | ❌ | ⚠️ 部分 | ✅ yield | ✅ 事件流 |
| **错误分类** | ⚠️ 粗糙 | ✅ 细粒度 | ⚠️ 基础 | ✅ 细粒度 |
| **工具注册** | ⚠️ 基础 | ✅ Collection | ⚠️ 基础 | ✅ 扩展工具 |

---

## 2. Omiga 当前架构问题

### 2.1 核心问题

```
┌─────────────────────────────────────────┐
│ processing.py: process_group_messages() │
│   ↓                                     │
│ agent.py: run_agent()                   │
│   ↓                                     │
│ [Docker 容器] ← 黑盒，无法干预            │
│   ↓                                     │
│ ContainerOutput (批量结果)               │
└─────────────────────────────────────────┘
```

**问题清单**：
- ❌ 无逐步推理（`think()` → `act()`）
- ❌ 工具调用在容器内，无法实时追踪
- ❌ 无会话树/分支概念
- ❌ 无上下文压缩，长对话会 token 超限
- ❌ 错误分类粗糙（只有 success/error）

### 2.2 与 pi-mono 的差距

```typescript
// pi-mono: 完整的 AgentSession
class AgentSession {
  async prompt(): Promise<void> {
    // 1. before_agent_start 事件
    // 2. LLM 流式调用
    // 3. 处理 message_start/update/end
    // 4. 处理 tool_call_start/update/end
    // 5. 会话持久化
    // 6. 检查自动重试
    // 7. 检查上下文压缩
  }
}
```

---

## 3. 提升路线图

### Phase 1: Agent 核心架构重构（2-3 周）

**目标**：实现进程内 `think()` → `act()` 循环

```python
# 新增：omiga/agent_session.py
class AgentSession:
    """Agent 会话管理（进程内）"""

    def __init__(self, group_folder: str):
        self.group_folder = group_folder
        self.state = AgentState.IDLE
        self.messages: list[Message] = []
        self.tool_calls: list[ToolCallRecord] = []
        self.step_count = 0
        self.max_steps = 20

    async def think(self, prompt: str) -> LLMResponse:
        """决定下一步动作"""
        response = await llm.ask(
            messages=self.messages,
            tools=self.registry.to_params()
        )
        return response

    async def act(self, tool_calls: list[ToolCall]) -> list[ToolResult]:
        """执行工具调用"""
        results = []
        for call in tool_calls:
            # 可选择在容器内或进程内执行
            result = await self._execute_tool(call)
            results.append(result)
        return results

    async def run(self, prompt: str) -> str:
        """运行完整循环"""
        self.messages.append(Message.user_message(prompt))

        while self.step_count < self.max_steps:
            # Think
            response = await self.think()
            self.messages.append(response.to_message())

            # Act
            if response.tool_calls:
                results = await self.act(response.tool_calls)
                for call, result in zip(response.tool_calls, results):
                    self.messages.append(Message.tool_message(result, call.id))
            else:
                return response.content

            self.step_count += 1

        return "Max steps reached"
```

**验收标准**：
- [ ] `AgentSession` 类实现并通过单元测试
- [ ] `think()` → `act()` 循环可执行简单任务
- [ ] 工具调用实时可追踪

---

### Phase 2: 会话管理增强（2-3 周）

**目标**：实现完整的会话树管理

```python
# 新增：omiga/session/manager.py
from dataclasses import dataclass
from typing import Literal, Optional
from datetime import datetime

SessionEntryType = Literal[
    "message",           # LLM 消息
    "compaction",        # 压缩摘要
    "branch_summary",    # 分支摘要
    "custom",           # 自定义数据
    "custom_message",   # 自定义消息（进入上下文）
]

@dataclass
class SessionEntry:
    type: SessionEntryType
    id: str
    parent_id: Optional[str]
    timestamp: str
    # 类型特定字段
    message: Optional[Message] = None
    summary: Optional[str] = None
    data: Optional[dict] = None

class SessionManager:
    """会话管理器"""

    def __init__(self, sessions_dir: Path):
        self.sessions_dir = sessions_dir
        self._sessions: dict[str, list[SessionEntry]] = {}

    # 树操作
    def get_tree(self, session_id: str) -> list[SessionEntry]:
        """获取会话树"""
        return self._sessions.get(session_id, [])

    def navigate_to(self, session_id: str, entry_id: str) -> None:
        """导航到指定条目"""

    def fork_from(self, session_id: str, entry_id: str) -> str:
        """从条目创建分支"""

    # 条目操作
    def append_message(self, message: Message) -> None:
        """添加消息"""
        entry = SessionEntry(
            type="message",
            id=generate_id(),
            parent_id=self._last_id(),
            timestamp=datetime.now(timezone.utc).isoformat(),
            message=message
        )
        self._sessions[session_id].append(entry)

    def append_custom(self, custom_type: str, data: dict) -> None:
        """添加自定义数据"""

    # 持久化
    def save(self, session_id: str) -> None:
        """保存会话到文件"""
```

**验收标准**：
- [ ] 会话树 API 实现
- [ ] 会话持久化（JSONL 格式）
- [ ] 导航/分支操作可用

---

### Phase 3: 上下文压缩（2 周）

**目标**：实现自动上下文压缩

```python
# 新增：omiga/session/compaction.py

@dataclass
class CompactionResult:
    summary: str
    first_kept_entry_id: str
    tokens_before: int
    file_operations: dict[str, list[str]]  # read_files, modified_files

async def compact(
    entries: list[SessionEntry],
    model: Model,
    max_tokens: int
) -> CompactionResult:
    """压缩会话上下文"""
    # 1. 序列化对话
    serialized = serialize_conversation(entries)

    # 2. LLM 生成摘要
    response = await llm.ask(
        system_prompt="Summarize this conversation...",
        messages=[Message.user_message(serialized)]
    )

    # 3. 提取文件操作
    file_ops = extract_file_operations(entries)

    return CompactionResult(
        summary=response.content,
        first_kept_entry_id=entries[threshold_index].id,
        tokens_before=count_tokens(entries),
        file_operations=file_ops
    )

# 集成到 AgentSession
async def _check_compaction(self):
    """检查是否需要压缩"""
    threshold = self.settings.get("context.compaction_threshold", 100000)
    current = count_tokens(self.messages)

    if current > threshold:
        result = await compact(self.messages, self.model, threshold * 0.5)
        self.session_manager.save_compaction(result)
        # 移除压缩的条目
        self.messages = self.messages[result.first_kept_entry_id:]
```

**验收标准**：
- [ ] `compact()` 函数实现
- [ ] 自动触发（阈值检测）
- [ ] 文件操作追踪

---

### Phase 4: 事件系统扩展（1-2 周）

**目标**：实现完整的 Agent 事件流

```python
# 扩展：omiga/memory/events.py

class AgentEventType(str, Enum):
    # Agent 生命周期
    AGENT_START = "agent_start"
    AGENT_END = "agent_end"

    # 消息事件
    MESSAGE_START = "message_start"
    MESSAGE_UPDATE = "message_update"  # 流式
    MESSAGE_END = "message_end"

    # 工具事件
    TOOL_CALL_START = "tool_call_start"
    TOOL_CALL_UPDATE = "tool_call_update"  # 流式
    TOOL_CALL_END = "tool_call_end"

    # 错误事件
    ERROR = "error"

@dataclass
class AgentEvent:
    type: AgentEventType
    timestamp: str
    data: dict

# 集成到 AgentSession
class AgentSession:
    async def run(self, prompt: str) -> str:
        self._emit(AgentEvent(type=AgentEventType.AGENT_START))

        try:
            while ...:
                response = await self.think()
                self._emit(AgentEvent(
                    type=AgentEventType.MESSAGE_END,
                    data={"message": response.to_dict()}
                ))

                for call in response.tool_calls:
                    self._emit(AgentEvent(
                        type=AgentEventType.TOOL_CALL_START,
                        data={"tool_name": call.function.name}
                    ))
                    result = await self._execute_tool(call)
                    self._emit(AgentEvent(
                        type=AgentEventType.TOOL_CALL_END,
                        data={"result": result}
                    ))

        except Exception as e:
            self._emit(AgentEvent(
                type=AgentEventType.ERROR,
                data={"error": str(e)}
            ))
            raise
        finally:
            self._emit(AgentEvent(type=AgentEventType.AGENT_END))
```

**验收标准**：
- [ ] 完整事件类型定义
- [ ] 流式事件支持
- [ ] 事件持久化

---

### Phase 5: 错误处理增强（1 周）

**目标**：实现细粒度错误分类和自动重试

```python
# 新增：omiga/exceptions.py

class OmigaError(Exception):
    pass

class SessionCorruptionError(OmigaError):
    pass

class TokenLimitExceeded(OmigaError):
    pass

class ToolExecutionError(OmigaError):
    def __init__(self, tool_name: str, error: str):
        self.tool_name = tool_name
        self.error = error

class RateLimitError(OmigaError):
    pass

class OverloadedError(OmigaError):
    pass

# 自动重试
RETRYABLE_ERRORS = (OverloadedError, RateLimitError)

async def _handle_retryable_error(self, error: OmigaError) -> bool:
    """处理可重试错误"""
    if not isinstance(error, RETRYABLE_ERRORS):
        return False

    self._retry_count += 1
    if self._retry_count > MAX_RETRIES:
        return False

    delay = exponential_backoff(self._retry_count)
    await asyncio.sleep(delay)
    return True
```

---

### Phase 6: 扩展系统基础（3-4 周）

**目标**：实现基础扩展机制

```python
# 新增：omiga/extensions/base.py

class Extension(ABC):
    """扩展基类"""

    name: str
    version: str

    async def on_activate(self, ctx: ExtensionContext):
        """扩展激活时调用"""

    async def on_event(self, event: AgentEvent):
        """事件触发时调用"""

    def get_tools(self) -> list[RegisteredTool]:
        """返回扩展工具"""
        return []

    def get_commands(self) -> list[SlashCommandInfo]:
        """返回斜杠命令"""
        return []

@dataclass
class ExtensionContext:
    """扩展上下文"""
    ui: ExtensionUIContext
    event_bus: EventBus
    fs: FileSystem
    shell: ShellExecutor
    model_registry: ModelRegistry
```

**验收标准**：
- [ ] `Extension` 基类实现
- [ ] 扩展加载机制
- [ ] 事件钩子集成

---

## 4. 总工作量估算

| Phase | 内容 | 时间估算 |
|-------|------|----------|
| Phase 1 | Agent 核心架构 | 2-3 周 |
| Phase 2 | 会话管理 | 2-3 周 |
| Phase 3 | 上下文压缩 | 2 周 |
| Phase 4 | 事件系统 | 1-2 周 |
| Phase 5 | 错误处理 | 1 周 |
| Phase 6 | 扩展系统 | 3-4 周 |
| **总计** | | **11-15 周** |

---

## 5. 关键设计决策

### 决策 1：容器架构保留还是放弃？

**推荐**：混合架构

```python
class AgentSession:
    async def _execute_tool(self, call: ToolCall):
        # 安全敏感工具在容器内执行
        if call.function.name in SANDBOX_TOOLS:
            return await self._execute_in_container(call)
        # 其他工具在进程内执行
        else:
            return await self.tool_registry.execute(call.function.name)
```

### 决策 2：会话持久化格式？

**推荐**：JSONL（每行一个 JSON 对象）

```jsonl
{"type":"header","id":"abc","timestamp":"...","cwd":"/app"}
{"type":"message","id":"e1","message":{"role":"user","content":"..."}}
{"type":"message","id":"e2","message":{"role":"assistant","content":"..."}}
{"type":"compaction","id":"e3","summary":"...","firstKeptEntryId":"e2"}
```

### 决策 3：与现有 Skill 系统整合？

**推荐**：保留 Skill 系统，扩展为统一抽象

```python
class Skill(ABC):
    """统一技能抽象（替代 Tool 和 Skill）"""
    name: str
    description: str
    parameters: dict

    async def execute(self, context: SkillContext, **kwargs) -> SkillResult:
        pass
```

---

## 6. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 架构重构破坏现有功能 | 高 | 分阶段实施，每阶段保留回滚 |
| 容器内外逻辑分离复杂 | 中 | 明确边界：think 在内，act 可在外 |
| 会话迁移工作量大 | 中 | 提供迁移工具，保持向后兼容 |
| 扩展系统性能开销 | 低 | 支持开关，默认关闭 |

---

## 7. 下一步行动

1. **立即可做**（本周）：
   - [ ] Review 并批准本方案
   - [ ] 开始 Phase 1: AgentSession 设计

2. **Phase 1 准备**：
   - [ ] 创建 `omiga/agent_session.py`
   - [ ] 定义 `AgentState` 枚举
   - [ ] 实现 `think()` 和 `act()` 方法

3. **并行工作**：
   - [ ] 会话管理设计（Phase 2）
   - [ ] 上下文压缩算法研究（Phase 3）

---

## 8. 参考资料

- [记忆整合实施方案](memory-integration-plan.md)
- [Phase 1&2 实施总结](phase1-2-summary.md)
- [Agent 差距分析](agent-gap-analysis.md)
- [pi-mono 对比分析](pi-mono-comparison.md)
