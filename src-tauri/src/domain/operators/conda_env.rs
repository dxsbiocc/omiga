//! Operator conda execution helpers (migrated from execution.rs).

use std::ffi::OsStr;

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct OperatorCondaEnvironmentSelection {
    pub(crate) env_prefix: String,
    pub(crate) source_b64: String,
    pub(crate) source_filename: String,
    pub(crate) kind: CondaSourceKind,
    pub(crate) env_hash: String,
    pub(crate) env_vars: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CondaSourceKind {
    Yaml,
    ExplicitLock,
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
    let (bytes, source_filename, source_kind) =
        if let Some(conda_lock_file) = operator_conda_lock_file(profile, ctx)? {
            let bytes = fs::read(&conda_lock_file).map_err(|err| {
                format!(
                    "Read conda lock file `{}`: {err}",
                    conda_lock_file.display()
                )
            })?;
            let source_filename = conda_lock_file
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| {
                    format!(
                        "Conda/mamba environment profile `{}` lock filename is not valid UTF-8.",
                        profile.canonical_id
                    )
                })?
                .to_string();

            (bytes, source_filename, CondaSourceKind::ExplicitLock)
        } else {
            let conda_file = operator_conda_environment_file(profile)?;
            let bytes = fs::read(&conda_file).map_err(|err| {
                format!(
                    "Read conda environment file `{}`: {err}",
                    conda_file.display()
                )
            })?;
            (
                bytes,
                "conda-environment.yaml".to_string(),
                CondaSourceKind::Yaml,
            )
        };
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
        source_b64: general_purpose::STANDARD.encode(bytes),
        source_filename,
        kind: source_kind,
        env_hash,
        env_vars: profile.runtime.env.clone(),
    })
}

fn operator_conda_lock_file(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    _ctx: &crate::domain::tools::ToolContext,
) -> Result<Option<PathBuf>, String> {
    for key in ["condaLockFile", "conda_lock_file"] {
        if let Some(raw) = profile_runtime_extra_str(profile, &[key]) {
            let path = operator_profile_relative_path(profile, raw)?;
            validate_conda_environment_lock_path(profile, raw, &path)?;
            if !path.is_file() {
                return Err(format!(
                    "Environment profile `{}` declares conda lock file `{}` but it does not exist.",
                    profile.canonical_id,
                    path.display()
                ));
            }
            return Ok(Some(path));
        }
    }

    let manifest_dir = operator_environment_manifest_dir(profile)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&manifest_dir).map_err(|err| {
        format!(
            "Read environment manifest directory `{}`: {err}",
            manifest_dir.display()
        )
    })? {
        let entry = entry.map_err(|err| {
            format!(
                "Read environment manifest entry in `{}`: {err}",
                manifest_dir.display()
            )
        })?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };

        if !matches!(
            path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()),
            Some(ext) if ext == "lock"
        ) {
            continue;
        }
        if !file_name.starts_with("conda-") {
            continue;
        }
        if path.is_file() {
            candidates.push(path);
        }
    }
    candidates.sort();

    if candidates.len() == 1 {
        return Ok(candidates.pop());
    }

    if !candidates.is_empty() {
        return Err(format!(
            "Environment profile `{}` declares multiple explicit conda lock candidates in `{}`; set `runtime.condaLockFile` / `runtime.conda_lock_file` to select one.",
            profile.canonical_id,
            manifest_dir.display()
        ));
    }

    Ok(None)
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
            validate_conda_environment_yaml_path(profile, raw, &path)?;
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
    raw: &str,
    path: &Path,
) -> Result<(), String> {
    validate_conda_environment_declared_path(profile, raw, path, &["yaml", "yml"], "YAML file")?;
    Ok(())
}

fn validate_conda_environment_lock_path(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    raw: &str,
    path: &Path,
) -> Result<(), String> {
    validate_conda_environment_declared_path(profile, raw, path, &["lock"], "lock file")?;
    Ok(())
}

fn validate_conda_environment_declared_path(
    profile: &crate::domain::environments::EnvironmentProfileSummary,
    raw: &str,
    path: &Path,
    extensions: &[&str],
    kind: &str,
) -> Result<(), String> {
    if has_parent_directory(raw) {
        return Err(format!(
            "Conda/mamba environment profile `{}` declares a path with traversal (`{}`): `{}`.",
            profile.canonical_id, kind, raw
        ));
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    if extension.is_none()
        || !extensions
            .iter()
            .any(|candidate| extension.as_deref() == Some(*candidate))
    {
        return Err(format!(
            "Conda/mamba environment profile `{}` declares a non-{kind} file: `{}`. Allowed extensions: {}. must use a `.yaml` or `.yml` file.",
            profile.canonical_id,
            path.display(),
            extensions.join(", ")
        ));
    }
    Ok(())
}

fn has_parent_directory(raw: &str) -> bool {
    let mut depth: isize = 0;
    for component in Path::new(raw).components() {
        match component {
            std::path::Component::ParentDir => {
                if depth <= 0 {
                    return true;
                }
                depth -= 1;
            }
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir | std::path::Component::RootDir => {
                continue;
            }
            _ => {}
        }
    }
    false
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
    let env_source = if selection.kind == CondaSourceKind::Yaml {
        env_yaml.to_string()
    } else {
        format!(
            "{run_dir}/env/{source_filename}",
            source_filename = selection.source_filename
        )
    };
    let source_kind = match selection.kind {
        CondaSourceKind::Yaml => "yaml",
        CondaSourceKind::ExplicitLock => "explicit_lock",
    };
    let exports = crate::domain::env_hygiene::shell_export_lines(&selection.env_vars);
    format!(
        r#"{bootstrap}
set -e
OMIGA_CONDA_PREFIX={env_prefix}
OMIGA_CONDA_YAML={env_yaml}
OMIGA_CONDA_SRC={env_source}
OMIGA_CONDA_HASH={env_hash}
OMIGA_CONDA_SOURCE_KIND={source_kind}
OMIGA_CONDA_FINGERPRINT_FILE={fingerprint_file}
OMIGA_MICROMAMBA="${{OMIGA_MICROMAMBA:-$HOME/.omiga/bin/micromamba}}"
mkdir -p "$(dirname "$OMIGA_CONDA_YAML")" "$(dirname "$OMIGA_CONDA_PREFIX")" "$(dirname "$OMIGA_CONDA_FINGERPRINT_FILE")"
printf %s {env_yaml_b64} | python3 -c 'import base64,sys; sys.stdout.buffer.write(base64.b64decode(sys.stdin.read()))' > "$OMIGA_CONDA_SRC"
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
  if [ "$OMIGA_CONDA_SOURCE_KIND" = "explicit_lock" ]; then
    case "$OMIGA_CONDA_MANAGER_KIND" in
      micromamba)
        "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" -f "$OMIGA_CONDA_SRC"
        ;;
      mamba)
        "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" --file "$OMIGA_CONDA_SRC"
        ;;
      conda)
        "$OMIGA_CONDA_BIN" create -y -p "$OMIGA_CONDA_PREFIX" --file "$OMIGA_CONDA_SRC"
        ;;
    esac
  else
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
  fi
  printf '%s' "$OMIGA_CONDA_HASH" > "$OMIGA_CONDA_PREFIX/.omiga-env-hash"
fi
case "$OMIGA_CONDA_MANAGER_KIND" in
  micromamba)
    "$OMIGA_CONDA_BIN" env export -p "$OMIGA_CONDA_PREFIX" --explicit --md5 > "$OMIGA_CONDA_FINGERPRINT_FILE" 2>/dev/null || true
    ;;
  mamba)
    "$OMIGA_CONDA_BIN" env export -p "$OMIGA_CONDA_PREFIX" --explicit --md5 > "$OMIGA_CONDA_FINGERPRINT_FILE" 2>/dev/null || true
    ;;
  conda)
    "$OMIGA_CONDA_BIN" list -p "$OMIGA_CONDA_PREFIX" --explicit --md5 > "$OMIGA_CONDA_FINGERPRINT_FILE" 2>/dev/null || true
    ;;
esac
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
        env_source = sh_quote(&env_source),
        source_kind = sh_quote(source_kind),
        env_hash = sh_quote(&selection.env_hash),
        env_yaml_b64 = sh_quote(&selection.source_b64),
        exports = exports,
        inner = sh_quote(inner_command),
        fingerprint_file = sh_quote(&format!("{run_dir}/logs/conda-env-explicit.txt")),
        bootstrap = MICROMAMBA_BOOTSTRAP_SHELL,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environments::EnvironmentRuntimeProfile;
    use base64::Engine as _;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn build_conda_profile(
        root: &Path,
        profile_id: &str,
        runtime_kind: &str,
        extra: &[(&str, &str)],
    ) -> crate::domain::environments::EnvironmentProfileSummary {
        let manifest_dir = root.join("environments").join(profile_id);
        fs::create_dir_all(&manifest_dir).expect("create manifest dir");
        let manifest_path = manifest_dir.join("environment.yaml");
        fs::write(&manifest_path, "name: test\n").expect("write manifest");

        let mut runtime_extra = serde_json::Map::new();
        for (key, value) in extra {
            runtime_extra.insert((*key).to_string(), json!(value));
        }

        crate::domain::environments::EnvironmentProfileSummary {
            id: profile_id.to_string(),
            version: "0.1.0".to_string(),
            canonical_id: format!("plugin-a/environment/{profile_id}"),
            source_plugin: "plugin-a@local".to_string(),
            manifest_path: manifest_path.to_string_lossy().into_owned(),
            name: None,
            description: None,
            tags: Vec::new(),
            runtime: EnvironmentRuntimeProfile {
                kind: Some(runtime_kind.to_string()),
                command: None,
                args: Vec::new(),
                image: None,
                module: None,
                env: BTreeMap::new(),
                extra: runtime_extra,
            },
            requirements: crate::domain::environments::EnvironmentRequirements {
                system: Vec::new(),
                r_packages: Vec::new(),
                notes: Vec::new(),
            },
            diagnostics: crate::domain::environments::EnvironmentDiagnostics {
                install_hint: None,
                check_command: Vec::new(),
                notes: Vec::new(),
            },
        }
    }

    #[test]
    fn explicit_lock_profile_takes_lock_source() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let profile = build_conda_profile(
            root,
            "alignment",
            "conda",
            &[("condaLockFile", "./explicit.lock")],
        );
        let lock = root.join("environments/alignment/explicit.lock");
        let lock_bytes =
            b"https://conda.anaconda.org/conda-forge/noarch::demoalign-1.0-py_0.tar.bz2\n";
        fs::write(&lock, lock_bytes).expect("write lock");
        fs::write(
            root.join("environments/alignment/conda.yaml"),
            "channels: []\n",
        )
        .expect("write yaml");

        let selection = operator_conda_environment_selection(
            &crate::domain::tools::ToolContext::new(root.to_path_buf()),
            &profile,
            crate::domain::operators::OperatorExecutionSurfaceKind::Local,
        )
        .expect("select lock");

        assert_eq!(selection.kind, CondaSourceKind::ExplicitLock);
        assert_eq!(selection.source_filename, "explicit.lock");
        assert_eq!(selection.env_hash, super::sha256_hex(lock_bytes));
        assert_eq!(
            selection.source_b64,
            base64::engine::general_purpose::STANDARD.encode(lock_bytes)
        );
    }

    #[test]
    fn explicit_lock_convention_candidate_selected_when_singleton() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let profile = build_conda_profile(root, "alignment", "conda", &[]);
        let lock = root.join("environments/alignment/conda-linux-64.lock");
        let lock_bytes =
            b"https://conda.anaconda.org/conda-forge/noarch::demoalign-1.0-py_0.tar.bz2\n";
        fs::write(&lock, lock_bytes).expect("write lock");

        let selection = operator_conda_environment_selection(
            &crate::domain::tools::ToolContext::new(root.to_path_buf()),
            &profile,
            crate::domain::operators::OperatorExecutionSurfaceKind::Local,
        )
        .expect("select lock");

        assert_eq!(selection.kind, CondaSourceKind::ExplicitLock);
        assert_eq!(selection.source_filename, "conda-linux-64.lock");
        assert_eq!(selection.env_hash, super::sha256_hex(lock_bytes));
    }

    #[test]
    fn explicit_lock_path_traversal_is_rejected() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let profile = build_conda_profile(
            root,
            "alignment",
            "conda",
            &[("condaLockFile", "../outside.conda")],
        );
        let outside = root.join("outside.conda");
        fs::write(&outside, "https://bad.example\n").expect("write lock outside");
        fs::write(
            root.join("environments/alignment/conda.yaml"),
            "channels: []\n",
        )
        .expect("write yaml");

        let error = operator_conda_environment_selection(
            &crate::domain::tools::ToolContext::new(root.to_path_buf()),
            &profile,
            crate::domain::operators::OperatorExecutionSurfaceKind::Local,
        )
        .unwrap_err();

        assert!(error.contains("path with traversal"));
    }

    #[test]
    fn yaml_selection_is_used_when_no_lock() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let profile = build_conda_profile(root, "alignment", "conda", &[]);
        let yaml = root.join("environments/alignment/conda.yaml");
        let yaml_bytes = b"name: test\ndependencies:\n  - demoalign\n";
        fs::write(&yaml, yaml_bytes).expect("write yaml");

        let selection = operator_conda_environment_selection(
            &crate::domain::tools::ToolContext::new(root.to_path_buf()),
            &profile,
            crate::domain::operators::OperatorExecutionSurfaceKind::Local,
        )
        .expect("select yaml");

        assert_eq!(selection.kind, CondaSourceKind::Yaml);
        assert_eq!(selection.source_filename, "conda-environment.yaml");
        assert_eq!(selection.env_hash, super::sha256_hex(yaml_bytes));
        assert_eq!(
            selection.source_b64,
            base64::engine::general_purpose::STANDARD.encode(yaml_bytes)
        );
    }

    #[test]
    fn shell_script_preserves_yaml_path_and_uses_explicit_lock_source() {
        let selection = OperatorCondaEnvironmentSelection {
            env_prefix: "/tmp/conda-prefix".to_string(),
            source_b64: base64::engine::general_purpose::STANDARD.encode("@EXPLICIT\n"),
            source_filename: "conda-linux-64.lock".to_string(),
            kind: CondaSourceKind::ExplicitLock,
            env_hash: "abcd1234".to_string(),
            env_vars: BTreeMap::new(),
        };
        let command = conda_environment_shell_script(&selection, "/tmp/oprun_conda", "demoalign");

        assert!(command.contains("OMIGA_CONDA_SOURCE_KIND='explicit_lock'"));
        assert!(command.contains("OMIGA_CONDA_SRC='/tmp/oprun_conda/env/conda-linux-64.lock'"));
        assert!(command.contains(
            "OMIGA_CONDA_FINGERPRINT_FILE='/tmp/oprun_conda/logs/conda-env-explicit.txt'"
        ));
        assert!(command.contains("case \"$OMIGA_CONDA_MANAGER_KIND\" in"));
        assert!(
            command.contains(
                "\"$OMIGA_CONDA_BIN\" env export -p \"$OMIGA_CONDA_PREFIX\" --explicit --md5 > \"$OMIGA_CONDA_FINGERPRINT_FILE\" 2>/dev/null || true"
            )
        );
        assert!(
            command.contains(
                "\"$OMIGA_CONDA_BIN\" list -p \"$OMIGA_CONDA_PREFIX\" --explicit --md5 > \"$OMIGA_CONDA_FINGERPRINT_FILE\" 2>/dev/null || true"
            )
        );
        assert!(
            command.contains("create -y -p \"$OMIGA_CONDA_PREFIX\" --file \"$OMIGA_CONDA_SRC\"")
        );
        assert!(command.contains("printf %s"));
        assert!(command.contains("> \"$OMIGA_CONDA_SRC\""));
    }

    #[test]
    fn shell_script_preserves_yaml_path_and_exports_fingerprint() {
        let selection = OperatorCondaEnvironmentSelection {
            env_prefix: "/tmp/conda-prefix".to_string(),
            source_b64: base64::engine::general_purpose::STANDARD.encode("name: test\n"),
            source_filename: "conda-environment.yaml".to_string(),
            kind: CondaSourceKind::Yaml,
            env_hash: "abcd1234".to_string(),
            env_vars: BTreeMap::new(),
        };
        let command = conda_environment_shell_script(&selection, "/tmp/oprun_conda", "demoalign");

        assert!(command.contains("OMIGA_CONDA_SOURCE_KIND='yaml'"));
        assert!(command.contains("OMIGA_CONDA_SRC='/tmp/oprun_conda/env/conda-environment.yaml'"));
        assert!(
            command.contains(
                "\"$OMIGA_CONDA_BIN\" env export -p \"$OMIGA_CONDA_PREFIX\" --explicit --md5 > \"$OMIGA_CONDA_FINGERPRINT_FILE\" 2>/dev/null || true"
            )
        );
        assert!(
            command.contains(
                "\"$OMIGA_CONDA_BIN\" list -p \"$OMIGA_CONDA_PREFIX\" --explicit --md5 > \"$OMIGA_CONDA_FINGERPRINT_FILE\" 2>/dev/null || true"
            )
        );
    }
}
