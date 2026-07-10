//! Operator container execution helpers (migrated from execution.rs).

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperatorContainerKind {
    Docker,
    Singularity,
}

impl OperatorContainerKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Singularity => "singularity",
        }
    }
}

impl std::fmt::Display for OperatorContainerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperatorContainerSelection {
    pub(crate) kind: OperatorContainerKind,
    pub(crate) image: String,
    pub(crate) prepare: Option<OperatorContainerImagePrepare>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperatorContainerImagePrepare {
    Dockerfile {
        dockerfile: String,
        context: String,
        tag: String,
    },
    SingularityDefinition {
        definition: String,
        sif: String,
        hash: String,
    },
}

pub(crate) fn selected_direct_container(
    ctx: &crate::domain::tools::ToolContext,
    runtime: &JsonValue,
) -> Option<OperatorContainerSelection> {
    let declared = declared_runtime_containers(runtime);
    if declared.contains("none") {
        let explicit = explicit_runtime_container_kind(runtime);
        return explicit
            .filter(|kind| declared.contains(kind.as_str()))
            .map(|kind| OperatorContainerSelection {
                kind,
                image: runtime_container_image(runtime, kind),
                prepare: None,
            });
    }

    let backend = ctx.sandbox_backend.trim().to_ascii_lowercase();
    let preferred =
        explicit_runtime_container_kind(runtime).or_else(|| container_kind_from_name(&backend));
    preferred
        .filter(|kind| declared.contains(kind.as_str()))
        .map(|kind| OperatorContainerSelection {
            kind,
            image: runtime_container_image(runtime, kind),
            prepare: None,
        })
}

fn declared_runtime_containers(runtime: &JsonValue) -> HashSet<String> {
    let mut out = runtime_axis_values(runtime, "container")
        .into_iter()
        .filter(|value| matches!(value.as_str(), "none" | "docker" | "singularity"))
        .collect::<HashSet<_>>();
    if let Some(items) = runtime.get("supported").and_then(JsonValue::as_array) {
        for item in items {
            if let Some(value) = item.as_str().map(|value| value.trim().to_ascii_lowercase()) {
                if matches!(value.as_str(), "none" | "docker" | "singularity") {
                    out.insert(value);
                }
            }
        }
    }
    if out.is_empty() {
        out.insert("none".to_string());
    }
    out
}

fn explicit_runtime_container_kind(runtime: &JsonValue) -> Option<OperatorContainerKind> {
    let container = runtime.get("container")?;
    ["default", "preferred", "type", "backend"]
        .into_iter()
        .filter_map(|key| container.get(key).and_then(JsonValue::as_str))
        .find_map(|value| container_kind_from_name(value.trim()))
}

fn container_kind_from_name(value: &str) -> Option<OperatorContainerKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "docker" => Some(OperatorContainerKind::Docker),
        "singularity" => Some(OperatorContainerKind::Singularity),
        _ => None,
    }
}

fn runtime_container_image(runtime: &JsonValue, kind: OperatorContainerKind) -> String {
    let container = runtime.get("container").unwrap_or(&JsonValue::Null);
    let by_kind = match kind {
        OperatorContainerKind::Docker => container
            .get("dockerImage")
            .or_else(|| container.get("docker_image")),
        OperatorContainerKind::Singularity => container
            .get("singularityImage")
            .or_else(|| container.get("singularity_image")),
    };
    by_kind
        .and_then(JsonValue::as_str)
        .or_else(|| {
            container
                .get("images")
                .and_then(|images| images.get(kind.as_str()))
                .and_then(JsonValue::as_str)
        })
        .or_else(|| container.get("image").and_then(JsonValue::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match kind {
            OperatorContainerKind::Docker => {
                std::env::var("OMIGA_DOCKER_IMAGE").unwrap_or_else(|_| "ubuntu:22.04".to_string())
            }
            OperatorContainerKind::Singularity => std::env::var("OMIGA_SINGULARITY_IMAGE")
                .unwrap_or_else(|_| "docker://ubuntu:22.04".to_string()),
        })
}

pub(crate) fn operator_container_for_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<OperatorContainerSelection> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    spec.runtime
        .as_ref()
        .and_then(|runtime| selected_direct_container(ctx, runtime))
        .or_else(|| operator_container_from_environment_profile(ctx, spec, surface_kind))
}

pub(crate) fn operator_container_from_environment_profile(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<OperatorContainerSelection> {
    operator_environment_container_selection(ctx, spec, surface_kind)
        .ok()
        .flatten()
}

pub(crate) fn operator_environment_ref_error_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Option<String> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    let env_ref = operator_runtime_env_ref(spec)?;
    let profile = operator_environment_profile(spec)?;
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let message = if matches!(kind.as_str(), "docker" | "singularity") {
        match operator_environment_container_selection(ctx, spec, surface_kind) {
            Ok(Some(_)) | Ok(None) => return None,
            Err(message) => message,
        }
    } else if matches!(
        kind.as_str(),
        "system" | "local" | "host" | "conda" | "mamba" | "micromamba"
    ) {
        return None;
    } else {
        format!(
            "Operator environment envRef `{env_ref}` resolved to unsupported runtime.type `{kind}`. Supported environment runtimes are system/local/host, conda/mamba/micromamba, docker, and singularity."
        )
    };
    Some(command_with_log_capture(&[
        "/bin/sh".to_string(),
        "-lc".to_string(),
        format!("printf '%s\\n' {} >&2; exit 127", sh_quote(&message)),
    ]))
}

pub(crate) fn operator_environment_container_selection(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Result<Option<OperatorContainerSelection>, String> {
    let Some(profile) = operator_environment_profile(spec) else {
        return Ok(None);
    };
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let Some(container_kind) = container_kind_from_name(&kind) else {
        return Ok(None);
    };
    if let Some(image) = operator_environment_profile_image(&profile) {
        return Ok(Some(OperatorContainerSelection {
            kind: container_kind,
            image,
            prepare: None,
        }));
    }
    if surface_kind != OperatorExecutionSurfaceKind::Local {
        return Err(format!(
            "Environment profile `{}` uses `{kind}` without runtime.image. File-based `{kind}` builds are only supported for local Operator runs; build the image on the target system and set runtime.image.",
            profile.canonical_id
        ));
    }
    match container_kind {
        OperatorContainerKind::Docker => {
            let dockerfile = operator_dockerfile_from_environment_profile(&profile)?;
            let context =
                operator_docker_build_context_from_environment_profile(&profile, &dockerfile);
            let dockerfile_bytes = fs::read(&dockerfile).map_err(|err| {
                format!(
                    "Read Dockerfile for environment profile `{}` at `{}`: {err}",
                    profile.canonical_id,
                    dockerfile.display()
                )
            })?;
            let env_hash = sha256_hex(&dockerfile_bytes);
            let tag = format!(
                "omiga-env-{}:{}",
                safe_operator_env_component(&profile.canonical_id).to_ascii_lowercase(),
                &env_hash[..12]
            );
            Ok(Some(OperatorContainerSelection {
                kind: OperatorContainerKind::Docker,
                image: tag.clone(),
                prepare: Some(OperatorContainerImagePrepare::Dockerfile {
                    dockerfile: dockerfile.to_string_lossy().into_owned(),
                    context: context.to_string_lossy().into_owned(),
                    tag,
                }),
            }))
        }
        OperatorContainerKind::Singularity => {
            let definition = operator_singularity_definition_from_environment_profile(&profile)?;
            let definition_bytes = fs::read(&definition).map_err(|err| {
                format!(
                    "Read Singularity definition for environment profile `{}` at `{}`: {err}",
                    profile.canonical_id,
                    definition.display()
                )
            })?;
            let env_hash = sha256_hex(&definition_bytes);
            let env_key = format!(
                "{}-{}",
                safe_operator_env_component(&profile.canonical_id),
                &env_hash[..12]
            );
            let sif = ctx
                .project_root
                .join(".omiga/operator-envs/singularity")
                .join(format!("{env_key}.sif"))
                .to_string_lossy()
                .into_owned();
            Ok(Some(OperatorContainerSelection {
                kind: OperatorContainerKind::Singularity,
                image: sif.clone(),
                prepare: Some(OperatorContainerImagePrepare::SingularityDefinition {
                    definition: definition.to_string_lossy().into_owned(),
                    sif,
                    hash: env_hash,
                }),
            }))
        }
    }
}

pub(crate) fn operator_singularity_definition_from_environment_profile(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    if let Some(raw) = profile_runtime_extra_str(
        profile,
        &[
            "definitionFile",
            "definition_file",
            "singularityDef",
            "singularity_def",
        ],
    ) {
        let path = operator_profile_relative_path(profile, raw)?;
        if path.extension().and_then(|ext| ext.to_str()) != Some("def") {
            return Err(format!(
                "Singularity environment profile `{}` must use a `.def` definition file; got `{}`.",
                profile.canonical_id,
                path.display()
            ));
        }
        if !path.is_file() {
            return Err(format!(
                "Singularity environment profile `{}` declares definition file `{}` but it does not exist.",
                profile.canonical_id,
                path.display()
            ));
        }
        return Ok(path);
    }
    let manifest_dir = operator_environment_manifest_dir(profile)?;
    let candidate = manifest_dir.join("singularity.def");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(format!(
        "Singularity environment profile `{}` requires runtime.image or a standard `singularity.def` next to environment.yaml.",
        profile.canonical_id
    ))
}

pub(crate) fn operator_container_selection_for_profile(
    ctx: &crate::domain::tools::ToolContext,
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Result<Option<OperatorContainerSelection>, String> {
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let container_kind = container_kind_from_name(&kind).ok_or_else(|| {
        format!(
            "Environment profile `{}` runtime.type must be docker or singularity for container prewarm: `{kind}`",
            profile.canonical_id
        )
    })?;

    if let Some(image) = operator_environment_profile_image(profile) {
        return Ok(Some(OperatorContainerSelection {
            kind: container_kind,
            image,
            prepare: None,
        }));
    }

    if surface_kind != OperatorExecutionSurfaceKind::Local {
        return Err(format!(
            "Environment profile `{}` uses `{kind}` without runtime.image. File-based `{kind}` builds are only supported for local Operator runs; build the image on the target system and set runtime.image.",
            profile.canonical_id
        ));
    }

    match container_kind {
        OperatorContainerKind::Docker => {
            let dockerfile = operator_dockerfile_from_environment_profile(profile)?;
            let context =
                operator_docker_build_context_from_environment_profile(profile, &dockerfile);
            let dockerfile_bytes = fs::read(&dockerfile).map_err(|err| {
                format!(
                    "Read Dockerfile for environment profile `{}` at `{}`: {err}",
                    profile.canonical_id,
                    dockerfile.display()
                )
            })?;
            let env_hash = sha256_hex(&dockerfile_bytes);
            let tag = format!(
                "omiga-env-{}:{}",
                safe_operator_env_component(&profile.canonical_id).to_ascii_lowercase(),
                &env_hash[..12]
            );
            Ok(Some(OperatorContainerSelection {
                kind: OperatorContainerKind::Docker,
                image: tag.clone(),
                prepare: Some(OperatorContainerImagePrepare::Dockerfile {
                    dockerfile: dockerfile.to_string_lossy().into_owned(),
                    context: context.to_string_lossy().into_owned(),
                    tag,
                }),
            }))
        }
        OperatorContainerKind::Singularity => {
            let definition = operator_singularity_definition_from_environment_profile(profile)?;
            let definition_bytes = fs::read(&definition).map_err(|err| {
                format!(
                    "Read Singularity definition for environment profile `{}` at `{}`: {err}",
                    profile.canonical_id,
                    definition.display()
                )
            })?;
            let env_hash = sha256_hex(&definition_bytes);
            let env_key = format!(
                "{}-{}",
                safe_operator_env_component(&profile.canonical_id),
                &env_hash[..12]
            );
            let sif = ctx
                .project_root
                .join(".omiga/operator-envs/singularity")
                .join(format!("{env_key}.sif"))
                .to_string_lossy()
                .into_owned();
            Ok(Some(OperatorContainerSelection {
                kind: OperatorContainerKind::Singularity,
                image: sif.clone(),
                prepare: Some(OperatorContainerImagePrepare::SingularityDefinition {
                    definition: definition.to_string_lossy().into_owned(),
                    sif,
                    hash: env_hash,
                }),
            }))
        }
    }
}

pub(crate) fn operator_environment_profile(
    spec: &OperatorSpec,
) -> Option<crate::domain::environments::EnvironmentProfileSummary> {
    let env_ref = operator_runtime_env_ref(spec)?;
    let resolved = crate::domain::environments::resolve_environment_ref(
        Some(env_ref),
        &spec.source.source_plugin,
        &spec.source.plugin_root,
    );
    resolved.profile
}

fn operator_environment_profile_image(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Option<String> {
    profile
        .runtime
        .image
        .clone()
        .or_else(|| {
            profile
                .runtime
                .extra
                .get("image")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn operator_dockerfile_from_environment_profile(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    if let Some(raw) = profile_runtime_extra_str(profile, &["dockerfile", "dockerFile"]) {
        let path = operator_profile_relative_path(profile, raw)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if file_name != "Dockerfile" {
            return Err(format!(
                "Docker environment profile `{}` must use a standard `Dockerfile`; got `{}`.",
                profile.canonical_id,
                path.display()
            ));
        }
        if !path.is_file() {
            return Err(format!(
                "Docker environment profile `{}` declares Dockerfile `{}` but it does not exist.",
                profile.canonical_id,
                path.display()
            ));
        }
        return Ok(path);
    }
    let manifest_dir = operator_environment_manifest_dir(profile)?;
    let candidate = manifest_dir.join("Dockerfile");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(format!(
        "Docker environment profile `{}` requires runtime.image or a standard `Dockerfile` next to environment.yaml.",
        profile.canonical_id
    ))
}

fn operator_docker_build_context_from_environment_profile(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    dockerfile: &Path,
) -> PathBuf {
    profile_runtime_extra_str(profile, &["context", "buildContext", "build_context"])
        .and_then(|raw| operator_profile_relative_path(profile, raw).ok())
        .unwrap_or_else(|| {
            dockerfile
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

pub(crate) fn containerized_operator_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    selection: OperatorContainerSelection,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    argv: &[String],
    inputs: &BTreeMap<String, JsonValue>,
) -> String {
    let inner = command_with_log_capture(argv);
    let mounts = operator_container_mounts(ctx, spec, surface_kind, run_dir, inputs);
    let kind = selection.kind;
    let container_command = match selection.kind {
        OperatorContainerKind::Docker => {
            let mut tokens = vec!["docker".to_string(), "run".to_string(), "--rm".to_string()];
            for mount in mounts {
                tokens.push("-v".to_string());
                tokens.push(container_bind_spec(&mount.path, mount.writable));
            }
            tokens.extend([
                "-w".to_string(),
                run_dir.to_string(),
                selection.image.clone(),
                "/bin/sh".to_string(),
                "-lc".to_string(),
                inner,
            ]);
            shell_join(&tokens)
        }
        OperatorContainerKind::Singularity => {
            let mut tokens = vec![
                "singularity".to_string(),
                "exec".to_string(),
                "--cleanenv".to_string(),
                "--pwd".to_string(),
                run_dir.to_string(),
            ];
            for mount in mounts {
                tokens.push("--bind".to_string());
                tokens.push(container_bind_spec(&mount.path, mount.writable));
            }
            tokens.extend([
                selection.image.clone(),
                "/bin/sh".to_string(),
                "-lc".to_string(),
                inner,
            ]);
            shell_join(&tokens)
        }
    };
    container_runtime_shell_script(kind, selection.prepare.as_ref(), &container_command)
}

fn container_runtime_shell_script(
    kind: OperatorContainerKind,
    prepare: Option<&OperatorContainerImagePrepare>,
    container_command: &str,
) -> String {
    let preflight = container_runtime_preflight_script(kind);
    let prepare = prepare
        .map(container_runtime_prepare_script)
        .unwrap_or_default();
    format!(
        r#"set -e
mkdir -p logs
omiga_container_runtime_missing() {{
  message=$1
  printf '%s\n' "$message" >&2
  printf '%s\n' "$message" >> logs/stderr.txt
  printf '\n__OMIGA_OPERATOR_EXIT_CODE=127\n'
  exit 127
}}
{preflight}
{prepare}
set +e
{container_command} >> logs/stdout.txt 2>> logs/stderr.txt
code=$?
printf '\n__OMIGA_OPERATOR_EXIT_CODE=%s\n' "$code"
exit "$code""#
    )
}

pub(crate) fn container_runtime_preflight_script(kind: OperatorContainerKind) -> &'static str {
    match kind {
        OperatorContainerKind::Docker => {
            r#"if ! command -v docker >/dev/null 2>&1; then
  omiga_container_runtime_missing 'Docker runtime is required for this Operator environment but `docker` was not found in the active PATH/base environment/virtual environment. Install Docker Desktop/Engine, make the `docker` CLI available, and retry.'
fi
if ! docker version >/dev/null 2>&1; then
  omiga_container_runtime_missing 'Docker CLI was found, but `docker version` failed. Start Docker Desktop/daemon or fix Docker permissions, then retry.'
fi"#
        }
        OperatorContainerKind::Singularity => {
            r#"if command -v singularity >/dev/null 2>&1; then
  :
elif command -v apptainer >/dev/null 2>&1; then
  singularity() { apptainer "$@"; }
else
  omiga_container_runtime_missing 'Singularity/Apptainer runtime is required for this Operator environment but neither `singularity` nor `apptainer` was found in the active PATH/base environment/virtual environment. Install SingularityCE or Apptainer and retry.'
fi"#
        }
    }
}

pub(crate) fn container_runtime_prepare_script(prepare: &OperatorContainerImagePrepare) -> String {
    match prepare {
        OperatorContainerImagePrepare::Dockerfile {
            dockerfile,
            context,
            tag,
        } => format!(
            r#"OMIGA_DOCKERFILE={dockerfile}
OMIGA_DOCKER_CONTEXT={context}
OMIGA_DOCKER_IMAGE={tag}
if ! docker image inspect "$OMIGA_DOCKER_IMAGE" >/dev/null 2>&1; then
  docker build -t "$OMIGA_DOCKER_IMAGE" -f "$OMIGA_DOCKERFILE" "$OMIGA_DOCKER_CONTEXT" >> logs/stdout.txt 2>> logs/stderr.txt
fi"#,
            dockerfile = sh_quote(dockerfile),
            context = sh_quote(context),
            tag = sh_quote(tag),
        ),
        OperatorContainerImagePrepare::SingularityDefinition {
            definition,
            sif,
            hash,
        } => format!(
            r#"OMIGA_SINGULARITY_DEF={definition}
OMIGA_SINGULARITY_SIF={sif}
OMIGA_SINGULARITY_HASH={hash}
mkdir -p "$(dirname "$OMIGA_SINGULARITY_SIF")"
if [ ! -f "$OMIGA_SINGULARITY_SIF" ] || [ "$(cat "$OMIGA_SINGULARITY_SIF.omiga-env-hash" 2>/dev/null || true)" != "$OMIGA_SINGULARITY_HASH" ]; then
  rm -f "$OMIGA_SINGULARITY_SIF.tmp"
  singularity build "$OMIGA_SINGULARITY_SIF.tmp" "$OMIGA_SINGULARITY_DEF" >> logs/stdout.txt 2>> logs/stderr.txt
  mv "$OMIGA_SINGULARITY_SIF.tmp" "$OMIGA_SINGULARITY_SIF"
  printf '%s' "$OMIGA_SINGULARITY_HASH" > "$OMIGA_SINGULARITY_SIF.omiga-env-hash"
fi"#,
            definition = sh_quote(definition),
            sif = sh_quote(sif),
            hash = sh_quote(hash),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OperatorContainerMount {
    path: String,
    writable: bool,
}

pub(crate) fn operator_container_mounts(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    inputs: &BTreeMap<String, JsonValue>,
) -> Vec<OperatorContainerMount> {
    let mut mounts = BTreeMap::<String, bool>::new();
    insert_container_mount(&mut mounts, run_dir, true);

    if surface_kind == OperatorExecutionSurfaceKind::Local {
        insert_container_mount(&mut mounts, &ctx.project_root.to_string_lossy(), false);
        insert_container_mount(
            &mut mounts,
            &spec.source.plugin_root.to_string_lossy(),
            false,
        );
    }

    for path in path_like_input_values(spec, inputs) {
        insert_container_mount(&mut mounts, &path, false);
    }

    mounts
        .into_iter()
        .map(|(path, writable)| OperatorContainerMount { path, writable })
        .collect()
}

fn insert_container_mount(mounts: &mut BTreeMap<String, bool>, path: &str, writable: bool) {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return;
    }
    let entry = mounts.entry(trimmed.to_string()).or_insert(false);
    *entry = *entry || writable;
}

fn path_like_input_values(
    spec: &OperatorSpec,
    inputs: &BTreeMap<String, JsonValue>,
) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for (name, field) in &spec.interface.inputs {
        if !field.kind.is_path_like() {
            continue;
        }
        let Some(value) = inputs.get(name) else {
            continue;
        };
        if field.kind.is_array() {
            if let Some(items) = value.as_array() {
                for item in items {
                    if let Some(path) = item.as_str() {
                        paths.insert(path.to_string());
                    }
                }
            }
        } else if let Some(path) = value.as_str() {
            paths.insert(path.to_string());
        }
    }
    paths.into_iter().collect()
}

fn container_bind_spec(path: &str, writable: bool) -> String {
    if writable {
        format!("{path}:{path}")
    } else {
        format!("{path}:{path}:ro")
    }
}
