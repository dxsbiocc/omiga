# Operator System — User Validation Findings

Live record of bugs / friction discovered during real dogfooding.
Each item must be reproducible. Any future codex dispatch must be
tied to a finding here.

---

## #1 — Operator 目录覆盖太薄，导致 agent 大量回退到 bash

- **Section / Step**: 实际使用，非清单某节
- **Severity**: 🟡 **major** — 产品定位问题，不是 bug
- **Observed**: 用户向 chat agent 问 *"用 seqtk 对 B_PP7 的 merge 文件进行统计分析，如 reads 数量等"*。Agent 整轮 18 步全部走 `bash`：`seqtk stats`、`seqtk size`、`seqtk fqchk`、`wc -l` 等。**未调用任何 `operator__seqtk_*` 工具**。
- **Expected mental model**: 用户已安装 `operator-seqtk` plugin，期望 agent 走"注册过的 operator"路径。
- **Root cause**: 检查 `~/.omiga/operators/registry.json` 发现：
  - `seqtk_sample_reads` **已 enabled，schema 也确实注入到 LLM tool list**（`commands/chat/mod.rs:3218` `enabled_operator_tool_schemas()`）
  - 但它只覆盖 **subsample** 一个用例
  - **没有 `seqtk_stats` / `seqtk_size` / `fastq_count` / `fastq_qc` 等覆盖"统计/质控"语义的 operator**
  - LLM 判断 `seqtk_sample_reads`(subsample) 不匹配 *统计* 任务 → 落回 bash。**LLM 决策正确。**
- **Reproduce**:
  1. 在 chat 里输入任何与"seqtk 统计 / count / qc"相关的需求
  2. 查看 ReAct 步骤
  3. 所有 seqtk 调用都是 `bash` 工具，无 `operator__*`

### 关联追问 / 设计抉择（不立即修，需 PM 拍板）

| 问题 | 选项 |
|------|------|
| 是不是应该补一个 `fastq_stats` operator？ | A) 加 `seqtk_size` `seqtk_fqchk` `count_reads` 等若干原子 operator <br> B) 加一个 wrapper `fastq_quality_check` 把多种 seqtk 子命令组合 |
| Agent 应不应该提示用户"该操作没有匹配的 operator，用了 bash"？ | UI 上加 hint，让用户感受到 operator 系统的边界 |
| 这条路是不是说明 operator 系统的 ROI 主要是"长任务 + SLURM 编排"，而不是"日常 shell 包装"？ | 如果是，那 favorites/chain 编辑器/DAG 的优先级要再评估 |

### 不推荐立即扩 operator 库的理由

- 一个 `seqtk` 工具有 ~10 个子命令，每个写一个 operator = 10 个 manifest 维护成本
- bash 路径已经在用户场景里跑通了（截图里 agent 自己回退、自己用 wc 统计、自己整合成表）
- 真正的价值不在"包装每个子命令"，而在"长流程、可复现、HPC 化"。先验证那条路是否真的被需要。

### 建议下一步

不修这条 finding。改为：**找一个真正在用 SLURM/HPC 的场景跑通一次 chain**，验证产品 ROI 的真实形状。再决定要不要补 operator 目录。

---

## #2 — (template for future findings)

- **Section / Step**:
- **Severity**:
- **Expected**:
- **Observed**:
- **Reproduce**:
