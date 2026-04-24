---
name: literature-search
description: 文献检索 — 搜索 PubMed、bioRxiv、arXiv，整理相关论文并生成摘要。
triggers:
  - search papers
  - search literature
  - find papers
  - pubmed
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
- 时间范围（默认近 5 年）
- 数据库优先级：PubMed（临床/生物） > bioRxiv（生物预印本） > arXiv（计算/物理）

### Step 2 — 执行检索

使用 MCP 工具（若可用）：
```
mcp__claude_ai_PubMed__search_articles(query, max_results=20)
mcp__claude_ai_bioRxiv__search_preprints(query, date_range)
mcp__claude_ai_Tavily__tavily_search(query, topic="general")
```

若 MCP 不可用，使用 WebSearch：
```
WebSearch("site:pubmed.ncbi.nlm.nih.gov <keywords>")
WebSearch("<keywords> filetype:pdf arxiv")
```

### Step 3 — 整理结果

按相关性排序，提取每篇文献的：
- 标题、作者、年份、期刊/来源
- 核心发现（1-2 句）
- 与用户目标的相关性评分（High/Medium/Low）

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
```

## 本次任务

$ARGUMENTS
