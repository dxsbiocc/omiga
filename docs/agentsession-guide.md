# AgentSession 使用指南

> Phase 1 实施完成后的核心架构升级

---

## 概述

`AgentSession` 是 Omiga Agent 的核心类，实现了完整的 `think()` → `act()` 循环。它提供了：

- **会话状态管理** - 追踪 Agent 执行状态（IDLE, THINKING, ACTING, FINISHED, ERROR）
- **消息历史** - 管理对话上下文
- **工具调用追踪** - 记录每次工具调用的详细日志
- **防卡死检测** - 自动检测重复响应
- **事件回调** - 支持订阅 Agent 生命周期事件

---

## 快速开始

### 1. 创建 AgentSession

```python
from omiga.agent_session import AgentSession
from omiga.tools.registry import ToolRegistry
from omiga.tools.base import ToolContext

# 创建工具注册表
ctx = ToolContext(working_dir="/app", data_dir="/app/data")
registry = ToolRegistry(ctx)

# 注册工具
from omiga.tools.file_tools import ReadFileTool, WriteFileTool
registry.register(ReadFileTool(ctx))
registry.register(WriteFileTool(ctx))

# 创建 Agent 会话
session = AgentSession(
    group_folder="my_group",
    tool_registry=registry,
    max_steps=20,
)
```

### 2. 设置事件回调（可选）

```python
async def on_thinking_start():
    print("Agent is thinking...")

async def on_thinking_end(response):
    print(f"Thought: {response.content[:50]}...")

async def on_tool_call_start(record):
    print(f"Calling tool: {record.tool_name}")

async def on_tool_call_end(record):
    print(f"Tool result: {record.success}")

session.on_thinking_start(on_thinking_start)
session.on_thinking_end(on_thinking_end)
session.on_tool_call_start(on_tool_call_start)
session.on_tool_call_end(on_tool_call_end)
```

### 3. 运行会话

```python
import asyncio

async def main():
    result = await session.run("请帮我读取 /app/config.json 文件")
    print(f"Final result: {result}")

asyncio.run(main())
```

---

## 核心 API

### AgentSession 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `group_folder` | str | 群组文件夹标识符 |
| `state` | SessionState | 当前会话状态 |
| `messages` | List[Message] | 对话历史 |
| `tool_calls` | List[ToolCallRecord] | 工具调用历史 |
| `step_count` | int | 当前步骤数 |
| `max_steps` | int | 最大步骤限制 |

### AgentSession 方法

#### 生命周期方法

```python
# 运行完整的 think→act 循环
result = await session.run(prompt: str, system_prompt: Optional[str] = None) -> str

# 清除会话状态
session.clear()

# 检查会话是否结束
is_done = session.is_finished()

# 检查是否卡死
is_stuck = session.is_stuck(threshold: int = 2)
```

#### 消息管理

```python
# 添加消息
session.add_message(Message.user_message("Hello"))
session.add_message(Message.assistant_message("Hi there"))
session.add_message(Message.tool_message(result, "call_123"))

# 获取会话摘要
summary = session.get_summary()
# {
#     "group_folder": "my_group",
#     "state": "IDLE",
#     "message_count": 5,
#     "tool_call_count": 2,
#     "step_count": 1
# }
```

#### 事件订阅

```python
# 订阅思考事件
session.on_thinking_start(callback)
session.on_thinking_end(callback)

# 订阅工具调用事件
session.on_tool_call_start(callback)
session.on_tool_call_end(callback)
```

---

## 高级用法

### 1. 手动控制 think/act 循环

```python
async def custom_loop():
    # 添加用户消息
    session.add_message(Message.user_message("任务描述"))

    while session.step_count < session.max_steps:
        # Think
        response = await session.think()
        session.add_message(Message.assistant_message(
            content=response.content,
            tool_calls=response.tool_calls
        ))

        # Act
        if response.tool_calls:
            results = await session.act(response.tool_calls)
            for call, result in zip(response.tool_calls, results):
                session.add_message(Message.tool_message(
                    result, call.get("id")
                ))
        else:
            return response.content

        session.step_count += 1

    return "Max steps reached"
```

### 2. 与 state 模块集成

```python
import omiga.state as state

# 获取或创建会话
chat_jid = "tg:123456"
session = state.get_agent_session(chat_jid)

if session is None:
    group = state._registered_groups.get(chat_jid)
    session = state.create_agent_session(chat_jid, group.folder)

# 使用会话
result = await session.run("用户请求")
```

### 3. 错误处理

```python
from omiga.exceptions import (
    OmigaError,
    ToolExecutionError,
    StuckDetectedError,
    TokenLimitExceeded,
    RETRYABLE_ERRORS,
)

async def run_with_retry(session, prompt, max_retries=3):
    for attempt in range(max_retries):
        try:
            return await session.run(prompt)
        except StuckDetectedError:
            logger.warning("Agent stuck, clearing session")
            session.clear()
            continue
        except TokenLimitExceeded as e:
            logger.error(f"Token limit: {e.used}/{e.limit}")
            return "上下文过长，请开始新对话"
        except ToolExecutionError as e:
            logger.error(f"Tool {e.tool_name} failed: {e.error}")
            if attempt == max_retries - 1:
                raise
        except RETRYABLE_ERRORS:
            logger.warning(f"Retryable error, attempt {attempt + 1}")
            if attempt == max_retries - 1:
                raise

    return "Max retries reached"
```

---

## 与现有系统集成

### 集成到 processing.py

当前的 `process_group_messages()` 调用 `run_agent()`，后者在容器内执行。
集成 `AgentSession` 后：

```python
# processing.py 修改示例
from omiga.agent_session import AgentSession
import omiga.state as state

async def process_group_messages(chat_jid: str) -> bool:
    group = state._registered_groups.get(chat_jid)
    if not group:
        return True

    # 获取或创建会话
    session = state.get_agent_session(chat_jid)
    if session is None:
        session = state.create_agent_session(chat_jid, group.folder)

    # 检查是否卡死
    if session.is_stuck():
        logger.warning("Agent stuck detected")
        session.clear()
        return True

    # 运行会话
    try:
        result = await session.run(prompt)

        # 发送结果
        if result:
            await channel.send_message(chat_jid, result)

        state.set_agent_state(chat_jid, AgentState.IDLE)

    except Exception as e:
        logger.error(f"Agent error: {e}")
        state.set_agent_state(chat_jid, AgentState.ERROR)
        raise

    return True
```

---

## 测试示例

```python
import pytest
from omiga.agent_session import AgentSession, Message

class TestAgentSession:
    @pytest.fixture
    def session(self):
        return AgentSession(group_folder="test")

    def test_add_message(self, session):
        session.add_message(Message.user_message("Hello"))
        assert len(session.messages) == 1

    def test_is_stuck(self, session):
        session.add_message(Message.assistant_message("Same"))
        session.add_message(Message.assistant_message("Same"))
        session.add_message(Message.assistant_message("Same"))
        assert session.is_stuck() is True

    def test_clear(self, session):
        session.add_message(Message.user_message("Test"))
        session.clear()
        assert len(session.messages) == 0
```

---

## 下一步

Phase 1 完成后，后续阶段将添加：

- **Phase 2**: SessionManager - 会话树管理和持久化
- **Phase 3**: 上下文自动压缩
- **Phase 4**: 完整事件系统（流式事件）
- **Phase 5**: 细粒度错误分类和自动重试
- **Phase 6**: 扩展系统基础

---

## 参考资料

- [comprehensive-improvement-plan.md](../docs/comprehensive-improvement-plan.md)
- [agent-gap-analysis.md](../docs/agent-gap-analysis.md)
- [pi-mono-comparison.md](../docs/pi-mono-comparison.md)
