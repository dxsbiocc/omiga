//! 权限系统 - 生产级细粒度权限控制

pub mod types;
pub mod manager;
pub mod patterns;
pub mod compat;
pub mod tool_rules;

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
pub use tool_rules::{
    blanket_deny_rule_matches,
    canonical_permission_tool_name,
    filter_tool_schemas_by_deny_rule_entries,
    filter_tool_schemas_by_deny_rules,
    load_merged_permission_deny_rule_entries,
    load_merged_permission_deny_rules,
    matching_deny_entry,
    validate_permission_deny_entries,
    DenyRuleEntry,
    PermissionRuleValue,
    permission_rule_value_from_string,
    OmigaPermissionsFile,
    read_omiga_permissions_file,
    write_omiga_permissions_file,
};
