# Omiga 增强实施指南

> 详细的代码模板和接口定义

---

## 1. Bridge 系统详细设计

### 1.1 核心接口定义

```rust
// src-tauri/src/bridge/types.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdeType {
    VsCode,
    JetBrains,
    Cursor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeConnectionInfo {
    pub id: String,
    pub ide_type: IdeType,
    pub connected_at: String,
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BridgeMessage {
    // IDE -> Omiga
    Context {
        kind: ContextKind,
        file_path: String,
        content: String,
        line_range: Option<(usize, usize)>,
    },
    ExecuteTool {
        tool_name: String,
        arguments: serde_json::Value,
    },
    
    // Omiga -> IDE
    Diff {
        file_path: String,
        original: String,
        modified: String,
    },
    PermissionRequest {
        request_id: String,
        tool_name: String,
        arguments: serde_json::Value,
        description: String,
    },
    PermissionResponse {
        request_id: String,
        approved: bool,
    },
    FocusFile {
        file_path: String,
        line: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextKind {
    Selection,      // 选中代码
    CurrentFile,    // 当前文件
    OpenFiles,      // 所有打开文件
    Workspace,      // 整个工作区
}
```

### 1.2 Bridge Server 实现模板

```rust
// src-tauri/src/bridge/server.rs

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};

pub struct BridgeServer {
    port: u16,
    jwt_secret: String,
    connections: Arc<RwLock<HashMap<String, IdeConnection>>>,
    message_handler: Arc<dyn Fn(BridgeMessage) -> BoxFuture<'static, Result<BridgeMessage>> + Send + Sync>,
}

impl BridgeServer {
    pub async fn start<F, Fut>(
        port: u16,
        jwt_secret: String,
        handler: F,
    ) -> Result<Self>
    where
        F: Fn(BridgeMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<BridgeMessage>> + Send + 'static,
    {
        let server = Self {
            port,
            jwt_secret,
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_handler: Arc::new(move |msg| Box::pin(handler(msg))),
        };
        
        // 启动 WebSocket 服务器
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
        tracing::info!("Bridge server listening on port {}", port);
        
        let connections = server.connections.clone();
        let message_handler = server.message_handler.clone();
        let jwt_secret = server.jwt_secret.clone();
        
        tokio::spawn(async move {
            while let Ok((stream, addr)) = listener.accept().await {
                let connections = connections.clone();
                let handler = message_handler.clone();
                let secret = jwt_secret.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = Self::handle_connection(
                        stream, addr, connections, handler, secret
                    ).await {
                        tracing::error!("Connection error: {}", e);
                    }
                });
            }
        });
        
        Ok(server)
    }
    
    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        connections: Arc<RwLock<HashMap<String, IdeConnection>>>,
        handler: Arc<dyn Fn(BridgeMessage) -> BoxFuture<'static, Result<BridgeMessage>> + Send + Sync>,
        jwt_secret: String,
    ) -> Result<()> {
        let ws_stream = accept_async(stream).await?;
        let (mut write, mut read) = ws_stream.split();
        
        // 1. 等待认证消息
        let auth_msg = read.next().await.ok_or("No auth message")??;
        let token = Self::extract_token(&auth_msg)?;
        
        // 2. 验证 JWT
        let claims = Self::verify_jwt(&token, &jwt_secret)?;
        let conn_id = claims.sub;
        
        tracing::info!("IDE connected: {} from {}", conn_id, addr);
        
        // 3. 注册连接
        let (tx, mut rx) = mpsc::channel::<BridgeMessage>(100);
        {
            let mut conns = connections.write().await;
            conns.insert(conn_id.clone(), IdeConnection {
                ide_type: claims.ide_type,
                tx,
            });
        }
        
        // 4. 消息循环
        loop {
            tokio::select! {
                // 接收来自 IDE 的消息
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            let bridge_msg: BridgeMessage = serde_json::from_str(&text)?;
                            let response = handler(bridge_msg).await?;
                            let response_text = serde_json::to_string(&response)?;
                            write.send(WsMessage::Text(response_text)).await?;
                        }
                        Some(Ok(WsMessage::Close(_))) | None => break,
                        _ => {}
                    }
                }
                // 发送消息到 IDE
                msg = rx.recv() => {
                    if let Some(msg) = msg {
                        let text = serde_json::to_string(&msg)?;
                        write.send(WsMessage::Text(text)).await?;
                    }
                }
            }
        }
        
        // 5. 清理连接
        connections.write().await.remove(&conn_id);
        tracing::info!("IDE disconnected: {}", conn_id);
        
        Ok(())
    }
    
    pub async fn broadcast(&self, message: BridgeMessage) {
        let conns = self.connections.read().await;
        for (id, conn) in conns.iter() {
            if let Err(e) = conn.tx.send(message.clone()).await {
                tracing::error!("Failed to send to {}: {}", id, e);
            }
        }
    }
}
```

### 1.3 Tauri 命令集成

```rust
// src-tauri/src/bridge/commands.rs

#[tauri::command]
pub async fn bridge_get_status(
    state: tauri::State<'_, BridgeState>,
) -> Result<BridgeStatus, String> {
    let server = state.server.read().await;
    Ok(BridgeStatus {
        enabled: server.is_some(),
        port: server.as_ref().map(|s| s.port),
        connections: state.get_connection_count().await,
    })
}

#[tauri::command]
pub async fn bridge_start(
    app: AppHandle,
    state: tauri::State<'_, BridgeState>,
) -> Result<u16, String> {
    let mut server_guard = state.server.write().await;
    
    if server_guard.is_some() {
        return Err("Bridge already running".to_string());
    }
    
    let port = pick_free_port(30000..31000).map_err(|e| e.to_string())?;
    let secret = generate_jwt_secret();
    
    let server = BridgeServer::start(port, secret.clone(), move |msg| {
        handle_bridge_message(msg, app.clone())
    }).await.map_err(|e| e.to_string())?;
    
    *server_guard = Some(server);
    
    // 保存 JWT secret 到 keychain
    save_bridge_secret(&secret).map_err(|e| e.to_string())?;
    
    Ok(port)
}

async fn handle_bridge_message(msg: BridgeMessage, app: AppHandle) -> Result<BridgeMessage> {
    match msg {
        BridgeMessage::Context { kind, file_path, content, line_range } => {
            // 转发到前端
            app.emit("bridge:context", BridgeContextEvent {
                kind, file_path, content, line_range
            })?;
            Ok(BridgeMessage::Ack)
        }
        BridgeMessage::ExecuteTool { tool_name, arguments } => {
            // 执行工具并返回结果
            let result = execute_tool(&tool_name, arguments).await?;
            Ok(BridgeMessage::ToolResult { result })
        }
        _ => Ok(BridgeMessage::Error("Unknown message type".to_string())),
    }
}
```

---

## 2. 权限系统详细设计

### 2.1 核心类型定义

```rust
// src-tauri/src/domain/permissions/types.rs

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PermissionMode {
    AskEveryTime,   // 每次询问
    Session,        // 会话级确认（一次会话内只问一次）
    Plan,           // Plan 模式（批量确认）
    Auto,           // 自动批准安全操作
    Bypass,         // 完全绕过（危险！）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub name: String,
    pub tool_pattern: String,           // "bash", "file_*", "*"
    pub path_pattern: Option<String>,   // "/safe/path/*", "*.md"
    pub mode: PermissionMode,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: ToolArgs,
    pub file_paths: Vec<PathBuf>,
    pub description: String,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,       // 读取操作
    Normal,     // 一般修改
    Dangerous,  // 删除/系统修改
    Critical,   // rm -rf / 级别
}

#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,                              // 直接允许
    Deny(String),                       // 拒绝 + 原因
    RequireApproval(PermissionRequest), // 需要用户确认
}
```

### 2.2 权限管理器实现

```rust
// src-tauri/src/domain/permissions/manager.rs

pub struct PermissionManager {
    rules: RwLock<Vec<PermissionRule>>,
    session_approvals: RwLock<HashMap<String, HashSet<String>>>, // session_id -> approved_tool_hashes
    recent_denials: RwLock<VecDeque<DenialRecord>>,
    dangerous_patterns: Vec<DangerousPattern>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            session_approvals: RwLock::new(HashMap::new()),
            recent_denials: RwLock::new(VecDeque::with_capacity(100)),
            dangerous_patterns: Self::load_dangerous_patterns(),
        }
    }
    
    /// 检查权限（核心方法）
    pub async fn check_permission(
        &self,
        session_id: &str,
        tool: &str,
        args: &ToolArgs,
    ) -> PermissionDecision {
        // 1. 计算风险等级
        let risk = self.assess_risk(tool, args);
        
        // 2. CRITICAL 级别直接要求确认
        if risk == RiskLevel::Critical {
            return PermissionDecision::RequireApproval(
                self.create_request(tool, args, risk).await
            );
        }
        
        // 3. 检查精确匹配规则
        if let Some(rule) = self.find_exact_rule(tool, args).await {
            return self.apply_rule(rule, session_id, tool, args).await;
        }
        
        // 4. 检查模式匹配规则
        if let Some(rule) = self.find_pattern_rule(tool, args).await {
            return self.apply_rule(rule, session_id, tool, args).await;
        }
        
        // 5. 检查会话级已批准
        if self.is_session_approved(session_id, tool, args).await {
            return PermissionDecision::Allow;
        }
        
        // 6. 根据风险等级决定
        match risk {
            RiskLevel::Safe => PermissionDecision::Allow,
            RiskLevel::Normal => {
                // Auto 模式下自动批准
                if self.is_auto_mode() {
                    PermissionDecision::Allow
                } else {
                    PermissionDecision::RequireApproval(
                        self.create_request(tool, args, risk).await
                    )
                }
            }
            RiskLevel::Dangerous | RiskLevel::Critical => {
                PermissionDecision::RequireApproval(
                    self.create_request(tool, args, risk).await
                )
            }
        }
    }
    
    /// 评估风险等级
    fn assess_risk(&self, tool: &str, args: &ToolArgs) -> RiskLevel {
        // 危险命令检测
        if tool == "bash" || tool == "shell" {
            let cmd = args.get("command").unwrap_or_default();
            
            // CRITICAL 级别
            for pattern in &self.dangerous_patterns {
                if pattern.level == RiskLevel::Critical && pattern.matches(&cmd) {
                    return RiskLevel::Critical;
                }
            }
            
            // Dangerous 级别
            if cmd.contains("rm -rf") || cmd.contains("mkfs") {
                return RiskLevel::Dangerous;
            }
        }
        
        // 文件操作
        if tool == "file_write" || tool == "file_edit" {
            let path = args.get("path").unwrap_or_default();
            
            // 系统路径保护
            if path.starts_with("/etc/") || path.starts_with("/boot/") {
                return RiskLevel::Dangerous;
            }
            
            // .env 文件
            if path.contains(".env") || path.contains("secret") {
                return RiskLevel::Dangerous;
            }
        }
        
        // 读取操作通常是安全的
        if tool == "file_read" || tool == "glob" || tool == "grep" {
            return RiskLevel::Safe;
        }
        
        RiskLevel::Normal
    }
    
    /// 加载危险模式（来自 Claude Code）
    fn load_dangerous_patterns() -> Vec<DangerousPattern> {
        vec![
            DangerousPattern {
                pattern: regex::Regex::new(r"rm\s+-rf\s+/").unwrap(),
                level: RiskLevel::Critical,
                description: "删除根目录",
            },
            DangerousPattern {
                pattern: regex::Regex::new(r":\(\)\{\s*:\|:&\s*\};:").unwrap(),
                level: RiskLevel::Critical,
                description: "Fork bomb",
            },
            DangerousPattern {
                pattern: regex::Regex::new(r">\s+/dev/sda").unwrap(),
                level: RiskLevel::Critical,
                description: "覆盖硬盘",
            },
            DangerousPattern {
                pattern: regex::Regex::new(r"mkfs\.").unwrap(),
                level: RiskLevel::Dangerous,
                description: "格式化文件系统",
            },
        ]
    }
    
    /// 批准请求
    pub async fn approve_request(
        &self,
        session_id: &str,
        request_id: &str,
        mode: PermissionMode,
    ) -> Result<()> {
        let request = self.get_request(request_id).await?;
        
        match mode {
            PermissionMode::Session => {
                // 添加到会话级批准
                let mut approvals = self.session_approvals.write().await;
                let hash = self.compute_tool_hash(&request.tool_name, &request.arguments);
                approvals
                    .entry(session_id.to_string())
                    .or_default()
                    .insert(hash);
            }
            PermissionMode::Plan => {
                // 创建规则
                let rule = PermissionRule {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: format!("Plan approval for {}", request.tool_name),
                    tool_pattern: request.tool_name.clone(),
                    path_pattern: request.file_paths.first().map(|p| p.to_string_lossy().to_string()),
                    mode: PermissionMode::Plan,
                    expires_at: Some(Utc::now() + Duration::hours(1)),
                    created_at: Utc::now(),
                };
                self.add_rule(rule).await?;
            }
            _ => {}
        }
        
        Ok(())
    }
}
```

---

## 3. Gateway 系统详细设计

### 3.1 Telegram 适配器模板

```rust
// src-tauri/src/gateway/telegram.rs

use teloxide::prelude::*;
use teloxide::types::{Message, Update};

pub struct TelegramAdapter {
    bot: Bot,
    handler: MessageHandler,
}

#[derive(Clone)]
pub struct MessageHandler {
    callback: Arc<dyn Fn(PlatformMessage) -> BoxFuture<'static, Result<String>> + Send + Sync>,
}

impl TelegramAdapter {
    pub fn new(bot_token: String, handler: MessageHandler) -> Self {
        Self {
            bot: Bot::new(bot_token),
            handler,
        }
    }
    
    pub async fn start(self) -> Result<()> {
        let handler = self.handler.clone();
        
        let update_handler = Update::filter_message()
            .endpoint(move |bot: Bot, msg: Message| {
                let handler = handler.clone();
                async move {
                    if let Some(text) = msg.text() {
                        let platform_msg = PlatformMessage {
                            platform: "telegram".to_string(),
                            user_id: msg.from.id.to_string(),
                            chat_id: msg.chat.id.to_string(),
                            message_id: msg.id.to_string(),
                            content: text.to_string(),
                            timestamp: Utc::now(),
                        };
                        
                        // 调用处理器
                        match handler.handle(platform_msg).await {
                            Ok(response) => {
                                // 发送响应
                                bot.send_message(msg.chat.id, response).await?;
                            }
                            Err(e) => {
                                bot.send_message(
                                    msg.chat.id,
                                    format!("Error: {}", e)
                                ).await?;
                            }
                        }
                    }
                    respond(())
                }
            });
        
        Dispatcher::builder(self.bot, update_handler)
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;
        
        Ok(())
    }
    
    /// 流式发送消息（打字机效果）
    pub async fn send_streaming(
        &self,
        chat_id: &str,
        stream: impl Stream<Item = Result<String>>,
    ) -> Result<()> {
        let chat_id: ChatId = chat_id.parse()?;
        let mut message: Option<Message> = None;
        let mut buffer = String::new();
        let mut last_update = Instant::now();
        
        tokio::pin!(stream);
        
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(text) => {
                    buffer.push_str(&text);
                    
                    // 每 500ms 或 100 字符更新一次
                    if last_update.elapsed() > Duration::from_millis(500) 
                        || buffer.len() - message.as_ref().map(|m| m.text().unwrap_or("").len()).unwrap_or(0) > 100 {
                        
                        match message {
                            Some(ref msg) => {
                                self.bot.edit_message_text(chat_id, msg.id, &buffer).await?;
                            }
                            None => {
                                message = Some(self.bot.send_message(chat_id, &buffer).await?);
                            }
                        }
                        last_update = Instant::now();
                    }
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    break;
                }
            }
        }
        
        // 发送最终消息
        if let Some(msg) = message {
            self.bot.edit_message_text(chat_id, msg.id, &buffer).await?;
        }
        
        Ok(())
    }
}
```

### 3.2 会话路由系统

```rust
// src-tauri/src/gateway/router.rs

pub struct SessionRouter {
    /// 平台用户ID -> Omiga 会话ID 映射
    user_sessions: RwLock<HashMap<String, String>>,
    /// 平台 -> 用户ID 映射
    platform_users: RwLock<HashMap<String, HashSet<String>>>,
}

impl SessionRouter {
    /// 查找或创建会话
    pub async fn find_or_create_session(
        &self,
        platform: &str,
        user_id: &str,
        chat_id: &str,
    ) -> Result<String> {
        let key = format!("{}:{}", platform, user_id);
        
        // 1. 检查现有映射
        {
            let sessions = self.user_sessions.read().await;
            if let Some(session_id) = sessions.get(&key) {
                // 验证会话是否仍然有效
                if self.is_session_valid(session_id).await? {
                    return Ok(session_id.clone());
                }
            }
        }
        
        // 2. 创建新会话
        let session_id = self.create_session(platform, user_id, chat_id).await?;
        
        // 3. 更新映射
        {
            let mut sessions = self.user_sessions.write().await;
            sessions.insert(key.clone(), session_id.clone());
        }
        
        {
            let mut platforms = self.platform_users.write().await;
            platforms
                .entry(platform.to_string())
                .or_default()
                .insert(user_id.to_string());
        }
        
        Ok(session_id)
    }
    
    /// 获取用户的所有活跃会话
    pub async fn get_user_sessions(&self, user_id: &str) -> Vec<SessionInfo> {
        // 实现...
        vec![]
    }
    
    /// 清理过期会话
    pub async fn cleanup_expired_sessions(&self) -> Result<usize> {
        // 实现...
        Ok(0)
    }
}
```

---

## 4. 执行后端详细设计

### 4.1 Docker 后端

```rust
// src-tauri/src/domain/execution/docker.rs

use bollard::Docker;
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecOptions};

pub struct DockerBackend {
    docker: Docker,
    container_id: String,
    work_dir: PathBuf,
}

impl DockerBackend {
    pub async fn new(image: &str, work_dir: impl AsRef<Path>) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        
        // 确保镜像存在
        Self::pull_image_if_needed(&docker, image).await?;
        
        // 创建容器
        let container_config = Config {
            image: Some(image),
            working_dir: Some("/workspace"),
            host_config: Some(bollard::service::HostConfig {
                binds: Some(vec![format!("{}:/workspace", work_dir.as_ref().display())]),
                ..Default::default()
            }),
            ..Default::default()
        };
        
        let container = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: format!("omiga-{}", uuid::Uuid::new_v4()),
                    platform: None,
                }),
                container_config,
            )
            .await?;
        
        docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await?;
        
        Ok(Self {
            docker,
            container_id: container.id,
            work_dir: work_dir.as_ref().to_path_buf(),
        })
    }
}

#[async_trait]
impl ExecutionBackend for DockerBackend {
    fn name(&self) -> &str {
        "docker"
    }
    
    async fn execute(&self, command: &str, cwd: &Path) -> Result<ExecutionResult> {
        // 将本地路径转换为容器内路径
        let container_cwd = if cwd.starts_with(&self.work_dir) {
            PathBuf::from("/workspace").join(cwd.strip_prefix(&self.work_dir)?)
        } else {
            PathBuf::from("/workspace")
        };
        
        let exec = self.docker
            .create_exec(
                &self.container_id,
                CreateExecOptions {
                    cmd: Some(vec!["sh", "-c", command]),
                    working_dir: Some(container_cwd.to_str().unwrap()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        
        let mut stdout = String::new();
        let mut stderr = String::new();
        
        let stream = self.docker.start_exec(&exec.id, None);
        tokio::pin!(stream);
        
        while let Some(result) = stream.next().await {
            match result {
                Ok(bollard::container::LogOutput::StdOut { message }) => {
                    stdout.push_str(&String::from_utf8_lossy(&message));
                }
                Ok(bollard::container::LogOutput::StdErr { message }) => {
                    stderr.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }
        
        // 获取退出码
        let inspect = self.docker.inspect_exec(&exec.id).await?;
        let exit_code = inspect.exit_code.unwrap_or(0);
        
        Ok(ExecutionResult {
            stdout,
            stderr,
            exit_code,
        })
    }
    
    async fn read_file(&self, path: &Path) -> Result<String> {
        // 使用 docker cp 或 exec cat
        let result = self.execute(&format!("cat {}", path.display()), Path::new("/")).await?;
        Ok(result.stdout)
    }
    
    async fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        // 使用 docker exec 写入
        let encoded = base64::encode(content);
        let cmd = format!(
            "echo {} | base64 -d > {}",
            encoded,
            path.display()
        );
        self.execute(&cmd, Path::new("/")).await?;
        Ok(())
    }
}

impl Drop for DockerBackend {
    fn drop(&mut self) {
        // 清理容器
        let docker = self.docker.clone();
        let container_id = self.container_id.clone();
        tokio::spawn(async move {
            let _ = docker.stop_container(&container_id, None).await;
            let _ = docker.remove_container(&container_id, None).await;
        });
    }
}
```

### 4.2 SSH 后端

```rust
// src-tauri/src/domain/execution/ssh.rs

use async_ssh2_tokio::client::{Client as SshClient, AuthMethod, ServerCheckMethod};

pub struct SshBackend {
    client: SshClient,
    work_dir: PathBuf,
}

impl SshBackend {
    pub async fn connect(
        host: &str,
        port: u16,
        username: &str,
        auth: SshAuth,
    ) -> Result<Self> {
        let auth_method = match auth {
            SshAuth::Password(pwd) => AuthMethod::Password(pwd),
            SshAuth::Key { private_key, passphrase } => {
                AuthMethod::PrivateKey { key: private_key, passphrase }
            }
        };
        
        let client = SshClient::connect(
            (host, port),
            username,
            auth_method,
            ServerCheckMethod::NoCheck, // 生产环境应该检查 host key
        ).await?;
        
        Ok(Self {
            client,
            work_dir: PathBuf::from("~"),
        })
    }
}

#[async_trait]
impl ExecutionBackend for SshBackend {
    fn name(&self) -> &str {
        "ssh"
    }
    
    async fn execute(&self, command: &str, cwd: &Path) -> Result<ExecutionResult> {
        let full_cmd = if cwd == Path::new(".") || cwd == Path::new("") {
            command.to_string()
        } else {
            format!("cd {} && {}", cwd.display(), command)
        };
        
        let result = self.client.execute(&full_cmd).await?;
        
        Ok(ExecutionResult {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_status,
        })
    }
    
    async fn read_file(&self, path: &Path) -> Result<String> {
        let result = self.execute(&format!("cat {}", path.display()), &self.work_dir).await?;
        Ok(result.stdout)
    }
    
    async fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        // 使用 scp 或 sftp
        let sftp = self.client.sftp().await?;
        let mut file = sftp.create(path).await?;
        file.write_all(content.as_bytes()).await?;
        Ok(())
    }
}
```

---

## 5. 前端集成指南

### 5.1 Bridge 状态管理

```typescript
// src/state/bridgeStore.ts

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface BridgeState {
  isRunning: boolean;
  port: number | null;
  connections: IdeConnectionInfo[];
  startBridge: () => Promise<void>;
  stopBridge: () => Promise<void>;
  refreshStatus: () => Promise<void>;
}

export const useBridgeStore = create<BridgeState>((set, get) => ({
  isRunning: false,
  port: null,
  connections: [],
  
  startBridge: async () => {
    const port = await invoke<number>('bridge_start');
    set({ isRunning: true, port });
    
    // 监听 Bridge 事件
    await listen<BridgeContextEvent>('bridge:context', (event) => {
      // 将 IDE 上下文添加到当前会话
      useSessionStore.getState().addIdeContext(event.payload);
    });
  },
  
  stopBridge: async () => {
    await invoke('bridge_stop');
    set({ isRunning: false, port: null });
  },
  
  refreshStatus: async () => {
    const status = await invoke<BridgeStatus>('bridge_get_status');
    set({ 
      isRunning: status.enabled, 
      port: status.port,
      connections: status.connections 
    });
  },
}));
```

### 5.2 权限对话框组件

```tsx
// src/components/permissions/PermissionDialog.tsx

import React, { useState } from 'react';
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  Typography,
  Alert,
  FormControl,
  InputLabel,
  Select,
  MenuItem,
  Box,
} from '@mui/material';

interface PermissionDialogProps {
  open: boolean;
  request: PermissionRequest;
  onApprove: (mode: PermissionMode) => void;
  onDeny: () => void;
}

export const PermissionDialog: React.FC<PermissionDialogProps> = ({
  open,
  request,
  onApprove,
  onDeny,
}) => {
  const [mode, setMode] = useState<PermissionMode>('session');
  
  const isDangerous = request.riskLevel === 'dangerous' || request.riskLevel === 'critical';
  
  return (
    <Dialog open={open} maxWidth="sm" fullWidth>
      <DialogTitle>
        {isDangerous ? '⚠️ 危险操作确认' : '权限确认'}
      </DialogTitle>
      
      <DialogContent>
        {isDangerous && (
          <Alert severity="error" sx={{ mb: 2 }}>
            这是一个高风险操作，请谨慎确认
          </Alert>
        )}
        
        <Typography variant="body1" gutterBottom>
          <strong>工具:</strong> {request.tool_name}
        </Typography>
        
        <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
          {request.description}
        </Typography>
        
        <Box sx={{ bgcolor: 'grey.100', p: 1, borderRadius: 1, mb: 2 }}>
          <code style={{ fontSize: '0.875rem' }}>
            {JSON.stringify(request.arguments, null, 2)}
          </code>
        </Box>
        
        <FormControl fullWidth>
          <InputLabel>记住我的选择</InputLabel>
          <Select
            value={mode}
            onChange={(e) => setMode(e.target.value as PermissionMode)}
          >
            <MenuItem value="ask">每次询问</MenuItem>
            <MenuItem value="session">本次会话</MenuItem>
            <MenuItem value="plan">接下来1小时</MenuItem>
          </Select>
        </FormControl>
      </DialogContent>
      
      <DialogActions>
        <Button onClick={onDeny} color="error">
          拒绝
        </Button>
        <Button 
          onClick={() => onApprove(mode)} 
          color={isDangerous ? 'error' : 'primary'}
          variant="contained"
        >
          允许
        </Button>
      </DialogActions>
    </Dialog>
  );
};
```

---

## 6. 测试策略

### 6.1 Bridge 测试

```rust
// src-tauri/tests/bridge_tests.rs

#[tokio::test]
async fn test_bridge_authentication() {
    let server = BridgeServer::start(0, "test_secret", |_| async { Ok(()) }).await.unwrap();
    
    // 测试无效 JWT
    let client = tokio_tungstenite::connect_async("ws://127.0.0.1:PORT").await.unwrap();
    // ... 发送无效 token
    
    // 验证连接被拒绝
}

#[tokio::test]
async fn test_bridge_message_roundtrip() {
    // 测试消息往返
}
```

### 6.2 权限测试

```rust
// src-tauri/tests/permission_tests.rs

#[tokio::test]
async fn test_dangerous_command_detection() {
    let manager = PermissionManager::new();
    
    // rm -rf / 应该被检测为 CRITICAL
    let result = manager.check_permission(
        "session_1",
        "bash",
        &args!({"command": "rm -rf /"}),
    ).await;
    
    assert!(matches!(result, PermissionDecision::RequireApproval(_)));
}

#[tokio::test]
async fn test_permission_rule_matching() {
    // 测试规则匹配
}
```

---

*文档版本: 1.0*  
*最后更新: 2026-04-07*
