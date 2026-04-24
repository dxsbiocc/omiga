---
name: interpret-results
description: 结果解读 — 解读统计分析输出（DESeq2、Seurat、GSEA 等），给出生物学意义说明。
triggers:
  - interpret results
  - analyze results
  - result interpretation
  - 解读结果
  - 结果解读
  - 分析结果
  - 解释结果
---

# Interpret Results

## 任务

读取分析输出文件，从统计数字中提炼生物学意义，识别关键发现和异常。

## Steps

### Step 1 — 读取结果文件

```bash
# 确认输出文件存在
ls -lh results/ figures/ 2>/dev/null

# 预览关键输出
head -20 results/*.csv 2>/dev/null
wc -l results/*.csv 2>/dev/null
```

### Step 2 — 统计层解读

根据分析类型：

**差异表达（DESeq2/edgeR）**：
```python
import pandas as pd
res = pd.read_csv("results/deseq2_results.csv")
sig = res[res['padj'] < 0.05]
print(f"总基因: {len(res)}, 显著DEG: {len(sig)}")
print(f"上调: {len(sig[sig['log2FoldChange']>1])}, 下调: {len(sig[sig['log2FoldChange']<-1])}")
print("Top 10 DEG:\n", sig.nsmallest(10,'padj')[['gene','log2FoldChange','padj']])
```

**单细胞（Seurat/Scanpy）**：
```python
import scanpy as sc
adata = sc.read_h5ad("results/processed.h5ad")
print(f"细胞: {adata.n_obs}, 基因: {adata.n_vars}")
print("细胞类型分布:\n", adata.obs['cell_type'].value_counts())
```

**通路富集（GSEA/clusterProfiler）**：
```r
res <- read.csv("results/gsea_results.csv")
cat("显著通路 (p.adjust<0.05):", sum(res$p.adjust<0.05), "\n")
print(head(res[order(res$p.adjust),c("Description","NES","p.adjust")], 10))
```

### Step 3 — 生物学意义

结合已知背景，解读：
- 主要发现是什么？（核心结论）
- 结果是否符合预期？意外发现是什么？
- 潜在的生物学机制是什么？
- 结果的局限性和需要进一步验证的点

### Step 4 — 输出报告

```markdown
## 结果解读：{分析类型}

### 统计摘要
- {关键数字}

### 主要发现
1. {发现1}
2. {发现2}

### 生物学意义
{解读}

### 异常/注意点
{如有}

### 建议后续分析
- {建议}
```

## 本次任务

$ARGUMENTS
