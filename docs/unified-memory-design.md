# Unified Memory System Design

## 核心概念

**记忆（Memory）** = 所有持久化知识的统一抽象

### 记忆类型

```
Memory
├── Explicit Memory (显性记忆) = Wiki
│   └── 外部知识库文档（PDF、Markdown、TXT 等）
│   └── 用户显式导入和管理
│   └── 主观、精选、长期有效
│   └── 例子：技术文档、API 参考、设计规范
│
└── Implicit Memory (隐性记忆) = PageIndex
    └── 自动索引的聊天历史
    └── 动态记录用户与 AI 的对话
    └── 客观、可追溯、随对话增长
    └── 例子："之前讨论过的认证方案"
```

### 对比

| 维度 | Explicit (Wiki) | Implicit (PageIndex) |
|------|----------------|---------------------|
| **谁维护** | 用户显式导入 | 系统自动索引 |
| **数据来源** | 外部文档（PDF、MD、TXT） | 聊天记录 |
| **更新方式** | 手动导入/编辑 | 每次对话后自动更新 |
| **生命周期** | 长期（除非手动删除） | 随会话持续增长 |
| **结构** | 扁平页面列表 | 层次化树形结构（按会话） |
| **检索方式** | 关键词匹配 | TF-IDF 评分 |
| **存储位置** | `.omiga/memory/wiki/` | `.omiga/memory/implicit/` |

## 统一目录结构

```
.omiga/memory/              # 统一记忆根目录（可配置）
│
├── wiki/                   # 显性记忆
│   ├── index.md           # 知识目录
│   ├── log.md             # 操作日志
│   └── pages/             # 页面文件（可选子目录）
│       ├── user-prefs.md
│       └── architecture.md
│
├── implicit/              # 隐性记忆（原 PageIndex）
│   ├── tree.json          # 文档树索引
│   ├── cache.json         # 内容哈希缓存
│   └── content/           # 处理后内容存储
│
└── config.json            # 记忆系统配置
    ├── memory_dir: ".omiga/memory"
    ├── wiki_subdir: "wiki"
    ├── implicit_subdir: "implicit"
    ├── auto_index: true
    └── index_extensions: ["md", "rs", "py", ...]
```

## API 设计

### 统一接口

```rust
// 获取记忆上下文（自动合并两种类型）
memory::query_context(query: &str, limit: usize) -> Option<String>

// 分别查询
memory::explicit::query(query: &str) -> WikiResults
memory::implicit::query(query: &str) -> IndexResults

// 管理
memory::get_config() -> MemoryConfig
memory::set_config(config: MemoryConfig)
memory::get_stats() -> MemoryStats  // 包含两种类型的统计
```

### 配置结构

```rust
pub struct MemoryConfig {
    /// 记忆根目录（相对或绝对路径）
    pub root_dir: PathBuf,
    
    /// Wiki 子目录名
    pub wiki_dir: String,
    
    /// 隐式索引子目录名
    pub implicit_dir: String,
    
    /// 是否自动构建隐式索引
    pub auto_build_index: bool,
    
    /// 自动构建间隔（秒）
    pub auto_build_interval: u64,
    
    /// 要索引的文件扩展名
    pub index_extensions: Vec<String>,
    
    /// 排除的目录
    pub exclude_dirs: Vec<String>,
    
    /// 最大文件大小（字节）
    pub max_file_size: usize,
}
```

## 前端设计

### 统一设置界面

```
Settings → Memory（统一入口）
├── 概览
│   ├── 显性记忆：X 个页面
│   ├── 隐性记忆：Y 个文档，Z 个章节
│   └── 存储路径：.omiga/memory/
│
├── 显性记忆（Wiki）
│   ├── 页面列表
│   ├── 搜索 Wiki
│   └── 摄入新内容
│
├── 隐性记忆（索引）
│   ├── 索引状态
│   ├── 搜索索引
│   ├── 重建/更新
│   └── 索引设置
│
└── 高级设置
    ├── 修改记忆路径
    ├── 文件类型配置
    └── 排除目录
```

### 路径配置

支持两种路径模式：

1. **项目相对路径**（默认）
   - `.omiga/memory/`
   - 随项目移动

2. **绝对路径**（用户自定义）
   - `~/.omiga/projects/my-project/memory/`
   - 适合多工作树共享记忆
   - 适合隐私需求（不提交到 git）

## 迁移策略

### 从现有系统迁移

```rust
// 检测旧结构并自动迁移
fn migrate_legacy_memory(project_root: &Path) -> Result<()> {
    // 1. 检测 .omiga/wiki/ → 移动到 .omiga/memory/wiki/
    // 2. 检测 .omiga/memory/ (旧 pageindex) → 移动到 .omiga/memory/implicit/
    // 3. 创建新的 config.json
}
```

### 向后兼容

- 保留旧的 API 端点作为别名
- Wiki 命令仍然可用
-  gradual migration

## 实现计划

### Phase 1: 统一目录结构
1. 修改 `domain/memory/mod.rs` 定义统一结构
2. 添加配置管理
3. 实现自动迁移

### Phase 2: 可配置路径
1. 添加 `memory_get_config` / `memory_set_config` 命令
2. 支持绝对/相对路径
3. 路径验证和安全性检查

### Phase 3: 前端统一
1. 合并 WikiSettingsTab 和 MemorySettingsTab
2. 添加路径配置 UI
3. 显示统一的记忆统计

### Phase 4: 智能集成
1. 统一查询接口（自动合并两种记忆）
2. 智能去重和相关性排序
3. 记忆来源标注（来自 Wiki / 来自聊天记录）

## 代码结构

```
domain/
├── memory/
│   ├── mod.rs              # 统一入口，配置管理
│   ├── config.rs           # MemoryConfig 定义
│   ├── migration.rs        # 迁移逻辑
│   │
│   ├── explicit/           # 显性记忆（原 wiki）
│   │   ├── mod.rs
│   │   ├── storage.rs
│   │   └── query.rs
│   │
│   └── implicit/           # 隐性记忆（原 pageindex）
│       ├── mod.rs
│       ├── tree.rs
│       ├── parser.rs
│       ├── storage.rs
│       └── query.rs
```

## 关键决策

### Q: 是否保留 wiki 命令？
A: 保留，作为记忆系统的子命令别名
- `wiki_get_status` → `memory_explicit_get_status`
- 保持前端兼容性

### Q: 如何防止路径遍历攻击？
A: 路径验证规则：
- 禁止指向项目根目录之外的相对路径（如 `../memory`）
- 绝对路径必须位于用户主目录下（`~/*`）
- 禁止指向系统目录（`/etc`, `/usr`, `/System` 等）
- 禁止包含 `..` 组件的路径

### Q: 多项目共享记忆？
A: 通过配置实现：
```json
{
  "root_dir": "~/.omiga/shared-memory/project-a",
  "shared_with": ["/path/to/worktree-a", "/path/to/worktree-b"]
}
```

## 总结

统一 Memory 系统的核心价值：
1. **概念统一**：用户不需要理解 Wiki vs PageIndex 的区别
2. **灵活配置**：支持不同场景的路径需求
3. **平滑迁移**：自动检测和转换旧数据
4. **未来扩展**：易于添加新的记忆类型（如浏览器书签、笔记同步等）
