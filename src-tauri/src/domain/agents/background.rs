//! 后台 Agent 执行系统
//!
//! 支持在后台异步执行 Agent 任务，不阻塞主会话。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;
use uuid::Uuid;

/// 后台 Agent 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackgroundAgentStatus {
    /// 等待执行
    Pending,
    /// 运行中
    Running,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 后台 Agent 任务信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundAgentTask {
    /// 任务唯一 ID
    pub task_id: String,
    /// Agent 类型
    pub agent_type: String,
    /// 任务描述
    pub description: String,
    /// 当前状态
    pub status: BackgroundAgentStatus,
    /// 创建时间 (Unix timestamp)
    pub created_at: u64,
    /// 开始时间 (Unix timestamp)
    pub started_at: Option<u64>,
    /// 完成时间 (Unix timestamp)
    pub completed_at: Option<u64>,
    /// 结果摘要
    pub result_summary: Option<String>,
    /// 错误信息
    pub error_message: Option<String>,
    /// 输出文件路径
    pub output_path: Option<String>,
    /// 会话 ID
    pub session_id: String,
    /// 消息 ID
    pub message_id: String,
}

/// 后台 Agent 管理器
pub struct BackgroundAgentManager {
    tasks: Arc<RwLock<HashMap<String, BackgroundAgentTask>>>,
    cancel_tokens: Arc<Mutex<HashMap<String, tokio_util::sync::CancellationToken>>>,
}

impl BackgroundAgentManager {
    /// 创建新的管理器
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 注册新任务
    pub async fn register_task(
        &self,
        agent_type: String,
        description: String,
        session_id: String,
        message_id: String,
    ) -> String {
        let task_id = Uuid::new_v4().to_string();
        let task = BackgroundAgentTask {
            task_id: task_id.clone(),
            agent_type,
            description,
            status: BackgroundAgentStatus::Pending,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            started_at: None,
            completed_at: None,
            result_summary: None,
            error_message: None,
            output_path: None,
            session_id,
            message_id,
        };

        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id.clone(), task);
        task_id
    }

    /// 更新任务状态
    pub async fn update_task_status(
        &self,
        task_id: &str,
        status: BackgroundAgentStatus,
    ) -> Option<BackgroundAgentTask> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = status.clone();
            match status {
                BackgroundAgentStatus::Running => {
                    task.started_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    );
                }
                BackgroundAgentStatus::Completed | BackgroundAgentStatus::Failed | BackgroundAgentStatus::Cancelled => {
                    task.completed_at = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    );
                }
                _ => {}
            }
            Some(task.clone())
        } else {
            None
        }
    }

    /// 设置任务结果
    pub async fn set_task_result(
        &self,
        task_id: &str,
        result_summary: String,
        output_path: String,
    ) -> Option<BackgroundAgentTask> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.result_summary = Some(result_summary);
            task.output_path = Some(output_path);
            task.status = BackgroundAgentStatus::Completed;
            task.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            Some(task.clone())
        } else {
            None
        }
    }

    /// 设置任务错误
    pub async fn set_task_error(
        &self,
        task_id: &str,
        error_message: String,
    ) -> Option<BackgroundAgentTask> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.error_message = Some(error_message);
            task.status = BackgroundAgentStatus::Failed;
            task.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            Some(task.clone())
        } else {
            None
        }
    }

    /// 获取任务信息
    pub async fn get_task(&self, task_id: &str) -> Option<BackgroundAgentTask> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// 获取所有任务
    pub async fn get_all_tasks(&self) -> Vec<BackgroundAgentTask> {
        let tasks = self.tasks.read().await;
        tasks.values().cloned().collect()
    }

    /// 获取会话的所有任务
    pub async fn get_session_tasks(&self, session_id: &str) -> Vec<BackgroundAgentTask> {
        let tasks = self.tasks.read().await;
        tasks
            .values()
            .filter(|t| t.session_id == session_id)
            .cloned()
            .collect()
    }

    /// 取消任务
    pub async fn cancel_task(&self, task_id: &str) -> Option<BackgroundAgentTask> {
        // 发送取消信号
        if let Some(token) = self.cancel_tokens.lock().unwrap().get(task_id) {
            token.cancel();
        }

        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = BackgroundAgentStatus::Cancelled;
            task.completed_at = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            Some(task.clone())
        } else {
            None
        }
    }

    /// 创建取消令牌
    pub fn create_cancel_token(&self, task_id: &str) -> tokio_util::sync::CancellationToken {
        let token = tokio_util::sync::CancellationToken::new();
        self.cancel_tokens
            .lock()
            .unwrap()
            .insert(task_id.to_string(), token.clone());
        token
    }

    /// 清理已完成/失败的旧任务
    pub async fn cleanup_old_tasks(&self, max_age_secs: u64) -> usize {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut tasks = self.tasks.write().await;
        let to_remove: Vec<String> = tasks
            .iter()
            .filter(|(_, task)| {
                if let Some(completed_at) = task.completed_at {
                    now - completed_at > max_age_secs
                } else {
                    false
                }
            })
            .map(|(id, _)| id.clone())
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            tasks.remove(&id);
            self.cancel_tokens.lock().unwrap().remove(&id);
        }

        count
    }
}

impl Default for BackgroundAgentManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Tauri 事件：后台 Agent 任务完成
pub const BACKGROUND_AGENT_COMPLETE_EVENT: &str = "background-agent-complete";

/// Tauri 事件：后台 Agent 任务更新（进度、状态变化）
pub const BACKGROUND_AGENT_UPDATE_EVENT: &str = "background-agent-update";

/// 后台 Agent 完成事件载荷
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundAgentCompletePayload {
    pub session_id: String,
    pub message_id: String,
    pub task_id: String,
    pub agent_type: String,
    pub description: String,
    pub status: BackgroundAgentStatus,
    pub result_summary: Option<String>,
    pub error_message: Option<String>,
    pub output_path: Option<String>,
}

/// 发送后台 Agent 完成事件
pub fn emit_background_agent_complete(
    app: &AppHandle,
    task: &BackgroundAgentTask,
) -> Result<(), String> {
    let payload = BackgroundAgentCompletePayload {
        session_id: task.session_id.clone(),
        message_id: task.message_id.clone(),
        task_id: task.task_id.clone(),
        agent_type: task.agent_type.clone(),
        description: task.description.clone(),
        status: task.status.clone(),
        result_summary: task.result_summary.clone(),
        error_message: task.error_message.clone(),
        output_path: task.output_path.clone(),
    };

    app.emit(BACKGROUND_AGENT_COMPLETE_EVENT, payload)
        .map_err(|e| e.to_string())
}

/// 发送后台 Agent 更新事件
pub fn emit_background_agent_update(
    app: &AppHandle,
    task: &BackgroundAgentTask,
) -> Result<(), String> {
    app.emit(BACKGROUND_AGENT_UPDATE_EVENT, task.clone())
        .map_err(|e| e.to_string())
}

/// 生成后台 Agent 输出文件路径
pub fn get_background_agent_output_path(
    app: &AppHandle,
    session_id: &str,
    task_id: &str,
) -> Result<PathBuf, String> {
    let base_dir: PathBuf = app
        .path()
        .app_data_dir()
        .map_err(|e: tauri::Error| e.to_string())?
        .join("background-agents")
        .join(session_id);

    std::fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;

    Ok(base_dir.join(format!("{}.md", task_id)))
}

/// 全局后台 Agent 管理器实例
use std::sync::OnceLock;
static BACKGROUND_AGENT_MANAGER: OnceLock<BackgroundAgentManager> = OnceLock::new();

/// 获取全局后台 Agent 管理器
pub fn get_background_agent_manager() -> &'static BackgroundAgentManager {
    BACKGROUND_AGENT_MANAGER.get_or_init(BackgroundAgentManager::new)
}
