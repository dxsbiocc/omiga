//! Jupyter notebook helpers for the UI (execute code cells).

use super::CommandResult;
use crate::errors::{AppError, FsError};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

const EXEC_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_IO_CHARS: usize = 512 * 1024;

fn normalize_language(raw: Option<String>) -> String {
    let s = raw
        .as_deref()
        .unwrap_or("python")
        .trim()
        .to_lowercase();
    match s.as_str() {
        "r" | "ir" => "r".to_string(),
        "python" | "ipython" | "py" => "python".to_string(),
        _ => "python".to_string(),
    }
}

#[derive(Debug, Serialize)]
pub struct IpynbCellExecuteResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_IO_CHARS {
        return s.to_string();
    }
    let mut out = s.chars().take(MAX_IO_CHARS).collect::<String>();
    out.push_str("\n… [输出已截断]");
    out
}

fn python_executable() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

/// Escape a string for use inside Python double-quoted source.
fn escape_python_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// IPython-style `!command` per line: each such line becomes `subprocess.run(...)`.
/// Lines starting with `!!` are left as normal Python (IPython re-run magic — not supported).
fn transform_python_shell_magic(source: &str) -> String {
    let mut has_shell = false;
    for line in source.lines() {
        let t = line.trim_start();
        if t.starts_with('!') && !t.starts_with("!!") {
            has_shell = true;
            break;
        }
    }
    if !has_shell {
        return source.to_string();
    }
    let mut out = String::from("import subprocess\n");
    for line in source.lines() {
        let t = line.trim_start();
        if t.starts_with('!') && !t.starts_with("!!") {
            let cmd = t[1..].trim();
            if cmd.is_empty() {
                out.push('\n');
                continue;
            }
            let esc = escape_python_double_quoted(cmd);
            out.push_str(&format!("subprocess.run(\"{}\", shell=True)\n", esc));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Run a code cell in the notebook file's directory (`cwd` = parent of `.ipynb`).
/// - `language`: optional kernel language — `python` (default) or `r` / `ir`.
///   Python: temp `.py` + `python3` / `python`. R: temp `.R` + `Rscript`.
/// - `shell_magic`: when `Some(false)`, skip IPython-style `!` line transform for Python.
#[tauri::command]
pub async fn execute_ipynb_cell(
    notebook_path: String,
    _cell_index: usize,
    source: String,
    language: Option<String>,
    shell_magic: Option<bool>,
) -> CommandResult<IpynbCellExecuteResponse> {
    let lang = normalize_language(language);

    let path_buf = PathBuf::from(&notebook_path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", notebook_path, e),
        })
    })?;

    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: notebook_path.clone(),
        }));
    }

    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "ipynb" {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: format!("not a Jupyter notebook: {notebook_path}"),
        }));
    }

    let cwd = canonical.parent().ok_or_else(|| AppError::Fs(FsError::InvalidPath {
        path: notebook_path.clone(),
    }))?;

    let (tmp_path, exe, err_hint): (PathBuf, &str, &'static str) = if lang == "r" {
        (
            std::env::temp_dir().join(format!("omiga_ipynb_{}.R", Uuid::new_v4())),
            "Rscript",
            "无法运行 Rscript（请确认已安装 R 且在 PATH 中）",
        )
    } else {
        (
            std::env::temp_dir().join(format!("omiga_ipynb_{}.py", Uuid::new_v4())),
            python_executable(),
            "无法运行 Python（请确认已安装且在 PATH 中）",
        )
    };

    let use_shell_magic = shell_magic.unwrap_or(true);
    let source_to_run = if lang == "python" && use_shell_magic {
        transform_python_shell_magic(&source)
    } else {
        source.clone()
    };

    tokio::fs::write(&tmp_path, source_to_run.as_bytes())
        .await
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!("temp file: {e}"),
            })
        })?;

    let run = Command::new(exe)
        .arg(&tmp_path)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output_res = timeout(EXEC_TIMEOUT, run).await;

    let _ = tokio::fs::remove_file(&tmp_path).await;

    let output = output_res
        .map_err(|_| {
            AppError::Fs(FsError::IoError {
                message: "代码单元执行超时（超过 2 分钟）".to_string(),
            })
        })?
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!("{err_hint}: {e}", err_hint = err_hint, e = e),
            })
        })?;

    let exit_code = output.status.code().unwrap_or(-1);
    Ok(IpynbCellExecuteResponse {
        stdout: truncate(&String::from_utf8_lossy(&output.stdout)),
        stderr: truncate(&String::from_utf8_lossy(&output.stderr)),
        exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_magic_inserts_subprocess() {
        let g = transform_python_shell_magic("print(1)\n!echo hi\n");
        assert!(g.contains("import subprocess"));
        assert!(g.contains("subprocess.run"));
        assert!(g.contains("echo hi"));
        assert!(g.contains("print(1)"));
    }

    #[test]
    fn no_bang_unchanged() {
        let a = "x = 1\n";
        assert_eq!(transform_python_shell_magic(a), a);
    }

    #[test]
    fn double_bang_not_shell() {
        let s = "x = '!not magic'\n";
        assert_eq!(transform_python_shell_magic(s), s);
    }
}
