# Omiga 增强计划：融合 Hermes + Claude Code

> 文档版本: 1.0  
> 创建时间: 2026-04-07  
> 目标: 通过借鉴 Hermes Agent 和 Claude Code 的优秀设计，将 Omiga 打造成全栈 AI 编程助手

---

## 📊 三方能力对比总览

| 维度 | Hermes Agent | Claude Code | Omiga (当前) |
|------|--------------|-------------|--------------|
| **定位** | 自进化多平台AI Agent | 专业开发者CLI工具 | IDE-like AI编程助手 |
| **形态** | CLI + 多消息平台 | 终端CLI | 桌面应用 (Tauri) |
| **后端语言** | Python 3.11+ | TypeScript (Bun) | Rust + TS |
| **架构** | Python单体 + Gateway | TypeScript单体 | Rust后端 + React前端 |
| **规模** | ~340K行 | ~512K行 | ~55K行 |

### 能力雷达对比

```
Omiga 能力雷达 (当前 vs 目标)

                    IDE集成
                      ▲
                     /|\
                    / | \
                   /  |  \
    消息平台 ◄────/───┼───\────► 多后端执行
                  /    |    \
                 /     |     \
                /      |      \
       Agent调度 ──────┼────── 记忆系统
                \      |      /
                 \     |     /
                  \    |    /
                   \   |   /
                    \  |  /
                     \ | /
                      \|/
                       ▼
                    工具生态

● = 当前 Omiga 强项
○ = 需要补充的能力

当前强项:
  ● Agent调度系统 (selector/planner/orchestrator)
  ● 统一记忆系统 (Wiki + PageIndex)
  ● MCP连接池管理
  ● 多Provider LLM支持

需要补充:
  ○ IDE深度集成 (Bridge系统)
  ○ 消息平台接入 (Gateway)
  ○ 精细权限系统
  ○ 多后端执行环境
  ○ 上下文压缩策略
```

---

## 🎯 核心增强方向

### 方向一：IDE 集成 (来自 Claude Code)

**目标**: 让 Omiga 成为 VS Code / JetBrains 的完美伴侣

**Claude Code 借鉴点**:
- `src/bridge/` - Bridge 系统完整实现
- `src/hooks/toolPermission/` - 权限处理
- `src/components/permissions/` - 权限对话框

**具体实现**:

```rust
// src-tauri/src/bridge/mod.rs
//! IDE Bridge 系统 — 适配自 Claude Code

pub struct BridgeServer {
    port: u16,
    jwt_secret: String,
    ide_connections: Arc<RwLock<HashMap<String, IdeConnection>>>,
}

pub struct IdeConnection {
    client_type: IdeType,  // VSCode | JetBrains
    socket: WebSocket,
    permissions_tx: mpsc::Sender<PermissionRequest>,
}
```

**关键功能**:
1. 选中代码发送到 Omiga
2. 在 IDE 中显示 Diff
3. 权限确认弹窗
4. 文件变更同步

---

### 方向二：权限系统升级 (来自 Claude Code)

**目标**: 生产级精细权限控制

**Claude Code 权限模式**:
- `default` - 标准权限确认
- `plan` - Plan 模式（批准确认）
- `bypassPermissions` - 绕过权限（危险）
- `auto` - 自动模式
- `coordinator` - 协调器模式

**实现规划**:

```rust
// src-tauri/src/domain/permissions/mod.rs
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PermissionMode {
    AskEveryTime,  // 每次询问
    Session,       // 会话级确认
    Plan,          // Plan 模式
    Auto,          // 自动批准安全操作
    Bypass,        // 完全绕过
}

pub struct PermissionManager {
    rules: Vec<PermissionRule>,
    recent_denials: Vec<DenialRecord>,
}
```

**安全特性**:
- 危险命令检测 (rm -rf /, fork bomb 等)
- 路径遍历防护
- 敏感文件保护 (/etc, /boot, .env)

---

### 方向三：消息平台 Gateway (来自 Hermes)

**目标**: 15+ 消息平台接入 (Telegram, Discord, Slack, 等)

**Hermes 借鉴点**:
- `gateway/` - 35万行 Gateway 代码
- `gateway/telegram.py` - Telegram 适配器
- `gateway/slack.py` - Slack 适配器

**实现规划**:

```rust
// src-tauri/src/gateway/mod.rs
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn platform_name(&self) -> &str;
    async fn start(&self, handler: MessageHandler) -> Result<()>;
    async fn send_message(&self, chat_id: &str, content: &str) -> Result<()>;
}

pub struct GatewayManager {
    adapters: HashMap<String, Box<dyn PlatformAdapter>>,
    session_router: SessionRouter,
}
```

**Phase 1 平台**:
1. Telegram (最高优先级)
2. Slack
3. Discord

---

### 方向四：多后端执行 (来自 Hermes)

**目标**: 支持 Docker / SSH / 云端执行

**Hermes 6种后端**:
- `local` - 本地执行
- `docker` - Docker 容器
- `modal` - Serverless
- `daytona` - 云端开发环境
- `ssh` - 远程服务器
- `singularity` - HPC集群

**实现规划**:

```rust
// src-tauri/src/domain/execution/mod.rs
#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, command: &str, cwd: &Path) -> Result<ExecutionResult>;
    async fn read_file(&self, path: &Path) -> Result<String>;
    async fn write_file(&self, path: &Path, content: &str) -> Result<()>;
}

pub struct DockerBackend { /* ... */ }
pub struct SshBackend { /* ... */ }
```

---

### 方向五：工具注册中心 (来自 Hermes)

**目标**: 动态工具发现和注册

**Hermes 借鉴点**:
- `tools/registry.py` - 工具注册中心
- `toolsets.py` - 工具集系统

**实现规划**:

```rust
// src-tauri/src/domain/tools/registry.rs
pub struct ToolRegistry {
    tools: HashMap<String, ToolRegistration>,
    toolsets: HashMap<String, Vec<String>>,
}

impl ToolRegistry {
    pub fn register(&mut self, reg: ToolRegistration);
    pub fn discover_available_tools(&self) -> Vec<&ToolRegistration>;
    pub fn get_toolset(&self, name: &str) -> Vec<&ToolRegistration>;
}

// 全局注册表
pub static REGISTRY: Lazy<Mutex<ToolRegistry>> = Lazy::new(|| { /* ... */ });
```

---

### 方向六：上下文压缩 (Claude Code + Hermes)

**目标**: 智能 Token 管理，降低 API 成本

**策略**:
1. 先剪枝旧工具输出（低成本）
2. 保护头部消息（系统提示+首轮对话）
3. 保护尾部消息（最近对话）
4. LLM 生成结构化摘要

**实现规划**:

```rust
// src-tauri/src/domain/context_compressor.rs
pub struct ContextCompressor {
    llm_client: Arc<dyn LlmClient>,
}

impl ContextCompressor {
    pub async fn compress(&self, messages: &[Message], target_tokens: usize) -> CompressionResult;
    async fn summarize_messages(&self, messages: &[Message]) -> Result<String>;
    fn prune_old_tool_outputs(&self, messages: &[Message]) -> Vec<Message>;
}
```

---

## 📅 实施路线图

### Phase 1: IDE 集成 (4-6周) 🔥 高优先级

**目标**: 实现 VS Code / JetBrains Bridge

**任务清单**:
- [ ] 实现 BridgeServer 核心 (WebSocket + JWT)
- [ ] 开发 VS Code 扩展
  - [ ] 选中代码发送到 Omiga
  - [ ] 在 IDE 中显示 Diff
  - [ ] 权限确认弹窗
- [ ] 在 Omiga UI 中添加 "Connect IDE" 按钮
- [ ] 开发 JetBrains 插件 (可选)

**参考代码**:
- Claude Code: `src/bridge/*.ts`
- 需要改造为 Tauri 兼容

**验收标准**:
- [ ] VS Code 可以发送选中代码到 Omiga
- [ ] Omiga 的修改可以在 VS Code 中显示 Diff
- [ ] IDE 中可以确认 Omiga 的权限请求

---

### Phase 2: 权限系统升级 (2-3周) 🔥 高优先级

**目标**: 生产级精细权限控制

**任务清单**:
- [ ] 实现 PermissionManager
- [ ] 添加 PermissionMode 枚举 (5种模式)
- [ ] 危险命令检测规则
- [ ] UI 权限对话框组件
- [ ] 权限规则持久化
- [ ] recent_denials 功能

**参考代码**:
- Claude Code: `src/hooks/toolPermission/`
- Claude Code: `src/components/permissions/`

**验收标准**:
- [ ] 支持 5 种权限模式切换
- [ ] rm -rf / 等危险命令被拦截
- [ ] 权限规则可以按路径/工具名配置

---

### Phase 3: 消息平台 (3-4周) 中优先级

**目标**: Telegram + Slack 接入

**任务清单**:
- [ ] 实现 GatewayManager
- [ ] Telegram 适配器
  - [ ] Bot API 集成
  - [ ] 消息接收处理
  - [ ] 流式响应发送
- [ ] Slack 适配器
- [ ] 会话路由系统
- [ ] 平台用户绑定

**参考代码**:
- Hermes: `gateway/telegram.py` (翻译为 Rust)
- Hermes: `gateway/slack.py`

**验收标准**:
- [ ] Telegram Bot 可以接收消息并转发到 Omiga
- [ ] Omiga 的回复可以发送到 Telegram
- [ ] 支持流式响应 (打字机效果)

---

### Phase 4: 多后端执行 (3-4周) 中优先级

**目标**: Docker + SSH 后端

**任务清单**:
- [ ] ExecutionBackend trait 设计
- [ ] Docker 后端实现
  - [ ] 容器生命周期管理
  - [ ] 命令执行
  - [ ] 文件读写
- [ ] SSH 后端实现
  - [ ] 连接管理
  - [ ] SFTP 文件传输
- [ ] 后端切换 UI

**参考代码**:
- Hermes: `tools/environments/*.py`

**验收标准**:
- [ ] 可以在 Docker 容器中执行命令
- [ ] 可以通过 SSH 连接远程服务器
- [ ] 后端切换不影响会话状态

---

### Phase 5: 工具注册中心 (2-3周) 中优先级

**目标**: 动态工具发现和注册

**任务清单**:
- [ ] ToolRegistry 全局注册表
- [ ] 工具集系统 (toolsets)
- [ ] 动态可用性检查 (check_fn)
- [ ] 工具发现 API
- [ ] 插件加载机制

**参考代码**:
- Hermes: `tools/registry.py`
- Hermes: `toolsets.py`

**验收标准**:
- [ ] 工具可以在运行时注册
- [ ] 支持工具集分组
- [ ] 工具可以根据环境变量动态可用/不可用

---

### Phase 6: 上下文压缩 (1-2周) 低优先级

**目标**: 智能 Token 管理

**任务清单**:
- [ ] ContextCompressor 实现
- [ ] 消息剪枝策略
- [ ] LLM 摘要生成
- [ ] 压缩阈值配置
- [ ] 与现有会话系统集成

**参考代码**:
- Hermes: `agent/context_compressor.py`
- Claude Code: `src/services/compact/`

**验收标准**:
- [ ] 长会话可以自动压缩
- [ ] 关键信息不丢失
- [ ] Token 使用量降低 30%+

---

### Phase 7: 更多平台 (4-6周) 长期规划

**目标**: 完整 Gateway 生态

**任务清单**:
- [ ] Discord 适配器
- [ ] WhatsApp 适配器
- [ ] Signal 适配器
- [ ] Email 适配器
- [ ] Webhook 适配器

**参考代码**:
- Hermes: `gateway/` 完整目录

---

## 🛠️ 技术实现细节

### 目录结构规划

```
src-tauri/src/
├── bridge/              # 新增: IDE Bridge (Claude Code)
│   ├── mod.rs
│   ├── server.rs
│   ├── auth.rs          # JWT 认证
│   └── handlers.rs
│
├── gateway/             # 新增: 消息平台 Gateway (Hermes)
│   ├── mod.rs
│   ├── manager.rs
│   ├── telegram.rs
│   └── slack.rs
│
├── domain/
│   ├── permissions/     # 新增: 权限系统 (Claude Code)
│   │   ├── mod.rs
│   │   ├── manager.rs
│   │   └── rules.rs
│   │
│   ├── execution/       # 新增: 多后端执行 (Hermes)
│   │   ├── mod.rs
│   │   ├── docker.rs
│   │   └── ssh.rs
│   │
│   ├── tools/
│   │   ├── registry.rs  # 新增: 工具注册中心 (Hermes)
│   │   └── toolsets.rs
│   │
│   └── context_compressor.rs  # 新增: 上下文压缩
│
└── lib.rs               # 修改: 添加模块声明
```

---

### 关键依赖添加

```toml
# src-tauri/Cargo.toml

[dependencies]
# Bridge 依赖
tokio-tungstenite = "0.24"        # WebSocket
jsonwebtoken = "9"                 # JWT

# Gateway 依赖
teloxide = "0.13"                  # Telegram
slack-morphism = "2"               # Slack

# 执行后端依赖
bollard = "0.18"                   # Docker API
async-ssh2-tokio = "0.23"          # SSH

# 其他
once_cell = "1"                    # 全局注册表
```

---

### 前端组件规划

```typescript
// src/components/Bridge/
// IDE 集成相关组件

- BridgeStatus.tsx          # Bridge 连接状态
- IdePermissionDialog.tsx   # IDE 权限确认对话框
- ConnectedIdesPanel.tsx    # 已连接的 IDE 列表

// src/components/Gateway/
// 消息平台相关组件

- GatewayStatus.tsx         # Gateway 状态面板
- PlatformSettings.tsx      # 平台配置
- TelegramSetup.tsx         # Telegram Bot 设置

// src/components/Permissions/
// 权限系统组件

- PermissionRulesEditor.tsx # 权限规则编辑
- RecentDenials.tsx         # 最近拒绝记录
- DangerousCommandAlert.tsx # 危险命令警告
```

---

## 📚 代码参考指南

### 可直接翻译的模块

| 来源 | 模块 | 目标语言 | 工作量 | 优先级 |
|------|------|----------|--------|--------|
| Claude Code | `src/bridge/*.ts` | Rust | 2周 | P0 |
| Claude Code | `src/hooks/toolPermission/*.ts` | Rust | 1周 | P0 |
| Hermes | `tools/environments/*.py` | Rust | 2周 | P1 |
| Hermes | `gateway/telegram.py` | Rust | 1周 | P1 |
| Hermes | `context_compressor.py` | Rust | 3天 | P2 |
| Hermes | `tools/registry.py` | Rust | 3天 | P2 |

### 需要重新设计的模块

| 来源 | 模块 | 原因 |
|------|------|------|
| Claude Code | `components/*.tsx` | Omiga 使用 MUI，需要适配样式 |
| Claude Code | `QueryEngine.ts` | Omiga 已有 LLM 层，只需适配接口 |
| Hermes | `run_agent.py` | Omiga 已有 Agent 调度器，功能重复 |
| Hermes | `cli.py` | Omiga 是桌面应用，非 CLI 工具 |

---

## 🔄 迁移检查清单

### 从 Claude Code 迁移时需要注意：

- [ ] TypeScript 类型 → Rust struct
- [ ] React hooks → Rust async/await
- [ ] 事件系统 → Tauri Event
- [ ] 状态管理 → Tauri State

### 从 Hermes 迁移时需要注意：

- [ ] Python 动态类型 → Rust 静态类型
- [ ] 装饰器模式 → Rust trait
- [ ] 全局变量 → Lazy<Mutex<>>
- [ ] asyncio → tokio

---

## 📈 成功指标

### Phase 1 (IDE 集成)
- VS Code 扩展下载量 > 1000
- 日活跃用户中 IDE 用户占比 > 30%

### Phase 2 (权限系统)
- 危险命令拦截率 100%
- 权限确认次数降低 50% (通过规则)

### Phase 3 (消息平台)
- Telegram 用户 > 500
- 消息平台消息量占比 > 20%

### Phase 4 (多后端)
- Docker 后端使用率 > 40%
- 远程 SSH 连接数 > 200

---

## 📝 附录

### A. 参考文档链接

- Hermes Agent: https://github.com/NousResearch/hermes-agent
- Claude Code: (泄露源码) src/ 目录
- Omiga: 当前项目

### B. 相关技能文件

- `skills/software-development/writing-plans/SKILL.md`
- `skills/software-development/systematic-debugging/SKILL.md`

### C. 设计文档

- `docs/unified-memory-design.md` - 统一记忆系统设计
- `docs/AGENT_SYSTEM_MIGRATION_PLAN.md` - Agent 系统迁移规划

---

## 🤝 贡献指南

1. 每个 Phase 应该独立分支开发
2. 遵循 Omiga 的 Rust 代码规范
3. 前端组件使用 MUI 组件库
4. 添加完整的单元测试
5. 更新相关文档

---

*最后更新: 2026-04-07*  
*文档维护者: Omiga Team*
