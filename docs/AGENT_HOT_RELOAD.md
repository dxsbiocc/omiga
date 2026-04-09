# Agent 热重载系统

Omiga 支持从 `.omiga/agents/*.md` 文件动态加载 Agent 定义，无需重启应用即可更新 Agent。

---

## 功能特性

- ✅ **自动加载**：启动时自动加载所有 Agent 定义
- ✅ **热重载**：文件修改后自动更新 Agent
- ✅ **实时卸载**：删除文件后自动卸载 Agent
- ✅ **多级目录**：支持用户级和项目级 Agent
- ✅ **错误处理**：解析失败时发送事件通知

---

## Agent 文件格式

创建 `.omiga/agents/my-agent.md`：

```markdown
---
name: my-custom-agent
description: 用于代码审查的自定义 Agent
model: haiku
tools:
  - file_read
  - file_edit
  - ripgrep
disallowed_tools:
  - bash
permission_mode: acceptEdits
background: false
omit_claude_md: true
color: blue
max_turns: 10
---

# 代码审查 Agent

你是一个专业的代码审查助手。

## 职责
1. 检查代码风格和最佳实践
2. 发现潜在的 bug 和安全问题
3. 提供改进建议

## 输出格式
- 问题等级：严重/警告/建议
- 具体位置：文件名和行号
- 修复建议：具体的代码示例
```

### Frontmatter 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | ✅ | Agent 唯一标识 |
| `description` | string | ✅ | 使用场景描述 |
| `model` | string | ❌ | 模型别名或 ID |
| `tools` | string[] | ❌ | 允许的工具列表 |
| `disallowed_tools` | string[] | ❌ | 禁止的工具列表 |
| `permission_mode` | string | ❌ | 权限模式 |
| `background` | boolean | ❌ | 是否后台运行 |
| `omit_claude_md` | boolean | ❌ | 是否省略 CLAUDE.md |
| `color` | string | ❌ | UI 颜色标识 |
| `max_turns` | number | ❌ | 最大轮数限制 |

### 权限模式

- `default` - 默认权限（需要用户确认）
- `acceptEdits` - 自动接受文件编辑
- `plan` - 计划模式
- `bypassPermissions` - 绕过权限检查（谨慎使用）

---

## 目录结构

```
~/.omiga/agents/          # 用户级 Agent（全局可用）
  ├── code-review.md
  ├── doc-writer.md
  └── test-generator.md

./.omiga/agents/          # 项目级 Agent（仅当前项目）
  ├── project-specific.md
  └── custom-workflow.md
```

**优先级**：项目级 > 用户级 > 内置 Agent

同名 Agent 会被后加载的覆盖。

---

## 使用方法

### 1. 启动热重载

```rust
use crate::domain::agents::hot_reload::start_agent_hot_reload;
use crate::domain::agents::get_agent_router;
use std::sync::Arc;
use tokio::sync::RwLock;

// 在应用启动时
async fn init_agent_system(app: &AppHandle) -> Result<(), String> {
    let router = get_agent_router();
    let project_root = "/path/to/project";
    
    // 启动热重载
    let (manager, watcher, mut event_rx) = 
        start_agent_hot_reload(
            Arc::new(RwLock::new(router)),
            Path::new(project_root)
        ).await?;
    
    // 在后台处理事件
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                HotReloadEvent::AgentLoaded { agent_type, source } => {
                    println!("Agent 已加载: {} (来自 {:?})", agent_type, source);
                }
                HotReloadEvent::AgentUnloaded { agent_type } => {
                    println!("Agent 已卸载: {}", agent_type);
                }
                HotReloadEvent::LoadFailed { path, error } => {
                    eprintln!("加载失败 {:?}: {}", path, error);
                }
            }
        }
    });
    
    // 保持 watcher 存活
    app.manage(watcher);
    
    Ok(())
}
```

### 2. 使用动态 Agent

```rust
// Agent 加载后像内置 Agent 一样使用
Agent({
    "description": "代码审查",
    "prompt": "审查 src/main.rs 的代码质量",
    "subagent_type": "my-custom-agent"  // 使用自定义 Agent
})
```

### 3. 手动触发重载

```rust
use crate::domain::agents::hot_reload::AgentHotReloadManager;

// 重新加载特定 Agent
manager.reload_agent("/path/to/agent.md", &router).await?;

// 重新加载整个目录
manager.reload_all_in_dir("/path/to/agents/", &router).await?;
```

---

## 前端集成

### 显示动态 Agent 列表

```typescript
import { useAgentStore } from "./state/agentStore";

function AgentList() {
  const { dynamicAgents } = useAgentStore();
  
  return (
    <div>
      <h3>自定义 Agent</h3>
      {dynamicAgents.map(agent => (
        <div key={agent.agent_type}>
          <span style={{ color: agent.color }}>
            {agent.agent_type}
          </span>
          <span>{agent.when_to_use}</span>
        </div>
      ))}
    </div>
  );
}
```

### 监听热重载事件

```typescript
import { listen } from "@tauri-apps/api/event";

useEffect(() => {
  const unlisten = listen("agent-hot-reload", (event) => {
    const { agent_type, action } = event.payload;
    
    switch (action) {
      case "loaded":
        toast.success(`Agent ${agent_type} 已加载`);
        break;
      case "unloaded":
        toast.info(`Agent ${agent_type} 已卸载`);
        break;
      case "failed":
        toast.error(`Agent ${agent_type} 加载失败`);
        break;
    }
  });
  
  return () => unlisten();
}, []);
```

---

## 示例 Agent

### 代码审查 Agent

```markdown
---
name: code-reviewer
description: 专业的代码审查助手，检查代码质量和最佳实践
model: haiku
tools:
  - file_read
  - ripgrep
  - glob
disallowed_tools:
  - file_edit
  - file_write
  - bash
permission_mode: default
omit_claude_md: true
color: purple
---

# 代码审查助手

你是一个经验丰富的代码审查专家。

## 审查维度
1. **代码风格** - 命名规范、格式化
2. **最佳实践** - 设计模式、代码组织
3. **性能** - 算法复杂度、资源使用
4. **安全** - 常见漏洞、注入风险
5. **可维护性** - 复杂度、注释、文档

## 输出格式
```
## 总体评价
[优秀/良好/需改进]

## 发现的问题

### 🔴 严重
- 位置: `文件名:行号`
- 问题: 描述
- 建议: 修复方案

### 🟡 警告
...

### 🟢 建议
...

## 正面反馈
- 代码亮点...
```
```

### API 文档生成 Agent

```markdown
---
name: api-doc-generator
description: 从代码注释生成 API 文档
model: sonnet
tools:
  - file_read
  - file_write
  - glob
permission_mode: acceptEdits
omit_claude_md: true
color: green
---

# API 文档生成器

分析代码中的注释和类型定义，生成 Markdown 格式的 API 文档。

## 工作流程
1. 扫描项目中的源代码文件
2. 解析函数/类/接口的注释和签名
3. 生成结构化的 API 文档
4. 输出到 docs/api/ 目录

## 输出格式
- 模块索引
- 接口定义
- 函数说明
- 使用示例
```

---

## 故障排除

### Agent 未加载

**检查清单**：
1. 文件扩展名是否为 `.md`
2. frontmatter 是否以 `---` 开头和结束
3. `name` 和 `description` 是否填写
4. 文件编码是否为 UTF-8

### 热重载不生效

**可能原因**：
1. 目录不在监控列表中
2. 文件权限问题
3. 磁盘事件被节流

**解决方案**：
```rust
// 手动触发重载
manager.reload_all_in_dir("/path/to/agents", &router).await?;
```

### 解析错误

常见错误：
- `缺少 YAML frontmatter` - 文件必须以 `---` 开头
- `解析 frontmatter 失败` - YAML 语法错误
- `Agent 名称不能为空` - name 字段必填

---

## 高级用法

### 条件加载

```rust
// 只在特定条件下加载 Agent
if feature_enabled("advanced-agents") {
    manager.add_watch_dir("/path/to/advanced/agents");
}
```

### 插件系统集成

```rust
// 从插件目录加载 Agent
for plugin_dir in get_plugin_dirs() {
    let agents_dir = plugin_dir.join("agents");
    if agents_dir.exists() {
        manager.add_watch_dir(agents_dir);
    }
}
```

### 版本控制集成

```rust
// Git hook 触发重载
// .git/hooks/post-checkout
// .git/hooks/post-merge

// 在应用内监听分支切换
tokio::spawn(async move {
    while let Ok(branch) = branch_rx.recv().await {
        // 重新加载项目级 Agent
        manager.reload_all_in_dir(&project_agents_dir, &router).await;
    }
});
```

---

## 与 TS 版本的对比

| 特性 | Claude Code (TS) | Omiga (Rust) |
|------|------------------|--------------|
| 文件格式 | YAML + Markdown | YAML + Markdown ✅ |
| 热重载 | ✅ 支持 | ✅ 支持 |
| 目录 | `.claude/agents/` | `.omiga/agents/` ✅ |
| 用户级 | `~/.claude/agents/` | `~/.omiga/agents/` ✅ |
| 项目级 | `.claude/agents/` | `.omiga/agents/` ✅ |
| 优先级 | 后加载覆盖 | 后加载覆盖 ✅ |
| 错误提示 | 日志 | 事件 + 日志 ✅ |

---

## 迁移指南

### 从 Claude Code 迁移 Agent

1. **复制文件**
   ```bash
   cp ~/.claude/agents/*.md ~/.omiga/agents/
   # 或
   cp .claude/agents/*.md .omiga/agents/
   ```

2. **更新字段名**（如果需要）
   - `agentType` → `name`
   - `whenToUse` → `description`
   - `disallowedTools` → `disallowed_tools`
   - `permissionMode` → `permission_mode`

3. **测试验证**
   - 启动 Omiga
   - 查看 Agent 是否正确加载
   - 测试 Agent 功能

---

## API 参考

### `AgentHotReloadManager`

```rust
impl AgentHotReloadManager {
    /// 创建新的管理器
    pub fn new() -> (Self, mpsc::Receiver<HotReloadEvent>)
    
    /// 添加监控目录
    pub fn add_watch_dir(&mut self, dir: PathBuf)
    
    /// 启动文件监控
    pub async fn start_watching(
        &self,
        router: Arc<RwLock<AgentRouter>>,
    ) -> Result<RecommendedWatcher, String>
    
    /// 获取所有动态 Agent
    pub async fn get_dynamic_agents(&self) -> Vec<DynamicAgent>
}
```

### `HotReloadEvent`

```rust
pub enum HotReloadEvent {
    AgentLoaded { agent_type: String, source: PathBuf },
    AgentUnloaded { agent_type: String },
    LoadFailed { path: PathBuf, error: String },
}
```

### 便捷函数

```rust
/// 启动热重载系统
pub async fn start_agent_hot_reload(
    router: Arc<RwLock<AgentRouter>>,
    project_root: &Path,
) -> Result<(AgentHotReloadManager, RecommendedWatcher, mpsc::Receiver<HotReloadEvent>), String>
```

---

**状态**: 已实现 ✅  
**代码位置**: `domain/agents/hot_reload.rs`
