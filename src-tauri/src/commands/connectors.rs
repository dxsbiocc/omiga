//! Tauri commands for Omiga connectors.
//!
//! Connectors are intentionally user-level. The optional `project_root` parameters are accepted
//! only for backward compatibility with older frontend calls; connector state always resolves to
//! `~/.omiga/connectors/config.json`.

use crate::commands::CommandResult;
use crate::domain::connectors::{
    self, ConnectorAuditEvent, ConnectorCatalog, ConnectorConnectRequest,
    ConnectorConnectionTestResult, ConnectorInfo, ConnectorLoginPollResult,
    ConnectorLoginStartResult, CustomConnectorExport, CustomConnectorImportRequest,
    CustomConnectorRequest,
};
use crate::errors::AppError;
use std::path::PathBuf;

fn connector_error(error: String) -> AppError {
    AppError::Config(error)
}

#[tauri::command]
pub fn list_omiga_connectors(_project_root: Option<String>) -> CommandResult<ConnectorCatalog> {
    Ok(connectors::list_connector_catalog())
}

#[tauri::command]
pub fn list_omiga_connector_audit_events(
    connector_id: Option<String>,
    limit: Option<usize>,
    _project_root: Option<String>,
) -> CommandResult<Vec<ConnectorAuditEvent>> {
    connectors::list_connector_audit_events(connector_id.as_deref(), limit).map_err(connector_error)
}

#[tauri::command]
pub fn set_omiga_connector_enabled(
    connector_id: String,
    enabled: bool,
    _project_root: Option<String>,
) -> CommandResult<ConnectorInfo> {
    connectors::set_connector_enabled(&connector_id, enabled).map_err(connector_error)
}

#[tauri::command]
pub fn connect_omiga_connector(
    request: ConnectorConnectRequest,
    _project_root: Option<String>,
) -> CommandResult<ConnectorInfo> {
    connectors::connect_connector(request).map_err(connector_error)
}

#[tauri::command]
pub fn disconnect_omiga_connector(
    connector_id: String,
    _project_root: Option<String>,
) -> CommandResult<ConnectorInfo> {
    connectors::disconnect_connector(&connector_id).map_err(connector_error)
}

#[tauri::command]
pub async fn test_omiga_connector_connection(
    connector_id: String,
    project_root: Option<String>,
) -> CommandResult<ConnectorConnectionTestResult> {
    let project_root = project_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    connectors::test_connector_connection(&connector_id, project_root.as_deref())
        .await
        .map_err(connector_error)
}

#[tauri::command]
pub async fn start_omiga_connector_login(
    connector_id: String,
    _project_root: Option<String>,
) -> CommandResult<ConnectorLoginStartResult> {
    connectors::start_connector_login(&connector_id)
        .await
        .map_err(connector_error)
}

#[tauri::command]
pub async fn poll_omiga_connector_login(
    login_session_id: String,
    _project_root: Option<String>,
) -> CommandResult<ConnectorLoginPollResult> {
    connectors::poll_connector_login(&login_session_id)
        .await
        .map_err(connector_error)
}

#[tauri::command]
pub fn upsert_omiga_custom_connector(
    request: CustomConnectorRequest,
    _project_root: Option<String>,
) -> CommandResult<ConnectorCatalog> {
    connectors::upsert_custom_connector(request).map_err(connector_error)
}

#[tauri::command]
pub fn delete_omiga_custom_connector(
    connector_id: String,
    _project_root: Option<String>,
) -> CommandResult<ConnectorCatalog> {
    connectors::delete_custom_connector(&connector_id).map_err(connector_error)
}

#[tauri::command]
pub fn export_omiga_custom_connectors(
    _project_root: Option<String>,
) -> CommandResult<CustomConnectorExport> {
    Ok(connectors::export_custom_connectors())
}

#[tauri::command]
pub fn import_omiga_custom_connectors(
    request: CustomConnectorImportRequest,
    _project_root: Option<String>,
) -> CommandResult<ConnectorCatalog> {
    connectors::import_custom_connectors(request).map_err(connector_error)
}
