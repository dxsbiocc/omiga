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

                app_handle.manage(app_state);

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
            commands::chat::cancel_stream,
            commands::chat::cancel_session_rounds,
            commands::chat::set_api_key,
            commands::chat::get_api_key_status,
            commands::chat::set_llm_config,
            commands::chat::get_llm_config_state,
            commands::chat::set_brave_search_api_key,
            commands::chat::get_brave_search_api_key_state,
            commands::chat::test_model,
            commands::permissions::get_omiga_permission_denies,
            commands::permissions::save_omiga_permission_denies,
            app_state::get_app_state_snapshot,
            commands::tools::execute_tool,
            commands::session::list_sessions,
            commands::session::load_session,
            commands::session::save_session,
            commands::session::create_session,
            commands::session::delete_session,
            commands::session::rename_session,
            commands::session::update_session_project_path,
            commands::session::save_message,
            commands::session::clear_session_messages,
            commands::session::get_setting,
            commands::session::set_setting,
            commands::fs::read_file,
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
            commands::wiki::wiki_get_status,
            commands::wiki::wiki_write_page,
            commands::wiki::wiki_read_page,
            commands::wiki::wiki_delete_page,
            commands::wiki::wiki_list_pages,
            commands::wiki::wiki_write_index,
            commands::wiki::wiki_read_index,
            commands::wiki::wiki_append_log,
            commands::wiki::wiki_read_log,
            commands::wiki::wiki_query,
            commands::wiki::wiki_get_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
