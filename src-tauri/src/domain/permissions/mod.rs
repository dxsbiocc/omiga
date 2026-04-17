//! 权限系统 - 生产级细粒度权限控制

pub mod compat;
pub mod manager;
pub mod patterns;
pub mod tool_rules;
pub mod types;

pub use compat::{
    build_permission_context, check_permissions, AskDecision, DenyDecision,
    PermissionContextCompat, PermissionDecisionCompat,
};
pub use manager::PermissionManager;
pub use tool_rules::{
    blanket_deny_rule_matches, canonical_permission_tool_name,
    filter_tool_schemas_by_deny_rule_entries, filter_tool_schemas_by_deny_rules,
    load_merged_permission_deny_rule_entries, load_merged_permission_deny_rules,
    matching_deny_entry, permission_rule_value_from_string, read_omiga_permissions_file,
    validate_permission_deny_entries, write_omiga_permissions_file, DenyRuleEntry,
    OmigaPermissionsFile, PermissionRuleValue,
};
pub use types::*;
