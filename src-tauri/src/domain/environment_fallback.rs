use crate::domain::environment_availability::EnvironmentAvailabilityRecord;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProvisioningFailureKind {
    CondaManagerMissing,
    MicromambaBootstrapFailed,
    CondaEnvCreateFailed,
    DockerRuntimeMissing,
    SingularityRuntimeMissing,
    ContainerImageBuildFailed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FallbackSuggestion {
    pub title: String,
    pub detail: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProvisioningFailure {
    pub kind: ProvisioningFailureKind,
    pub suggestions: Vec<FallbackSuggestion>,
}

pub fn classify_provisioning_failure(
    exit_code: Option<i64>,
    stderr_tail: &str,
) -> Option<ProvisioningFailureKind> {
    let stderr = stderr_tail.to_ascii_lowercase();
    if stderr.trim().is_empty() {
        return None;
    }

    if contains_any(
        &stderr,
        &[
            "no micromamba, mamba, or conda executable",
            "omiga_conda_manager_missing",
            "conda manager missing",
        ],
    ) {
        return Some(ProvisioningFailureKind::CondaManagerMissing);
    }

    if contains_any(
        &stderr,
        &[
            "automatic micromamba installation failed",
            "micromamba bootstrap download failed",
            "micromamba bootstrap checksum mismatch",
            "unsupported platform for micromamba bootstrap",
            "micromamba bootstrap is disabled",
            "micromamba bootstrap installation failed",
            "micromamba bootstrap self-check failed",
            "micromamba bootstrap binary is not executable",
            "micromamba bootstrap checksum unavailable",
            "no supported downloader for micromamba bootstrap",
        ],
    ) {
        return Some(ProvisioningFailureKind::MicromambaBootstrapFailed);
    }

    if contains_any(
        &stderr,
        &[
            "docker runtime is required",
            "docker runtime is required for this operator environment",
            "docker cli was found, but `docker version` failed",
        ],
    ) {
        return Some(ProvisioningFailureKind::DockerRuntimeMissing);
    }

    if contains_any(
        &stderr,
        &[
            "singularity/apptainer runtime is required",
            "singularity/apptainer is required",
            "singularity/apptainer is required for this operator environment",
            "singularity/apptainer runtime is required for this operator environment",
        ],
    ) {
        return Some(ProvisioningFailureKind::SingularityRuntimeMissing);
    }

    let exit_nonzero = exit_code.is_none_or(|code| code != 0);
    if !exit_nonzero {
        return None;
    }

    if contains_any(
        &stderr,
        &["docker build", "docker image build", "dockerfile"],
    ) || contains_any(&stderr, &["singularity build", "singularity definition"])
    {
        return Some(ProvisioningFailureKind::ContainerImageBuildFailed);
    }

    if contains_any(
        &stderr,
        &[
            "conda create",
            "mamba env create",
            "micromamba create",
            "env create -y",
        ],
    ) {
        return Some(ProvisioningFailureKind::CondaEnvCreateFailed);
    }

    // Quoted manager paths ("$OMIGA_CONDA_BIN" create -y ...) break the
    // adjacent-word markers above; accept `create -y` only alongside a
    // conda-ecosystem token so unrelated commands are not misclassified.
    if stderr.contains("create -y") && contains_any(&stderr, &["micromamba", "mamba", "conda"]) {
        return Some(ProvisioningFailureKind::CondaEnvCreateFailed);
    }

    (exit_code == Some(127)).then_some(ProvisioningFailureKind::Unknown)
}

pub fn fallback_suggestions(
    kind: &ProvisioningFailureKind,
    availability: &[EnvironmentAvailabilityRecord],
) -> Vec<FallbackSuggestion> {
    let docker_available = runtime_available(availability, "docker");
    let singularity_available = runtime_available(availability, "singularity");

    let mut suggestions = match kind {
        ProvisioningFailureKind::CondaManagerMissing => {
            let mut suggestions = vec![
                suggestion(
                    "安装 Micromamba",
                    "安装 micromamba 并确保其在 PATH 中可执行。",
                    "install_micromamba",
                ),
                suggestion(
                    "指定 micromamba 路径",
                    "设置 OMIGA_MICROMAMBA 为可执行文件绝对路径后重试。",
                    "set_omiga_micromamba",
                ),
                suggestion(
                    "关闭 bootstrap",
                    "如环境已固定，可设置 OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1。",
                    "disable_bootstrap_escape",
                ),
            ];
            if docker_available {
                suggestions.push(suggestion(
                    "切换到 docker 运行时变体",
                    "当前检测到 docker 可用，可先尝试 docker 变体继续运行。",
                    "use_docker_environment",
                ));
            }
            if singularity_available {
                suggestions.push(suggestion(
                    "切换到 singularity 运行时变体",
                    "当前检测到 singularity 可用，可先尝试 singularity 变体继续运行。",
                    "use_singularity_environment",
                ));
            }
            suggestions
        }

        ProvisioningFailureKind::MicromambaBootstrapFailed => {
            let mut suggestions = vec![
                suggestion(
                    "安装 Micromamba",
                    "提前安装 micromamba，避免自动 bootstrap 路径导致的不确定失败。",
                    "install_micromamba",
                ),
                suggestion(
                    "指定 micromamba 路径",
                    "设置 OMIGA_MICROMAMBA 指向现有可执行文件。",
                    "set_omiga_micromamba",
                ),
                suggestion(
                    "关闭 bootstrap",
                    "设置 OMIGA_DISABLE_MICROMAMBA_BOOTSTRAP=1 跳过下载流程。",
                    "disable_bootstrap_escape",
                ),
                suggestion(
                    "重试网络",
                    "检查网络连通性后重新执行 bootstrap。",
                    "check_network_retry",
                ),
            ];
            if docker_available {
                suggestions.push(suggestion(
                    "切换到 docker 运行时变体",
                    "当前检测到 docker 可用，可先尝试 docker 变体继续执行。",
                    "use_docker_environment",
                ));
            }
            if singularity_available {
                suggestions.push(suggestion(
                    "切换到 singularity 运行时变体",
                    "当前检测到 singularity 可用，可先尝试 singularity 变体继续执行。",
                    "use_singularity_environment",
                ));
            }
            suggestions
        }

        ProvisioningFailureKind::CondaEnvCreateFailed => vec![
            suggestion(
                "检查日志",
                "先查看 stderr 尾部确认依赖下载或网络请求细节。",
                "check_network_retry",
            ),
            suggestion(
                "重试预热",
                "执行环境预热/刷新后再重试，避免缓存状态不一致。",
                "retry_prewarm",
            ),
            suggestion(
                "指定 micromamba",
                "如当前环境有可用管理器，设置 OMIGA_MICROMAMBA 指向其路径。",
                "set_omiga_micromamba",
            ),
        ],

        ProvisioningFailureKind::ContainerImageBuildFailed => vec![
            suggestion(
                "检查日志",
                "查看 build 阶段 stderr，确认镜像拉取、磁盘或权限问题。",
                "check_network_retry",
            ),
            suggestion(
                "重试预热",
                "执行环境预热/刷新后重新发起构建并重试。",
                "retry_prewarm",
            ),
        ],

        ProvisioningFailureKind::DockerRuntimeMissing => {
            let mut suggestions = vec![suggestion(
                "启动 Docker",
                "安装并启动 Docker，或确认当前用户有 docker 访问权限。",
                "start_docker_daemon",
            )];
            if singularity_available {
                suggestions.push(suggestion(
                    "切换到 singularity 变体",
                    "当前检测到 singularity 可用，可先尝试 singularity 变体继续执行。",
                    "use_singularity_environment",
                ));
            }
            suggestions
        }

        ProvisioningFailureKind::SingularityRuntimeMissing => {
            let mut suggestions = vec![suggestion(
                "安装运行时",
                "安装 Singularity/Apptainer 并确认命令可执行。",
                "check_network_retry",
            )];
            if docker_available {
                suggestions.push(suggestion(
                    "切换到 docker 变体",
                    "当前检测到 docker 可用，可先尝试 docker 运行时变体。",
                    "use_docker_environment",
                ));
            }
            suggestions
        }

        ProvisioningFailureKind::Unknown => vec![
            suggestion(
                "重试预热",
                "未匹配到明确标签，请先执行环境预热/刷新后重试。",
                "retry_prewarm",
            ),
            suggestion(
                "重试网络",
                "执行一次网络/镜像服务连通性检查后再次运行。",
                "check_network_retry",
            ),
        ],
    };

    suggestions.truncate(4);
    suggestions
}

fn runtime_available(records: &[EnvironmentAvailabilityRecord], runtime_type: &str) -> bool {
    let runtime_type = runtime_type.to_ascii_lowercase();
    records
        .iter()
        .any(|record| record.runtime_type == runtime_type && record.status == "available")
}

fn suggestion(title: &str, detail: &str, action: &str) -> FallbackSuggestion {
    FallbackSuggestion {
        title: title.to_string(),
        detail: detail.to_string(),
        action: action.to_string(),
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::environment_availability::EnvironmentAvailabilityRecord;

    fn record(runtime_type: &str, status: &str) -> EnvironmentAvailabilityRecord {
        EnvironmentAvailabilityRecord {
            canonical_id: format!("owner/environment/{runtime_type}"),
            runtime_type: runtime_type.to_string(),
            status: status.to_string(),
            manager: None,
            executable_path: None,
            error: None,
            message: format!("{runtime_type} record"),
            install_hint: None,
            checked_at_ms: 0,
            scope: "local".to_string(),
            prewarm_status: None,
            prewarmed_at_ms: None,
            prewarm_error: None,
        }
    }

    #[test]
    fn classifies_known_markers_to_expected_kinds() {
        let cases = [
            (
                "No micromamba, mamba, or conda executable in PATH",
                Some(ProvisioningFailureKind::CondaManagerMissing),
            ),
            (
                "Automatic micromamba installation failed (reason above).",
                Some(ProvisioningFailureKind::MicromambaBootstrapFailed),
            ),
            (
                "micromamba bootstrap download failed with wget",
                Some(ProvisioningFailureKind::MicromambaBootstrapFailed),
            ),
            (
                "micromamba bootstrap checksum mismatch for downloaded binary",
                Some(ProvisioningFailureKind::MicromambaBootstrapFailed),
            ),
            (
                "unsupported platform for micromamba bootstrap: Linux:riscv64",
                Some(ProvisioningFailureKind::MicromambaBootstrapFailed),
            ),
            (
                "Docker runtime is required for this Operator environment but `docker` was not found in the active PATH.",
                Some(ProvisioningFailureKind::DockerRuntimeMissing),
            ),
            (
                "Singularity/Apptainer runtime is required for this Operator environment but neither `singularity` nor `apptainer` was found in the active PATH.",
                Some(ProvisioningFailureKind::SingularityRuntimeMissing),
            ),
        ];

        for (message, expected) in cases {
            assert_eq!(
                classify_provisioning_failure(Some(127), message),
                expected,
                "unexpected kind for {message}"
            );
        }
    }

    #[test]
    fn classifies_business_failures_as_none() {
        assert_eq!(
            classify_provisioning_failure(Some(1), "bad input in operator logic"),
            None
        );
        assert_eq!(
            classify_provisioning_failure(Some(1), "exit 127: command not found in user logic"),
            None
        );
    }

    #[test]
    fn classifies_stage_markers_for_conda_and_image_builds() {
        assert_eq!(
            classify_provisioning_failure(
                Some(1),
                "INFO: running env create -y -p /tmp/env -f env.yaml"
            ),
            Some(ProvisioningFailureKind::CondaEnvCreateFailed)
        );
        assert_eq!(
            classify_provisioning_failure(
                Some(1),
                "\"/home/test/micromamba\" create -y -p /tmp/env -f /tmp/env.yaml"
            ),
            Some(ProvisioningFailureKind::CondaEnvCreateFailed)
        );
        assert_eq!(
            classify_provisioning_failure(Some(1), "docker build -t omiga-env ."),
            Some(ProvisioningFailureKind::ContainerImageBuildFailed)
        );
        assert_eq!(
            classify_provisioning_failure(
                Some(1),
                "singularity build --fakeroot /tmp/image.sif /tmp/def.def"
            ),
            Some(ProvisioningFailureKind::ContainerImageBuildFailed)
        );
    }

    #[test]
    fn classifies_unknown_exit_127_as_unknown() {
        assert_eq!(
            classify_provisioning_failure(Some(127), "some unrelated exit-127 business error"),
            Some(ProvisioningFailureKind::Unknown)
        );
        assert_eq!(
            classify_provisioning_failure(Some(2), "unrelated command failure"),
            None
        );
    }

    #[test]
    fn fallback_suggestions_for_conda_manager_missing_prefers_available_container_runtime() {
        let availability = vec![record("docker", "available"), record("conda", "missing")];
        let suggestions =
            fallback_suggestions(&ProvisioningFailureKind::CondaManagerMissing, &availability);
        let actions = suggestions
            .iter()
            .map(|suggestion| suggestion.action.as_str())
            .collect::<Vec<_>>();
        assert!(actions.contains(&"use_docker_environment"));
        assert!(!actions.contains(&"use_singularity_environment"));
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn fallback_suggestions_for_conda_manager_missing_limits_four_with_install_first() {
        let availability = vec![
            record("docker", "available"),
            record("singularity", "available"),
            record("conda", "missing"),
        ];
        let suggestions =
            fallback_suggestions(&ProvisioningFailureKind::CondaManagerMissing, &availability);

        assert!(suggestions.len() <= 4);
        assert_eq!(suggestions.first().unwrap().action, "install_micromamba");
        assert_eq!(suggestions[0].title, "安装 Micromamba");
    }

    #[test]
    fn fallback_suggestions_for_docker_missing_without_alternative() {
        let availability = vec![record("conda", "available")];
        let suggestions = fallback_suggestions(
            &ProvisioningFailureKind::DockerRuntimeMissing,
            &availability,
        );
        let actions = suggestions
            .iter()
            .map(|suggestion| suggestion.action.as_str())
            .collect::<Vec<_>>();
        assert!(actions.contains(&"start_docker_daemon"));
        assert!(!actions.contains(&"use_singularity_environment"));
    }
}
