---
name: write-paper
description: 论文写作 — 根据分析结果起草论文章节（Results、Methods、Discussion）。
triggers:
  - write paper
  - write manuscript
  - draft results
  - draft discussion
  - 写论文
  - 撰写论文
  - 写结果
  - 写讨论
  - 论文草稿
---

# Write Paper

## 任务

根据已完成的分析结果，起草论文指定章节，遵循目标期刊格式要求。

## Steps

### Step 1 — 收集素材

```bash
# 确认有哪些结果可用
ls -lh results/ figures/ 2>/dev/null
head -5 results/*.csv 2>/dev/null
```

询问（如未在 $ARGUMENTS 中说明）：
- 目标章节：Results / Methods / Discussion / Abstract
- 目标期刊（影响写作风格和格式）
- 字数限制

### Step 2 — 读取关键数据

```python
import pandas as pd
# 读取核心结果用于引用具体数字
res = pd.read_csv("results/main_results.csv")
```

### Step 3 — 起草章节

**Results 章节结构**：
1. 实验设计概述（1句）
2. 主要发现（数字精确，使用被动语态）
3. 辅助结果（次要发现）
4. 图表引用（Figure X shows...）

**Methods 章节结构**：
1. 数据来源和样本信息
2. 软件/工具版本
3. 参数设置（可复现）
4. 统计检验方法

**Discussion 章节结构**：
1. 主要发现重申（1段）
2. 与已有文献对比
3. 机制解释
4. 局限性
5. 结论和展望

### Step 4 — 格式化输出

- 精确引用统计数字（不得编造，只用 Step 2 读取的实际数据）
- 图表用占位符：`(Figure 1A)`, `(Supplementary Table S1)`
- 参考文献用占位符：`[CITE: author year]`

## 本次任务

$ARGUMENTS
