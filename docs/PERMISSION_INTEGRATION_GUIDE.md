# 权限系统集成指南

> 如何在 Omiga 中使用新的权限管理系统

---

## 1. 系统概览

权限系统已集成到 Omiga，包含以下组件：

```
┌─────────────────────────────────────────────────────────────┐
│                     权限系统组件                             │
├─────────────────────────────────────────────────────────────┤
│  Backend (Rust)                                             │
│  ├── PermissionManager - 核心管理器                        │
│  ├── DangerousPatternDB - 危险命令检测                     │
│  └── 命令: permission_check, permission_approve...         │
├─────────────────────────────────────────────────────────────┤
│  Frontend (React)                                           │
│  ├── PermissionDialog - 确认对话框                         │
│  ├── PermissionSettings - 设置面板                         │
│  └── permissionStore - Zustand 状态管理                    │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 在工具调用中集成

### 方式一：直接在 Chat 组件中检查

```tsx
// src/components/Chat/index.tsx

import { usePermissionStore } from "../../state/permissionStore";

export const Chat: React.FC = () => {
  const { checkPermission, pendingRequest } = usePermissionStore();
  
  const handleToolExecution = async (toolName: string, args: any) => {
    // 1. 检查权限
    const allowed = await checkPermission(sessionId, toolName, args);
    
    if (!allowed) {
      // 权限对话框会自动显示
      // 等待用户决策...
      return;
    }
    
    // 2. 执行工具
    await executeTool(toolName, args);
  };
};
```

### 方式二：在工具层统一拦截

```rust
// src-tauri/src/domain/tools/mod.rs

pub async fn execute_tool_with_permission(
    tool_name: &str,
    args: &Value,
    session_id: &str,
    state: &OmigaAppState,
) -> Result<Value, String> {
    // 1. 检查权限
    let decision = state.permission_manager
        .check_tool(session_id, tool_name, args)
        .await;
    
    match decision {
        PermissionDecision::Allow => {
            // 执行工具
            execute_tool(tool_name, args).await
        }
        PermissionDecision::Deny(reason) => {
            Err(format!("权限被拒绝: {}", reason))
        }
        PermissionDecision::RequireApproval(request) => {
            // 返回需要确认的信号
            Err("NEED_PERMISSION_APPROVAL".to_string())
        }
    }
}
```

---

## 3. 前端使用

### 显示权限对话框

PermissionDialog 已自动集成到 App.tsx，当 pendingRequest 存在时会自动显示：

```tsx
// App.tsx 中已包含
<PermissionDialog />
```

### 在设置中管理权限

```tsx
// src/components/Settings/index.tsx

import { PermissionSettings } from "../permissions";

// 在设置标签页中添加
<Tab label="权限">
  <PermissionSettings />
</Tab>
```

或使用现有的 PermissionSettingsTab。

---

## 4. 已实现的危险命令检测

权限系统会自动检测以下危险命令：

| 命令模式 | 风险等级 | 说明 |
|---------|---------|------|
| `rm -rf /` | Critical | 删除根目录 |
| `:(){ :|:& };:` | Critical | Fork bomb |
| `> /dev/sda` | Critical | 覆盖磁盘 |
| `chmod -R 777 /` | High | 修改根目录权限 |
| `curl \| sh` | Medium | 管道执行远程脚本 |
| `sudo` | Medium | 提权操作 |

---

## 5. 测试权限系统

### 测试命令

```bash
# 1. 启动 Omiga
cd omiga && bun run dev

# 2. 在 Chat 中发送测试消息
"帮我执行 rm -rf /"

# 3. 应该弹出权限对话框，显示严重风险警告
```

### 单元测试

```bash
cd omiga/src-tauri
cargo test permission_manager::tests
```

---

## 6. 自定义权限规则

### 通过前端添加规则

```typescript
import { usePermissionStore } from "./state/permissionStore";

const { addRule } = usePermissionStore();

// 添加自动批准规则
await addRule({
  name: "自动批准文件读取",
  tool_matcher: { type: "Exact", pattern: "file_read" },
  mode: "auto",
  priority: 100,
});
```

### 预设配置

系统提供三种预设：

1. **开发模式** - 项目内操作自动批准
2. **安全模式** - 所有操作询问
3. **CI/CD 模式** - Plan 模式批量确认

---

## 7. 故障排查

### 权限对话框不弹出

```
1. 检查 App.tsx 是否包含 <PermissionDialog />
2. 检查 permissionStore.pendingRequest 是否有值
3. 检查浏览器控制台是否有错误
```

### 危险命令未被拦截

```
1. 检查 DangerousPatternDB 是否正确加载
2. 检查命令格式是否匹配正则
3. 查看后端日志
```

---

## 8. 文件清单

### 已创建的文件

```
src-tauri/src/domain/permissions/
├── mod.rs          # 模块导出
├── types.rs        # 类型定义
├── patterns.rs     # 危险模式数据库
└── manager.rs      # 核心管理器 ✅

src-tauri/src/commands/
└── permissions.rs  # Tauri 命令 ✅

src/state/
└── permissionStore.ts  # 前端状态 ✅

src/components/permissions/
├── index.ts               # 导出
├── PermissionDialog.tsx   # 对话框 ✅
└── PermissionSettings.tsx # 设置面板 ✅
```

### 修改的文件

```
src-tauri/src/
├── lib.rs          # 注册命令
├── app_state.rs    # 添加 PermissionManager
└── domain/mod.rs   # 导出 permissions

src/
└── App.tsx         # 添加 PermissionDialog
```

---

## 下一步

1. **集成到工具调用流程** - 在 execute_tool 前添加权限检查
2. **添加更多危险模式** - 根据实际需求扩展
3. **规则持久化** - 将规则保存到 SQLite
4. **审计日志完善** - 记录所有权限决策

---

*最后更新: 2026-04-09*
