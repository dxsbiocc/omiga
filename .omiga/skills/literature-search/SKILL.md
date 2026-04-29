---
name: literature-search
description: 文献检索 — 搜索 PubMed、bioRxiv、arXiv 及用户显式启用的可选来源，整理相关论文并生成摘要。
triggers:
  - search papers
  - search literature
  - find papers
  - pubmed
  - semantic scholar
  - arxiv
  - biorxiv
  - literature review
  - 文献检索
  - 检索文献
  - 查文献
  - 搜索论文
---

# Literature Search

## 任务

搜索与目标主题相关的文献，整理关键结果，生成可读摘要。

## Steps

### Step 1 — 确定检索策略

根据用户目标确定：
- 核心关键词（英文优先，PubMed/arXiv 检索用）
- 时间范围：默认以“最近 5 年”为主，但必须按当前日期动态计算或使用“recent/latest/last 5 years”等相对表达，不得写死具体年份
- 历史覆盖：不能忽略更早的奠基文献、方法学文献、首次报道、高影响力论文或被近期文献反复引用的经典工作
- 数据库优先级：PubMed（临床/生物） > arXiv（计算/物理） > Crossref/OpenAlex（通用元数据/DOI） > bioRxiv/medRxiv（近期预印本）；Semantic Scholar 等需要 API key/用户显式开启的来源不能默认调用。

### Step 2 — 执行检索

使用内置 search / fetch：
```
search(category="literature", source="pubmed", query="<keywords>")
search(category="literature", source="arxiv", query="<keywords>")
search(category="literature", source="crossref", query="<keywords>")
search(category="literature", source="openalex", query="<keywords>")
search(category="literature", source="biorxiv", query="<keywords>")
search(category="literature", source="medrxiv", query="<keywords>")
search(category="web", source="auto", query="<keywords> recent review OR latest review")
search(category="web", source="auto", query="<keywords> seminal OR foundational OR landmark OR classic")
fetch(category="literature", source="pubmed", id="<PMID>")
```

### Step 3 — 整理结果

按相关性排序，提取每篇文献的：
- 标题、作者、年份、期刊/来源
- 核心发现（1-2 句）
- 与用户目标的相关性评分（High/Medium/Low）
- 文献类型：原始研究 / 综述 / 预印本 / Meta 分析 / 经典奠基文献

### Step 4 — 输出

```markdown
## 文献检索报告：{主题}

**检索时间**：{日期}
**检索词**：{keywords}
**共找到**：{N} 篇相关文献

### 核心文献（High 相关性）

1. **{标题}** — {作者} ({年份})
   - 来源：{期刊/arXiv}
   - 核心发现：{1-2句}
   - DOI/链接：{链接}

...

### 中等相关文献（Medium）

...

### 检索建议

- 扩展关键词：{建议}
- 相关综述文章：{建议}

## 参考文献

[1] {作者}. {标题}. {期刊/来源}. {年份}. DOI/URL: {链接}
[2] ...
```

要求：
- 正文综合结论中的引用必须是可点击、可 hover 的链接；优先 Markdown 超链接，也可使用安全 HTML 锚点 `<a href="https://...">标签</a>`。
- 如果使用 [1]、[2] 这类编号引用，编号本身也必须带链接，例如 `[[1]](https://doi.org/...)`，不能输出裸 `[1]`。
- 链接文本优先使用期刊/来源、作者年份、PMID/DOI 或论文标题，避免只显示裸 URL。
- 最后必须给出“参考文献”列表，且每条参考文献必须有可核查 DOI 或 URL。

## 本次任务

$ARGUMENTS
