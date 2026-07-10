//! Slurm-specific operator execution helpers (migrated from execution.rs).

use super::*;

pub(crate) fn operator_uses_slurm_scheduler(
    spec: &OperatorSpec,
    ctx: &crate::domain::tools::ToolContext,
) -> bool {
    if !ctx.execution_environment.trim().eq_ignore_ascii_case("ssh") {
        return false;
    }
    let Some(runtime) = spec.runtime.as_ref() else {
        return false;
    };
    runtime_axis_values(runtime, "scheduler")
        .iter()
        .any(|s| s.eq_ignore_ascii_case("slurm"))
}

pub(crate) struct SlurmExecResult {
    pub(crate) exec_result: crate::execution::ExecResult,
    pub(crate) diagnostic: Option<SacctDiagnostic>,
}

async fn fetch_sacct_diagnostics(
    ctx: &crate::domain::tools::ToolContext,
    job_id: &str,
) -> Option<SacctDiagnostic> {
    let job_id = job_id.trim();
    if job_id.is_empty() {
        return None;
    }
    let job_step = format!("{job_id}.batch");
    let command = format!(
        "sacct -j {} --format=State,ExitCode,MaxRSS,Elapsed,Reason,ReqMem -P --noheader",
        sh_quote(&job_step)
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .ok()?;
    if result.returncode != 0 {
        return None;
    }
    parse_sacct_diagnostic_output(&result.output)
}

pub(crate) fn parse_sacct_diagnostic_output(output: &str) -> Option<SacctDiagnostic> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let fields = line.split('|').collect::<Vec<_>>();
    if fields.len() < 6 {
        return None;
    }
    let state = fields[0].trim().to_string();
    let exit_code = fields[1].trim().to_string();
    let max_rss_kb = parse_sacct_memory_kb(fields[2]);
    let elapsed = fields[3].trim().to_string();
    let reason = clean_sacct_optional(fields[4]);
    let req_mem = clean_sacct_optional(fields[5]);
    let category = sacct_failure_category(&state, &exit_code, reason.as_deref());
    let suggested_action = sacct_suggested_action(category, max_rss_kb, &elapsed);

    Some(SacctDiagnostic {
        state,
        exit_code,
        max_rss_kb,
        elapsed,
        reason,
        req_mem,
        category,
        suggested_action,
    })
}

fn clean_sacct_optional(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value == "-"
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("unknown")
    {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_sacct_memory_kb(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty()
        || value == "-"
        || value.eq_ignore_ascii_case("none")
        || value.eq_ignore_ascii_case("unknown")
    {
        return None;
    }

    let split_at = value
        .char_indices()
        .find_map(|(idx, ch)| (!(ch.is_ascii_digit() || ch == '.')).then_some(idx))
        .unwrap_or(value.len());
    let number = value[..split_at].trim();
    if number.is_empty() {
        return None;
    }
    let numeric = number.parse::<f64>().ok()?;
    if !numeric.is_finite() || numeric < 0.0 {
        return None;
    }

    let suffix = value[split_at..].trim().to_ascii_uppercase();
    let multiplier = match suffix.chars().next().unwrap_or('K') {
        'B' => 1.0 / 1024.0,
        'K' => 1.0,
        'M' => 1024.0,
        'G' => 1024.0 * 1024.0,
        'T' => 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    let kb = (numeric * multiplier).ceil();
    (kb <= u64::MAX as f64).then_some(kb as u64)
}

fn sacct_failure_category(
    state: &str,
    exit_code: &str,
    reason: Option<&str>,
) -> SacctFailureCategory {
    let state = state.to_ascii_uppercase();
    let reason = reason.unwrap_or_default().to_ascii_uppercase();
    if state.contains("OUT_OF_MEMORY") || sacct_exit_signal(exit_code) == Some(9) {
        SacctFailureCategory::Oom
    } else if state.contains("TIMEOUT")
        || state.contains("TIME_LIMIT")
        || reason.contains("TIME_LIMIT")
        || reason.contains("TIMELIMIT")
    {
        SacctFailureCategory::Timeout
    } else if state.contains("CANCELLED") {
        SacctFailureCategory::Cancelled
    } else if exit_code.trim() != "0:0" {
        SacctFailureCategory::FailedExit
    } else {
        SacctFailureCategory::Other
    }
}

fn sacct_exit_signal(exit_code: &str) -> Option<i32> {
    exit_code
        .trim()
        .split_once(':')
        .and_then(|(_, signal)| signal.trim().parse::<i32>().ok())
}

pub(crate) fn sacct_returncode_from_diagnostic(diagnostic: &SacctDiagnostic) -> i32 {
    let (status, signal) = diagnostic
        .exit_code
        .trim()
        .split_once(':')
        .map(|(status, signal)| {
            (
                status.trim().parse::<i32>().unwrap_or(0),
                signal.trim().parse::<i32>().unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    if status != 0 {
        status
    } else if signal != 0 {
        128 + signal
    } else if diagnostic.category != SacctFailureCategory::Other {
        1
    } else {
        0
    }
}

fn sacct_suggested_action(
    category: SacctFailureCategory,
    max_rss_kb: Option<u64>,
    elapsed: &str,
) -> Option<String> {
    match category {
        SacctFailureCategory::Oom => {
            let max_rss_kb = max_rss_kb?;
            let suggested_mb = ((max_rss_kb as u128) * 3).div_ceil(2048);
            Some(format!("Re-run with --mem={}MB", suggested_mb.max(1)))
        }
        SacctFailureCategory::Timeout => {
            let elapsed_secs = parse_sacct_elapsed_secs(elapsed)?;
            Some(format!(
                "Re-run with --time={}",
                format_slurm_duration(elapsed_secs.saturating_mul(2))
            ))
        }
        SacctFailureCategory::Cancelled
        | SacctFailureCategory::FailedExit
        | SacctFailureCategory::Other => None,
    }
}

fn parse_sacct_elapsed_secs(elapsed: &str) -> Option<u64> {
    let elapsed = elapsed.trim();
    if elapsed.is_empty() {
        return None;
    }
    let (days, time) = if let Some((days, time)) = elapsed.split_once('-') {
        (days.trim().parse::<u64>().ok()?, time)
    } else {
        (0, elapsed)
    };
    let parts = time
        .split(':')
        .map(|part| part.trim().parse::<u64>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let seconds = match parts.as_slice() {
        [hours, minutes, seconds] => hours
            .saturating_mul(3600)
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(*seconds),
        [minutes, seconds] => minutes.saturating_mul(60).saturating_add(*seconds),
        [seconds] => *seconds,
        _ => return None,
    };
    Some(days.saturating_mul(86_400).saturating_add(seconds))
}

fn format_slurm_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let hours = remainder / 3600;
    let minutes = (remainder % 3600) / 60;
    let seconds = remainder % 60;
    if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// Submit the operator command via sbatch and poll squeue until completion.
/// Returns a synthetic ExecResult plus optional sacct diagnostics.
pub(crate) async fn execute_via_slurm(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    command: &str,
    walltime_secs: u64,
    cpus: u32,
    operator_id: &str,
    queue_status_sender: Option<OperatorQueueStatusSender>,
) -> Result<SlurmExecResult, OperatorToolError> {
    let safe_id = operator_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let walltime_hhmmss = {
        let h = walltime_secs / 3600;
        let m = (walltime_secs % 3600) / 60;
        let s = walltime_secs % 60;
        format!("{h:02}:{m:02}:{s:02}")
    };
    let script = format!(
        "#!/bin/bash\n#SBATCH --job-name=omiga_{safe_id}\n#SBATCH --output={run_dir}/logs/stdout.txt\n#SBATCH --error={run_dir}/logs/stderr.txt\n#SBATCH --cpus-per-task={cpus}\n#SBATCH --time={walltime_hhmmss}\n\nset +e\ncd {run_dir}\n{command}\necho $? > {run_dir}/logs/exit_code.txt\n",
        run_dir = sh_quote(run_dir),
    );
    // Write sbatch script to remote
    let script_path = format!("{run_dir}/omiga_slurm.sh");
    let write_cmd = format!(
        "cat > {} << 'OMIGA_SBATCH_EOF'\n{}\nOMIGA_SBATCH_EOF\nchmod +x {}",
        sh_quote(&script_path),
        script,
        sh_quote(&script_path)
    );
    execute_env_command(ctx, run_dir, &write_cmd, 30).await?;
    // Submit job
    let submit_cmd = format!("sbatch {}", sh_quote(&script_path));
    let submit_result = execute_env_command(ctx, run_dir, &submit_cmd, 30).await?;
    if submit_result.returncode != 0 {
        return Err(OperatorToolError::new(
            "slurm_submission_failed",
            false,
            format!(
                "sbatch failed with code {}: {}",
                submit_result.returncode,
                submit_result.output.trim()
            ),
        )
        .with_run_dir(run_dir)
        .with_suggested_action("Ensure SLURM is available and sbatch is on PATH."));
    }
    let job_id = submit_result
        .output
        .split('\n')
        .find_map(|line: &str| {
            line.trim()
                .strip_prefix("Submitted batch job ")
                .map(|s| s.trim().to_string())
        })
        .ok_or_else(|| {
            OperatorToolError::new(
                "slurm_job_id_missing",
                false,
                format!(
                    "Could not parse job ID from sbatch output: {}",
                    submit_result.output.trim()
                ),
            )
            .with_run_dir(run_dir)
        })?;
    // Write job ID for provenance
    let record_cmd = format!(
        "echo {} > {}/logs/slurm_job_id.txt",
        sh_quote(&job_id),
        sh_quote(run_dir)
    );
    let _ = execute_env_command(ctx, run_dir, &record_cmd, 10).await;
    // Poll squeue frequently enough for the async operator UI to show live SLURM state.
    let poll_interval = std::time::Duration::from_secs(5);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(walltime_secs + 120);
    loop {
        if std::time::Instant::now() >= deadline {
            // Cancel job on timeout
            let _ =
                execute_env_command(ctx, run_dir, &format!("scancel {}", sh_quote(&job_id)), 10)
                    .await;
            return Err(OperatorToolError::new(
                "slurm_timeout",
                false,
                "SLURM job exceeded walltime limit.",
            )
            .with_run_dir(run_dir));
        }
        let poll_cmd = format!(
            "squeue --noheader -j {} -o '%t' 2>/dev/null || true",
            sh_quote(&job_id)
        );
        let poll = execute_env_command(ctx, run_dir, &poll_cmd, 30).await?;
        let state = poll.output.trim().to_ascii_uppercase();
        if !state.is_empty() {
            if let Some(sender) = queue_status_sender.as_ref() {
                let _ = sender.send((job_id.clone(), state.clone()));
            }
        }
        let failed_state = state.split_whitespace().any(|part| {
            matches!(
                part,
                "F" | "FAILED" | "CA" | "CANCELLED" | "TO" | "TIMEOUT" | "NF" | "NODE_FAIL"
            )
        });
        if failed_state {
            let diagnostic = fetch_sacct_diagnostics(ctx, &job_id).await;
            let returncode = diagnostic
                .as_ref()
                .map(sacct_returncode_from_diagnostic)
                .filter(|code| *code != 0)
                .unwrap_or(1);
            return Ok(SlurmExecResult {
                exec_result: crate::execution::ExecResult {
                    returncode,
                    output: format!("SLURM job {job_id} ended with state: {state}"),
                },
                diagnostic,
            });
        }
        if state.is_empty() {
            // Job finished — read exit code
            let code_cmd = format!(
                "cat {}/logs/exit_code.txt 2>/dev/null || true",
                sh_quote(run_dir)
            );
            let code_result = execute_env_command(ctx, run_dir, &code_cmd, 10).await?;
            let exit_code_text = code_result.output.trim();
            let mut returncode = exit_code_text.parse::<i32>().unwrap_or(0);
            let diagnostic = if returncode != 0 || exit_code_text.is_empty() {
                fetch_sacct_diagnostics(ctx, &job_id).await
            } else {
                None
            };
            if returncode == 0 {
                if let Some(diagnostic) = diagnostic.as_ref() {
                    let sacct_returncode = sacct_returncode_from_diagnostic(diagnostic);
                    if sacct_returncode != 0 {
                        returncode = sacct_returncode;
                    }
                }
            }
            return Ok(SlurmExecResult {
                exec_result: crate::execution::ExecResult {
                    returncode,
                    output: format!("SLURM job {job_id} completed"),
                },
                diagnostic,
            });
        }
        tokio::time::sleep(poll_interval).await;
    }
}
