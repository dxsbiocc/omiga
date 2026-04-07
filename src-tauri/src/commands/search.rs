//! Search commands - Grep and Glob

use super::CommandResult;
use serde::Serialize;

/// Search for patterns in files
#[tauri::command]
pub async fn grep_files(
    _pattern: String,
    _path_pattern: Option<String>,
    _case_insensitive: Option<bool>,
    _max_results: Option<usize>,
) -> CommandResult<GrepResponse> {
    // TODO: Implement grep search
    Ok(GrepResponse {
        matches: vec![],
        files_searched: 0,
        truncated: false,
    })
}

/// Find files matching a glob pattern
#[tauri::command]
pub async fn glob_files(
    _pattern: String,
    _max_results: Option<usize>,
    _include_hidden: Option<bool>,
) -> CommandResult<GlobResponse> {
    // TODO: Implement glob search
    Ok(GlobResponse {
        matches: vec![],
        truncated: false,
    })
}

/// A grep match
#[derive(Debug, Serialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
}

/// Response from grep
#[derive(Debug, Serialize)]
pub struct GrepResponse {
    pub matches: Vec<GrepMatch>,
    pub files_searched: usize,
    pub truncated: bool,
}

/// A glob match
#[derive(Debug, Serialize)]
pub struct GlobMatch {
    pub path: String,
    pub is_file: bool,
    pub size: u64,
}

/// Response from glob
#[derive(Debug, Serialize)]
pub struct GlobResponse {
    pub matches: Vec<GlobMatch>,
    pub truncated: bool,
}
