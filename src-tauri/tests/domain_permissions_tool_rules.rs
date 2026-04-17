//! Permission tool rules tests

use omiga_lib::domain::permissions::{
    blanket_deny_rule_matches, canonical_permission_tool_name,
    load_merged_permission_deny_rule_entries, matching_deny_entry,
    permission_rule_value_from_string, DenyRuleEntry,
};
use std::fs;
use std::path::PathBuf;

#[test]
fn parser_plain_and_parens() {
    let v = permission_rule_value_from_string("Bash");
    assert_eq!(v.tool_name, "Bash");
    assert!(v.rule_content.is_none());

    let v = permission_rule_value_from_string("Bash(npm install)");
    assert_eq!(v.tool_name, "Bash");
    assert_eq!(v.rule_content.as_deref(), Some("npm install"));
}

#[test]
fn blanket_ignores_content_rules() {
    let r = permission_rule_value_from_string("Bash(rm:*)");
    assert!(!blanket_deny_rule_matches(&r, "bash"));
}

#[test]
fn bash_alias_matches() {
    let r = permission_rule_value_from_string("Bash");
    assert!(blanket_deny_rule_matches(&r, "bash"));
}

#[test]
fn mcp_server_rule() {
    let r = permission_rule_value_from_string("mcp__figma");
    assert!(blanket_deny_rule_matches(
        &r,
        "mcp__figma__get_design_context"
    ));
    assert!(!blanket_deny_rule_matches(&r, "mcp__other__x"));
}

#[test]
fn mcp_server_wildcard_rule() {
    let r = permission_rule_value_from_string("mcp__figma__*");
    assert!(blanket_deny_rule_matches(
        &r,
        "mcp__figma__get_design_context"
    ));
}

#[test]
fn matching_deny_entry_finds_source() {
    let entries = vec![DenyRuleEntry {
        source: PathBuf::from("/tmp/settings.json"),
        rule: "Bash".to_string(),
    }];
    let hit = matching_deny_entry("bash", &entries).unwrap();
    assert_eq!(hit.rule, "Bash");
    assert_eq!(hit.source, PathBuf::from("/tmp/settings.json"));
}

#[test]
fn canonical_maps_tool_enum_display_names() {
    assert_eq!(canonical_permission_tool_name("FileRead"), "file_read");
    assert_eq!(
        canonical_permission_tool_name("ListMcpResources"),
        "list_mcp_resources"
    );
}

// File-backed loader tests (unique rule markers so real `~/.claude` merge does not break assertions).

#[test]
fn load_merged_entries_from_project_settings_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).expect("mkdir");
    let path = claude.join("settings.json");
    fs::write(
        &path,
        r#"{"permissions":{"deny":["__OMIGA_LOADER_A__","Read"]}}"#,
    )
    .expect("write");
    let entries = load_merged_permission_deny_rule_entries(dir.path());
    assert!(
        entries
            .iter()
            .any(|e| { e.rule == "__OMIGA_LOADER_A__" && e.source == path }),
        "expected project rule with source path, got {:?}",
        entries
    );
    assert!(entries.iter().any(|e| e.rule == "Read"));
}

#[test]
fn load_merges_settings_and_settings_local() {
    let dir = tempfile::tempdir().expect("tempdir");
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).expect("mkdir");
    fs::write(
        claude.join("settings.json"),
        r#"{"permissions":{"deny":["__OMIGA_MERGE_A__"]}}"#,
    )
    .expect("write");
    fs::write(
        claude.join("settings.local.json"),
        r#"{"permissions":{"deny":["__OMIGA_MERGE_B__"]}}"#,
    )
    .expect("write");
    let entries = load_merged_permission_deny_rule_entries(dir.path());
    assert!(entries.iter().any(|e| e.rule == "__OMIGA_MERGE_A__"));
    assert!(entries.iter().any(|e| e.rule == "__OMIGA_MERGE_B__"));
}

#[test]
fn load_trims_rules_and_skips_empty_strings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).expect("mkdir");
    fs::write(
        claude.join("settings.json"),
        r#"{"permissions":{"deny":["  ","  __OMIGA_TRIM__  "]}}"#,
    )
    .expect("write");
    let entries = load_merged_permission_deny_rule_entries(dir.path());
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.source == claude.join("settings.json"))
            .count(),
        1
    );
    assert!(entries.iter().any(|e| e.rule == "__OMIGA_TRIM__"));
}

#[test]
fn load_invalid_json_does_not_add_entries_from_that_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).expect("mkdir");
    let bad = claude.join("settings.json");
    fs::write(&bad, "{ not json").expect("write");
    let entries = load_merged_permission_deny_rule_entries(dir.path());
    assert!(
        !entries.iter().any(|e| e.source == bad),
        "broken file should not contribute entries"
    );
}

#[test]
fn load_includes_omiga_permissions_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let omiga = dir.path().join(".omiga");
    fs::create_dir_all(&omiga).expect("mkdir");
    let omiga_path = omiga.join("permissions.json");
    fs::write(&omiga_path, r#"{"deny":["__OMIGA_FILE_ONLY__"]}"#).expect("write");
    let entries = load_merged_permission_deny_rule_entries(dir.path());
    assert!(
        entries
            .iter()
            .any(|e| { e.rule == "__OMIGA_FILE_ONLY__" && e.source == omiga_path }),
        "expected .omiga/permissions.json rule with source path, got {:?}",
        entries
    );
}

#[test]
fn load_merges_claude_settings_and_omiga_permissions_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let claude = dir.path().join(".claude");
    fs::create_dir_all(&claude).expect("mkdir");
    let settings_path = claude.join("settings.json");
    fs::write(
        &settings_path,
        r#"{"permissions":{"deny":["__OMIGA_FROM_CLAUDE__"]}}"#,
    )
    .expect("write");
    let omiga = dir.path().join(".omiga");
    fs::create_dir_all(&omiga).expect("mkdir");
    let omiga_path = omiga.join("permissions.json");
    fs::write(&omiga_path, r#"{"deny":["__OMIGA_FROM_OMIGA_FILE__"]}"#).expect("write");
    let entries = load_merged_permission_deny_rule_entries(dir.path());
    assert!(
        entries
            .iter()
            .any(|e| { e.rule == "__OMIGA_FROM_CLAUDE__" && e.source == settings_path }),
        "expected .claude rule, got {:?}",
        entries
    );
    assert!(
        entries
            .iter()
            .any(|e| { e.rule == "__OMIGA_FROM_OMIGA_FILE__" && e.source == omiga_path }),
        "expected .omiga rule, got {:?}",
        entries
    );
}
