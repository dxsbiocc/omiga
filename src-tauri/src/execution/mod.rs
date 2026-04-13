//! 执行环境模块
//!
//! 将 hermes-agent 的执行环境架构移植到 Rust/Tauri
//!
//! 架构设计：
//! - BaseEnvironment trait: 定义统一执行接口
//! - 每种环境实现 BaseEnvironment
//! - 工厂函数 create_environment 根据配置创建对应环境

pub mod base;
pub mod daytona;
pub mod docker;
pub mod local;
pub mod modal;
pub mod singularity;
pub mod ssh;
pub mod types;

pub use base::{BaseEnvironment, generate_session_id};
pub use types::*;

use daytona::DaytonaEnvironment;
use docker::DockerEnvironment;
use local::LocalEnvironment;
use modal::ModalEnvironment;
use singularity::SingularityEnvironment;
use ssh::SshEnvironment;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 环境实例缓存
///
/// 使用 task_id 作为键缓存环境实例
pub type EnvironmentCache = Arc<Mutex<HashMap<String, Arc<Mutex<dyn BaseEnvironment>>>>>;

/// 创建执行环境
///
/// 根据配置创建对应类型的执行环境
pub async fn create_environment(
    config: EnvironmentConfig,
) -> Result<Arc<Mutex<dyn BaseEnvironment>>, ExecutionError> {
    match config.r#type {
        EnvironmentType::Local => {
            let env = LocalEnvironment::new(
                Some(config.cwd),
                Some(config.timeout),
                Some(config.env),
            )
            .await?;
            Ok(Arc::new(Mutex::new(env)))
        }
        EnvironmentType::Docker => {
            let image = config.image.ok_or_else(|| 
                ExecutionError::InvalidConfig("Docker image is required".to_string())
            )?;
            
            let env = DockerEnvironment::new(
                image,
                Some(config.cwd),
                Some(config.timeout),
                Some(config.cpu),
                Some(config.memory),
                Some(config.disk),
                config.persistent_filesystem,
                config.task_id,
                config.volumes,
                config.forward_env,
                config.env,
                config.network,
            ).await?;
            Ok(Arc::new(Mutex::new(env)))
        }
        EnvironmentType::Modal => {
            let image = config.image.ok_or_else(|| 
                ExecutionError::InvalidConfig("Modal image is required".to_string())
            )?;
            
            let env = ModalEnvironment::new(
                image,
                Some(config.cwd),
                Some(config.timeout),
                config.modal_sandbox_kwargs,
                config.persistent_filesystem,
                config.task_id,
            ).await?;
            Ok(Arc::new(Mutex::new(env)))
        }
        EnvironmentType::Daytona => {
            let image = config.image.clone().unwrap_or_else(|| "ubuntu:22.04".to_string());
            let workspace_id = format!("{}-{}", config.task_id, generate_session_id());
            
            let env = DaytonaEnvironment::new(
                image,
                Some(config.cwd),
                Some(config.timeout),
                config.persistent_filesystem,
                workspace_id,
                None,  // daytona_url from env
                None,  // api_key from env
            ).await?;
            Ok(Arc::new(Mutex::new(env)))
        }
        EnvironmentType::Ssh => {
            let host = config.ssh_host.ok_or_else(|| 
                ExecutionError::InvalidConfig("SSH host is required".to_string())
            )?;
            let user = config.ssh_user.ok_or_else(|| 
                ExecutionError::InvalidConfig("SSH user is required".to_string())
            )?;
            
            let env = SshEnvironment::new(
                host,
                user,
                Some(config.cwd),
                Some(config.timeout),
                config.ssh_port,
                config.ssh_key_path,
            ).await?;
            Ok(Arc::new(Mutex::new(env)))
        }
        EnvironmentType::Singularity => {
            let image = config.image.ok_or_else(|| 
                ExecutionError::InvalidConfig("Singularity image is required".to_string())
            )?;
            
            let env = SingularityEnvironment::new(
                image,
                Some(config.cwd),
                Some(config.timeout),
                config.volumes,
                config.network,
                config.task_id,
            ).await?;
            Ok(Arc::new(Mutex::new(env)))
        }
    }
}

/// 获取环境缓存键
pub fn get_environment_key(env_type: EnvironmentType, task_id: &str) -> String {
    format!("{}:{}", env_type, task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_environment_key() {
        assert_eq!(
            get_environment_key(EnvironmentType::Local, "task1"),
            "local:task1"
        );
        assert_eq!(
            get_environment_key(EnvironmentType::Docker, "default"),
            "docker:default"
        );
    }

    #[tokio::test]
    async fn test_create_environment_local() {
        // Local 环境现在已经实现，应该成功创建
        let config = EnvironmentConfig::default();
        let result = create_environment(config).await;
        assert!(result.is_ok(), "Local environment should be available");
    }

    #[tokio::test]
    async fn test_create_environment_docker_missing_image() {
        // Docker 环境需要 image 参数
        let config = EnvironmentConfig {
            r#type: EnvironmentType::Docker,
            image: None,
            ..Default::default()
        };
        let result = create_environment(config).await;
        match result {
            Err(ExecutionError::InvalidConfig(_)) => (), // expected
            _ => panic!("expected InvalidConfig error for missing Docker image"),
        }
    }
}
