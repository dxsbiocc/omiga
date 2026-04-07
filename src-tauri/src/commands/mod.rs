//! Tauri command handlers
//!
//! These are the entry points from the frontend. Each command:
//! - Deserializes arguments from frontend
//! - Delegates to domain layer
//! - Returns structured errors for frontend handling

pub mod chat;
pub mod claude_import;
pub mod integrations_settings;
pub mod fs;
pub mod notebook;
pub mod git_workspace;
pub mod permissions;
pub mod search;
pub mod session;
pub mod shell;
pub mod tools;

use crate::errors::AppError;

/// Standard command result type
pub type CommandResult<T> = Result<T, AppError>;
