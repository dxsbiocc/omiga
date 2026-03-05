# 消息管理与记忆系统整合实施方案

## 1. 现状分析与架构对比

### 1.1 当前 Omiga 架构

```
┌─────────────────────────────────────────────────────────────┐
│                     Omiga 当前架构                           │
├─────────────────────────────────────────────────────────────┤
│  message_loop.py  →  process_group_messages()               │
│       ↓                  ↓                                   │
│  GroupQueue (per-group stdin 管道)                          │
│       ↓                                                       │
│  ContainerOutput (result/error + execution_log)             │
│       ↓                                                       │
│  SOPGenerator.generate() → MemoryManager                     │
│                              ↓                               │
│  ┌───────────────────────────────────────────────┐          │
│  │           三层记忆系统                          │          │
│  │  L1: index.md (导航索引 ≤30 条)                  │          │
│  │  L2: facts.md (全局事实库)                     │          │
│  │  L3: pending/active/archived/lessons/         │          │
│  └───────────────────────────────────────────────┘          │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 参考项目核心机制

| 项目 | 核心机制 | 可借鉴点 |
|------|----------|----------|
| **pc-agent-loop** | 三层记忆 + 行动验证公理 | 记忆写入原则、L1 硬约束 |
| **OpenManus** | AgentState 状态机 + Memory 类 | 状态追踪、防卡死检测 |
| **pi-mono** | AgentEvent 事件流 + Session 持久化 | 事件订阅、消息分区 |
| **opencode-dev** | MessageV2 分区架构 | 细粒度消息类型 |

---

## 2. 整合架构设计

### 2.1 整体架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                        消息入口层                                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │ Telegram    │  │ WhatsApp    │  │  Discord    │  ...        │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                     消息处理层 (processing.py)                   │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  process_group_messages()                                │   │
│  │    ↓                                                     │   │
│  │  1. Admin Command 检测                                    │   │
│  │  2. Trigger Pattern 匹配                                  │   │
│  │  3. 【新增】Agent State 检查                               │   │
│  │  4. 【新增】Stuck Detection 检测                          │   │
│  │  5. ContainerOutput 处理                                  │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    事件总线层 (新增)                              │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  MemoryEvent Bus                                        │   │
│  │    - TOOL_CALL_START / TOOL_CALL_END                    │   │
│  │    - SOP_GENERATED / LESSON_LEARNED                     │   │
│  │    - AGENT_STATE_CHANGED                                │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    记忆持久层                                    │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  MemoryManager (已有)                                    │   │
│  │    ↓                                                     │   │
│  │  L1: index.md + rules                                    │   │
│  │  L2: facts.md (按 section 组织)                           │   │
│  │  L3: SOPs + Lessons                                      │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. 模块详细设计

### 3.1 State 模块扩展 (`omiga/state.py`)

```python
# 新增：Agent 状态枚举
class AgentState(str, Enum):
    IDLE = "IDLE"           # 空闲，可接受新消息
    RUNNING = "RUNNING"     # 正在执行
    FINISHED = "FINISHED"   # 执行完成
    ERROR = "ERROR"         # 错误状态

# 新增：全局状态变量
_agent_states: dict[str, AgentState] = {}  # chat_jid -> AgentState
_agent_retry_counts: dict[str, int] = {}   # chat_jid -> retry count

# 新增：状态访问函数
def get_agent_state(jid: str) -> AgentState:
    return _agent_states.get(jid, AgentState.IDLE)

def set_agent_state(jid: str, state: AgentState) -> None:
    _agent_states[jid] = state
```

**职责边界**：
- 仅存储状态，不包含业务逻辑
- 状态变更由 `processing.py` 触发

---

### 3.2 事件总线 (`omiga/memory/events.py` - 新增)

```python
from enum import Enum
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List
from datetime import datetime, timezone

class MemoryEventType(str, Enum):
    # 工具调用事件
    TOOL_CALL_START = "tool_call_start"
    TOOL_CALL_END = "tool_call_end"

    # SOP/教训事件
    SOP_GENERATED = "sop_generated"
    SOP_STATUS_CHANGED = "sop_status_changed"
    LESSON_LEARNED = "lesson_learned"

    # Agent 状态事件
    AGENT_STATE_CHANGED = "agent_state_changed"

@dataclass
class MemoryEvent:
    type: MemoryEventType
    timestamp: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())
    chat_jid: Optional[str] = None
    data: Dict[str, Any] = field(default_factory=dict)

class MemoryEventBus:
    """轻量级事件总线，用于解耦记忆写入"""

    def __init__(self):
        self._subscribers: Dict[MemoryEventType, List[Callable[[MemoryEvent], None]]] = {}

    def subscribe(self, event_type: MemoryEventType, callback: Callable[[MemoryEvent], None]) -> Callable[[], None]:
        """订阅事件，返回取消订阅函数"""
        self._subscribers.setdefault(event_type, []).append(callback)
        def unsubscribe():
            self._subscribers[event_type].remove(callback)
        return unsubscribe

    def publish(self, event: MemoryEvent) -> None:
        """发布事件，通知所有订阅者"""
        for callback in self._subscribers.get(event.type, []):
            try:
                callback(event)
            except Exception as e:
                logger.warning(f"Event callback failed: {e}")
```

**与 MemoryManager 的关系**：
- 事件总线是**可选的**，用于解耦
- MemoryManager 可以直接调用，也可以通过事件触发

---

### 3.3 Processing 模块扩展 (`omiga/processing.py`)

```python
# 新增：防卡死检测
def is_stuck(chat_jid: str, max_duplicates: int = 2) -> bool:
    """检测 agent 是否陷入重复响应循环"""
    from omiga.database import get_recent_messages

    messages = get_recent_messages(chat_jid, limit=4)
    if len(messages) < 2:
        return False

    # 检查最近 assistant 消息是否有重复
    assistant_contents = [m.content for m in messages if not m.is_from_me]
    if len(assistant_contents) < 2:
        return False

    last_content = assistant_contents[-1]
    duplicate_count = sum(1 for c in assistant_contents[:-1] if c == last_content)
    return duplicate_count >= max_duplicates

# 修改：process_group_messages() 添加状态检查
async def process_group_messages(chat_jid: str) -> bool:
    # ... 现有代码 ...

    # 新增：检查 agent 状态
    current_state = state.get_agent_state(chat_jid)
    if current_state == AgentState.ERROR:
        # 错误恢复逻辑或跳过
        return True

    # 新增：设置 RUNNING 状态
    state.set_agent_state(chat_jid, AgentState.RUNNING)

    try:
        status = await run_agent(group, prompt, chat_jid, _on_output)

        # 新增：检测卡死
        if is_stuck(chat_jid):
            logger.warning("Agent stuck detected, advancing cursor")
            # 处理卡死逻辑

        # 更新状态
        state.set_agent_state(chat_jid,
            AgentState.ERROR if status == "error" else AgentState.IDLE)

    except Exception as e:
        state.set_agent_state(chat_jid, AgentState.ERROR)
        raise
```

---

### 3.4 记忆公理文档 (`omiga/memory/README.md` - 新增)

```markdown
# 记忆系统核心公理

## 公理 1：行动验证原则 (Action-Verified Only)

**定义**：任何写入 L1/L2/L3 的信息，必须源自**成功的工具调用结果**。

**执行标准**：
- ✅ 允许写入：`file_read` 成功返回内容、`shell` 执行成功返回输出
- ❌ 禁止写入：模型推理的猜测、未执行的计划、假设性结论

**口号**：No Execution, No Memory. (无行动，不记忆)

## 公理 2：神圣不可删改性 (Sanctity of Verified Data)

**定义**：经过行动验证的配置、避坑指南，在重构时**严禁丢弃**。

**执行标准**：
- 可以压缩文字、可以迁移层级（L2→L3）
- 绝不能丢失信息的准确性和可追溯性
- 记忆修改只能少量 patch，改不动宁愿不改

## 公理 3：禁止存储易变状态 (No Volatile State)

**定义**：严禁存储随时间/会话高频变化的数据。

**禁止示例**：
- ❌ 当前时间戳
- ❌ 临时 Session ID
- ❌ 正在运行的 PID
- ❌ 绝对路径（除非是固定配置）

## 信息分类决策树

```
这条信息该放哪层？
    ↓
是『环境特异性事实』？(IP、非标路径、凭证、ID、API 密钥等)
    ├─ YES → L2 (global_mem.txt 按 section 组织)
    │         然后 → 按频率归入 L1 第一层 (key→value) 或第二层 (仅关键词)
    │
    └─ NO
         ↓
         是『通用操作规律』？(全局性避坑指南、排查方法)
         ├─ YES → L1 [RULES] (仅限 1 句压缩准则)
         │
         └─ NO
              ↓
              是『特定任务技术』？(艰难尝试成功且未来可复用)
              ├─ YES → L3 (专项 SOP 或脚本)
              │
              └─ NO → 判定为『通用常识』或『冗余信息』: 严禁存储，直接丢弃
```

## L1 硬约束

- **行数限制**：≤ 30 行（硬约束）
- **内容**：
  - 第一层：高频场景 key→value（直接给出 SOP/py/L2 section 名）
  - 第二层：低频场景仅列关键词
  - RULES：红线规则 + 高频犯错点（压缩版）
- **更新时机**：L2/L3 有新增/删除时判断频率归入
```

---

### 3.5 Message 分区扩展 (`omiga/models.py` - 可选)

```python
@dataclass
class MessagePart:
    """消息分区（参考 opencode-dev MessageV2）"""
    type: str  # "text", "tool_result", "reasoning", "file"
    content: Any
    metadata: dict[str, Any] = field(default_factory=dict)
    timestamp: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())

@dataclass
class Message:
    """扩展消息支持分区"""
    id: str
    chat_jid: str
    sender: str
    content: str  # 保持向后兼容
    parts: list[MessagePart] = field(default_factory=list)  # 新增
    timestamp: str
    # ... 其他现有字段 ...
```

**注意**：这是**可选**的，需要评估是否必要：
- 优点：更细粒度的消息处理
- 缺点：需要修改数据库 schema 和现有代码

---

## 4. 实施阶段

### Phase 1: 基础架构（第 1 周）

| 任务 | 文件 | 优先级 |
|------|------|--------|
| 1.1 添加 `AgentState` 枚举 | `omiga/state.py` | 高 |
| 1.2 添加状态追踪变量 | `omiga/state.py` | 高 |
| 1.3 创建事件总线 | `omiga/memory/events.py` | 中 |
| 1.4 编写记忆公理文档 | `omiga/memory/README.md` | 高 |

### Phase 2: 处理逻辑扩展（第 2 周）

| 任务 | 文件 | 优先级 |
|------|------|--------|
| 2.1 添加 `is_stuck()` 检测 | `omiga/processing.py` | 高 |
| 2.2 扩展 `process_group_messages()` | `omiga/processing.py` | 高 |
| 2.3 添加状态变更钩子 | `omiga/agent.py` | 中 |

### Phase 3: 记忆系统优化（第 3 周）

| 任务 | 文件 | 优先级 |
|------|------|--------|
| 3.1 集成事件总线到 MemoryManager | `omiga/memory/manager.py` | 中 |
| 3.2 添加 SOP 执行统计追踪 | `omiga/memory/models.py` | 中 |
| 3.3 实现 L1 行数硬约束检查 | `omiga/memory/manager.py` | 低 |

### Phase 4: 可选增强（第 4 周）

| 任务 | 文件 | 优先级 |
|------|------|--------|
| 4.1 Message 分区架构 | `omiga/models.py` | 低 |
| 4.2 错误细粒度分类 | `omiga/container/runner.py` | 低 |
| 4.3 事件持久化日志 | `omiga/memory/events.py` | 低 |

---

## 5. 冲突与重叠分析

### 5.1 潜在冲突点

| 冲突点 | 描述 | 解决方案 |
|--------|------|----------|
| **状态追踪重复** | `state._agent_states` vs `container_runner` 内部状态 | 明确边界：state.py 只存状态值，状态逻辑在 processing.py |
| **事件 vs 直接调用** | MemoryEventBus vs MemoryManager 直接方法 | 事件总线是可选的，初始阶段直接调用，后期可切换 |
| **Message 分区 vs 现有 content** | 新 `parts` 字段 vs 旧 `content` 字段 | 保持 `content` 向后兼容，`parts` 作为扩展 |
| **L1 硬约束 vs 现有索引** | ≤30 行约束 vs 现有 index.md | 添加自动修剪机制，新增强制约束 |

### 5.2 重叠检测

| 重叠区域 | 描述 | 整合方案 |
|----------|------|----------|
| **SOP 生成 vs Lesson 记录** | 都从 TaskExecution 生成 | 保持现状：成功→SOP，失败→Lesson，清晰分离 |
| **Agent 状态 vs GroupQueue** | 都追踪群组执行状态 | GroupQueue 管调度，AgentState 管业务状态 |
| **防卡死 vs 错误计数** | `_consecutive_errors` vs `is_stuck()` | 互补：错误计数管失败次数，is_stuck() 管重复内容 |

---

## 6. 测试验证

### 6.1 单元测试

```python
# test_agent_state.py
def test_agent_state_transitions():
    state.set_agent_state("jid1", AgentState.IDLE)
    assert state.get_agent_state("jid1") == AgentState.IDLE

# test_is_stuck.py
def test_is_stuck_detection():
    # 模拟重复消息
    assert is_stuck("jid1") == True

# test_memory_axioms.py
def test_action_verified_only():
    # 验证只有成功的工具调用能写入记忆
    assert memory_manager.write_fact(...) requires success=True
```

### 6.2 集成测试

```python
# test_integration.py
async def test_full_message_flow():
    # 消息接收 → 状态检查 → 处理 → 记忆写入 → 状态更新
    assert state.get_agent_state("jid") == AgentState.IDLE
    await process_group_messages("jid")
    assert state.get_agent_state("jid") == AgentState.IDLE  # 回归空闲
```

---

## 7. 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| **数据库 schema 变更** | Message 分区需要迁移 | 保持 content 字段，parts 作为可选扩展 |
| **事件总线性能** | 大量事件可能影响性能 | 初始阶段不使用事件持久化，仅内存通知 |
| **状态不一致** | 异常导致状态未更新 | 使用 try/finally 确保状态恢复 |
| **L1 超限** | 自动修剪可能误删重要信息 | 修剪前归档，支持人工审查 |

---

## 8. 验收标准

### Phase 1 验收
- [ ] `AgentState` 枚举和状态变量已添加
- [ ] 记忆公理文档已编写并团队 review 通过
- [ ] 现有测试全部通过

### Phase 2 验收
- [ ] `is_stuck()` 检测函数正常工作
- [ ] `process_group_messages()` 集成状态检查
- [ ] 添加防卡死单元测试

### Phase 3 验收
- [ ] 事件总线集成到 MemoryManager
- [ ] L1 行数约束自动检查生效
- [ ] SOP 执行统计准确追踪

### Phase 4 验收（可选）
- [ ] Message 分区架构完成（如实施）
- [ ] 错误细粒度分类实现
- [ ] 性能测试通过

---

## 9. 时间估算

| 阶段 | 估算时间 | 依赖 |
|------|----------|------|
| Phase 1 | 3-5 天 | 无 |
| Phase 2 | 5-7 天 | Phase 1 |
| Phase 3 | 3-5 天 | Phase 2 |
| Phase 4 | 5-7 天（可选） | Phase 3 |

**总计**：11-17 天（不含 Phase 4 则 8-12 天）

---

## 10. 关键决策点

### 决策 1：是否实施 Message 分区？
**推荐**：暂缓，先评估现有 `content` 字段是否足够

### 决策 2：事件总线是否必须？
**推荐**：Phase 1 不实施，作为 Phase 3 可选功能

### 决策 3：L1 硬约束是否强制执行？
**推荐**：是，但提供人工审查机制
