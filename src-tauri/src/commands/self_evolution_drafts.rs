//! Read-only review commands for inert self-evolution draft batches.

use super::CommandResult;
use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DRAFT_ROOT_RELATIVE: &str = ".omiga/learning/self-evolution-drafts";
const PROMOTION_ARTIFACT_ROOT_RELATIVE: &str = ".omiga/review/self-evolution-promotions/artifacts";
const MAX_TEXT_PREVIEW_BYTES: u64 = 64 * 1024;
const MAX_PROMOTION_CONTENT_BYTES: u64 = 1024 * 1024;
const COMPANION_PAYLOAD_DIR: &str = "companion-payloads";
const SAFETY_NOTE: &str = "Read-only self-evolution draft review. These commands never apply drafts, register units, update defaults, archive artifacts, or mutate project configuration.";
const PROMOTION_DRY_RUN_NOTE: &str = "Promotion patch dry-run only. This command only computes the proposed target path, risk notes, and unified diff preview; it never writes target files, applies patches, registers units, changes defaults, or mutates archives.";
const PROMOTION_ARTIFACT_NOTE: &str = "Promotion review artifact only. This command writes inert review files under .omiga/review/self-evolution-promotions/artifacts and never writes the proposed target, applies patches, registers units, changes defaults, or mutates archives.";
const SPECIALIZED_DRAFT_FILENAMES: &[&str] = &[
    "template.yaml.draft",
    "operator.yaml.draft",
    "project-preference.json.draft",
    "archive-marker.json.draft",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftSummary {
    pub draft_dir: String,
    pub candidate_id: String,
    pub kind: String,
    pub title: Option<String>,
    pub priority: Option<String>,
    pub created_by: Option<String>,
    pub files: Vec<String>,
    pub specialized_drafts: Vec<String>,
    pub companion_drafts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftBatchSummary {
    pub batch_dir: String,
    pub index_path: Option<String>,
    pub generated_at: Option<String>,
    pub draft_count: usize,
    pub drafts: Vec<SelfEvolutionDraftSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftListResponse {
    pub root_dir: String,
    pub batch_count: usize,
    pub batches: Vec<SelfEvolutionDraftBatchSummary>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftFilePreview {
    pub path: String,
    pub role: String,
    pub bytes: u64,
    pub truncated: bool,
    pub text: Option<String>,
    pub json: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftReviewPreview {
    pub status: String,
    pub safety_note: String,
    pub candidate_id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub target_hint: Option<String>,
    pub actions: Vec<String>,
    pub diff_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftDetailResponse {
    pub found: bool,
    pub draft_dir: String,
    pub candidate: JsonValue,
    pub files: Vec<SelfEvolutionDraftFilePreview>,
    pub review_preview: SelfEvolutionDraftReviewPreview,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionPreviewResponse {
    pub status: String,
    pub safety_note: String,
    pub draft_dir: String,
    pub candidate_id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub draft_file: Option<String>,
    pub proposed_target_path: Option<String>,
    pub target_exists: bool,
    pub diff_preview: Option<String>,
    pub companion_drafts: Vec<String>,
    pub companion_review_steps: Vec<String>,
    pub risk_notes: Vec<String>,
    pub required_review_steps: Vec<String>,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionCompanionPayload {
    pub source_path: String,
    pub artifact_path: String,
    pub role: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionArtifactResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub patch_path: String,
    pub manifest_path: String,
    pub readme_path: String,
    pub proposed_content_path: String,
    pub proposed_content_sha256: String,
    pub companion_payloads: Vec<SelfEvolutionDraftPromotionCompanionPayload>,
    pub proposed_target_path: Option<String>,
    pub preview: SelfEvolutionDraftPromotionPreviewResponse,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionArtifactSummary {
    pub artifact_dir: String,
    pub patch_path: Option<String>,
    pub manifest_path: Option<String>,
    pub readme_path: Option<String>,
    pub proposed_content_path: Option<String>,
    pub proposed_content_sha256: Option<String>,
    pub candidate_id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub proposed_target_path: Option<String>,
    pub target_exists: Option<bool>,
    pub modified_at_millis: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionArtifactListResponse {
    pub root_dir: String,
    pub artifact_count: usize,
    pub artifacts: Vec<SelfEvolutionDraftPromotionArtifactSummary>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionArtifactDetailResponse {
    pub found: bool,
    pub artifact_dir: String,
    pub manifest: JsonValue,
    pub files: Vec<SelfEvolutionDraftFilePreview>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionReadinessCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub required: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionApplyPlanResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub patch_path: Option<String>,
    pub manifest_path: Option<String>,
    pub proposed_content_path: Option<String>,
    pub proposed_target_path: Option<String>,
    pub candidate_id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub patch_sha256: Option<String>,
    pub proposed_content_sha256: Option<String>,
    pub target_exists: bool,
    pub target_current_sha256: Option<String>,
    pub companion_drafts: Vec<String>,
    pub companion_payloads: Vec<SelfEvolutionDraftPromotionCompanionPayload>,
    pub checks: Vec<SelfEvolutionDraftPromotionReadinessCheck>,
    pub required_confirmations: Vec<String>,
    pub suggested_verification: Vec<String>,
    pub apply_command_available: bool,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionApplyPlanArtifactResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub plan_json_path: String,
    pub plan_readme_path: String,
    pub plan: SelfEvolutionDraftPromotionApplyPlanResponse,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionApplyRequestResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub proposed_target_path: Option<String>,
    pub candidate_id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub patch_sha256: Option<String>,
    pub proposed_content_sha256: Option<String>,
    pub target_exists: bool,
    pub target_current_sha256: Option<String>,
    pub companion_drafts: Vec<String>,
    pub checks: Vec<SelfEvolutionDraftPromotionReadinessCheck>,
    pub required_confirmations: Vec<String>,
    pub suggested_verification: Vec<String>,
    pub apply_command_available: bool,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionApplyResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub proposed_content_path: Option<String>,
    pub proposed_target_path: Option<String>,
    pub candidate_id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub proposed_content_sha256: Option<String>,
    pub target_exists_before: bool,
    pub target_previous_sha256: Option<String>,
    pub target_new_sha256: Option<String>,
    pub companion_drafts: Vec<String>,
    pub bytes_written: u64,
    pub checks: Vec<SelfEvolutionDraftPromotionReadinessCheck>,
    pub suggested_verification: Vec<String>,
    pub apply_command_available: bool,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionCompanionTargetInput {
    pub artifact_path: String,
    pub target_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionCompanionTargetPlan {
    pub source_path: String,
    pub artifact_path: String,
    pub role: String,
    pub bytes: u64,
    pub sha256: String,
    pub proposed_target_path: Option<String>,
    pub target_exists: bool,
    pub target_current_sha256: Option<String>,
    pub diff_preview: Option<String>,
    pub checks: Vec<SelfEvolutionDraftPromotionReadinessCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionMultiFilePlanResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub manifest_target_path: Option<String>,
    pub companion_targets: Vec<SelfEvolutionDraftPromotionCompanionTargetPlan>,
    pub checks: Vec<SelfEvolutionDraftPromotionReadinessCheck>,
    pub required_review_steps: Vec<String>,
    pub suggested_verification: Vec<String>,
    pub apply_command_available: bool,
    pub would_write: bool,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse {
    pub status: String,
    pub safety_note: String,
    pub artifact_dir: String,
    pub plan_json_path: String,
    pub plan_readme_path: String,
    pub plan: SelfEvolutionDraftPromotionMultiFilePlanResponse,
    pub would_write: bool,
    pub applied: bool,
}

fn resolve_project_root(project_root: Option<String>) -> PathBuf {
    let raw = project_root.unwrap_or_default();
    let trimmed = raw.trim();
    let path = if trimmed.is_empty() || trimmed == "." {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(trimmed)
    };
    path.canonicalize().unwrap_or(path)
}

fn draft_root(project_root: &Path) -> PathBuf {
    project_root.join(DRAFT_ROOT_RELATIVE)
}

fn promotion_artifact_root(project_root: &Path) -> PathBuf {
    project_root.join(PROMOTION_ARTIFACT_ROOT_RELATIVE)
}

fn review_promotion_root(project_root: &Path) -> PathBuf {
    project_root.join(".omiga/review/self-evolution-promotions")
}

fn project_relative_path(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn candidate_string(candidate: &JsonValue, field: &str) -> Option<String> {
    candidate
        .get(field)
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
}

fn nested_string(value: &JsonValue, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(ToOwned::to_owned)
}

fn nested_string_array(value: &JsonValue, path: &[&str]) -> Vec<String> {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return Vec::new();
        };
        current = next;
    }
    current
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .filter(|item| !item.trim().is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn companion_payloads_from_manifest(
    manifest: &JsonValue,
) -> Vec<SelfEvolutionDraftPromotionCompanionPayload> {
    manifest
        .get("companionPayloads")
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    serde_json::from_value::<SelfEvolutionDraftPromotionCompanionPayload>(
                        item.clone(),
                    )
                    .ok()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_text_lossy(path: &Path, limit: u64) -> Option<(String, u64, bool)> {
    let metadata = std::fs::metadata(path).ok()?;
    let bytes = metadata.len();
    let truncated = bytes > limit;
    let file = std::fs::File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.take(limit).read_to_end(&mut buffer).ok()?;
    Some((
        String::from_utf8_lossy(&buffer).into_owned(),
        bytes,
        truncated,
    ))
}

fn read_text_strict(path: &Path, limit: u64) -> Result<String, AppError> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| AppError::Config(format!("read file metadata failed: {err}")))?;
    if metadata.len() > limit {
        return Err(AppError::Config(format!(
            "file is too large for promotion artifact payload ({} bytes > {} bytes)",
            metadata.len(),
            limit
        )));
    }
    std::fs::read_to_string(path)
        .map_err(|err| AppError::Config(format!("read text file failed: {err}")))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn sha256_text(text: &str) -> String {
    sha256_bytes(text.as_bytes())
}

fn sha256_file(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| sha256_bytes(&bytes))
}

fn safe_artifact_filename(value: &str) -> String {
    let mut out = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').to_string()
}

fn resolve_draft_file_path(project_root: &Path, raw: &str) -> Result<PathBuf, AppError> {
    let root = draft_root(project_root)
        .canonicalize()
        .unwrap_or_else(|_| draft_root(project_root));
    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        project_root.join(raw)
    };
    let candidate = candidate
        .canonicalize()
        .map_err(|_| AppError::Config("draft file was not found".to_string()))?;
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Err(AppError::Config(
            "draft file must be inside .omiga/learning/self-evolution-drafts".to_string(),
        ));
    }
    Ok(candidate)
}

fn read_candidate_json(draft_dir: &Path) -> JsonValue {
    std::fs::read_to_string(draft_dir.join("candidate.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
        .unwrap_or(JsonValue::Null)
}

fn parse_generated_at(readme: &str) -> Option<String> {
    readme.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("- Generated at:") {
            return None;
        }
        trimmed
            .split_once(':')
            .map(|(_, value)| value.trim().trim_matches('`').to_string())
            .filter(|value| !value.is_empty())
    })
}

fn file_role(path: &Path) -> String {
    if path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some(COMPANION_PAYLOAD_DIR)
    {
        return "promotion_companion_payload".to_string();
    }
    match path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
    {
        "README.md" => "batch_index",
        "DRAFT.md" => "review_checklist",
        "candidate.json" => "candidate_json",
        "manifest.json" => "promotion_manifest",
        "promotion.patch" => "promotion_patch",
        "proposed-target.content" => "promotion_proposed_content",
        "apply-readiness.json" => "promotion_apply_readiness_json",
        "APPLY_READINESS.md" => "promotion_apply_readiness",
        "multi-file-promotion-plan.json" => "promotion_multi_file_plan_json",
        "MULTI_FILE_PROMOTION_PLAN.md" => "promotion_multi_file_plan",
        "template.yaml.draft" => "template_draft",
        "template.sh.j2.draft" => "template_entry_draft",
        "template.R.j2.draft" => "template_entry_draft",
        "template.py.j2.draft" => "template_entry_draft",
        "example-input.tsv.draft" => "template_example_input_draft",
        "example.tsv.draft" => "template_example_input_draft",
        "operator.yaml.draft" => "operator_draft",
        "operator.py.draft" => "operator_script_draft",
        "operator-script.py.draft" => "operator_script_draft",
        "fixture.json.draft" => "operator_fixture_draft",
        "project-preference.json.draft" => "project_preference_draft",
        "archive-marker.json.draft" => "archive_marker_draft",
        name if name.ends_with(".j2.draft") => "template_entry_draft",
        name if name.ends_with(".fixture.json.draft") => "operator_fixture_draft",
        name if name.ends_with(".draft") => "draft_file",
        _ => "supporting_file",
    }
    .to_string()
}

fn list_immediate_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    files
}

fn list_immediate_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    dirs
}

fn find_specialized_draft_file(draft_dir: &Path) -> Option<PathBuf> {
    for filename in SPECIALIZED_DRAFT_FILENAMES {
        let path = draft_dir.join(filename);
        if path.is_file() {
            return Some(path);
        }
    }
    list_immediate_files(draft_dir).into_iter().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".draft"))
    })
}

fn companion_draft_files(draft_dir: &Path, primary_draft: Option<&Path>) -> Vec<PathBuf> {
    list_immediate_files(draft_dir)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".draft"))
        })
        .filter(|path| primary_draft != Some(path.as_path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !SPECIALIZED_DRAFT_FILENAMES.contains(&name))
        })
        .collect()
}

fn companion_review_steps(companion_drafts: &[String]) -> Vec<String> {
    if companion_drafts.is_empty() {
        return Vec::new();
    }
    vec![
        format!(
            "Review {} companion draft file(s) before promotion; the current single-file apply writes only the selected manifest target.",
            companion_drafts.len()
        ),
        "Move or merge companion scripts, fixtures, examples, and template entries through a separate reviewed patch when promoting into an active plugin path.".to_string(),
        "Run unit_authoring_validate and deterministic smoke tests after companion files are placed beside the promoted manifest.".to_string(),
    ]
}

fn safe_slug(value: Option<String>, fallback: &str) -> String {
    let raw = value.unwrap_or_else(|| fallback.to_string());
    let slug = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    };
    slug.chars().take(80).collect()
}

fn default_promotion_target_for_draft(candidate: &JsonValue, draft_file: &Path) -> Option<String> {
    let slug = safe_slug(
        candidate_string(candidate, "id")
            .or_else(|| candidate_string(candidate, "title"))
            .or_else(|| {
                draft_file
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
            }),
        "draft",
    );
    match draft_file.file_name().and_then(|name| name.to_str())? {
        "template.yaml.draft" => Some(format!(
            ".omiga/review/self-evolution-promotions/templates/{slug}/template.yaml"
        )),
        "operator.yaml.draft" => Some(format!(
            ".omiga/review/self-evolution-promotions/operators/{slug}/operator.yaml"
        )),
        "project-preference.json.draft" => Some(format!(
            ".omiga/review/self-evolution-promotions/project-preferences/{slug}.json"
        )),
        "archive-marker.json.draft" => Some(format!(
            ".omiga/review/self-evolution-promotions/archive-markers/{slug}.json"
        )),
        name if name.ends_with(".draft") => {
            let target = name.trim_end_matches(".draft");
            Some(format!(
                ".omiga/review/self-evolution-promotions/drafts/{slug}/{target}"
            ))
        }
        _ => None,
    }
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(value) => normalized.push(value),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn nearest_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        if current.exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn validate_explicit_target_path(project_root: &Path, raw: &str) -> Result<PathBuf, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config("targetPath must not be empty".to_string()));
    }
    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| normalize_path_components(project_root));
    let raw_path = Path::new(trimmed);
    let target = if raw_path.is_absolute() {
        normalize_path_components(raw_path)
    } else {
        normalize_path_components(&project_root.join(raw_path))
    };
    if !target.starts_with(&project_root) {
        return Err(AppError::Config(
            "targetPath must stay inside projectRoot".to_string(),
        ));
    }
    let draft_root = draft_root(&project_root)
        .canonicalize()
        .unwrap_or_else(|_| normalize_path_components(&draft_root(&project_root)));
    if target.starts_with(&draft_root) {
        return Err(AppError::Config(
            "targetPath must not point into self-evolution draft storage".to_string(),
        ));
    }
    if target.exists() && !target.is_file() {
        return Err(AppError::Config(
            "targetPath must point to a file or missing file path".to_string(),
        ));
    }
    let target_check = if target.exists() {
        target.canonicalize().unwrap_or_else(|_| target.clone())
    } else {
        target.clone()
    };
    if !target_check.starts_with(&project_root) || target_check.starts_with(&draft_root) {
        return Err(AppError::Config(
            "targetPath resolves outside the safe project target area".to_string(),
        ));
    }
    let Some(parent) = nearest_existing_parent(&target) else {
        return Err(AppError::Config(
            "targetPath parent could not be validated".to_string(),
        ));
    };
    let parent = parent.canonicalize().unwrap_or(parent);
    if !parent.starts_with(&project_root) || parent.starts_with(&draft_root) {
        return Err(AppError::Config(
            "targetPath parent must stay inside projectRoot and outside draft storage".to_string(),
        ));
    }
    Ok(target)
}

fn diff_line_count(text: &str) -> usize {
    text.lines().count()
}

fn append_prefixed_lines(out: &mut String, prefix: char, text: &str, limit: usize) {
    for line in text.lines().take(limit) {
        out.push(prefix);
        out.push_str(line);
        out.push('\n');
    }
    if diff_line_count(text) > limit {
        out.push(prefix);
        out.push_str("… truncated …\n");
    }
}

fn promotion_diff_preview(target_label: &str, existing: Option<&str>, draft: &str) -> String {
    let mut out = String::new();
    out.push_str("# PROMOTION PATCH DRY-RUN — not applied\n");
    match existing {
        Some(current) => {
            out.push_str(&format!("--- {target_label}\n"));
            out.push_str(&format!("+++ {target_label} (draft)\n"));
            out.push_str("@@\n");
            append_prefixed_lines(&mut out, '-', current, 120);
            append_prefixed_lines(&mut out, '+', draft, 160);
        }
        None => {
            out.push_str("--- /dev/null\n");
            out.push_str(&format!("+++ {target_label}\n"));
            out.push_str("@@\n");
            append_prefixed_lines(&mut out, '+', draft, 160);
        }
    }
    out
}

fn base_promotion_risk_notes(
    candidate: &JsonValue,
    draft_file: Option<&Path>,
    explicit_target: bool,
    target_exists: bool,
    companion_drafts: &[String],
) -> Vec<String> {
    let mut notes = vec![
        "Dry-run only: no filesystem write, unit registration, default update, archive mutation, or apply step was performed.".to_string(),
        "Generated drafts can be stale or incomplete; validate provenance, parameters, fixtures, and deterministic behavior before promotion.".to_string(),
    ];
    if candidate == &JsonValue::Null {
        notes.push("candidate.json is missing or invalid; provenance and candidate metadata require manual reconstruction.".to_string());
    }
    if draft_file.is_none() {
        notes.push(
            "No specialized .draft file was found; there is no patch content to promote."
                .to_string(),
        );
    }
    if !explicit_target {
        notes.push("No explicit targetPath was supplied; proposed target is a review-only holding path, not an active plugin/config location.".to_string());
    }
    if target_exists {
        notes.push("Target already exists; promotion would replace or merge with existing content and needs careful review.".to_string());
    }
    if !companion_drafts.is_empty() {
        notes.push("Companion draft files are present; this promotion preview/artifact/apply path only carries the selected manifest payload, not companion scripts, fixtures, examples, or rendered entries.".to_string());
    }
    notes
}

fn required_promotion_review_steps(
    target_exists: bool,
    companion_drafts: &[String],
) -> Vec<String> {
    let mut steps = vec![
        "Review candidate.json provenance, source ExecutionRecords, and DRAFT.md checklist.".to_string(),
        "Inspect the unified diff and confirm the proposed target path is intentional.".to_string(),
        "Add or update deterministic unit fixtures/tests before any real promotion patch.".to_string(),
        "Apply changes only through a separate explicit reviewed commit or patch; this dry-run is not an apply command.".to_string(),
    ];
    if target_exists {
        steps.insert(
            2,
            "Compare existing target behavior and decide whether to merge, replace, or reject the draft.".to_string(),
        );
    }
    if !companion_drafts.is_empty() {
        steps.insert(
            steps.len().saturating_sub(1),
            "Review companion draft files and plan their separate active-plugin locations; single-file apply will not move them.".to_string(),
        );
    }
    steps
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn promotion_artifact_slug(preview: &SelfEvolutionDraftPromotionPreviewResponse) -> String {
    let label = preview
        .candidate_id
        .clone()
        .or_else(|| preview.title.clone())
        .or_else(|| {
            Path::new(&preview.draft_dir)
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        });
    format!(
        "promotion-{}-{}",
        unix_timestamp_millis(),
        safe_slug(label, "draft")
    )
}

fn render_promotion_artifact_readme(
    preview: &SelfEvolutionDraftPromotionPreviewResponse,
    patch_path: &str,
    manifest_path: &str,
    proposed_content_path: &str,
    proposed_content_sha256: &str,
    companion_payloads: &[SelfEvolutionDraftPromotionCompanionPayload],
) -> String {
    let mut out = String::new();
    out.push_str("# Self-Evolution Promotion Review Artifact\n\n");
    out.push_str(&format!("- Status: `{}`\n", preview.status));
    out.push_str(&format!("- Safety: {PROMOTION_ARTIFACT_NOTE}\n"));
    out.push_str(&format!(
        "- Candidate: `{}`\n",
        preview
            .candidate_id
            .as_deref()
            .unwrap_or("unknown-candidate")
    ));
    if let Some(kind) = preview.kind.as_deref() {
        out.push_str(&format!("- Kind: `{kind}`\n"));
    }
    if let Some(title) = preview.title.as_deref() {
        out.push_str(&format!("- Title: {title}\n"));
    }
    out.push_str(&format!("- Draft dir: `{}`\n", preview.draft_dir));
    if let Some(draft_file) = preview.draft_file.as_deref() {
        out.push_str(&format!("- Draft file: `{draft_file}`\n"));
    }
    if let Some(target) = preview.proposed_target_path.as_deref() {
        out.push_str(&format!("- Proposed target: `{target}`\n"));
    }
    out.push_str(&format!("- Target exists: `{}`\n", preview.target_exists));
    out.push_str("- Would write target: `false`\n");
    out.push_str("- Applied: `false`\n");
    out.push_str(&format!("- Patch: `{patch_path}`\n"));
    out.push_str(&format!("- Manifest: `{manifest_path}`\n"));
    out.push_str(&format!("- Proposed content: `{proposed_content_path}`\n"));
    out.push_str(&format!(
        "- Proposed content sha256: `{proposed_content_sha256}`\n\n"
    ));
    if !companion_payloads.is_empty() {
        out.push_str("## Companion payloads\n\n");
        out.push_str("These files are inert review evidence copied from companion `.draft` files. They are not applied automatically by the single-file apply path.\n\n");
        for payload in companion_payloads {
            out.push_str(&format!(
                "- `{}` → `{}` ({}, {}, {} bytes)\n",
                payload.source_path,
                payload.artifact_path,
                payload.role,
                payload.sha256,
                payload.bytes
            ));
        }
        out.push('\n');
    }
    out.push_str("## Required review steps\n\n");
    for step in &preview.required_review_steps {
        out.push_str(&format!("- [ ] {step}\n"));
    }
    if !preview.risk_notes.is_empty() {
        out.push_str("\n## Risk notes\n\n");
        for note in &preview.risk_notes {
            out.push_str(&format!("- {note}\n"));
        }
    }
    out.push_str(
        "\nThis artifact is inert review evidence. Apply any real Operator/Template/config change through a separate explicit reviewed patch.\n",
    );
    out
}

fn modified_at_millis(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn summarize_promotion_artifact(
    project_root: &Path,
    artifact_dir: &Path,
) -> Option<SelfEvolutionDraftPromotionArtifactSummary> {
    let manifest_path = artifact_dir.join("manifest.json");
    let patch_path = artifact_dir.join("promotion.patch");
    let readme_path = artifact_dir.join("README.md");
    let proposed_content_path = artifact_dir.join("proposed-target.content");
    let manifest = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
        .unwrap_or(JsonValue::Null);
    Some(SelfEvolutionDraftPromotionArtifactSummary {
        artifact_dir: project_relative_path(project_root, artifact_dir),
        patch_path: patch_path
            .is_file()
            .then(|| project_relative_path(project_root, &patch_path)),
        manifest_path: manifest_path
            .is_file()
            .then(|| project_relative_path(project_root, &manifest_path)),
        readme_path: readme_path
            .is_file()
            .then(|| project_relative_path(project_root, &readme_path)),
        proposed_content_path: proposed_content_path
            .is_file()
            .then(|| project_relative_path(project_root, &proposed_content_path)),
        proposed_content_sha256: nested_string(&manifest, &["proposedContentSha256"]),
        candidate_id: nested_string(&manifest, &["preview", "candidateId"]),
        kind: nested_string(&manifest, &["preview", "kind"]),
        title: nested_string(&manifest, &["preview", "title"]),
        proposed_target_path: nested_string(&manifest, &["proposedTargetPath"])
            .or_else(|| nested_string(&manifest, &["preview", "proposedTargetPath"])),
        target_exists: manifest
            .get("preview")
            .and_then(|preview| preview.get("targetExists"))
            .and_then(JsonValue::as_bool),
        modified_at_millis: modified_at_millis(&manifest_path)
            .or_else(|| modified_at_millis(artifact_dir)),
    })
}

fn readiness_check(
    checks: &mut Vec<SelfEvolutionDraftPromotionReadinessCheck>,
    id: &str,
    label: &str,
    passed: bool,
    required: bool,
    detail: impl Into<String>,
) {
    checks.push(SelfEvolutionDraftPromotionReadinessCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        required,
        detail: detail.into(),
    });
}

fn target_is_review_holding_path(project_root: &Path, target_path: &Path) -> bool {
    let review_root = review_promotion_root(project_root)
        .canonicalize()
        .unwrap_or_else(|_| normalize_path_components(&review_promotion_root(project_root)));
    target_path.starts_with(review_root)
}

fn expected_target_suffix_for_kind(kind: Option<&str>) -> Option<&'static str> {
    match kind {
        Some("template_candidate") => Some("template.yaml"),
        Some("operator_candidate") => Some("operator.yaml"),
        Some("project_preference_candidate") => Some(".json"),
        Some("archive_candidate") => Some(".json"),
        _ => None,
    }
}

fn suggested_promotion_verification(kind: Option<&str>) -> Vec<String> {
    let mut verification = vec![
        "Review promotion.patch and manifest.json in a separate branch.".to_string(),
        "Run unit_authoring_validate or the equivalent manifest validation after applying manually."
            .to_string(),
        "Run targeted Operator/Template smoke tests for the promoted unit.".to_string(),
    ];
    match kind {
        Some("template_candidate") => verification.push(
            "Run template discovery/execution tests for the promoted template and any migration target."
                .to_string(),
        ),
        Some("operator_candidate") => verification.push(
            "Run operator schema/discovery tests and at least one offline_fixture smoke path."
                .to_string(),
        ),
        Some("project_preference_candidate") => verification.push(
            "Verify explicit user params still override promoted project preferences.".to_string(),
        ),
        Some("archive_candidate") => verification.push(
            "Verify archive recommendations do not delete or move artifacts without a separate approval."
                .to_string(),
        ),
        _ => verification.push("Identify the candidate kind before applying any real patch.".to_string()),
    }
    verification
}

fn render_promotion_apply_plan_readme(
    plan: &SelfEvolutionDraftPromotionApplyPlanResponse,
    plan_json_path: &str,
) -> String {
    let mut out = String::new();
    out.push_str("# Self-Evolution Promotion Apply Readiness\n\n");
    out.push_str(&format!("- Status: `{}`\n", plan.status));
    out.push_str(&format!("- Safety: {}\n", plan.safety_note));
    out.push_str(&format!("- Artifact: `{}`\n", plan.artifact_dir));
    out.push_str(&format!("- Plan JSON: `{plan_json_path}`\n"));
    if let Some(candidate_id) = plan.candidate_id.as_deref() {
        out.push_str(&format!("- Candidate: `{candidate_id}`\n"));
    }
    if let Some(kind) = plan.kind.as_deref() {
        out.push_str(&format!("- Kind: `{kind}`\n"));
    }
    if let Some(title) = plan.title.as_deref() {
        out.push_str(&format!("- Title: {title}\n"));
    }
    if let Some(target) = plan.proposed_target_path.as_deref() {
        out.push_str(&format!("- Proposed target: `{target}`\n"));
    }
    if let Some(hash) = plan.patch_sha256.as_deref() {
        out.push_str(&format!("- Patch sha256: `{hash}`\n"));
    }
    if let Some(path) = plan.proposed_content_path.as_deref() {
        out.push_str(&format!("- Proposed content payload: `{path}`\n"));
    }
    if let Some(hash) = plan.proposed_content_sha256.as_deref() {
        out.push_str(&format!("- Proposed content sha256: `{hash}`\n"));
    }
    out.push_str(&format!("- Target exists: `{}`\n", plan.target_exists));
    if let Some(hash) = plan.target_current_sha256.as_deref() {
        out.push_str(&format!("- Current target sha256: `{hash}`\n"));
    }
    if !plan.companion_payloads.is_empty() {
        out.push_str(&format!(
            "- Companion payloads: `{}`\n",
            plan.companion_payloads.len()
        ));
    }
    out.push_str(&format!(
        "- Apply command available: `{}`\n",
        plan.apply_command_available
    ));
    out.push_str("- Would write target: `false`\n");
    out.push_str("- Applied: `false`\n\n");
    if !plan.companion_payloads.is_empty() {
        out.push_str("## Companion payload audit\n\n");
        for payload in &plan.companion_payloads {
            out.push_str(&format!(
                "- `{}` → `{}` ({}, {}, {} bytes)\n",
                payload.source_path,
                payload.artifact_path,
                payload.role,
                payload.sha256,
                payload.bytes
            ));
        }
        out.push('\n');
    }
    out.push_str("## Readiness checks\n\n");
    for check in &plan.checks {
        let marker = if check.status == "passed" {
            "[x]"
        } else {
            "[ ]"
        };
        out.push_str(&format!(
            "- {marker} `{}` — {} ({})\n  - {}\n",
            check.id, check.label, check.status, check.detail
        ));
    }
    if !plan.required_confirmations.is_empty() {
        out.push_str("\n## Required confirmations before any future apply\n\n");
        for item in &plan.required_confirmations {
            out.push_str(&format!("- [ ] {item}\n"));
        }
    }
    if !plan.suggested_verification.is_empty() {
        out.push_str("\n## Suggested verification after a separate reviewed apply\n\n");
        for item in &plan.suggested_verification {
            out.push_str(&format!("- [ ] {item}\n"));
        }
    }
    out.push_str(
        "\nThis file is inert review evidence. It is not an apply command and does not write the proposed target.\n",
    );
    out
}

fn render_multi_file_promotion_plan_readme(
    plan: &SelfEvolutionDraftPromotionMultiFilePlanResponse,
    plan_json_path: &str,
) -> String {
    let mut out = String::new();
    out.push_str("# Self-Evolution Multi-File Promotion Plan\n\n");
    out.push_str(&format!("- Status: `{}`\n", plan.status));
    out.push_str(&format!("- Safety: {}\n", plan.safety_note));
    out.push_str(&format!("- Artifact: `{}`\n", plan.artifact_dir));
    out.push_str(&format!("- Plan JSON: `{plan_json_path}`\n"));
    if let Some(target) = plan.manifest_target_path.as_deref() {
        out.push_str(&format!("- Manifest target: `{target}`\n"));
    }
    out.push_str(&format!(
        "- Companion targets: `{}`\n",
        plan.companion_targets.len()
    ));
    out.push_str("- Would write: `false`\n");
    out.push_str("- Applied: `false`\n\n");

    if !plan.companion_targets.is_empty() {
        out.push_str("## Companion target plan\n\n");
        for target in &plan.companion_targets {
            out.push_str(&format!(
                "### `{}`\n\n- Source: `{}`\n- Role: `{}`\n- Payload sha256: `{}`\n",
                target.artifact_path, target.source_path, target.role, target.sha256
            ));
            match target.proposed_target_path.as_deref() {
                Some(path) => out.push_str(&format!("- Proposed target: `{path}`\n")),
                None => out.push_str("- Proposed target: `<missing>`\n"),
            }
            out.push_str(&format!("- Target exists: `{}`\n", target.target_exists));
            if let Some(hash) = target.target_current_sha256.as_deref() {
                out.push_str(&format!("- Current target sha256: `{hash}`\n"));
            }
            out.push_str("- Checks:\n");
            for check in &target.checks {
                let marker = if check.status == "passed" {
                    "[x]"
                } else {
                    "[ ]"
                };
                out.push_str(&format!(
                    "  - {marker} `{}` — {} ({})\n    - {}\n",
                    check.id, check.label, check.status, check.detail
                ));
            }
            out.push('\n');
        }
    }

    out.push_str("## Reviewed patch application checklist\n\n");
    out.push_str(
        "- [ ] Create a separate reviewed branch or commit for companion file placement.\n",
    );
    out.push_str("- [ ] Review the manifest promotion patch together with every companion target diff below.\n");
    for target in &plan.companion_targets {
        match target.proposed_target_path.as_deref() {
            Some(path) => out.push_str(&format!(
                "- [ ] Copy reviewed payload `{}` to `{}` ({}, {}, {}).\n",
                target.artifact_path,
                path,
                target.role,
                if target.target_exists {
                    "replace/merge"
                } else {
                    "create"
                },
                target.sha256
            )),
            None => out.push_str(&format!(
                "- [ ] Choose an active target for `{}` before applying any companion patch.\n",
                target.artifact_path
            )),
        }
    }
    out.push_str("- [ ] Run `unit_authoring_validate` and deterministic Operator/Template smoke tests after the separate patch.\n");
    out.push_str("- [ ] Do not register units, change defaults, or mutate archives unless a separate reviewed step explicitly approves it.\n\n");

    out.push_str("## Plan-level checks\n\n");
    for check in &plan.checks {
        let marker = if check.status == "passed" {
            "[x]"
        } else {
            "[ ]"
        };
        out.push_str(&format!(
            "- {marker} `{}` — {} ({})\n  - {}\n",
            check.id, check.label, check.status, check.detail
        ));
    }
    if !plan.required_review_steps.is_empty() {
        out.push_str("\n## Required review steps\n\n");
        for step in &plan.required_review_steps {
            out.push_str(&format!("- [ ] {step}\n"));
        }
    }
    if !plan.suggested_verification.is_empty() {
        out.push_str("\n## Suggested verification\n\n");
        for item in &plan.suggested_verification {
            out.push_str(&format!("- [ ] {item}\n"));
        }
    }
    out.push_str(
        "\nThis file is inert review evidence. It does not apply companion files or write any active Operator/Template target.\n",
    );
    out
}

fn summarize_draft(project_root: &Path, draft_dir: &Path) -> Option<SelfEvolutionDraftSummary> {
    let candidate_path = draft_dir.join("candidate.json");
    let candidate = std::fs::read_to_string(candidate_path)
        .ok()
        .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
        .unwrap_or(JsonValue::Null);
    let candidate_id = candidate_string(&candidate, "id").or_else(|| {
        draft_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
    })?;
    let kind = candidate_string(&candidate, "kind").unwrap_or_else(|| "candidate".to_string());
    let files = list_immediate_files(draft_dir)
        .iter()
        .map(|path| project_relative_path(project_root, path))
        .collect::<Vec<_>>();
    let specialized_drafts = files
        .iter()
        .filter(|path| path.ends_with(".draft"))
        .cloned()
        .collect::<Vec<_>>();
    let companion_drafts =
        companion_draft_files(draft_dir, find_specialized_draft_file(draft_dir).as_deref())
            .iter()
            .map(|path| project_relative_path(project_root, path))
            .collect::<Vec<_>>();
    Some(SelfEvolutionDraftSummary {
        draft_dir: project_relative_path(project_root, draft_dir),
        candidate_id,
        kind,
        title: candidate_string(&candidate, "title"),
        priority: candidate_string(&candidate, "priority"),
        created_by: nested_string(&candidate, &["evidence", "createdBy"]),
        files,
        specialized_drafts,
        companion_drafts,
    })
}

fn summarize_batch(project_root: &Path, batch_dir: &Path) -> SelfEvolutionDraftBatchSummary {
    let index_path = batch_dir.join("README.md");
    let generated_at = read_text_lossy(&index_path, MAX_TEXT_PREVIEW_BYTES)
        .and_then(|(text, _, _)| parse_generated_at(&text));
    let drafts = list_immediate_dirs(batch_dir)
        .iter()
        .filter_map(|draft_dir| summarize_draft(project_root, draft_dir))
        .collect::<Vec<_>>();
    SelfEvolutionDraftBatchSummary {
        batch_dir: project_relative_path(project_root, batch_dir),
        index_path: index_path
            .is_file()
            .then(|| project_relative_path(project_root, &index_path)),
        generated_at,
        draft_count: drafts.len(),
        drafts,
    }
}

fn resolve_draft_dir(project_root: &Path, raw: &str) -> Result<Option<PathBuf>, AppError> {
    let root = draft_root(project_root)
        .canonicalize()
        .unwrap_or_else(|_| draft_root(project_root));
    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        project_root.join(raw)
    };
    let Some(candidate) = candidate.canonicalize().ok() else {
        return Ok(None);
    };
    if !candidate.starts_with(&root) {
        return Err(AppError::Config(
            "draftDir must be inside .omiga/learning/self-evolution-drafts".to_string(),
        ));
    }
    Ok(candidate.is_dir().then_some(candidate))
}

fn resolve_promotion_artifact_dir(
    project_root: &Path,
    raw: &str,
) -> Result<Option<PathBuf>, AppError> {
    let root = promotion_artifact_root(project_root)
        .canonicalize()
        .unwrap_or_else(|_| promotion_artifact_root(project_root));
    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        project_root.join(raw)
    };
    let Some(candidate) = candidate.canonicalize().ok() else {
        return Ok(None);
    };
    if !candidate.starts_with(&root) {
        return Err(AppError::Config(
            "artifactDir must be inside .omiga/review/self-evolution-promotions/artifacts"
                .to_string(),
        ));
    }
    Ok(candidate.is_dir().then_some(candidate))
}

fn read_file_preview(project_root: &Path, path: &Path) -> Option<SelfEvolutionDraftFilePreview> {
    let (text, bytes, truncated) = read_text_lossy(path, MAX_TEXT_PREVIEW_BYTES)?;
    let filename = path.file_name().and_then(|name| name.to_str());
    let json = if filename == Some("candidate.json")
        || filename == Some("manifest.json")
        || filename == Some("apply-readiness.json")
        || filename == Some("multi-file-promotion-plan.json")
        || path.extension().and_then(|ext| ext.to_str()) == Some("draft")
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".json.draft"))
    {
        serde_json::from_str::<JsonValue>(&text).ok()
    } else {
        None
    };
    Some(SelfEvolutionDraftFilePreview {
        path: project_relative_path(project_root, path),
        role: file_role(path),
        bytes,
        truncated,
        text: Some(text),
        json,
    })
}

fn list_promotion_artifact_files(artifact_dir: &Path) -> Vec<PathBuf> {
    let mut files = list_immediate_files(artifact_dir);
    let companion_dir = artifact_dir.join(COMPANION_PAYLOAD_DIR);
    if companion_dir.is_dir() {
        files.extend(list_immediate_files(&companion_dir));
    }
    files.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    files
}

fn target_hint_for_file(path: &Path) -> Option<String> {
    match path.file_name().and_then(|name| name.to_str())? {
        "template.yaml.draft" => {
            Some("Manual target after review: plugin templates/<id>/template.yaml".to_string())
        }
        "operator.yaml.draft" => {
            Some("Manual target after review: plugin operators/<id>/operator.yaml".to_string())
        }
        "project-preference.json.draft" => Some(
            "Manual target after review: project preference store or reviewed config patch"
                .to_string(),
        ),
        "archive-marker.json.draft" => {
            Some("Manual target after review: explicit archive manifest/marker patch".to_string())
        }
        _ => None,
    }
}

fn diff_preview_for_file(path: &Path, text: &str) -> String {
    let target = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.trim_end_matches(".draft"))
        .unwrap_or("review-target");
    let mut out = String::new();
    out.push_str("# REVIEW PREVIEW ONLY — not applied\n");
    out.push_str("--- /dev/null\n");
    out.push_str(&format!("+++ review-target/{target}\n"));
    out.push_str("@@\n");
    for line in text.lines().take(160) {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    if text.lines().count() > 160 {
        out.push_str("+… truncated …\n");
    }
    out
}

fn build_review_preview(
    draft_dir: &Path,
    candidate: &JsonValue,
    files: &[SelfEvolutionDraftFilePreview],
) -> SelfEvolutionDraftReviewPreview {
    let specialized = find_specialized_draft_file(draft_dir);
    let target_hint = specialized.as_deref().and_then(target_hint_for_file);
    let diff_preview = specialized.as_ref().and_then(|path| {
        read_text_lossy(path, MAX_TEXT_PREVIEW_BYTES)
            .map(|(text, _, _)| diff_preview_for_file(path, &text))
    });
    let mut actions = vec![
        "Inspect DRAFT.md checklist and candidate.json provenance.".to_string(),
        "Review specialized *.draft content and edit it in a separate branch if useful.".to_string(),
        "Add deterministic fixtures/tests before promoting any real Operator or Template.".to_string(),
        "Apply only through a separate explicit reviewed patch; this preview does not write targets.".to_string(),
    ];
    let companion_drafts = companion_draft_files(draft_dir, specialized.as_deref());
    if !companion_drafts.is_empty() {
        actions.insert(
            2,
            "Review companion .draft files; single-file promotion apply writes only the selected manifest file and will not move scripts, fixtures, examples, or template entries.".to_string(),
        );
    }
    if files.iter().all(|file| file.role != "candidate_json") {
        actions.insert(
            0,
            "Candidate JSON is missing; treat this draft as incomplete.".to_string(),
        );
    }
    SelfEvolutionDraftReviewPreview {
        status: "review_only".to_string(),
        safety_note: SAFETY_NOTE.to_string(),
        candidate_id: candidate_string(candidate, "id"),
        kind: candidate_string(candidate, "kind"),
        title: candidate_string(candidate, "title"),
        target_hint,
        actions,
        diff_preview,
    }
}

#[tauri::command]
pub async fn list_self_evolution_drafts(
    project_root: Option<String>,
    limit: Option<usize>,
) -> CommandResult<SelfEvolutionDraftListResponse> {
    let project_root = resolve_project_root(project_root);
    let root = draft_root(&project_root);
    if !root.is_dir() {
        return Ok(SelfEvolutionDraftListResponse {
            root_dir: project_relative_path(&project_root, &root),
            batch_count: 0,
            batches: Vec::new(),
            note: SAFETY_NOTE.to_string(),
        });
    }
    let mut batch_dirs = list_immediate_dirs(&root)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("draft-batch-") || name.starts_with("creator-batch-")
                })
        })
        .collect::<Vec<_>>();
    batch_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    let batches = batch_dirs
        .iter()
        .take(limit.unwrap_or(20).clamp(1, 100))
        .map(|batch_dir| summarize_batch(&project_root, batch_dir))
        .collect::<Vec<_>>();
    Ok(SelfEvolutionDraftListResponse {
        root_dir: project_relative_path(&project_root, &root),
        batch_count: batches.len(),
        batches,
        note: SAFETY_NOTE.to_string(),
    })
}

#[tauri::command]
pub async fn list_self_evolution_draft_promotion_artifacts(
    project_root: Option<String>,
    limit: Option<usize>,
) -> CommandResult<SelfEvolutionDraftPromotionArtifactListResponse> {
    let project_root = resolve_project_root(project_root);
    let root = promotion_artifact_root(&project_root);
    if !root.is_dir() {
        return Ok(SelfEvolutionDraftPromotionArtifactListResponse {
            root_dir: project_relative_path(&project_root, &root),
            artifact_count: 0,
            artifacts: Vec::new(),
            note: PROMOTION_ARTIFACT_NOTE.to_string(),
        });
    }
    let mut artifact_dirs = list_immediate_dirs(&root);
    artifact_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    let artifacts = artifact_dirs
        .iter()
        .take(limit.unwrap_or(20).clamp(1, 100))
        .filter_map(|artifact_dir| summarize_promotion_artifact(&project_root, artifact_dir))
        .collect::<Vec<_>>();
    Ok(SelfEvolutionDraftPromotionArtifactListResponse {
        root_dir: project_relative_path(&project_root, &root),
        artifact_count: artifacts.len(),
        artifacts,
        note: PROMOTION_ARTIFACT_NOTE.to_string(),
    })
}

#[tauri::command]
pub async fn read_self_evolution_draft_promotion_artifact(
    project_root: Option<String>,
    artifact_dir: String,
) -> CommandResult<SelfEvolutionDraftPromotionArtifactDetailResponse> {
    let project_root = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }
    let Some(resolved_dir) = resolve_promotion_artifact_dir(&project_root, &artifact_dir_raw)?
    else {
        return Ok(SelfEvolutionDraftPromotionArtifactDetailResponse {
            found: false,
            artifact_dir: artifact_dir_raw,
            manifest: JsonValue::Null,
            files: Vec::new(),
            note: PROMOTION_ARTIFACT_NOTE.to_string(),
        });
    };
    let files = list_promotion_artifact_files(&resolved_dir)
        .iter()
        .filter_map(|path| read_file_preview(&project_root, path))
        .collect::<Vec<_>>();
    let manifest = files
        .iter()
        .find(|file| file.role == "promotion_manifest")
        .and_then(|file| file.json.clone())
        .unwrap_or(JsonValue::Null);
    Ok(SelfEvolutionDraftPromotionArtifactDetailResponse {
        found: true,
        artifact_dir: project_relative_path(&project_root, &resolved_dir),
        manifest,
        files,
        note: PROMOTION_ARTIFACT_NOTE.to_string(),
    })
}

#[tauri::command]
pub async fn plan_self_evolution_draft_promotion_apply(
    project_root: Option<String>,
    artifact_dir: String,
) -> CommandResult<SelfEvolutionDraftPromotionApplyPlanResponse> {
    let project_root = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }
    let Some(resolved_dir) = resolve_promotion_artifact_dir(&project_root, &artifact_dir_raw)?
    else {
        return Ok(SelfEvolutionDraftPromotionApplyPlanResponse {
            status: "missing".to_string(),
            safety_note: PROMOTION_ARTIFACT_NOTE.to_string(),
            artifact_dir: artifact_dir_raw,
            patch_path: None,
            manifest_path: None,
            proposed_content_path: None,
            proposed_target_path: None,
            candidate_id: None,
            kind: None,
            title: None,
            patch_sha256: None,
            proposed_content_sha256: None,
            target_exists: false,
            target_current_sha256: None,
            companion_drafts: Vec::new(),
            companion_payloads: Vec::new(),
            checks: vec![SelfEvolutionDraftPromotionReadinessCheck {
                id: "artifact_exists".to_string(),
                label: "Artifact directory exists".to_string(),
                status: "blocked".to_string(),
                required: true,
                detail: "Promotion review artifact directory was not found.".to_string(),
            }],
            required_confirmations: Vec::new(),
            suggested_verification: Vec::new(),
            apply_command_available: false,
            would_write: false,
            applied: false,
        });
    };

    let manifest_path = resolved_dir.join("manifest.json");
    let patch_path = resolved_dir.join("promotion.patch");
    let manifest_text = std::fs::read_to_string(&manifest_path).ok();
    let manifest = manifest_text
        .as_deref()
        .and_then(|text| serde_json::from_str::<JsonValue>(text).ok())
        .unwrap_or(JsonValue::Null);
    let patch_text = std::fs::read_to_string(&patch_path).ok();
    let proposed_content_path = nested_string(&manifest, &["proposedContentPath"]);
    let proposed_content_sha256_manifest = nested_string(&manifest, &["proposedContentSha256"]);
    let proposed_content_file = proposed_content_path.as_deref().and_then(|raw| {
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            project_root.join(raw)
        };
        let candidate = candidate.canonicalize().ok()?;
        (candidate.starts_with(&resolved_dir) && candidate.is_file()).then_some(candidate)
    });
    let proposed_content_sha256_actual = proposed_content_file
        .as_ref()
        .and_then(|path| sha256_file(path));
    let proposed_content_hash_matches = proposed_content_sha256_actual.is_some()
        && proposed_content_sha256_actual == proposed_content_sha256_manifest;
    let proposed_target_path = nested_string(&manifest, &["proposedTargetPath"])
        .or_else(|| nested_string(&manifest, &["preview", "proposedTargetPath"]));
    let kind = nested_string(&manifest, &["preview", "kind"]);
    let companion_drafts = nested_string_array(&manifest, &["preview", "companionDrafts"]);
    let companion_payloads = companion_payloads_from_manifest(&manifest);
    let mut checks = Vec::new();
    readiness_check(
        &mut checks,
        "manifest_readable",
        "Artifact manifest is readable",
        manifest_text.is_some() && manifest != JsonValue::Null,
        true,
        if manifest_text.is_some() && manifest != JsonValue::Null {
            "manifest.json parsed successfully"
        } else {
            "manifest.json is missing or invalid JSON"
        },
    );
    readiness_check(
        &mut checks,
        "patch_readable",
        "Promotion patch is readable",
        patch_text
            .as_deref()
            .is_some_and(|text| text.contains("PROMOTION PATCH DRY-RUN")),
        true,
        if patch_text
            .as_deref()
            .is_some_and(|text| text.contains("PROMOTION PATCH DRY-RUN"))
        {
            "promotion.patch is present and still marked as dry-run evidence"
        } else {
            "promotion.patch is missing or does not look like dry-run evidence"
        },
    );
    readiness_check(
        &mut checks,
        "proposed_content_readable",
        "Immutable proposed content payload is readable",
        proposed_content_hash_matches,
        true,
        match (
            proposed_content_file.as_ref(),
            proposed_content_sha256_actual.as_deref(),
            proposed_content_sha256_manifest.as_deref(),
        ) {
            (Some(_), Some(actual), Some(expected)) if actual == expected => {
                "proposed-target.content is present and matches manifest sha256".to_string()
            }
            (Some(_), Some(actual), Some(expected)) => {
                format!("proposed content sha256 mismatch: actual `{actual}` expected `{expected}`")
            }
            (Some(_), Some(_), None) => {
                "manifest does not record proposedContentSha256".to_string()
            }
            _ => "proposed-target.content is missing or outside the artifact directory".to_string(),
        },
    );

    let target_path_result = proposed_target_path
        .as_deref()
        .map(|target| validate_explicit_target_path(&project_root, target));
    let target_path = target_path_result
        .as_ref()
        .and_then(|result| result.as_ref().ok());
    let target_exists = target_path.is_some_and(|target| target.is_file());
    let target_current_sha256 = target_path
        .filter(|target| target.is_file())
        .and_then(|target| sha256_file(target));
    let patch_sha256 = patch_text.as_deref().map(sha256_text);
    readiness_check(
        &mut checks,
        "target_path_safe",
        "Proposed target stays inside project and outside draft storage",
        target_path_result
            .as_ref()
            .is_some_and(|result| result.is_ok()),
        true,
        match &target_path_result {
            Some(Ok(_)) => "target path passed project boundary validation".to_string(),
            Some(Err(err)) => format!("{err}"),
            None => "manifest does not name a proposed target path".to_string(),
        },
    );
    let target_is_review_path =
        target_path.is_some_and(|target| target_is_review_holding_path(&project_root, target));
    readiness_check(
        &mut checks,
        "target_not_review_holding_path",
        "Proposed target is not the inert review holding path",
        !target_is_review_path && target_path.is_some(),
        true,
        if target_is_review_path {
            "target points under .omiga/review/self-evolution-promotions; save a new artifact with an explicit active project target before applying".to_string()
        } else if target_path.is_some() {
            "target is outside the inert review holding area".to_string()
        } else {
            "target could not be validated".to_string()
        },
    );
    let suffix_ok = match (
        expected_target_suffix_for_kind(kind.as_deref()),
        proposed_target_path.as_deref(),
    ) {
        (Some(suffix), Some(target)) => target.ends_with(suffix),
        (None, Some(_)) => false,
        _ => false,
    };
    readiness_check(
        &mut checks,
        "target_suffix_matches_kind",
        "Target filename matches candidate kind",
        suffix_ok,
        true,
        match (
            expected_target_suffix_for_kind(kind.as_deref()),
            proposed_target_path.as_deref(),
        ) {
            (Some(suffix), Some(target)) if target.ends_with(suffix) => {
                format!("target `{target}` matches expected suffix `{suffix}`")
            }
            (Some(suffix), Some(target)) => {
                format!("target `{target}` does not end with expected suffix `{suffix}`")
            }
            _ => "candidate kind or target path is missing".to_string(),
        },
    );
    readiness_check(
        &mut checks,
        "target_exists_reviewed",
        "Existing target state is visible",
        true,
        false,
        if target_exists {
            "target exists; replacement/merge must be reviewed manually"
        } else {
            "target does not exist; creation path must still be reviewed manually"
        },
    );
    readiness_check(
        &mut checks,
        "companion_draft_handling_visible",
        "Companion draft handling is visible",
        true,
        false,
        if companion_drafts.is_empty() {
            "artifact preview did not record companion draft files".to_string()
        } else {
            format!(
                "{} companion draft file(s) must be reviewed and placed separately; single-file apply only writes the manifest payload",
                companion_drafts.len()
            )
        },
    );
    let mut companion_payload_failures = Vec::new();
    for payload in &companion_payloads {
        let payload_path = if Path::new(&payload.artifact_path).is_absolute() {
            PathBuf::from(&payload.artifact_path)
        } else {
            project_root.join(&payload.artifact_path)
        };
        match payload_path.canonicalize() {
            Ok(canonical) if canonical.starts_with(&resolved_dir) && canonical.is_file() => {
                match sha256_file(&canonical) {
                    Some(actual) if actual == payload.sha256 => {}
                    Some(actual) => companion_payload_failures.push(format!(
                        "`{}` sha256 mismatch: actual `{actual}` expected `{}`",
                        payload.artifact_path, payload.sha256
                    )),
                    None => companion_payload_failures
                        .push(format!("`{}` could not be hashed", payload.artifact_path)),
                }
            }
            _ => companion_payload_failures.push(format!(
                "`{}` is missing or outside the promotion artifact",
                payload.artifact_path
            )),
        }
    }
    let companion_payloads_verified = if companion_drafts.is_empty() {
        true
    } else {
        !companion_payloads.is_empty()
            && companion_payloads.len() == companion_drafts.len()
            && companion_payload_failures.is_empty()
    };
    readiness_check(
        &mut checks,
        "companion_payloads_verified",
        "Companion payload artifacts are immutable and hash-verified",
        companion_payloads_verified,
        !companion_drafts.is_empty(),
        if companion_drafts.is_empty() {
            "no companion drafts were recorded by the preview".to_string()
        } else if companion_payloads_verified {
            format!(
                "{} companion payload file(s) are present and match recorded sha256 values",
                companion_payloads.len()
            )
        } else if companion_payloads.is_empty() {
            "companion drafts were recorded, but no companion payload artifacts were saved; resave the promotion artifact".to_string()
        } else if companion_payloads.len() != companion_drafts.len() {
            format!(
                "companion payload count mismatch: {} payload(s) for {} draft(s)",
                companion_payloads.len(),
                companion_drafts.len()
            )
        } else {
            companion_payload_failures.join("; ")
        },
    );

    let blocked = checks
        .iter()
        .any(|check| check.required && check.status != "passed");
    let status = if blocked {
        "blocked"
    } else {
        "ready_for_explicit_apply_review"
    };
    let candidate_id = nested_string(&manifest, &["preview", "candidateId"]);
    let proposed_content_sha256 = proposed_content_sha256_actual
        .clone()
        .or(proposed_content_sha256_manifest);
    let mut required_confirmations = vec![
        format!(
            "Type candidate id exactly: {}",
            candidate_id
                .clone()
                .unwrap_or_else(|| "<missing>".to_string())
        ),
        "Type the proposed target path exactly before any future apply command.".to_string(),
        format!(
            "Type proposed content sha256 exactly: {}",
            proposed_content_sha256
                .clone()
                .unwrap_or_else(|| "<missing>".to_string())
        ),
        "For existing targets, type the current target sha256 exactly before applying.".to_string(),
        "Confirm deterministic tests/fixtures were added or updated and have passed.".to_string(),
        "Confirm this will be applied in a separate reviewed branch/commit.".to_string(),
    ];
    if !companion_drafts.is_empty() {
        required_confirmations.push(
            "Confirm companion draft files were reviewed, moved/merged separately, or intentionally deferred before single-file apply.".to_string(),
        );
    }
    let mut suggested_verification = suggested_promotion_verification(kind.as_deref());
    if !companion_drafts.is_empty() {
        suggested_verification.push(
            "Verify companion scripts, fixtures, examples, and template entries exist beside the promoted manifest before relying on the unit.".to_string(),
        );
    }
    Ok(SelfEvolutionDraftPromotionApplyPlanResponse {
        status: status.to_string(),
        safety_note: "Apply readiness plan only. This command never writes the proposed target, applies patches, registers units, changes defaults, or mutates archives.".to_string(),
        artifact_dir: project_relative_path(&project_root, &resolved_dir),
        patch_path: patch_path
            .is_file()
            .then(|| project_relative_path(&project_root, &patch_path)),
        manifest_path: manifest_path
            .is_file()
            .then(|| project_relative_path(&project_root, &manifest_path)),
        proposed_content_path: proposed_content_file
            .as_ref()
            .map(|path| project_relative_path(&project_root, path)),
        proposed_target_path,
        candidate_id: candidate_id.clone(),
        kind: kind.clone(),
        title: nested_string(&manifest, &["preview", "title"]),
        patch_sha256,
        proposed_content_sha256: proposed_content_sha256.clone(),
        target_exists,
        target_current_sha256,
        companion_drafts,
        companion_payloads,
        checks,
        required_confirmations,
        suggested_verification,
        apply_command_available: !blocked,
        would_write: false,
        applied: false,
    })
}

#[tauri::command]
pub async fn save_self_evolution_draft_promotion_apply_plan(
    project_root: Option<String>,
    artifact_dir: String,
) -> CommandResult<SelfEvolutionDraftPromotionApplyPlanArtifactResponse> {
    let project_root_path = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }
    let Some(resolved_dir) = resolve_promotion_artifact_dir(&project_root_path, &artifact_dir_raw)?
    else {
        return Err(AppError::Config(
            "promotion review artifact directory was not found".to_string(),
        ));
    };
    let artifact_dir_rel = project_relative_path(&project_root_path, &resolved_dir);
    let plan = plan_self_evolution_draft_promotion_apply(
        Some(project_root_path.to_string_lossy().into_owned()),
        artifact_dir_rel.clone(),
    )
    .await?;
    let plan_json_path = resolved_dir.join("apply-readiness.json");
    let plan_readme_path = resolved_dir.join("APPLY_READINESS.md");
    let plan_json_path_rel = project_relative_path(&project_root_path, &plan_json_path);
    let plan_readme_path_rel = project_relative_path(&project_root_path, &plan_readme_path);
    let payload = serde_json::json!({
        "status": "apply_readiness_saved",
        "safetyNote": "Apply readiness review artifact only. This command writes only inert readiness evidence under the saved promotion artifact and never writes the proposed target, applies patches, registers units, changes defaults, or mutates archives.",
        "artifactDir": artifact_dir_rel,
        "planJsonPath": plan_json_path_rel,
        "planReadmePath": plan_readme_path_rel,
        "wouldWrite": false,
        "applied": false,
        "plan": &plan,
    });
    let payload_text = serde_json::to_string_pretty(&payload).map_err(|err| {
        AppError::Config(format!(
            "serialize promotion apply readiness plan failed: {err}"
        ))
    })?;
    std::fs::write(&plan_json_path, payload_text)
        .map_err(|err| AppError::Config(format!("write apply readiness JSON failed: {err}")))?;
    let readme = render_promotion_apply_plan_readme(&plan, &plan_json_path_rel);
    std::fs::write(&plan_readme_path, readme)
        .map_err(|err| AppError::Config(format!("write apply readiness README failed: {err}")))?;

    Ok(SelfEvolutionDraftPromotionApplyPlanArtifactResponse {
        status: "apply_readiness_saved".to_string(),
        safety_note: "Apply readiness review artifact only. This command writes only inert readiness evidence under the saved promotion artifact and never writes the proposed target, applies patches, registers units, changes defaults, or mutates archives.".to_string(),
        artifact_dir: artifact_dir_rel,
        plan_json_path: plan_json_path_rel,
        plan_readme_path: plan_readme_path_rel,
        plan,
        would_write: false,
        applied: false,
    })
}

#[tauri::command]
pub async fn plan_self_evolution_draft_multi_file_promotion(
    project_root: Option<String>,
    artifact_dir: String,
    companion_targets: Option<Vec<SelfEvolutionDraftPromotionCompanionTargetInput>>,
) -> CommandResult<SelfEvolutionDraftPromotionMultiFilePlanResponse> {
    let project_root = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }
    let Some(resolved_dir) = resolve_promotion_artifact_dir(&project_root, &artifact_dir_raw)?
    else {
        return Ok(SelfEvolutionDraftPromotionMultiFilePlanResponse {
            status: "missing".to_string(),
            safety_note: "Multi-file promotion plan only. This command never writes active targets and never applies companion files.".to_string(),
            artifact_dir: artifact_dir_raw,
            manifest_target_path: None,
            companion_targets: Vec::new(),
            checks: vec![SelfEvolutionDraftPromotionReadinessCheck {
                id: "artifact_exists".to_string(),
                label: "Promotion artifact directory exists".to_string(),
                status: "blocked".to_string(),
                required: true,
                detail: "Promotion review artifact directory was not found.".to_string(),
            }],
            required_review_steps: Vec::new(),
            suggested_verification: Vec::new(),
            apply_command_available: false,
            would_write: false,
            applied: false,
        });
    };

    let manifest_path = resolved_dir.join("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path).ok();
    let manifest = manifest_text
        .as_deref()
        .and_then(|text| serde_json::from_str::<JsonValue>(text).ok())
        .unwrap_or(JsonValue::Null);
    let manifest_target_path = nested_string(&manifest, &["proposedTargetPath"])
        .or_else(|| nested_string(&manifest, &["preview", "proposedTargetPath"]));
    let companion_payloads = companion_payloads_from_manifest(&manifest);
    let target_inputs = companion_targets.unwrap_or_default();
    let target_for_payload = |payload: &SelfEvolutionDraftPromotionCompanionPayload| {
        target_inputs
            .iter()
            .find(|input| input.artifact_path == payload.artifact_path)
            .map(|input| input.target_path.trim().to_string())
            .filter(|target| !target.is_empty())
    };

    let mut planned_targets = Vec::new();
    let mut resolved_target_paths = Vec::<String>::new();
    for payload in &companion_payloads {
        let mut target_checks = Vec::new();
        let payload_path = if Path::new(&payload.artifact_path).is_absolute() {
            PathBuf::from(&payload.artifact_path)
        } else {
            project_root.join(&payload.artifact_path)
        };
        let payload_canonical = payload_path.canonicalize().ok();
        let payload_hash = payload_canonical
            .as_ref()
            .filter(|path| path.starts_with(&resolved_dir) && path.is_file())
            .and_then(|path| sha256_file(path));
        let payload_verified = payload_hash.as_deref() == Some(payload.sha256.as_str());
        readiness_check(
            &mut target_checks,
            "companion_payload_verified",
            "Companion payload is present and hash-verified",
            payload_verified,
            true,
            if payload_verified {
                "companion payload exists inside the artifact and matches recorded sha256"
                    .to_string()
            } else {
                "companion payload is missing, outside the artifact, or sha256 mismatched"
                    .to_string()
            },
        );

        let requested_target = target_for_payload(payload);
        readiness_check(
            &mut target_checks,
            "companion_target_provided",
            "Companion target path is provided",
            requested_target.is_some(),
            true,
            if requested_target.is_some() {
                "reviewer provided an explicit active project target path".to_string()
            } else {
                "provide a project-relative target path for this companion payload".to_string()
            },
        );

        let target_path_result = requested_target
            .as_deref()
            .map(|target| validate_explicit_target_path(&project_root, target));
        let target_path = target_path_result
            .as_ref()
            .and_then(|result| result.as_ref().ok());
        let proposed_target_path =
            target_path.map(|path| project_relative_path(&project_root, path));
        readiness_check(
            &mut target_checks,
            "companion_target_path_safe",
            "Companion target stays inside project and outside draft storage",
            target_path_result
                .as_ref()
                .is_some_and(|result| result.is_ok()),
            true,
            match &target_path_result {
                Some(Ok(_)) => "target path passed project boundary validation".to_string(),
                Some(Err(err)) => format!("{err}"),
                None => "target path was not provided".to_string(),
            },
        );
        let companion_target_is_review_path =
            target_path.is_some_and(|target| target_is_review_holding_path(&project_root, target));
        readiness_check(
            &mut target_checks,
            "companion_target_not_review_holding_path",
            "Companion target is not inside the inert review holding path",
            target_path.is_some() && !companion_target_is_review_path,
            true,
            if companion_target_is_review_path {
                "companion target points under .omiga/review/self-evolution-promotions; choose an active plugin/project path".to_string()
            } else if target_path.is_some() {
                "companion target is outside the inert review holding area".to_string()
            } else {
                "companion target could not be validated".to_string()
            },
        );
        readiness_check(
            &mut target_checks,
            "companion_target_not_manifest_target",
            "Companion target does not overwrite the manifest target",
            proposed_target_path.as_deref().is_some_and(|target| {
                Some(target) != manifest_target_path.as_deref()
            }),
            true,
            match (proposed_target_path.as_deref(), manifest_target_path.as_deref()) {
                (Some(target), Some(manifest_target)) if target != manifest_target => {
                    "companion target is distinct from the manifest target".to_string()
                }
                (Some(_), Some(_)) => {
                    "companion target equals the manifest target; choose a script/fixture/example path".to_string()
                }
                (Some(_), None) => "manifest target is missing from the artifact".to_string(),
                _ => "companion target is missing".to_string(),
            },
        );

        let target_exists = target_path.is_some_and(|path| path.is_file());
        let target_current_sha256 = target_path
            .filter(|path| path.is_file())
            .and_then(|path| sha256_file(path));
        if let Some(target) = proposed_target_path.as_ref() {
            resolved_target_paths.push(target.clone());
        }
        let payload_text = payload_canonical
            .as_ref()
            .and_then(|path| read_text_lossy(path, MAX_TEXT_PREVIEW_BYTES))
            .map(|(text, _, _)| text);
        let existing_text = target_path
            .filter(|path| path.is_file())
            .and_then(|path| read_text_lossy(path, MAX_TEXT_PREVIEW_BYTES))
            .map(|(text, _, _)| text);
        let diff_preview = proposed_target_path.as_ref().and_then(|target| {
            payload_text.as_ref().map(|payload_text| {
                promotion_diff_preview(target, existing_text.as_deref(), payload_text)
            })
        });

        planned_targets.push(SelfEvolutionDraftPromotionCompanionTargetPlan {
            source_path: payload.source_path.clone(),
            artifact_path: payload.artifact_path.clone(),
            role: payload.role.clone(),
            bytes: payload.bytes,
            sha256: payload.sha256.clone(),
            proposed_target_path,
            target_exists,
            target_current_sha256,
            diff_preview,
            checks: target_checks,
        });
    }

    let mut checks = Vec::new();
    readiness_check(
        &mut checks,
        "manifest_readable",
        "Promotion artifact manifest is readable",
        manifest_text.is_some() && manifest != JsonValue::Null,
        true,
        if manifest_text.is_some() && manifest != JsonValue::Null {
            "manifest.json parsed successfully"
        } else {
            "manifest.json is missing or invalid JSON"
        },
    );
    readiness_check(
        &mut checks,
        "companion_payloads_present",
        "Companion payloads are present for multi-file planning",
        !companion_payloads.is_empty(),
        true,
        if companion_payloads.is_empty() {
            "no companion payloads were recorded; save a new promotion artifact from a creator package first".to_string()
        } else {
            format!(
                "{} companion payload(s) are available",
                companion_payloads.len()
            )
        },
    );
    let duplicate_targets = resolved_target_paths
        .iter()
        .enumerate()
        .any(|(index, target)| {
            resolved_target_paths
                .iter()
                .skip(index + 1)
                .any(|other| other == target)
        });
    readiness_check(
        &mut checks,
        "companion_targets_unique",
        "Companion target paths are unique",
        !resolved_target_paths.is_empty()
            && resolved_target_paths.len() == companion_payloads.len()
            && !duplicate_targets,
        true,
        if duplicate_targets {
            "two or more companion payloads point at the same target path".to_string()
        } else if resolved_target_paths.len() == companion_payloads.len() {
            "all companion payloads have unique explicit targets".to_string()
        } else {
            "one or more companion payloads are missing explicit target paths".to_string()
        },
    );

    let blocked = checks
        .iter()
        .chain(
            planned_targets
                .iter()
                .flat_map(|target| target.checks.iter()),
        )
        .any(|check| check.required && check.status != "passed");
    let status = if blocked {
        "blocked"
    } else {
        "ready_for_reviewed_multi_file_patch"
    };
    Ok(SelfEvolutionDraftPromotionMultiFilePlanResponse {
        status: status.to_string(),
        safety_note: "Multi-file promotion plan only. This command computes explicit companion target review evidence and never writes active targets, applies patches, registers units, changes defaults, or mutates archives.".to_string(),
        artifact_dir: project_relative_path(&project_root, &resolved_dir),
        manifest_target_path,
        companion_targets: planned_targets,
        checks,
        required_review_steps: vec![
            "Review the manifest promotion artifact and each companion target diff together.".to_string(),
            "Move or merge companion payloads only through a separate explicit reviewed patch.".to_string(),
            "Run unit_authoring_validate plus deterministic smoke tests after the reviewed multi-file patch is applied.".to_string(),
        ],
        suggested_verification: vec![
            "Confirm companion scripts, fixtures, examples, and template entries are located beside the promoted manifest as expected.".to_string(),
            "Run the promoted Operator/Template in offline fixture mode before any default or registry change.".to_string(),
        ],
        apply_command_available: false,
        would_write: false,
        applied: false,
    })
}

#[tauri::command]
pub async fn save_self_evolution_draft_multi_file_promotion_plan(
    project_root: Option<String>,
    artifact_dir: String,
    companion_targets: Option<Vec<SelfEvolutionDraftPromotionCompanionTargetInput>>,
) -> CommandResult<SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse> {
    let project_root_path = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }
    let Some(resolved_dir) = resolve_promotion_artifact_dir(&project_root_path, &artifact_dir_raw)?
    else {
        return Err(AppError::Config(
            "promotion review artifact directory was not found".to_string(),
        ));
    };
    let artifact_dir_rel = project_relative_path(&project_root_path, &resolved_dir);
    let plan = plan_self_evolution_draft_multi_file_promotion(
        Some(project_root_path.to_string_lossy().into_owned()),
        artifact_dir_rel.clone(),
        companion_targets,
    )
    .await?;
    let plan_json_path = resolved_dir.join("multi-file-promotion-plan.json");
    let plan_readme_path = resolved_dir.join("MULTI_FILE_PROMOTION_PLAN.md");
    let plan_json_path_rel = project_relative_path(&project_root_path, &plan_json_path);
    let plan_readme_path_rel = project_relative_path(&project_root_path, &plan_readme_path);
    let payload = serde_json::json!({
        "status": "multi_file_plan_saved",
        "safetyNote": "Multi-file promotion plan review artifact only. This command writes only inert plan evidence and never writes active targets or applies companion files.",
        "artifactDir": artifact_dir_rel,
        "planJsonPath": plan_json_path_rel,
        "planReadmePath": plan_readme_path_rel,
        "wouldWrite": false,
        "applied": false,
        "plan": &plan,
    });
    let payload_text = serde_json::to_string_pretty(&payload).map_err(|err| {
        AppError::Config(format!("serialize multi-file promotion plan failed: {err}"))
    })?;
    std::fs::write(&plan_json_path, payload_text)
        .map_err(|err| AppError::Config(format!("write multi-file plan JSON failed: {err}")))?;
    let readme = render_multi_file_promotion_plan_readme(&plan, &plan_json_path_rel);
    std::fs::write(&plan_readme_path, readme)
        .map_err(|err| AppError::Config(format!("write multi-file plan README failed: {err}")))?;

    Ok(SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse {
        status: "multi_file_plan_saved".to_string(),
        safety_note: "Multi-file promotion plan review artifact only. This command writes only inert plan evidence and never writes active targets or applies companion files.".to_string(),
        artifact_dir: artifact_dir_rel,
        plan_json_path: plan_json_path_rel,
        plan_readme_path: plan_readme_path_rel,
        plan,
        would_write: false,
        applied: false,
    })
}

#[tauri::command]
pub async fn validate_self_evolution_draft_promotion_apply_request(
    project_root: Option<String>,
    artifact_dir: String,
    candidate_id_confirmation: Option<String>,
    target_path_confirmation: Option<String>,
    tests_confirmed: Option<bool>,
    reviewed_branch_confirmed: Option<bool>,
    companion_files_confirmed: Option<bool>,
) -> CommandResult<SelfEvolutionDraftPromotionApplyRequestResponse> {
    let project_root_path = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }
    let plan = plan_self_evolution_draft_promotion_apply(
        Some(project_root_path.to_string_lossy().into_owned()),
        artifact_dir_raw,
    )
    .await?;
    let mut checks = plan.checks.clone();

    readiness_check(
        &mut checks,
        "readiness_gate_passed",
        "Readiness gate is fully passed",
        plan.status == "ready_for_explicit_apply_review",
        true,
        if plan.status == "ready_for_explicit_apply_review" {
            "saved artifact passed all required readiness checks".to_string()
        } else {
            format!("readiness status is `{}`", plan.status)
        },
    );

    let expected_candidate = plan.candidate_id.as_deref().unwrap_or("");
    let actual_candidate = candidate_id_confirmation
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    readiness_check(
        &mut checks,
        "candidate_id_confirmation_exact",
        "Candidate id confirmation is exact",
        !expected_candidate.is_empty() && actual_candidate == expected_candidate,
        true,
        if expected_candidate.is_empty() {
            "candidate id is missing from the saved artifact".to_string()
        } else if actual_candidate == expected_candidate {
            "candidate id was typed exactly".to_string()
        } else {
            "candidate id confirmation did not match exactly".to_string()
        },
    );

    let expected_target = plan.proposed_target_path.as_deref().unwrap_or("");
    let actual_target = target_path_confirmation
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    readiness_check(
        &mut checks,
        "target_path_confirmation_exact",
        "Target path confirmation is exact",
        !expected_target.is_empty() && actual_target == expected_target,
        true,
        if expected_target.is_empty() {
            "proposed target path is missing from the saved artifact".to_string()
        } else if actual_target == expected_target {
            "target path was typed exactly".to_string()
        } else {
            "target path confirmation did not match exactly".to_string()
        },
    );

    readiness_check(
        &mut checks,
        "deterministic_tests_confirmed",
        "Deterministic tests or fixtures are confirmed",
        tests_confirmed.unwrap_or(false),
        true,
        if tests_confirmed.unwrap_or(false) {
            "reviewer confirmed deterministic tests/fixtures passed".to_string()
        } else {
            "reviewer has not confirmed deterministic tests/fixtures".to_string()
        },
    );

    readiness_check(
        &mut checks,
        "reviewed_branch_confirmed",
        "Separate reviewed branch or commit is confirmed",
        reviewed_branch_confirmed.unwrap_or(false),
        true,
        if reviewed_branch_confirmed.unwrap_or(false) {
            "reviewer confirmed this would be applied in a separate reviewed branch/commit"
                .to_string()
        } else {
            "reviewer has not confirmed a separate reviewed branch/commit".to_string()
        },
    );

    if !plan.companion_drafts.is_empty() {
        readiness_check(
            &mut checks,
            "companion_files_confirmed",
            "Companion draft files were handled",
            companion_files_confirmed.unwrap_or(false),
            true,
            if companion_files_confirmed.unwrap_or(false) {
                "reviewer confirmed companion drafts were reviewed, moved/merged separately, or intentionally deferred".to_string()
            } else {
                "reviewer has not confirmed companion draft handling; single-file apply writes only the manifest payload".to_string()
            },
        );
    }

    let blocked = checks
        .iter()
        .any(|check| check.required && check.status != "passed");
    let status = if blocked {
        "blocked"
    } else {
        "ready_for_explicit_apply"
    };

    Ok(SelfEvolutionDraftPromotionApplyRequestResponse {
        status: status.to_string(),
        safety_note: "Apply request validation only. This command checks explicit confirmations and never writes the proposed target, applies patches, registers units, changes defaults, or mutates archives.".to_string(),
        artifact_dir: plan.artifact_dir.clone(),
        proposed_target_path: plan.proposed_target_path.clone(),
        candidate_id: plan.candidate_id.clone(),
        kind: plan.kind.clone(),
        title: plan.title.clone(),
        patch_sha256: plan.patch_sha256.clone(),
        proposed_content_sha256: plan.proposed_content_sha256.clone(),
        target_exists: plan.target_exists,
        target_current_sha256: plan.target_current_sha256.clone(),
        companion_drafts: plan.companion_drafts.clone(),
        checks,
        required_confirmations: plan.required_confirmations,
        suggested_verification: plan.suggested_verification,
        apply_command_available: !blocked && plan.apply_command_available,
        would_write: false,
        applied: false,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
// Tauri invoke callers pass these confirmations as flat command arguments.
pub async fn apply_self_evolution_draft_promotion(
    project_root: Option<String>,
    artifact_dir: String,
    candidate_id_confirmation: Option<String>,
    target_path_confirmation: Option<String>,
    proposed_content_sha256_confirmation: Option<String>,
    target_current_sha256_confirmation: Option<String>,
    tests_confirmed: Option<bool>,
    reviewed_branch_confirmed: Option<bool>,
    companion_files_confirmed: Option<bool>,
) -> CommandResult<SelfEvolutionDraftPromotionApplyResponse> {
    let project_root_path = resolve_project_root(project_root);
    let artifact_dir_raw = artifact_dir.trim().to_string();
    if artifact_dir_raw.is_empty() {
        return Err(AppError::Config(
            "artifactDir must not be empty".to_string(),
        ));
    }

    let request = validate_self_evolution_draft_promotion_apply_request(
        Some(project_root_path.to_string_lossy().into_owned()),
        artifact_dir_raw.clone(),
        candidate_id_confirmation,
        target_path_confirmation,
        tests_confirmed,
        reviewed_branch_confirmed,
        companion_files_confirmed,
    )
    .await?;
    let plan = plan_self_evolution_draft_promotion_apply(
        Some(project_root_path.to_string_lossy().into_owned()),
        artifact_dir_raw,
    )
    .await?;
    let mut checks = request.checks.clone();

    let expected_content_sha = plan.proposed_content_sha256.as_deref().unwrap_or("");
    let actual_content_sha_confirmation = proposed_content_sha256_confirmation
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    readiness_check(
        &mut checks,
        "proposed_content_sha256_confirmation_exact",
        "Proposed content sha256 confirmation is exact",
        !expected_content_sha.is_empty() && actual_content_sha_confirmation == expected_content_sha,
        true,
        if expected_content_sha.is_empty() {
            "proposed content sha256 is missing from the saved artifact".to_string()
        } else if actual_content_sha_confirmation == expected_content_sha {
            "proposed content sha256 was typed exactly".to_string()
        } else {
            "proposed content sha256 confirmation did not match exactly".to_string()
        },
    );

    let target_hash_confirmation = target_current_sha256_confirmation
        .as_deref()
        .map(str::trim)
        .unwrap_or("");
    let target_hash_matches = if plan.target_exists {
        plan.target_current_sha256
            .as_deref()
            .is_some_and(|hash| hash == target_hash_confirmation)
    } else {
        true
    };
    readiness_check(
        &mut checks,
        "target_current_sha256_confirmation_exact",
        "Current target sha256 confirmation is exact when replacing",
        target_hash_matches,
        true,
        if plan.target_exists {
            match plan.target_current_sha256.as_deref() {
                Some(hash) if hash == target_hash_confirmation => {
                    "current target sha256 was typed exactly".to_string()
                }
                Some(_) => "current target sha256 confirmation did not match exactly".to_string(),
                None => "current target sha256 could not be computed".to_string(),
            }
        } else {
            "target does not exist; no current target sha256 is required".to_string()
        },
    );

    let Some(resolved_dir) =
        resolve_promotion_artifact_dir(&project_root_path, &plan.artifact_dir)?
    else {
        return Err(AppError::Config(
            "promotion review artifact directory was not found".to_string(),
        ));
    };
    let proposed_content_file = plan.proposed_content_path.as_deref().and_then(|raw| {
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            project_root_path.join(raw)
        };
        let candidate = candidate.canonicalize().ok()?;
        (candidate.starts_with(&resolved_dir) && candidate.is_file()).then_some(candidate)
    });
    let mut payload_error = None;
    let proposed_content = proposed_content_file.as_ref().and_then(|path| {
        match read_text_strict(path, MAX_PROMOTION_CONTENT_BYTES) {
            Ok(text) => Some(text),
            Err(err) => {
                payload_error = Some(format!("{err}"));
                None
            }
        }
    });
    let payload_sha256 = proposed_content.as_deref().map(sha256_text);
    readiness_check(
        &mut checks,
        "proposed_content_payload_reverified",
        "Immutable proposed content payload is reverified immediately before write",
        payload_sha256.as_deref() == plan.proposed_content_sha256.as_deref()
            && payload_sha256.is_some(),
        true,
        match (
            proposed_content_file.as_ref(),
            payload_sha256.as_deref(),
            plan.proposed_content_sha256.as_deref(),
            payload_error.as_deref(),
        ) {
            (_, _, _, Some(err)) => format!("proposed content payload could not be read: {err}"),
            (Some(_), Some(actual), Some(expected), None) if actual == expected => {
                "proposed-target.content was re-read and matches saved sha256".to_string()
            }
            (Some(_), Some(actual), Some(expected), None) => {
                format!("proposed content sha256 mismatch before apply: actual `{actual}` expected `{expected}`")
            }
            _ => "proposed-target.content is missing or outside the artifact directory".to_string(),
        },
    );

    let target_path_for_recheck = plan
        .proposed_target_path
        .as_deref()
        .and_then(|target| validate_explicit_target_path(&project_root_path, target).ok());
    let immediate_target_sha256 = target_path_for_recheck
        .as_ref()
        .filter(|target| target.is_file())
        .and_then(|target| sha256_file(target));
    let target_state_still_matches = if plan.target_exists {
        immediate_target_sha256 == plan.target_current_sha256
    } else {
        target_path_for_recheck
            .as_ref()
            .is_some_and(|target| !target.exists())
    };
    readiness_check(
        &mut checks,
        "target_current_sha256_reverified",
        "Target current sha256 is reverified immediately before write",
        target_state_still_matches,
        true,
        if plan.target_exists {
            match (
                immediate_target_sha256.as_deref(),
                plan.target_current_sha256.as_deref(),
            ) {
                (Some(actual), Some(expected)) if actual == expected => {
                    "current target sha256 still matches the readiness plan".to_string()
                }
                (Some(actual), Some(expected)) => {
                    format!(
                        "current target sha256 changed: actual `{actual}` expected `{expected}`"
                    )
                }
                _ => "current target sha256 could not be reverified".to_string(),
            }
        } else if target_state_still_matches {
            "target still does not exist immediately before write".to_string()
        } else {
            "target appeared after readiness planning; re-run review before applying".to_string()
        },
    );

    let blocked = checks
        .iter()
        .any(|check| check.required && check.status != "passed");
    if blocked {
        return Ok(SelfEvolutionDraftPromotionApplyResponse {
            status: "blocked".to_string(),
            safety_note: "Explicit promotion apply was blocked before any target write. No file was modified, no unit was registered, and no defaults or archives were changed.".to_string(),
            artifact_dir: plan.artifact_dir.clone(),
            proposed_content_path: plan.proposed_content_path.clone(),
            proposed_target_path: plan.proposed_target_path.clone(),
            candidate_id: plan.candidate_id.clone(),
            kind: plan.kind.clone(),
            title: plan.title.clone(),
            proposed_content_sha256: plan.proposed_content_sha256.clone(),
            target_exists_before: plan.target_exists,
            target_previous_sha256: plan.target_current_sha256.clone(),
            target_new_sha256: None,
            companion_drafts: plan.companion_drafts.clone(),
            bytes_written: 0,
            checks,
            suggested_verification: plan.suggested_verification.clone(),
            apply_command_available: plan.apply_command_available,
            would_write: false,
            applied: false,
        });
    }

    let target_path = target_path_for_recheck.ok_or_else(|| {
        AppError::Config("proposed target path could not be revalidated".to_string())
    })?;
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| AppError::Config(format!("create target parent failed: {err}")))?;
    }
    let proposed_content = proposed_content.ok_or_else(|| {
        AppError::Config("proposed-target.content could not be read for apply".to_string())
    })?;
    std::fs::write(&target_path, proposed_content.as_bytes())
        .map_err(|err| AppError::Config(format!("write promoted target failed: {err}")))?;
    let target_new_sha256 = sha256_file(&target_path);

    Ok(SelfEvolutionDraftPromotionApplyResponse {
        status: "applied".to_string(),
        safety_note: "Explicit promotion apply wrote exactly one confirmed target file from immutable proposed-target.content. It did not register units, update defaults, or mutate archives.".to_string(),
        artifact_dir: plan.artifact_dir.clone(),
        proposed_content_path: plan.proposed_content_path.clone(),
        proposed_target_path: plan.proposed_target_path.clone(),
        candidate_id: plan.candidate_id.clone(),
        kind: plan.kind.clone(),
        title: plan.title.clone(),
        proposed_content_sha256: plan.proposed_content_sha256.clone(),
        target_exists_before: plan.target_exists,
        target_previous_sha256: plan.target_current_sha256.clone(),
        target_new_sha256,
        companion_drafts: plan.companion_drafts.clone(),
        bytes_written: proposed_content.len() as u64,
        checks,
        suggested_verification: plan.suggested_verification.clone(),
        apply_command_available: true,
        would_write: true,
        applied: true,
    })
}

#[tauri::command]
pub async fn read_self_evolution_draft(
    project_root: Option<String>,
    draft_dir: String,
) -> CommandResult<SelfEvolutionDraftDetailResponse> {
    let project_root = resolve_project_root(project_root);
    let draft_dir_raw = draft_dir.trim().to_string();
    if draft_dir_raw.is_empty() {
        return Err(AppError::Config("draftDir must not be empty".to_string()));
    }
    let Some(resolved_dir) = resolve_draft_dir(&project_root, &draft_dir_raw)? else {
        return Ok(SelfEvolutionDraftDetailResponse {
            found: false,
            draft_dir: draft_dir_raw,
            candidate: JsonValue::Null,
            files: Vec::new(),
            review_preview: SelfEvolutionDraftReviewPreview {
                status: "missing".to_string(),
                safety_note: SAFETY_NOTE.to_string(),
                candidate_id: None,
                kind: None,
                title: None,
                target_hint: None,
                actions: vec!["Draft directory was not found.".to_string()],
                diff_preview: None,
            },
            note: SAFETY_NOTE.to_string(),
        });
    };
    let files = list_immediate_files(&resolved_dir)
        .iter()
        .filter_map(|path| read_file_preview(&project_root, path))
        .collect::<Vec<_>>();
    let candidate = files
        .iter()
        .find(|file| file.role == "candidate_json")
        .and_then(|file| file.json.clone())
        .unwrap_or(JsonValue::Null);
    let review_preview = build_review_preview(&resolved_dir, &candidate, &files);
    Ok(SelfEvolutionDraftDetailResponse {
        found: true,
        draft_dir: project_relative_path(&project_root, &resolved_dir),
        candidate,
        files,
        review_preview,
        note: SAFETY_NOTE.to_string(),
    })
}

#[tauri::command]
pub async fn preview_self_evolution_draft_promotion(
    project_root: Option<String>,
    draft_dir: String,
    target_path: Option<String>,
) -> CommandResult<SelfEvolutionDraftPromotionPreviewResponse> {
    let project_root = resolve_project_root(project_root);
    let draft_dir_raw = draft_dir.trim().to_string();
    if draft_dir_raw.is_empty() {
        return Err(AppError::Config("draftDir must not be empty".to_string()));
    }
    if target_path
        .as_deref()
        .is_some_and(|target| target.trim().is_empty())
    {
        return Err(AppError::Config("targetPath must not be empty".to_string()));
    }
    let Some(resolved_dir) = resolve_draft_dir(&project_root, &draft_dir_raw)? else {
        return Ok(SelfEvolutionDraftPromotionPreviewResponse {
            status: "missing".to_string(),
            safety_note: PROMOTION_DRY_RUN_NOTE.to_string(),
            draft_dir: draft_dir_raw,
            candidate_id: None,
            kind: None,
            title: None,
            draft_file: None,
            proposed_target_path: None,
            target_exists: false,
            diff_preview: None,
            companion_drafts: Vec::new(),
            companion_review_steps: Vec::new(),
            risk_notes: vec!["Draft directory was not found.".to_string()],
            required_review_steps: vec!["Select an existing draft directory.".to_string()],
            would_write: false,
            applied: false,
        });
    };

    let candidate = read_candidate_json(&resolved_dir);
    let draft_file = find_specialized_draft_file(&resolved_dir);
    let explicit_target = target_path
        .as_deref()
        .is_some_and(|target| !target.trim().is_empty());

    let proposed_target_path = match (&target_path, draft_file.as_ref()) {
        (Some(target), _) if !target.trim().is_empty() => {
            let target = validate_explicit_target_path(&project_root, target)?;
            Some(project_relative_path(&project_root, &target))
        }
        (_, Some(draft_file)) => default_promotion_target_for_draft(&candidate, draft_file),
        _ => None,
    };

    let target_path_for_read = if explicit_target {
        target_path
            .as_deref()
            .filter(|target| !target.trim().is_empty())
            .map(|target| validate_explicit_target_path(&project_root, target))
            .transpose()?
    } else {
        proposed_target_path
            .as_deref()
            .map(|target| normalize_path_components(&project_root.join(target)))
    };
    let target_exists = target_path_for_read
        .as_ref()
        .is_some_and(|target| target.is_file());
    let existing_text = target_path_for_read
        .as_ref()
        .filter(|target| target.is_file())
        .and_then(|target| read_text_lossy(target, MAX_TEXT_PREVIEW_BYTES))
        .map(|(text, _, _)| text);
    let draft_text = draft_file
        .as_ref()
        .and_then(|path| read_text_lossy(path, MAX_TEXT_PREVIEW_BYTES).map(|(text, _, _)| text));
    let companion_drafts = companion_draft_files(&resolved_dir, draft_file.as_deref())
        .iter()
        .map(|path| project_relative_path(&project_root, path))
        .collect::<Vec<_>>();
    let companion_review_steps = companion_review_steps(&companion_drafts);
    let diff_preview = match (&proposed_target_path, &draft_text) {
        (Some(target), Some(draft)) => Some(promotion_diff_preview(
            target,
            existing_text.as_deref(),
            draft,
        )),
        _ => None,
    };

    Ok(SelfEvolutionDraftPromotionPreviewResponse {
        status: "dry_run".to_string(),
        safety_note: PROMOTION_DRY_RUN_NOTE.to_string(),
        draft_dir: project_relative_path(&project_root, &resolved_dir),
        candidate_id: candidate_string(&candidate, "id"),
        kind: candidate_string(&candidate, "kind"),
        title: candidate_string(&candidate, "title"),
        draft_file: draft_file
            .as_ref()
            .map(|path| project_relative_path(&project_root, path)),
        proposed_target_path,
        target_exists,
        diff_preview,
        companion_drafts: companion_drafts.clone(),
        companion_review_steps,
        risk_notes: base_promotion_risk_notes(
            &candidate,
            draft_file.as_deref(),
            explicit_target,
            target_exists,
            &companion_drafts,
        ),
        required_review_steps: required_promotion_review_steps(target_exists, &companion_drafts),
        would_write: false,
        applied: false,
    })
}

#[tauri::command]
pub async fn save_self_evolution_draft_promotion_artifact(
    project_root: Option<String>,
    draft_dir: String,
    target_path: Option<String>,
) -> CommandResult<SelfEvolutionDraftPromotionArtifactResponse> {
    let project_root_path = resolve_project_root(project_root);
    let preview = preview_self_evolution_draft_promotion(
        Some(project_root_path.to_string_lossy().into_owned()),
        draft_dir,
        target_path,
    )
    .await?;
    let Some(diff_preview) = preview.diff_preview.as_deref() else {
        return Err(AppError::Config(
            "promotion preview did not produce a diff; nothing to save".to_string(),
        ));
    };
    let Some(proposed_target_path) = preview.proposed_target_path.clone() else {
        return Err(AppError::Config(
            "promotion preview did not produce a target path; nothing to save".to_string(),
        ));
    };
    let Some(draft_file) = preview.draft_file.as_deref() else {
        return Err(AppError::Config(
            "promotion preview did not produce a draft file; nothing to save".to_string(),
        ));
    };
    let draft_file_path = resolve_draft_file_path(&project_root_path, draft_file)?;
    let proposed_content = read_text_strict(&draft_file_path, MAX_PROMOTION_CONTENT_BYTES)?;
    let proposed_content_sha256 = sha256_text(&proposed_content);

    let artifact_root = promotion_artifact_root(&project_root_path);
    std::fs::create_dir_all(&artifact_root)
        .map_err(|err| AppError::Config(format!("create promotion artifact root failed: {err}")))?;

    let base_slug = promotion_artifact_slug(&preview);
    let mut artifact_dir = artifact_root.join(&base_slug);
    let mut counter = 1usize;
    while artifact_dir.exists() {
        artifact_dir = artifact_root.join(format!("{base_slug}-{counter}"));
        counter += 1;
    }
    std::fs::create_dir_all(&artifact_dir)
        .map_err(|err| AppError::Config(format!("create promotion artifact dir failed: {err}")))?;

    let patch_path = artifact_dir.join("promotion.patch");
    let manifest_path = artifact_dir.join("manifest.json");
    let readme_path = artifact_dir.join("README.md");
    let proposed_content_path = artifact_dir.join("proposed-target.content");
    let companion_payload_dir = artifact_dir.join(COMPANION_PAYLOAD_DIR);

    std::fs::write(&patch_path, diff_preview)
        .map_err(|err| AppError::Config(format!("write promotion patch failed: {err}")))?;
    std::fs::write(&proposed_content_path, proposed_content)
        .map_err(|err| AppError::Config(format!("write proposed content failed: {err}")))?;

    let artifact_dir_rel = project_relative_path(&project_root_path, &artifact_dir);
    let mut companion_payloads = Vec::new();
    if !preview.companion_drafts.is_empty() {
        std::fs::create_dir_all(&companion_payload_dir).map_err(|err| {
            AppError::Config(format!("create companion payload dir failed: {err}"))
        })?;
        for (index, companion_path) in preview.companion_drafts.iter().enumerate() {
            let companion_source = resolve_draft_file_path(&project_root_path, companion_path)?;
            let companion_text = read_text_strict(&companion_source, MAX_PROMOTION_CONTENT_BYTES)?;
            let companion_sha256 = sha256_text(&companion_text);
            let companion_bytes = companion_text.len() as u64;
            let source_name = companion_source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("companion.draft");
            let artifact_filename = format!(
                "{:02}-{}.content",
                index + 1,
                safe_artifact_filename(source_name)
            );
            let companion_artifact_path = companion_payload_dir.join(artifact_filename);
            std::fs::write(&companion_artifact_path, companion_text).map_err(|err| {
                AppError::Config(format!("write companion payload failed: {err}"))
            })?;
            companion_payloads.push(SelfEvolutionDraftPromotionCompanionPayload {
                source_path: project_relative_path(&project_root_path, &companion_source),
                artifact_path: project_relative_path(&project_root_path, &companion_artifact_path),
                role: file_role(&companion_source),
                bytes: companion_bytes,
                sha256: companion_sha256,
            });
        }
    }
    let patch_path_rel = project_relative_path(&project_root_path, &patch_path);
    let manifest_path_rel = project_relative_path(&project_root_path, &manifest_path);
    let readme_path_rel = project_relative_path(&project_root_path, &readme_path);
    let proposed_content_path_rel =
        project_relative_path(&project_root_path, &proposed_content_path);
    let manifest = serde_json::json!({
        "status": "artifact_saved",
        "safetyNote": PROMOTION_ARTIFACT_NOTE,
        "artifactDir": artifact_dir_rel,
        "patchPath": patch_path_rel,
        "manifestPath": manifest_path_rel,
        "readmePath": readme_path_rel,
        "proposedContentPath": &proposed_content_path_rel,
        "proposedContentSha256": &proposed_content_sha256,
        "companionPayloads": &companion_payloads,
        "proposedTargetPath": &proposed_target_path,
        "wouldWrite": false,
        "applied": false,
        "preview": &preview,
    });
    let manifest_text = serde_json::to_string_pretty(&manifest)
        .map_err(|err| AppError::Config(format!("serialize promotion artifact failed: {err}")))?;
    std::fs::write(&manifest_path, manifest_text)
        .map_err(|err| AppError::Config(format!("write promotion manifest failed: {err}")))?;

    let readme = render_promotion_artifact_readme(
        &preview,
        &patch_path_rel,
        &manifest_path_rel,
        &proposed_content_path_rel,
        &proposed_content_sha256,
        &companion_payloads,
    );
    std::fs::write(&readme_path, readme)
        .map_err(|err| AppError::Config(format!("write promotion README failed: {err}")))?;

    Ok(SelfEvolutionDraftPromotionArtifactResponse {
        status: "artifact_saved".to_string(),
        safety_note: PROMOTION_ARTIFACT_NOTE.to_string(),
        artifact_dir: artifact_dir_rel,
        patch_path: patch_path_rel,
        manifest_path: manifest_path_rel,
        readme_path: readme_path_rel,
        proposed_content_path: proposed_content_path_rel,
        proposed_content_sha256,
        companion_payloads,
        proposed_target_path: Some(proposed_target_path),
        preview,
        would_write: false,
        applied: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_and_reads_self_evolution_drafts_as_review_only() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-template-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.parent().unwrap().join("README.md"),
            "# Self-Evolution Draft Batch\n\n- Generated at: `2026-05-10T00:00:00Z`\n",
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("DRAFT.md"),
            "# Self-Evolution Candidate Draft\n\n- [ ] Confirm this candidate is still relevant.\n",
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "candidate-template-demo",
                "kind": "template_candidate",
                "title": "Reusable DE workflow",
                "priority": "medium"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.yaml.draft"),
            "apiVersion: omiga.ai/template/v1alpha1\nkind: Template\n",
        )
        .unwrap();

        let list =
            list_self_evolution_drafts(Some(tmp.path().to_string_lossy().into_owned()), Some(5))
                .await
                .unwrap();
        assert_eq!(list.batch_count, 1);
        assert_eq!(list.batches[0].draft_count, 1);
        assert_eq!(
            list.batches[0].drafts[0].candidate_id,
            "candidate-template-demo"
        );
        assert_eq!(
            list.batches[0].generated_at.as_deref(),
            Some("2026-05-10T00:00:00Z")
        );

        let before = std::fs::read_to_string(draft_dir.join("template.yaml.draft")).unwrap();
        let detail = read_self_evolution_draft(
            Some(tmp.path().to_string_lossy().into_owned()),
            list.batches[0].drafts[0].draft_dir.clone(),
        )
        .await
        .unwrap();
        let after = std::fs::read_to_string(draft_dir.join("template.yaml.draft")).unwrap();
        assert_eq!(before, after);
        assert!(detail.found);
        assert_eq!(detail.candidate["kind"], "template_candidate");
        assert!(detail
            .review_preview
            .target_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("templates/<id>/template.yaml")));
        assert!(detail
            .review_preview
            .diff_preview
            .as_deref()
            .is_some_and(|diff| diff.contains("+++ review-target/template.yaml")));
    }

    #[tokio::test]
    async fn rejects_draft_detail_paths_outside_draft_root() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let err = read_self_evolution_draft(
            Some(tmp.path().to_string_lossy().into_owned()),
            outside.to_string_lossy().into_owned(),
        )
        .await
        .unwrap_err();
        assert!(format!("{err}").contains("self-evolution-drafts"));
    }

    #[tokio::test]
    async fn previews_promotion_patch_as_dry_run_without_writing_target() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-template-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "candidate-template-demo",
                "kind": "template_candidate",
                "title": "Reusable DE workflow"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.yaml.draft"),
            "apiVersion: omiga.ai/template/v1alpha1\nkind: Template\nmetadata:\n  id: reusable-de\n",
        )
        .unwrap();

        let preview = preview_self_evolution_draft_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            None,
        )
        .await
        .unwrap();

        assert_eq!(preview.status, "dry_run");
        assert!(!preview.would_write);
        assert!(!preview.applied);
        assert!(!preview.target_exists);
        assert_eq!(preview.kind.as_deref(), Some("template_candidate"));
        assert!(preview
            .draft_file
            .as_deref()
            .is_some_and(|path| path.ends_with("template.yaml.draft")));
        assert!(preview
            .proposed_target_path
            .as_deref()
            .is_some_and(|path| path.contains(
                ".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml"
            )));
        assert!(preview
            .diff_preview
            .as_deref()
            .is_some_and(|diff| diff.contains("--- /dev/null")
                && diff.contains("+++ .omiga/review/self-evolution-promotions/templates/")
                && diff.contains("+kind: Template")));
        assert!(!tmp
            .path()
            .join(".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml")
            .exists());
    }

    #[tokio::test]
    async fn surfaces_creator_companion_drafts_before_promotion() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("creator-batch-20260510T000000Z-test")
            .join("01-template-candidate-reusable-de");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "creator-template-reusable-de",
                "kind": "template_candidate",
                "title": "Reusable DE template",
                "evidence": {
                    "createdBy": "learning_self_evolution_creator"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("DRAFT.md"),
            "# Self-Evolution Unit Creator Draft\n\n- [ ] Review companion files.\n",
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.yaml.draft"),
            "apiVersion: omiga.ai/unit/v1alpha1\nkind: Template\nmetadata:\n  id: reusable-de\n",
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.sh.j2.draft"),
            "#!/usr/bin/env bash\necho draft\n",
        )
        .unwrap();
        std::fs::write(draft_dir.join("example-input.tsv.draft"), "sample\tvalue\n").unwrap();

        let list =
            list_self_evolution_drafts(Some(tmp.path().to_string_lossy().into_owned()), Some(5))
                .await
                .unwrap();
        let draft = &list.batches[0].drafts[0];
        assert_eq!(
            draft.created_by.as_deref(),
            Some("learning_self_evolution_creator")
        );
        assert_eq!(draft.companion_drafts.len(), 2);
        assert!(draft
            .companion_drafts
            .iter()
            .any(|path| path.ends_with("template.sh.j2.draft")));

        let detail = read_self_evolution_draft(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
        )
        .await
        .unwrap();
        assert!(detail.review_preview.actions.iter().any(|action| {
            action.contains("Review companion .draft files") && action.contains("single-file")
        }));
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "template_entry_draft"));
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "template_example_input_draft"));

        let preview = preview_self_evolution_draft_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(preview.companion_drafts.len(), 2);
        assert!(preview
            .companion_review_steps
            .iter()
            .any(|step| step.contains("single-file apply writes only")));
        assert!(preview
            .risk_notes
            .iter()
            .any(|note| note.contains("Companion draft files are present")));
        assert!(preview
            .required_review_steps
            .iter()
            .any(|step| step.contains("single-file apply will not move them")));
        assert!(preview
            .diff_preview
            .as_deref()
            .is_some_and(|diff| diff.contains("kind: Template")));

        let artifact = save_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(artifact.companion_payloads.len(), 2);
        assert!(artifact
            .companion_payloads
            .iter()
            .all(|payload| tmp.path().join(&payload.artifact_path).is_file()));
        let artifact_detail = read_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert!(
            artifact_detail
                .files
                .iter()
                .filter(|file| file.role == "promotion_companion_payload")
                .count()
                >= 2
        );
        let plan = plan_self_evolution_draft_promotion_apply(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert_eq!(plan.companion_drafts.len(), 2);
        assert_eq!(plan.companion_payloads.len(), 2);
        assert!(plan.checks.iter().any(|check| {
            check.id == "companion_payloads_verified" && check.status == "passed"
        }));
        assert!(plan.required_confirmations.iter().any(|confirmation| {
            confirmation.contains("Confirm companion draft files")
                && confirmation.contains("single-file apply")
        }));
        let blocked_request = validate_self_evolution_draft_promotion_apply_request(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some("creator-template-reusable-de".to_string()),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
            Some(true),
            Some(true),
            Some(false),
        )
        .await
        .unwrap();
        assert_eq!(blocked_request.status, "blocked");
        assert!(blocked_request
            .checks
            .iter()
            .any(|check| { check.id == "companion_files_confirmed" && check.status == "blocked" }));
        let ready_request = validate_self_evolution_draft_promotion_apply_request(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some("creator-template-reusable-de".to_string()),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
            Some(true),
            Some(true),
            Some(true),
        )
        .await
        .unwrap();
        assert_eq!(ready_request.status, "ready_for_explicit_apply");

        let missing_target_plan = plan_self_evolution_draft_multi_file_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(missing_target_plan.status, "blocked");
        assert!(missing_target_plan
            .checks
            .iter()
            .any(|check| check.id == "companion_targets_unique" && check.status == "blocked"));

        let review_holding_targets = artifact
            .companion_payloads
            .iter()
            .enumerate()
            .map(
                |(index, payload)| SelfEvolutionDraftPromotionCompanionTargetInput {
                    artifact_path: payload.artifact_path.clone(),
                    target_path: format!(
                        ".omiga/review/self-evolution-promotions/companion-target-{index}.txt"
                    ),
                },
            )
            .collect::<Vec<_>>();
        let review_holding_plan = plan_self_evolution_draft_multi_file_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some(review_holding_targets),
        )
        .await
        .unwrap();
        assert_eq!(review_holding_plan.status, "blocked");
        assert!(review_holding_plan.companion_targets.iter().any(|target| {
            target.checks.iter().any(|check| {
                check.id == "companion_target_not_review_holding_path" && check.status == "blocked"
            })
        }));

        let companion_targets = artifact
            .companion_payloads
            .iter()
            .map(|payload| {
                let target_path = if payload.source_path.ends_with("template.sh.j2.draft") {
                    "plugins/demo/templates/reusable-de/template.sh.j2"
                } else {
                    "plugins/demo/templates/reusable-de/example-input.tsv"
                };
                SelfEvolutionDraftPromotionCompanionTargetInput {
                    artifact_path: payload.artifact_path.clone(),
                    target_path: target_path.to_string(),
                }
            })
            .collect::<Vec<_>>();
        let multi_file_plan = plan_self_evolution_draft_multi_file_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some(companion_targets.clone()),
        )
        .await
        .unwrap();
        assert_eq!(
            multi_file_plan.status,
            "ready_for_reviewed_multi_file_patch"
        );
        assert!(!multi_file_plan.apply_command_available);
        assert!(!multi_file_plan.would_write);
        assert_eq!(multi_file_plan.companion_targets.len(), 2);
        assert!(multi_file_plan.companion_targets.iter().all(|target| {
            target
                .checks
                .iter()
                .all(|check| !check.required || check.status == "passed")
        }));
        let saved_multi_file_plan = save_self_evolution_draft_multi_file_promotion_plan(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some(companion_targets),
        )
        .await
        .unwrap();
        assert_eq!(saved_multi_file_plan.status, "multi_file_plan_saved");
        assert!(tmp
            .path()
            .join(&saved_multi_file_plan.plan_json_path)
            .is_file());
        assert!(tmp
            .path()
            .join(&saved_multi_file_plan.plan_readme_path)
            .is_file());
        let saved_multi_file_readme =
            std::fs::read_to_string(tmp.path().join(&saved_multi_file_plan.plan_readme_path))
                .unwrap();
        assert!(saved_multi_file_readme.contains("Reviewed patch application checklist"));
        assert!(saved_multi_file_readme.contains("Copy reviewed payload"));
        let detail_with_plan = read_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert!(detail_with_plan
            .files
            .iter()
            .any(|file| { file.role == "promotion_multi_file_plan_json" && file.json.is_some() }));
        assert!(detail_with_plan
            .files
            .iter()
            .any(|file| file.role == "promotion_multi_file_plan"));
    }

    #[tokio::test]
    async fn blocks_apply_plan_for_default_review_holding_target() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-template-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "candidate-template-demo",
                "kind": "template_candidate",
                "title": "Reusable DE workflow"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.yaml.draft"),
            "apiVersion: omiga.ai/template/v1alpha1\nkind: Template\nmetadata:\n  id: reusable-de\n",
        )
        .unwrap();

        let artifact = save_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            None,
        )
        .await
        .unwrap();
        let plan = plan_self_evolution_draft_promotion_apply(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();

        assert_eq!(plan.status, "blocked");
        assert!(!plan.apply_command_available);
        assert!(!plan.would_write);
        assert!(!plan.applied);
        assert!(plan
            .proposed_target_path
            .as_deref()
            .is_some_and(|path| path.starts_with(".omiga/review/self-evolution-promotions/")));
        let holding_check = plan
            .checks
            .iter()
            .find(|check| check.id == "target_not_review_holding_path")
            .unwrap();
        assert_eq!(holding_check.status, "blocked");
        assert!(holding_check
            .detail
            .contains("explicit active project target"));
        assert!(!tmp
            .path()
            .join(".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml")
            .exists());
    }

    #[tokio::test]
    async fn previews_existing_explicit_target_without_mutating_it() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-operator-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "candidate-operator-demo",
                "kind": "operator_candidate",
                "title": "Reusable Operator"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("operator.yaml.draft"),
            "apiVersion: omiga.ai/operator/v1alpha1\nkind: Operator\nmetadata:\n  id: new-demo\n",
        )
        .unwrap();
        let target = tmp.path().join("plugins/demo/operators/demo/operator.yaml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "kind: Operator\nmetadata:\n  id: old-demo\n").unwrap();

        let preview = preview_self_evolution_draft_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            Some("plugins/demo/operators/demo/operator.yaml".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(preview.status, "dry_run");
        assert!(preview.target_exists);
        assert_eq!(
            preview.proposed_target_path.as_deref(),
            Some("plugins/demo/operators/demo/operator.yaml")
        );
        let diff = preview.diff_preview.as_deref().unwrap();
        assert!(diff.contains("--- plugins/demo/operators/demo/operator.yaml"));
        assert!(diff.contains("-  id: old-demo"));
        assert!(diff.contains("+  id: new-demo"));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "kind: Operator\nmetadata:\n  id: old-demo\n"
        );
    }

    #[tokio::test]
    async fn blocks_apply_plan_when_proposed_content_payload_hash_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-template-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "candidate-template-demo",
                "kind": "template_candidate",
                "title": "Reusable DE workflow"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.yaml.draft"),
            "apiVersion: omiga.ai/template/v1alpha1\nkind: Template\nmetadata:\n  id: reusable-de\n",
        )
        .unwrap();

        let artifact = save_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
        )
        .await
        .unwrap();
        std::fs::write(
            tmp.path().join(&artifact.proposed_content_path),
            "kind: Template\nmetadata:\n  id: tampered\n",
        )
        .unwrap();

        let plan = plan_self_evolution_draft_promotion_apply(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();

        assert_eq!(plan.status, "blocked");
        let content_check = plan
            .checks
            .iter()
            .find(|check| check.id == "proposed_content_readable")
            .unwrap();
        assert_eq!(content_check.status, "blocked");
        assert!(content_check.detail.contains("sha256 mismatch"));
        assert!(plan
            .proposed_content_sha256
            .as_deref()
            .is_some_and(|hash| hash != artifact.proposed_content_sha256));
        assert!(!tmp
            .path()
            .join("plugins/demo/templates/reusable-de/template.yaml")
            .exists());
    }

    #[tokio::test]
    async fn saves_promotion_review_artifact_without_applying_patch() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-template-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(
            draft_dir.join("candidate.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "candidate-template-demo",
                "kind": "template_candidate",
                "title": "Reusable DE workflow"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            draft_dir.join("template.yaml.draft"),
            "apiVersion: omiga.ai/template/v1alpha1\nkind: Template\nmetadata:\n  id: reusable-de\n",
        )
        .unwrap();
        let target = tmp
            .path()
            .join("plugins/demo/templates/reusable-de/template.yaml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "kind: Template\nmetadata:\n  id: old-template\n").unwrap();

        let artifact = save_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(artifact.status, "artifact_saved");
        assert!(!artifact.would_write);
        assert!(!artifact.applied);
        assert_eq!(
            artifact.proposed_target_path.as_deref(),
            Some("plugins/demo/templates/reusable-de/template.yaml")
        );
        assert!(artifact
            .proposed_content_path
            .ends_with("proposed-target.content"));
        assert!(artifact.proposed_content_sha256.starts_with("sha256:"));
        let proposed_content =
            std::fs::read_to_string(tmp.path().join(&artifact.proposed_content_path)).unwrap();
        assert!(proposed_content.contains("id: reusable-de"));
        let patch = std::fs::read_to_string(tmp.path().join(&artifact.patch_path)).unwrap();
        assert!(patch.contains("# PROMOTION PATCH DRY-RUN"));
        assert!(patch.contains("-  id: old-template"));
        assert!(patch.contains("+  id: reusable-de"));
        let manifest = std::fs::read_to_string(tmp.path().join(&artifact.manifest_path)).unwrap();
        assert!(manifest.contains("\"wouldWrite\": false"));
        assert!(manifest.contains("\"applied\": false"));
        assert!(manifest.contains("\"proposedContentPath\""));
        assert!(manifest.contains("\"proposedContentSha256\""));
        let readme = std::fs::read_to_string(tmp.path().join(&artifact.readme_path)).unwrap();
        assert!(readme.contains("Self-Evolution Promotion Review Artifact"));
        assert!(readme.contains("Proposed content"));
        assert!(readme.contains("Proposed content sha256"));
        let list = list_self_evolution_draft_promotion_artifacts(
            Some(tmp.path().to_string_lossy().into_owned()),
            Some(10),
        )
        .await
        .unwrap();
        assert_eq!(list.artifact_count, 1);
        assert_eq!(
            list.artifacts[0].candidate_id.as_deref(),
            Some("candidate-template-demo")
        );
        assert_eq!(
            list.artifacts[0].kind.as_deref(),
            Some("template_candidate")
        );
        assert_eq!(
            list.artifacts[0].proposed_target_path.as_deref(),
            Some("plugins/demo/templates/reusable-de/template.yaml")
        );
        assert_eq!(list.artifacts[0].target_exists, Some(true));
        assert!(list.artifacts[0]
            .patch_path
            .as_deref()
            .is_some_and(|path| path.ends_with("promotion.patch")));
        assert!(list.artifacts[0]
            .proposed_content_path
            .as_deref()
            .is_some_and(|path| path.ends_with("proposed-target.content")));
        assert_eq!(
            list.artifacts[0].proposed_content_sha256,
            Some(artifact.proposed_content_sha256.clone())
        );
        let detail = read_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert!(detail.found);
        assert_eq!(detail.manifest["wouldWrite"], false);
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "promotion_patch"
                && file
                    .text
                    .as_deref()
                    .is_some_and(|text| text.contains("# PROMOTION PATCH DRY-RUN"))));
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "promotion_manifest" && file.json.is_some()));
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "promotion_proposed_content"
                && file
                    .text
                    .as_deref()
                    .is_some_and(|text| text.contains("id: reusable-de"))));
        let plan = plan_self_evolution_draft_promotion_apply(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert_eq!(plan.status, "ready_for_explicit_apply_review");
        assert!(plan.apply_command_available);
        assert!(!plan.would_write);
        assert!(!plan.applied);
        assert!(plan.target_exists);
        assert!(plan
            .patch_sha256
            .as_deref()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        assert!(plan
            .proposed_content_path
            .as_deref()
            .is_some_and(|path| path.ends_with("proposed-target.content")));
        assert_eq!(
            plan.proposed_content_sha256.as_deref(),
            Some(artifact.proposed_content_sha256.as_str())
        );
        assert!(plan
            .target_current_sha256
            .as_deref()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        assert!(plan
            .checks
            .iter()
            .all(|check| !check.required || check.status == "passed"));
        assert!(plan
            .required_confirmations
            .iter()
            .any(|item| item.contains("candidate-template-demo")));
        let saved_plan = save_self_evolution_draft_promotion_apply_plan(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert_eq!(saved_plan.status, "apply_readiness_saved");
        assert_eq!(saved_plan.plan.status, "ready_for_explicit_apply_review");
        assert!(!saved_plan.would_write);
        assert!(!saved_plan.applied);
        let saved_plan_json =
            std::fs::read_to_string(tmp.path().join(&saved_plan.plan_json_path)).unwrap();
        assert!(saved_plan_json.contains("\"applyCommandAvailable\": true"));
        let saved_plan_readme =
            std::fs::read_to_string(tmp.path().join(&saved_plan.plan_readme_path)).unwrap();
        assert!(saved_plan_readme.contains("Self-Evolution Promotion Apply Readiness"));
        assert!(saved_plan_readme.contains("Patch sha256"));
        assert!(saved_plan_readme.contains("Proposed content sha256"));
        assert!(saved_plan_readme.contains("Current target sha256"));
        let detail = read_self_evolution_draft_promotion_artifact(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
        )
        .await
        .unwrap();
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "promotion_apply_readiness_json" && file.json.is_some()));
        assert!(detail
            .files
            .iter()
            .any(|file| file.role == "promotion_apply_readiness"
                && file
                    .text
                    .as_deref()
                    .is_some_and(|text| text.contains("Apply Readiness"))));
        let apply_request = validate_self_evolution_draft_promotion_apply_request(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some("candidate-template-demo".to_string()),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
            Some(true),
            Some(true),
            None,
        )
        .await
        .unwrap();
        assert_eq!(apply_request.status, "ready_for_explicit_apply");
        assert!(apply_request.apply_command_available);
        assert!(!apply_request.would_write);
        assert!(!apply_request.applied);
        assert!(apply_request.target_exists);
        assert_eq!(apply_request.patch_sha256, plan.patch_sha256);
        assert_eq!(
            apply_request.proposed_content_sha256,
            plan.proposed_content_sha256
        );
        assert_eq!(
            apply_request.target_current_sha256,
            plan.target_current_sha256
        );
        assert!(apply_request
            .checks
            .iter()
            .all(|check| !check.required || check.status == "passed"));
        let blocked_request = validate_self_evolution_draft_promotion_apply_request(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some("candidate-template-demo".to_string()),
            Some("plugins/demo/templates/wrong/template.yaml".to_string()),
            Some(false),
            Some(true),
            None,
        )
        .await
        .unwrap();
        assert_eq!(blocked_request.status, "blocked");
        assert!(
            blocked_request
                .checks
                .iter()
                .any(|check| check.id == "target_path_confirmation_exact"
                    && check.status == "blocked")
        );
        assert!(blocked_request
            .checks
            .iter()
            .any(|check| check.id == "deterministic_tests_confirmed" && check.status == "blocked"));
        let current_target_sha256 = plan.target_current_sha256.clone().unwrap();
        let blocked_apply = apply_self_evolution_draft_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some("candidate-template-demo".to_string()),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
            Some("sha256:wrong".to_string()),
            Some(current_target_sha256.clone()),
            Some(true),
            Some(true),
            None,
        )
        .await
        .unwrap();
        assert_eq!(blocked_apply.status, "blocked");
        assert!(!blocked_apply.would_write);
        assert!(!blocked_apply.applied);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "kind: Template\nmetadata:\n  id: old-template\n"
        );
        let applied = apply_self_evolution_draft_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            artifact.artifact_dir.clone(),
            Some("candidate-template-demo".to_string()),
            Some("plugins/demo/templates/reusable-de/template.yaml".to_string()),
            Some(artifact.proposed_content_sha256.clone()),
            Some(current_target_sha256),
            Some(true),
            Some(true),
            None,
        )
        .await
        .unwrap();
        assert_eq!(applied.status, "applied");
        assert!(applied.apply_command_available);
        assert!(applied.would_write);
        assert!(applied.applied);
        assert!(applied.target_exists_before);
        assert!(applied.bytes_written > 0);
        assert_eq!(
            applied.proposed_content_sha256.as_deref(),
            Some(artifact.proposed_content_sha256.as_str())
        );
        assert!(applied
            .target_new_sha256
            .as_deref()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("id: reusable-de"));
    }

    #[tokio::test]
    async fn rejects_promotion_target_inside_draft_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let draft_dir = tmp
            .path()
            .join(DRAFT_ROOT_RELATIVE)
            .join("draft-batch-20260510T000000Z-test")
            .join("01-template-demo");
        std::fs::create_dir_all(&draft_dir).unwrap();
        std::fs::write(draft_dir.join("candidate.json"), "{}").unwrap();
        std::fs::write(draft_dir.join("template.yaml.draft"), "kind: Template\n").unwrap();

        let err = preview_self_evolution_draft_promotion(
            Some(tmp.path().to_string_lossy().into_owned()),
            project_relative_path(tmp.path(), &draft_dir),
            Some(
                ".omiga/learning/self-evolution-drafts/draft-batch-20260510T000000Z-test/out.yaml"
                    .to_string(),
            ),
        )
        .await
        .unwrap_err();
        assert!(format!("{err}").contains("draft storage"));
    }
}
