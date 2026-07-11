use crate::domain::environments::{
    environment_summary, EnvironmentProfileSummary, EnvironmentSpecWithSource,
};
use crate::domain::tools::env_store;
use crate::domain::tools::ToolContext;
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const ENVIRONMENT_STATE_DIR_NAME: &str = ".omiga";
const ENVIRONMENTS_SUBDIR: &str = "environments";
const AVAILABILITY_FILE_NAME: &str = "availability.json";
static CACHE_WRITE_LOCK: Mutex<()> = Mutex::new(());
const CONDA_MANAGER_PROBE_SCRIPT: &str = r#"
if [ -n "${OMIGA_MICROMAMBA:-}" ] && [ -x "$OMIGA_MICROMAMBA" ]; then
  printf 'micromamba\t%s\n' "$OMIGA_MICROMAMBA"
  exit 0
fi
if [ -x "$HOME/.omiga/bin/micromamba" ]; then
  printf 'micromamba\t%s\n' "$HOME/.omiga/bin/micromamba"
  exit 0
fi
for name in micromamba mamba conda; do
  if found=$(command -v "$name" 2>/dev/null) && [ -n "$found" ]; then
    printf '%s\t%s\n' "$name" "$found"
    exit 0
  fi
done
  exit 127
"#;

#[derive(Debug, Clone)]
pub enum RefreshScope {
    All,
    Plugin(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentAvailabilityCache {
    #[serde(default)]
    pub records: BTreeMap<String, EnvironmentAvailabilityRecord>,
    #[serde(default, rename = "updatedAtMs")]
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentAvailabilityRecord {
    pub canonical_id: String,
    pub runtime_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manager: Option<String>,
    #[serde(rename = "executablePath", skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub message: String,
    #[serde(rename = "installHint", skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
    pub checked_at_ms: u64,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prewarm_status: Option<String>,
    #[serde(default)]
    pub prewarmed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prewarm_error: Option<String>,
}

impl EnvironmentAvailabilityRecord {
    pub fn as_json_value(&self) -> JsonValue {
        let mut value = serde_json::json!({
            "status": self.status,
            "runtimeType": self.runtime_type,
            "manager": self.manager,
            "executablePath": self.executable_path,
            "message": self.message,
            "installHint": self.install_hint,
            "checked": match self.runtime_type.as_str() {
                "conda" | "mamba" | "micromamba" => {
                    serde_json::json!(["OMIGA_MICROMAMBA", "$HOME/.omiga/bin/micromamba", "micromamba", "mamba", "conda"])
                }
                "docker" => serde_json::json!(["docker"]),
                "singularity" => serde_json::json!(["singularity", "apptainer"]),
                _ => JsonValue::Null,
            },
            "scope": self.scope,
            "canonicalId": self.canonical_id,
            "checkedAtMs": self.checked_at_ms,
        });
        if let Some(error) = &self.error {
            value["error"] = serde_json::json!(error);
        }
        if let Some(prewarm_status) = &self.prewarm_status {
            value["prewarmStatus"] = serde_json::json!(prewarm_status);
        }
        if let Some(prewarmed_at_ms) = self.prewarmed_at_ms {
            value["prewarmedAtMs"] = serde_json::json!(prewarmed_at_ms);
        }
        if let Some(error) = &self.prewarm_error {
            value["prewarmError"] = serde_json::json!(error);
        }
        value
    }

    pub fn checked_at_iso8601_ms(checked_at_ms: u64) -> String {
        let Ok(checked_at_ms) = i64::try_from(checked_at_ms) else {
            return checked_at_ms.to_string();
        };
        chrono::Utc
            .timestamp_millis_opt(checked_at_ms)
            .single()
            .map(|value: chrono::DateTime<chrono::Utc>| value.to_rfc3339())
            .unwrap_or_else(|| checked_at_ms.to_string())
    }
}

pub fn cache_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(ENVIRONMENT_STATE_DIR_NAME)
        .join(ENVIRONMENTS_SUBDIR)
        .join(AVAILABILITY_FILE_NAME)
}

pub fn load_cache() -> EnvironmentAvailabilityCache {
    load_cache_at_path(&cache_file_path())
}

pub fn load_cache_at_path(path: &Path) -> EnvironmentAvailabilityCache {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return EnvironmentAvailabilityCache::default(),
    };
    serde_json::from_str::<EnvironmentAvailabilityCache>(&raw).unwrap_or_default()
}

pub fn store_record(record: &EnvironmentAvailabilityRecord) -> Result<(), String> {
    store_records(std::slice::from_ref(record))
}

pub fn store_record_at_path(
    path: &Path,
    record: &EnvironmentAvailabilityRecord,
) -> Result<(), String> {
    store_records_at_path(path, std::slice::from_ref(record))
}

pub fn store_records(records: &[EnvironmentAvailabilityRecord]) -> Result<(), String> {
    store_records_at_path(&cache_file_path(), records)
}

pub fn store_records_at_path(
    path: &Path,
    records: &[EnvironmentAvailabilityRecord],
) -> Result<(), String> {
    if records.is_empty() {
        return Ok(());
    }
    mutate_cache_with_lock(path, |cache| {
        for record in records {
            let mut merged = record.clone();
            if merged.prewarm_status.is_none() {
                // 探测记录从不携带 prewarm 字段，None 即视为“未涉及”而继承。
                if let Some(previous) = cache.records.get(&record.canonical_id) {
                    merged.prewarm_status = previous.prewarm_status.clone();
                    merged.prewarmed_at_ms = previous.prewarmed_at_ms;
                    merged.prewarm_error = previous.prewarm_error.clone();
                }
            }
            cache.records.insert(merged.canonical_id.clone(), merged);
        }
    })
}

pub fn mutate_record_at_path(
    path: &Path,
    canonical_id: &str,
    mutate: impl FnOnce(Option<EnvironmentAvailabilityRecord>) -> EnvironmentAvailabilityRecord,
) -> Result<(), String> {
    mutate_cache_with_lock(path, |cache| {
        let current = cache.records.remove(canonical_id);
        let merged = mutate(current);
        cache.records.insert(merged.canonical_id.clone(), merged);
    })
}

pub fn replace_records_at_path(
    path: &Path,
    scope: RefreshScope,
    records: &[EnvironmentAvailabilityRecord],
) -> Result<(), String> {
    mutate_cache_with_lock(path, |cache| {
        let pre_existing = cache.records.clone();
        match scope {
            RefreshScope::All => {
                cache.records.clear();
            }
            RefreshScope::Plugin(plugin_id) => {
                let prefix = format!("{plugin_id}/environment/");
                cache
                    .records
                    .retain(|canonical_id, _| !canonical_id.starts_with(&prefix));
            }
        }
        for record in records {
            let mut merged = record.clone();
            if let Some(previous) = pre_existing.get(&record.canonical_id) {
                merged.prewarm_status = previous.prewarm_status.clone();
                merged.prewarmed_at_ms = previous.prewarmed_at_ms;
                merged.prewarm_error = previous.prewarm_error.clone();
            }
            cache.records.insert(merged.canonical_id.clone(), merged);
        }
    })
}

fn refresh_scope_for_plugin_id(plugin_id: Option<&str>) -> RefreshScope {
    plugin_id
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then_some(RefreshScope::Plugin(trimmed.to_string()))
        })
        .unwrap_or(RefreshScope::All)
}

fn mutate_cache_with_lock(
    path: &Path,
    mutate: impl FnOnce(&mut EnvironmentAvailabilityCache),
) -> Result<(), String> {
    let _guard = CACHE_WRITE_LOCK
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    let mut cache = load_cache_at_path(path);
    mutate(&mut cache);
    cache.updated_at_ms = current_epoch_ms();
    write_cache_to_path(path, &cache)
}

fn write_cache_to_path(path: &Path, cache: &EnvironmentAvailabilityCache) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create environment availability cache dir: {err}"))?;
    }
    let rendered = serde_json::to_string_pretty(cache)
        .map_err(|err| format!("serialize environment availability cache: {err}"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(AVAILABILITY_FILE_NAME);
    let temp_path = path.with_file_name(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        current_epoch_ms(),
    ));
    fs::write(&temp_path, format!("{rendered}\n"))
        .map_err(|err| format!("write environment availability cache temp file: {err}"))?;
    fs::rename(&temp_path, path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        format!("replace environment availability cache file: {err}")
    })?;
    Ok(())
}

pub fn cached_record(canonical_id: &str) -> Option<EnvironmentAvailabilityRecord> {
    cached_record_at_path(&cache_file_path(), canonical_id)
}

pub fn cached_record_at_path(
    path: &Path,
    canonical_id: &str,
) -> Option<EnvironmentAvailabilityRecord> {
    load_cache_at_path(path).records.get(canonical_id).cloned()
}

async fn probe_and_cache_profiles_with_context_async(
    ctx: &ToolContext,
    profiles: &[EnvironmentSpecWithSource],
) -> Vec<EnvironmentAvailabilityRecord> {
    let mut records = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let summary = environment_summary(profile.clone());
        let record = runtime_availability_for_profile_async(ctx, &summary).await;
        records.push(record);
    }
    let _ = store_records(&records);
    records
}

#[cfg(test)]
async fn probe_and_cache_profiles_at_path_async(
    path: &Path,
    profiles: &[EnvironmentSpecWithSource],
) -> Vec<EnvironmentAvailabilityRecord> {
    let ctx = ToolContext::new(std::env::temp_dir());
    let records = probe_and_cache_profiles_with_context_async(&ctx, profiles).await;
    let _ = store_records_at_path(path, &records);
    records
}

pub async fn probe_and_cache_enabled_profiles_with_context_async(
    ctx: &ToolContext,
    plugin_id: Option<&str>,
) -> Vec<EnvironmentAvailabilityRecord> {
    let scope = refresh_scope_for_plugin_id(plugin_id);
    let profiles = environment_profiles_for_refresh(plugin_id);
    let records = probe_and_cache_profiles_with_context_async(ctx, &profiles).await;
    let _ = replace_records_at_path(&cache_file_path(), scope, &records);
    records
}

pub async fn probe_profile_and_cache_async(
    ctx: &ToolContext,
    profile: &EnvironmentProfileSummary,
) -> EnvironmentAvailabilityRecord {
    let record = runtime_availability_for_profile_async(ctx, profile).await;
    if let Err(err) = store_record(&record) {
        tracing::warn!(
            canonical_id = %record.canonical_id,
            error = %err,
            "failed to cache environment availability record"
        );
    }
    record
}

pub async fn runtime_availability_for_profile_async(
    ctx: &ToolContext,
    profile: &EnvironmentProfileSummary,
) -> EnvironmentAvailabilityRecord {
    let runtime_type = profile
        .runtime
        .kind
        .as_deref()
        .unwrap_or("system")
        .trim()
        .to_ascii_lowercase();
    let scope = ctx.execution_environment.clone();

    match runtime_type.as_str() {
        "conda" | "mamba" | "micromamba" => {
            probe_conda_manager(ctx, &scope, &profile.canonical_id).await
        }
        "docker" => {
            probe_single_runtime(
                ctx,
                &scope,
                profile,
                "docker",
                &["docker"],
                "Docker runtime is required but `docker` was not found in the active PATH/base environment/virtual environment.",
                "Install Docker Desktop/Engine, make the docker CLI available in the selected environment, start the Docker daemon, then retry. Operator execution will run `docker version` before use.",
            )
            .await
        }
        "singularity" => {
            probe_single_runtime(
                ctx,
                &scope,
                profile,
                "singularity",
                &["singularity", "apptainer"],
                "Singularity/Apptainer is required but neither `singularity` nor `apptainer` was found in the active PATH/base environment/virtual environment.",
                "Install SingularityCE or Apptainer and make `singularity` or `apptainer` available in the selected environment, then retry.",
            )
            .await
        }
        "system" | "local" | "host" => probe_system_command(ctx, &scope, profile).await,
        other => EnvironmentAvailabilityRecord {
            canonical_id: profile.canonical_id.clone(),
            runtime_type: other.to_string(),
            status: "unsupported".to_string(),
            manager: None,
            executable_path: None,
            error: None,
            message: format!(
                "Environment runtime.type `{other}` is not supported by runtime availability probing."
            ),
            install_hint: Some(runtime_install_hint(other)),
            checked_at_ms: current_epoch_ms(),
            scope,
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        },
    }
}

async fn probe_conda_manager(
    ctx: &ToolContext,
    scope: &str,
    canonical_id: &str,
) -> EnvironmentAvailabilityRecord {
    match run_local_probe_async(ctx, CONDA_MANAGER_PROBE_SCRIPT).await {
        Ok((manager, path)) => EnvironmentAvailabilityRecord {
            canonical_id: canonical_id.to_string(),
            runtime_type: "conda".to_string(),
            status: "available".to_string(),
            manager: Some(manager),
            executable_path: Some(path),
            error: None,
            message: "A conda-compatible manager was found in the active PATH/base environment/virtual environment.".to_string(),
            install_hint: Some(runtime_install_hint("conda")),
            checked_at_ms: current_epoch_ms(),
            scope: scope.to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        },
        Err(error) => missing_or_not_run_probe_record(
            canonical_id,
            "conda",
            error,
            scope,
            "No micromamba, mamba, or conda executable was found in the active PATH/base environment/virtual environment. Operator execution will bootstrap micromamba from official releases to $HOME/.omiga/bin/micromamba when needed.",
            &runtime_install_hint("conda"),
        ),
    }
}

async fn probe_single_runtime(
    ctx: &ToolContext,
    scope: &str,
    profile: &EnvironmentProfileSummary,
    runtime_type: &str,
    candidates: &[&str],
    missing_message: &str,
    install_hint: &str,
) -> EnvironmentAvailabilityRecord {
    let script = candidates
        .iter()
        .map(|candidate| {
            format!(
                "if command -v {candidate} >/dev/null 2>&1; then printf '%s\\t%s\\n' {candidate_q} \"$(command -v {candidate})\"; exit 0; fi",
                candidate = shell_quote(candidate),
                candidate_q = shell_quote(candidate),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\nexit 127\n";
    match run_local_probe_async(ctx, &script).await {
        Ok((manager, path)) => EnvironmentAvailabilityRecord {
            canonical_id: profile.canonical_id.clone(),
            runtime_type: runtime_type.to_string(),
            status: "available".to_string(),
            manager: Some(manager.clone()),
            executable_path: Some(path),
            error: None,
            message: format!(
                "`{manager}` was found in the active PATH/base environment/virtual environment."
            ),
            install_hint: Some(install_hint.to_string()),
            checked_at_ms: current_epoch_ms(),
            scope: scope.to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        },
        Err(error) => missing_or_not_run_probe_record(
            &profile.canonical_id,
            runtime_type,
            error,
            scope,
            missing_message,
            install_hint,
        ),
    }
}

async fn probe_system_command(
    ctx: &ToolContext,
    scope: &str,
    profile: &EnvironmentProfileSummary,
) -> EnvironmentAvailabilityRecord {
    let Some(command) = profile
        .runtime
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return EnvironmentAvailabilityRecord {
            canonical_id: profile.canonical_id.clone(),
            runtime_type: profile.runtime.kind.as_deref().unwrap_or("system").to_string(),
            status: "notConfigured".to_string(),
            manager: None,
            executable_path: None,
            error: None,
            message: "System/local environment profile does not declare runtime.command; no executable probe was run."
                .to_string(),
            install_hint: profile.diagnostics.install_hint.clone(),
            checked_at_ms: current_epoch_ms(),
            scope: scope.to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        };
    };
    probe_single_runtime(
        ctx,
        scope,
        profile,
        "system",
        &[command],
        "The profile runtime.command was not found in the active PATH/base environment/virtual environment.",
        profile
            .diagnostics
            .install_hint
            .as_deref()
            .unwrap_or("Install the required command or make it available on PATH, then retry."),
    )
    .await
}

async fn run_local_probe_async(
    ctx: &ToolContext,
    script: &str,
) -> Result<(String, String), String> {
    let output = env_store::run_probe_shell(ctx, script, ctx.timeout_secs).await?;
    parse_probe_output(&output)
}

fn parse_probe_output(output: &str) -> Result<(String, String), String> {
    let mut parts = output
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .splitn(2, '\t');
    let manager = parts.next().unwrap_or_default().trim();
    let path = parts.next().unwrap_or_default().trim();
    if manager.is_empty() || path.is_empty() {
        return Err("probe did not return an executable path".to_string());
    }
    Ok((manager.to_string(), path.to_string()))
}

fn missing_or_not_run_probe_record(
    canonical_id: &str,
    runtime_type: &str,
    error: String,
    scope: &str,
    missing_message: &str,
    install_hint: &str,
) -> EnvironmentAvailabilityRecord {
    if is_remote_probe_missing(scope, &error) {
        EnvironmentAvailabilityRecord {
            canonical_id: canonical_id.to_string(),
            runtime_type: runtime_type.to_string(),
            status: "missing".to_string(),
            manager: None,
            executable_path: None,
            error: Some(error),
            message: missing_message.to_string(),
            install_hint: Some(install_hint.to_string()),
            checked_at_ms: current_epoch_ms(),
            scope: scope.to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        }
    } else {
        EnvironmentAvailabilityRecord {
            canonical_id: canonical_id.to_string(),
            runtime_type: runtime_type.to_string(),
            status: "notRun".to_string(),
            manager: None,
            executable_path: None,
            error: Some(error.clone()),
            message: format!("远端探测未能执行：{error}"),
            install_hint: Some(install_hint.to_string()),
            checked_at_ms: current_epoch_ms(),
            scope: scope.to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        }
    }
}

fn is_remote_probe_missing(scope: &str, error: &str) -> bool {
    if scope == "local" {
        return true;
    }
    if let Some(code) = parse_probe_exit_code(error) {
        return code == 127;
    }
    false
}

fn parse_probe_exit_code(error: &str) -> Option<i32> {
    let marker = "probe command failed with status code ";
    let rest = error.strip_prefix(marker)?;
    let code = rest.split(':').next()?.trim();
    code.parse().ok()
}

fn runtime_install_hint(runtime_type: &str) -> String {
    match runtime_type {
        "conda" | "mamba" | "micromamba" => {
            "Operator execution will auto-install micromamba to $HOME/.omiga/bin/micromamba when missing. For manual setup, install the official micromamba binary there, or set OMIGA_MICROMAMBA=/absolute/path/to/micromamba. Set OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1 to disable auto-install."
                .to_string()
        }
        "docker" => {
            "Install Docker Desktop/Engine, make the docker CLI available in the selected environment, and start the Docker daemon.".to_string()
        }
        "singularity" => {
            "Install SingularityCE or Apptainer and make singularity or apptainer available in the selected environment."
                .to_string()
        }
        _ => "Install the runtime required by this Environment profile or adjust runtime.type/runtime.command.".to_string(),
    }
}

fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

fn current_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub(crate) fn environment_profiles_for_refresh(
    plugin_id: Option<&str>,
) -> Vec<EnvironmentSpecWithSource> {
    let outcome = crate::domain::plugins::plugin_load_outcome();
    let plugin_id = plugin_id.map(str::trim).filter(|value| !value.is_empty());

    outcome
        .plugins()
        .iter()
        .filter(|plugin| plugin.is_active())
        .filter(|plugin| plugin_id.map(|target| plugin.id == target).unwrap_or(true))
        .flat_map(|plugin| {
            crate::domain::environments::discover_environment_manifest_paths(&plugin.root)
                .into_iter()
                .filter_map(move |manifest_path| {
                    crate::domain::environments::load_environment_manifest(
                        &manifest_path,
                        plugin.id.clone(),
                        plugin.root.clone(),
                    )
                    .ok()
                })
                .filter(|profile| {
                    crate::domain::plugins::environment_profile_enabled(
                        &plugin.id,
                        &profile.spec.metadata.id,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environments::{
        EnvironmentDiagnostics, EnvironmentMetadata, EnvironmentRequirements,
        EnvironmentRuntimeProfile, EnvironmentSource, EnvironmentSpec,
    };
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::process::Command;
    use std::thread;
    use tempfile::tempdir;

    static RUN_LOCAL_PROBE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ScopedEnv {
        key: &'static str,
        old: Option<OsString>,
    }

    impl ScopedEnv {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match self.old.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn sample_spec(
        profile_id: &str,
        command: &str,
        kind: &str,
        source_plugin: &str,
    ) -> EnvironmentSpecWithSource {
        let root = tempfile::tempdir().expect("tmp root");
        EnvironmentSpecWithSource {
            spec: EnvironmentSpec {
                api_version: "omiga.ai/environment/v1alpha1".to_string(),
                kind: "Environment".to_string(),
                metadata: EnvironmentMetadata {
                    id: profile_id.to_string(),
                    version: "0.1.0".to_string(),
                    name: None,
                    description: None,
                    tags: Vec::new(),
                },
                runtime: EnvironmentRuntimeProfile {
                    kind: Some(kind.to_string()),
                    command: Some(command.to_string()),
                    args: Vec::new(),
                    image: None,
                    module: None,
                    env: BTreeMap::new(),
                    extra: serde_json::Map::new(),
                },
                requirements: EnvironmentRequirements::default(),
                diagnostics: EnvironmentDiagnostics::default(),
            },
            source: EnvironmentSource {
                source_plugin: source_plugin.to_string(),
                plugin_root: root.path().to_path_buf(),
                manifest_path: root.path().join("environment.yaml"),
            },
        }
    }

    fn test_record(
        plugin_id: &str,
        profile_id: &str,
        status: &str,
    ) -> EnvironmentAvailabilityRecord {
        EnvironmentAvailabilityRecord {
            canonical_id: format!("{plugin_id}/environment/{profile_id}"),
            runtime_type: "system".to_string(),
            status: status.to_string(),
            manager: None,
            executable_path: None,
            error: None,
            message: format!("mock {status} record"),
            install_hint: None,
            checked_at_ms: 0,
            scope: "local".to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        }
    }

    fn test_record_with_prewarm(
        plugin_id: &str,
        profile_id: &str,
        status: &str,
        prewarm_status: Option<&str>,
        prewarmed_at_ms: Option<u64>,
        prewarm_error: Option<&str>,
    ) -> EnvironmentAvailabilityRecord {
        EnvironmentAvailabilityRecord {
            prewarm_status: prewarm_status.map(str::to_string),
            prewarmed_at_ms,
            prewarm_error: prewarm_error.map(str::to_string),
            ..test_record(plugin_id, profile_id, status)
        }
    }

    #[test]
    fn load_cache_missing_or_invalid_file_returns_default() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");

        assert_eq!(
            load_cache_at_path(&path),
            EnvironmentAvailabilityCache::default()
        );

        fs::write(&path, "{invalid").expect("invalid json");
        assert_eq!(
            load_cache_at_path(&path),
            EnvironmentAvailabilityCache::default()
        );
    }

    #[test]
    fn store_and_load_record_with_injected_path() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let record = EnvironmentAvailabilityRecord {
            canonical_id: "plugin@local/environment/sample".to_string(),
            runtime_type: "system".to_string(),
            status: "missing".to_string(),
            manager: None,
            executable_path: None,
            error: None,
            message: "missing".to_string(),
            install_hint: Some("install command".to_string()),
            checked_at_ms: 0,
            scope: "local".to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        };

        store_record_at_path(&path, &record).expect("store");
        let cached =
            cached_record_at_path(&path, "plugin@local/environment/sample").expect("cached");

        assert_eq!(cached.canonical_id, "plugin@local/environment/sample");
        assert_eq!(cached.status, "missing");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_and_cache_profiles_writes_records_at_path() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let profile = sample_spec(
            "cache-test",
            "this-command-does-not-exist-omiga",
            "system",
            "test-plugin@local",
        );

        let records =
            probe_and_cache_profiles_at_path_async(&path, std::slice::from_ref(&profile)).await;

        assert_eq!(records.len(), 1);
        let cached = cached_record_at_path(&path, &environment_summary(profile).canonical_id)
            .expect("should have cached record");
        assert_eq!(cached.status, "missing");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_local_probe_filters_sensitive_env_vars() {
        let _guard = RUN_LOCAL_PROBE_ENV_LOCK.lock().expect("environment lock");
        let _secret = ScopedEnv::set("FAKE_SECRET_TOKEN", "super-secret");
        let _keep = ScopedEnv::remove("OMIGA_ENV_KEEP");
        let tmp = tempdir().expect("temp dir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let script = r#"printf "micromamba\t${FAKE_SECRET_TOKEN:-absent}\n""#;

        let (manager, path) = run_local_probe_async(&ctx, script)
            .await
            .expect("probe succeeds");

        assert_eq!(manager, "micromamba");
        assert_eq!(path, "absent");
    }

    #[tokio::test]
    async fn remote_runtime_availability_without_env_store_marks_not_run() {
        let tmp = tempdir().expect("temp dir");
        let ctx = ToolContext::new(tmp.path().to_path_buf())
            .with_execution_environment("ssh")
            .with_ssh_server(Some("mock-gpu".to_string()));
        let profile = sample_spec("remote-conda", "dummy-cmd", "conda", "remote@plugin");
        let record =
            runtime_availability_for_profile_async(&ctx, &environment_summary(profile)).await;

        assert_eq!(record.scope, "ssh");
        assert_eq!(record.status, "notRun");
        assert!(record.message.contains("远端探测未能执行"));
        assert_eq!(record.error.as_deref(), Some("no execution environment"));
    }

    #[test]
    fn remote_probe_missing_status_distinguishes_missing_from_not_run_by_exit_code() {
        let missing = missing_or_not_run_probe_record(
            "remote-plugin/environment/conda",
            "conda",
            "probe command failed with status code 127".to_string(),
            "ssh",
            "missing runtime",
            "bootstrap it",
        );
        let not_run = missing_or_not_run_probe_record(
            "remote-plugin/environment/conda",
            "conda",
            "no execution environment".to_string(),
            "ssh",
            "missing runtime",
            "bootstrap it",
        );

        assert_eq!(missing.status, "missing");
        assert_eq!(not_run.status, "notRun");
        assert!(not_run.message.contains("远端探测未能执行"));
    }

    #[test]
    fn conda_manager_probe_script_prefers_path_order() {
        let tmp = tempdir().expect("tempdir");
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).expect("home directory");
        let fake_micromamba = tmp.path().join("micromamba");
        fs::write(&fake_micromamba, "#!/bin/sh\nexit 0\n").expect("write fake manager");
        let chmod_output = Command::new("chmod")
            .arg("+x")
            .arg(&fake_micromamba)
            .output()
            .expect("chmod fake manager");
        assert!(chmod_output.status.success(), "{:?}", chmod_output);
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(CONDA_MANAGER_PROBE_SCRIPT)
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", tmp.path().display()))
            .output()
            .expect("run conda probe script");

        assert!(output.status.success(), "{:?}", output);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default();
        assert_eq!(
            first_line,
            format!("micromamba\t{}", fake_micromamba.display())
        );
    }

    #[test]
    fn runtime_install_hint_for_conda_mentions_auto_bootstrap_and_disable_switch() {
        let hint = runtime_install_hint("conda");
        assert!(hint.contains("auto-install"));
        assert!(hint.contains("$HOME/.omiga/bin/micromamba"));
        assert!(hint.contains("OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP"));
    }

    #[test]
    fn replace_records_with_plugin_scope_removes_old_plugin_records() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let seed = vec![
            test_record("plugin-a@local", "old-a-1", "missing"),
            test_record("plugin-a@local", "old-a-2", "missing"),
            test_record("plugin-b@local", "keep", "available"),
        ];
        store_records_at_path(&path, &seed).expect("seed");

        let replacement = vec![test_record("plugin-a@local", "new-a", "available")];
        replace_records_at_path(
            &path,
            RefreshScope::Plugin("plugin-a@local".to_string()),
            &replacement,
        )
        .expect("replace plugin scope");

        let cache = load_cache_at_path(&path);
        assert!(cache
            .records
            .contains_key("plugin-a@local/environment/new-a"));
        assert!(!cache
            .records
            .contains_key("plugin-a@local/environment/old-a-1"));
        assert!(!cache
            .records
            .contains_key("plugin-a@local/environment/old-a-2"));
        assert!(cache
            .records
            .contains_key("plugin-b@local/environment/keep"));
    }

    #[test]
    fn replace_records_with_plugin_scope_preserves_matching_prewarm_fields() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let seed = vec![
            test_record_with_prewarm(
                "plugin-a@local",
                "keep",
                "available",
                Some("warmed"),
                Some(123),
                Some("bootstrap timeout"),
            ),
            test_record("plugin-a@local", "old-a", "missing"),
            test_record("plugin-b@local", "keep", "available"),
        ];
        store_records_at_path(&path, &seed).expect("seed");

        let replacement = vec![test_record("plugin-a@local", "keep", "notRun")];
        replace_records_at_path(
            &path,
            RefreshScope::Plugin("plugin-a@local".to_string()),
            &replacement,
        )
        .expect("replace plugin scope");

        let cache = load_cache_at_path(&path);
        assert_eq!(
            cache
                .records
                .get("plugin-a@local/environment/keep")
                .expect("kept record")
                .status,
            "notRun"
        );
        let keep = cache
            .records
            .get("plugin-a@local/environment/keep")
            .expect("kept record");
        assert_eq!(keep.prewarm_status.as_deref(), Some("warmed"));
        assert_eq!(keep.prewarmed_at_ms, Some(123));
        assert_eq!(keep.prewarm_error.as_deref(), Some("bootstrap timeout"));
        assert!(!cache
            .records
            .contains_key("plugin-a@local/environment/old-a"));
        assert!(cache
            .records
            .contains_key("plugin-b@local/environment/keep"));
    }

    #[test]
    fn store_records_at_path_preserves_existing_prewarm_when_probe_does_not_touch() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let canonical_id = "plugin-a@local/environment/prefetch";
        let original = test_record_with_prewarm(
            "plugin-a@local",
            "prefetch",
            "available",
            Some("warmed"),
            Some(1234),
            Some("bootstrap timeout"),
        );
        store_records_at_path(&path, &[original.clone()]).expect("store seed");

        let incoming = EnvironmentAvailabilityRecord {
            status: "missing".to_string(),
            checked_at_ms: 4321,
            ..test_record("plugin-a@local", "prefetch", "missing")
        };
        store_records_at_path(&path, &[incoming.clone()]).expect("store probe");

        let cached = cached_record_at_path(&path, canonical_id).expect("cached");
        assert_eq!(cached.status, incoming.status);
        assert_eq!(cached.prewarm_status.as_deref(), Some("warmed"));
        assert_eq!(cached.prewarmed_at_ms, Some(1234));
        assert_eq!(cached.prewarm_error.as_deref(), Some("bootstrap timeout"));
    }

    #[test]
    fn store_records_at_path_uses_incoming_prewarm_when_present() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let canonical_id = "plugin-a@local/environment/prefetch";
        let original = test_record_with_prewarm(
            "plugin-a@local",
            "prefetch",
            "available",
            Some("warmed"),
            Some(1234),
            Some("bootstrap timeout"),
        );
        store_records_at_path(&path, &[original]).expect("store seed");

        let incoming = test_record_with_prewarm(
            "plugin-a@local",
            "prefetch",
            "missing",
            Some("failed"),
            Some(5678),
            Some("write cache failed"),
        );
        store_records_at_path(&path, &[incoming.clone()]).expect("store own prewarm update");

        let cached = cached_record_at_path(&path, canonical_id).expect("cached");
        assert_eq!(cached.status, incoming.status);
        assert_eq!(cached.prewarm_status.as_deref(), Some("failed"));
        assert_eq!(cached.prewarmed_at_ms, Some(5678));
        assert_eq!(cached.prewarm_error.as_deref(), Some("write cache failed"));
    }

    #[test]
    fn replace_records_all_scope_with_empty_records_clears_cache() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        store_records_at_path(
            &path,
            &[
                test_record("plugin-a@local", "old", "missing"),
                test_record("plugin-b@local", "old", "available"),
            ],
        )
        .expect("seed");

        replace_records_at_path(&path, RefreshScope::All, &[]).expect("replace all empty");
        let cache = load_cache_at_path(&path);
        assert!(cache.records.is_empty());
    }

    #[test]
    fn replace_records_concurrent_plugin_scopes_keep_both_plugin_records() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("availability.json");
        let path_for_a = path.clone();
        let path_for_b = path.clone();

        let handle_a = thread::spawn(move || {
            replace_records_at_path(
                &path_for_a,
                RefreshScope::Plugin("plugin-a@local".to_string()),
                &[test_record("plugin-a@local", "a", "available")],
            )
        });
        let handle_b = thread::spawn(move || {
            replace_records_at_path(
                &path_for_b,
                RefreshScope::Plugin("plugin-b@local".to_string()),
                &[test_record("plugin-b@local", "b", "available")],
            )
        });

        assert!(handle_a.join().expect("thread a").is_ok());
        assert!(handle_b.join().expect("thread b").is_ok());

        let cache = load_cache_at_path(&path);
        assert!(cache.records.contains_key("plugin-a@local/environment/a"));
        assert!(cache.records.contains_key("plugin-b@local/environment/b"));
    }
}
