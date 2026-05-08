---
name: ralph
description: >
  持久执行循环 — 接管任务，循环直到分析完成且结果经过验证。
  适用于数据分析、流水线执行、需要"跑完为止"的科研任务。
when_to_use: >
  当任务需要持续执行直到完成时使用：运行 Snakemake/Nextflow 流水线、
  执行统计分析并验证结果、用户说"不要停"/"ralph"/"keep going"/"持续执行"。
tags:
  - orchestration
  - persistence
  - autonomous
  - long-running
  - research
---

# Ralph — 持久执行循环

## 角色

你是执行负责人。接手任务后，循环执行直到结果正确、经过 Architect 验收，
或遇到无法自行解决的阻塞才暂停报告。

不要问"要继续吗"。继续。

## 执行策略

- 独立的子步骤用 `run_in_background: true` 并行执行（安装、编译、长时运行）
- 优先找到并修复根因，不要绕过错误
- 每次迭代结束前用 Architect agent 做结果验收
- 同一错误出现 3 次 → 停下来报告，附完整错误信息和已尝试的方案

## 状态持久化

每次进入新步骤时更新状态文件，支持崩溃后恢复：

```bash
# 状态文件路径：.omiga/state/ralph-{SESSION_ID}.json
# SESSION_ID = 当前对话/任务的唯一标识（用时间戳或 uuidgen 生成）
```

状态 JSON 结构：

```json
{
  "version": 1,
  "session_id": "ralph-20260417-143022",
  "goal": "用户原始目标",
  "phase": "executing",
  "iteration": 2,
  "consecutive_errors": 0,
  "project_root": "/path/to/project",
  "env": "conda:rnaseq",
  "todos_completed": ["env check", "data validation"],
  "todos_pending": ["run DESeq2", "generate figures"],
  "last_error": null,
  "started_at": "2026-04-17T14:30:22Z",
  "updated_at": "2026-04-17T14:45:10Z"
}
```

---

## Step 0 — 任务接收与规划

### 0a. 检查已有状态（支持恢复）

```bash
mkdir -p .omiga/state
ls .omiga/state/ralph-*.json 2>/dev/null && cat .omiga/state/ralph-*.json 2>/dev/null || echo "NO_PRIOR_STATE"
```

**如果发现已有状态文件**，显示摘要并询问：
```
发现上次未完成的 Ralph 任务：
  目标：[goal]
  阶段：[phase]（第 [iteration] 轮）
  已完成：[todos_completed]
  待完成：[todos_pending]

从上次中断处继续执行。如需重新开始，请告知。
```

然后直接跳到对应步骤（`phase` 字段指示从哪里继续）。

**如果没有状态文件**，生成新的 SESSION_ID 并继续：

```bash
SESSION_ID="ralph-$(date +%Y%m%d-%H%M%S)"
echo $SESSION_ID
```

### 0b. 写入上下文快照

在 `.omiga/context/` 下创建一份 Markdown 快照，记录本次任务的完整上下文。
快照名称：`{slug}-{SESSION_ID}.md`，slug 由目标文本生成（小写，非字母数字转 `-`，最多 40 字符）。

```bash
mkdir -p .omiga/context
# 生成 slug
GOAL="$ARGUMENTS_OR_GOAL_TEXT"
SLUG=$(echo "$GOAL" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | sed 's/-\+/-/g' | cut -c1-40 | sed 's/^-//;s/-$//')
SNAPSHOT_FILE=".omiga/context/${SLUG}-${SESSION_ID}.md"

python3 - << PYEOF
import subprocess, datetime, os, sys

goal      = os.environ.get('GOAL', 'unknown goal')
session   = os.environ.get('SESSION_ID', 'ralph-unknown')
snap_file = os.environ.get('SNAPSHOT_FILE', '.omiga/context/unknown.md')

def run(cmd):
    try:
        return subprocess.check_output(cmd, shell=True, stderr=subprocess.DEVNULL, text=True).strip()
    except Exception:
        return '(unavailable)'

lines = [
    f"# Context Snapshot: {goal[:80]}",
    "",
    f"**Session**: {session}",
    f"**Mode**: ralph",
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
    f"- **R**: {run('Rscript --version 2>&1 | head -1')}",
    f"- **Conda env**: {run('echo ${CONDA_DEFAULT_ENV:-none}')}",
    f"- **OS**: {run('uname -sr')}",
    "",
    "## Key Paths",
    "",
    f"- **CWD**: {os.getcwd()}",
    f"- **Data**: {run('ls -d data/ raw/ inputs/ 2>/dev/null | tr chr(10) , ')}",
    "",
    "## Constraints",
    "",
    "(fill in during planning)",
    "",
    "## Project State",
    "",
    "```",
    run("git log --oneline -5 2>/dev/null || echo 'not a git repo'"),
    "",
    run("ls -la 2>/dev/null | head -20"),
    "```",
]

os.makedirs(os.path.dirname(snap_file), exist_ok=True)
with open(snap_file, 'w') as f:
    f.write('\n'.join(lines) + '\n')
print(f"Context snapshot written: {snap_file}")
PYEOF
```

> 快照写完后在终端打印路径即可，无需展示给用户。
> 恢复执行时（0a 发现已有状态），读取对应快照以重建上下文：
> ```bash
> cat .omiga/context/*${SESSION_ID}*.md 2>/dev/null || echo "No snapshot for this session"
> ```

### 0c. 理解目标，调用 todo_write

立即调用 `todo_write` 列出所有步骤：
- 第一项：`in_progress`（当前步骤）
- 其余：`pending`

记录关键上下文：
- 项目路径 / 数据目录
- 执行环境（conda env 名称、R version、本地/SSH/HPC）
- 预期输出（文件路径、图表、统计结果）

### 0d. 写入初始状态

```bash
PROJECT_ROOT=$(pwd)
cat > .omiga/state/${SESSION_ID}.json << 'EOF'
{
  "version": 1,
  "session_id": "PLACEHOLDER_SESSION_ID",
  "goal": "PLACEHOLDER_GOAL",
  "phase": "env_check",
  "iteration": 1,
  "consecutive_errors": 0,
  "project_root": "PLACEHOLDER_ROOT",
  "todos_completed": [],
  "todos_pending": [],
  "last_error": null,
  "started_at": "PLACEHOLDER_TIME",
  "updated_at": "PLACEHOLDER_TIME"
}
EOF
```

实际执行时将占位符替换为真实值，用 `python3 -c` 或 `jq` 生成正确 JSON：

```bash
python3 -c "
import json, datetime, os, sys
state = {
  'version': 1,
  'session_id': os.environ.get('SESSION_ID', 'ralph-unknown'),
  'goal': sys.argv[1] if len(sys.argv) > 1 else 'unknown',
  'phase': 'env_check',
  'iteration': 1,
  'consecutive_errors': 0,
  'project_root': os.getcwd(),
  'todos_completed': [],
  'todos_pending': [],
  'last_error': None,
  'started_at': datetime.datetime.utcnow().isoformat() + 'Z',
  'updated_at': datetime.datetime.utcnow().isoformat() + 'Z',
}
os.makedirs('.omiga/state', exist_ok=True)
with open(f'.omiga/state/{state[\"session_id\"]}.json', 'w') as f:
    json.dump(state, f, indent=2)
print('State written:', state['session_id'])
" "GOAL_TEXT"
```

---

## Step 1 — 环境检查

更新状态 phase → `env_check`，然后检查环境：

```bash
# 更新状态
python3 -c "
import json, datetime
with open('.omiga/state/\$SESSION_ID.json') as f: s = json.load(f)
s['phase'] = 'env_check'; s['updated_at'] = datetime.datetime.utcnow().isoformat() + 'Z'
with open('.omiga/state/\$SESSION_ID.json', 'w') as f: json.dump(s, f, indent=2)
"

# Python 环境
conda activate <env> && python -c "import scanpy, pandas, scipy; print('OK')"
# R 环境
Rscript -e "suppressMessages(library(DESeq2)); cat('DESeq2 OK\n')"
# 数据文件
ls -lh data/ && head -3 data/samples.csv
```

缺失依赖 → 自动安装：
```bash
pip install <pkg>           # Python
conda install -c bioconda <pkg>
Rscript -e "install.packages('<pkg>', repos='https://cloud.r-project.org')"
```

---

## Step 2 — 执行分析

更新 phase → `executing`，逐步执行：

```bash
# 更新状态（每次迭代开始时）
python3 -c "
import json, datetime
with open('.omiga/state/\$SESSION_ID.json') as f: s = json.load(f)
s['phase'] = 'executing'; s['updated_at'] = datetime.datetime.utcnow().isoformat() + 'Z'
with open('.omiga/state/\$SESSION_ID.json', 'w') as f: json.dump(s, f, indent=2)
"
```

**Python 脚本**：
```bash
python scripts/analysis.py --input data/ --output results/ 2>&1 | tee logs/run.log
```

**R 分析**：
```bash
Rscript scripts/deseq2.R 2>&1 | tee logs/deseq2.log
```

**Snakemake 流水线**：
```bash
snakemake --cores 8 --use-conda -n   # dry-run
snakemake --cores 8 --use-conda
```

**Nextflow**：
```bash
nextflow run main.nf -profile conda --outdir results/
```

**流水线监控**：
- 解析输出，识别失败的 rule/process
- 读取对应日志文件（`.snakemake/log/`、`work/xx/yy/.command.log`）
- 精准修复失败节点，重跑该步骤而非整体重跑
- 更新 todo 状态反映当前进度

遇到错误时，调用错误去重助手，它会自动判断是否应停止：

```bash
# 将 "ERROR_SUMMARY" 替换为捕获的错误摘要（首行或关键行）
DEDUP=$(python3 .omiga/scripts/ralph_record_error.py \
  ".omiga/state/${SESSION_ID}.json" \
  "ERROR_SUMMARY")
echo "$DEDUP"

# 如果返回 STOP，立即停止并报告
if echo "$DEDUP" | grep -q "^STOP"; then
  echo "=== RALPH STUCK: same error 3 times, halting ==="
  echo "Error: ERROR_SUMMARY"
  echo "Please resolve this manually and restart the task."
  exit 0   # 不抛异常，让用户看到完整输出
fi
```

错误修复成功后清零：
```bash
python3 -c "
import json, datetime
with open('.omiga/state/${SESSION_ID}.json') as f: s = json.load(f)
s['consecutive_errors'] = 0; s['last_error'] = None
s['updated_at'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
with open('.omiga/state/${SESSION_ID}.json', 'w') as f: json.dump(s, f, indent=2)
"
```

执行后立刻验证：
```bash
ls -lh results/          # 文件是否生成
wc -l results/*.csv      # 行数是否合理
head -5 results/key.csv  # 内容是否正确
```

---

## Step 3 — 结果质量检查

更新 phase → `quality_check`：

```bash
python3 -c "
import json, datetime
with open('.omiga/state/\$SESSION_ID.json') as f: s = json.load(f)
s['phase'] = 'quality_check'; s['updated_at'] = datetime.datetime.utcnow().isoformat() + 'Z'
with open('.omiga/state/\$SESSION_ID.json', 'w') as f: json.dump(s, f, indent=2)
"
```

根据分析类型检查：

**差异表达分析**：
```r
res <- read.csv("results/deseq2_results.csv")
cat("总基因数:", nrow(res), "\n")
cat("显著DEG (padj<0.05):", sum(res$padj < 0.05, na.rm=TRUE), "\n")
```

**单细胞分析**：
```python
import scanpy as sc
adata = sc.read_h5ad("results/processed.h5ad")
print(f"细胞数: {adata.n_obs}, 基因数: {adata.n_vars}")
```

**可视化**：
```bash
ls -lh figures/*.pdf figures/*.png 2>/dev/null
```

异常信号（触发重新分析）：差异基因数 = 0、大量 NA 值、图片为 0 字节。

---

## Step 4 — Architect 验收

更新 phase → `verifying`：

```bash
python3 -c "
import json, datetime
with open('.omiga/state/\$SESSION_ID.json') as f: s = json.load(f)
s['phase'] = 'verifying'; s['updated_at'] = datetime.datetime.utcnow().isoformat() + 'Z'
with open('.omiga/state/\$SESSION_ID.json', 'w') as f: json.dump(s, f, indent=2)
"
```

提交给 Architect agent：

```
请验收以下分析：
目标：[用户原始目标]
完成步骤：[已完成的 todo 列表]
输出文件：
  - [路径]: [内容描述]
关键指标：[核心数字]
请给出 APPROVED 或 REJECTED 以及具体理由。
```

**APPROVED** → 更新 todo 为 completed，继续 Step 5。
**REJECTED** → 读取具体问题，回到 Step 2 修复，同时增加 iteration 计数：

```bash
python3 -c "
import json, datetime
with open('.omiga/state/\$SESSION_ID.json') as f: s = json.load(f)
s['iteration'] = s['iteration'] + 1
s['updated_at'] = datetime.datetime.utcnow().isoformat() + 'Z'
with open('.omiga/state/\$SESSION_ID.json', 'w') as f: json.dump(s, f, indent=2)
"
```

---

## Step 5 — 循环或完成

还有未完成的 todo → 回到 Step 2。

全部完成且 APPROVED → 删除状态文件和快照，输出最终报告：

```bash
# 清理状态文件和上下文快照（任务完成）
rm -f .omiga/state/${SESSION_ID}.json
rm -f .omiga/context/*${SESSION_ID}*.md
echo "Session files removed — Ralph session complete."
```

输出最终报告：

```
## 完成报告

**任务**：[目标]
**执行轮次**：[N 次迭代]
**输出文件**：
  - [路径]: [说明]
**关键结果**：[核心数据/发现]
**注意事项**：[限制、待确认项]
```

---

## 阻塞条件（需要用户介入）

停止并报告当：
- 同一错误出现 3 次，已穷尽常规修复方案
- 需要外部凭证（API key、HPC 账号）
- 分析结果高度异常且无法诊断根因
- 磁盘满 / 内存 OOM 且无法释放
- 数据文件损坏或格式完全不符合预期

**阻塞时保留状态文件**（不删除），以便用户解决问题后能恢复执行。

---

## 本次任务

$ARGUMENTS
