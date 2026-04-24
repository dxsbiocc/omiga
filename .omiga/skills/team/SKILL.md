---
name: team
description: >
  并行多 Agent 协作 — 把大型分析任务分解为独立切片，
  同时派遣多个 Executor 并行执行，最后由 Architect 汇总验收。
when_to_use: >
  当任务有多个可独立执行的切片时使用（多组样本、多条分析路线、
  多个可视化任务）。触发词：parallel/team/并行/团队模式。
tags:
  - orchestration
  - parallel
  - team
  - multi-agent
  - research
---

# Team — 并行多 Agent 协作

## 适用场景

- 分析 N 个样本组，每组相互独立
- 同时执行多条不同的分析流程（表达量分析 + 通路富集 + 可视化）
- 批量处理：多个文件、多个时间点、多个条件对比

**不适合 Team 的情况**：步骤之间有强依赖（A 的输出是 B 的输入）→ 用 Ralph 顺序执行。

---

## Phase 0 — 分解与规划

**0.0 写入上下文快照**

在分解任务之前，先记录本次协作的完整上下文：

```bash
SESSION_ID="team-$(date +%Y%m%d-%H%M%S)"
GOAL="$ARGUMENTS_OR_GOAL_TEXT"
SLUG=$(echo "$GOAL" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | sed 's/-\+/-/g' | cut -c1-40 | sed 's/^-//;s/-$//')
mkdir -p .omiga/context

python3 - << PYEOF
import subprocess, datetime, os

goal    = os.environ.get('GOAL', 'unknown goal')
session = os.environ.get('SESSION_ID', 'team-unknown')
slug    = os.environ.get('SLUG', 'task')
snap    = f".omiga/context/{slug}-{session}.md"

def run(cmd):
    try:
        return subprocess.check_output(cmd, shell=True, stderr=subprocess.DEVNULL, text=True).strip()
    except Exception:
        return '(unavailable)'

lines = [
    f"# Context Snapshot: {goal[:80]}",
    "",
    f"**Session**: {session}",
    f"**Mode**: team",
    f"**Project**: {os.getcwd()}",
    f"**Created**: {datetime.datetime.utcnow().isoformat()}Z",
    "",
    "## Goal",
    "",
    goal,
    "",
    "## Environment",
    "",
    f"- **Python**: {run('python3 --version')}",
    f"- **Conda env**: {run('echo ${CONDA_DEFAULT_ENV:-none}')}",
    f"- **OS**: {run('uname -sr')}",
    "",
    "## Project State",
    "",
    "```",
    run("git log --oneline -5 2>/dev/null || echo 'not a git repo'"),
    "```",
]
os.makedirs(os.path.dirname(snap), exist_ok=True)
with open(snap, 'w') as f:
    f.write('\n'.join(lines) + '\n')
print(f"Context snapshot: {snap}")
PYEOF
```

**0.1 理解任务**

明确：
- 有哪些独立切片？（每个切片 = 一个 Worker 的工作量）
- 每个切片需要什么输入、产出什么输出？
- 切片之间是否真的独立？（确认无依赖）

**0.2 制定分工方案**

| Worker | 切片描述 | 输入 | 输出 | Agent 类型 |
|--------|---------|------|------|-----------|
| W1 | 样本组 A 的 DESeq2 分析 | data/groupA/ | results/groupA/ | executor |
| W2 | 样本组 B 的 DESeq2 分析 | data/groupB/ | results/groupB/ | executor |
| W3 | 通路富集分析（基于 W1+W2 完成后） | — | — | executor |

Agent 类型选择：
- `executor`：执行代码、运行脚本
- `Plan`：制定子任务的详细方案（用于复杂切片）
- `Explore`：只读探索数据结构

**0.3 调用 TodoWrite 记录工作计划**

```json
{
  "todos": [
    {"content": "分解任务，确认切片独立性", "status": "completed"},
    {"content": "并行执行：[W1描述]", "status": "in_progress"},
    {"content": "并行执行：[W2描述]", "status": "in_progress"},
    {"content": "汇总所有 Worker 结果", "status": "pending"},
    {"content": "Architect 验收最终结果", "status": "pending"}
  ]
}
```

---

## Phase 1 — 并行执行

为每个切片调用 `Agent` tool，`run_in_background: true`：

```
Agent(
  description: "样本组 A — DESeq2 差异分析",
  prompt: "执行以下分析任务：
    输入：data/groupA/（包含 counts.csv 和 metadata.csv）
    任务：运行 DESeq2，输出差异表达结果到 results/groupA/deseq2_results.csv
    验证：检查输出文件存在且行数 > 100
    conda 环境：research-env
    详细步骤：...",
  subagent_type: "executor",
  run_in_background: true
)
```

**关键约定**：
- 每个 Worker 只操作自己负责的目录，避免文件冲突
- 共享的只读输入文件可以同时读取
- 每个 Worker 的输出写到独立子目录

**最大并发**：6 个 Worker（避免资源竞争）

---

## Phase 2 — 监控进度

等待后台 Agent 完成（用 `TaskList` / `TaskOutput` 查看状态）：

```
TaskList() → 查看所有后台任务状态
TaskOutput(taskId) → 读取某个 Worker 的输出
```

某个 Worker 失败：
1. 读取其错误信息（`TaskOutput`）
2. 诊断原因
3. 修复后重新派遣该切片（不影响其他正在运行的 Worker）

---

## Phase 3 — 汇总

所有 Worker 完成后：

**合并结果**：
```python
import pandas as pd, glob
all_results = pd.concat([pd.read_csv(f) for f in glob.glob("results/*/deseq2_results.csv")])
all_results.to_csv("results/combined_results.csv", index=False)
print(f"合并后总行数: {len(all_results)}")
```

**生成汇总统计**：
```python
summary = all_results.groupby('group').agg({
    'padj': lambda x: (x < 0.05).sum()
}).rename(columns={'padj': 'sig_DEG_count'})
print(summary)
```

---

## Phase 4 — Architect 验收

向 Architect agent 提交所有结果：

```
请验收并行分析结果：

任务：[原始目标]
Worker 完成情况：
  - W1 [描述]：[状态] — 输出 [文件] [关键指标]
  - W2 [描述]：[状态] — 输出 [文件] [关键指标]
汇总文件：[路径] [指标]

请给出 APPROVED 或 REJECTED，以及具体理由。
```

**APPROVED** → 输出最终报告。
**REJECTED** → 针对具体问题修复，重新提交。

---

## Phase 5 — 最终报告

清理本次会话文件：

```bash
rm -f .omiga/context/*${SESSION_ID}*.md
echo "Context snapshot removed — Team session complete."
```

```
## Team 执行报告

**任务**：[目标]
**Worker 数量**：N 个并行执行
**执行时间**：[估计]

**各 Worker 结果**：
| Worker | 切片 | 状态 | 关键指标 |
|--------|------|------|---------|
| W1 | ... | 完成 | ... |
| W2 | ... | 完成 | ... |

**汇总输出**：[路径和说明]
**关键发现**：[跨切片的核心结论]
**注意事项**：[差异、异常、待跟进]
```

---

## 本次任务

$ARGUMENTS
