//! Operator conda execution helpers (migrated from execution.rs).

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct OperatorCondaEnvironmentSelection {
    pub(crate) env_prefix: String,
    pub(crate) env_yaml_b64: String,
    pub(crate) env_hash: String,
    pub(crate) env_vars: BTreeMap<String, String>,
}

pub(crate) fn operator_conda_environment_command(
    ctx: &crate::domain::tools::ToolContext,
    spec: &OperatorSpec,
    surface_kind: OperatorExecutionSurfaceKind,
    run_dir: &str,
    argv: &[String],
) -> Option<String> {
    if surface_kind == OperatorExecutionSurfaceKind::Sandbox {
        return None;
    }
    let env_ref = operator_runtime_env_ref(spec)?;
    let Some(profile) = operator_environment_profile(spec) else {
        return Some(command_with_log_capture(&[
            "/bin/sh".to_string(),
            "-lc".to_string(),
            format!(
                "printf '%s\\n' {} >&2; exit 127",
                sh_quote(&format!(
                    "Operator environment envRef `{env_ref}` did not resolve for plugin `{}`.",
                    spec.source.source_plugin
                ))
            ),
        ]));
    };
    let kind = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    if !matches!(kind.as_str(), "conda" | "mamba" | "micromamba") {
        return None;
    }
    match operator_conda_environment_selection(ctx, &profile, surface_kind) {
        Ok(selection) => {
            let shell_script =
                conda_environment_shell_script(&selection, run_dir, &shell_join(argv));
            Some(command_with_log_capture(&[
                "/bin/sh".to_string(),
                "-lc".to_string(),
                shell_script,
            ]))
        }
        Err(message) => Some(command_with_log_capture(&[
            "/bin/sh".to_string(),
            "-lc".to_string(),
            format!("printf '%s\\n' {} >&2; exit 127", sh_quote(&message)),
        ])),
    }
}

pub(crate) fn operator_conda_environment_selection(
    ctx: &crate::domain::tools::ToolContext,
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    surface_kind: OperatorExecutionSurfaceKind,
) -> Result<OperatorCondaEnvironmentSelection, String> {
    let conda_file = operator_conda_environment_file(profile)?;
    let bytes = fs::read(&conda_file).map_err(|err| {
        format!(
            "Read conda environment file `{}`: {err}",
            conda_file.display()
        )
    })?;
    let env_hash = sha256_hex(&bytes);
    let env_key = format!(
        "{}-{}",
        safe_operator_env_component(&profile.canonical_id),
        &env_hash[..12]
    );
    let env_prefix = operator_conda_env_prefix(ctx, surface_kind, &env_key);
    use base64::{engine::general_purpose, Engine as _};
    Ok(OperatorCondaEnvironmentSelection {
        env_prefix,
        env_yaml_b64: general_purpose::STANDARD.encode(bytes),
        env_hash,
        env_vars: profile.runtime.env.clone(),
    })
}

fn operator_conda_environment_file(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    for key in [
        "condaEnvFile",
        "conda_env_file",
        "condaFile",
        "conda_file",
        "environmentFile",
        "environment_file",
    ] {
        if let Some(raw) = profile_runtime_extra_str(profile, &[key]) {
            let path = operator_profile_relative_path(profile, raw)?;
            validate_conda_environment_yaml_path(profile, &path)?;
            if !path.is_file() {
                return Err(format!(
                    "Environment profile `{}` declares conda YAML file `{}` but it does not exist.",
                    profile.canonical_id,
                    path.display()
                ));
            }
            return Ok(path);
        }
    }
    let manifest_dir = operator_environment_manifest_dir(profile)?;
    for name in ["conda.yaml", "conda.yml"] {
        let candidate = manifest_dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Environment profile `{}` does not declare or contain a standard conda YAML file. Use `runtime.condaEnvFile: ./conda.yaml` or `./conda.yml`.",
        profile.canonical_id
    ))
}

fn validate_conda_environment_yaml_path(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    path: &Path,
) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    if !matches!(extension.as_deref(), Some("yaml" | "yml")) {
        return Err(format!(
            "Conda/mamba environment profile `{}` must use a `.yaml` or `.yml` file; got `{}`.",
            profile.canonical_id,
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn operator_conda_env_prefix(
    ctx: &crate::domain::tools::ToolContext,
    surface_kind: OperatorExecutionSurfaceKind,
    env_key: &str,
) -> String {
    let rel = format!(".omiga/operator-envs/conda/{env_key}");
    if surface_kind == OperatorExecutionSurfaceKind::Local {
        return ctx.project_root.join(rel).to_string_lossy().into_owned();
    }
    crate::domain::tools::env_store::remote_path(ctx, &rel)
}

pub(crate) const MICROMAMBA_BOOTSTRAP_SHELL: &str = r#"omiga_bootstrap_micromamba() {
  if [ "${OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP:-}" = "1" ]; then
    printf 'micromamba bootstrap is disabled by OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1\n' >&2
    return 1
  fi
  os=$(uname -s 2>/dev/null || true)
  arch=$(uname -m 2>/dev/null || true)
  case "${os}:${arch}" in
    Linux:x86_64) platform=linux-64 ;;
    Linux:arm64) platform=linux-aarch64 ;;
    Linux:aarch64) platform=linux-aarch64 ;;
    Darwin:x86_64) platform=osx-64 ;;
    Darwin:arm64) platform=osx-arm64 ;;
    *)
      printf 'unsupported platform for micromamba bootstrap: %s:%s\n' "$os" "$arch" >&2
      return 1
      ;;
  esac

  micromamba_url="${OMIGA_MICROMAMBA_URL:-https://github.com/mamba-org/micromamba-releases/releases/latest/download/micromamba-${platform}}"
  target_dir="$HOME/.omiga/bin"
  target_bin="$target_dir/micromamba"
  tmp_bin="$target_dir/.micromamba.tmp-$$"

  mkdir -p "$target_dir"
  rm -f "$tmp_bin"
  if command -v curl >/dev/null 2>&1; then
    if ! curl -fsSL --max-time 300 "$micromamba_url" -o "$tmp_bin" >/dev/null 2>&1; then
      printf 'micromamba bootstrap download failed with curl\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
  elif command -v wget >/dev/null 2>&1; then
    if ! wget -T 300 -t 2 -qO- "$micromamba_url" > "$tmp_bin" 2>/dev/null; then
      printf 'micromamba bootstrap download failed with wget\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
  else
    printf 'no supported downloader for micromamba bootstrap\n' >&2
    return 1
  fi
  if [ -n "$OMIGA_MICROMAMBA_SHA256" ]; then
    if command -v shasum >/dev/null 2>&1; then
      checksum="$(shasum -a 256 "$tmp_bin" | awk '{print $1}')"
    elif command -v sha256sum >/dev/null 2>&1; then
      checksum="$(sha256sum "$tmp_bin" | awk '{print $1}')"
    else
      printf 'micromamba bootstrap checksum unavailable: neither shasum nor sha256sum\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
    checksum_expected="$(printf '%s' "$OMIGA_MICROMAMBA_SHA256" | tr '[:upper:]' '[:lower:]')"
    checksum_actual="$(printf '%s' "$checksum" | tr '[:upper:]' '[:lower:]')"
    if [ "$checksum_actual" != "$checksum_expected" ]; then
      printf 'micromamba bootstrap checksum mismatch for downloaded binary\n' >&2
      rm -f "$tmp_bin"
      return 1
    fi
  fi

  if ! chmod +x "$tmp_bin" >/dev/null 2>&1; then
    printf 'micromamba bootstrap binary is not executable\n' >&2
    rm -f "$tmp_bin"
    return 1
  fi
  if ! "$tmp_bin" --version >/dev/null 2>&1; then
    printf 'micromamba bootstrap self-check failed\n' >&2
    rm -f "$tmp_bin"
    return 1
  fi
  if ! mv "$tmp_bin" "$target_bin"; then
    printf 'micromamba bootstrap installation failed\n' >&2
    rm -f "$tmp_bin"
    return 1
  fi
  OMIGA_CONDA_MANAGER_KIND=micromamba
  OMIGA_CONDA_BIN=$target_bin
  return 0
}
"#;

pub(crate) fn conda_environment_shell_script(
    selection: &OperatorCondaEnvironmentSelection,
    run_dir: &str,
    inner_command: &str,
) -> String {
    let env_yaml = format!("{run_dir}/env/conda-environment.yaml");
    let exports = crate::domain::env_hygiene::shell_export_lines(&selection.env_vars);
    format!(
        r#"{bootstrap}
set -e
OMIGA_CONDA_PREFIX={env_prefix}
OMIGA_CONDA_YAML={env_yaml}
OMIGA_CONDA_HASH={env_hash}
OMIGA_MICROMAMBA="${{OMIGA_MICROMAMBA:-$HOME/.omiga/bin/micromamba}}"
mkdir -p "$(dirname "$OMIGA_CONDA_YAML")" "$(dirname "$OMIGA_CONDA_PREFIX")"
printf %s {env_yaml_b64} | python3 -c 'import base64,sys; sys.stdout.buffer.write(base64.b64decode(sys.stdin.read()))' > "$OMIGA_CONDA_YAML"
omiga_find_conda_manager() {{
  OMIGA_CONDA_MANAGER_KIND=
  OMIGA_CONDA_BIN=
  if [ -n "${{OMIGA_MICROMAMBA:-}}" ] && [ -x "$OMIGA_MICROMAMBA" ]; then
    OMIGA_CONDA_MANAGER_KIND=micromamba
    OMIGA_CONDA_BIN=$OMIGA_MICROMAMBA
    return 0
  fi
  if command -v micromamba >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=micromamba
    OMIGA_CONDA_BIN=$(command -v micromamba)
    return 0
  fi
  if command -v mamba >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=mamba
    OMIGA_CONDA_BIN=$(command -v mamba)
    return 0
  fi
  if command -v conda >/dev/null 2>&1; then
    OMIGA_CONDA_MANAGER_KIND=conda
    OMIGA_CONDA_BIN=$(command -v conda)
    return 0
  fi
  return 1
}}
omiga_missing_conda_manager() {{
  cat >&2 <<'OMIGA_CONDA_HINT'
Automatic micromamba installation failed (reason above).
Install the official micromamba binary at $HOME/.omiga/bin/micromamba, set OMIGA_MICROMAMBA=/absolute/path/to/micromamba, or set OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1 to disable bootstrap.
Then rerun the Operator; Omiga will create and reuse the isolated env from conda.yaml/conda.yml under .omiga/operator-envs/conda.
OMIGA_CONDA_HINT
  exit 127
}}
omiga_find_conda_manager || true
if [ -z "$OMIGA_CONDA_BIN" ]; then
  omiga_bootstrap_micromamba || true
fi
if [ ! -f "$OMIGA_CONDA_PREFIX/.omiga-env-hash" ] || [ "$(cat "$OMIGA_CONDA_PREFIX/.omiga-env-hash" 2>/dev/null || true)" != "$OMIGA_CONDA_HASH" ]; then
  if [ -z "$OMIGA_CONDA_BIN" ]; then
    omiga_missing_conda_manager
  fi
  rm -rf "$OMIGA_CONDA_PREFIX"
  case "$OMIGA_CONDA_MANAGER_KIND" in
    micromamba)
      "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
    mamba)
      "$OMIGA_CONDA_BIN" env create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML" || "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
    conda)
      "$OMIGA_CONDA_BIN" env create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML" || "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_YAML"
      ;;
  esac
  printf '%s' "$OMIGA_CONDA_HASH" > "$OMIGA_CONDA_PREFIX/.omiga-env-hash"
fi
{exports}
case "${{OMIGA_CONDA_MANAGER_KIND:-}}" in
  micromamba)
    "$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
    ;;
  mamba)
    "$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
    ;;
  conda)
    "$OMIGA_CONDA_BIN" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
    ;;
  *)
    PATH="$OMIGA_CONDA_PREFIX/bin:$PATH" /bin/sh -lc {inner}
    ;;
esac"#,
        env_prefix = sh_quote(&selection.env_prefix),
        env_yaml = sh_quote(&env_yaml),
        env_hash = sh_quote(&selection.env_hash),
        env_yaml_b64 = sh_quote(&selection.env_yaml_b64),
        exports = exports,
        inner = sh_quote(inner_command),
        bootstrap = MICROMAMBA_BOOTSTRAP_SHELL,
    )
}
