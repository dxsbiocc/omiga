import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Box,
  Button,
  Checkbox,
  Chip,
  CircularProgress,
  FormControlLabel,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { alpha } from "@mui/material/styles";
import { compactLabel } from "../../utils/compactLabel";

const ACCENT = "#8b5cf6";

export interface SelfEvolutionDraftSummary {
  draftDir: string;
  candidateId: string;
  kind: string;
  title?: string | null;
  priority?: string | null;
  createdBy?: string | null;
  files: string[];
  specializedDrafts: string[];
  companionDrafts?: string[];
}

export interface SelfEvolutionDraftBatchSummary {
  batchDir: string;
  indexPath?: string | null;
  generatedAt?: string | null;
  draftCount: number;
  drafts: SelfEvolutionDraftSummary[];
}

export interface SelfEvolutionDraftListResponse {
  rootDir: string;
  batchCount: number;
  batches: SelfEvolutionDraftBatchSummary[];
  note: string;
}

export interface SelfEvolutionDraftFilePreview {
  path: string;
  role: string;
  bytes: number;
  truncated: boolean;
  text?: string | null;
  json?: unknown;
}

export interface SelfEvolutionDraftReviewPreview {
  status: string;
  safetyNote: string;
  candidateId?: string | null;
  kind?: string | null;
  title?: string | null;
  targetHint?: string | null;
  actions: string[];
  diffPreview?: string | null;
}

export interface SelfEvolutionDraftPromotionPreviewResponse {
  status: string;
  safetyNote: string;
  draftDir: string;
  candidateId?: string | null;
  kind?: string | null;
  title?: string | null;
  draftFile?: string | null;
  proposedTargetPath?: string | null;
  targetExists: boolean;
  diffPreview?: string | null;
  companionDrafts?: string[];
  companionReviewSteps?: string[];
  riskNotes: string[];
  requiredReviewSteps: string[];
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionCompanionPayload {
  sourcePath: string;
  artifactPath: string;
  role: string;
  bytes: number;
  sha256: string;
}

export interface SelfEvolutionDraftPromotionArtifactResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  patchPath: string;
  manifestPath: string;
  readmePath: string;
  proposedContentPath: string;
  proposedContentSha256: string;
  companionPayloads?: SelfEvolutionDraftPromotionCompanionPayload[];
  proposedTargetPath?: string | null;
  preview: SelfEvolutionDraftPromotionPreviewResponse;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionArtifactSummary {
  artifactDir: string;
  patchPath?: string | null;
  manifestPath?: string | null;
  readmePath?: string | null;
  proposedContentPath?: string | null;
  proposedContentSha256?: string | null;
  candidateId?: string | null;
  kind?: string | null;
  title?: string | null;
  proposedTargetPath?: string | null;
  targetExists?: boolean | null;
  modifiedAtMillis?: number | null;
}

export interface SelfEvolutionDraftPromotionArtifactListResponse {
  rootDir: string;
  artifactCount: number;
  artifacts: SelfEvolutionDraftPromotionArtifactSummary[];
  note: string;
}

export interface SelfEvolutionDraftPromotionArtifactDetailResponse {
  found: boolean;
  artifactDir: string;
  manifest: unknown;
  files: SelfEvolutionDraftFilePreview[];
  note: string;
}

export interface SelfEvolutionDraftPromotionReadinessCheck {
  id: string;
  label: string;
  status: string;
  required: boolean;
  detail: string;
}

export interface SelfEvolutionDraftPromotionApplyPlanResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  patchPath?: string | null;
  manifestPath?: string | null;
  proposedContentPath?: string | null;
  proposedTargetPath?: string | null;
  candidateId?: string | null;
  kind?: string | null;
  title?: string | null;
  patchSha256?: string | null;
  proposedContentSha256?: string | null;
  targetExists: boolean;
  targetCurrentSha256?: string | null;
  companionDrafts?: string[];
  companionPayloads?: SelfEvolutionDraftPromotionCompanionPayload[];
  checks: SelfEvolutionDraftPromotionReadinessCheck[];
  requiredConfirmations: string[];
  suggestedVerification: string[];
  applyCommandAvailable: boolean;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionApplyPlanArtifactResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  planJsonPath: string;
  planReadmePath: string;
  plan: SelfEvolutionDraftPromotionApplyPlanResponse;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionApplyRequestResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  proposedTargetPath?: string | null;
  candidateId?: string | null;
  kind?: string | null;
  title?: string | null;
  patchSha256?: string | null;
  proposedContentSha256?: string | null;
  targetExists: boolean;
  targetCurrentSha256?: string | null;
  companionDrafts?: string[];
  checks: SelfEvolutionDraftPromotionReadinessCheck[];
  requiredConfirmations: string[];
  suggestedVerification: string[];
  applyCommandAvailable: boolean;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionApplyResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  proposedContentPath?: string | null;
  proposedTargetPath?: string | null;
  candidateId?: string | null;
  kind?: string | null;
  title?: string | null;
  proposedContentSha256?: string | null;
  targetExistsBefore: boolean;
  targetPreviousSha256?: string | null;
  targetNewSha256?: string | null;
  companionDrafts?: string[];
  bytesWritten: number;
  checks: SelfEvolutionDraftPromotionReadinessCheck[];
  suggestedVerification: string[];
  applyCommandAvailable: boolean;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionCompanionTargetInput {
  artifactPath: string;
  targetPath: string;
}

export interface SelfEvolutionDraftPromotionCompanionTargetPlan {
  sourcePath: string;
  artifactPath: string;
  role: string;
  bytes: number;
  sha256: string;
  proposedTargetPath?: string | null;
  targetExists: boolean;
  targetCurrentSha256?: string | null;
  diffPreview?: string | null;
  checks: SelfEvolutionDraftPromotionReadinessCheck[];
}

export interface SelfEvolutionDraftPromotionMultiFilePlanResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  manifestTargetPath?: string | null;
  companionTargets: SelfEvolutionDraftPromotionCompanionTargetPlan[];
  checks: SelfEvolutionDraftPromotionReadinessCheck[];
  requiredReviewSteps: string[];
  suggestedVerification: string[];
  applyCommandAvailable: boolean;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse {
  status: string;
  safetyNote: string;
  artifactDir: string;
  planJsonPath: string;
  planReadmePath: string;
  plan: SelfEvolutionDraftPromotionMultiFilePlanResponse;
  wouldWrite: boolean;
  applied: boolean;
}

export interface SelfEvolutionDraftDetailResponse {
  found: boolean;
  draftDir: string;
  candidate: unknown;
  files: SelfEvolutionDraftFilePreview[];
  reviewPreview: SelfEvolutionDraftReviewPreview;
  note: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function candidateCreatedBy(candidate: unknown): string | null {
  if (!isRecord(candidate) || !isRecord(candidate.evidence)) return null;
  return typeof candidate.evidence.createdBy === "string"
    ? candidate.evidence.createdBy
    : null;
}

function isCreatorDraft(candidate: unknown, createdBy?: string | null): boolean {
  return (
    createdBy === "learning_self_evolution_creator" ||
    candidateCreatedBy(candidate) === "learning_self_evolution_creator"
  );
}

function isCompanionDraftFile(file: SelfEvolutionDraftFilePreview): boolean {
  return [
    "operator_script_draft",
    "operator_fixture_draft",
    "template_entry_draft",
    "template_example_input_draft",
  ].includes(file.role);
}

function multiFilePlanPatchText(plan: SelfEvolutionDraftPromotionMultiFilePlanResponse): string {
  const lines = [
    "# Reviewed companion patch plan",
    "",
    `Status: ${plan.status}`,
    `Manifest target: ${plan.manifestTargetPath ?? "<missing>"}`,
    "Safety: plan-only; apply through a separate reviewed patch.",
    "",
    "Companion files:",
  ];
  for (const target of plan.companionTargets) {
    lines.push(
      `- ${target.role}: ${target.artifactPath} -> ${target.proposedTargetPath ?? "<missing target>"}`,
      `  sha256: ${target.sha256}`,
      `  target: ${target.targetExists ? "exists; review merge/replacement" : "new file"}`,
    );
    const firstBlocked = target.checks.find((check) => check.required && check.status !== "passed");
    if (firstBlocked) lines.push(`  blocked: ${firstBlocked.detail}`);
  }
  lines.push(
    "",
    "Checklist:",
    "- Create a separate reviewed branch/commit.",
    "- Review manifest and companion diffs together.",
    "- Copy companion payloads to their approved target paths only after review.",
    "- Run unit_authoring_validate and deterministic smoke tests.",
    "- Do not register units, change defaults, or mutate archives in this step.",
  );
  return lines.join("\n");
}

interface SelfEvolutionDraftBrowserProps {
  projectRoot: string | null | undefined;
  refreshToken?: number;
}

export function SelfEvolutionDraftBrowser({
  projectRoot,
  refreshToken,
}: SelfEvolutionDraftBrowserProps) {
  const [response, setResponse] = useState<SelfEvolutionDraftListResponse | null>(null);
  const [selectedDraftDir, setSelectedDraftDir] = useState<string | null>(null);
  const [detail, setDetail] = useState<SelfEvolutionDraftDetailResponse | null>(null);
  const [promotionPreview, setPromotionPreview] =
    useState<SelfEvolutionDraftPromotionPreviewResponse | null>(null);
  const [promotionArtifact, setPromotionArtifact] =
    useState<SelfEvolutionDraftPromotionArtifactResponse | null>(null);
  const [artifactList, setArtifactList] =
    useState<SelfEvolutionDraftPromotionArtifactListResponse | null>(null);
  const [selectedArtifactDir, setSelectedArtifactDir] = useState<string | null>(null);
  const [promotionArtifactDetail, setPromotionArtifactDetail] =
    useState<SelfEvolutionDraftPromotionArtifactDetailResponse | null>(null);
  const [promotionApplyPlan, setPromotionApplyPlan] =
    useState<SelfEvolutionDraftPromotionApplyPlanResponse | null>(null);
  const [promotionApplyPlanArtifact, setPromotionApplyPlanArtifact] =
    useState<SelfEvolutionDraftPromotionApplyPlanArtifactResponse | null>(null);
  const [promotionApplyRequest, setPromotionApplyRequest] =
    useState<SelfEvolutionDraftPromotionApplyRequestResponse | null>(null);
  const [promotionApplyResult, setPromotionApplyResult] =
    useState<SelfEvolutionDraftPromotionApplyResponse | null>(null);
  const [multiFilePlan, setMultiFilePlan] =
    useState<SelfEvolutionDraftPromotionMultiFilePlanResponse | null>(null);
  const [multiFilePlanArtifact, setMultiFilePlanArtifact] =
    useState<SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse | null>(null);
  const [promotionTargetPath, setPromotionTargetPath] = useState("");
  const [companionTargetPaths, setCompanionTargetPaths] = useState<Record<string, string>>({});
  const [applyCandidateConfirmation, setApplyCandidateConfirmation] = useState("");
  const [applyTargetConfirmation, setApplyTargetConfirmation] = useState("");
  const [applyContentHashConfirmation, setApplyContentHashConfirmation] = useState("");
  const [applyTargetHashConfirmation, setApplyTargetHashConfirmation] = useState("");
  const [applyTestsConfirmed, setApplyTestsConfirmed] = useState(false);
  const [applyBranchConfirmed, setApplyBranchConfirmed] = useState(false);
  const [applyCompanionFilesConfirmed, setApplyCompanionFilesConfirmed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [promotionLoading, setPromotionLoading] = useState(false);
  const [artifactSaving, setArtifactSaving] = useState(false);
  const [applyPlanSaving, setApplyPlanSaving] = useState(false);
  const [applyRequestValidating, setApplyRequestValidating] = useState(false);
  const [applySubmitting, setApplySubmitting] = useState(false);
  const [multiFilePlanning, setMultiFilePlanning] = useState(false);
  const [multiFilePlanSaving, setMultiFilePlanSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const resetApplyRequestGate = () => {
    setPromotionApplyRequest(null);
    setPromotionApplyResult(null);
    setMultiFilePlan(null);
    setMultiFilePlanArtifact(null);
    setCompanionTargetPaths({});
    setApplyCandidateConfirmation("");
    setApplyTargetConfirmation("");
    setApplyContentHashConfirmation("");
    setApplyTargetHashConfirmation("");
    setApplyTestsConfirmed(false);
    setApplyBranchConfirmed(false);
    setApplyCompanionFilesConfirmed(false);
    setApplyRequestValidating(false);
    setApplySubmitting(false);
    setMultiFilePlanning(false);
    setMultiFilePlanSaving(false);
  };

  const loadPromotionPreview = async (targetPathOverride?: string | null) => {
    if (!projectRoot || !selectedDraftDir) {
      setPromotionPreview(null);
      setPromotionLoading(false);
      return;
    }
    setPromotionLoading(true);
    setPromotionPreview(null);
    setPromotionArtifact(null);
    setPromotionApplyPlanArtifact(null);
    resetApplyRequestGate();
    setError(null);
    try {
      const targetPath = targetPathOverride?.trim();
      const next = await invoke<SelfEvolutionDraftPromotionPreviewResponse>(
        "preview_self_evolution_draft_promotion",
        {
          projectRoot,
          draftDir: selectedDraftDir,
          ...(targetPath ? { targetPath } : {}),
        },
      );
      setPromotionPreview(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPromotionPreview(null);
    } finally {
      setPromotionLoading(false);
    }
  };

  const savePromotionArtifact = async () => {
    if (!projectRoot || !selectedDraftDir) {
      setArtifactSaving(false);
      return;
    }
    setArtifactSaving(true);
    setError(null);
    try {
      const targetPath = promotionTargetPath.trim();
      const next = await invoke<SelfEvolutionDraftPromotionArtifactResponse>(
        "save_self_evolution_draft_promotion_artifact",
        {
          projectRoot,
          draftDir: selectedDraftDir,
          ...(targetPath ? { targetPath } : {}),
        },
      );
      setPromotionArtifact(next);
      setPromotionPreview(next.preview);
      setSelectedArtifactDir(next.artifactDir);
      void loadPromotionArtifactDetail(next.artifactDir);
      void loadPromotionApplyPlan(next.artifactDir);
      void loadPromotionArtifacts(next.artifactDir);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPromotionArtifact(null);
    } finally {
      setArtifactSaving(false);
    }
  };

  const loadPromotionApplyPlan = async (artifactDir: string | null | undefined) => {
    if (!projectRoot || !artifactDir) {
      setPromotionApplyPlan(null);
      return;
    }
    try {
      const next = await invoke<SelfEvolutionDraftPromotionApplyPlanResponse>(
        "plan_self_evolution_draft_promotion_apply",
        {
          projectRoot,
          artifactDir,
        },
      );
      setPromotionApplyPlan(next);
    } catch {
      setPromotionApplyPlan(null);
    }
  };

  const loadPromotionArtifactDetail = async (artifactDir: string | null | undefined) => {
    if (!projectRoot || !artifactDir) {
      setPromotionArtifactDetail(null);
      return;
    }
    try {
      const next = await invoke<SelfEvolutionDraftPromotionArtifactDetailResponse>(
        "read_self_evolution_draft_promotion_artifact",
        {
          projectRoot,
          artifactDir,
        },
      );
      setPromotionArtifactDetail(next);
    } catch {
      setPromotionArtifactDetail(null);
    }
  };

  const selectPromotionArtifact = (artifactDir: string | null | undefined) => {
    const nextArtifactDir = artifactDir ?? null;
    setSelectedArtifactDir(nextArtifactDir);
    setPromotionApplyPlanArtifact(null);
    resetApplyRequestGate();
    void loadPromotionArtifactDetail(nextArtifactDir);
    void loadPromotionApplyPlan(nextArtifactDir);
  };

  const savePromotionApplyPlan = async () => {
    const artifactDir = selectedArtifactDir ?? promotionApplyPlan?.artifactDir;
    if (!projectRoot || !artifactDir) {
      setApplyPlanSaving(false);
      return;
    }
    setApplyPlanSaving(true);
    setError(null);
    try {
      const next = await invoke<SelfEvolutionDraftPromotionApplyPlanArtifactResponse>(
        "save_self_evolution_draft_promotion_apply_plan",
        {
          projectRoot,
          artifactDir,
        },
      );
      setPromotionApplyPlanArtifact(next);
      setPromotionApplyPlan(next.plan);
      void loadPromotionArtifactDetail(next.artifactDir);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPromotionApplyPlanArtifact(null);
    } finally {
      setApplyPlanSaving(false);
    }
  };

  const validatePromotionApplyRequest = async () => {
    const artifactDir = selectedArtifactDir ?? promotionApplyPlan?.artifactDir;
    if (!projectRoot || !artifactDir) {
      setApplyRequestValidating(false);
      return;
    }
    setApplyRequestValidating(true);
    setError(null);
    try {
      const next = await invoke<SelfEvolutionDraftPromotionApplyRequestResponse>(
        "validate_self_evolution_draft_promotion_apply_request",
        {
          projectRoot,
          artifactDir,
          candidateIdConfirmation: applyCandidateConfirmation,
          targetPathConfirmation: applyTargetConfirmation,
          testsConfirmed: applyTestsConfirmed,
          reviewedBranchConfirmed: applyBranchConfirmed,
          companionFilesConfirmed: applyCompanionFilesConfirmed,
        },
      );
      setPromotionApplyRequest(next);
      setPromotionApplyResult(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPromotionApplyRequest(null);
    } finally {
      setApplyRequestValidating(false);
    }
  };

  const applyPromotion = async () => {
    const artifactDir = selectedArtifactDir ?? promotionApplyPlan?.artifactDir;
    if (!projectRoot || !artifactDir) {
      setApplySubmitting(false);
      return;
    }
    setApplySubmitting(true);
    setError(null);
    try {
      const next = await invoke<SelfEvolutionDraftPromotionApplyResponse>(
        "apply_self_evolution_draft_promotion",
        {
          projectRoot,
          artifactDir,
          candidateIdConfirmation: applyCandidateConfirmation,
          targetPathConfirmation: applyTargetConfirmation,
          proposedContentSha256Confirmation: applyContentHashConfirmation,
          targetCurrentSha256Confirmation: applyTargetHashConfirmation,
          testsConfirmed: applyTestsConfirmed,
          reviewedBranchConfirmed: applyBranchConfirmed,
          companionFilesConfirmed: applyCompanionFilesConfirmed,
        },
      );
      setPromotionApplyResult(next);
      void loadPromotionApplyPlan(next.artifactDir);
      void loadPromotionArtifactDetail(next.artifactDir);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPromotionApplyResult(null);
    } finally {
      setApplySubmitting(false);
    }
  };

  const companionTargetInputs = (): SelfEvolutionDraftPromotionCompanionTargetInput[] =>
    (promotionApplyPlan?.companionPayloads ?? []).map((payload) => ({
      artifactPath: payload.artifactPath,
      targetPath: companionTargetPaths[payload.artifactPath] ?? "",
    }));

  const planMultiFilePromotion = async () => {
    const artifactDir = selectedArtifactDir ?? promotionApplyPlan?.artifactDir;
    if (!projectRoot || !artifactDir) {
      setMultiFilePlanning(false);
      return;
    }
    setMultiFilePlanning(true);
    setError(null);
    try {
      const next = await invoke<SelfEvolutionDraftPromotionMultiFilePlanResponse>(
        "plan_self_evolution_draft_multi_file_promotion",
        {
          projectRoot,
          artifactDir,
          companionTargets: companionTargetInputs(),
        },
      );
      setMultiFilePlan(next);
      setMultiFilePlanArtifact(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setMultiFilePlan(null);
    } finally {
      setMultiFilePlanning(false);
    }
  };

  const saveMultiFilePromotionPlan = async () => {
    const artifactDir = selectedArtifactDir ?? promotionApplyPlan?.artifactDir;
    if (!projectRoot || !artifactDir) {
      setMultiFilePlanSaving(false);
      return;
    }
    setMultiFilePlanSaving(true);
    setError(null);
    try {
      const next = await invoke<SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse>(
        "save_self_evolution_draft_multi_file_promotion_plan",
        {
          projectRoot,
          artifactDir,
          companionTargets: companionTargetInputs(),
        },
      );
      setMultiFilePlanArtifact(next);
      setMultiFilePlan(next.plan);
      void loadPromotionArtifactDetail(next.artifactDir);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setMultiFilePlanArtifact(null);
    } finally {
      setMultiFilePlanSaving(false);
    }
  };

  const loadPromotionArtifacts = async (preferredArtifactDir?: string | null) => {
    if (!projectRoot) {
      setArtifactList(null);
      setSelectedArtifactDir(null);
      setPromotionArtifactDetail(null);
      setPromotionApplyPlan(null);
      setPromotionApplyPlanArtifact(null);
      resetApplyRequestGate();
      return;
    }
    try {
      const next = await invoke<SelfEvolutionDraftPromotionArtifactListResponse>(
        "list_self_evolution_draft_promotion_artifacts",
        {
          projectRoot,
          limit: 10,
        },
      );
      setArtifactList(next);
      const preferred = preferredArtifactDir ?? selectedArtifactDir;
      const nextArtifactDir =
        preferred && next.artifacts.some((artifact) => artifact.artifactDir === preferred)
          ? preferred
          : next.artifacts[0]?.artifactDir ?? null;
      if (nextArtifactDir !== selectedArtifactDir) {
        setPromotionApplyPlanArtifact(null);
        resetApplyRequestGate();
      }
      setSelectedArtifactDir(nextArtifactDir);
      void loadPromotionArtifactDetail(nextArtifactDir);
      void loadPromotionApplyPlan(nextArtifactDir);
    } catch {
      setArtifactList(null);
      setSelectedArtifactDir(null);
      setPromotionArtifactDetail(null);
      setPromotionApplyPlan(null);
      setPromotionApplyPlanArtifact(null);
      resetApplyRequestGate();
    }
  };

  const loadDrafts = async () => {
    if (!projectRoot) {
      setResponse(null);
      setSelectedDraftDir(null);
      setDetail(null);
      setPromotionPreview(null);
      setPromotionArtifact(null);
      setArtifactList(null);
      setSelectedArtifactDir(null);
      setPromotionArtifactDetail(null);
      setPromotionApplyPlan(null);
      setPromotionApplyPlanArtifact(null);
      resetApplyRequestGate();
      setPromotionLoading(false);
      setArtifactSaving(false);
      setApplyPlanSaving(false);
      setApplySubmitting(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<SelfEvolutionDraftListResponse>("list_self_evolution_drafts", {
        projectRoot,
        limit: 20,
      });
      void loadPromotionArtifacts();
      setResponse(next);
      const allDrafts = next.batches.flatMap((batch) => batch.drafts);
      const nextSelected =
        selectedDraftDir && allDrafts.some((draft) => draft.draftDir === selectedDraftDir)
          ? selectedDraftDir
          : allDrafts[0]?.draftDir ?? null;
      setSelectedDraftDir(nextSelected);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setResponse(null);
      setSelectedDraftDir(null);
      setDetail(null);
      setPromotionPreview(null);
      setPromotionArtifact(null);
      setArtifactList(null);
      setSelectedArtifactDir(null);
      setPromotionArtifactDetail(null);
      setPromotionApplyPlan(null);
      setPromotionApplyPlanArtifact(null);
      resetApplyRequestGate();
      setPromotionLoading(false);
      setArtifactSaving(false);
      setApplyPlanSaving(false);
      setApplySubmitting(false);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadDrafts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectRoot, refreshToken]);

  useEffect(() => {
    if (!projectRoot || !selectedDraftDir) {
      setDetail(null);
      setPromotionPreview(null);
      setPromotionArtifact(null);
      setPromotionTargetPath("");
      resetApplyRequestGate();
      setPromotionLoading(false);
      setArtifactSaving(false);
      setApplyPlanSaving(false);
      setApplySubmitting(false);
      return;
    }
    let cancelled = false;
    setDetailLoading(true);
    setPromotionLoading(true);
    setPromotionPreview(null);
    setPromotionArtifact(null);
    setPromotionApplyPlanArtifact(null);
    resetApplyRequestGate();
    setPromotionTargetPath("");
    setError(null);
    invoke<SelfEvolutionDraftDetailResponse>("read_self_evolution_draft", {
      projectRoot,
      draftDir: selectedDraftDir,
    })
      .then((next) => {
        if (!cancelled) setDetail(next);
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setDetail(null);
        }
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    invoke<SelfEvolutionDraftPromotionPreviewResponse>(
      "preview_self_evolution_draft_promotion",
      {
        projectRoot,
        draftDir: selectedDraftDir,
      },
    )
      .then((next) => {
        if (!cancelled) setPromotionPreview(next);
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setPromotionPreview(null);
        }
      })
      .finally(() => {
        if (!cancelled) setPromotionLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectRoot, selectedDraftDir]);

  return (
    <SelfEvolutionDraftBrowserView
      response={response}
      selectedDraftDir={selectedDraftDir}
      detail={detail}
      promotionPreview={promotionPreview}
      promotionArtifact={promotionArtifact}
      artifactList={artifactList}
      selectedArtifactDir={selectedArtifactDir}
      promotionArtifactDetail={promotionArtifactDetail}
      promotionApplyPlan={promotionApplyPlan}
      promotionApplyPlanArtifact={promotionApplyPlanArtifact}
      promotionApplyRequest={promotionApplyRequest}
      promotionApplyResult={promotionApplyResult}
      multiFilePlan={multiFilePlan}
      multiFilePlanArtifact={multiFilePlanArtifact}
      promotionTargetPath={promotionTargetPath}
      companionTargetPaths={companionTargetPaths}
      applyCandidateConfirmation={applyCandidateConfirmation}
      applyTargetConfirmation={applyTargetConfirmation}
      applyContentHashConfirmation={applyContentHashConfirmation}
      applyTargetHashConfirmation={applyTargetHashConfirmation}
      applyTestsConfirmed={applyTestsConfirmed}
      applyBranchConfirmed={applyBranchConfirmed}
      applyCompanionFilesConfirmed={applyCompanionFilesConfirmed}
      loading={loading}
      detailLoading={detailLoading}
      promotionLoading={promotionLoading}
      artifactSaving={artifactSaving}
      applyPlanSaving={applyPlanSaving}
      applyRequestValidating={applyRequestValidating}
      applySubmitting={applySubmitting}
      multiFilePlanning={multiFilePlanning}
      multiFilePlanSaving={multiFilePlanSaving}
      error={error}
      onRefresh={() => void loadDrafts()}
      onSelect={setSelectedDraftDir}
      onPromotionTargetPathChange={setPromotionTargetPath}
      onPreviewPromotionTarget={() => void loadPromotionPreview(promotionTargetPath)}
      onSavePromotionArtifact={() => void savePromotionArtifact()}
      onSelectPromotionArtifact={(artifactDir) => selectPromotionArtifact(artifactDir)}
      onSavePromotionApplyPlan={() => void savePromotionApplyPlan()}
      onApplyCandidateConfirmationChange={setApplyCandidateConfirmation}
      onApplyTargetConfirmationChange={setApplyTargetConfirmation}
      onApplyContentHashConfirmationChange={setApplyContentHashConfirmation}
      onApplyTargetHashConfirmationChange={setApplyTargetHashConfirmation}
      onApplyTestsConfirmedChange={setApplyTestsConfirmed}
      onApplyBranchConfirmedChange={setApplyBranchConfirmed}
      onApplyCompanionFilesConfirmedChange={setApplyCompanionFilesConfirmed}
      onCompanionTargetPathChange={(artifactPath, targetPath) =>
        setCompanionTargetPaths((current) => ({
          ...current,
          [artifactPath]: targetPath,
        }))
      }
      onValidatePromotionApplyRequest={() => void validatePromotionApplyRequest()}
      onApplyPromotion={() => void applyPromotion()}
      onPlanMultiFilePromotion={() => void planMultiFilePromotion()}
      onSaveMultiFilePromotionPlan={() => void saveMultiFilePromotionPlan()}
    />
  );
}

export interface SelfEvolutionDraftBrowserViewProps {
  response: SelfEvolutionDraftListResponse | null;
  selectedDraftDir: string | null;
  detail: SelfEvolutionDraftDetailResponse | null;
  promotionPreview?: SelfEvolutionDraftPromotionPreviewResponse | null;
  promotionArtifact?: SelfEvolutionDraftPromotionArtifactResponse | null;
  artifactList?: SelfEvolutionDraftPromotionArtifactListResponse | null;
  selectedArtifactDir?: string | null;
  promotionArtifactDetail?: SelfEvolutionDraftPromotionArtifactDetailResponse | null;
  promotionApplyPlan?: SelfEvolutionDraftPromotionApplyPlanResponse | null;
  promotionApplyPlanArtifact?: SelfEvolutionDraftPromotionApplyPlanArtifactResponse | null;
  promotionApplyRequest?: SelfEvolutionDraftPromotionApplyRequestResponse | null;
  promotionApplyResult?: SelfEvolutionDraftPromotionApplyResponse | null;
  multiFilePlan?: SelfEvolutionDraftPromotionMultiFilePlanResponse | null;
  multiFilePlanArtifact?: SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse | null;
  promotionTargetPath?: string;
  companionTargetPaths?: Record<string, string>;
  applyCandidateConfirmation?: string;
  applyTargetConfirmation?: string;
  applyContentHashConfirmation?: string;
  applyTargetHashConfirmation?: string;
  applyTestsConfirmed?: boolean;
  applyBranchConfirmed?: boolean;
  applyCompanionFilesConfirmed?: boolean;
  loading?: boolean;
  detailLoading?: boolean;
  promotionLoading?: boolean;
  artifactSaving?: boolean;
  applyPlanSaving?: boolean;
  applyRequestValidating?: boolean;
  applySubmitting?: boolean;
  multiFilePlanning?: boolean;
  multiFilePlanSaving?: boolean;
  error?: string | null;
  onRefresh?: () => void;
  onSelect?: (draftDir: string) => void;
  onPromotionTargetPathChange?: (targetPath: string) => void;
  onPreviewPromotionTarget?: () => void;
  onSavePromotionArtifact?: () => void;
  onSelectPromotionArtifact?: (artifactDir: string) => void;
  onSavePromotionApplyPlan?: () => void;
  onApplyCandidateConfirmationChange?: (value: string) => void;
  onApplyTargetConfirmationChange?: (value: string) => void;
  onApplyContentHashConfirmationChange?: (value: string) => void;
  onApplyTargetHashConfirmationChange?: (value: string) => void;
  onApplyTestsConfirmedChange?: (value: boolean) => void;
  onApplyBranchConfirmedChange?: (value: boolean) => void;
  onApplyCompanionFilesConfirmedChange?: (value: boolean) => void;
  onCompanionTargetPathChange?: (artifactPath: string, targetPath: string) => void;
  onValidatePromotionApplyRequest?: () => void;
  onApplyPromotion?: () => void;
  onPlanMultiFilePromotion?: () => void;
  onSaveMultiFilePromotionPlan?: () => void;
}

export function SelfEvolutionDraftBrowserView({
  response,
  selectedDraftDir,
  detail,
  promotionPreview,
  promotionArtifact,
  artifactList,
  selectedArtifactDir,
  promotionArtifactDetail,
  promotionApplyPlan,
  promotionApplyPlanArtifact,
  promotionApplyRequest,
  promotionApplyResult,
  multiFilePlan,
  multiFilePlanArtifact,
  promotionTargetPath = "",
  companionTargetPaths = {},
  applyCandidateConfirmation = "",
  applyTargetConfirmation = "",
  applyContentHashConfirmation = "",
  applyTargetHashConfirmation = "",
  applyTestsConfirmed = false,
  applyBranchConfirmed = false,
  applyCompanionFilesConfirmed = false,
  loading = false,
  detailLoading = false,
  promotionLoading = false,
  artifactSaving = false,
  applyPlanSaving = false,
  applyRequestValidating = false,
  applySubmitting = false,
  multiFilePlanning = false,
  multiFilePlanSaving = false,
  error,
  onRefresh,
  onSelect,
  onPromotionTargetPathChange,
  onPreviewPromotionTarget,
  onSavePromotionArtifact,
  onSelectPromotionArtifact,
  onSavePromotionApplyPlan,
  onApplyCandidateConfirmationChange,
  onApplyTargetConfirmationChange,
  onApplyContentHashConfirmationChange,
  onApplyTargetHashConfirmationChange,
  onApplyTestsConfirmedChange,
  onApplyBranchConfirmedChange,
  onApplyCompanionFilesConfirmedChange,
  onCompanionTargetPathChange,
  onValidatePromotionApplyRequest,
  onApplyPromotion,
  onPlanMultiFilePromotion,
  onSaveMultiFilePromotionPlan,
}: SelfEvolutionDraftBrowserViewProps) {
  const drafts = response?.batches.flatMap((batch) => batch.drafts) ?? [];
  return (
    <Stack spacing={1}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={1}>
        <Box sx={{ minWidth: 0 }}>
          <Typography variant="caption" sx={{ display: "block", fontSize: 10, fontWeight: 800 }}>
            Self-evolution Draft Review
          </Typography>
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ display: "block", fontSize: 9.5, lineHeight: 1.35 }}
          >
            默认只读审阅草稿；显式 apply 只写确认的单个文件，不注册单元或修改配置。
          </Typography>
        </Box>
        <Button
          size="small"
          variant="outlined"
          onClick={onRefresh}
          disabled={loading}
          sx={{ minWidth: 0, fontSize: 10, py: 0.15 }}
        >
          {loading ? "刷新中" : "刷新"}
        </Button>
      </Stack>

      {error ? (
        <Typography variant="caption" color="error" sx={{ fontSize: 10 }}>
          {error}
        </Typography>
      ) : null}

      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
        <Chip size="small" label={`${response?.batchCount ?? 0} batches`} sx={{ height: 18, fontSize: 9 }} />
        <Chip size="small" label={`${drafts.length} drafts`} variant="outlined" sx={{ height: 18, fontSize: 9 }} />
        <Chip
          size="small"
          label={`${artifactList?.artifactCount ?? 0} review artifacts`}
          variant="outlined"
          sx={{ height: 18, fontSize: 9 }}
        />
      </Stack>

      {artifactList?.artifacts.length ? (
        <PromotionArtifactListPanel
          artifactList={artifactList}
          selectedArtifactDir={selectedArtifactDir}
          onSelectArtifact={onSelectPromotionArtifact}
        />
      ) : null}
      {promotionArtifactDetail?.found ? (
        <PromotionArtifactDetailPanel detail={promotionArtifactDetail} />
      ) : null}
      {promotionApplyPlan ? (
        <PromotionApplyPlanPanel
          plan={promotionApplyPlan}
          savedPlanArtifact={promotionApplyPlanArtifact}
          applyRequest={promotionApplyRequest}
          applyResult={promotionApplyResult}
          multiFilePlan={multiFilePlan}
          multiFilePlanArtifact={multiFilePlanArtifact}
          saving={applyPlanSaving}
          requestValidating={applyRequestValidating}
          applying={applySubmitting}
          multiFilePlanning={multiFilePlanning}
          multiFilePlanSaving={multiFilePlanSaving}
          candidateConfirmation={applyCandidateConfirmation}
          targetConfirmation={applyTargetConfirmation}
          contentHashConfirmation={applyContentHashConfirmation}
          targetHashConfirmation={applyTargetHashConfirmation}
          testsConfirmed={applyTestsConfirmed}
          branchConfirmed={applyBranchConfirmed}
          companionFilesConfirmed={applyCompanionFilesConfirmed}
          companionTargetPaths={companionTargetPaths}
          onSavePlan={onSavePromotionApplyPlan}
          onCandidateConfirmationChange={onApplyCandidateConfirmationChange}
          onTargetConfirmationChange={onApplyTargetConfirmationChange}
          onContentHashConfirmationChange={onApplyContentHashConfirmationChange}
          onTargetHashConfirmationChange={onApplyTargetHashConfirmationChange}
          onTestsConfirmedChange={onApplyTestsConfirmedChange}
          onBranchConfirmedChange={onApplyBranchConfirmedChange}
          onCompanionFilesConfirmedChange={onApplyCompanionFilesConfirmedChange}
          onCompanionTargetPathChange={onCompanionTargetPathChange}
          onValidateApplyRequest={onValidatePromotionApplyRequest}
          onApplyPromotion={onApplyPromotion}
          onPlanMultiFilePromotion={onPlanMultiFilePromotion}
          onSaveMultiFilePromotionPlan={onSaveMultiFilePromotionPlan}
        />
      ) : null}

      {loading && drafts.length === 0 ? (
        <Stack direction="row" alignItems="center" spacing={0.75}>
          <CircularProgress size={14} />
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
            正在读取 self-evolution 草稿…
          </Typography>
        </Stack>
      ) : drafts.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10, lineHeight: 1.5 }}>
          当前项目暂无 `.omiga/learning/self-evolution-drafts/` 草稿。运行
          learning_self_evolution_draft_write 后会在这里出现。
        </Typography>
      ) : (
        <Stack spacing={0.8}>
          {response?.batches.map((batch) => (
            <Box key={batch.batchDir}>
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: "block", mb: 0.35, fontSize: 9, fontWeight: 800 }}
              >
                {compactLabel(batch.batchDir, 52)}
                {batch.generatedAt ? ` · ${batch.generatedAt}` : ""}
              </Typography>
              <Stack spacing={0.6}>
                {batch.drafts.map((draft) => (
                  <SelfEvolutionDraftRow
                    key={draft.draftDir}
                    draft={draft}
                    selected={draft.draftDir === selectedDraftDir}
                    onClick={() => onSelect?.(draft.draftDir)}
                  />
                ))}
              </Stack>
            </Box>
          ))}
        </Stack>
      )}

      {detailLoading ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
          正在读取草稿详情…
        </Typography>
      ) : detail?.found ? (
        <SelfEvolutionDraftDetailPanel
          detail={detail}
          promotionPreview={promotionPreview}
          promotionArtifact={promotionArtifact}
          promotionTargetPath={promotionTargetPath}
          promotionLoading={promotionLoading}
          artifactSaving={artifactSaving}
          onPromotionTargetPathChange={onPromotionTargetPathChange}
          onPreviewPromotionTarget={onPreviewPromotionTarget}
          onSavePromotionArtifact={onSavePromotionArtifact}
        />
      ) : selectedDraftDir ? (
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: 10 }}>
          草稿未找到：{selectedDraftDir}
        </Typography>
      ) : null}
    </Stack>
  );
}

function PromotionArtifactListPanel({
  artifactList,
  selectedArtifactDir,
  onSelectArtifact,
}: {
  artifactList: SelfEvolutionDraftPromotionArtifactListResponse;
  selectedArtifactDir?: string | null;
  onSelectArtifact?: (artifactDir: string) => void;
}) {
  return (
    <Box
      sx={{
        p: 0.75,
        borderRadius: 1,
        border: `1px solid ${alpha("#10b981", 0.18)}`,
        bgcolor: alpha("#10b981", 0.035),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9.5, fontWeight: 900 }}>
        Saved promotion review artifacts
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 8.6 }}>
        选择 artifact 可只读查看 patch/manifest，并刷新对应 readiness gate。
      </Typography>
      <Stack spacing={0.45} sx={{ mt: 0.45 }}>
        {artifactList.artifacts.slice(0, 3).map((artifact) => {
          const selected = artifact.artifactDir === selectedArtifactDir;
          return (
          <Box
            key={artifact.artifactDir}
            role="button"
            tabIndex={0}
            onClick={() => onSelectArtifact?.(artifact.artifactDir)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                onSelectArtifact?.(artifact.artifactDir);
              }
            }}
            sx={{
              p: 0.45,
              borderRadius: 0.8,
              border: `1px solid ${alpha(selected ? "#10b981" : "#64748b", selected ? 0.28 : 0.1)}`,
              bgcolor: selected ? alpha("#10b981", 0.075) : "transparent",
              cursor: "pointer",
            }}
          >
            <Stack direction="row" spacing={0.4} alignItems="center" flexWrap="wrap" useFlexGap>
              {selected ? (
                <Chip size="small" label="selected" sx={{ height: 15, fontSize: 8 }} />
              ) : null}
              {artifact.kind ? (
                <Chip size="small" label={artifact.kind} variant="outlined" sx={{ height: 15, fontSize: 8 }} />
              ) : null}
            </Stack>
            <Typography variant="caption" sx={{ display: "block", fontSize: 9.2, fontWeight: 700 }}>
              {compactLabel(artifact.title ?? artifact.candidateId ?? artifact.artifactDir, 52)}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 8.8, lineHeight: 1.35 }}>
              {compactLabel(artifact.patchPath ?? artifact.artifactDir, 76)}
            </Typography>
            {artifact.proposedTargetPath ? (
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 8.6, lineHeight: 1.35 }}>
                target: {compactLabel(artifact.proposedTargetPath, 76)}
              </Typography>
            ) : null}
            {artifact.proposedContentSha256 ? (
              <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 8.6, lineHeight: 1.35 }}>
                content: {compactLabel(artifact.proposedContentSha256, 42)}
              </Typography>
            ) : null}
          </Box>
          );
        })}
      </Stack>
    </Box>
  );
}

function PromotionArtifactDetailPanel({
  detail,
}: {
  detail: SelfEvolutionDraftPromotionArtifactDetailResponse;
}) {
  const patch = detail.files.find((file) => file.role === "promotion_patch");
  const manifest = detail.files.find((file) => file.role === "promotion_manifest");
  const proposedContent = detail.files.find(
    (file) => file.role === "promotion_proposed_content",
  );
  const companionPayloads = detail.files.filter(
    (file) => file.role === "promotion_companion_payload",
  );
  const applyReadiness = detail.files.find(
    (file) => file.role === "promotion_apply_readiness",
  );
  const applyReadinessJson = detail.files.find(
    (file) => file.role === "promotion_apply_readiness_json",
  );
  const multiFilePlan = detail.files.find(
    (file) => file.role === "promotion_multi_file_plan",
  );
  const multiFilePlanJson = detail.files.find(
    (file) => file.role === "promotion_multi_file_plan_json",
  );
  const readme = detail.files.find((file) => file.path.endsWith("README.md"));
  const status =
    typeof detail.manifest === "object" &&
    detail.manifest !== null &&
    "status" in detail.manifest &&
    typeof (detail.manifest as { status?: unknown }).status === "string"
      ? (detail.manifest as { status: string }).status
      : "artifact_detail";
  return (
    <Box
      sx={{
        p: 0.75,
        borderRadius: 1,
        border: `1px solid ${alpha("#10b981", 0.2)}`,
        bgcolor: alpha("#10b981", 0.04),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9.5, fontWeight: 900 }}>
        Saved artifact detail (read-only)
      </Typography>
      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.45 }}>
        <Chip size="small" label={status} sx={{ height: 17, fontSize: 8.5 }} />
        <Chip size="small" label={`${detail.files.length} files`} variant="outlined" sx={{ height: 17, fontSize: 8.5 }} />
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.45, fontSize: 8.8 }}>
        Selected artifact: {compactLabel(detail.artifactDir, 84)}
      </Typography>
      {patch?.text ? (
        <PreviewBlock title="Promotion patch preview (saved)" text={patch.text} maxChars={1400} />
      ) : null}
      {proposedContent?.text ? (
        <PreviewBlock title="Proposed content preview (saved)" text={proposedContent.text} maxChars={1400} />
      ) : null}
      {companionPayloads.length > 0 ? (
        <Typography variant="caption" color="warning.main" sx={{ display: "block", mt: 0.55, fontSize: 8.7, fontWeight: 800, lineHeight: 1.35 }}>
          Saved companion payloads: {companionPayloads.length} inert file(s), not applied by single-file apply.
        </Typography>
      ) : null}
      {companionPayloads.slice(0, 3).map((file) =>
        file.text ? (
          <PreviewBlock
            key={file.path}
            title={`Companion payload preview (saved) · ${file.path}`}
            text={file.text}
            maxChars={900}
          />
        ) : null,
      )}
      {manifest?.text ? (
        <PreviewBlock title="Manifest preview (saved)" text={manifest.text} maxChars={900} />
      ) : null}
      {applyReadiness?.text ? (
        <PreviewBlock title="Apply readiness preview (saved)" text={applyReadiness.text} maxChars={900} />
      ) : null}
      {applyReadinessJson?.text ? (
        <PreviewBlock title="Apply readiness JSON preview (saved)" text={applyReadinessJson.text} maxChars={900} />
      ) : null}
      {multiFilePlan?.text ? (
        <PreviewBlock title="Multi-file promotion plan preview (saved)" text={multiFilePlan.text} maxChars={900} />
      ) : null}
      {multiFilePlanJson?.text ? (
        <PreviewBlock title="Multi-file promotion JSON preview (saved)" text={multiFilePlanJson.text} maxChars={900} />
      ) : null}
      {readme?.text ? (
        <PreviewBlock title="Artifact README preview" text={readme.text} maxChars={700} />
      ) : null}
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.45, fontSize: 8.5, lineHeight: 1.35 }}>
        {detail.note}
      </Typography>
    </Box>
  );
}

function PromotionApplyPlanPanel({
  plan,
  savedPlanArtifact,
  applyRequest,
  applyResult,
  multiFilePlan,
  multiFilePlanArtifact,
  saving,
  requestValidating,
  applying,
  multiFilePlanning,
  multiFilePlanSaving,
  candidateConfirmation,
  targetConfirmation,
  contentHashConfirmation,
  targetHashConfirmation,
  testsConfirmed,
  branchConfirmed,
  companionFilesConfirmed,
  companionTargetPaths,
  onSavePlan,
  onCandidateConfirmationChange,
  onTargetConfirmationChange,
  onContentHashConfirmationChange,
  onTargetHashConfirmationChange,
  onTestsConfirmedChange,
  onBranchConfirmedChange,
  onCompanionFilesConfirmedChange,
  onCompanionTargetPathChange,
  onValidateApplyRequest,
  onApplyPromotion,
  onPlanMultiFilePromotion,
  onSaveMultiFilePromotionPlan,
}: {
  plan: SelfEvolutionDraftPromotionApplyPlanResponse;
  savedPlanArtifact?: SelfEvolutionDraftPromotionApplyPlanArtifactResponse | null;
  applyRequest?: SelfEvolutionDraftPromotionApplyRequestResponse | null;
  applyResult?: SelfEvolutionDraftPromotionApplyResponse | null;
  multiFilePlan?: SelfEvolutionDraftPromotionMultiFilePlanResponse | null;
  multiFilePlanArtifact?: SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse | null;
  saving?: boolean;
  requestValidating?: boolean;
  applying?: boolean;
  multiFilePlanning?: boolean;
  multiFilePlanSaving?: boolean;
  candidateConfirmation?: string;
  targetConfirmation?: string;
  contentHashConfirmation?: string;
  targetHashConfirmation?: string;
  testsConfirmed?: boolean;
  branchConfirmed?: boolean;
  companionFilesConfirmed?: boolean;
  companionTargetPaths?: Record<string, string>;
  onSavePlan?: () => void;
  onCandidateConfirmationChange?: (value: string) => void;
  onTargetConfirmationChange?: (value: string) => void;
  onContentHashConfirmationChange?: (value: string) => void;
  onTargetHashConfirmationChange?: (value: string) => void;
  onTestsConfirmedChange?: (value: boolean) => void;
  onBranchConfirmedChange?: (value: boolean) => void;
  onCompanionFilesConfirmedChange?: (value: boolean) => void;
  onCompanionTargetPathChange?: (artifactPath: string, targetPath: string) => void;
  onValidateApplyRequest?: () => void;
  onApplyPromotion?: () => void;
  onPlanMultiFilePromotion?: () => void;
  onSaveMultiFilePromotionPlan?: () => void;
}) {
  const blockedCount = plan.checks.filter(
    (check) => check.required && check.status !== "passed",
  ).length;
  const firstBlocked = plan.checks.find(
    (check) => check.required && check.status !== "passed",
  );
  return (
    <Box
      sx={{
        p: 0.75,
        borderRadius: 1,
        border: `1px solid ${alpha(blockedCount ? "#f59e0b" : "#10b981", 0.24)}`,
        bgcolor: alpha(blockedCount ? "#f59e0b" : "#10b981", 0.045),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9.5, fontWeight: 900 }}>
        Promotion apply readiness gate
      </Typography>
      <Stack direction="row" alignItems="center" justifyContent="space-between" spacing={0.75} sx={{ mt: 0.45 }}>
        <Typography variant="caption" color="text.secondary" sx={{ minWidth: 0, fontSize: 8.6, lineHeight: 1.35 }}>
          {plan.applyCommandAvailable
            ? "Explicit single-file apply is available after exact confirmations."
            : "Readiness evidence only; real apply remains unavailable."}
        </Typography>
        {onSavePlan ? (
          <Button
            size="small"
            variant="outlined"
            onClick={onSavePlan}
            disabled={saving}
            sx={{ minWidth: 106, fontSize: 8.5, py: 0.2 }}
          >
            {saving ? "保存中" : "保存 readiness"}
          </Button>
        ) : null}
      </Stack>
      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.45 }}>
        <Chip size="small" label={plan.status} sx={{ height: 17, fontSize: 8.5 }} />
        {blockedCount > 0 ? (
          <Chip
            size="small"
            label={`${blockedCount} blocked`}
            color="warning"
            variant="outlined"
            sx={{ height: 17, fontSize: 8.5 }}
          />
        ) : null}
        <Chip
          size="small"
          label={plan.applyCommandAvailable ? "apply available" : "apply unavailable"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
        <Chip
          size="small"
          label={plan.wouldWrite ? "wouldWrite=true" : "wouldWrite=false"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
      </Stack>
      {plan.proposedTargetPath ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.45, fontSize: 8.8 }}>
          apply target: {compactLabel(plan.proposedTargetPath, 84)}
        </Typography>
      ) : null}
      {plan.proposedContentPath ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25, fontSize: 8.8 }}>
          immutable payload: {compactLabel(plan.proposedContentPath, 84)}
        </Typography>
      ) : null}
      {plan.companionDrafts?.length ? (
        <Typography variant="caption" color="warning.main" sx={{ display: "block", mt: 0.3, fontSize: 8.7, lineHeight: 1.35 }}>
          Companion apply gate: {plan.companionDrafts.length} companion draft(s), {plan.companionPayloads?.length ?? 0} saved payload(s), not moved by single-file apply.
        </Typography>
      ) : null}
      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.35 }}>
        <Chip
          size="small"
          label={plan.targetExists ? "target exists" : "new target"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
        {plan.patchSha256 ? (
          <Chip
            size="small"
            label={`patch ${compactLabel(plan.patchSha256, 22)}`}
            variant="outlined"
            sx={{ height: 17, fontSize: 8.5 }}
          />
        ) : null}
        {plan.proposedContentSha256 ? (
          <Chip
            size="small"
            label={`content ${compactLabel(plan.proposedContentSha256, 22)}`}
            variant="outlined"
            sx={{ height: 17, fontSize: 8.5 }}
          />
        ) : null}
        {plan.targetCurrentSha256 ? (
          <Chip
            size="small"
            label={`target ${compactLabel(plan.targetCurrentSha256, 22)}`}
            variant="outlined"
            sx={{ height: 17, fontSize: 8.5 }}
          />
        ) : null}
      </Stack>
      <Stack component="ul" spacing={0.15} sx={{ mt: 0.5, mb: 0, pl: 1.8 }}>
        {plan.checks.slice(0, 5).map((check) => (
          <Typography key={check.id} component="li" variant="caption" color={check.status === "passed" ? "text.secondary" : "warning.main"} sx={{ display: "list-item", fontSize: 8.7, lineHeight: 1.35 }}>
            {check.label}: {check.status}
          </Typography>
        ))}
      </Stack>
      {firstBlocked ? (
        <Typography variant="caption" color="warning.main" sx={{ display: "block", mt: 0.45, fontSize: 8.7, lineHeight: 1.35 }}>
          Blocked reason: {firstBlocked.detail}
        </Typography>
      ) : null}
      {plan.requiredConfirmations.length > 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.5, fontSize: 8.7, lineHeight: 1.35 }}>
          Required confirmation: {plan.requiredConfirmations[0]}
        </Typography>
      ) : null}
      {plan.suggestedVerification.length > 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.3, fontSize: 8.7, lineHeight: 1.35 }}>
          Test gate: {plan.suggestedVerification[0]}
        </Typography>
      ) : null}
      {savedPlanArtifact ? (
        <Box sx={{ mt: 0.5 }}>
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 8.6, lineHeight: 1.35 }}>
            Saved readiness: APPLY_READINESS.md · {compactLabel(savedPlanArtifact.planReadmePath, 84)}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 8.6, lineHeight: 1.35 }}>
            JSON: apply-readiness.json · {compactLabel(savedPlanArtifact.planJsonPath, 84)}
          </Typography>
        </Box>
      ) : null}
      {plan.companionPayloads?.length ? (
        <MultiFilePromotionPlanPanel
          payloads={plan.companionPayloads}
          plan={multiFilePlan}
          savedPlanArtifact={multiFilePlanArtifact}
          targetPaths={companionTargetPaths ?? {}}
          planning={multiFilePlanning}
          saving={multiFilePlanSaving}
          onTargetPathChange={onCompanionTargetPathChange}
          onPlan={onPlanMultiFilePromotion}
          onSavePlan={onSaveMultiFilePromotionPlan}
        />
      ) : null}
      {onValidateApplyRequest ? (
        <PromotionApplyRequestGate
          request={applyRequest}
          result={applyResult}
          candidateConfirmation={candidateConfirmation}
          targetConfirmation={targetConfirmation}
          contentHashConfirmation={contentHashConfirmation}
          targetHashConfirmation={targetHashConfirmation}
          testsConfirmed={testsConfirmed}
          branchConfirmed={branchConfirmed}
          companionFilesConfirmed={companionFilesConfirmed}
          companionDraftCount={plan.companionDrafts?.length ?? 0}
          validating={requestValidating}
          applying={applying}
          onCandidateConfirmationChange={onCandidateConfirmationChange}
          onTargetConfirmationChange={onTargetConfirmationChange}
          onContentHashConfirmationChange={onContentHashConfirmationChange}
          onTargetHashConfirmationChange={onTargetHashConfirmationChange}
          onTestsConfirmedChange={onTestsConfirmedChange}
          onBranchConfirmedChange={onBranchConfirmedChange}
          onCompanionFilesConfirmedChange={onCompanionFilesConfirmedChange}
          onValidate={onValidateApplyRequest}
          onApply={onApplyPromotion}
        />
      ) : null}
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.45, fontSize: 8.5, lineHeight: 1.35 }}>
        {plan.safetyNote}
      </Typography>
    </Box>
  );
}

function MultiFilePromotionPlanPanel({
  payloads,
  plan,
  savedPlanArtifact,
  targetPaths,
  planning,
  saving,
  onTargetPathChange,
  onPlan,
  onSavePlan,
}: {
  payloads: SelfEvolutionDraftPromotionCompanionPayload[];
  plan?: SelfEvolutionDraftPromotionMultiFilePlanResponse | null;
  savedPlanArtifact?: SelfEvolutionDraftPromotionMultiFilePlanArtifactResponse | null;
  targetPaths: Record<string, string>;
  planning?: boolean;
  saving?: boolean;
  onTargetPathChange?: (artifactPath: string, targetPath: string) => void;
  onPlan?: () => void;
  onSavePlan?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const blockedCount =
    plan?.checks.filter((check) => check.required && check.status !== "passed").length ?? 0;
  const targetBlockedCount =
    plan?.companionTargets.reduce(
      (count, target) =>
        count + target.checks.filter((check) => check.required && check.status !== "passed").length,
      0,
    ) ?? 0;
  const reviewedPatchPlan = plan ? multiFilePlanPatchText(plan) : "";
  const copyReviewedPatchPlan = async () => {
    if (!reviewedPatchPlan || typeof navigator === "undefined" || !navigator.clipboard) return;
    await navigator.clipboard.writeText(reviewedPatchPlan);
    setCopied(true);
  };
  return (
    <Box
      sx={{
        mt: 0.75,
        p: 0.75,
        borderRadius: 1,
        border: `1px dashed ${alpha("#0ea5e9", 0.28)}`,
        bgcolor: alpha("#0ea5e9", 0.035),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9, fontWeight: 900 }}>
        Multi-file companion promotion plan
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 8.4, lineHeight: 1.35 }}>
        Plans explicit active targets for companion payloads; dry-run only, no companion file is written.
      </Typography>
      <Stack spacing={0.5} sx={{ mt: 0.55 }}>
        {payloads.map((payload) => (
          <TextField
            key={payload.artifactPath}
            size="small"
            label={`Companion target · ${payload.role}`}
            value={targetPaths[payload.artifactPath] ?? ""}
            onChange={(event) => onTargetPathChange?.(payload.artifactPath, event.target.value)}
            placeholder="plugins/example/templates/demo/template.sh.j2"
            helperText={`${compactLabel(payload.sourcePath, 72)} · ${compactLabel(payload.sha256, 28)}`}
            inputProps={{ style: { fontSize: 9.5, padding: "6px 8px" } }}
            InputLabelProps={{ sx: { fontSize: 9.5 } }}
            FormHelperTextProps={{ sx: { fontSize: 8.2, mt: 0.15 } }}
          />
        ))}
      </Stack>
      <Stack direction="row" spacing={0.5} sx={{ mt: 0.55 }}>
        <Button
          size="small"
          variant="outlined"
          onClick={onPlan}
          disabled={planning || saving}
          sx={{ minWidth: 116, fontSize: 8.5, py: 0.25 }}
        >
          {planning ? "规划中" : "预览 multi-file plan"}
        </Button>
        <Button
          size="small"
          variant="outlined"
          onClick={onSavePlan}
          disabled={planning || saving}
          sx={{ minWidth: 116, fontSize: 8.5, py: 0.25 }}
        >
          {saving ? "保存中" : "保存 multi-file plan"}
        </Button>
      </Stack>
      {plan ? (
        <Box sx={{ mt: 0.55 }}>
          <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
            <Chip size="small" label={plan.status} sx={{ height: 17, fontSize: 8.5 }} />
            <Chip
              size="small"
              label={`${blockedCount + targetBlockedCount} blocked`}
              color={blockedCount + targetBlockedCount > 0 ? "warning" : "default"}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
            <Chip
              size="small"
              label={plan.wouldWrite ? "wouldWrite=true" : "wouldWrite=false"}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
          </Stack>
          {plan.manifestTargetPath ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, fontSize: 8.4, lineHeight: 1.35 }}>
              Manifest target: {compactLabel(plan.manifestTargetPath, 84)}
            </Typography>
          ) : null}
          <Stack component="ul" spacing={0.12} sx={{ mt: 0.4, mb: 0, pl: 1.8 }}>
            {plan.companionTargets.slice(0, 4).map((target) => {
              const firstBlocked = target.checks.find(
                (check) => check.required && check.status !== "passed",
              );
              return (
                <Typography key={target.artifactPath} component="li" variant="caption" color={firstBlocked ? "warning.main" : "text.secondary"} sx={{ display: "list-item", fontSize: 8.4, lineHeight: 1.35 }}>
                  {target.role}: {target.proposedTargetPath ? compactLabel(target.proposedTargetPath, 72) : "missing target"}
                  {` · ${target.targetExists ? "merge/replacement" : "new file"} · ${target.diffPreview ? "diff ready" : "diff missing"}`}
                  {firstBlocked ? ` · blocked: ${firstBlocked.detail}` : ""}
                </Typography>
              );
            })}
          </Stack>
          {reviewedPatchPlan ? (
            <Box sx={{ mt: 0.5 }}>
              <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap>
                <Typography variant="caption" color="text.secondary" sx={{ fontSize: 8.5, fontWeight: 800 }}>
                  Reviewed patch guidance
                </Typography>
                <Button
                  size="small"
                  variant="text"
                  onClick={() => void copyReviewedPatchPlan()}
                  sx={{ minWidth: 0, fontSize: 8.2, py: 0.05, px: 0.5 }}
                >
                  {copied ? "Copied reviewed patch plan" : "Copy reviewed patch plan"}
                </Button>
              </Stack>
              <PreviewBlock
                title="Reviewed patch plan (copy text)"
                text={reviewedPatchPlan}
                maxChars={1200}
              />
            </Box>
          ) : null}
          {plan.companionTargets[0]?.diffPreview ? (
            <PreviewBlock
              title="Companion diff preview (multi-file dry-run)"
              text={plan.companionTargets[0].diffPreview}
              maxChars={900}
            />
          ) : null}
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, fontSize: 8.3, lineHeight: 1.35 }}>
            {plan.safetyNote}
          </Typography>
        </Box>
      ) : null}
      {savedPlanArtifact ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.4, fontSize: 8.3, lineHeight: 1.35 }}>
          Saved multi-file plan: MULTI_FILE_PROMOTION_PLAN.md · {compactLabel(savedPlanArtifact.planReadmePath, 84)}
        </Typography>
      ) : null}
    </Box>
  );
}

function PromotionApplyRequestGate({
  request,
  result,
  candidateConfirmation = "",
  targetConfirmation = "",
  contentHashConfirmation = "",
  targetHashConfirmation = "",
  testsConfirmed = false,
  branchConfirmed = false,
  companionFilesConfirmed = false,
  companionDraftCount = 0,
  validating,
  applying,
  onCandidateConfirmationChange,
  onTargetConfirmationChange,
  onContentHashConfirmationChange,
  onTargetHashConfirmationChange,
  onTestsConfirmedChange,
  onBranchConfirmedChange,
  onCompanionFilesConfirmedChange,
  onValidate,
  onApply,
}: {
  request?: SelfEvolutionDraftPromotionApplyRequestResponse | null;
  result?: SelfEvolutionDraftPromotionApplyResponse | null;
  candidateConfirmation?: string;
  targetConfirmation?: string;
  contentHashConfirmation?: string;
  targetHashConfirmation?: string;
  testsConfirmed?: boolean;
  branchConfirmed?: boolean;
  companionFilesConfirmed?: boolean;
  companionDraftCount?: number;
  validating?: boolean;
  applying?: boolean;
  onCandidateConfirmationChange?: (value: string) => void;
  onTargetConfirmationChange?: (value: string) => void;
  onContentHashConfirmationChange?: (value: string) => void;
  onTargetHashConfirmationChange?: (value: string) => void;
  onTestsConfirmedChange?: (value: boolean) => void;
  onBranchConfirmedChange?: (value: boolean) => void;
  onCompanionFilesConfirmedChange?: (value: boolean) => void;
  onValidate?: () => void;
  onApply?: () => void;
}) {
  const firstBlocked = request?.checks.find(
    (check) => check.required && check.status !== "passed",
  );
  const applyFirstBlocked = result?.checks.find(
    (check) => check.required && check.status !== "passed",
  );
  const canApply = request?.status === "ready_for_explicit_apply" && request.applyCommandAvailable;
  return (
    <Box
      sx={{
        mt: 0.75,
        p: 0.75,
        borderRadius: 1,
        border: `1px dashed ${alpha("#f59e0b", 0.3)}`,
        bgcolor: alpha("#f59e0b", 0.035),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9, fontWeight: 900 }}>
        Apply request confirmation gate
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 8.4, lineHeight: 1.35 }}>
        Validates exact confirmations before the single-file apply button is enabled.
      </Typography>
      <Stack spacing={0.55} sx={{ mt: 0.55 }}>
        <TextField
          size="small"
          label="Candidate id confirmation"
          value={candidateConfirmation}
          onChange={(event) => onCandidateConfirmationChange?.(event.target.value)}
          placeholder="Type candidate id exactly"
          inputProps={{ style: { fontSize: 9.5, padding: "6px 8px" } }}
          InputLabelProps={{ sx: { fontSize: 9.5 } }}
        />
        <TextField
          size="small"
          label="Target path confirmation"
          value={targetConfirmation}
          onChange={(event) => onTargetConfirmationChange?.(event.target.value)}
          placeholder="Type proposed target path exactly"
          inputProps={{ style: { fontSize: 9.5, padding: "6px 8px" } }}
          InputLabelProps={{ sx: { fontSize: 9.5 } }}
        />
        <TextField
          size="small"
          label="Proposed content sha256 confirmation"
          value={contentHashConfirmation}
          onChange={(event) => onContentHashConfirmationChange?.(event.target.value)}
          placeholder="Type proposed content sha256 exactly"
          inputProps={{ style: { fontSize: 9.5, padding: "6px 8px" } }}
          InputLabelProps={{ sx: { fontSize: 9.5 } }}
        />
        <TextField
          size="small"
          label="Current target sha256 confirmation"
          value={targetHashConfirmation}
          onChange={(event) => onTargetHashConfirmationChange?.(event.target.value)}
          placeholder="Required only when replacing an existing target"
          inputProps={{ style: { fontSize: 9.5, padding: "6px 8px" } }}
          InputLabelProps={{ sx: { fontSize: 9.5 } }}
        />
        <Stack direction={{ xs: "column", sm: "row" }} spacing={0.5}>
          <FormControlLabel
            control={
              <Checkbox
                size="small"
                checked={testsConfirmed}
                onChange={(event) => onTestsConfirmedChange?.(event.target.checked)}
              />
            }
            label="Tests/fixtures passed"
            sx={{ m: 0, "& .MuiFormControlLabel-label": { fontSize: 8.7 } }}
          />
          <FormControlLabel
            control={
              <Checkbox
                size="small"
                checked={branchConfirmed}
                onChange={(event) => onBranchConfirmedChange?.(event.target.checked)}
              />
            }
            label="Separate reviewed branch"
            sx={{ m: 0, "& .MuiFormControlLabel-label": { fontSize: 8.7 } }}
          />
          {companionDraftCount > 0 ? (
            <FormControlLabel
              control={
                <Checkbox
                  size="small"
                  checked={companionFilesConfirmed}
                  onChange={(event) => onCompanionFilesConfirmedChange?.(event.target.checked)}
                />
              }
              label="Companion files handled"
              sx={{ m: 0, "& .MuiFormControlLabel-label": { fontSize: 8.7 } }}
            />
          ) : null}
        </Stack>
        <Button
          size="small"
          variant="outlined"
          onClick={onValidate}
          disabled={validating}
          sx={{ alignSelf: "flex-start", minWidth: 112, fontSize: 8.5, py: 0.25 }}
        >
          {validating ? "验证中" : "验证 apply 请求"}
        </Button>
        {onApply ? (
          <Button
            size="small"
            variant="contained"
            color="warning"
            onClick={onApply}
            disabled={applying || !canApply}
            sx={{ alignSelf: "flex-start", minWidth: 128, fontSize: 8.5, py: 0.25, boxShadow: "none" }}
          >
            {applying ? "应用中" : "执行单文件 apply"}
          </Button>
        ) : null}
      </Stack>
      {request ? (
        <Box sx={{ mt: 0.55 }}>
          <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
            <Chip size="small" label={request.status} sx={{ height: 17, fontSize: 8.5 }} />
            <Chip
              size="small"
              label={request.applyCommandAvailable ? "apply available" : "apply unavailable"}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
            <Chip
              size="small"
              label={request.wouldWrite ? "wouldWrite=true" : "wouldWrite=false"}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
            <Chip
              size="small"
              label={request.targetExists ? "target exists" : "new target"}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
          </Stack>
          {request.patchSha256 ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, fontSize: 8.3, lineHeight: 1.35 }}>
              Request patch hash: {compactLabel(request.patchSha256, 64)}
            </Typography>
          ) : null}
          {request.targetCurrentSha256 ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 8.3, lineHeight: 1.35 }}>
              Request target hash: {compactLabel(request.targetCurrentSha256, 64)}
            </Typography>
          ) : null}
          {request.proposedContentSha256 ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 8.3, lineHeight: 1.35 }}>
              Request proposed content hash: {compactLabel(request.proposedContentSha256, 64)}
            </Typography>
          ) : null}
          <Stack component="ul" spacing={0.12} sx={{ mt: 0.4, mb: 0, pl: 1.8 }}>
            {request.checks.slice(-5).map((check) => (
              <Typography key={check.id} component="li" variant="caption" color={check.status === "passed" ? "text.secondary" : "warning.main"} sx={{ display: "list-item", fontSize: 8.5, lineHeight: 1.35 }}>
                {check.label}: {check.status}
              </Typography>
            ))}
          </Stack>
          {firstBlocked ? (
            <Typography variant="caption" color="warning.main" sx={{ display: "block", mt: 0.4, fontSize: 8.5, lineHeight: 1.35 }}>
              Apply request blocked: {firstBlocked.detail}
            </Typography>
          ) : null}
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.35, fontSize: 8.3, lineHeight: 1.35 }}>
            {request.safetyNote}
          </Typography>
        </Box>
      ) : null}
      {result ? (
        <Box sx={{ mt: 0.55 }}>
          <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
            <Chip size="small" label={result.status} sx={{ height: 17, fontSize: 8.5 }} />
            <Chip
              size="small"
              label={result.applied ? "applied=true" : "applied=false"}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
            <Chip
              size="small"
              label={`${result.bytesWritten} bytes`}
              variant="outlined"
              sx={{ height: 17, fontSize: 8.5 }}
            />
          </Stack>
          {result.targetNewSha256 ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.3, fontSize: 8.3, lineHeight: 1.35 }}>
              Applied target hash: {compactLabel(result.targetNewSha256, 64)}
            </Typography>
          ) : null}
          {applyFirstBlocked ? (
            <Typography variant="caption" color="warning.main" sx={{ display: "block", mt: 0.3, fontSize: 8.3, lineHeight: 1.35 }}>
              Apply blocked: {applyFirstBlocked.detail}
            </Typography>
          ) : null}
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.3, fontSize: 8.3, lineHeight: 1.35 }}>
            {result.safetyNote}
          </Typography>
        </Box>
      ) : null}
    </Box>
  );
}

function SelfEvolutionDraftRow({
  draft,
  selected,
  onClick,
}: {
  draft: SelfEvolutionDraftSummary;
  selected: boolean;
  onClick?: () => void;
}) {
  const companionCount = draft.companionDrafts?.length ?? 0;
  return (
    <Box
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") onClick?.();
      }}
      sx={{
        p: 0.75,
        borderRadius: 1.25,
        border: `1px solid ${alpha(selected ? ACCENT : "#64748b", selected ? 0.34 : 0.16)}`,
        bgcolor: selected ? alpha(ACCENT, 0.075) : alpha("#64748b", 0.025),
        cursor: "pointer",
      }}
    >
      <Stack direction="row" spacing={0.5} alignItems="center" flexWrap="wrap" useFlexGap>
        <Chip size="small" label={draft.kind} sx={{ height: 16, fontSize: 8.5 }} />
        {draft.priority ? (
          <Chip size="small" label={draft.priority} variant="outlined" sx={{ height: 16, fontSize: 8.5 }} />
        ) : null}
        {draft.specializedDrafts.length > 0 ? (
          <Chip size="small" label={`${draft.specializedDrafts.length} .draft`} variant="outlined" sx={{ height: 16, fontSize: 8.5 }} />
        ) : null}
        {companionCount > 0 ? (
          <Chip size="small" label={`${companionCount} companion`} variant="outlined" sx={{ height: 16, fontSize: 8.5 }} />
        ) : null}
        {isCreatorDraft(null, draft.createdBy) ? (
          <Chip size="small" label="creator package" variant="outlined" sx={{ height: 16, fontSize: 8.5 }} />
        ) : null}
      </Stack>
      <Typography variant="caption" sx={{ display: "block", mt: 0.3, fontSize: 10.5, fontWeight: 700 }}>
        {compactLabel(draft.title ?? draft.candidateId, 56)}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 9, lineHeight: 1.35 }}>
        {compactLabel(draft.draftDir, 64)}
      </Typography>
    </Box>
  );
}

function SelfEvolutionDraftDetailPanel({
  detail,
  promotionPreview,
  promotionArtifact,
  promotionTargetPath,
  promotionLoading,
  artifactSaving,
  onPromotionTargetPathChange,
  onPreviewPromotionTarget,
  onSavePromotionArtifact,
}: {
  detail: SelfEvolutionDraftDetailResponse;
  promotionPreview?: SelfEvolutionDraftPromotionPreviewResponse | null;
  promotionArtifact?: SelfEvolutionDraftPromotionArtifactResponse | null;
  promotionTargetPath?: string;
  promotionLoading?: boolean;
  artifactSaving?: boolean;
  onPromotionTargetPathChange?: (targetPath: string) => void;
  onPreviewPromotionTarget?: () => void;
  onSavePromotionArtifact?: () => void;
}) {
  const checklist = detail.files.find((file) => file.role === "review_checklist")?.text;
  const draftFile = detail.files.find(
    (file) => file.role === "template_draft" || file.role === "operator_draft",
  ) ?? detail.files.find((file) => file.path.endsWith(".draft"));
  const companionDrafts = detail.files.filter(isCompanionDraftFile);
  const creatorPackage = isCreatorDraft(detail.candidate);
  return (
    <Box
      sx={{
        mt: 0.25,
        p: 1,
        borderRadius: 1.25,
        border: `1px solid ${alpha(ACCENT, 0.18)}`,
        bgcolor: alpha(ACCENT, 0.04),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 10, fontWeight: 800 }}>
        草稿详情 · {compactLabel(detail.reviewPreview.title ?? detail.reviewPreview.candidateId ?? detail.draftDir, 42)}
      </Typography>
      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.45 }}>
        <Chip size="small" label={detail.reviewPreview.status} sx={{ height: 17, fontSize: 8.5 }} />
        {detail.reviewPreview.kind ? (
          <Chip size="small" label={detail.reviewPreview.kind} variant="outlined" sx={{ height: 17, fontSize: 8.5 }} />
        ) : null}
        {creatorPackage ? (
          <Chip size="small" label="creator package" variant="outlined" sx={{ height: 17, fontSize: 8.5 }} />
        ) : null}
        {companionDrafts.length > 0 ? (
          <Chip size="small" label={`${companionDrafts.length} companion drafts`} variant="outlined" sx={{ height: 17, fontSize: 8.5 }} />
        ) : null}
      </Stack>
      {detail.reviewPreview.targetHint ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.55, fontSize: 9.5 }}>
          Target hint: {detail.reviewPreview.targetHint}
        </Typography>
      ) : null}
      {promotionLoading ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.55, fontSize: 9.5 }}>
          正在生成 promotion patch dry-run…
        </Typography>
      ) : null}
      <Stack direction={{ xs: "column", sm: "row" }} spacing={0.6} alignItems={{ xs: "stretch", sm: "flex-start" }} sx={{ mt: 0.65 }}>
        <TextField
          size="small"
          label="Optional targetPath"
          value={promotionTargetPath ?? ""}
          onChange={(event) => onPromotionTargetPathChange?.(event.target.value)}
          placeholder="plugins/example/templates/demo/template.yaml"
          helperText="Project-relative; dry-run only."
          fullWidth
          inputProps={{ style: { fontSize: 10, padding: "6px 8px" } }}
          InputLabelProps={{ sx: { fontSize: 10 } }}
          FormHelperTextProps={{ sx: { fontSize: 8.5, mt: 0.2 } }}
        />
        <Button
          size="small"
          variant="outlined"
          onClick={onPreviewPromotionTarget}
          disabled={promotionLoading || artifactSaving}
          sx={{ minWidth: 92, fontSize: 9.5, py: 0.45 }}
        >
          {promotionLoading ? "预览中" : "预览 target"}
        </Button>
        <Button
          size="small"
          variant="contained"
          onClick={onSavePromotionArtifact}
          disabled={promotionLoading || artifactSaving}
          sx={{ minWidth: 108, fontSize: 9.5, py: 0.45, boxShadow: "none" }}
        >
          {artifactSaving ? "保存中" : "保存审阅 patch"}
        </Button>
      </Stack>
      {promotionPreview ? (
        <PromotionPreviewPanel preview={promotionPreview} />
      ) : null}
      {promotionArtifact ? (
        <PromotionArtifactPanel artifact={promotionArtifact} />
      ) : null}
      <Stack component="ul" spacing={0.15} sx={{ mt: 0.55, mb: 0, pl: 1.8 }}>
        {detail.reviewPreview.actions.slice(0, 5).map((action) => (
          <Typography key={action} component="li" variant="caption" color="text.secondary" sx={{ display: "list-item", fontSize: 9, lineHeight: 1.35 }}>
            {action}
          </Typography>
        ))}
      </Stack>
      {checklist ? (
        <PreviewBlock title="Checklist preview" text={checklist} maxChars={700} />
      ) : null}
      {draftFile?.text ? (
        <PreviewBlock title={`Primary draft file · ${draftFile.path}`} text={draftFile.text} maxChars={900} />
      ) : null}
      {companionDrafts.length > 0 ? (
        <CompanionDraftChecklist files={companionDrafts} />
      ) : null}
      {companionDrafts.slice(0, 3).map((file) =>
        file.text ? (
          <PreviewBlock
            key={file.path}
            title={`Companion draft · ${file.path}`}
            text={file.text}
            maxChars={700}
          />
        ) : null,
      )}
      {detail.reviewPreview.diffPreview ? (
        <PreviewBlock title="Diff preview (review-only)" text={detail.reviewPreview.diffPreview} maxChars={1200} />
      ) : null}
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.65, fontSize: 8.8 }}>
        {detail.note}
      </Typography>
    </Box>
  );
}

function CompanionDraftChecklist({
  files,
}: {
  files: SelfEvolutionDraftFilePreview[];
}) {
  return (
    <Box sx={{ mt: 0.75 }}>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 0.25, fontSize: 8.8, fontWeight: 800 }}>
        Companion draft checklist
      </Typography>
      <Box
        sx={{
          p: 0.65,
          borderRadius: 1,
          border: `1px dashed ${alpha("#f59e0b", 0.28)}`,
          bgcolor: alpha("#f59e0b", 0.04),
        }}
      >
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", fontSize: 8.6, lineHeight: 1.35 }}>
          Single-file promotion writes only the primary manifest target; move companion scripts,
          fixtures, examples, or template entries through a separate reviewed patch.
        </Typography>
        <Stack component="ul" spacing={0.12} sx={{ mt: 0.35, mb: 0, pl: 1.6 }}>
          {files.map((file) => (
            <Typography key={file.path} component="li" variant="caption" color="text.secondary" sx={{ display: "list-item", fontSize: 8.5, lineHeight: 1.35 }}>
              {file.role}: {compactLabel(file.path, 86)}
            </Typography>
          ))}
        </Stack>
      </Box>
    </Box>
  );
}

function PromotionArtifactPanel({
  artifact,
}: {
  artifact: SelfEvolutionDraftPromotionArtifactResponse;
}) {
  return (
    <Box
      sx={{
        mt: 0.75,
        p: 0.75,
        borderRadius: 1,
        border: `1px solid ${alpha("#10b981", 0.28)}`,
        bgcolor: alpha("#10b981", 0.055),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9.5, fontWeight: 900 }}>
        Saved promotion review artifact
      </Typography>
      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.45 }}>
        <Chip size="small" label={artifact.status} sx={{ height: 17, fontSize: 8.5 }} />
        <Chip
          size="small"
          label={artifact.wouldWrite ? "wouldWrite=true" : "wouldWrite=false"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
        <Chip
          size="small"
          label={artifact.applied ? "applied=true" : "not applied"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.5, fontSize: 9.1 }}>
        Artifact dir: {artifact.artifactDir}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 9.1 }}>
        Patch: {artifact.patchPath}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 9.1 }}>
        Manifest: {artifact.manifestPath}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 9.1 }}>
        Proposed content: {artifact.proposedContentPath}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.2, fontSize: 9.1 }}>
        Proposed content sha256: {compactLabel(artifact.proposedContentSha256, 72)}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.45, fontSize: 8.6, lineHeight: 1.35 }}>
        {artifact.safetyNote}
      </Typography>
    </Box>
  );
}

function PromotionPreviewPanel({
  preview,
}: {
  preview: SelfEvolutionDraftPromotionPreviewResponse;
}) {
  return (
    <Box
      sx={{
        mt: 0.75,
        p: 0.8,
        borderRadius: 1,
        border: `1px dashed ${alpha(ACCENT, 0.28)}`,
        bgcolor: alpha("#f59e0b", 0.045),
      }}
    >
      <Typography variant="caption" sx={{ display: "block", fontSize: 9.5, fontWeight: 900 }}>
        Promotion patch dry-run
      </Typography>
      <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap sx={{ mt: 0.45 }}>
        <Chip size="small" label={preview.status} sx={{ height: 17, fontSize: 8.5 }} />
        <Chip
          size="small"
          label={preview.wouldWrite ? "wouldWrite=true" : "wouldWrite=false"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
        <Chip
          size="small"
          label={preview.applied ? "applied=true" : "not applied"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
        <Chip
          size="small"
          label={preview.targetExists ? "target exists" : "new target"}
          variant="outlined"
          sx={{ height: 17, fontSize: 8.5 }}
        />
      </Stack>
      {preview.proposedTargetPath ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.55, fontSize: 9.2 }}>
          Proposed target: {preview.proposedTargetPath}
        </Typography>
      ) : null}
      {preview.draftFile ? (
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.25, fontSize: 9.2 }}>
          Draft source: {preview.draftFile}
        </Typography>
      ) : null}
      {preview.requiredReviewSteps.length > 0 ? (
        <Stack component="ul" spacing={0.15} sx={{ mt: 0.5, mb: 0, pl: 1.8 }}>
          {preview.requiredReviewSteps.slice(0, 5).map((step) => (
            <Typography key={step} component="li" variant="caption" color="text.secondary" sx={{ display: "list-item", fontSize: 8.8, lineHeight: 1.35 }}>
              {step}
            </Typography>
          ))}
        </Stack>
      ) : null}
      {preview.companionDrafts?.length ? (
        <Box sx={{ mt: 0.55 }}>
          <Typography variant="caption" color="warning.main" sx={{ display: "block", fontSize: 8.8, fontWeight: 800, lineHeight: 1.35 }}>
            Companion files require separate review before promotion
          </Typography>
          <Stack component="ul" spacing={0.12} sx={{ mt: 0.25, mb: 0, pl: 1.8 }}>
            {preview.companionDrafts.slice(0, 4).map((path) => (
              <Typography key={path} component="li" variant="caption" color="text.secondary" sx={{ display: "list-item", fontSize: 8.5, lineHeight: 1.35 }}>
                {compactLabel(path, 86)}
              </Typography>
            ))}
          </Stack>
          {preview.companionReviewSteps?.[0] ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.3, fontSize: 8.5, lineHeight: 1.35 }}>
              {preview.companionReviewSteps[0]}
            </Typography>
          ) : null}
        </Box>
      ) : null}
      {preview.riskNotes.length > 0 ? (
        <Typography variant="caption" color="warning.main" sx={{ display: "block", mt: 0.55, fontSize: 8.8, lineHeight: 1.35 }}>
          Risk: {preview.riskNotes[0]}
        </Typography>
      ) : null}
      {preview.diffPreview ? (
        <PreviewBlock title="Promotion diff preview (dry-run)" text={preview.diffPreview} maxChars={1400} />
      ) : null}
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mt: 0.55, fontSize: 8.6 }}>
        {preview.safetyNote}
      </Typography>
    </Box>
  );
}

function PreviewBlock({
  title,
  text,
  maxChars,
}: {
  title: string;
  text: string;
  maxChars: number;
}) {
  const truncated = text.length > maxChars;
  const preview = truncated ? `${text.slice(0, maxChars)}\n… truncated …` : text;
  return (
    <Box sx={{ mt: 0.75 }}>
      <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 0.25, fontSize: 8.8, fontWeight: 800 }}>
        {title}
      </Typography>
      <Box
        component="pre"
        sx={{
          m: 0,
          p: 0.75,
          borderRadius: 1,
          bgcolor: alpha("#0f172a", 0.06),
          fontSize: 9,
          lineHeight: 1.35,
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          maxHeight: 220,
          overflow: "auto",
        }}
      >
        {preview}
      </Box>
    </Box>
  );
}
