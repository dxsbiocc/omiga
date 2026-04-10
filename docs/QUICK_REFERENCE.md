# Omiga 增强快速参考

> 日常开发速查表

---

## 📋 Phase 任务清单

### Phase 1: IDE Bridge (4-6周)

```
进度: [░░░░░░░░░░] 0%

任务:
□ 1. Bridge Server 核心 (WebSocket + JWT)
  □ src-tauri/src/bridge/mod.rs
  □ src-tauri/src/bridge/server.rs
  □ src-tauri/src/bridge/auth.rs

□ 2. VS Code 扩展
  □ 基础扩展框架
  □ 选中代码发送
  □ Diff 显示
  □ 权限确认

□ 3. Omiga 集成
  □ Bridge 状态 UI
  □ IDE 连接管理
  □ 上下文接收

预计完成: 2026-05-15
```

### Phase 2: 权限系统 (2-3周)

```
进度: [░░░░░░░░░░] 0%

任务:
□ 1. 核心权限管理
  □ PermissionManager
  □ PermissionMode 枚举
  □ 规则存储

□ 2. 安全检测
  □ 危险命令模式
  □ 路径遍历防护
  □ 敏感文件保护

□ 3. UI 组件
  □ PermissionDialog
  □ 规则编辑器
  □ 拒绝历史

预计完成: 2026-05-01
```

### Phase 3: Gateway (3-4周)

```
进度: [░░░░░░░░░░] 0%

任务:
□ 1. Telegram
  □ Bot API 集成
  □ 消息处理
  □ 流式响应

□ 2. Slack
  □ Slack API
  □ 事件处理

□ 3. 路由系统
  □ 会话映射
  □ 用户绑定

预计完成: 2026-05-30
```

---

## 🔧 常用命令

### 开发命令

```bash
# 启动开发环境
cd omiga && bun run dev

# 运行测试
cd omiga/src-tauri && cargo test

# 检查 Rust 代码
cd omiga/src-tauri && cargo clippy

# 构建发布版本
cd omiga && bun run build
```

### 依赖管理

```bash
# 添加 Rust 依赖
cd omiga/src-tauri && cargo add tokio-tungstenite

# 添加前端依赖
cd omiga && bun add @tauri-apps/api
```

---

## 📁 文件结构速查

### 新增目录规划

```
src-tauri/src/
├── bridge/              # IDE Bridge (Claude Code)
│   ├── mod.rs
│   ├── server.rs
│   ├── auth.rs
│   ├── handlers.rs
│   └── commands.rs      # Tauri 命令
│
├── gateway/             # 消息平台 (Hermes)
│   ├── mod.rs
│   ├── manager.rs
│   ├── telegram.rs
│   └── slack.rs
│
├── domain/
│   ├── permissions/     # 权限系统 (Claude Code)
│   │   ├── mod.rs
│   │   ├── manager.rs
│   │   ├── rules.rs
│   │   └── types.rs
│   │
│   ├── execution/       # 多后端 (Hermes)
│   │   ├── mod.rs
│   │   ├── docker.rs
│   │   └── ssh.rs
│   │
│   └── context_compressor.rs  # 上下文压缩
│
└── commands/            # Tauri 命令扩展
    ├── bridge.rs
    ├── gateway.rs
    └── permissions.rs
```

### 前端目录规划

```
src/
├── components/
│   ├── bridge/
│   │   ├── BridgeStatus.tsx
│   │   ├── IdePermissionDialog.tsx
│   │   └── ConnectedIdesPanel.tsx
│   │
│   ├── gateway/
│   │   ├── GatewayStatus.tsx
│   │   ├── TelegramSetup.tsx
│   │   └── PlatformSettings.tsx
│   │
│   └── permissions/
│       ├── PermissionDialog.tsx
│       ├── PermissionRulesEditor.tsx
│       └── RecentDenials.tsx
│
└── state/
    ├── bridgeStore.ts
    └── permissionStore.ts
```

---

## 🔌 关键依赖

### Bridge 依赖

```toml
# Cargo.toml
[dependencies]
tokio-tungstenite = "0.24"    # WebSocket
jsonwebtoken = "9"             # JWT
```

### Gateway 依赖

```toml
[dependencies]
teloxide = "0.13"              # Telegram
slack-morphism = "2"           # Slack
```

### 执行后端依赖

```toml
[dependencies]
bollard = "0.18"               # Docker
async-ssh2-tokio = "0.23"      # SSH
```

---

## 📝 代码模板

### 1. 创建新的 Tauri 命令

```rust
// src-tauri/src/commands/my_feature.rs

#[tauri::command]
pub async fn my_command(
    arg: String,
    state: tauri::State<'_, OmigaAppState>,
) -> Result<MyResult, String> {
    // 实现
    Ok(result)
}

// 在 lib.rs 中注册
.invoke_handler(tauri::generate_handler![
    // ... existing commands
    commands::my_feature::my_command,
])
```

### 2. 创建新的 Zustand Store

```typescript
// src/state/myStore.ts

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

interface MyState {
  data: string[];
  loading: boolean;
  fetchData: () => Promise<void>;
}

export const useMyStore = create<MyState>((set) => ({
  data: [],
  loading: false,
  fetchData: async () => {
    set({ loading: true });
    const data = await invoke<string[]>('my_command');
    set({ data, loading: false });
  },
}));
```

### 3. 创建新的 MUI 组件

```tsx
// src/components/MyComponent.tsx

import React from 'react';
import { Box, Typography, Paper } from '@mui/material';

interface MyComponentProps {
  title: string;
}

export const MyComponent: React.FC<MyComponentProps> = ({ title }) => {
  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="h6">{title}</Typography>
    </Paper>
  );
};
```

---

## 🔍 调试技巧

### Rust 后端调试

```rust
// 添加日志
tracing::info!("Processing request: {:?}", request);
tracing::debug!("Detailed debug: {:?}", data);
tracing::error!("Error occurred: {}", e);

// 运行时查看日志
RUST_LOG=debug cargo run
```

### 前端调试

```typescript
// 添加日志
console.log('[Bridge] Message received:', message);
console.debug('[Debug] State:', state);

// React DevTools
// 使用 Zustand DevTools 中间件
```

### Tauri 调试

```bash
# 打开 DevTools
cargo tauri dev -- --debug

# 查看 WebView 日志
cargo tauri dev 2>&1 | grep -i error
```

---

## 🐛 常见问题

### Bridge 连接失败

```
问题: WebSocket 连接被拒绝
解决:
1. 检查防火墙设置
2. 确认端口未被占用
3. 验证 JWT secret 正确
```

### 权限规则不生效

```
问题: 规则匹配失败
解决:
1. 检查 tool_pattern 语法
2. 确认规则未过期
3. 查看规则存储路径
```

### Gateway 消息丢失

```
问题: Telegram 消息未收到
解决:
1. 检查 Bot Token 有效
2. 确认 Webhook 设置正确
3. 查看网关日志
```

---

## 📚 参考资源

### Claude Code 参考

| 文件 | 功能 | 优先级 |
|------|------|--------|
| `src/bridge/*.ts` | Bridge 系统 | P0 |
| `src/hooks/toolPermission/*.ts` | 权限处理 | P0 |
| `src/components/permissions/*.tsx` | 权限 UI | P0 |
| `src/services/compact/*.ts` | 上下文压缩 | P2 |

### Hermes 参考

| 文件 | 功能 | 优先级 |
|------|------|--------|
| `tools/environments/*.py` | 多后端执行 | P1 |
| `gateway/telegram.py` | Telegram 适配器 | P1 |
| `tools/registry.py` | 工具注册 | P2 |
| `context_compressor.py` | 上下文压缩 | P2 |

---

## 🎯 验收标准

### Bridge 系统

- [ ] VS Code 可以发送选中代码到 Omiga
- [ ] Omiga 的修改可以在 VS Code 中显示 Diff
- [ ] IDE 中可以确认 Omiga 的权限请求
- [ ] 支持同时连接多个 IDE

### 权限系统

- [ ] 支持 5 种权限模式切换
- [ ] rm -rf / 等危险命令被拦截
- [ ] 权限规则可以按路径/工具名配置
- [ ] 拒绝历史可查看

### Gateway

- [ ] Telegram Bot 可以接收消息
- [ ] Omiga 的回复可以发送到 Telegram
- [ ] 支持流式响应
- [ ] 会话正确路由

---

## 📞 升级求助

当需要查看参考代码时：

```bash
# Claude Code 源码位置
/Users/dengxsh/Downloads/Work/Agent/claude-code-main/src/

# Hermes Agent 源码位置
/Users/dengxsh/Downloads/Work/Agent/hermes-agent/

# 当前项目位置
/Users/dengxsh/Downloads/Work/Agent/claude-code-main/omiga/
```

---

*最后更新: 2026-04-07*
