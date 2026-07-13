use crate::domain::environments::{
    check_environment_profile, discover_environment_manifest_paths, environment_summary,
    load_environment_manifest, EnvironmentCheckResult, EnvironmentProfileSummary,
};
use crate::domain::operators::MICROMAMBA_BOOTSTRAP_SHELL;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::time::Instant;

use super::{
    active_plugin_root, plugin_store_root, read_marketplace, resolve_marketplace_source_path,
    PluginEnvironmentCheckResult, PluginId,
};

pub(crate) fn plugin_environment_runtime_file(
    manifest_path: &str,
    runtime: &crate::domain::environments::EnvironmentRuntimeProfile,
    runtime_type: &str,
) -> (Option<String>, Option<String>) {
    let manifest = PathBuf::from(manifest_path);
    let manifest_dir = manifest.parent().map(Path::to_path_buf);
    let extra_path = |keys: &[&str]| -> Option<PathBuf> {
        keys.iter()
            .find_map(|key| runtime.extra.get(*key).and_then(JsonValue::as_str))
            .map(|raw| {
                let path = PathBuf::from(raw.trim());
                if path.is_absolute() {
                    path
                } else {
                    manifest_dir
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(path)
                }
            })
    };
    let candidate = match runtime_type {
        "conda" | "mamba" | "micromamba" => extra_path(&[
            "condaEnvFile",
            "conda_env_file",
            "condaFile",
            "conda_file",
            "environmentFile",
            "environment_file",
        ])
        .or_else(|| {
            let dir = manifest_dir?;
            ["conda.yaml", "conda.yml"]
                .into_iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        })
        .map(|path| (path, "conda.yaml|conda.yml".to_string())),
        "docker" => extra_path(&["dockerfile", "dockerFile"])
            .or_else(|| {
                let path = manifest_dir?.join("Dockerfile");
                path.is_file().then_some(path)
            })
            .map(|path| (path, "Dockerfile".to_string())),
        "singularity" => extra_path(&[
            "definitionFile",
            "definition_file",
            "singularityDef",
            "singularity_def",
        ])
        .or_else(|| {
            let path = manifest_dir?.join("singularity.def");
            path.is_file().then_some(path)
        })
        .map(|path| (path, "singularity.def".to_string())),
        _ => None,
    };
    match candidate {
        Some((path, kind)) => (Some(path.to_string_lossy().into_owned()), Some(kind)),
        None => (None, None),
    }
}

pub(crate) fn plugin_environment_availability(
    runtime: &crate::domain::environments::EnvironmentRuntimeProfile,
    runtime_type: &str,
) -> (String, Option<String>, String) {
    let result = match runtime_type {
        "conda" | "mamba" | "micromamba" => find_conda_manager(),
        "docker" => find_executable_on_path("docker").map(|path| ("docker".to_string(), path)),
        "singularity" => find_executable_on_path("singularity")
            .map(|path| ("singularity".to_string(), path))
            .or_else(|| {
                find_executable_on_path("apptainer").map(|path| ("apptainer".to_string(), path))
            }),
        "system" | "local" | "host" => runtime
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|command| {
                find_executable_on_path(command).map(|path| (command.to_string(), path))
            }),
        _ => None,
    };
    if let Some((manager, path)) = result {
        (
            "available".to_string(),
            Some(manager.clone()),
            format!(
                "Found `{manager}` at {} in the Omiga app process PATH.",
                path.display()
            ),
        )
    } else {
        (
            "missing".to_string(),
            None,
            match runtime_type {
                "conda" | "mamba" | "micromamba" => "No micromamba, mamba, or conda executable was found in the Omiga app process PATH. Operator execution checks the selected base/virtual environment again.".to_string(),
                "docker" => "Docker CLI was not found in the Omiga app process PATH. Install Docker Desktop/Engine and ensure `docker` is available.".to_string(),
                "singularity" => "Neither singularity nor apptainer was found in the Omiga app process PATH.".to_string(),
                "system" | "local" | "host" => "Profile runtime.command was not found or not configured in PATH.".to_string(),
                other => format!("Runtime type `{other}` is not supported by plugin-level availability probing."),
            },
        )
    }
}

pub fn check_plugin_environment(
    plugin_id: &str,
    marketplace_path: Option<&Path>,
    plugin_name: Option<&str>,
    env_ref: &str,
    project_root: Option<&Path>,
) -> Result<PluginEnvironmentCheckResult, String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let installed_root = active_plugin_root(&plugin_id);
    let (plugin_root, installed) = if let Some(root) = installed_root {
        (root, true)
    } else {
        let marketplace_path = marketplace_path
            .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;
        let plugin_name = plugin_name.unwrap_or(&plugin_id.name);
        let marketplace = read_marketplace(marketplace_path)?;
        let entry = marketplace
            .plugins
            .iter()
            .find(|entry| entry.name == plugin_name)
            .ok_or_else(|| {
                format!(
                    "plugin `{plugin_name}` not found in marketplace `{}`",
                    marketplace.name
                )
            })?;
        (
            resolve_marketplace_source_path(marketplace_path, &entry.source)?,
            false,
        )
    };

    let needle = env_ref.trim();
    if needle.is_empty() {
        return Err("environment id must not be empty".to_string());
    }
    let Some(profile) = discover_environment_manifest_paths(&plugin_root)
        .into_iter()
        .filter_map(|manifest_path| {
            load_environment_manifest(&manifest_path, plugin_id.key(), &plugin_root).ok()
        })
        .map(environment_summary)
        .find(|profile| {
            profile.id == needle
                || profile.canonical_id == needle
                || profile
                    .canonical_id
                    .rsplit('/')
                    .next()
                    .is_some_and(|tail| tail == needle)
        })
    else {
        return Err(format!(
            "environment `{needle}` was not found in plugin `{}`",
            plugin_id.key()
        ));
    };

    let runtime_type = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let check = if matches!(runtime_type.as_str(), "conda" | "mamba" | "micromamba") {
        check_conda_plugin_environment(&profile, project_root)?
    } else {
        check_environment_profile(&profile)
    };

    Ok(PluginEnvironmentCheckResult {
        plugin_id: plugin_id.key(),
        environment_id: profile.id,
        canonical_id: profile.canonical_id,
        installed,
        plugin_root: plugin_root.to_string_lossy().into_owned(),
        check,
    })
}

fn check_conda_plugin_environment(
    profile: &EnvironmentProfileSummary,
    project_root: Option<&Path>,
) -> Result<EnvironmentCheckResult, String> {
    let command = profile.diagnostics.check_command.clone();
    if command.is_empty() {
        return Ok(EnvironmentCheckResult {
            status: "notConfigured".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "environment profile does not declare diagnostics.checkCommand".to_string(),
            ),
            duration_ms: 0,
        });
    }
    let conda_file = plugin_conda_environment_file(profile)?;
    if !is_allowed_plugin_environment_check_command(profile, &command, &conda_file) {
        return Ok(EnvironmentCheckResult {
            status: "blocked".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "diagnostics.checkCommand is not in the safe plugin environment-check allowlist"
                    .to_string(),
            ),
            duration_ms: 0,
        });
    }

    let bytes = std::fs::read(&conda_file).map_err(|err| {
        format!(
            "Read conda environment file `{}`: {err}",
            conda_file.display()
        )
    })?;
    let env_hash = sha256_hex(&bytes);
    let env_key = format!(
        "{}-{}",
        safe_environment_component(&profile.canonical_id),
        &env_hash[..12]
    );
    let project_root = project_root
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| plugin_store_root()));
    let env_prefix = project_root
        .join(".omiga/operator-envs/conda")
        .join(env_key);
    let started = Instant::now();
    let script = conda_environment_check_shell_script(
        &env_prefix,
        &conda_file,
        &env_hash,
        &profile.runtime.env,
        &shell_join(&command),
    );
    match std::process::Command::new("/bin/sh")
        .arg("-lc")
        .arg(script)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(4000)
                .collect::<String>();
            let stderr = String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4000)
                .collect::<String>();
            let success = output.status.success()
                || plugin_environment_check_accepts_nonzero_version_output(
                    &command, &stdout, &stderr,
                );
            Ok(EnvironmentCheckResult {
                status: if success {
                    "available".to_string()
                } else {
                    "unavailable".to_string()
                },
                command,
                exit_code: output.status.code(),
                stdout,
                stderr,
                error: None,
                duration_ms: started.elapsed().as_millis(),
            })
        }
        Err(err) => Ok(EnvironmentCheckResult {
            status: "unavailable".to_string(),
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(err.to_string()),
            duration_ms: started.elapsed().as_millis(),
        }),
    }
}

fn plugin_environment_check_accepts_nonzero_version_output(
    command: &[String],
    stdout: &str,
    stderr: &str,
) -> bool {
    if !plugin_environment_check_uses_version_arg(command) {
        return false;
    }
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    combined.contains("version")
}

fn plugin_environment_check_uses_version_arg(command: &[String]) -> bool {
    let args = command
        .iter()
        .skip(1)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    matches!(args.as_slice(), [arg] if matches!(arg.as_str(), "--version" | "-v" | "version"))
}

fn conda_dependency_name(raw: &str) -> Option<String> {
    let package = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .rsplit("::")
        .next()
        .unwrap_or(raw)
        .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '=' | '<' | '>' | '!' | '~'))
        .next()?
        .trim();
    (!package.is_empty()).then(|| package.to_ascii_lowercase())
}

fn conda_environment_declares_executable(env_yaml: &Path, executable: &str) -> bool {
    let wanted = executable.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return false;
    }
    let Ok(raw) = std::fs::read_to_string(env_yaml) else {
        return false;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&raw) else {
        return false;
    };
    let Some(dependencies) = value
        .get("dependencies")
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return false;
    };
    dependencies.iter().any(|dependency| {
        dependency
            .as_str()
            .and_then(conda_dependency_name)
            .is_some_and(|package| package == wanted)
    })
}

pub(crate) fn plugin_conda_environment_file(
    profile: &EnvironmentProfileSummary,
) -> Result<PathBuf, String> {
    let manifest = PathBuf::from(&profile.manifest_path);
    let manifest_dir = manifest.parent().ok_or_else(|| {
        format!(
            "Environment profile `{}` has no manifest parent directory.",
            profile.canonical_id
        )
    })?;
    for key in [
        "condaEnvFile",
        "conda_env_file",
        "condaFile",
        "conda_file",
        "environmentFile",
        "environment_file",
    ] {
        if let Some(raw) = profile.runtime.extra.get(key).and_then(JsonValue::as_str) {
            let path = if Path::new(raw.trim()).is_absolute() {
                PathBuf::from(raw.trim())
            } else {
                manifest_dir.join(raw.trim())
            };
            validate_plugin_conda_yaml_path(profile, &path)?;
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

fn validate_plugin_conda_yaml_path(
    profile: &EnvironmentProfileSummary,
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

pub(crate) fn conda_environment_check_shell_script(
    env_prefix: &Path,
    env_yaml: &Path,
    env_hash: &str,
    env_vars: &BTreeMap<String, String>,
    inner_command: &str,
) -> String {
    let exports = crate::domain::env_hygiene::shell_export_lines(env_vars);
    format!(
        r#"{bootstrap}
set -e
OMIGA_CONDA_PREFIX={env_prefix}
OMIGA_CONDA_YAML={env_yaml}
OMIGA_CONDA_HASH={env_hash}
OMIGA_MICROMAMBA="${{OMIGA_MICROMAMBA:-$HOME/.omiga/bin/micromamba}}"
mkdir -p "$(dirname "$OMIGA_CONDA_PREFIX")"
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
omiga_find_conda_manager || true
if [ -z "$OMIGA_CONDA_BIN" ]; then
  omiga_bootstrap_micromamba || true
fi
if [ -z "$OMIGA_CONDA_BIN" ]; then
  cat >&2 <<'OMIGA_CONDA_HINT'
No micromamba, mamba, or conda executable was found in the active PATH/base environment/virtual environment.
Automatic micromamba installation failed (reason above).
Install the official micromamba binary at $HOME/.omiga/bin/micromamba, set OMIGA_MICROMAMBA=/absolute/path/to/micromamba, or set OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1 to disable bootstrap.
OMIGA_CONDA_HINT
  exit 127
fi
if [ ! -f "$OMIGA_CONDA_PREFIX/.omiga-env-hash" ] || [ "$(cat "$OMIGA_CONDA_PREFIX/.omiga-env-hash" 2>/dev/null || true)" != "$OMIGA_CONDA_HASH" ]; then
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
\"$OMIGA_CONDA_BIN\" run -p "$OMIGA_CONDA_PREFIX" /bin/sh -lc {inner}
"#,
        env_prefix = sh_quote(&env_prefix.to_string_lossy()),
        env_yaml = sh_quote(&env_yaml.to_string_lossy()),
        env_hash = sh_quote(env_hash),
        exports = exports,
        inner = sh_quote(inner_command),
        bootstrap = MICROMAMBA_BOOTSTRAP_SHELL,
    )
}

pub(crate) fn is_allowed_plugin_environment_check_command(
    profile: &EnvironmentProfileSummary,
    command: &[String],
    conda_file: &Path,
) -> bool {
    let Some(executable) = command.first() else {
        return false;
    };
    let basename = Path::new(executable)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(executable)
        .trim()
        .to_ascii_lowercase();
    let args = command
        .iter()
        .skip(1)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let version_arg = match args.as_slice() {
        [] => true,
        [arg] => matches!(arg.as_str(), "--version" | "-v" | "version"),
        _ => false,
    };
    let is_bare_executable = executable == &basename
        && !basename.is_empty()
        && basename
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+'));
    if !is_bare_executable || !version_arg {
        return false;
    }
    let runtime_command_matches = profile
        .runtime
        .command
        .as_deref()
        .map(|command| {
            Path::new(command)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(command)
                .trim()
                .eq_ignore_ascii_case(&basename)
        })
        .unwrap_or(false);
    runtime_command_matches || conda_environment_declares_executable(conda_file, &basename)
}

fn shell_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| sh_quote(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn safe_environment_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').trim_matches('.');
    if out.is_empty() {
        "environment".to_string()
    } else {
        out.to_string()
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn find_conda_manager() -> Option<(String, PathBuf)> {
    if let Ok(raw) = std::env::var("OMIGA_MICROMAMBA") {
        let path = PathBuf::from(raw.trim());
        if path.is_file() {
            return Some(("micromamba".to_string(), path));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(".omiga/bin/micromamba");
        if path.is_file() {
            return Some(("micromamba".to_string(), path));
        }
    }
    ["micromamba", "mamba", "conda"]
        .into_iter()
        .find_map(|name| find_executable_on_path(name).map(|path| (name.to_string(), path)))
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate);
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).find_map(|dir| {
        let path = dir.join(name);
        path.is_file().then_some(path)
    })
}
