//! Omiga - An IDE-like AI coding assistant built with Tauri
//!
//! Architecture:
//! - commands/: Tauri command handlers (frontend entry points)
//! - domain/: Core business logic (tools, session management, persistence)
//! - infrastructure/: Technical details (filesystem, streaming, git)

pub mod api;
pub mod app_state;
pub mod commands;
pub mod constants;
pub mod domain;
pub mod errors;
pub mod execution;
pub mod infrastructure;
pub mod llm;
pub mod utils;

use app_state::OmigaAppState;
use commands::integrations_settings;
use domain::persistence::SessionRepository;
use tauri::Manager;

/// Run the Tauri application
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Initialize tracing/logging
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            tracing::info!("Omiga starting up...");

            // Get app data directory for database
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            std::fs::create_dir_all(&app_data_dir).expect("Failed to create app data directory");
            match domain::app_data_migration::migrate_legacy_app_data_if_needed(&app_data_dir) {
                Ok(Some(legacy_dir)) => {
                    tracing::info!(
                        "Migrated legacy app data from {:?} to {:?}",
                        legacy_dir,
                        app_data_dir
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Failed to migrate legacy app data: {}", e);
                }
            }

            let db_path = app_data_dir.join("omiga.db");
            tracing::info!("Database path: {:?}", db_path);

            // Initialize database in a blocking task
            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                // Initialize database
                let pool = match domain::persistence::init_db(&db_path).await {
                    Ok(pool) => pool,
                    Err(e) => {
                        tracing::error!("Failed to initialize database: {}", e);
                        panic!("Database initialization failed: {}", e);
                    }
                };

                let repo = SessionRepository::new(pool);
                let app_state = OmigaAppState::new(repo);
                // `permission_*` Tauri commands take `State<Arc<PermissionManager>>`; register the
                // same Arc as held by `OmigaAppState` so chat/tools and IPC approve/deny share one manager.
                let permission_manager = app_state.permission_manager.clone();
                app_handle.manage(permission_manager);
                app_handle.manage(app_state);

                // Load `omiga.yaml` default provider into memory so the first message uses the
                // saved default; per-session choice is restored from SQLite when loading a session.
                {
                    let state = app_handle.state::<OmigaAppState>();
                    let mut g = state.chat.llm_config.lock().await;
                    if g.is_none() {
                        match crate::llm::load_config() {
                            Ok(cfg) if !cfg.api_key.is_empty() => {
                                *g = Some(cfg);
                                if let Ok(cf) = crate::llm::config::load_config_file() {
                                    *state.chat.active_provider_entry_name.lock().await =
                                        cf.default_provider;
                                }
                                tracing::info!(target: "omiga::llm", "Loaded default LLM from config file");
                            }
                            _ => {}
                        }
                    }
                }

                tracing::info!("Database initialized successfully");

                // Warm integrations catalog metadata in the background so opening Settings →
                // Integrations is usually instant without probing remote MCP servers on startup.
                let warm_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let Some(state) = warm_handle.try_state::<OmigaAppState>() else {
                        tracing::warn!("Integrations preload: OmigaAppState not available");
                        return;
                    };
                    integrations_settings::warm_integrations_catalog_cache(&state, "").await;
                });

                // Hourly cleanup of completed/failed background agent tasks older than 1 hour.
                tauri::async_runtime::spawn(async {
                    let mut interval =
                        tokio::time::interval(std::time::Duration::from_secs(3600));
                    loop {
                        interval.tick().await;
                        let removed = crate::domain::agents::background::get_background_agent_manager()
                            .cleanup_old_tasks(3600)
                            .await;
                        if removed > 0 {
                            tracing::debug!("cleanup_old_tasks: removed {} stale tasks", removed);
                        }
                    }
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::send_message,
            commands::chat::research::list_available_agents,
            commands::chat::research::list_agent_roles,
            commands::chat::research::run_research_command,
            commands::chat::research_goal::get_research_goal_status,
            commands::chat::research_goal::run_research_goal_command,
            commands::chat::research_goal::suggest_research_goal_criteria,
            commands::chat::research_goal::test_research_goal_second_opinion_provider,
            commands::chat::research_goal::update_research_goal_criteria,
            commands::chat::research_goal::update_research_goal_settings,
            commands::chat::list_orchestration_events,
            commands::chat::run_mock_orchestration_scenario,
            commands::chat::list_session_background_tasks,
            commands::chat::load_background_agent_transcript,
            commands::chat::cancel_background_agent_task,
            commands::chat::submit_ask_user_answer,
            commands::chat::cancel_stream,
            commands::chat::cancel_session_rounds,
            commands::chat::set_api_key,
            commands::chat::get_api_key_status,
            commands::chat::set_llm_config,
            commands::chat::save_llm_settings_to_config,
            commands::chat::save_global_settings_to_config,
            commands::chat::get_global_settings,
            commands::chat::get_llm_config_state,
            commands::chat::set_tavily_search_api_key,
            commands::chat::get_tavily_search_api_key_state,
            commands::chat::set_web_search_api_keys,
            commands::chat::get_web_search_api_keys_state,
            commands::chat::get_retrieval_source_registry,
            commands::chat::test_model,
            commands::chat::suggest_session_title,
            commands::chat::spawn_session_title_async,
            commands::chat::list_provider_configs,
            commands::chat::switch_provider,
            commands::chat::save_provider_config,
            commands::chat::delete_provider_config,
            commands::chat::quick_switch_provider,
            commands::chat::set_default_provider_config,
            commands::chat::run_agent_schedule,
            commands::chat::run_existing_agent_plan,
            commands::chat::cancel_agent_schedule,
            commands::computer_use::computer_use_audit_summary,
            commands::computer_use::computer_use_clear_audit,
            commands::computer_use::computer_use_permission_status,
            commands::computer_use::computer_use_stop_active_run,
            commands::citation::fetch_citation_metadata,
            commands::permissions::permission_check,
            commands::permissions::permission_approve,
            commands::permissions::permission_deny,
            commands::permissions::permission_list_rules,
            commands::permissions::permission_add_rule,
            commands::permissions::permission_delete_rule,
            commands::permissions::permission_get_recent_denials,
            commands::permissions::permission_update_rule,
            commands::permissions::permission_set_default_mode,
            commands::permissions::permission_get_approval_status,
            commands::permissions::permission_clear_session_approvals,
            app_state::get_app_state_snapshot,
            commands::tools::execute_tool,
            commands::session::list_sessions,
            commands::session::search_sessions,
            commands::session::load_session,
            commands::session::load_more_messages,
            commands::session::save_session,
            commands::session::create_session,
            commands::session::delete_session,
            commands::session::rename_session,
            commands::session::update_session_project_path,
            commands::session::save_message,
            commands::session::clear_session_messages,
            commands::session::refresh_session_mcp_connections,
            commands::session::get_mcp_connection_stats,
            commands::session::prewarm_session,
            commands::session::get_session_config,
            commands::session::save_session_config_command,
            commands::session::get_runtime_constraints_config,
            commands::session::save_project_runtime_constraints_config,
            commands::session::save_session_runtime_constraints_config,
            commands::session::get_runtime_constraint_trace,
            commands::session::list_runtime_constraint_trace_rounds,
            commands::session::summarize_runtime_constraint_trace,
            commands::session::get_setting,
            commands::session::set_setting,
            commands::fs::read_file,
            commands::fs::read_file_bytes,
            commands::fs::read_file_bytes_fast,
            commands::fs::read_local_file_for_view,
            commands::fs::write_file,
            commands::fs::read_image_base64,
            commands::fs::list_directory,
            commands::fs::create_directory,
            commands::fs::create_file,
            commands::fs::delete_fs_entry,
            commands::fs::rename_fs_entry,
            commands::ssh_fs::ssh_get_home_directory,
            commands::ssh_fs::ssh_list_directory,
            commands::ssh_fs::ssh_read_file,
            commands::ssh_fs::ssh_write_file,
            commands::ssh_fs::ssh_create_directory,
            commands::sandbox_fs::sandbox_list_directory,
            commands::sandbox_fs::sandbox_read_file,
            commands::sandbox_fs::sandbox_write_file,
            commands::local_envs::list_local_venvs,
            commands::fs::agent_tools_directory,
            commands::shell::render_rmarkdown,
            commands::shell::render_quarto,
            commands::terminal::terminal_start,
            commands::terminal::terminal_write,
            commands::terminal::terminal_stop,
            commands::notebook::execute_ipynb_cell,
            commands::git_workspace::git_workspace_info,
            commands::search::grep_files,
            commands::search::glob_files,
            commands::claude_import::import_merge_project_mcp_json,
            commands::claude_import::upsert_project_mcp_server,
            commands::claude_import::delete_project_mcp_server,
            commands::claude_import::import_skills_from_directory,
            commands::claude_import::import_claude_default_user_skills,
            commands::claude_import::list_omiga_imported_skills,
            commands::claude_import::remove_omiga_imported_skill,
            commands::claude_import::get_claude_default_paths,
            commands::integrations_settings::list_available_skills,
            commands::integrations_settings::get_integrations_catalog,
            commands::integrations_settings::verify_mcp_server,
            commands::integrations_settings::start_mcp_oauth_login,
            commands::integrations_settings::poll_mcp_oauth_login,
            commands::integrations_settings::logout_mcp_oauth_server,
            commands::integrations_settings::save_integrations_state,
            commands::connectors::list_omiga_connectors,
            commands::connectors::list_omiga_connector_audit_events,
            commands::connectors::set_omiga_connector_enabled,
            commands::connectors::connect_omiga_connector,
            commands::connectors::save_omiga_mail_connector_credentials,
            commands::connectors::disconnect_omiga_connector,
            commands::connectors::test_omiga_connector_connection,
            commands::connectors::start_omiga_connector_login,
            commands::connectors::poll_omiga_connector_login,
            commands::connectors::upsert_omiga_custom_connector,
            commands::connectors::delete_omiga_custom_connector,
            commands::connectors::export_omiga_custom_connectors,
            commands::connectors::import_omiga_custom_connectors,
            commands::plugins::list_omiga_plugin_marketplaces,
            commands::plugins::read_omiga_plugin,
            commands::plugins::install_omiga_plugin,
            commands::plugins::uninstall_omiga_plugin,
            commands::plugins::set_omiga_plugin_enabled,
            commands::plugins::list_omiga_plugin_retrieval_statuses,
            commands::plugins::list_omiga_plugin_process_pool_statuses,
            commands::plugins::clear_omiga_plugin_process_pool,
            commands::plugins::validate_omiga_retrieval_plugin,
            commands::operators::list_operators,
            commands::operators::describe_operator,
            commands::operators::set_operator_enabled,
            commands::operators::run_operator,
            commands::operators::list_operator_runs,
            commands::operators::read_operator_run,
            commands::operators::read_operator_run_log,
            commands::operators::verify_operator_run,
            commands::operators::cleanup_operator_runs,
            commands::learning_proposals::learning_proposal_next,
            commands::learning_proposals::learning_proposal_respond,
            commands::learning_proposals::learning_preference_candidates,
            commands::learning_proposals::learning_preference_candidate_promote,
            commands::extensions::vscode_extensions_dir,
            commands::extensions::install_vscode_extension,
            commands::extensions::list_recommended_vscode_extensions,
            commands::extensions::install_recommended_vscode_extension,
            commands::extensions::uninstall_vscode_extension,
            commands::extensions::list_vscode_extensions,
            commands::extensions::read_vscode_extension_file,

            // Execution environments configuration
            commands::execution_envs::get_execution_envs_config,
            commands::execution_envs::save_execution_envs_config,
            commands::execution_envs::get_modal_config,
            commands::execution_envs::save_modal_config,
            commands::execution_envs::get_daytona_config,
            commands::execution_envs::save_daytona_config,
            commands::execution_envs::get_ssh_configs,
            commands::execution_envs::get_ssh_config,
            commands::execution_envs::save_ssh_config,
            commands::execution_envs::delete_ssh_config,
            commands::execution_envs::is_modal_configured,
            commands::execution_envs::is_daytona_configured,
            commands::execution_envs::get_execution_envs_config_path,
            commands::execution_envs::is_rsync_available,

            commands::memory::memory_get_status,
            commands::memory::memory_build_index,
            commands::memory::memory_update_index,
            commands::memory::memory_query,
            commands::memory::memory_clear_index,
            commands::memory::memory_get_dir,
            commands::memory::memory_get_config,
            commands::memory::memory_set_config,
            commands::memory::memory_detect_version,
            commands::memory::memory_migrate,
            commands::memory::memory_get_unified_status,
            commands::memory::memory_import_to_wiki,
            commands::memory::memory_get_import_extensions,
            commands::memory::crud::memory_list_long_term,
            commands::memory::crud::memory_archive_long_term_entry,
            commands::memory::crud::memory_delete_long_term_entry,
            commands::memory::crud::memory_prune_stale,
            commands::memory::crud::memory_list_sources,
            commands::memory::crud::memory_delete_source,
            commands::memory::crud::memory_get_dossier,
            commands::memory::crud::memory_save_dossier,
            commands::memory::read_user_omiga_file,
            commands::memory::write_user_omiga_file,
            commands::memory::ensure_user_profile_files,
            commands::memory::init_user_context_files,
            commands::ralph::list_ralph_sessions,
            commands::ralph::clear_ralph_session,
            commands::ralph::clear_all_ralph_sessions,
            commands::ralph::list_autopilot_sessions,
            commands::ralph::clear_autopilot_session,
            commands::ralph::clear_all_autopilot_sessions,
            commands::ralph::list_active_mode_lanes,
            commands::ralph::check_ralph_stuck,
            commands::ralph::list_team_sessions,
            commands::ralph::clear_team_session,
            commands::ralph::cancel_all,
            commands::ralph::cancel_team_session,
            commands::blackboard::get_blackboard,
            commands::blackboard::post_blackboard_entry,
            commands::blackboard::clear_blackboard,
            commands::context_snapshot::list_context_snapshots,
            commands::context_snapshot::read_context_snapshot,
            commands::context_snapshot::delete_context_snapshot,
            commands::context_snapshot::clear_all_context_snapshots,
            commands::test_notification,
            commands::get_notification_permission_status,
            commands::request_notification_permission,
            commands::send_notification,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
