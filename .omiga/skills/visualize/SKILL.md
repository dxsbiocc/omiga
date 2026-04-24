---
name: visualize
description: 数据可视化 — 生成科研图表（火山图、热图、UMAP、箱线图等）。
triggers:
  - make plot
  - create plot
  - generate figure
  - visualize
  - 画图
  - 生成图表
  - 可视化
  - 出图
  - 绘图
---

# Visualize

## 任务

根据分析结果生成科研质量的图表，保存为 PDF/PNG，确认文件非空。

## Steps

### Step 1 — 确认数据和图表类型

```bash
ls -lh results/ 2>/dev/null
head -3 results/*.csv 2>/dev/null
```

常用图表类型：
- **火山图**：差异表达结果（log2FC vs -log10 padj）
- **热图**：基因表达矩阵
- **UMAP/tSNE**：单细胞聚类
- **箱线图/小提琴图**：组间比较
- **条形图**：富集分析
- **折线图**：时间序列

### Step 2 — 生成图表

**Python (matplotlib/seaborn)**：
```python
import matplotlib
matplotlib.use('Agg')  # 无 GUI 模式
import matplotlib.pyplot as plt
import seaborn as sns
import pandas as pd

# 读取数据
df = pd.read_csv("results/main_results.csv")

# 设置期刊风格
plt.rcParams.update({
    'font.size': 12,
    'figure.dpi': 300,
    'savefig.bbox': 'tight'
})

# 绘图
fig, ax = plt.subplots(figsize=(8, 6))
# ... 绘图代码 ...

plt.savefig("figures/figure1.pdf")
plt.savefig("figures/figure1.png", dpi=300)
plt.close()
print("Saved figures/figure1.pdf")
```

**R (ggplot2)**：
```r
library(ggplot2)
df <- read.csv("results/main_results.csv")

p <- ggplot(df, aes(...)) +
  geom_point() +
  theme_classic() +
  labs(title = "...", x = "...", y = "...")

ggsave("figures/figure1.pdf", p, width=8, height=6, dpi=300)
ggsave("figures/figure1.png", p, width=8, height=6, dpi=300)
cat("Saved figures/figure1.pdf\n")
```

### Step 3 — 验证输出

```bash
ls -lh figures/*.pdf figures/*.png 2>/dev/null
# 确认文件非零大小
find figures/ -name "*.pdf" -size 0 -o -name "*.png" -size 0 2>/dev/null | grep . && echo "WARNING: empty files found" || echo "All figures OK"
```

### Step 4 — 报告

列出生成的文件及每张图的简短说明：
```
生成的图表：
- figures/figure1.pdf — 火山图，显示 N 个 DEG（红色：padj<0.05, |log2FC|>1）
- figures/figure2.pdf — 热图，Top 50 DEG
```

## 本次任务

$ARGUMENTS
