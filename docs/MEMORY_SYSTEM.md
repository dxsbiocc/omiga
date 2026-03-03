# Omiga 记忆系统使用指南

## 概述

Omiga 的记忆系统实现了**受控成长的 SOP 自生长机制**，灵感来自 pc-agent-loop，但加入了审查机制确保稳定性。

## 核心设计理念

### 1. 行动验证原则 (Action-Verified Only)
> **无执行，不记忆 (No Execution, No Memory)**

任何写入记忆的信息必须源自**成功的工具调用结果**，禁止将模型的推理猜测作为事实写入。

### 2. 审查式成长 (Reviewed Growth)
> 新 SOP 需要审查后才能生效

所有自动生成的 SOP 默认进入 `pending/` 目录，需要用户审查确认后才移动到 `active/`。

### 3. 稳定性优先 (Stability First)
> 核心技能不受影响

- 现有技能目录保持不变
- 新 SOP 在隔离区域生长
- 用户可随时删除/修改 SOP

### 4. 失败学习 (Learn from Failure)
> 失败是成功之母

从失败中提取的教训记录到 `lessons/`，下次遇到类似错误时自动匹配。

---

## 记忆层级架构

```
data/memory/
├── L1/
│   └── index.md           # ≤30 行导航索引 + RULES
├── L2/
│   └── facts.md           # 全局事实库（路径/配置/凭证）
└── L3/
    ├── pending/           # 待审查 SOP
    ├── active/            # 已激活 SOP
    ├── archived/          # 已归档 SOP
    └── lessons/           # 从失败中学习的教训
```

### L1: 导航索引
- **容量限制**: ≤30 条目（硬约束）
- **内容**:
  - 高频场景关键词 → L2/L3 位置映射
  - RULES: 红线规则和高频犯错点

### L2: 全局事实库
- 存储环境特异性事实（路径、配置、凭证）
- 按 SECTION 组织
- 所有事实必须经过行动验证

### L3: SOP 和教训
- **SOP 生命周期**: pending → active → archived
- **教训记录**: 从失败中提取的可复用经验

---

## CLI 命令使用

### 查看记忆系统状态

```bash
# 查看整体状态
omiga memory status

# 输出示例:
# === Omiga Memory Status ===
# L1 Index:
#   Topics: 5/30
#   Rules:  3
# L2 Facts:
#   Sections: 2
# L3 SOPs:
#   Pending:  2 (awaiting review)
#   Active:   3 (in use)
#   Archived: 1 (historical)
# Lessons:
#   Recorded: 5
```

### 管理 SOP

```bash
# 列出待审查 SOP
omiga memory list --pending

# 列出已激活 SOP
omiga memory list --active

# 查看 SOP 详情
omiga memory show SOP_abc123

# 审查 SOP - 批准
omiga memory approve SOP_abc123

# 审查 SOP - 拒绝
omiga memory reject SOP_abc123 --reason="步骤不完整"

# 归档旧 SOP
omiga memory archive SOP_abc123

# 清理 90 天前的归档 SOP
omiga memory cleanup --older-than 90
```

### 查看 L1/L2

```bash
# 查看 L1 索引
omiga memory index

# 查看 L2 事实库
omiga memory facts

# 查看特定 section 的事实
omiga memory facts paths
```

### 查看教训

```bash
# 查看记录的教训
omiga memory lessons
```

---

## 工作流程

### 1. 任务执行 → 自动生成 SOP

当 Agent 成功执行任务后：

```
用户指令 → Agent 执行 → 成功
                ↓
        SOPGenerator 分析执行过程
                ↓
        提取步骤、前置条件、避坑指南
                ↓
        写入 L3/pending/SOP_*.md
```

生成的 SOP 包含：
- 执行步骤
- 前置条件
- 避坑指南
- 元数据（执行时长、工具使用等）

### 2. 失败 → 记录教训

当任务执行失败后：

```
用户指令 → Agent 执行 → 失败
                ↓
        提取错误模式
                ↓
        记录教训到 L3/lessons/
                ↓
        关联到相关 SOP（如有）
```

### 3. 审查激活

用户审查待处理的 SOP：

```bash
# 查看有哪些待审查的 SOP
omiga memory list --pending

# 查看详情
omiga memory show SOP_abc123

# 批准 - 移动到 active/
omiga memory approve SOP_abc123

# 拒绝 - 移动到 archived/
omiga memory reject SOP_abc123 --reason="..."
```

---

## API 使用

### 在技能中使用记忆系统

```python
from omiga.skills.base import Skill, SkillContext
from omiga.memory.manager import MemoryManager

class MySkill(Skill):
    async def execute(self, **kwargs):
        # 获取记忆管理器
        memory_manager = self.context.memory_manager

        # 读取 L2 事实
        facts = memory_manager.get_facts()
        paths = facts.get_section("paths")

        # 添加已验证的事实
        memory_manager.add_fact(
            section="config",
            key="api_endpoint",
            value="https://api.example.com",
            source=self.name,
            verified=True,
        )

        # 执行成功后，SOP 会自动生成
        return result
```

### 手动创建 SOP

```python
from omiga.memory.manager import MemoryManager
from omiga.memory.models import SOPType

memory_manager = MemoryManager(DATA_DIR / "memory")
await memory_manager.initialize()

sop = memory_manager.create_sop(
    name="配置 Gmail OAuth",
    sop_type=SOPType.CONFIGURATION,
    task_id="manual",
    steps=[
        "1. 访问 Google Cloud Console",
        "2. 创建新项目",
        "3. 启用 Gmail API",
        "4. 创建 OAuth 凭据",
        "5. 下载 credentials.json",
    ],
    prerequisites=[
        "需要 Google Cloud 账号",
        "需要管理员权限",
    ],
    pitfalls=[
        "OAuth 回调地址必须正确配置",
        "credentials.json 不要提交到 git",
    ],
)
```

---

## 最佳实践

### 1. 定期审查 SOP

```bash
# 每周检查待审查的 SOP
omiga memory list --pending
```

### 2. 保持 L1 精简

L1 索引限制 30 条目，只添加最高频的场景。

### 3. 利用教训避免重复错误

当遇到错误时：
```bash
# 查看是否有相关教训
omiga memory lessons
```

### 4. 归档旧 SOP

定期清理不再生效的 SOP：
```bash
omiga memory archive SOP_abc123
```

---

## 故障排除

### SOP 没有自动生成

检查：
1. 记忆系统是否正常初始化（查看启动日志）
2. 任务是否执行成功
3. SOP 生成是否达到置信度阈值（>0.5）

### 记忆目录不存在

运行一次 `omiga memory status`，系统会自动初始化。

### SOP 生成质量不高

SOP 生成是基于执行分析的，如果执行过程不清晰，生成的 SOP 质量会降低。可以尝试：
1. 手动创建 SOP（使用 `memory_manager.create_sop()`）
2. 提供更多执行上下文（工具使用、步骤等）

---

## 与 pc-agent-loop 的差异

| 特性 | pc-agent-loop | Omiga |
|------|---------------|-------|
| SOP 生效 | 自动 | 审查后生效 |
| 记忆存储 | 纯文件 | 文件 + 数据库索引 |
| 成长模式 | 完全自主 | 受控成长 |
| 撤销机制 | 手动删除 | archive 命令 |
| 来源追踪 | 弱 | 强（任务 ID 绑定） |
| 教训记录 | 无 | 有（从失败中学习） |

---

## 未来计划

1. **SOP 相似度匹配** - 自动推荐相关 SOP
2. **教训自动应用** - 遇到错误时自动匹配教训
3. **执行统计** - 追踪 SOP 执行成功率和时长
4. **SOP 合并** - 合并相似的 SOP
5. **Web UI** - 可视化管理 SOP 和教训
