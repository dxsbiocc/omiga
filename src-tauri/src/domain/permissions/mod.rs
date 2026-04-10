//! 权限系统 - 生产级细粒度权限控制

pub mod types;
pub mod manager;
pub mod patterns;
pub mod compat;

pub use types::*;
pub use manager::PermissionManager;
pub use compat::{
    build_permission_context,
    check_permissions,
    PermissionContextCompat,
    PermissionDecisionCompat,
    DenyDecision,
    AskDecision,
};
