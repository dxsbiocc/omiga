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
    let s = raw.as_deref().unwrap_or("python").trim().to_lowercase();
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

/// Python scripts do not auto-display the final expression, while notebooks do.
/// Wrap the cell so `1` renders `1` and `df.head()` renders its `repr`, without
/// requiring IPython/Jupyter as a runtime dependency.
/// The optional prelude is executed first in the same globals so earlier cells
/// provide imports/variables without also auto-printing their final expression.
fn wrap_python_cell_for_display(prelude: &str, source: &str) -> String {
    let prelude_literal = serde_json::to_string(prelude).unwrap_or_else(|_| "\"\"".to_string());
    let source_literal = serde_json::to_string(source).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"import ast as __omiga_ast
__omiga_prelude = {prelude_literal}
__omiga_source = {source_literal}
__omiga_globals = globals()
try:
    if __omiga_prelude.strip():
        __omiga_prelude_module = __omiga_ast.parse(__omiga_prelude, filename="<omiga-prelude>", mode="exec")
        __omiga_ast.fix_missing_locations(__omiga_prelude_module)
        exec(compile(__omiga_prelude_module, "<omiga-prelude>", "exec"), __omiga_globals, __omiga_globals)
    __omiga_module = __omiga_ast.parse(__omiga_source, filename="<omiga-cell>", mode="exec")
    if __omiga_module.body and isinstance(__omiga_module.body[-1], __omiga_ast.Expr):
        __omiga_expr = __omiga_ast.Expression(__omiga_module.body.pop().value)
        __omiga_ast.fix_missing_locations(__omiga_module)
        __omiga_ast.fix_missing_locations(__omiga_expr)
        exec(compile(__omiga_module, "<omiga-cell>", "exec"), __omiga_globals, __omiga_globals)
        __omiga_value = eval(compile(__omiga_expr, "<omiga-cell>", "eval"), __omiga_globals, __omiga_globals)
        if __omiga_value is not None:
            print(repr(__omiga_value))
    else:
        __omiga_ast.fix_missing_locations(__omiga_module)
        exec(compile(__omiga_module, "<omiga-cell>", "exec"), __omiga_globals, __omiga_globals)
finally:
    for __omiga_name in (
        "__omiga_ast",
        "__omiga_prelude",
        "__omiga_source",
        "__omiga_globals",
        "__omiga_prelude_module",
        "__omiga_module",
        "__omiga_expr",
        "__omiga_value",
        "__omiga_name",
    ):
        globals().pop(__omiga_name, None)
"#
    )
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
    prelude: Option<String>,
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

    let cwd = canonical.parent().ok_or_else(|| {
        AppError::Fs(FsError::InvalidPath {
            path: notebook_path.clone(),
        })
    })?;

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
    let prelude = prelude.unwrap_or_default();
    let source_to_run = if lang == "python" {
        let prelude_to_run = if use_shell_magic {
            transform_python_shell_magic(&prelude)
        } else {
            prelude.clone()
        };
        let source_to_run = if use_shell_magic {
            transform_python_shell_magic(&source)
        } else {
            source.clone()
        };
        wrap_python_cell_for_display(&prelude_to_run, &source_to_run)
    } else {
        let prelude = prelude.trim_end();
        if prelude.is_empty() {
            source.clone()
        } else {
            format!("{prelude}\n\n{source}")
        }
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

    #[test]
    fn python_display_wrapper_auto_prints_last_expression() {
        let wrapped = wrap_python_cell_for_display("", "x = 1\nx\n");
        assert!(wrapped.contains("eval(compile(__omiga_expr"));
        assert!(wrapped.contains("print(repr(__omiga_value))"));
        assert!(wrapped.contains("\"x = 1\\nx\\n\""));
    }

    #[test]
    fn python_display_wrapper_executes_prelude_before_cell() {
        let wrapped =
            wrap_python_cell_for_display("import os\nbase = '/tmp'\n", "os.path.basename(base)\n");
        assert!(wrapped.contains("<omiga-prelude>"));
        assert!(wrapped.contains("exec(compile(__omiga_prelude_module"));
        assert!(wrapped.contains("\"import os\\nbase = '/tmp'\\n\""));
        assert!(wrapped.contains("\"os.path.basename(base)\\n\""));
    }
}
