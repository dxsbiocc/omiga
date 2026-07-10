//! Operator output collection and run registry helpers (migrated from execution.rs).

use super::*;

pub(crate) fn collect_local_outputs(
    spec: &OperatorSpec,
    run_dir: &str,
    out_dir: &Path,
) -> Result<BTreeMap<String, Vec<ArtifactRef>>, OperatorToolError> {
    let canonical_run_dir = Path::new(run_dir).canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator run dir `{run_dir}`: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    let canonical_out_dir = out_dir.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator output dir {}: {err}", out_dir.display()),
        )
        .with_run_dir(run_dir)
    })?;
    if !canonical_out_dir.starts_with(&canonical_run_dir) {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Operator output directory escaped the active session run workspace.",
        )
        .with_run_dir(run_dir)
        .with_suggested_action("Write results only under `${outdir}` for this operator run."));
    }
    let mut outputs = BTreeMap::new();
    for (name, field) in &spec.interface.outputs {
        let Some(pattern) = field.glob.as_deref() else {
            outputs.insert(name.clone(), Vec::new());
            continue;
        };
        let pattern = validate_output_glob_pattern(name, pattern)?.into_owned();
        let search = out_dir.join(&pattern).to_string_lossy().into_owned();
        let mut artifacts = Vec::new();
        for entry in glob::glob(&search).map_err(|err| {
            OperatorToolError::new("artifact_collection_failed", false, err.to_string())
        })? {
            let path = entry.map_err(|err| {
                OperatorToolError::new("artifact_collection_failed", false, err.to_string())
            })?;
            if path.is_file() {
                let canonical_path = path.canonicalize().map_err(|err| {
                    OperatorToolError::new(
                        "artifact_collection_failed",
                        false,
                        format!("resolve output artifact {}: {err}", path.display()),
                    )
                    .with_run_dir(run_dir)
                })?;
                if !canonical_path.starts_with(&canonical_out_dir) {
                    return Err(OperatorToolError::new(
                        "output_validation_failed",
                        false,
                        format!(
                            "Output `{name}` matched artifact outside the active session outdir: {}",
                            path.display()
                        ),
                    )
                    .with_field(format!("outputs.{name}"))
                    .with_run_dir(run_dir)
                    .with_suggested_action(
                        "Write result artifacts only under `${outdir}` for this operator run.",
                    ));
                }
                let size = canonical_path.metadata().ok().map(|m| m.len());
                artifacts.push(ArtifactRef {
                    location: "local".to_string(),
                    server: None,
                    path: canonical_path.to_string_lossy().into_owned(),
                    size,
                    fingerprint: size.map(|s| json!({"mode": "stat", "size": s})),
                });
            }
        }
        if field.required && artifacts.is_empty() {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!("Required output `{name}` matched no files with glob `{pattern}`."),
            )
            .with_field(format!("outputs.{name}")));
        }
        outputs.insert(name.clone(), artifacts);
    }
    Ok(outputs)
}

pub(crate) async fn collect_environment_outputs(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface: &OperatorExecutionSurface,
) -> Result<BTreeMap<String, Vec<ArtifactRef>>, OperatorToolError> {
    let run_dir = surface.run_dir.as_str();
    let mut outputs = BTreeMap::new();
    for (name, field) in &spec.interface.outputs {
        let pattern =
            validate_output_glob_pattern(name, field.glob.as_deref().unwrap_or("*"))?.into_owned();
        let command = format!(
            "find out -type f -path {} -print",
            sh_quote(&format!("out/{pattern}"))
        );
        let result = execute_env_command(ctx, run_dir, &command, 30).await?;
        let mut artifacts = Vec::new();
        for line in result
            .output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let path = if line.starts_with('/') {
                line.to_string()
            } else {
                format!("{run_dir}/{line}")
            };
            artifacts.push(ArtifactRef {
                location: surface.artifact_location().to_string(),
                server: (surface.kind == OperatorExecutionSurfaceKind::Ssh)
                    .then(|| ctx.ssh_server.clone())
                    .flatten(),
                path,
                size: None,
                fingerprint: None,
            });
        }
        if field.required && artifacts.is_empty() {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!("Required output `{name}` matched no remote files with glob `{pattern}`."),
            )
            .with_field(format!("outputs.{name}"))
            .with_run_dir(run_dir));
        }
        outputs.insert(name.clone(), artifacts);
    }
    Ok(outputs)
}

fn safe_operator_export_component(value: &str, fallback: &str) -> String {
    let mut out = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    out = out
        .trim_matches(|ch| matches!(ch, '.' | '_' | '-' | ' '))
        .to_string();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

fn operator_export_relative_path(resolved: &ResolvedOperator, run_id: &str) -> String {
    let alias = safe_operator_export_component(&resolved.alias, "operator");
    let run = safe_operator_export_component(run_id, "run");
    format!("operator-results/{alias}/{run}")
}

fn local_operator_export_dir(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
) -> PathBuf {
    ctx.project_root
        .join(operator_export_relative_path(resolved, run_id))
}

fn environment_operator_export_dir(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
) -> String {
    crate::domain::tools::env_store::remote_path(
        ctx,
        &operator_export_relative_path(resolved, run_id),
    )
}

fn copy_dir_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    if !source_dir.is_dir() {
        return Err(format!(
            "operator output directory {} does not exist",
            source_dir.display()
        ));
    }
    if target_dir.exists() {
        fs::remove_dir_all(target_dir).map_err(|err| {
            format!(
                "remove previous exported results {}: {err}",
                target_dir.display()
            )
        })?;
    }
    fs::create_dir_all(target_dir).map_err(|err| {
        format!(
            "create exported results dir {}: {err}",
            target_dir.display()
        )
    })?;
    for entry in fs::read_dir(source_dir).map_err(|err| {
        format!(
            "read operator output directory {}: {err}",
            source_dir.display()
        )
    })? {
        let entry = entry.map_err(|err| format!("read operator output entry: {err}"))?;
        let source = entry.path();
        let target = target_dir.join(entry.file_name());
        copy_path_recursively(&source, &target)?;
    }
    Ok(())
}

fn copy_path_recursively(source: &Path, target: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|err| format!("read metadata for {}: {err}", source.display()))?;
    if metadata.is_dir() {
        fs::create_dir_all(target)
            .map_err(|err| format!("create exported subdir {}: {err}", target.display()))?;
        for entry in fs::read_dir(source)
            .map_err(|err| format!("read output subdir {}: {err}", source.display()))?
        {
            let entry = entry.map_err(|err| format!("read output subdir entry: {err}"))?;
            copy_path_recursively(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if metadata.is_file() || metadata.file_type().is_symlink() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create exported parent {}: {err}", parent.display()))?;
        }
        fs::copy(source, target).map_err(|err| {
            format!(
                "copy operator result {} to {}: {err}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn export_local_operator_results(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    source_out_dir: &Path,
) -> Result<String, OperatorToolError> {
    let export_dir = local_operator_export_dir(ctx, resolved, run_id);
    copy_dir_contents(source_out_dir, &export_dir).map_err(|err| {
        OperatorToolError::new("result_export_failed", false, err)
            .with_suggested_action("Check session workspace write permissions and retry.")
    })?;
    Ok(export_dir.to_string_lossy().into_owned())
}

pub(crate) async fn export_environment_operator_results(
    ctx: &crate::domain::tools::ToolContext,
    resolved: &ResolvedOperator,
    run_id: &str,
    source_out_dir: &str,
) -> Result<String, OperatorToolError> {
    let export_dir = environment_operator_export_dir(ctx, resolved, run_id);
    let command = format!(
        "if [ ! -d {} ]; then echo 'operator output directory missing' >&2; exit 2; fi\nrm -rf {}\nmkdir -p {}\ncp -R {}/. {}/",
        sh_quote(source_out_dir),
        sh_quote(&export_dir),
        sh_quote(&export_dir),
        sh_quote(source_out_dir),
        sh_quote(&export_dir),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 60).await?;
    if result.returncode != 0 {
        return Err(OperatorToolError::new(
            "result_export_failed",
            false,
            format!(
                "copy operator results to session workspace failed with exit code {}.",
                result.returncode
            ),
        )
        .with_suggested_action("Check session workspace write permissions and retry."));
    }
    Ok(export_dir)
}

pub(crate) fn operator_result_markdown_report(
    resolved: &ResolvedOperator,
    export_dir: Option<&str>,
    outputs: &BTreeMap<String, Vec<ArtifactRef>>,
) -> Option<String> {
    let export_dir = export_dir?.trim();
    if export_dir.is_empty() {
        return None;
    }
    let title = resolved
        .spec
        .metadata
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(resolved.alias.as_str());
    let mut lines = vec![
        format!("# {title}"),
        String::new(),
        "Generated artifacts are exported together in this folder so the final reply can reference static files directly instead of embedding JSON, HTML, or base64 payloads.".to_string(),
        "Use the PNG Markdown image below in the final reply; Omiga renders it through the chat image component. Keep the full path inside `<...>` and do not shorten it to `figure.png`. PNG exports are generated at 300 DPI minimum.".to_string(),
        String::new(),
    ];

    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["figure_png", "plot_png", "png"])
    {
        lines.push(format!("![{title}](<{path}>)"));
        lines.push(String::new());
    }

    let mut primary_links = Vec::new();
    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["figure_pdf", "plot_pdf", "pdf"])
    {
        primary_links.push(format!("[PDF](<{path}>)"));
    }
    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["plot_script", "script", "r_script"])
    {
        primary_links.push(format!("[plot-script.R](<{path}>)"));
    }
    if let Some(path) =
        first_exported_artifact_path(export_dir, outputs, &["rerun_script", "rerun"])
    {
        primary_links.push(format!("[rerun.sh](<{path}>)"));
    }
    primary_links.push(format!("[Result folder](<{export_dir}>)"));
    lines.push(format!("Primary files: {}", primary_links.join(" · ")));
    lines.push(String::new());
    lines.push("## Files".to_string());

    let mut seen = std::collections::BTreeSet::new();
    for (name, artifacts) in outputs {
        for artifact in artifacts {
            let path = exported_artifact_path(export_dir, &artifact.path);
            if !seen.insert(path.clone()) {
                continue;
            }
            let size = artifact
                .size
                .map(|value| format!(" — {} bytes", value))
                .unwrap_or_default();
            lines.push(format!(
                "- `{name}`: [{file}](<{path}>){size}",
                file = exported_artifact_label(&path)
            ));
        }
    }
    if seen.is_empty() {
        lines.push("- No declared output artifacts were exported.".to_string());
    }
    Some(lines.join("\n"))
}

fn first_exported_artifact_path(
    export_dir: &str,
    outputs: &BTreeMap<String, Vec<ArtifactRef>>,
    preferred_output_names: &[&str],
) -> Option<String> {
    for preferred in preferred_output_names {
        if let Some(path) = outputs
            .get(*preferred)
            .and_then(|artifacts| artifacts.first())
            .map(|artifact| exported_artifact_path(export_dir, &artifact.path))
        {
            return Some(path);
        }
    }
    for (name, artifacts) in outputs {
        let lower_name = name.to_ascii_lowercase();
        if preferred_output_names
            .iter()
            .any(|preferred| lower_name.contains(preferred.trim_matches('_')))
        {
            if let Some(path) = artifacts
                .first()
                .map(|artifact| exported_artifact_path(export_dir, &artifact.path))
            {
                return Some(path);
            }
        }
    }
    None
}

fn exported_artifact_path(export_dir: &str, source_path: &str) -> String {
    let file = exported_artifact_label(source_path);
    if export_dir.ends_with('/') || export_dir.ends_with('\\') {
        format!("{export_dir}{file}")
    } else if export_dir.contains('\\') && !export_dir.contains('/') {
        format!("{export_dir}\\{file}")
    } else {
        format!("{export_dir}/{file}")
    }
}

fn exported_artifact_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn write_local_operator_result_readme(
    export_dir: &str,
    markdown: &str,
) -> Result<(), OperatorToolError> {
    fs::write(Path::new(export_dir).join("README.md"), markdown).map_err(|err| {
        OperatorToolError::new(
            "result_export_failed",
            false,
            format!("write result README.md: {err}"),
        )
        .with_suggested_action("Check session workspace write permissions and retry.")
    })
}

pub(crate) async fn write_environment_operator_result_readme(
    ctx: &crate::domain::tools::ToolContext,
    export_dir: &str,
    markdown: &str,
) -> Result<(), OperatorToolError> {
    use base64::{engine::general_purpose, Engine as _};
    let encoded = general_purpose::STANDARD.encode(markdown.as_bytes());
    let target = format!("{}/README.md", export_dir.trim_end_matches('/'));
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        sh_quote(export_dir),
        sh_quote(&encoded),
        sh_quote(&target),
    );
    execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map(|_| ())
        .map_err(|err| {
            OperatorToolError::new("result_export_failed", true, err.message)
                .with_suggested_action("Check session workspace write permissions and retry.")
        })
}

pub(crate) fn read_local_structured_outputs(
    out_dir: &Path,
    run_dir: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let target = out_dir.join(OPERATOR_STRUCTURED_OUTPUTS_FILE);
    if !target.exists() {
        return Ok(None);
    }
    let canonical_run_dir = Path::new(run_dir).canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator run dir `{run_dir}`: {err}"),
        )
        .with_run_dir(run_dir)
    })?;
    let canonical_out_dir = out_dir.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!("resolve operator output dir {}: {err}", out_dir.display()),
        )
        .with_run_dir(run_dir)
    })?;
    let canonical_target = target.canonicalize().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "resolve structured output manifest {}: {err}",
                target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
    })?;
    if !canonical_out_dir.starts_with(&canonical_run_dir)
        || !canonical_target.starts_with(&canonical_out_dir)
    {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Structured output manifest must stay under the active session outdir.",
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write structured metadata only to `${outdir}/outputs.json`."));
    }
    let metadata = canonical_target.metadata().map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "read structured output manifest metadata {}: {err}",
                canonical_target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
    })?;
    if !metadata.is_file() {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Structured output manifest must be a regular JSON file.",
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write a JSON object to `${outdir}/outputs.json`."));
    }
    if metadata.len() > OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "Structured output manifest exceeds {} bytes.",
                OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action(
            "Keep `${outdir}/outputs.json` small and put large payloads in declared output artifacts.",
        ));
    }
    let raw = fs::read_to_string(&canonical_target).map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "read structured output manifest {}: {err}",
                target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
    })?;
    let value = serde_json::from_str::<JsonValue>(&raw).map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "parse structured output manifest {}: {err}",
                target.display()
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write valid JSON object metadata to `${outdir}/outputs.json`.")
    })?;
    validate_structured_outputs_shape(value, run_dir).map(Some)
}

pub(crate) async fn read_environment_structured_outputs(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let missing = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_MISSING__";
    let prefix = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_JSON__";
    let escaped = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_ESCAPED__";
    let not_file = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_NOT_FILE__";
    let too_large = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_TOO_LARGE__";
    let bad_size = "__OMIGA_OPERATOR_STRUCTURED_OUTPUTS_BAD_SIZE__";
    let command = format!(
        r#"target={target}
if [ ! -e "$target" ]; then printf %s {missing}; exit 0; fi
if [ ! -f "$target" ]; then printf %s {not_file}; exit 0; fi
out_root=$(cd out 2>/dev/null && pwd -P) || exit 65
resolved=$(readlink -f "$target" 2>/dev/null || realpath "$target" 2>/dev/null || printf '')
case "$resolved" in "$out_root"/*) ;; *) printf %s {escaped}; exit 0 ;; esac
size=$(wc -c < "$target" | tr -d '[:space:]')
case "$size" in ''|*[!0-9]*) printf %s {bad_size}; exit 0 ;; esac
if [ "$size" -gt {max_bytes} ]; then printf %s {too_large}; exit 0; fi
printf '%s\n' {prefix}
cat "$target""#,
        target = sh_quote(&format!("out/{OPERATOR_STRUCTURED_OUTPUTS_FILE}")),
        missing = sh_quote(missing),
        not_file = sh_quote(not_file),
        escaped = sh_quote(escaped),
        bad_size = sh_quote(bad_size),
        too_large = sh_quote(too_large),
        max_bytes = OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES,
        prefix = sh_quote(prefix),
    );
    let result = execute_env_command(ctx, run_dir, &command, 30).await?;
    if result.returncode != 0 {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "Structured output manifest validation exited with code {}.",
                result.returncode
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Check `${outdir}/outputs.json` and retry."));
    }
    let trimmed = result.output.trim();
    match trimmed {
        value if value == missing => return Ok(None),
        value if value == not_file => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                "Structured output manifest must be a regular JSON file.",
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action("Write a JSON object to `${outdir}/outputs.json`."))
        }
        value if value == escaped => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                "Structured output manifest must stay under the active session outdir.",
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action(
                "Write structured metadata only to `${outdir}/outputs.json`.",
            ))
        }
        value if value == too_large => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!(
                    "Structured output manifest exceeds {} bytes.",
                    OPERATOR_STRUCTURED_OUTPUTS_MAX_BYTES
                ),
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action(
                "Keep `${outdir}/outputs.json` small and put large payloads in declared output artifacts.",
            ))
        }
        value if value == bad_size => {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                "Structured output manifest size could not be validated.",
            )
            .with_field("structuredOutputs")
            .with_run_dir(run_dir)
            .with_suggested_action("Check `${outdir}/outputs.json` and retry."))
        }
        _ => {}
    }
    let Some(raw) = result.output.strip_prefix(prefix) else {
        return Err(OperatorToolError::new(
            "output_validation_failed",
            false,
            "Structured output manifest reader returned an unexpected payload.",
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Check `${outdir}/outputs.json` and retry."));
    };
    let raw = raw
        .strip_prefix("\r\n")
        .or_else(|| raw.strip_prefix('\n'))
        .unwrap_or(raw);
    let value = serde_json::from_str::<JsonValue>(raw).map_err(|err| {
        OperatorToolError::new(
            "output_validation_failed",
            false,
            format!(
                "parse structured output manifest out/{OPERATOR_STRUCTURED_OUTPUTS_FILE}: {err}"
            ),
        )
        .with_field("structuredOutputs")
        .with_run_dir(run_dir)
        .with_suggested_action("Write valid JSON object metadata to `${outdir}/outputs.json`.")
    })?;
    validate_structured_outputs_shape(value, run_dir).map(Some)
}

fn validate_structured_outputs_shape(
    value: JsonValue,
    run_dir: &str,
) -> Result<JsonValue, OperatorToolError> {
    if value.is_object() {
        return Ok(value);
    }
    Err(OperatorToolError::new(
        "output_validation_failed",
        false,
        "Structured output manifest must contain a JSON object.",
    )
    .with_field("structuredOutputs")
    .with_run_dir(run_dir)
    .with_suggested_action("Write object-shaped metadata to `${outdir}/outputs.json`."))
}

pub(crate) fn validate_structured_outputs_against_manifest(
    value: Option<JsonValue>,
    spec: &OperatorSpec,
    run_dir: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let Some(value) = value else {
        if let Some((name, _field)) = spec
            .interface
            .outputs
            .iter()
            .find(|(_name, field)| is_structured_output_field(field) && field.required)
        {
            return Err(OperatorToolError::new(
                "output_validation_failed",
                false,
                format!(
                    "Required structured output `{name}` is missing because `${{outdir}}/{OPERATOR_STRUCTURED_OUTPUTS_FILE}` was not written."
                ),
            )
            .with_field(format!("structuredOutputs.{name}"))
            .with_run_dir(run_dir)
            .with_suggested_action(
                "Write a JSON object to `${outdir}/outputs.json` with all required structured output fields.",
            ));
        }
        return Ok(None);
    };
    let Some(object) = value.as_object() else {
        return validate_structured_outputs_shape(value, run_dir).map(Some);
    };
    for (name, field) in &spec.interface.outputs {
        if !is_structured_output_field(field) {
            continue;
        }
        match object.get(name) {
            Some(field_value) => {
                validate_field_value("structuredOutputs", name, field, field_value).map_err(
                    |error| {
                        if error.run_dir.is_none() {
                            error.with_run_dir(run_dir)
                        } else {
                            error
                        }
                    },
                )?
            }
            None if field.required => {
                return Err(OperatorToolError::new(
                    "output_validation_failed",
                    false,
                    format!("Required structured output `{name}` is missing."),
                )
                .with_field(format!("structuredOutputs.{name}"))
                .with_run_dir(run_dir)
                .with_suggested_action(
                    "Write all required structured output fields in `${outdir}/outputs.json`.",
                ))
            }
            None => {}
        }
    }
    Ok(Some(value))
}

fn is_structured_output_field(field: &OperatorFieldSpec) -> bool {
    field.glob.is_none() && !field.kind.is_path_like()
}

pub(crate) async fn remote_tail(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
) -> Option<String> {
    let command = format!("tail -c 4000 {}", sh_quote(rel));
    execute_env_command(ctx, run_dir, &command, 15)
        .await
        .ok()
        .map(|result| result.output)
}

pub(crate) async fn read_environment_json(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
) -> Result<Option<JsonValue>, OperatorToolError> {
    let target = format!("{}/{}", run_dir.trim_end_matches('/'), rel);
    let command = format!(
        "if [ -f {} ]; then cat {}; else printf %s __OMIGA_OPERATOR_MISSING__; fi",
        sh_quote(&target),
        sh_quote(&target),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await?;
    if result.output.trim() == "__OMIGA_OPERATOR_MISSING__" {
        return Ok(None);
    }
    serde_json::from_str::<JsonValue>(&result.output)
        .map(Some)
        .map_err(|err| {
            OperatorToolError::new(
                "run_state_read_failed",
                true,
                format!("parse remote run JSON {target}: {err}"),
            )
            .with_run_dir(run_dir)
        })
}

pub(crate) async fn read_environment_text_tail(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
    limit_bytes: u64,
) -> Result<Option<String>, OperatorToolError> {
    let target = format!("{}/{}", run_dir.trim_end_matches('/'), rel);
    let command = format!(
        "if [ -f {} ]; then tail -c {} {}; else printf %s __OMIGA_OPERATOR_MISSING__; fi",
        sh_quote(&target),
        limit_bytes,
        sh_quote(&target),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await?;
    if result.output.trim() == "__OMIGA_OPERATOR_MISSING__" {
        Ok(None)
    } else {
        Ok(Some(result.output))
    }
}

pub(crate) async fn update_environment_status(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    status: &str,
    error: Option<&OperatorToolError>,
    metadata: Option<&OperatorRunStatusMetadata>,
) -> Result<(), OperatorToolError> {
    let mut value = json!({
        "status": status,
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "error": error,
    });
    apply_status_metadata(&mut value, metadata);
    write_environment_json(ctx, run_dir, "status.json", &value).await
}

pub(crate) async fn write_environment_json(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
    rel: &str,
    value: &impl Serialize,
) -> Result<(), OperatorToolError> {
    let raw = serde_json::to_vec_pretty(value).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err.to_string())
            .with_run_dir(run_dir)
    })?;
    use base64::{engine::general_purpose, Engine as _};
    let encoded = general_purpose::STANDARD.encode(raw);
    let target = format!("{}/{}", run_dir.trim_end_matches('/'), rel);
    let command = format!(
        "mkdir -p {} && printf %s {} | base64 -d > {}",
        sh_quote(run_dir),
        sh_quote(&encoded),
        sh_quote(&target),
    );
    execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map(|_| ())
        .map_err(|err| {
            OperatorToolError::new("provenance_write_failed", true, err.message)
                .with_run_dir(run_dir)
        })
}

pub(crate) fn update_local_status(
    run_path: &Path,
    status: &str,
    error: Option<&OperatorToolError>,
    metadata: Option<&OperatorRunStatusMetadata>,
) -> Result<(), OperatorToolError> {
    fs::create_dir_all(run_path).map_err(|err| {
        OperatorToolError::new("execution_infra_error", true, err.to_string())
            .with_run_dir(run_path.to_string_lossy())
    })?;
    let mut value = json!({
        "status": status,
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "error": error,
    });
    apply_status_metadata(&mut value, metadata);
    write_json_file(&run_path.join("status.json"), &value).map_err(|err| {
        OperatorToolError::new("provenance_write_failed", false, err)
            .with_run_dir(run_path.to_string_lossy())
    })
}

pub(crate) fn apply_status_metadata(
    value: &mut JsonValue,
    metadata: Option<&OperatorRunStatusMetadata>,
) {
    let Some(metadata) = metadata else {
        return;
    };
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "runId".to_string(),
            JsonValue::String(metadata.run_id.clone()),
        );
        object.insert(
            "location".to_string(),
            JsonValue::String(metadata.location.clone()),
        );
        object.insert(
            "runDir".to_string(),
            JsonValue::String(metadata.run_dir.clone()),
        );
        object.insert(
            "operator".to_string(),
            serde_json::to_value(&metadata.operator).unwrap_or(JsonValue::Null),
        );
        if let Some(run_context) = &metadata.run_context {
            object.insert(
                "runContext".to_string(),
                serde_json::to_value(run_context).unwrap_or(JsonValue::Null),
            );
        }
        if let Some(retry) = &metadata.retry {
            object.insert("attempt".to_string(), json!(retry.attempt));
            object.insert("maxAttempts".to_string(), json!(retry.max_attempts));
            if !retry.previous_errors.is_empty() {
                object.insert(
                    "previousErrors".to_string(),
                    serde_json::to_value(&retry.previous_errors).unwrap_or(JsonValue::Null),
                );
            }
        }
    }
}

pub fn list_local_operator_runs(project_root: &Path, limit: usize) -> Vec<OperatorRunSummary> {
    let runs_root = operator_runs_root(project_root);
    let Ok(entries) = fs::read_dir(&runs_root) else {
        return Vec::new();
    };
    let mut summaries = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            let run_id = entry.file_name().to_string_lossy().into_owned();
            if !is_safe_operator_run_id(&run_id) {
                return None;
            }
            summarize_local_operator_run_dir(&entry.path(), &run_id)
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .summary
            .updated_at
            .cmp(&left.summary.updated_at)
            .then_with(|| right.sort_key.cmp(&left.sort_key))
            .then_with(|| right.summary.run_id.cmp(&left.summary.run_id))
    });
    summaries
        .into_iter()
        .take(limit)
        .map(|item| item.summary)
        .collect()
}

pub async fn list_operator_runs_for_context(
    ctx: &crate::domain::tools::ToolContext,
    limit: usize,
) -> Result<Vec<OperatorRunSummary>, String> {
    let surface = OperatorExecutionSurface::for_runs_root(ctx);
    if surface.kind == OperatorExecutionSurfaceKind::Local {
        return Ok(list_local_operator_runs(&ctx.project_root, limit));
    }

    let command = format!(
        "if [ -d {} ]; then find {} -mindepth 1 -maxdepth 1 -type d -name 'oprun_*' -print; fi",
        sh_quote(&surface.run_dir),
        sh_quote(&surface.run_dir)
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map_err(|err| err.message)?;
    let mut summaries = Vec::new();
    for run_dir in result
        .output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let run_id = run_dir.rsplit('/').next().unwrap_or(run_dir);
        if !is_safe_operator_run_id(run_id) {
            continue;
        }
        let provenance = read_environment_json(ctx, run_dir, "provenance.json")
            .await
            .map_err(|err| err.message)?;
        let status_doc = read_environment_json(ctx, run_dir, "status.json")
            .await
            .map_err(|err| err.message)?;
        let updated_at = status_doc
            .as_ref()
            .and_then(|value| json_string_at(value, &["updatedAt"]))
            .or_else(|| {
                provenance
                    .as_ref()
                    .and_then(|value| json_string_at(value, &["updatedAt"]))
            });
        let sort_key = rfc3339_sort_key(updated_at.as_deref());
        if let Some(summary) = summarize_operator_run_documents(
            run_id,
            surface.artifact_location(),
            run_dir.to_string(),
            Some(format!("{}/provenance.json", run_dir.trim_end_matches('/'))),
            provenance,
            status_doc,
            updated_at,
            sort_key,
        ) {
            summaries.push(summary);
        }
    }
    summaries.sort_by(|left, right| {
        right
            .summary
            .updated_at
            .cmp(&left.summary.updated_at)
            .then_with(|| right.sort_key.cmp(&left.sort_key))
            .then_with(|| right.summary.run_id.cmp(&left.summary.run_id))
    });
    Ok(summaries
        .into_iter()
        .take(limit)
        .map(|item| item.summary)
        .collect())
}

pub fn read_local_operator_run(project_root: &Path, run_id: &str) -> Result<JsonValue, String> {
    let run_id = run_id.trim();
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let runs_root = operator_runs_root(project_root);
    let run_dir = runs_root.join(run_id);
    let canonical_root = runs_root.canonicalize().unwrap_or(runs_root);
    let canonical_run_dir = run_dir
        .canonicalize()
        .map_err(|err| format!("operator run `{run_id}` not found: {err}"))?;
    if !canonical_run_dir.starts_with(&canonical_root) {
        return Err(format!(
            "operator run `{run_id}` is outside the run registry"
        ));
    }
    let provenance = canonical_run_dir.join("provenance.json");
    if provenance.is_file() {
        return read_json_value(&provenance);
    }
    let status = canonical_run_dir.join("status.json");
    if status.is_file() {
        return read_json_value(&status);
    }
    Err(format!(
        "operator run `{run_id}` has no provenance.json or status.json"
    ))
}

pub async fn read_operator_run_for_context(
    ctx: &crate::domain::tools::ToolContext,
    run_id: &str,
) -> Result<OperatorRunDetail, String> {
    let run_id = run_id.trim();
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let surface = OperatorExecutionSurface::for_context(ctx, run_id);
    if surface.kind == OperatorExecutionSurfaceKind::Local {
        let document = read_local_operator_run(&ctx.project_root, run_id)?;
        let source_path = document
            .get("provenancePath")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                operator_run_dir(&ctx.project_root, run_id)
                    .join("status.json")
                    .to_string_lossy()
                    .into_owned()
            });
        return Ok(OperatorRunDetail {
            run_id: run_id.to_string(),
            location: "local".to_string(),
            run_dir: surface.run_dir,
            source_path,
            document,
        });
    }

    let run_dir = surface.run_dir.clone();
    for rel in ["provenance.json", "status.json"] {
        if let Some(document) = read_environment_json(ctx, &run_dir, rel)
            .await
            .map_err(|err| err.message)?
        {
            return Ok(OperatorRunDetail {
                run_id: run_id.to_string(),
                location: surface.artifact_location().to_string(),
                run_dir,
                source_path: format!("{}/{}", surface.run_dir.trim_end_matches('/'), rel),
                document,
            });
        }
    }
    Err(format!(
        "operator run `{run_id}` has no remote provenance.json or status.json at {}",
        surface.run_dir
    ))
}

pub async fn read_operator_run_log_for_context(
    ctx: &crate::domain::tools::ToolContext,
    run_id: &str,
    log_name: &str,
    limit_bytes: u64,
) -> Result<OperatorRunLog, String> {
    let run_id = run_id.trim();
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let normalized_log = match log_name.trim() {
        "stdout" | "stdout.txt" => "stdout",
        "stderr" | "stderr.txt" => "stderr",
        other => return Err(format!("unsupported operator log `{other}`")),
    };
    let rel = format!("logs/{normalized_log}.txt");
    let surface = OperatorExecutionSurface::for_context(ctx, run_id);
    let limit = limit_bytes.clamp(1, 64 * 1024);
    let path = format!("{}/{}", surface.run_dir.trim_end_matches('/'), rel);
    let content = if surface.kind == OperatorExecutionSurfaceKind::Local {
        read_tail_limited(&path, limit as usize).ok_or_else(|| {
            format!("operator run `{run_id}` has no local `{normalized_log}` log at {path}")
        })?
    } else {
        read_environment_text_tail(ctx, &surface.run_dir, &rel, limit)
            .await
            .map_err(|err| err.message)?
            .ok_or_else(|| {
                format!("operator run `{run_id}` has no remote `{normalized_log}` log at {path}")
            })?
    };
    Ok(OperatorRunLog {
        run_id: run_id.to_string(),
        location: surface.artifact_location().to_string(),
        log_name: normalized_log.to_string(),
        path,
        content,
    })
}

pub async fn verify_operator_run_for_context(
    ctx: &crate::domain::tools::ToolContext,
    run_id: &str,
) -> Result<OperatorRunVerification, String> {
    let detail = read_operator_run_for_context(ctx, run_id).await?;
    let mut checks = Vec::new();
    checks.push(OperatorRunCheck {
        name: "run_state_readable".to_string(),
        ok: true,
        severity: "info".to_string(),
        message: "Run status/provenance is readable.".to_string(),
        path: Some(detail.source_path.clone()),
    });

    let status =
        json_string_at(&detail.document, &["status"]).unwrap_or_else(|| "unknown".to_string());
    let status_ok = status == "succeeded";
    checks.push(OperatorRunCheck {
        name: "run_status".to_string(),
        ok: status_ok,
        severity: if status_ok { "info" } else { "error" }.to_string(),
        message: if status_ok {
            "Run status is succeeded.".to_string()
        } else {
            format!("Run status is `{status}`.")
        },
        path: Some(detail.source_path.clone()),
    });

    for log_name in ["stdout", "stderr"] {
        match read_operator_run_log_for_context(ctx, run_id, log_name, 256).await {
            Ok(log) => checks.push(OperatorRunCheck {
                name: format!("{log_name}_log_readable"),
                ok: true,
                severity: "info".to_string(),
                message: format!("{} log is readable.", log_name),
                path: Some(log.path),
            }),
            Err(error) => checks.push(OperatorRunCheck {
                name: format!("{log_name}_log_readable"),
                ok: false,
                severity: "warning".to_string(),
                message: error,
                path: Some(format!(
                    "{}/logs/{log_name}.txt",
                    detail.run_dir.trim_end_matches('/')
                )),
            }),
        }
    }

    let artifacts = output_artifact_paths(&detail.document);
    if artifacts.is_empty() {
        checks.push(OperatorRunCheck {
            name: "output_artifacts_declared".to_string(),
            ok: true,
            severity: "info".to_string(),
            message: "No output artifact refs were declared in this run.".to_string(),
            path: None,
        });
    } else {
        for (output_name, path) in artifacts {
            let check =
                verify_artifact_path_for_context(ctx, &detail.location, &output_name, &path).await;
            checks.push(check);
        }
    }

    let ok = checks
        .iter()
        .filter(|check| check.severity == "error")
        .all(|check| check.ok);
    Ok(OperatorRunVerification {
        run_id: detail.run_id,
        location: detail.location,
        run_dir: detail.run_dir,
        ok,
        checks,
    })
}

pub async fn cleanup_operator_runs_for_context(
    ctx: &crate::domain::tools::ToolContext,
    request: OperatorRunCleanupRequest,
) -> Result<OperatorRunCleanupResult, String> {
    let limit = request.limit.unwrap_or(500).clamp(1, 2_000);
    let surface = OperatorExecutionSurface::for_runs_root(ctx);
    let summaries = list_operator_runs_for_context(ctx, limit).await?;
    let selected = select_operator_cleanup_candidates(&summaries, &request);
    let mut candidates = Vec::new();
    let mut estimated_total = 0_u64;

    for (summary, reason) in selected {
        let estimated_bytes = if surface.kind == OperatorExecutionSurfaceKind::Local {
            Some(local_operator_run_dir_size(&ctx.project_root, &summary.run_id).unwrap_or(0))
        } else {
            estimate_environment_run_dir_size(ctx, &summary.run_dir).await
        };
        if let Some(bytes) = estimated_bytes {
            estimated_total = estimated_total.saturating_add(bytes);
        }
        let mut candidate = OperatorRunCleanupCandidate {
            run_id: summary.run_id.clone(),
            status: summary.status.clone(),
            location: summary.location.clone(),
            run_dir: summary.run_dir.clone(),
            updated_at: summary.updated_at.clone(),
            cache_hit: summary.cache_hit,
            cache_source_run_id: summary.cache_source_run_id.clone(),
            output_count: summary.output_count,
            reason,
            estimated_bytes,
            deleted: false,
            error: None,
        };
        if !request.dry_run {
            let deletion = if surface.kind == OperatorExecutionSurfaceKind::Local {
                delete_local_operator_run_dir(&ctx.project_root, &summary.run_id)
            } else {
                delete_environment_operator_run_dir(ctx, &surface.run_dir, &summary.run_dir).await
            };
            match deletion {
                Ok(()) => candidate.deleted = true,
                Err(error) => candidate.error = Some(error),
            }
        }
        candidates.push(candidate);
    }

    let deleted_count = candidates
        .iter()
        .filter(|candidate| candidate.deleted)
        .count();
    let skipped_count = candidates
        .iter()
        .filter(|candidate| candidate.error.is_some())
        .count();
    Ok(OperatorRunCleanupResult {
        dry_run: request.dry_run,
        location: surface.artifact_location().to_string(),
        runs_root: surface.run_dir,
        scanned_count: summaries.len(),
        matched_count: candidates.len(),
        deleted_count,
        skipped_count,
        estimated_bytes: Some(estimated_total),
        candidates,
    })
}

pub(crate) fn select_operator_cleanup_candidates(
    summaries: &[OperatorRunSummary],
    request: &OperatorRunCleanupRequest,
) -> Vec<(OperatorRunSummary, String)> {
    let keep_latest = request.keep_latest.unwrap_or(25);
    let scoped = summaries
        .iter()
        .filter(|summary| cleanup_request_matches_summary(summary, request))
        .collect::<Vec<_>>();
    let protected = scoped
        .iter()
        .take(keep_latest)
        .map(|summary| summary.run_id.as_str())
        .collect::<HashSet<_>>();
    let mut selected = Vec::new();
    let mut selected_ids = HashSet::new();
    for summary in &scoped {
        if protected.contains(summary.run_id.as_str())
            || !is_terminal_operator_status(&summary.status)
        {
            continue;
        }
        let reason = if request.include_cache_hits && summary.cache_hit == Some(true) {
            Some("cache_hit_record".to_string())
        } else if request.include_failed
            && is_failed_operator_status(&summary.status)
            && run_matches_cleanup_age(summary, request.max_age_days)
        {
            Some("old_failed_run".to_string())
        } else if request.include_succeeded
            && is_succeeded_operator_status(&summary.status)
            && run_matches_cleanup_age(summary, request.max_age_days)
        {
            Some("old_succeeded_run".to_string())
        } else {
            None
        };
        if let Some(reason) = reason {
            selected_ids.insert(summary.run_id.clone());
            selected.push(((*summary).clone(), reason));
        }
    }

    if request.include_cache_hits {
        let selected_sources = selected
            .iter()
            .filter(|(summary, _)| summary.cache_hit != Some(true))
            .map(|(summary, _)| summary.run_id.clone())
            .collect::<HashSet<_>>();
        for summary in &scoped {
            if protected.contains(summary.run_id.as_str())
                || selected_ids.contains(&summary.run_id)
                || summary.cache_hit != Some(true)
            {
                continue;
            }
            if summary
                .cache_source_run_id
                .as_ref()
                .map(|source| selected_sources.contains(source))
                .unwrap_or(false)
            {
                selected_ids.insert(summary.run_id.clone());
                selected.push(((*summary).clone(), "cache_source_cleanup".to_string()));
            }
        }
    }

    let retained_cache_sources = scoped
        .iter()
        .filter(|summary| {
            summary.cache_hit == Some(true) && !selected_ids.contains(&summary.run_id)
        })
        .filter_map(|summary| summary.cache_source_run_id.clone())
        .collect::<HashSet<_>>();
    selected
        .into_iter()
        .filter(|(summary, _)| {
            summary.cache_hit == Some(true) || !retained_cache_sources.contains(&summary.run_id)
        })
        .collect()
}

fn cleanup_request_matches_summary(
    summary: &OperatorRunSummary,
    request: &OperatorRunCleanupRequest,
) -> bool {
    cleanup_text_filter_matches(
        request.operator_id.as_deref(),
        summary.operator_id.as_deref(),
    ) && cleanup_operator_alias_matches(
        request.operator_alias.as_deref(),
        summary.operator_alias.as_deref(),
        summary.operator_id.as_deref(),
    ) && cleanup_text_filter_matches(
        request.operator_version.as_deref(),
        summary.operator_version.as_deref(),
    ) && cleanup_text_filter_matches(
        request.source_plugin.as_deref(),
        summary.source_plugin.as_deref(),
    )
}

fn cleanup_text_filter_matches(filter: Option<&str>, value: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    value.map(str::trim) == Some(filter)
}

fn cleanup_operator_alias_matches(
    filter: Option<&str>,
    alias: Option<&str>,
    operator_id: Option<&str>,
) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    alias.map(str::trim) == Some(filter) || operator_id.map(str::trim) == Some(filter)
}

fn is_terminal_operator_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(
        status.as_str(),
        "succeeded" | "success" | "failed" | "error" | "cancelled" | "timeout" | "timed_out"
    )
}

fn is_failed_operator_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(
        status.as_str(),
        "failed" | "error" | "cancelled" | "timeout" | "timed_out"
    )
}

fn is_succeeded_operator_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(status.as_str(), "succeeded" | "success")
}

fn run_matches_cleanup_age(summary: &OperatorRunSummary, max_age_days: Option<u64>) -> bool {
    let Some(max_age_days) = max_age_days else {
        return true;
    };
    let Some(updated_at) = summary.updated_at.as_deref() else {
        return true;
    };
    let Ok(updated_at) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return true;
    };
    let age = chrono::Utc::now().signed_duration_since(updated_at.with_timezone(&chrono::Utc));
    age.num_seconds() >= (max_age_days as i64).saturating_mul(24 * 60 * 60)
}

pub(crate) fn local_operator_run_dir_size(
    project_root: &Path,
    run_id: &str,
) -> Result<u64, String> {
    let run_dir = safe_local_operator_run_dir(project_root, run_id)?;
    Ok(path_tree_size(&run_dir))
}

fn safe_local_operator_run_dir(project_root: &Path, run_id: &str) -> Result<PathBuf, String> {
    if !is_safe_operator_run_id(run_id) {
        return Err(
            "operator run id must start with `oprun_` and contain only letters, numbers, `_`, or `-`"
                .to_string(),
        );
    }
    let runs_root = operator_runs_root(project_root);
    let run_dir = runs_root.join(run_id);
    let canonical_root = runs_root.canonicalize().unwrap_or(runs_root);
    let canonical_run_dir = run_dir
        .canonicalize()
        .map_err(|err| format!("operator run `{run_id}` not found: {err}"))?;
    if !canonical_run_dir.starts_with(&canonical_root) {
        return Err(format!(
            "operator run `{run_id}` is outside the run registry"
        ));
    }
    Ok(canonical_run_dir)
}

fn path_tree_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| path_tree_size(&entry.path()))
        .fold(0_u64, u64::saturating_add)
}

pub(crate) fn delete_local_operator_run_dir(
    project_root: &Path,
    run_id: &str,
) -> Result<(), String> {
    let run_dir = safe_local_operator_run_dir(project_root, run_id)?;
    fs::remove_dir_all(&run_dir).map_err(|err| {
        format!(
            "delete operator run `{run_id}` at {}: {err}",
            run_dir.display()
        )
    })
}

pub(crate) async fn estimate_environment_run_dir_size(
    ctx: &crate::domain::tools::ToolContext,
    run_dir: &str,
) -> Option<u64> {
    let command = format!(
        "du -sk {} 2>/dev/null | awk '{{print $1}}'",
        sh_quote(run_dir)
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .ok()?;
    result.output.trim().parse::<u64>().ok().map(|kb| kb * 1024)
}

pub(crate) async fn delete_environment_operator_run_dir(
    ctx: &crate::domain::tools::ToolContext,
    runs_root: &str,
    run_dir: &str,
) -> Result<(), String> {
    let normalized_root = runs_root.trim_end_matches('/');
    let normalized_run_dir = run_dir.trim_end_matches('/');
    let run_id = normalized_run_dir.rsplit('/').next().unwrap_or_default();
    if !is_safe_operator_run_id(run_id)
        || !normalized_run_dir.starts_with(&format!("{normalized_root}/oprun_"))
    {
        return Err(format!(
            "refusing to delete operator run outside active run registry: {run_dir}"
        ));
    }
    let command = format!(
        "target={}; root={}; case \"$target\" in \"$root\"/oprun_*) rm -rf -- \"$target\" ;; *) exit 64 ;; esac",
        sh_quote(normalized_run_dir),
        sh_quote(normalized_root),
    );
    let result = execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30)
        .await
        .map_err(|err| err.message)?;
    if result.returncode == 0 {
        Ok(())
    } else {
        Err(format!(
            "remote cleanup command exited with code {}",
            result.returncode
        ))
    }
}

pub(crate) fn output_artifact_paths(document: &JsonValue) -> Vec<(String, String)> {
    let Some(outputs) = json_value_at(document, &["outputs"]).and_then(JsonValue::as_object) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for (name, artifacts) in outputs {
        for artifact in artifacts.as_array().into_iter().flatten() {
            if let Some(path) = json_string_at(artifact, &["path"]) {
                paths.push((name.clone(), path));
            }
        }
    }
    paths
}

pub(crate) async fn verify_artifact_path_for_context(
    ctx: &crate::domain::tools::ToolContext,
    location: &str,
    output_name: &str,
    path: &str,
) -> OperatorRunCheck {
    if location == "local" {
        let metadata = fs::metadata(path).ok();
        let ok = metadata
            .as_ref()
            .map(|metadata| metadata.is_file())
            .unwrap_or(false);
        return OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok,
            severity: if ok { "info" } else { "error" }.to_string(),
            message: if ok {
                format!(
                    "Output artifact `{output_name}` exists ({} bytes).",
                    metadata.map(|metadata| metadata.len()).unwrap_or(0)
                )
            } else {
                format!("Output artifact `{output_name}` is missing.")
            },
            path: Some(path.to_string()),
        };
    }

    let command = format!(
        "if [ -f {} ]; then wc -c < {}; else exit 2; fi",
        sh_quote(path),
        sh_quote(path)
    );
    match execute_env_command(ctx, &operator_environment_cwd(ctx), &command, 30).await {
        Ok(result) if result.returncode == 0 => OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok: true,
            severity: "info".to_string(),
            message: format!(
                "Output artifact `{output_name}` exists remotely ({} bytes).",
                result.output.trim()
            ),
            path: Some(path.to_string()),
        },
        Ok(result) => OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok: false,
            severity: "error".to_string(),
            message: format!(
                "Output artifact `{output_name}` is missing or unreadable remotely (exit {}).",
                result.returncode
            ),
            path: Some(path.to_string()),
        },
        Err(error) => OperatorRunCheck {
            name: format!("output_artifact:{output_name}"),
            ok: false,
            severity: "error".to_string(),
            message: error.message,
            path: Some(path.to_string()),
        },
    }
}

pub(crate) fn operator_runs_root(project_root: &Path) -> PathBuf {
    project_root
        .join(OPERATOR_STATE_DIR_NAME)
        .join(RUNS_RELATIVE_PATH)
}

pub(crate) fn operator_run_dir(project_root: &Path, run_id: &str) -> PathBuf {
    operator_runs_root(project_root).join(run_id)
}

pub(crate) fn operator_run_relative_path(run_id: &str) -> String {
    format!("{OPERATOR_STATE_DIR_NAME}/{RUNS_RELATIVE_PATH}/{run_id}")
}

pub(crate) fn operator_runs_relative_path() -> String {
    format!("{OPERATOR_STATE_DIR_NAME}/{RUNS_RELATIVE_PATH}")
}

#[derive(Debug)]
pub(crate) struct OperatorRunSummaryWithSortKey {
    pub(crate) summary: OperatorRunSummary,
    pub(crate) sort_key: u64,
}

pub(crate) fn summarize_local_operator_run_dir(
    run_dir: &Path,
    run_id: &str,
) -> Option<OperatorRunSummaryWithSortKey> {
    let provenance_path = run_dir.join("provenance.json");
    let status_path = run_dir.join("status.json");
    let provenance = read_json_value(&provenance_path).ok();
    let status_doc = read_json_value(&status_path).ok();
    if provenance.is_none() && status_doc.is_none() {
        return None;
    }
    let modified_path = if provenance_path.is_file() {
        provenance_path.as_path()
    } else if status_path.is_file() {
        status_path.as_path()
    } else {
        run_dir
    };
    let updated_at = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["updatedAt"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["updatedAt"]))
        })
        .or_else(|| file_modified_rfc3339(modified_path));
    let default_provenance_path = if provenance_path.is_file() {
        Some(provenance_path.to_string_lossy().into_owned())
    } else {
        None
    };
    summarize_operator_run_documents(
        run_id,
        "local",
        run_dir.to_string_lossy().into_owned(),
        default_provenance_path,
        provenance,
        status_doc,
        updated_at,
        file_modified_sort_key(modified_path),
    )
}

pub(crate) fn summarize_operator_run_documents(
    run_id: &str,
    default_location: &str,
    default_run_dir: String,
    default_provenance_path: Option<String>,
    provenance: Option<JsonValue>,
    status_doc: Option<JsonValue>,
    updated_at: Option<String>,
    sort_key: u64,
) -> Option<OperatorRunSummaryWithSortKey> {
    if provenance.is_none() && status_doc.is_none() {
        return None;
    }
    let status = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["status"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["status"]))
        })
        .unwrap_or_else(|| "unknown".to_string());
    let location = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["location"]))
        .unwrap_or_else(|| default_location.to_string());
    let operator_alias = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "alias"]));
    let operator_id = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "id"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["operator", "id"]))
        });
    let operator_alias = operator_alias.or_else(|| {
        status_doc
            .as_ref()
            .and_then(|value| json_string_at(value, &["operator", "alias"]))
    });
    let operator_version = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "version"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["operator", "version"]))
        });
    let source_plugin = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["operator", "sourcePlugin"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["operator", "sourcePlugin"]))
        });
    let run_kind = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runContext", "kind"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runContext", "kind"]))
        });
    let smoke_test_id = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runContext", "smokeTestId"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runContext", "smokeTestId"]))
        });
    let smoke_test_name = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runContext", "smokeTestName"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runContext", "smokeTestName"]))
        });
    let run_dir_value = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["runDir"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["runDir"]))
        })
        .unwrap_or(default_run_dir);
    let provenance_path_value = provenance.as_ref().and_then(|value| {
        json_string_at(value, &["provenancePath"]).or(default_provenance_path.clone())
    });
    let export_dir = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["exportDir"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["exportDir"]))
        });
    let output_count = provenance.as_ref().map(output_artifact_count).unwrap_or(0);
    let structured_output_count = provenance
        .as_ref()
        .map(structured_output_count)
        .unwrap_or(0);
    let error_message = status_doc
        .as_ref()
        .and_then(operator_error_message)
        .or_else(|| provenance.as_ref().and_then(operator_error_message));
    let error_kind = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "kind"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "kind"]))
        });
    let retryable = status_doc
        .as_ref()
        .and_then(|value| json_bool_at(value, &["error", "retryable"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_bool_at(value, &["error", "retryable"]))
        });
    let suggested_action = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "suggestedAction"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "suggestedAction"]))
        });
    let slurm_diagnostic = status_doc
        .as_ref()
        .and_then(|value| json_value_at(value, &["error", "slurmDiagnostic"]).cloned())
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_value_at(value, &["error", "slurmDiagnostic"]).cloned())
        })
        .and_then(|value| serde_json::from_value::<SacctDiagnostic>(value).ok());
    let stdout_tail = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "stdoutTail"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "stdoutTail"]))
        });
    let stderr_tail = status_doc
        .as_ref()
        .and_then(|value| json_string_at(value, &["error", "stderrTail"]))
        .or_else(|| {
            provenance
                .as_ref()
                .and_then(|value| json_string_at(value, &["error", "stderrTail"]))
        });
    let cache_key = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["cache", "key"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["cache", "key"]))
        });
    let cache_hit = provenance
        .as_ref()
        .and_then(|value| json_bool_at(value, &["cache", "hit"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_bool_at(value, &["cache", "hit"]))
        });
    let cache_source_run_id = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["cache", "sourceRunId"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["cache", "sourceRunId"]))
        });
    let cache_source_run_dir = provenance
        .as_ref()
        .and_then(|value| json_string_at(value, &["cache", "sourceRunDir"]))
        .or_else(|| {
            status_doc
                .as_ref()
                .and_then(|value| json_string_at(value, &["cache", "sourceRunDir"]))
        });
    Some(OperatorRunSummaryWithSortKey {
        summary: OperatorRunSummary {
            run_id: run_id.to_string(),
            status,
            location,
            operator_alias,
            operator_id,
            operator_version,
            source_plugin,
            run_kind,
            smoke_test_id,
            smoke_test_name,
            run_dir: run_dir_value,
            updated_at,
            provenance_path: provenance_path_value,
            export_dir,
            output_count,
            structured_output_count,
            error_message,
            error_kind,
            retryable,
            suggested_action,
            slurm_diagnostic,
            stdout_tail,
            stderr_tail,
            cache_key,
            cache_hit,
            cache_source_run_id,
            cache_source_run_dir,
        },
        sort_key,
    })
}

pub(crate) fn is_safe_operator_run_id(run_id: &str) -> bool {
    run_id.starts_with("oprun_")
        && run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn json_value_at<'a>(value: &'a JsonValue, path: &[&str]) -> Option<&'a JsonValue> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

pub(crate) fn json_string_at(value: &JsonValue, path: &[&str]) -> Option<String> {
    json_value_at(value, path).and_then(|value| match value {
        JsonValue::String(value) if !value.trim().is_empty() => Some(value.clone()),
        _ => None,
    })
}

fn json_bool_at(value: &JsonValue, path: &[&str]) -> Option<bool> {
    json_value_at(value, path).and_then(JsonValue::as_bool)
}

fn operator_error_message(value: &JsonValue) -> Option<String> {
    json_string_at(value, &["error", "message"])
}

fn output_artifact_count(value: &JsonValue) -> usize {
    json_value_at(value, &["outputs"])
        .and_then(JsonValue::as_object)
        .map(|outputs| {
            outputs
                .values()
                .filter_map(JsonValue::as_array)
                .map(Vec::len)
                .sum()
        })
        .unwrap_or(0)
}

fn structured_output_count(value: &JsonValue) -> usize {
    json_value_at(value, &["structuredOutputs"])
        .and_then(JsonValue::as_object)
        .map(JsonMap::len)
        .unwrap_or(0)
}

fn file_modified_sort_key(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn rfc3339_sort_key(value: Option<&str>) -> u64 {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .and_then(|value| u64::try_from(value.timestamp()).ok())
        .unwrap_or(0)
}

fn file_modified_rfc3339(path: &Path) -> Option<String> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
    Some(datetime.to_rfc3339())
}
