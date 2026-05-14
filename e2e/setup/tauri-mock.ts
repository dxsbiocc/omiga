/**
 * Tauri IPC mock for Playwright E2E tests.
 *
 * Injects `window.__TAURI_INTERNALS__` so the app's `hasTauriBridge()` check
 * passes and all `invoke()` calls resolve with sensible default data instead
 * of throwing. Tests run against the Vite dev server without the native binary.
 */

export const TAURI_MOCK_SCRIPT = `
(function () {
  if (window.__TAURI_INTERNALS__) return;

  const sessions = [
    { id: "session-1", name: "Test session", project_path: "", working_directory: "", message_count: 0, created_at: new Date().toISOString(), updated_at: new Date().toISOString() },
  ];

  const defaultHandlers = {
    // Session commands — list_sessions returns SessionSummary[] directly (not wrapped)
    list_sessions: () => sessions,
    search_sessions: () => ({ results: [] }),
    load_session: () => ({
      session: sessions[0],
      messages: [],
      has_more_messages: false,
      active_provider_entry_name: null,
      session_config: null,
    }),
    load_more_messages: () => ({ messages: [], has_more_messages: false }),
    save_session: () => sessions[0],
    create_session: () => ({ ...sessions[0], id: "session-new-" + Date.now() }),
    delete_session: () => null,
    rename_session: () => sessions[0],
    update_session_project_path: () => null,
    save_message: () => null,
    clear_session_messages: () => null,
    get_session_artifacts: () => ([]),
    export_session_markdown: () => "# Session Export\n\nNo messages.",
    refresh_session_mcp_connections: () => null,
    get_mcp_connection_stats: () => ({}),
    prewarm_session: () => null,
    get_session_config: () => null,
    save_session_config_command: () => null,
    get_runtime_constraints_config: () => null,
    save_project_runtime_constraints_config: () => null,
    save_session_runtime_constraints_config: () => null,
    get_runtime_constraint_trace: () => null,
    list_runtime_constraint_trace_rounds: () => [],
    summarize_runtime_constraint_trace: () => null,
    get_setting: () => null,
    set_setting: () => null,

    // Chat / LLM commands
    send_message: () => null,
    cancel_stream: () => null,
    cancel_session_rounds: () => null,
    set_api_key: () => null,
    get_api_key_status: () => ({ has_key: false }),
    set_llm_config: () => null,
    save_llm_settings_to_config: () => null,
    save_global_settings_to_config: () => null,
    get_global_settings: () => ({}),
    get_llm_config_state: () => ({ provider: null, apiKeyPreview: null }),
    set_tavily_search_api_key: () => null,
    get_tavily_search_api_key_state: () => ({ has_key: false }),
    set_web_search_api_keys: () => null,
    get_web_search_api_keys_state: () => ({}),
    get_retrieval_source_registry: () => null,
    test_model: () => ({ ok: true }),
    suggest_session_title: () => null,
    spawn_session_title_async: () => null,
    list_provider_configs: () => ([]),
    switch_provider: () => null,
    save_provider_config: () => null,
    delete_provider_config: () => null,
    quick_switch_provider: () => null,
    set_default_provider_config: () => null,
    run_agent_schedule: () => null,
    run_existing_agent_plan: () => null,
    cancel_agent_schedule: () => null,
    list_orchestration_events: () => ([]),
    run_mock_orchestration_scenario: () => null,
    list_session_background_tasks: () => ([]),
    load_background_agent_transcript: () => ([]),
    cancel_background_agent_task: () => null,
    submit_ask_user_answer: () => null,

    // Research
    list_available_agents: () => ([]),
    list_agent_roles: () => ([]),
    run_research_command: () => null,
    get_research_goal_status: () => null,
    run_research_goal_command: () => null,
    suggest_research_goal_criteria: () => null,
    test_research_goal_second_opinion_provider: () => null,
    update_research_goal_criteria: () => null,
    update_research_goal_settings: () => null,

    // App state
    get_app_state_snapshot: () => ({}),

    // Permissions
    permission_check: () => ({ allowed: true, requires_approval: false }),
    permission_approve: () => null,
    permission_deny: () => null,
    permission_list_rules: () => ([]),
    permission_add_rule: () => null,
    permission_delete_rule: () => null,
    permission_get_recent_denials: () => ([]),
    permission_update_rule: () => null,
    permission_set_default_mode: () => null,
    permission_get_approval_status: () => ({}),
    permission_clear_session_approvals: () => null,

    // Tools / FS
    execute_tool: () => ({}),
    read_file: () => "",
    read_file_bytes: () => null,
    read_file_bytes_fast: () => null,
    read_local_file_for_view: () => null,
    write_file: () => null,
    read_image_base64: () => null,
    list_directory: () => ([]),
    create_directory: () => null,
    create_file: () => null,
    delete_fs_entry: () => null,
    rename_fs_entry: () => null,
    agent_tools_directory: () => "",

    // SSH / Sandbox FS
    ssh_get_home_directory: () => null,
    ssh_list_directory: () => ([]),
    ssh_read_file: () => null,
    ssh_write_file: () => null,
    ssh_create_directory: () => null,
    sandbox_list_directory: () => ([]),
    sandbox_read_file: () => null,
    sandbox_write_file: () => null,

    // Shell / Terminal
    render_rmarkdown: () => null,
    render_quarto: () => null,
    terminal_start: () => null,
    terminal_write: () => null,
    terminal_stop: () => null,

    // Misc
    execute_ipynb_cell: () => null,
    git_workspace_info: () => null,
    grep_files: () => ([]),
    glob_files: () => ([]),
    list_local_venvs: () => ([]),
    fetch_citation_metadata: () => null,

    // Claude import / skills
    import_merge_project_mcp_json: () => null,
    upsert_project_mcp_server: () => null,
    delete_project_mcp_server: () => null,
    import_skills_from_directory: () => null,
    import_claude_default_user_skills: () => null,
    list_omiga_imported_skills: () => ([]),
    remove_omiga_imported_skill: () => null,
    get_claude_default_paths: () => ({}),

    // Integrations
    list_available_skills: () => ([]),
    get_integrations_catalog: () => ({ integrations: [] }),
    verify_mcp_server: () => null,
    start_mcp_oauth_login: () => null,
    poll_mcp_oauth_login: () => null,
    logout_mcp_oauth_server: () => null,
    save_integrations_state: () => null,

    // Connectors
    list_omiga_connectors: () => ([]),
    list_omiga_connector_audit_events: () => ([]),
    set_omiga_connector_enabled: () => null,
    connect_omiga_connector: () => null,
    save_omiga_mail_connector_credentials: () => null,
    disconnect_omiga_connector: () => null,
    test_omiga_connector_connection: () => null,
    start_omiga_connector_login: () => null,
    poll_omiga_connector_login: () => null,
    upsert_omiga_custom_connector: () => null,
    delete_omiga_custom_connector: () => null,
    export_omiga_custom_connectors: () => null,
    import_omiga_custom_connectors: () => null,

    // Plugins
    list_omiga_plugin_marketplaces: () => ([]),
    read_omiga_plugin: () => null,
    install_omiga_plugin: () => null,
    uninstall_omiga_plugin: () => null,
    set_omiga_plugin_enabled: () => null,
    list_omiga_plugin_retrieval_statuses: () => ([]),
    list_omiga_plugin_process_pool_statuses: () => ([]),
    clear_omiga_plugin_process_pool: () => null,
    validate_omiga_retrieval_plugin: () => null,

    // Operators
    list_operators: () => ({ registryPath: "", operators: [], diagnostics: [] }),
    describe_operator: () => null,
    set_operator_enabled: () => null,
    run_operator: () => null,
    list_operator_runs: () => ([]),
    read_operator_run: () => null,
    read_operator_run_log: () => null,
    verify_operator_run: () => null,
    cleanup_operator_runs: () => null,
    save_user_script_operator: () => "/tmp/test-operator.yaml",
    get_user_operators_dir: () => "/tmp/user-operators",

    // Cron jobs
    list_cron_jobs: () => ([]),
    delete_cron_job: () => true,

    // Extensions
    vscode_extensions_dir: () => "",
    install_vscode_extension: () => null,
    list_recommended_vscode_extensions: () => ([]),
    install_recommended_vscode_extension: () => null,
    uninstall_vscode_extension: () => null,
    list_vscode_extensions: () => ([]),
    read_vscode_extension_file: () => null,

    // Execution envs
    get_execution_envs_config: () => ({}),
    save_execution_envs_config: () => null,
    get_modal_config: () => null,
    save_modal_config: () => null,
    get_daytona_config: () => null,
    save_daytona_config: () => null,
    get_ssh_configs: () => ([]),
    get_ssh_config: () => null,
    save_ssh_config: () => null,
    delete_ssh_config: () => null,
    is_modal_configured: () => false,
    is_daytona_configured: () => false,
    get_execution_envs_config_path: () => "",
    is_rsync_available: () => false,

    // Memory
    memory_get_status: () => null,
    memory_build_index: () => null,
    memory_update_index: () => null,
    memory_query: () => ([]),
    memory_clear_index: () => null,
    memory_get_dir: () => "",
    memory_get_config: () => ({}),
    memory_set_config: () => null,
    memory_detect_version: () => null,
    memory_migrate: () => null,
    memory_get_unified_status: () => null,
    memory_import_to_wiki: () => null,
    memory_get_import_extensions: () => ([]),
    memory_list_long_term: () => ([]),
    memory_archive_long_term_entry: () => null,
    memory_delete_long_term_entry: () => null,
    memory_prune_stale: () => null,
    memory_list_sources: () => ([]),
    memory_delete_source: () => null,
    memory_get_dossier: () => null,
    memory_save_dossier: () => null,
    read_user_omiga_file: () => null,
    write_user_omiga_file: () => null,
    ensure_user_profile_files: () => null,
    init_user_context_files: () => null,

    // Ralph / autopilot
    list_ralph_sessions: () => ([]),
    clear_ralph_session: () => null,
    clear_all_ralph_sessions: () => null,
    list_autopilot_sessions: () => ([]),
    clear_autopilot_session: () => null,
    clear_all_autopilot_sessions: () => null,
    list_active_mode_lanes: () => ([]),
    check_ralph_stuck: () => false,
    list_team_sessions: () => ([]),
    clear_team_session: () => null,
    cancel_all: () => null,
    cancel_team_session: () => null,

    // Blackboard / context snapshots
    get_blackboard: () => ([]),
    post_blackboard_entry: () => null,
    clear_blackboard: () => null,
    list_context_snapshots: () => ([]),
    read_context_snapshot: () => null,
    delete_context_snapshot: () => null,
    clear_all_context_snapshots: () => null,

    // Notifications
    test_notification: () => null,
    get_notification_permission_status: () => "granted",
    request_notification_permission: () => null,
    send_notification: () => null,
  };

  let _nextEventId = 1;

  const invoke = function (command, args) {
    // Tauri 2.x event plugin IPC — needed by @tauri-apps/api/event listen/unlisten
    if (command === 'plugin:event|listen') {
      return Promise.resolve(_nextEventId++);
    }
    if (command === 'plugin:event|unlisten') {
      return Promise.resolve(null);
    }
    if (command === 'plugin:event|emit' || command === 'plugin:event|emit_to') {
      return Promise.resolve(null);
    }

    const handler = defaultHandlers[command];
    if (handler) {
      try {
        return Promise.resolve(handler(args));
      } catch (e) {
        return Promise.reject(e);
      }
    }
    console.warn('[tauri-mock] unhandled command:', command, args);
    return Promise.resolve(null);
  };

  // Tauri 2.x checks __TAURI_INTERNALS__.transformCallback
  window.__TAURI_INTERNALS__ = {
    transformCallback: function (cb, once) {
      const id = Math.floor(Math.random() * 2 ** 31);
      window['_' + id] = cb;
      return id;
    },
    invoke: invoke,
    metadata: {},
    plugins: {},
  };

  // @tauri-apps/api/event calls window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener
  // during cleanup (component unmount). Stub it so no TypeError is thrown.
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: function () {},
  };

  // Also expose on window for direct access in tests
  window.__tauriMockInvoke = invoke;
})();
`;
