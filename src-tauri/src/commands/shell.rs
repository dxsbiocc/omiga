//! UI-triggered shell helpers (e.g. R Markdown `rmarkdown::render`, Quarto CLI).
//!
//! `.qmd` rendering prefers the `quarto` CLI when it is on `PATH` and reports a version ≥
//! [`min_quarto_cli_version`] (default **1.3.0**, override with env **`OMIGA_MIN_QUARTO_VERSION`**
//! e.g. `1.4.0`). Otherwise falls back to `Rscript -e 'quarto::quarto_render(...)'` (R package **quarto**).

use super::CommandResult;
use crate::errors::{AppError, FsError};
use regex::Regex;
use serde::Serialize;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// Knit / render can be slow for large docs.
const RENDER_TIMEOUT: Duration = Duration::from_secs(600);
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_OUTPUT_CHARS: usize = 2 * 1024 * 1024;

/// Default minimum Quarto CLI `(major, minor, patch)` — below this we use R `quarto::quarto_render`.
fn min_quarto_cli_version() -> (u32, u32, u32) {
    std::env::var("OMIGA_MIN_QUARTO_VERSION")
        .ok()
        .and_then(|s| parse_version_triple(&s))
        .unwrap_or((1, 3, 0))
}

fn parse_version_triple(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim().split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c = parts.next()?.parse().ok()?;
    Some((a, b, c))
}

/// First `major.minor.patch` in text (from `quarto --version` output).
fn parse_quarto_version_line(text: &str) -> Option<(u32, u32, u32)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").expect("regex"));
    let cap = re.captures(text)?;
    Some((cap[1].parse().ok()?, cap[2].parse().ok()?, cap[3].parse().ok()?))
}

fn version_lt(a: (u32, u32, u32), b: (u32, u32, u32)) -> bool {
    (a.0, a.1, a.2) < (b.0, b.1, b.2)
}

#[derive(Debug, Serialize)]
pub struct RmdRenderResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

fn canonical_file_with_ext(path: &str, expected_ext: &str) -> Result<PathBuf, AppError> {
    let path_buf = PathBuf::from(path);
    let canonical = path_buf.canonicalize().map_err(|e| {
        AppError::Fs(FsError::IoError {
            message: format!("{}: {}", path, e),
        })
    })?;

    if !canonical.is_file() {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: path.to_string(),
        }));
    }

    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != expected_ext {
        return Err(AppError::Fs(FsError::InvalidPath {
            path: format!("expected .{expected_ext} file: {path}"),
        }));
    }

    Ok(canonical)
}

/// Escape a path for use inside an R double-quoted string literal.
fn r_double_quoted_path(path: &str) -> String {
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return s.to_string();
    }
    let mut out = s.chars().take(MAX_OUTPUT_CHARS).collect::<String>();
    out.push_str("\n… [输出已截断]");
    out
}

/// Run `rmarkdown::render()` on an `.Rmd` file via `Rscript` (R must be on `PATH`).
#[tauri::command]
pub async fn render_rmarkdown(path: String) -> CommandResult<RmdRenderResponse> {
    let canonical = canonical_file_with_ext(&path, "rmd")?;
    let path_str = canonical.to_string_lossy().to_string();
    let r_expr = format!(
        "rmarkdown::render({})",
        r_double_quoted_path(&path_str)
    );

    let run = Command::new("Rscript")
        .arg("-e")
        .arg(&r_expr)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = timeout(RENDER_TIMEOUT, run)
        .await
        .map_err(|_| {
            AppError::Fs(FsError::IoError {
                message: "R Markdown 渲染超时（超过 10 分钟）".to_string(),
            })
        })?
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!(
                    "无法运行 Rscript（请确认已安装 R，且 `rmarkdown` 包可用）: {e}"
                ),
            })
        })?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

    Ok(RmdRenderResponse {
        stdout,
        stderr,
        exit_code,
    })
}

/// Run `quarto render` on a `.qmd` file. Uses the `quarto` CLI when available and new enough;
/// otherwise falls back to `quarto::quarto_render()` via `Rscript`.
#[tauri::command]
pub async fn render_quarto(path: String) -> CommandResult<RmdRenderResponse> {
    let canonical = canonical_file_with_ext(&path, "qmd")?;

    let probe = Command::new("quarto")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match timeout(VERSION_PROBE_TIMEOUT, probe).await {
        Ok(Ok(out)) if out.status.success() => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if let Some(ver) = parse_quarto_version_line(&text) {
                let min = min_quarto_cli_version();
                if version_lt(ver, min) {
                    return render_quarto_via_r(&canonical).await;
                }
            }
        }
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => {
            return render_quarto_via_r(&canonical).await;
        }
        _ => {
            // Version probe failed or timed out — still try `quarto render` below.
        }
    }

    let run = Command::new("quarto")
        .arg("render")
        .arg(&canonical)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match timeout(RENDER_TIMEOUT, run).await {
        Err(_) => {
            return Err(AppError::Fs(FsError::IoError {
                message: "Quarto 渲染超时（超过 10 分钟）".to_string(),
            }));
        }
        Ok(Ok(out)) => out,
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => {
            return render_quarto_via_r(&canonical).await;
        }
        Ok(Err(e)) => {
            return Err(AppError::Fs(FsError::IoError {
                message: format!(
                    "无法启动 quarto（请确认已安装 Quarto CLI 且在 PATH 中）: {e}"
                ),
            }));
        }
    };

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

    Ok(RmdRenderResponse {
        stdout,
        stderr,
        exit_code,
    })
}

/// `quarto::quarto_render()` — requires R and the **quarto** R package (`install.packages("quarto")`).
async fn render_quarto_via_r(canonical: &PathBuf) -> CommandResult<RmdRenderResponse> {
    let path_str = canonical.to_string_lossy().to_string();
    let r_expr = format!(
        "quarto::quarto_render({})",
        r_double_quoted_path(&path_str)
    );

    let run = Command::new("Rscript")
        .arg("-e")
        .arg(&r_expr)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = timeout(RENDER_TIMEOUT, run)
        .await
        .map_err(|_| {
            AppError::Fs(FsError::IoError {
                message: "Quarto 渲染超时（超过 10 分钟）".to_string(),
            })
        })?
        .map_err(|e| {
            AppError::Fs(FsError::IoError {
                message: format!(
                    "未找到 quarto 命令，且无法运行 Rscript（请安装 R，并安装 R 包 quarto：`install.packages(\"quarto\")`）: {e}"
                ),
            })
        })?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

    Ok(RmdRenderResponse {
        stdout,
        stderr,
        exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quarto_version_line_finds_semver() {
        assert_eq!(
            parse_quarto_version_line("quarto 1.5.23\n"),
            Some((1, 5, 23))
        );
        assert_eq!(parse_quarto_version_line("1.2.475"), Some((1, 2, 475)));
    }

    #[test]
    fn version_lt_orders() {
        assert!(version_lt((1, 2, 9), (1, 3, 0)));
        assert!(!version_lt((1, 3, 0), (1, 3, 0)));
    }

    #[test]
    fn parse_version_triple_env_style() {
        assert_eq!(parse_version_triple("1.4.0"), Some((1, 4, 0)));
        assert_eq!(parse_version_triple("1.4"), None);
    }
}
