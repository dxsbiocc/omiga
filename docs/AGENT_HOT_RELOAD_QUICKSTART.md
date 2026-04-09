# Agent 热重载 - 快速开始

## 1. 创建你的第一个自定义 Agent

```bash
mkdir -p .omiga/agents
cat > .omiga/agents/my-agent.md << 'EOF'
---
name: my-agent
description: 我的自定义 Agent
model: haiku
tools:
  - file_read
  - ripgrep
---

# 我的 Agent

这是自定义 Agent 的系统提示词。
EOF
```

## 2. 启动热重载

```rust
use crate::domain::agents::hot_reload::start_agent_hot_reload;

// 在应用启动时
let (manager, watcher, event_rx) = start_agent_hot_reload(
    Arc::new(RwLock::new(router)),
    Path::new(".")
).await?;

// 保持 watcher 存活
app.manage(watcher);
```

## 3. 使用自定义 Agent

```
Agent({
    "description": "测试",
    "prompt": "Hello",
    "subagent_type": "my-agent"
})
```

## 4. 修改 Agent

编辑 `.omiga/agents/my-agent.md`，保存后自动生效！

## 文件格式

```markdown
---
name: agent-name              # 必填：Agent 标识
description: 描述             # 必填：使用场景
model: haiku                  # 可选：模型
 tools:                       # 可选：允许的工具
  - file_read
  - ripgrep
disallowed_tools:             # 可选：禁止的工具
  - bash
permission_mode: default      # 可选：权限模式
background: false             # 可选：是否后台运行
omit_claude_md: true          # 可选：省略 CLAUDE.md
color: blue                   # 可选：UI 颜色
max_turns: 10                 # 可选：最大轮数
---

# 系统提示词

Agent 的行为定义...
```

## 目录优先级

```
~/.omiga/agents/      # 用户级（全局）
./.omiga/agents/      # 项目级（覆盖用户级）
```

后加载的 Agent 会覆盖先加载的同名 Agent。

## 热重载事件

```rust
while let Some(event) = event_rx.recv().await {
    match event {
        HotReloadEvent::AgentLoaded { agent_type, source } => {
            println!("✅ 已加载: {}", agent_type);
        }
        HotReloadEvent::AgentUnloaded { agent_type } => {
            println!("🗑️ 已卸载: {}", agent_type);
        }
        HotReloadEvent::LoadFailed { path, error } => {
            eprintln!("❌ 加载失败: {} - {}", path.display(), error);
        }
    }
}
```

## 完整示例

见 `docs/AGENT_HOT_RELOAD.md`
