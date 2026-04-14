//! Agent 热重载系统
//!
//! 监控 .omiga/agents/ 目录下的 *.md 文件变化，自动加载/更新/卸载 Agent。

use super::definition::{AgentDefinition, AgentSource, PermissionMode};
use super::router::AgentRouter;
use crate::domain::tools::ToolContext;
use notify::{Config, Event, RecommendedWatcher, Watcher};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// 热重载事件
#[derive(Debug, Clone)]
pub enum HotReloadEvent {
    /// Agent 已加载或更新
    AgentLoaded {
        agent_type: String,
        source: PathBuf,
    },
    /// Agent 已卸载
    AgentUnloaded {
        agent_type: String,
    },
    /// 加载失败
    LoadFailed {
        path: PathBuf,
        error: String,
    },
}

/// Agent frontmatter 配置
#[derive(Debug, Deserialize)]
struct AgentFrontmatter {
    /// Agent 名称（唯一标识）
    name: String,
    /// 使用场景描述
    description: String,
    /// 指定模型
    model: Option<String>,
    /// 允许的工具列表
    tools: Option<Vec<String>>,
    /// 禁止的工具列表
    disallowed_tools: Option<Vec<String>>,
    /// 权限模式
    permission_mode: Option<String>,
    /// 是否后台运行
    background: Option<bool>,
    /// 是否省略 CLAUDE.md
    omit_claude_md: Option<bool>,
    /// 颜色标识
    color: Option<String>,
    /// 最大轮数
    max_turns: Option<usize>,
    /// 内置人格预设（与 `personality` 模块中的名称一致，如 concise、teacher）
    personality: Option<String>,
    /// 持久身份片段（类似 Hermes SOUL.md），YAML 多行字符串
    soul: Option<String>,
}

/// 动态加载的 Agent
pub struct DynamicAgent {
    pub agent_type: String,
    pub when_to_use: String,
    pub system_prompt_text: String,
    pub source: AgentSource,
    pub allowed_tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub color: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    pub background: bool,
    pub omit_claude_md: bool,
    pub max_turns: Option<usize>,
    /// 人格预设名（内置列表）或自定义占位（未知键在叠层时忽略）
    pub personality_key: Option<String>,
    /// 身份片段（soul）
    pub soul_text: Option<String>,
    pub file_path: PathBuf,
}

impl AgentDefinition for DynamicAgent {
    fn agent_type(&self) -> &str {
        &self.agent_type
    }

    fn when_to_use(&self) -> &str {
        &self.when_to_use
    }

    fn system_prompt(&self, _ctx: &ToolContext) -> String {
        self.system_prompt_text.clone()
    }

    fn soul_fragment(&self) -> Option<&str> {
        self.soul_text.as_deref()
    }

    fn personality_preset(&self) -> Option<&str> {
        self.personality_key.as_deref()
    }

    fn source(&self) -> AgentSource {
        self.source
    }

    fn allowed_tools(&self) -> Option<Vec<String>> {
        self.allowed_tools.clone()
    }

    fn disallowed_tools(&self) -> Option<Vec<String>> {
        self.disallowed_tools.clone()
    }

    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }

    fn permission_mode(&self) -> Option<PermissionMode> {
        self.permission_mode
    }

    fn background(&self) -> bool {
        self.background
    }

    fn omit_claude_md(&self) -> bool {
        self.omit_claude_md
    }

    fn max_turns(&self) -> Option<usize> {
        self.max_turns
    }
}

/// Agent 热重载管理器
pub struct AgentHotReloadManager {
    /// 动态加载的 Agent 缓存
    dynamic_agents: Arc<RwLock<HashMap<String, DynamicAgent>>>,
    /// 事件发送器
    event_tx: mpsc::Sender<HotReloadEvent>,
    /// 监控的目录列表
    watch_dirs: Vec<PathBuf>,
}

impl AgentHotReloadManager {
    /// 创建新的热重载管理器
    pub fn new() -> (Self, mpsc::Receiver<HotReloadEvent>) {
        let (event_tx, event_rx) = mpsc::channel(100);
        
        let manager = Self {
            dynamic_agents: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            watch_dirs: Vec::new(),
        };
        
        (manager, event_rx)
    }

    /// 添加监控目录
    pub fn add_watch_dir(&mut self, dir: PathBuf) {
        self.watch_dirs.push(dir);
    }

    /// 初始化并启动文件监控
    pub async fn start_watching(
        &self,
        router: Arc<RwLock<AgentRouter>>,
    ) -> Result<RecommendedWatcher, String> {
        let dynamic_agents = self.dynamic_agents.clone();
        let event_tx = self.event_tx.clone();
        let router_for_watcher = router.clone();
        
        // 创建 watcher
        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        tokio::spawn(handle_notify_event(
                            event,
                            dynamic_agents.clone(),
                            event_tx.clone(),
                            router_for_watcher.clone(),
                        ));
                    }
                    Err(e) => {
                        error!("文件监控错误: {}", e);
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| format!("创建文件监控失败: {}", e))?;

        // 监控所有配置的目录
        for dir in &self.watch_dirs {
            if dir.exists() {
                // 初始加载所有现有文件
                self.load_all_agents_in_dir(dir, &router).await?;
                
                // 添加监控
                // 注意：watcher 需要被保持存活才能继续监控
                // 这里我们返回 watcher，由调用者保持
            }
        }

        Ok(watcher)
    }

    /// 加载目录下所有 Agent
    async fn load_all_agents_in_dir(
        &self,
        dir: &Path,
        router: &Arc<RwLock<AgentRouter>>,
    ) -> Result<(), String> {
        if !dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| format!("读取目录失败: {}", e))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Err(e) = self.load_agent_from_file(&path, router).await {
                    warn!("加载 Agent 文件失败 {}: {}", path.display(), e);
                    let _ = self.event_tx.send(HotReloadEvent::LoadFailed {
                        path: path.clone(),
                        error: e,
                    }).await;
                }
            }
        }

        Ok(())
    }

    /// 从文件加载单个 Agent
    async fn load_agent_from_file(
        &self,
        path: &Path,
        router: &Arc<RwLock<AgentRouter>>,
    ) -> Result<(), String> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("读取文件失败: {}", e))?;

        let agent = parse_agent_from_markdown(&content, path)?;
        let agent_type = agent.agent_type.clone();
        
        // 保存到动态缓存
        {
            let mut agents = self.dynamic_agents.write().await;
            agents.insert(agent_type.clone(), agent.clone());
        }

        // 更新 router - 注册新的 Agent
        {
            let mut router_guard = router.write().await;
            router_guard.register(Box::new(agent));
        }

        info!("Agent 已加载: {} (来自 {})", agent_type, path.display());
        
        let _ = self.event_tx.send(HotReloadEvent::AgentLoaded {
            agent_type,
            source: path.to_path_buf(),
        }).await;

        Ok(())
    }

    /// 卸载 Agent（保留供手动调用）
    #[allow(dead_code)]
    async fn unload_agent(&self, path: &Path, router: &Arc<RwLock<AgentRouter>>) {
        let agent_type = path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        if let Some(agent_type) = agent_type {
            // 从缓存移除
            {
                let mut agents = self.dynamic_agents.write().await;
                agents.remove(&agent_type);
            }

            // 从 router 注销
            {
                let mut router_guard = router.write().await;
                router_guard.unregister(&agent_type);
            }

            info!("Agent 已卸载: {}", agent_type);
            
            let _ = self.event_tx.send(HotReloadEvent::AgentUnloaded {
                agent_type,
            }).await;
        }
    }

    /// 获取所有动态加载的 Agent
    pub async fn get_dynamic_agents(&self) -> Vec<DynamicAgent> {
        let agents = self.dynamic_agents.read().await;
        agents.values().cloned().collect()
    }
}

impl Clone for DynamicAgent {
    fn clone(&self) -> Self {
        Self {
            agent_type: self.agent_type.clone(),
            when_to_use: self.when_to_use.clone(),
            system_prompt_text: self.system_prompt_text.clone(),
            source: self.source,
            allowed_tools: self.allowed_tools.clone(),
            disallowed_tools: self.disallowed_tools.clone(),
            model: self.model.clone(),
            color: self.color.clone(),
            permission_mode: self.permission_mode,
            background: self.background,
            omit_claude_md: self.omit_claude_md,
            max_turns: self.max_turns,
            personality_key: self.personality_key.clone(),
            soul_text: self.soul_text.clone(),
            file_path: self.file_path.clone(),
        }
    }
}

/// 处理 notify 事件
async fn handle_notify_event(
    event: Event,
    dynamic_agents: Arc<RwLock<HashMap<String, DynamicAgent>>>,
    event_tx: mpsc::Sender<HotReloadEvent>,
    router: Arc<RwLock<AgentRouter>>,
) {
    use notify::EventKind;

    for path in &event.paths {
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            match &event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    debug!("Agent 文件变化: {}", path.display());
                    // 重新加载 Agent
                    if let Err(e) = load_agent_file(path, &dynamic_agents, &event_tx).await {
                        warn!("重新加载 Agent 失败: {}", e);
                        let _ = event_tx.send(HotReloadEvent::LoadFailed {
                            path: path.clone(),
                            error: e,
                        }).await;
                    }
                }
                EventKind::Remove(_) => {
                    debug!("Agent 文件删除: {}", path.display());
                    // 卸载 Agent
                    if let Some(agent_type) = path.file_stem().and_then(|s| s.to_str()) {
                        {
                            let mut agents = dynamic_agents.write().await;
                            agents.remove(agent_type);
                        }
                        
                        // 从 router 注销
                        {
                            let mut router_guard = router.write().await;
                            router_guard.unregister(agent_type);
                        }
                        
                        info!("Agent 已卸载: {}", agent_type);
                        let _ = event_tx.send(HotReloadEvent::AgentUnloaded {
                            agent_type: agent_type.to_string(),
                        }).await;
                    }
                }
                _ => {}
            }
        }
    }
}

/// 从文件加载 Agent（独立函数，用于事件处理）
async fn load_agent_file(
    path: &Path,
    dynamic_agents: &Arc<RwLock<HashMap<String, DynamicAgent>>>,
    event_tx: &mpsc::Sender<HotReloadEvent>,
) -> Result<(), String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("读取文件失败: {}", e))?;

    let agent = parse_agent_from_markdown(&content, path)?;
    let agent_type = agent.agent_type.clone();

    {
        let mut agents = dynamic_agents.write().await;
        agents.insert(agent_type.clone(), agent);
    }

    info!("Agent 已热重载: {} (来自 {})", agent_type, path.display());

    let _ = event_tx.send(HotReloadEvent::AgentLoaded {
        agent_type,
        source: path.to_path_buf(),
    }).await;

    Ok(())
}

/// 解析 markdown 文件为 Agent
fn parse_agent_from_markdown(content: &str, path: &Path) -> Result<DynamicAgent, String> {
    // 解析 frontmatter
    let (frontmatter_str, body) = if content.starts_with("---\n") {
        let rest = &content[4..];
        if let Some(end_idx) = rest.find("---\n") {
            let frontmatter = &rest[..end_idx];
            let body = &rest[end_idx + 4..];
            (frontmatter, body.trim())
        } else {
            ("", content.trim())
        }
    } else if content.starts_with("---\r\n") {
        let rest = &content[5..];
        if let Some(end_idx) = rest.find("---\r\n") {
            let frontmatter = &rest[..end_idx];
            let body = &rest[end_idx + 5..];
            (frontmatter, body.trim())
        } else {
            ("", content.trim())
        }
    } else {
        ("", content.trim())
    };

    // 解析 YAML frontmatter
    let frontmatter: AgentFrontmatter = if frontmatter_str.is_empty() {
        return Err("缺少 YAML frontmatter".to_string());
    } else {
        serde_yaml::from_str(frontmatter_str)
            .map_err(|e| format!("解析 frontmatter 失败: {}", e))?
    };

    // 验证必填字段
    if frontmatter.name.is_empty() {
        return Err("Agent 名称不能为空".to_string());
    }
    if frontmatter.description.is_empty() {
        return Err("Agent 描述不能为空".to_string());
    }

    // 解析权限模式
    let permission_mode = frontmatter.permission_mode.as_deref().and_then(|pm| {
        match pm.to_lowercase().as_str() {
            "default" => Some(PermissionMode::Default),
            "acceptedits" => Some(PermissionMode::AcceptEdits),
            "plan" => Some(PermissionMode::Plan),
            "bypasspermissions" => Some(PermissionMode::BypassPermissions),
            _ => None,
        }
    });

    // 判断来源
    let source = if path.starts_with(dirs::home_dir().unwrap_or_default()) {
        AgentSource::UserSettings
    } else {
        AgentSource::ProjectSettings
    };

    Ok(DynamicAgent {
        agent_type: frontmatter.name,
        when_to_use: frontmatter.description,
        system_prompt_text: body.to_string(),
        source,
        allowed_tools: frontmatter.tools,
        disallowed_tools: frontmatter.disallowed_tools,
        model: frontmatter.model,
        color: frontmatter.color,
        permission_mode,
        background: frontmatter.background.unwrap_or(false),
        omit_claude_md: frontmatter.omit_claude_md.unwrap_or(false),
        max_turns: frontmatter.max_turns,
        personality_key: frontmatter.personality,
        soul_text: frontmatter.soul,
        file_path: path.to_path_buf(),
    })
}

/// 启动热重载系统（便捷函数）
pub async fn start_agent_hot_reload(
    router: Arc<RwLock<AgentRouter>>,
    project_root: &Path,
) -> Result<(AgentHotReloadManager, RecommendedWatcher, mpsc::Receiver<HotReloadEvent>), String> {
    let (mut manager, event_rx) = AgentHotReloadManager::new();

    // 添加监控目录
    let user_agents_dir = dirs::home_dir()
        .map(|h| h.join(".omiga").join("agents"))
        .unwrap_or_default();
    
    let project_agents_dir = project_root.join(".omiga").join("agents");

    manager.add_watch_dir(user_agents_dir);
    manager.add_watch_dir(project_agents_dir);

    // 启动监控
    let watcher = manager.start_watching(router).await?;

    Ok((manager, watcher, event_rx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_markdown() {
        let markdown = r#"---
name: test-agent
description: 测试 Agent
model: haiku
tools:
  - Read
  - Write
background: true
personality: teacher
soul: |
  用耐心、清晰的语气回答。
---

这是一个测试 Agent 的系统提示词。
可以有多行内容。
"#;

        let agent = parse_agent_from_markdown(markdown, Path::new("/tmp/test.md")).unwrap();
        
        assert_eq!(agent.agent_type, "test-agent");
        assert_eq!(agent.when_to_use, "测试 Agent");
        assert_eq!(agent.model, Some("haiku".to_string()));
        assert_eq!(agent.allowed_tools, Some(vec!["Read".to_string(), "Write".to_string()]));
        assert_eq!(agent.personality_key.as_deref(), Some("teacher"));
        assert!(agent.soul_text.as_ref().is_some_and(|s| s.contains("耐心")));
        assert!(agent.background);
        assert!(agent.system_prompt_text.contains("这是一个测试 Agent"));
    }

    #[test]
    fn test_parse_agent_missing_frontmatter() {
        let markdown = "没有 frontmatter 的内容";
        let result = parse_agent_from_markdown(markdown, Path::new("/tmp/test.md"));
        assert!(result.is_err());
    }
}
