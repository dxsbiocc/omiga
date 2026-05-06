//! Tauri commands for Omiga operators.

use crate::commands::CommandResult;
use crate::domain::operators::{
    self, OperatorCandidateSummary, OperatorRegistryUpdate, OperatorSpec,
};
use crate::errors::AppError;
use serde::Serialize;

fn operator_error(error: String) -> AppError {
    AppError::Config(error)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorCatalogResponse {
    pub registry_path: String,
    pub operators: Vec<OperatorCandidateSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDescribeResponse {
    pub alias: Option<String>,
    pub exposed: bool,
    pub tool_name: Option<String>,
    pub spec: OperatorSpec,
    pub schema: serde_json::Value,
}

#[tauri::command]
pub async fn list_omiga_operators() -> CommandResult<OperatorCatalogResponse> {
    Ok(OperatorCatalogResponse {
        registry_path: operators::registry_path().to_string_lossy().into_owned(),
        operators: operators::list_operator_summaries(),
    })
}

#[tauri::command]
pub async fn describe_omiga_operator(id: String) -> CommandResult<OperatorDescribeResponse> {
    let (alias, spec) = operators::describe_operator(&id).map_err(|error| {
        AppError::Config(
            serde_json::to_string_pretty(&error).unwrap_or_else(|_| error.message.clone()),
        )
    })?;
    let tool_name = alias
        .as_ref()
        .map(|alias| format!("{}{}", operators::OPERATOR_TOOL_PREFIX, alias));
    Ok(OperatorDescribeResponse {
        exposed: alias.is_some(),
        schema: operators::operator_parameters_schema(&spec),
        alias,
        tool_name,
        spec,
    })
}

#[tauri::command]
pub async fn set_omiga_operator_enabled(update: OperatorRegistryUpdate) -> CommandResult<()> {
    operators::set_operator_enabled(update).map_err(operator_error)
}
