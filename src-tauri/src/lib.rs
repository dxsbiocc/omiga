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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
