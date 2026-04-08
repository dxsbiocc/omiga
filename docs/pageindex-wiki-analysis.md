# PageIndex 与 Wiki 系统分析

## 1. 系统概述

### Wiki 系统（现有）
```
.omiga/wiki/
├── index.md      # 内容目录（手动维护）
├── log.md        # 操作日志（追加式）
└── <slug>.md     # 独立页面
```

**核心特性：**
- **手动管理**：用户/AI 显式创建和维护页面
- **轻量级**：简单的文件读写操作
- **透明注入**：每次 LLM 调用前自动将相关 wiki 内容注入系统提示
- **关键词匹配**：简单的关键词搜索，无复杂评分

### PageIndex 系统（新实现）
```
.omiga/memory/
├── tree.json     # 统一文档树（自动构建）
├── cache.json    # 内容哈希缓存
└── content/      # 处理后内容存储
```

**核心特性：**
- **自动索引**：扫描项目中的所有文件（Markdown、代码等）
- **层次化结构**：Folder → File → Section 的树形结构
- **增量更新**：基于哈希的缓存，跳过未更改文件
- **智能检索**：TF-IDF-like 评分 + 上下文扩展
- **多格式支持**：Markdown、代码文件、配置文件等

---

## 2. 详细差异对比

| 维度 | Wiki | PageIndex |
|------|------|-----------|
| **数据范围** | 手动创建的页面 | 项目中所有支持的文件 |
| **更新方式** | 显式写入 | 自动扫描 + 增量更新 |
| **数据结构** | 扁平（页面列表） | 层次化（树形） |
| **检索算法** | 简单关键词匹配 | TF-IDF-like 评分 + 上下文 |
| **存储位置** | `.omiga/wiki/` | `.omiga/memory/` |
| **持久化格式** | 原始 Markdown | JSON 树 + 缓存 |
| **使用场景** | 用户知识库、决策记录 | 代码理解、文档检索 |
| **生命周期** | 长期维护 | 随代码同步更新 |

---

## 3. 兼容性分析

### 3.1 数据兼容性

**文件格式冲突：**
- Wiki 使用原始 Markdown 文件
- PageIndex 使用 JSON 序列化 + 单独内容存储
- ✅ **无冲突**：可以共存于不同目录

**路径处理：**
- Wiki：相对路径转换为 slug
- PageIndex：保留完整相对路径
- ⚠️ **注意**：Wiki 页面可能被 PageIndex 同时索引（如果放在项目内）

### 3.2 功能兼容性

**检索功能：**
```rust
// Wiki: 简单的关键词匹配
pub async fn query_relevant_context(user_message: &str, project_root: &Path) -> Option<String>

// PageIndex: 复杂的评分系统  
pub async fn query(&self, query: &str, limit: usize) -> Result<Vec<QueryResult>, AppError>
```

**上下文注入：**
- Wiki：透明注入到系统提示（transparent hook）
- PageIndex：可以作为工具使用，或类似的透明注入
- ✅ **兼容**：两者可以并行注入

### 3.3 API 兼容性

**Wiki 命令：**
- `wiki_write_page` / `wiki_read_page`
- `wiki_write_index` / `wiki_read_index`
- `wiki_append_log` / `wiki_read_log`
- `wiki_query`

**PageIndex 命令（需要添加）：**
- `pageindex_build` - 构建/重建索引
- `pageindex_query` - 查询相关内容
- `pageindex_get_stats` - 获取统计信息

---

## 4. 使用场景对比

### Wiki 适合的场景

1. **用户显式知识管理**
   ```
   user: "记住我喜欢用 bun 而不是 npm"
   → 写入 wiki/user-preferences.md
   ```

2. **项目决策记录**
   ```
   user: "我们决定不用 Redux"
   → 写入 wiki/architecture-decisions.md
   ```

3. **长期参考信息**
   - API 密钥位置
   - 部署流程
   - 团队成员联系方式

### PageIndex 适合的场景

1. **代码理解**
   ```
   user: "这个项目的认证是怎么实现的？"
   → PageIndex 查询 auth 相关文件和函数
   ```

2. **文档检索**
   ```
   user: "README 中关于配置的部分"
   → PageIndex 定位到 README.md 的 Configuration 章节
   ```

3. **跨文件关联**
   - 查找某个函数在哪些文件中被调用
   - 理解模块间的依赖关系

---

## 5. 合并策略分析

### 策略 1：完全独立（推荐）

**架构：**
```
.omiga/
├── wiki/           # 用户知识库（手动管理）
└── memory/         # 项目索引（自动管理）
```

**优点：**
- ✅ 清晰的职责分离
- ✅ 用户可以直接编辑 wiki 文件
- ✅ PageIndex 可以索引 wiki 目录（可选）
- ✅ 互不干扰，独立演进

**缺点：**
- ⚠️ 两个系统的查询需要分别调用

### 策略 2：统一查询层

**架构：**
```rust
pub struct UnifiedMemory {
    wiki: WikiSystem,
    pageindex: PageIndex,
}

impl UnifiedMemory {
    pub async fn query(&self, query: &str) -> UnifiedResults {
        // 并行查询两个系统
        let wiki_results = self.wiki.query(query).await;
        let pageindex_results = self.pageindex.query(query).await;
        // 合并、排序、去重
        self.merge_results(wiki_results, pageindex_results)
    }
}
```

**优点：**
- ✅ 统一的查询接口
- ✅ 可以同时获取两种类型的结果

**缺点：**
- ⚠️ 需要额外的合并逻辑
- ⚠️ 评分体系不同，难以公平比较

### 策略 3：Wiki 基于 PageIndex 构建

**架构：**
- 移除 wiki 的独立存储
- 将 wiki 页面作为 PageIndex 的特殊文档类型

**优点：**
- ✅ 统一的存储和检索
- ✅ 维基页面可以被全文索引

**缺点：**
- ❌ 失去手动编辑的便利性（需要重建索引）
- ❌ 破坏了 wiki 的简单性
- ❌ 日志追加模式与 PageIndex 的更新模式冲突

---

## 6. 推荐方案

### 短期：独立运行 + 选择性集成

```rust
// domain/memory/mod.rs - 统一的内存模块入口

pub mod pageindex;
pub mod wiki;

/// Unified context retrieval for LLM
pub async fn get_relevant_context(
    project_root: &Path,
    query: &str,
) -> Option<String> {
    // Try wiki first (faster, more curated)
    if let Some(ctx) = wiki::query_relevant_context(query, project_root).await {
        return Some(ctx);
    }
    
    // Fall back to pageindex for code/docs
    let pageindex = PageIndex::new(project_root, IndexConfig::default());
    if let Ok(results) = pageindex.query(query, 3).await {
        if !results.is_empty() {
            return Some(format_results(results));
        }
    }
    
    None
}
```

### 长期：分层记忆系统

```
┌─────────────────────────────────────────────────────────────┐
│                    Unified Memory Layer                     │
│  (coordinates between different memory systems)            │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐    ┌─────────────────┐    ┌──────────────┐
│     Wiki     │    │   PageIndex     │    │  Conversation│
│  (Explicit)  │    │   (Implicit)    │    │   (Session)  │
├──────────────┤    ├─────────────────┤    ├──────────────┤
• User prefs   │    • Source code     │    • Chat history│
• Decisions    │    • Documentation   │    • This turn   │
• References   │    • Config files    │    • Files read  │
└──────────────┘    └─────────────────┘    └──────────────┘
```

---

## 7. 实现建议

### 立即实施（保持独立）

1. **完成 PageIndex 实现**
   - 添加 Tauri 命令暴露
   - 测试增量更新
   - 优化查询性能

2. **明确使用场景**
   - Wiki：用于用户显式要求"记住"的内容
   - PageIndex：用于代码和文档的自动索引

3. **选择性集成**
   - 在 chat 系统中优先使用 wiki
   - 当 wiki 无结果时，使用 PageIndex 作为补充

### 后续考虑（统一查询层）

1. 创建 `UnifiedMemory` 协调层
2. 实现结果合并和去重逻辑
3. 添加配置选项控制各系统的权重

---

## 8. 总结

| 问题 | 结论 |
|------|------|
| 是否兼容？ | ✅ 完全兼容，可以共存 |
| 是否合并？ | ❌ 不建议完全合并，保持独立更好 |
| 如何集成？ | 统一查询层作为可选的协调机制 |
| 用户感知？ | 对用户透明，自动选择最佳来源 |

**核心原则：**
- Wiki = 用户的笔记本（显式、精选）
- PageIndex = 项目的搜索引擎（自动、全面）
- 两者互补，而非替代
