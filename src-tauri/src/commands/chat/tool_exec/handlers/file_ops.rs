use super::super::dispatch::ToolDispatchContext;

pub(super) fn is_file_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "file_read"
            | "read_file"
            | "file_write"
            | "write_file"
            | "file_edit"
            | "edit_file"
            | "str_replace_editor"
            | "glob"
            | "grep"
            | "ripgrep"
            | "apply_patch"
            | "notebook_read"
            | "notebook_edit"
    )
}

pub(super) async fn handle_file_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    super::execute_domain_tool(ctx).await
}
