//! Error types for Omiga
//!
//! All errors implement Serialize to cross the Tauri IPC boundary.
//! They are grouped by domain for type-safe error handling in the frontend.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Top-level application error
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "details")]
pub enum AppError {
    #[error("Chat error: {0}")]
    Chat(ChatError),

    #[error("Tool error: {0}")]
    Tool(ToolError),

    #[error("File system error: {0}")]
    Fs(FsError),

    #[error("Session error: {0}")]
    Session(SessionError),

    #[error("API error: {0}")]
    Api(ApiError),

    #[error("Search error: {0}")]
    Search(SearchError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Persistence error: {0}")]
    Persistence(String),

    #[error("Resource not found: {resource}")]
    NotFound { resource: String },

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Historical name used by `commands/*` and Tauri handlers; same as [`AppError`].
pub type OmigaError = AppError;

/// Chat/LLM interaction errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", content = "message")]
pub enum ChatError {
    #[error("API key not configured")]
    ApiKeyMissing,

    #[error("Network error: {0}")]
    Network(String),

    #[error("Rate limited. Retry after: {retry_after}s")]
    RateLimited { retry_after: u64 },

    #[error("Streaming error: {0}")]
    StreamError(String),

    #[error("Invalid response from API: {0}")]
    InvalidResponse(String),

    #[error("Request timeout")]
    Timeout,

    #[error("Model error: {0}")]
    Model(String),
}

/// Tool execution errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum ToolError {
    #[error("Unknown tool: {name}")]
    UnknownTool { name: String },

    #[error("Invalid arguments: {message}")]
    InvalidArguments { message: String },

    #[error("Tool execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("Tool was cancelled")]
    Cancelled,

    #[error("Tool timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("Permission denied: {action}")]
    PermissionDenied { action: String },
}

/// File system operation errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum FsError {
    #[error("File not found: {path}")]
    NotFound { path: String },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },

    #[error("File too large: {path} ({size} bytes, max {max} bytes)")]
    FileTooLarge { path: String, size: u64, max: u64 },

    #[error("Invalid path: {path}")]
    InvalidPath { path: String },

    #[error("Path traversal detected: {path}")]
    PathTraversal { path: String },

    #[error("Conflict detected. Expected hash: {expected}, current: {current}")]
    ConflictDetected {
        path: String,
        expected: String,
        current: String,
    },

    #[error("Binary file not displayable: {path}")]
    BinaryFile { path: String },

    #[error("IO error: {message}")]
    IoError { message: String },
}

/// Session management errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum SessionError {
    #[error("Session not found: {id}")]
    NotFound { id: String },

    #[error("Database error: {message}")]
    Database { message: String },

    #[error("Session already exists: {id}")]
    AlreadyExists { id: String },

    #[error("Invalid session data: {message}")]
    InvalidData { message: String },

    #[error("Failed to persist session: {message}")]
    PersistFailed { message: String },
}

/// Claude API errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum ApiError {
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },

    #[error("Network error: {message}")]
    Network { message: String },

    #[error("API key invalid or missing")]
    Authentication,

    #[error("Request timeout")]
    Timeout,

    #[error("Rate limited")]
    RateLimited,

    #[error("Server error: {message}")]
    Server { message: String },

    #[error("SSE parse error: {message}")]
    SseParse { message: String },

    #[error("Configuration error: {message}")]
    Config { message: String },
}

/// Search (grep/glob) errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum SearchError {
    #[error("Invalid pattern: {pattern}")]
    InvalidPattern { pattern: String },

    #[error("Regex error: {message}")]
    RegexError { message: String },

    #[error("Search timeout")]
    Timeout,

    #[error("Too many results (max: {max})")]
    TooManyResults { max: usize },

    #[error("IO error during search: {message}")]
    IoError { message: String },
}

/// Bash/process execution errors
#[derive(Error, Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum BashError {
    #[error("Command execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("Command timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("Command was cancelled")]
    Cancelled,

    #[error("Command not found: {command}")]
    CommandNotFound { command: String },

    #[error("Working directory not found: {path}")]
    WorkingDirNotFound { path: String },

    #[error("Permission denied: {command}")]
    PermissionDenied { command: String },

    #[error("Dangerous command blocked: {command}")]
    DangerousCommandBlocked { command: String },

    #[error("Non-UTF8 output")]
    NonUtf8Output,

    #[error("Exit code: {code}")]
    NonZeroExit { code: i32, stderr: String },
}

/// Convert from std::io::Error to FsError
impl From<std::io::Error> for FsError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => FsError::NotFound {
                path: err.to_string(),
            },
            std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied {
                path: err.to_string(),
            },
            _ => FsError::IoError {
                message: err.to_string(),
            },
        }
    }
}

/// Convert from anyhow::Error to AppError
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Unknown(err.to_string())
    }
}

/// Convert from ApiError to AppError
impl From<ApiError> for AppError {
    fn from(err: ApiError) -> Self {
        AppError::Api(err)
    }
}

/// Convert from ToolError to AppError
impl From<ToolError> for AppError {
    fn from(err: ToolError) -> Self {
        AppError::Tool(err)
    }
}

/// Convert from reqwest::Error to ApiError
impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            ApiError::Timeout
        } else if err.is_connect() {
            ApiError::Network {
                message: err.to_string(),
            }
        } else {
            ApiError::Http {
                status: err.status().map(|s| s.as_u16()).unwrap_or(0),
                message: err.to_string(),
            }
        }
    }
}

/// Convert from sqlx::Error to SessionError
impl From<sqlx::Error> for SessionError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => SessionError::NotFound {
                id: "unknown".to_string(),
            },
            _ => SessionError::Database {
                message: err.to_string(),
            },
        }
    }
}

/// Helper to convert bash errors to tool errors
impl From<BashError> for ToolError {
    fn from(err: BashError) -> Self {
        ToolError::ExecutionFailed {
            message: err.to_string(),
        }
    }
}

/// Helper to convert fs errors to tool errors
impl From<FsError> for ToolError {
    fn from(err: FsError) -> Self {
        ToolError::ExecutionFailed {
            message: err.to_string(),
        }
    }
}

/// Helper to convert search errors to tool errors
impl From<SearchError> for ToolError {
    fn from(err: SearchError) -> Self {
        ToolError::ExecutionFailed {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
