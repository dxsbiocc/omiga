use super::super::dispatch::ToolDispatchContext;

pub(super) fn is_exec_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "bash"
            | "exec"
            | "exec_session"
            | "exec_session_create"
            | "exec_session_write"
            | "task_create"
            | "task_get"
            | "task_update"
            | "task_list"
            | "task_output"
            | "task_stop"
    )
}

pub(super) async fn handle_exec_tool(ctx: &ToolDispatchContext<'_>) -> (String, String, bool) {
    super::execute_domain_tool(ctx).await
}
