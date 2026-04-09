//! Search commands — Grep 和 Glob
//!
//! 对外提供结构化搜索接口，复用底层 `ignore` / `regex` / `glob` 库；
//! 与 `domain/tools/grep.rs` 和 `domain/tools/glob.rs` 共享搜索逻辑，
//! 但直接返回结构化结果（而非流）。

use super::CommandResult;
use crate::errors::AppError;
use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;
use std::path::Path;
use walkdir::WalkDir;

// ─── 常量 ─────────────────────────────────────────────────────────────────────

const DEFAULT_MAX_GREP: usize = 1000;
const DEFAULT_MAX_GLOB: usize = 5000;
const MAX_LINE_LEN: usize = 500;

// ─── grep_files ───────────────────────────────────────────────────────────────

/// 在项目文件中搜索正则表达式匹配行
#[tauri::command]
pub async fn grep_files(
    pattern: String,
    project_root: String,
    path_pattern: Option<String>,
    case_insensitive: Option<bool>,
    max_results: Option<usize>,
) -> CommandResult<GrepResponse> {
    let max = max_results.unwrap_or(DEFAULT_MAX_GREP).min(5000).max(1);
    let ci = case_insensitive.unwrap_or(false);
    let root = project_root.clone();
    let pp = path_pattern.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_grep(&root, &pattern, pp.as_deref(), ci, max)
    })
    .await
    .map_err(|e| AppError::Unknown(format!("grep task error: {}", e)))?
    .map_err(|e| AppError::Unknown(e))?;

    Ok(result)
}

fn run_grep(
    project_root: &str,
    pattern: &str,
    path_pattern: Option<&str>,
    case_insensitive: bool,
    max_results: usize,
) -> Result<GrepResponse, String> {
    let root = Path::new(project_root);

    // 编译正则
    let re_pattern = if case_insensitive {
        format!("(?i){}", pattern)
    } else {
        pattern.to_string()
    };
    let regex = Regex::new(&re_pattern).map_err(|e| format!("invalid regex: {}", e))?;

    // 构建路径过滤 glob
    let glob_filter = if let Some(pp) = path_pattern {
        let mut builder = GlobSetBuilder::new();
        for part in pp.split([',', ' ']).filter(|s| !s.is_empty()) {
            builder.add(Glob::new(part).map_err(|e| format!("invalid glob '{}': {}", part, e))?);
        }
        Some(builder.build().map_err(|e| e.to_string())?)
    } else {
        None
    };

    let mut matches = Vec::new();
    let mut files_searched: usize = 0;
    let mut truncated = false;

    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .build();

    'outer: for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // 路径过滤
        if let Some(ref gs) = glob_filter {
            let rel = path.strip_prefix(root).unwrap_or(path);
            if !gs.is_match(rel) {
                continue;
            }
        }

        // 跳过二进制/大文件
        if should_skip(path) {
            continue;
        }

        files_searched += 1;

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // 跳过无法读取的文件（如二进制）
        };

        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        for (line_idx, line) in content.lines().enumerate() {
            if let Some(m) = regex.find(line) {
                if matches.len() >= max_results {
                    truncated = true;
                    break 'outer;
                }
                let content_str = if line.len() > MAX_LINE_LEN {
                    format!("{}…", &line[..MAX_LINE_LEN])
                } else {
                    line.to_string()
                };
                matches.push(GrepMatch {
                    file: relative.clone(),
                    line: line_idx + 1,
                    column: m.start() + 1,
                    content: content_str,
                });
            }
        }
    }

    Ok(GrepResponse {
        matches,
        files_searched,
        truncated,
    })
}

fn should_skip(path: &Path) -> bool {
    // 跳过明显的二进制扩展名
    const BINARY_EXTS: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg",
        "pdf", "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
        "exe", "dll", "so", "dylib", "a", "lib",
        "wasm", "bin", "dat",
        "ttf", "otf", "woff", "woff2",
        "mp3", "mp4", "avi", "mov", "mkv",
    ];
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if BINARY_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
            return true;
        }
    }
    // 跳过超大文件（> 2 MB）
    if let Ok(meta) = path.metadata() {
        if meta.len() > 2 * 1024 * 1024 {
            return true;
        }
    }
    false
}

// ─── glob_files ───────────────────────────────────────────────────────────────

/// 在项目中按 glob 模式查找文件
#[tauri::command]
pub async fn glob_files(
    pattern: String,
    project_root: String,
    max_results: Option<usize>,
    include_hidden: Option<bool>,
) -> CommandResult<GlobResponse> {
    let max = max_results.unwrap_or(DEFAULT_MAX_GLOB).min(10_000).max(1);
    let hidden = include_hidden.unwrap_or(false);

    let result = tokio::task::spawn_blocking(move || {
        run_glob(&project_root, &pattern, max, hidden)
    })
    .await
    .map_err(|e| AppError::Unknown(format!("glob task error: {}", e)))?
    .map_err(|e| AppError::Unknown(e))?;

    Ok(result)
}

fn run_glob(
    project_root: &str,
    pattern: &str,
    max_results: usize,
    include_hidden: bool,
) -> Result<GlobResponse, String> {
    let root = Path::new(project_root);

    // 编译 glob 模式
    let glob_pat = glob::Pattern::new(pattern)
        .map_err(|e| format!("invalid glob pattern '{}': {}", pattern, e))?;

    let mut matches = Vec::new();
    let mut truncated = false;

    let walker = WalkDir::new(root).follow_links(false).max_depth(100);

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        // 跳过隐藏文件
        if !include_hidden {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }
        }

        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        if glob_pat.matches(&relative) {
            let (is_file, size) = match path.metadata() {
                Ok(m) => (m.is_file(), m.len()),
                Err(_) => continue,
            };
            matches.push(GlobMatch {
                path: relative,
                is_file,
                size,
            });
            if matches.len() >= max_results {
                truncated = true;
                break;
            }
        }
    }

    Ok(GlobResponse { matches, truncated })
}

// ─── 结构体 ────────────────────────────────────────────────────────────────────

/// 单条 grep 匹配
#[derive(Debug, Serialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
}

/// grep_files 响应
#[derive(Debug, Serialize)]
pub struct GrepResponse {
    pub matches: Vec<GrepMatch>,
    pub files_searched: usize,
    pub truncated: bool,
}

/// 单条 glob 匹配
#[derive(Debug, Serialize)]
pub struct GlobMatch {
    pub path: String,
    pub is_file: bool,
    pub size: u64,
}

/// glob_files 响应
#[derive(Debug, Serialize)]
pub struct GlobResponse {
    pub matches: Vec<GlobMatch>,
    pub truncated: bool,
}
