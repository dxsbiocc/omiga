use crate::domain::env_hygiene;
use crate::domain::environment_availability;
use crate::domain::environment_availability::EnvironmentAvailabilityRecord;
use crate::domain::environments::EnvironmentProfileSummary;
use crate::domain::operators::{
    self, operator_conda_environment_selection, operator_container_selection_for_profile,
    OperatorContainerImagePrepare, OperatorContainerKind, OperatorExecutionSurfaceKind,
};
use crate::domain::plugins;
use crate::domain::tools::ToolContext;
use async_trait::async_trait;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use tokio::process::Command as TokioCommand;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::{timeout, Duration};

pub const PREWARM_TIMEOUT_SECS: u64 = 1_800;
const PREWARM_TIMEOUT: Duration = Duration::from_secs(PREWARM_TIMEOUT_SECS);

static PREWARM_SERIALIZATION_SEMAPHORE: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));
static PREWARM_DEDUP_KEYS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Clone)]
pub struct PrewarmPlan {
    pub tasks: Vec<PrewarmTask>,
    pub diagnostics: Vec<PrewarmDiagnostic>,
}

impl PrewarmPlan {
    fn empty() -> Self {
        Self {
            tasks: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrewarmTask {
    pub canonical_id: String,
    pub runtime_kind: PrewarmRuntimeKind,
    pub shell_script: String,
    pub dedupe_key: String,
}

#[derive(Debug, Clone)]
pub struct PrewarmDiagnostic {
    pub canonical_id: String,
    pub runtime_kind: PrewarmRuntimeKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrewarmRuntimeKind {
    Conda,
    Docker,
    Singularity,
    Unsupported,
}

impl std::fmt::Display for PrewarmRuntimeKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Conda => "conda",
            Self::Docker => "docker",
            Self::Singularity => "singularity",
            Self::Unsupported => "unsupported",
        })
    }
}

#[derive(Debug)]
pub struct PrewarmOutcome {
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait PrewarmRunner: Send + Sync {
    async fn run(&self, script: &str) -> Result<PrewarmOutcome, String>;
}

#[derive(Clone, Copy, Default)]
pub struct LocalShellRunner;

#[async_trait]
impl PrewarmRunner for LocalShellRunner {
    async fn run(&self, script: &str) -> Result<PrewarmOutcome, String> {
        let scratch = {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            std::env::temp_dir().join(format!("omiga-prewarm-{}-{stamp}", std::process::id()))
        };
        std::fs::create_dir_all(&scratch).map_err(|err| err.to_string())?;

        let keep_exemptions =
            env_hygiene::keep_exemptions_from(std::env::var("OMIGA_ENV_KEEP").ok().as_deref());
        let mut command = TokioCommand::new("/bin/sh");
        command
            .arg("-lc")
            .arg(script)
            .current_dir(&scratch)
            // Ensure timeout-aborted prewarm jobs do not keep child processes alive.
            .kill_on_drop(true);
        for name in env_hygiene::filter_env_vars(std::env::vars(), &keep_exemptions).1 {
            command.env_remove(name);
        }

        let output = {
            let output = timeout(PREWARM_TIMEOUT, command.output()).await;
            let _ = std::fs::remove_dir_all(&scratch);
            output
        }
        .map_err(|_| format!("environment prewarm timed out after {PREWARM_TIMEOUT_SECS} seconds"))?
        .map_err(|err| err.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            return Ok(PrewarmOutcome { stdout, stderr });
        }

        let code = output
            .status
            .code()
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        Err(format!("prewarm failed with exit code {code}: {stderr}"))
    }
}

pub fn prewarm_disabled() -> bool {
    matches!(
        std::env::var("OMIGA_DISABLE_ENV_PREWARM").ok().as_deref(),
        Some(value) if value.trim() == "1"
    )
}

pub fn prewarm_plan_from_profiles(
    profiles: &[EnvironmentProfileSummary],
    project_root: Option<&Path>,
    disabled: bool,
) -> PrewarmPlan {
    if disabled {
        return PrewarmPlan::empty();
    }

    profiles.iter().fold(PrewarmPlan::empty(), |mut plan, profile| {
        let runtime_kind = profile
            .runtime
            .kind
            .as_deref()
            .unwrap_or("system")
            .trim()
            .to_ascii_lowercase();
        match runtime_kind.as_str() {
            "conda" | "mamba" | "micromamba" => {
                let Some(project_root) = project_root else {
                    plan.diagnostics.push(PrewarmDiagnostic {
                        canonical_id: profile.canonical_id.clone(),
                        runtime_kind: PrewarmRuntimeKind::Conda,
                        message: "conda prewarm skipped: project root was not provided".to_string(),
                    });
                    return plan;
                };

                match plugin_conda_plan_entry(profile, project_root) {
                    Ok(task) => plan.tasks.push(task),
                    Err(message) => {
                        plan.diagnostics.push(PrewarmDiagnostic {
                            canonical_id: profile.canonical_id.clone(),
                            runtime_kind: PrewarmRuntimeKind::Conda,
                            message,
                        });
                    }
                }
            }
            "docker" => match container_plan_entry(profile, OperatorContainerKind::Docker, project_root) {
                Ok(Some(task)) => plan.tasks.push(task),
                Ok(None) => plan.diagnostics.push(PrewarmDiagnostic {
                    canonical_id: profile.canonical_id.clone(),
                    runtime_kind: PrewarmRuntimeKind::Docker,
                    message: "docker runtime uses existing image and does not require local prebuild".to_string(),
                }),
                Err(message) => plan.diagnostics.push(PrewarmDiagnostic {
                    canonical_id: profile.canonical_id.clone(),
                    runtime_kind: PrewarmRuntimeKind::Docker,
                    message,
                }),
            },
            "singularity" => match container_plan_entry(profile, OperatorContainerKind::Singularity, project_root) {
                Ok(Some(task)) => plan.tasks.push(task),
                Ok(None) => plan.diagnostics.push(PrewarmDiagnostic {
                    canonical_id: profile.canonical_id.clone(),
                    runtime_kind: PrewarmRuntimeKind::Singularity,
                    message: "singularity runtime uses existing image and does not require local prebuild".to_string(),
                }),
                Err(message) => plan.diagnostics.push(PrewarmDiagnostic {
                    canonical_id: profile.canonical_id.clone(),
                    runtime_kind: PrewarmRuntimeKind::Singularity,
                    message,
                }),
            },
            _ => {
                plan.diagnostics.push(PrewarmDiagnostic {
                    canonical_id: profile.canonical_id.clone(),
                    runtime_kind: PrewarmRuntimeKind::Unsupported,
                    message: format!("runtime `{runtime_kind}` is not prewarmable"),
                });
            }
        }
        plan
    })
}

pub fn build_prewarm_plan_for_plugin(
    plugin_id: Option<&str>,
    project_root: Option<&Path>,
) -> PrewarmPlan {
    let profiles = environment_availability::environment_profiles_for_refresh(plugin_id)
        .into_iter()
        .map(crate::domain::environments::environment_summary)
        .collect::<Vec<_>>();
    prewarm_plan_from_profiles(&profiles, project_root, prewarm_disabled())
}

fn plugin_conda_plan_entry(
    profile: &EnvironmentProfileSummary,
    project_root: &Path,
) -> Result<PrewarmTask, String> {
    let ctx = ToolContext::new(project_root.to_owned());
    let selection =
        operator_conda_environment_selection(&ctx, profile, OperatorExecutionSurfaceKind::Local)?;
    let conda_file = plugins::plugin_conda_environment_file(profile)?;
    let script = plugins::conda_environment_check_shell_script(
        Path::new(&selection.env_prefix),
        &conda_file,
        &selection.env_hash,
        &selection.env_vars,
        "true",
    );
    Ok(PrewarmTask {
        canonical_id: profile.canonical_id.clone(),
        runtime_kind: PrewarmRuntimeKind::Conda,
        shell_script: prewarm_script_with_prelude(&script),
        dedupe_key: selection.env_prefix,
    })
}

fn container_plan_entry(
    profile: &EnvironmentProfileSummary,
    kind: OperatorContainerKind,
    project_root: Option<&Path>,
) -> Result<Option<PrewarmTask>, String> {
    let root = project_root
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let ctx = ToolContext::new(root);
    let selection = operator_container_selection_for_profile(
        &ctx,
        profile,
        OperatorExecutionSurfaceKind::Local,
    )?;
    let Some(selection) = selection else {
        return Ok(None);
    };
    if selection.kind != kind {
        return Ok(None);
    }
    let Some(prepare) = selection.prepare else {
        return Ok(None);
    };

    let dedupe_key = match &prepare {
        OperatorContainerImagePrepare::Dockerfile { tag, .. } => tag.clone(),
        OperatorContainerImagePrepare::SingularityDefinition { sif, .. } => sif.clone(),
    };
    let script = operators::container_runtime_prepare_script(&prepare);

    Ok(Some(PrewarmTask {
        canonical_id: profile.canonical_id.clone(),
        runtime_kind: match kind {
            OperatorContainerKind::Docker => PrewarmRuntimeKind::Docker,
            OperatorContainerKind::Singularity => PrewarmRuntimeKind::Singularity,
        },
        shell_script: prewarm_script_with_prelude(&script),
        dedupe_key,
    }))
}

fn prewarm_script_with_prelude(script: &str) -> String {
    let trimmed = script.trim_start();
    let mut output = String::new();
    if !trimmed.starts_with("set -e") {
        output.push_str("set -e\n");
    }
    output.push_str("mkdir -p logs\n");
    output.push_str(script);
    output
}

pub async fn run_prewarm_tasks<R: PrewarmRunner>(
    cache_path: &Path,
    tasks: &[PrewarmTask],
    runner: &R,
) -> Result<(), String> {
    let _permit = acquire_serialization().await?;
    for task in tasks {
        if is_deduped(&task.dedupe_key) {
            let error = Some(
                "prewarm skipped because same dedupe key was already processed in this process"
                    .to_string(),
            );
            let _ = update_prewarm_record(cache_path, task, "skipped", None, error);
            continue;
        }

        let outcome = runner.run(&task.shell_script).await;
        match outcome {
            Ok(_) => {
                let _ = update_prewarm_record(cache_path, task, "warmed", Some(now_ms()), None);
                remember_dedupe_key(&task.dedupe_key);
            }
            Err(error) => {
                let _ =
                    update_prewarm_record(cache_path, task, "failed", Some(now_ms()), Some(error));
            }
        }
    }

    Ok(())
}

fn is_deduped(dedupe_key: &str) -> bool {
    let dedupe_keys = PREWARM_DEDUP_KEYS
        .lock()
        .expect("prewarm dedupe lock poisoned");
    dedupe_keys.contains(dedupe_key)
}

fn remember_dedupe_key(dedupe_key: &str) {
    let mut dedupe_keys = PREWARM_DEDUP_KEYS
        .lock()
        .expect("prewarm dedupe lock poisoned");
    let _ = dedupe_keys.insert(dedupe_key.to_string());
}

fn acquire_serialization() -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<SemaphorePermit<'static>, String>> + Send>,
> {
    Box::pin(async move {
        PREWARM_SERIALIZATION_SEMAPHORE
            .acquire()
            .await
            .map_err(|err| format!("failed to acquire prewarm serialization lock: {err}"))
    })
}

fn update_prewarm_record(
    cache_path: &Path,
    task: &PrewarmTask,
    status: &str,
    prewarmed_at_ms: Option<u64>,
    error: Option<String>,
) -> Result<(), String> {
    environment_availability::mutate_record_at_path(cache_path, &task.canonical_id, |record| {
        let mut record = record.unwrap_or_else(|| placeholder_record(task));
        record.prewarm_status = Some(status.to_string());
        record.prewarmed_at_ms = prewarmed_at_ms;
        record.prewarm_error = error;
        record
    })
}

fn placeholder_record(task: &PrewarmTask) -> EnvironmentAvailabilityRecord {
    EnvironmentAvailabilityRecord {
        canonical_id: task.canonical_id.clone(),
        runtime_type: task.runtime_kind.to_string(),
        status: "notRun".to_string(),
        manager: None,
        executable_path: None,
        error: None,
        message: "prewarm cache placeholder from environment prewarm task".to_string(),
        install_hint: None,
        checked_at_ms: now_ms(),
        scope: "local".to_string(),
        prewarm_status: None,
        prewarmed_at_ms: None,
        prewarm_error: None,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, HashSet};
    use std::ffi::{OsStr, OsString};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    static PREWARM_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    fn hash_hex(value: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(value);
        format!("{:x}", hasher.finalize())
    }

    fn safe_component(raw: &str) -> String {
        raw.chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    }

    fn build_profile(
        root: &Path,
        profile_id: &str,
        source_plugin: &str,
        runtime_kind: &str,
        extra: &[(&str, &str)],
    ) -> EnvironmentProfileSummary {
        let manifest_dir = root.join("environments").join(profile_id);
        std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
        let manifest_path = manifest_dir.join("environment.yaml");
        std::fs::write(&manifest_path, "name: test\n").expect("manifest file");

        let mut runtime_extra = serde_json::Map::new();
        for (key, value) in extra {
            runtime_extra.insert((*key).to_string(), json!(value));
        }

        EnvironmentProfileSummary {
            id: profile_id.to_string(),
            version: "0.1.0".to_string(),
            canonical_id: format!("{source_plugin}/environment/{profile_id}"),
            source_plugin: source_plugin.to_string(),
            manifest_path: manifest_path.to_string_lossy().into_owned(),
            name: None,
            description: None,
            tags: Vec::new(),
            runtime: crate::domain::environments::EnvironmentRuntimeProfile {
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
    fn prewarm_plan_builds_expected_tasks_and_hash_dedupe_keys() {
        let root = tempdir().expect("temp dir");
        let conda_profile_root = root.path().join("plugin-a");
        std::fs::create_dir_all(&conda_profile_root).expect("conda root");
        let conda_profile =
            build_profile(&conda_profile_root, "conda", "plugin-a@local", "conda", &[]);
        let conda_yaml = conda_profile_root.join("environments/conda/conda.yaml");
        std::fs::write(&conda_yaml, "name: test\ndependencies: []\n").expect("conda yaml");
        let expected_conda_dedupe = operators::operator_conda_environment_selection(
            &ToolContext::new(root.path().to_path_buf()),
            &conda_profile,
            operators::OperatorExecutionSurfaceKind::Local,
        )
        .expect("conda selection")
        .env_prefix;

        let docker_profile = build_profile(
            &conda_profile_root,
            "docker",
            "plugin-a@local",
            "docker",
            &[("dockerfile", "Dockerfile")],
        );
        std::fs::write(
            conda_profile_root.join("environments/docker/Dockerfile"),
            "FROM scratch\n",
        )
        .expect("dockerfile");

        let singularity_profile = build_profile(
            &conda_profile_root,
            "singularity",
            "plugin-a@local",
            "singularity",
            &[("definition_file", "singularity.def")],
        );
        std::fs::write(
            conda_profile_root.join("environments/singularity/singularity.def"),
            "Bootstrap: docker\nFrom: ubuntu\n",
        )
        .expect("def file");

        let system_profile = build_profile(
            &conda_profile_root,
            "system",
            "plugin-a@local",
            "system",
            &[],
        );

        let plan = prewarm_plan_from_profiles(
            &vec![
                conda_profile,
                docker_profile,
                singularity_profile,
                system_profile,
            ],
            Some(root.path()),
            false,
        );

        assert_eq!(plan.diagnostics.len(), 1);
        let tasks = plan.tasks;
        assert_eq!(tasks.len(), 3);

        let conda_task = tasks
            .iter()
            .find(|task| task.runtime_kind == PrewarmRuntimeKind::Conda)
            .expect("conda task");
        assert!(conda_task
            .shell_script
            .starts_with("set -e\nmkdir -p logs\n"));
        assert!(conda_task
            .shell_script
            .contains("omiga_bootstrap_micromamba"));
        assert!(conda_task.shell_script.contains(".omiga-env-hash"));
        assert_eq!(conda_task.dedupe_key, expected_conda_dedupe);

        let docker_task = tasks
            .iter()
            .find(|task| task.runtime_kind == PrewarmRuntimeKind::Docker)
            .expect("docker task");
        assert!(docker_task
            .shell_script
            .starts_with("set -e\nmkdir -p logs\n"));
        let docker_hash = hash_hex("FROM scratch\n".as_bytes());
        assert_eq!(
            docker_task.dedupe_key,
            format!(
                "omiga-env-{}:{}",
                safe_component(&docker_task.canonical_id),
                &docker_hash[..12]
            )
        );

        let singularity_task = tasks
            .iter()
            .find(|task| task.runtime_kind == PrewarmRuntimeKind::Singularity)
            .expect("singularity task");
        assert!(singularity_task
            .shell_script
            .starts_with("set -e\nmkdir -p logs\n"));
        let singularity_hash = hash_hex("Bootstrap: docker\nFrom: ubuntu\n".as_bytes());
        let expected_singularity_sif =
            root.path()
                .join(".omiga/operator-envs/singularity")
                .join(format!(
                    "{}-{}.sif",
                    safe_component(&singularity_task.canonical_id),
                    &singularity_hash[..12]
                ));
        assert_eq!(
            singularity_task.dedupe_key,
            expected_singularity_sif.to_string_lossy().to_string()
        );
        assert_eq!(
            plan.diagnostics[0].runtime_kind,
            PrewarmRuntimeKind::Unsupported
        );
    }

    #[test]
    fn prewarm_plan_skips_conda_without_project_root_and_report_diagnostic() {
        let root = tempdir().expect("temp dir");
        let profile = build_profile(root.path(), "conda", "plugin-a@local", "conda", &[]);
        let conda_yaml = root.path().join("environments/conda/conda.yaml");
        std::fs::write(&conda_yaml, "name: test\n").expect("conda yaml");

        let plan = prewarm_plan_from_profiles(&[profile], None, false);
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.diagnostics.len(), 1);
        assert_eq!(plan.diagnostics[0].runtime_kind, PrewarmRuntimeKind::Conda);
    }

    #[test]
    fn prewarm_plan_is_empty_when_disabled_without_profile_access() {
        let root = tempdir().expect("temp dir");
        let profile = build_profile(root.path(), "conda", "plugin-a@local", "conda", &[]);
        let conda_yaml = root.path().join("environments/conda/conda.yaml");
        std::fs::write(&conda_yaml, "name: test\n").expect("conda yaml");

        let plan = prewarm_plan_from_profiles(&[profile], Some(root.path()), true);
        assert!(plan.tasks.is_empty());
        assert!(plan.diagnostics.is_empty());
    }

    #[derive(Default)]
    struct TestRunner {
        calls: Arc<Mutex<Vec<String>>>,
        active: Arc<Mutex<usize>>,
        parallel: Arc<Mutex<bool>>,
        fail_if_contains: HashSet<&'static str>,
        delay_ms: u64,
    }

    #[async_trait]
    impl PrewarmRunner for TestRunner {
        async fn run(&self, script: &str) -> Result<PrewarmOutcome, String> {
            assert!(script.starts_with("set -e\nmkdir -p logs\n"));

            {
                let mut calls = self.calls.lock().expect("calls");
                calls.push(script.to_string());
            }

            {
                let mut active = self.active.lock().expect("active");
                *active += 1;
                if *active > 1 {
                    let mut parallel = self.parallel.lock().expect("parallel");
                    *parallel = true;
                }
            }

            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }

            {
                let mut active = self.active.lock().expect("active");
                if *active > 0 {
                    *active -= 1;
                }
            }

            if self
                .fail_if_contains
                .iter()
                .any(|needle| script.contains(needle))
            {
                Err("mock failure".to_string())
            } else {
                Ok(PrewarmOutcome {
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        }
    }

    #[tokio::test]
    async fn run_prewarm_tasks_executes_serially_and_updates_cache() {
        let tmp = tempdir().expect("temp dir");
        let cache = tmp.path().join("availability.json");
        let tasks = vec![
            PrewarmTask {
                canonical_id: "plugin-a@local/environment/a".to_string(),
                runtime_kind: PrewarmRuntimeKind::Docker,
                shell_script: prewarm_script_with_prelude("ok-a"),
                dedupe_key: "dedupe-1".to_string(),
            },
            PrewarmTask {
                canonical_id: "plugin-a@local/environment/b".to_string(),
                runtime_kind: PrewarmRuntimeKind::Docker,
                shell_script: prewarm_script_with_prelude("fail-b"),
                dedupe_key: "dedupe-2".to_string(),
            },
            PrewarmTask {
                canonical_id: "plugin-a@local/environment/c".to_string(),
                runtime_kind: PrewarmRuntimeKind::Docker,
                shell_script: prewarm_script_with_prelude("ok-c"),
                dedupe_key: "dedupe-1".to_string(),
            },
        ];

        let runner = TestRunner {
            calls: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(0)),
            parallel: Arc::new(Mutex::new(false)),
            fail_if_contains: HashSet::from(["fail"]),
            delay_ms: 20,
        };

        run_prewarm_tasks(&cache, &tasks, &runner)
            .await
            .expect("prewarm run");

        let calls = runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        assert!(calls[0].ends_with("ok-a"));
        assert!(calls[1].ends_with("fail-b"));
        assert!(!*runner.parallel.lock().expect("parallel"));

        let cache = environment_availability::load_cache_at_path(&cache);
        let a = cache
            .records
            .get("plugin-a@local/environment/a")
            .expect("record a");
        assert_eq!(a.prewarm_status.as_deref(), Some("warmed"));
        assert!(a.prewarmed_at_ms.is_some());
        assert!(a.prewarm_error.is_none());

        let b = cache
            .records
            .get("plugin-a@local/environment/b")
            .expect("record b");
        assert_eq!(b.prewarm_status.as_deref(), Some("failed"));
        assert_eq!(b.prewarm_error.as_deref(), Some("mock failure"));

        let c = cache
            .records
            .get("plugin-a@local/environment/c")
            .expect("record c");
        assert_eq!(c.prewarm_status.as_deref(), Some("skipped"));
        assert_eq!(
            c.prewarm_error.as_deref().expect("missing skip reason"),
            "prewarm skipped because same dedupe key was already processed in this process"
        );
    }

    #[test]
    fn run_prewarm_update_uses_latest_cached_snapshot() {
        let tmp = tempdir().expect("temp dir");
        let cache = tmp.path().join("availability.json");
        let task = PrewarmTask {
            canonical_id: "plugin-a@local/environment/a".to_string(),
            runtime_kind: PrewarmRuntimeKind::Docker,
            shell_script: prewarm_script_with_prelude("ok-a"),
            dedupe_key: "dedupe-1".to_string(),
        };
        let placeholder = EnvironmentAvailabilityRecord {
            canonical_id: "plugin-a@local/environment/a".to_string(),
            runtime_type: "system".to_string(),
            status: "missing".to_string(),
            manager: None,
            executable_path: None,
            error: None,
            message: "mock missing record".to_string(),
            install_hint: None,
            checked_at_ms: 0,
            scope: "local".to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        };

        let _ = environment_availability::store_record_at_path(&cache, &placeholder);
        let discovered = EnvironmentAvailabilityRecord {
            runtime_type: "docker".to_string(),
            status: "available".to_string(),
            checked_at_ms: 2,
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
            ..placeholder
        };
        environment_availability::replace_records_at_path(
            &cache,
            environment_availability::RefreshScope::All,
            std::slice::from_ref(&discovered),
        )
        .expect("replace probe");

        update_prewarm_record(
            &cache,
            &task,
            "warmed",
            Some(1234),
            Some("mock warm success".to_string()),
        )
        .expect("update prewarm");

        let cache = environment_availability::load_cache_at_path(&cache);
        let record = cache
            .records
            .get("plugin-a@local/environment/a")
            .expect("cached prewarm record");
        assert_eq!(record.status, "available");
        assert_eq!(record.checked_at_ms, 2);
        assert_eq!(record.prewarm_status.as_deref(), Some("warmed"));
        assert_eq!(record.prewarmed_at_ms, Some(1234));
        assert_eq!(record.prewarm_error.as_deref(), Some("mock warm success"));
    }

    #[tokio::test]
    async fn run_prewarm_tasks_retries_same_dedupe_key_after_failure() {
        let tmp = tempdir().expect("temp dir");
        let cache = tmp.path().join("availability.json");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(Mutex::new(0usize));

        #[derive(Clone)]
        struct RetryRunner {
            calls: Arc<Mutex<Vec<String>>>,
            attempts: Arc<Mutex<usize>>,
        }

        #[async_trait]
        impl PrewarmRunner for RetryRunner {
            async fn run(&self, script: &str) -> Result<PrewarmOutcome, String> {
                let mut attempts = self.attempts.lock().expect("attempts");
                *attempts += 1;
                self.calls.lock().expect("calls").push(script.to_string());

                if *attempts == 1 {
                    return Err("mock failure".to_string());
                }

                Ok(PrewarmOutcome {
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        }

        let runner = RetryRunner { calls, attempts };
        let tasks = vec![
            PrewarmTask {
                canonical_id: "plugin-a@local/environment/a".to_string(),
                runtime_kind: PrewarmRuntimeKind::Docker,
                shell_script: prewarm_script_with_prelude("ok-a"),
                dedupe_key: "dedupe-shared".to_string(),
            },
            PrewarmTask {
                canonical_id: "plugin-a@local/environment/a".to_string(),
                runtime_kind: PrewarmRuntimeKind::Docker,
                shell_script: prewarm_script_with_prelude("ok-b"),
                dedupe_key: "dedupe-shared".to_string(),
            },
        ];

        run_prewarm_tasks(&cache, &tasks, &runner)
            .await
            .expect("prewarm run");

        let calls = runner.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        let cache = environment_availability::load_cache_at_path(&cache);
        let record = cache
            .records
            .get("plugin-a@local/environment/a")
            .expect("record");
        assert_eq!(record.prewarm_status.as_deref(), Some("warmed"));
        assert!(record.prewarm_error.is_none());
    }

    #[tokio::test]
    async fn prewarm_plan_builds_distinct_conda_dedupe_keys_for_distinct_canonical_ids() {
        let root = tempdir().expect("temp dir");
        let cache_path = root.path().join("availability.json");
        let profile_a = build_profile(
            &root.path().join("plugin-a"),
            "conda-a",
            "plugin-a@local",
            "conda",
            &[],
        );
        let profile_b = build_profile(
            &root.path().join("plugin-b"),
            "conda-b",
            "plugin-b@local",
            "conda",
            &[],
        );
        let conda_yaml = "name: test\ndependencies: []\n";
        let conda_dir_a = Path::new(&profile_a.manifest_path)
            .parent()
            .expect("manifest parent")
            .join("conda.yaml");
        let conda_dir_b = Path::new(&profile_b.manifest_path)
            .parent()
            .expect("manifest parent")
            .join("conda.yaml");
        std::fs::write(&conda_dir_a, conda_yaml).expect("conda yaml a");
        std::fs::write(&conda_dir_b, conda_yaml).expect("conda yaml b");

        let plan =
            prewarm_plan_from_profiles(&vec![profile_a, profile_b], Some(root.path()), false);
        let conda_tasks: Vec<_> = plan
            .tasks
            .into_iter()
            .filter(|task| task.runtime_kind == PrewarmRuntimeKind::Conda)
            .collect();
        assert_eq!(conda_tasks.len(), 2);
        assert_ne!(conda_tasks[0].dedupe_key, conda_tasks[1].dedupe_key);

        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner = TestRunner {
            calls: Arc::clone(&calls),
            active: Arc::new(Mutex::new(0)),
            parallel: Arc::new(Mutex::new(false)),
            fail_if_contains: HashSet::new(),
            delay_ms: 0,
        };
        run_prewarm_tasks(&cache_path, &conda_tasks, &runner)
            .await
            .expect("prewarm run");

        let calls = calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn container_runtime_only_image_profiles_report_existing_image_without_error() {
        let root = tempdir().expect("temp dir");
        let mut profile = build_profile(
            root.path(),
            "docker-image-only",
            "plugin-a@local",
            "docker",
            &[],
        );
        profile.runtime.image = Some("busybox:latest".to_string());

        let plan = prewarm_plan_from_profiles(&[profile], Some(root.path()), false);

        assert!(plan.tasks.is_empty());
        assert_eq!(plan.diagnostics.len(), 1);
        assert_eq!(plan.diagnostics[0].runtime_kind, PrewarmRuntimeKind::Docker);
        assert_eq!(
            plan.diagnostics[0].message,
            "docker runtime uses existing image and does not require local prebuild"
        );
        assert!(!plan.diagnostics[0].message.to_lowercase().contains("error"));
    }

    #[tokio::test]
    async fn local_shell_runner_uses_scratch_dir_and_cleans_up() {
        let runner = LocalShellRunner;
        let baseline = std::env::current_dir().expect("current dir");
        let baseline_marker = baseline.join("marker.txt");
        let _ = std::fs::remove_file(&baseline_marker);
        let outcome = runner
            .run("pwd; touch marker.txt; echo ok")
            .await
            .expect("run succeeded");
        let scratch = PathBuf::from(outcome.stdout.lines().next().expect("pwd output"));
        assert_ne!(scratch, baseline);
        assert!(!baseline_marker.exists());
        assert_eq!(outcome.stdout.lines().last().unwrap_or_default(), "ok");
        assert!(!scratch.join("marker.txt").exists());
    }

    #[tokio::test]
    async fn local_shell_runner_removes_sensitive_env_from_subprocess() {
        let _guard = PREWARM_ENV_LOCK.lock().expect("environment lock");
        let _secret = ScopedEnv::set("FAKE_SECRET_TOKEN", "super-secret");
        let _keep = ScopedEnv::remove("OMIGA_ENV_KEEP");

        let runner = LocalShellRunner;
        let outcome = runner
            .run("printf 'token=%s\\n' \"${FAKE_SECRET_TOKEN:-absent}\"")
            .await
            .expect("run succeeded");

        assert_eq!(
            outcome.stdout.lines().next().unwrap_or_default(),
            "token=absent"
        );
    }
}
