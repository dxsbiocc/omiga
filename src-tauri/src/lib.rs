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

                // Warm integrations catalog (MCP tools/list + skills scan) in the background so
                // opening Settings → Integrations is usually instant (cache hit for default cwd).
                let warm_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let Some(state) = warm_handle.try_state::<OmigaAppState>() else {
                        tracing::warn!("Integrations preload: OmigaAppState not available");
                        return;
                    };
                    integrations_settings::warm_integrations_catalog_cache(&state, "").await;
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::send_message,
            commands::chat::list_available_agents,
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
            commands::chat::get_llm_config_state,
            commands::chat::set_brave_search_api_key,
            commands::chat::get_brave_search_api_key_state,
            commands::chat::test_model,
            commands::chat::list_provider_configs,
            commands::chat::switch_provider,
            commands::chat::save_provider_config,
            commands::chat::delete_provider_config,
            commands::chat::quick_switch_provider,
            commands::chat::set_default_provider_config,
            commands::chat::run_agent_schedule,
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
            commands::session::get_setting,
            commands::session::set_setting,
            commands::fs::read_file,
            commands::fs::read_file_bytes_fast,
            commands::fs::write_file,
            commands::fs::read_image_base64,
            commands::fs::list_directory,
            commands::fs::agent_tools_directory,
            commands::shell::render_rmarkdown,
            commands::shell::render_quarto,
            commands::notebook::execute_ipynb_cell,
            commands::git_workspace::git_workspace_info,
            commands::search::grep_files,
            commands::search::glob_files,
            commands::claude_import::import_merge_project_mcp_json,
            commands::claude_import::import_skills_from_directory,
            commands::claude_import::import_claude_default_user_skills,
            commands::claude_import::list_omiga_imported_skills,
            commands::claude_import::remove_omiga_imported_skill,
            commands::claude_import::get_claude_default_paths,
            commands::integrations_settings::get_integrations_catalog,
            commands::integrations_settings::save_integrations_state,

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
            commands::test_notification,
            commands::get_notification_permission_status,
            commands::request_notification_permission,
            commands::send_notification,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
