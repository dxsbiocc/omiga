import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SelfEvolutionDraftBrowserView } from "./SelfEvolutionDraftBrowser";

describe("SelfEvolutionDraftBrowserView", () => {
  it("renders draft batches, checklist, draft file, and review-only diff preview", () => {
    const html = renderToStaticMarkup(
      <SelfEvolutionDraftBrowserView
        response={{
          rootDir: ".omiga/learning/self-evolution-drafts",
          batchCount: 1,
          note: "review-only",
          batches: [
            {
              batchDir: ".omiga/learning/self-evolution-drafts/draft-batch-1",
              indexPath: ".omiga/learning/self-evolution-drafts/draft-batch-1/README.md",
              generatedAt: "2026-05-10T00:00:00Z",
              draftCount: 1,
              drafts: [
                {
                  draftDir:
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo",
                  candidateId: "candidate-template-demo",
                  kind: "template_candidate",
                  title: "Reusable DE workflow",
                  priority: "medium",
                  createdBy: "learning_self_evolution_creator",
                  files: [
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/DRAFT.md",
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.yaml.draft",
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
                  ],
                  specializedDrafts: [
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.yaml.draft",
                  ],
                  companionDrafts: [
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
                    ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
                  ],
                },
              ],
            },
          ],
        }}
        selectedDraftDir=".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo"
        detail={{
          found: true,
          draftDir: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo",
          note: "Read-only self-evolution draft review.",
          candidate: {
            id: "candidate-template-demo",
            kind: "template_candidate",
            title: "Reusable DE workflow",
            evidence: {
              createdBy: "learning_self_evolution_creator",
            },
          },
          files: [
            {
              path: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/DRAFT.md",
              role: "review_checklist",
              bytes: 120,
              truncated: false,
              text: "- [ ] Confirm this candidate is still relevant.",
            },
            {
              path: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.yaml.draft",
              role: "template_draft",
              bytes: 80,
              truncated: false,
              text: "apiVersion: omiga.ai/template/v1alpha1\nkind: Template",
            },
            {
              path: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
              role: "template_entry_draft",
              bytes: 64,
              truncated: false,
              text: "#!/usr/bin/env bash\ncat input.tsv > outputs.json",
            },
            {
              path: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
              role: "template_example_input_draft",
              bytes: 24,
              truncated: false,
              text: "sample\tvalue\nA\t1\n",
            },
          ],
          reviewPreview: {
            status: "review_only",
            safetyNote: "Never apply automatically.",
            candidateId: "candidate-template-demo",
            kind: "template_candidate",
            title: "Reusable DE workflow",
            targetHint: "Manual target after review: plugin templates/<id>/template.yaml",
            actions: [
              "Inspect DRAFT.md checklist and candidate.json provenance.",
              "Review companion .draft files; single-file promotion apply writes only the selected manifest file and will not move scripts, fixtures, examples, or template entries.",
              "Apply only through a separate explicit reviewed patch.",
            ],
            diffPreview:
              "# REVIEW PREVIEW ONLY — not applied\n--- /dev/null\n+++ review-target/template.yaml\n+kind: Template",
          },
        }}
        promotionPreview={{
          status: "dry_run",
          safetyNote: "Promotion patch dry-run only; no files are written.",
          draftDir: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo",
          candidateId: "candidate-template-demo",
          kind: "template_candidate",
          title: "Reusable DE workflow",
          draftFile:
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.yaml.draft",
          proposedTargetPath:
            ".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml",
          targetExists: false,
          diffPreview:
            "# PROMOTION PATCH DRY-RUN — not applied\n--- /dev/null\n+++ .omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml\n+kind: Template",
          companionDrafts: [
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
          ],
          companionReviewSteps: [
            "Review 2 companion draft file(s) before promotion; the current single-file apply writes only the selected manifest target.",
          ],
          riskNotes: [
            "Dry-run only: no filesystem write, unit registration, default update, archive mutation, or apply step was performed.",
            "Companion draft files are present; this promotion preview/artifact/apply path only carries the selected manifest payload.",
          ],
          requiredReviewSteps: [
            "Review candidate.json provenance, source ExecutionRecords, and DRAFT.md checklist.",
            "Review companion draft files and plan their separate active-plugin locations; single-file apply will not move them.",
            "Apply changes only through a separate explicit reviewed commit or patch.",
          ],
          wouldWrite: false,
          applied: false,
        }}
        promotionArtifact={{
          status: "artifact_saved",
          safetyNote:
            "Promotion review artifact only; no proposed target files are written.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          patchPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/promotion.patch",
          manifestPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/manifest.json",
          readmePath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/README.md",
          proposedContentPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/proposed-target.content",
          proposedContentSha256: "sha256:content",
          companionPayloads: [
            {
              sourcePath:
                ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
              artifactPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/01-template.sh.j2.draft.content",
              role: "template_entry_draft",
              bytes: 64,
              sha256: "sha256:companion-script",
            },
            {
              sourcePath:
                ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
              artifactPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/02-example-input.tsv.draft.content",
              role: "template_example_input_draft",
              bytes: 24,
              sha256: "sha256:companion-example",
            },
          ],
          proposedTargetPath:
            ".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml",
          preview: {
            status: "dry_run",
            safetyNote: "Promotion patch dry-run only; no files are written.",
            draftDir: ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo",
            candidateId: "candidate-template-demo",
            kind: "template_candidate",
            title: "Reusable DE workflow",
            draftFile:
              ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.yaml.draft",
            proposedTargetPath:
              ".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml",
            targetExists: false,
            diffPreview:
              "# PROMOTION PATCH DRY-RUN — not applied\n--- /dev/null\n+++ .omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml\n+kind: Template",
            companionDrafts: [
              ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
              ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
            ],
            companionReviewSteps: [
              "Review 2 companion draft file(s) before promotion; the current single-file apply writes only the selected manifest target.",
            ],
            riskNotes: [],
            requiredReviewSteps: [],
            wouldWrite: false,
            applied: false,
          },
          wouldWrite: false,
          applied: false,
        }}
        artifactList={{
          rootDir: ".omiga/review/self-evolution-promotions/artifacts",
          artifactCount: 1,
          note: "review artifacts only",
          artifacts: [
            {
              artifactDir:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
              patchPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/promotion.patch",
              manifestPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/manifest.json",
              readmePath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/README.md",
              proposedContentPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/proposed-target.content",
              proposedContentSha256: "sha256:content",
              candidateId: "candidate-template-demo",
              kind: "template_candidate",
              title: "Reusable DE workflow",
              proposedTargetPath:
                "plugins/demo/templates/reusable-de/template.yaml",
              targetExists: false,
              modifiedAtMillis: 1778391438000,
            },
          ],
        }}
        selectedArtifactDir=".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo"
        promotionArtifactDetail={{
          found: true,
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          manifest: {
            status: "artifact_saved",
            wouldWrite: false,
            applied: false,
          },
          files: [
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/promotion.patch",
              role: "promotion_patch",
              bytes: 120,
              truncated: false,
              text: "# PROMOTION PATCH DRY-RUN — not applied\n+++ plugins/demo/templates/reusable-de/template.yaml",
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/proposed-target.content",
              role: "promotion_proposed_content",
              bytes: 90,
              truncated: false,
              text: "apiVersion: omiga.ai/template/v1alpha1\nkind: Template\nmetadata:\n  id: reusable-de",
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/01-template.sh.j2.draft.content",
              role: "promotion_companion_payload",
              bytes: 64,
              truncated: false,
              text: "#!/usr/bin/env bash\ncat input.tsv > outputs.json",
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/02-example-input.tsv.draft.content",
              role: "promotion_companion_payload",
              bytes: 24,
              truncated: false,
              text: "sample\tvalue\nA\t1\n",
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/manifest.json",
              role: "promotion_manifest",
              bytes: 80,
              truncated: false,
              text: '{ "status": "artifact_saved", "wouldWrite": false }',
              json: { status: "artifact_saved" },
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/APPLY_READINESS.md",
              role: "promotion_apply_readiness",
              bytes: 120,
              truncated: false,
              text: "# Self-Evolution Promotion Apply Readiness\n- Status: `ready_for_explicit_apply_review`",
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/apply-readiness.json",
              role: "promotion_apply_readiness_json",
              bytes: 90,
              truncated: false,
              text: '{ "status": "apply_readiness_saved" }',
              json: { status: "apply_readiness_saved" },
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/MULTI_FILE_PROMOTION_PLAN.md",
              role: "promotion_multi_file_plan",
              bytes: 160,
              truncated: false,
              text: "# Self-Evolution Multi-File Promotion Plan\n\n- Status: `ready_for_reviewed_multi_file_patch`\n\n## Reviewed patch application checklist\n\n- [ ] Copy reviewed payload",
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/multi-file-promotion-plan.json",
              role: "promotion_multi_file_plan_json",
              bytes: 120,
              truncated: false,
              text: '{ "status": "multi_file_plan_saved" }',
              json: { status: "multi_file_plan_saved" },
            },
            {
              path: ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/README.md",
              role: "batch_index",
              bytes: 80,
              truncated: false,
              text: "# Self-Evolution Promotion Review Artifact",
            },
          ],
          note: "Promotion review artifact only.",
        }}
        promotionApplyPlan={{
          status: "ready_for_explicit_apply_review",
          safetyNote:
            "Apply readiness plan only. This command never writes the proposed target.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          patchPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/promotion.patch",
          manifestPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/manifest.json",
          proposedContentPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/proposed-target.content",
          proposedTargetPath:
            "plugins/demo/templates/reusable-de/template.yaml",
          candidateId: "candidate-template-demo",
          kind: "template_candidate",
          title: "Reusable DE workflow",
          patchSha256: "sha256:patch",
          proposedContentSha256: "sha256:content",
          targetExists: true,
          targetCurrentSha256: "sha256:target",
          companionDrafts: [
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
          ],
          companionPayloads: [
            {
              sourcePath:
                ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
              artifactPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/01-template.sh.j2.draft.content",
              role: "template_entry_draft",
              bytes: 64,
              sha256: "sha256:companion-script",
            },
            {
              sourcePath:
                ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
              artifactPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/02-example-input.tsv.draft.content",
              role: "template_example_input_draft",
              bytes: 24,
              sha256: "sha256:companion-example",
            },
          ],
          checks: [
            {
              id: "manifest_readable",
              label: "Artifact manifest is readable",
              status: "passed",
              required: true,
              detail: "manifest.json parsed successfully",
            },
            {
              id: "target_not_review_holding_path",
              label: "Proposed target is not the inert review holding path",
              status: "passed",
              required: true,
              detail: "target is outside review holding area",
            },
          ],
          requiredConfirmations: [
            "Type candidate id exactly: candidate-template-demo",
          ],
          suggestedVerification: [
            "Review promotion.patch and manifest.json in a separate branch.",
          ],
          applyCommandAvailable: true,
          wouldWrite: false,
          applied: false,
        }}
        promotionApplyPlanArtifact={{
          status: "apply_readiness_saved",
          safetyNote: "Apply readiness review artifact only.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          planJsonPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/apply-readiness.json",
          planReadmePath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/APPLY_READINESS.md",
          plan: {
            status: "ready_for_explicit_apply_review",
            safetyNote:
              "Apply readiness plan only. This command never writes the proposed target.",
            artifactDir:
              ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
            proposedTargetPath:
              "plugins/demo/templates/reusable-de/template.yaml",
            patchSha256: "sha256:patch",
            proposedContentPath:
              ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/proposed-target.content",
            proposedContentSha256: "sha256:content",
            targetExists: true,
            targetCurrentSha256: "sha256:target",
            companionDrafts: [
              ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
              ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
            ],
            companionPayloads: [
              {
                sourcePath:
                  ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
                artifactPath:
                  ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/01-template.sh.j2.draft.content",
                role: "template_entry_draft",
                bytes: 64,
                sha256: "sha256:companion-script",
              },
              {
                sourcePath:
                  ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
                artifactPath:
                  ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/02-example-input.tsv.draft.content",
                role: "template_example_input_draft",
                bytes: 24,
                sha256: "sha256:companion-example",
              },
            ],
            checks: [],
            requiredConfirmations: [],
            suggestedVerification: [],
            applyCommandAvailable: true,
            wouldWrite: false,
            applied: false,
          },
          wouldWrite: false,
          applied: false,
        }}
        multiFilePlan={{
          status: "ready_for_reviewed_multi_file_patch",
          safetyNote:
            "Multi-file promotion plan only. This command computes explicit companion target review evidence and never writes active targets.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          manifestTargetPath:
            "plugins/demo/templates/reusable-de/template.yaml",
          companionTargets: [
            {
              sourcePath:
                ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
              artifactPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/01-template.sh.j2.draft.content",
              role: "template_entry_draft",
              bytes: 64,
              sha256: "sha256:companion-script",
              proposedTargetPath:
                "plugins/demo/templates/reusable-de/template.sh.j2",
              targetExists: false,
              diffPreview:
                "# PROMOTION PATCH DRY-RUN — not applied\n--- /dev/null\n+++ plugins/demo/templates/reusable-de/template.sh.j2\n+cat input.tsv > outputs.json",
              checks: [
                {
                  id: "companion_payload_verified",
                  label: "Companion payload is present and hash-verified",
                  status: "passed",
                  required: true,
                  detail: "companion payload exists",
                },
              ],
            },
            {
              sourcePath:
                ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
              artifactPath:
                ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/02-example-input.tsv.draft.content",
              role: "template_example_input_draft",
              bytes: 24,
              sha256: "sha256:companion-example",
              proposedTargetPath:
                "plugins/demo/templates/reusable-de/example-input.tsv",
              targetExists: false,
              checks: [],
            },
          ],
          checks: [
            {
              id: "companion_targets_unique",
              label: "Companion target paths are unique",
              status: "passed",
              required: true,
              detail: "all companion payloads have unique explicit targets",
            },
          ],
          requiredReviewSteps: [
            "Review the manifest promotion artifact and each companion target diff together.",
          ],
          suggestedVerification: [
            "Confirm companion scripts, fixtures, examples, and template entries are located beside the promoted manifest as expected.",
          ],
          applyCommandAvailable: false,
          wouldWrite: false,
          applied: false,
        }}
        multiFilePlanArtifact={{
          status: "multi_file_plan_saved",
          safetyNote: "Multi-file promotion plan review artifact only.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          planJsonPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/multi-file-promotion-plan.json",
          planReadmePath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/MULTI_FILE_PROMOTION_PLAN.md",
          plan: {
            status: "ready_for_reviewed_multi_file_patch",
            safetyNote: "Multi-file promotion plan only.",
            artifactDir:
              ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
            manifestTargetPath:
              "plugins/demo/templates/reusable-de/template.yaml",
            companionTargets: [],
            checks: [],
            requiredReviewSteps: [],
            suggestedVerification: [],
            applyCommandAvailable: false,
            wouldWrite: false,
            applied: false,
          },
          wouldWrite: false,
          applied: false,
        }}
        promotionApplyRequest={{
          status: "ready_for_explicit_apply",
          safetyNote:
            "Apply request validation only. This command checks explicit confirmations and never writes the proposed target.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          proposedTargetPath:
            "plugins/demo/templates/reusable-de/template.yaml",
          candidateId: "candidate-template-demo",
          kind: "template_candidate",
          title: "Reusable DE workflow",
          patchSha256: "sha256:patch",
          proposedContentSha256: "sha256:content",
          targetExists: true,
          targetCurrentSha256: "sha256:target",
          companionDrafts: [
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
          ],
          checks: [
            {
              id: "candidate_id_confirmation_exact",
              label: "Candidate id confirmation is exact",
              status: "passed",
              required: true,
              detail: "candidate id was typed exactly",
            },
            {
              id: "target_path_confirmation_exact",
              label: "Target path confirmation is exact",
              status: "passed",
              required: true,
              detail: "target path was typed exactly",
            },
            {
              id: "deterministic_tests_confirmed",
              label: "Deterministic tests or fixtures are confirmed",
              status: "passed",
              required: true,
              detail: "reviewer confirmed deterministic tests/fixtures passed",
            },
            {
              id: "reviewed_branch_confirmed",
              label: "Separate reviewed branch or commit is confirmed",
              status: "passed",
              required: true,
              detail: "reviewer confirmed a separate reviewed branch/commit",
            },
          ],
          requiredConfirmations: [
            "Type candidate id exactly: candidate-template-demo",
          ],
          suggestedVerification: [
            "Review promotion.patch and manifest.json in a separate branch.",
          ],
          applyCommandAvailable: true,
          wouldWrite: false,
          applied: false,
        }}
        promotionApplyResult={{
          status: "applied",
          safetyNote:
            "Explicit promotion apply wrote exactly one confirmed target file from immutable proposed-target.content.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          proposedContentPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/proposed-target.content",
          proposedTargetPath:
            "plugins/demo/templates/reusable-de/template.yaml",
          candidateId: "candidate-template-demo",
          kind: "template_candidate",
          title: "Reusable DE workflow",
          proposedContentSha256: "sha256:content",
          targetExistsBefore: true,
          targetPreviousSha256: "sha256:target",
          targetNewSha256: "sha256:new-target",
          companionDrafts: [
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/template.sh.j2.draft",
            ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template-demo/example-input.tsv.draft",
          ],
          bytesWritten: 86,
          checks: [
            {
              id: "proposed_content_payload_reverified",
              label:
                "Immutable proposed content payload is reverified immediately before write",
              status: "passed",
              required: true,
              detail: "proposed-target.content was re-read and matches saved sha256",
            },
          ],
          suggestedVerification: [
            "Run template discovery/execution tests for the promoted template and any migration target.",
          ],
          applyCommandAvailable: true,
          wouldWrite: true,
          applied: true,
        }}
        promotionTargetPath="plugins/demo/templates/reusable-de/template.yaml"
        companionTargetPaths={{
          ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/01-template.sh.j2.draft.content":
            "plugins/demo/templates/reusable-de/template.sh.j2",
          ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/companion-payloads/02-example-input.tsv.draft.content":
            "plugins/demo/templates/reusable-de/example-input.tsv",
        }}
        applyCandidateConfirmation="candidate-template-demo"
        applyTargetConfirmation="plugins/demo/templates/reusable-de/template.yaml"
        applyContentHashConfirmation="sha256:content"
        applyTargetHashConfirmation="sha256:target"
        applyTestsConfirmed
        applyBranchConfirmed
        applyCompanionFilesConfirmed
        applyPlanSaving={false}
        applyRequestValidating={false}
        applySubmitting={false}
        onPromotionTargetPathChange={() => undefined}
        onPreviewPromotionTarget={() => undefined}
        onSavePromotionArtifact={() => undefined}
        onSelectPromotionArtifact={() => undefined}
        onSavePromotionApplyPlan={() => undefined}
        onApplyCandidateConfirmationChange={() => undefined}
        onApplyTargetConfirmationChange={() => undefined}
        onApplyContentHashConfirmationChange={() => undefined}
        onApplyTargetHashConfirmationChange={() => undefined}
        onApplyTestsConfirmedChange={() => undefined}
        onApplyBranchConfirmedChange={() => undefined}
        onApplyCompanionFilesConfirmedChange={() => undefined}
        onCompanionTargetPathChange={() => undefined}
        onValidatePromotionApplyRequest={() => undefined}
        onApplyPromotion={() => undefined}
        onPlanMultiFilePromotion={() => undefined}
        onSaveMultiFilePromotionPlan={() => undefined}
      />,
    );

    expect(html).toContain("Self-evolution Draft Review");
    expect(html).toContain("只读审阅草稿");
    expect(html).toContain("1 batches");
    expect(html).toContain("1 drafts");
    expect(html).toContain("1 review artifacts");
    expect(html).toContain("Saved promotion review artifacts");
    expect(html).toContain("selected");
    expect(html).toContain("Saved artifact detail (read-only)");
    expect(html).toContain("Selected artifact");
    expect(html).toContain("Promotion patch preview (saved)");
    expect(html).toContain("Proposed content preview (saved)");
    expect(html).toContain("Saved companion payloads");
    expect(html).toContain("Companion payload preview (saved)");
    expect(html).toContain("companion-payloads");
    expect(html).toContain("Manifest preview (saved)");
    expect(html).toContain("Apply readiness preview (saved)");
    expect(html).toContain("Apply readiness JSON preview (saved)");
    expect(html).toContain("Multi-file promotion plan preview (saved)");
    expect(html).toContain("Multi-file promotion JSON preview (saved)");
    expect(html).toContain("Artifact README preview");
    expect(html).toContain("Promotion apply readiness gate");
    expect(html).toContain("Companion apply gate");
    expect(html).toContain("2 saved payload");
    expect(html).toContain("Multi-file companion promotion plan");
    expect(html).toContain("Companion target");
    expect(html).toContain("ready_for_reviewed_multi_file_patch");
    expect(html).toContain("Reviewed patch guidance");
    expect(html).toContain("Copy reviewed patch plan");
    expect(html).toContain("Reviewed patch plan (copy text)");
    expect(html).toContain("diff ready");
    expect(html).toContain("Companion diff preview (multi-file dry-run)");
    expect(html).toContain("Saved multi-file plan");
    expect(html).toContain("MULTI_FILE_PROMOTION_PLAN.md");
    expect(html).toContain("Companion files handled");
    expect(html).toContain("保存 readiness");
    expect(html).toContain("Saved readiness");
    expect(html).toContain("apply-readiness.json");
    expect(html).toContain("APPLY_READINESS.md");
    expect(html).toContain("Apply request confirmation gate");
    expect(html).toContain("Validates exact confirmations before");
    expect(html).toContain("Candidate id confirmation");
    expect(html).toContain("Target path confirmation");
    expect(html).toContain("Tests/fixtures passed");
    expect(html).toContain("Separate reviewed branch");
    expect(html).toContain("验证 apply 请求");
    expect(html).toContain("ready_for_explicit_apply");
    expect(html).toContain("Candidate id confirmation is exact");
    expect(html).toContain("Target path confirmation is exact");
    expect(html).toContain("Proposed content sha256 confirmation");
    expect(html).toContain("Current target sha256 confirmation");
    expect(html).toContain("执行单文件 apply");
    expect(html).toContain("applied=true");
    expect(html).toContain("Applied target hash");
    expect(html).toContain("sha256:new-target");
    expect(html).toContain("ready_for_explicit_apply_review");
    expect(html).toContain("apply available");
    expect(html).toContain("target exists");
    expect(html).toContain("patch sha256:patch");
    expect(html).toContain("content sha256:content");
    expect(html).toContain("target sha256:target");
    expect(html).toContain("Request patch hash");
    expect(html).toContain("Request proposed content hash");
    expect(html).toContain("Request target hash");
    expect(html).toContain("Artifact manifest is readable");
    expect(html).toContain("Required confirmation");
    expect(html).toContain("Test gate");
    expect(html).toContain("Reusable DE workflow");
    expect(html).toContain("template_candidate");
    expect(html).toContain("creator package");
    expect(html).toContain("2 companion");
    expect(html).toContain("2 companion drafts");
    expect(html).toContain("Target hint");
    expect(html).toContain("Checklist preview");
    expect(html).toContain("Primary draft file");
    expect(html).toContain("Companion draft checklist");
    expect(html).toContain("Companion draft");
    expect(html).toContain("template.sh.j2.draft");
    expect(html).toContain("example-input.tsv.draft");
    expect(html).toContain("Single-file promotion writes only the primary manifest target");
    expect(html).toContain("Companion files require separate review before promotion");
    expect(html).toContain("single-file apply writes only the selected manifest target");
    expect(html).toContain("Diff preview (review-only)");
    expect(html).toContain("+++ review-target/template.yaml");
    expect(html).toContain("Promotion patch dry-run");
    expect(html).toContain("wouldWrite=false");
    expect(html).toContain("not applied");
    expect(html).toContain("Proposed target");
    expect(html).toContain("Optional targetPath");
    expect(html).toContain("Project-relative; dry-run only.");
    expect(html).toContain("预览 target");
    expect(html).toContain("保存审阅 patch");
    expect(html).toContain("plugins/demo/templates/reusable-de/template.yaml");
    expect(html).toContain("Promotion diff preview (dry-run)");
    expect(html).toContain("Saved promotion review artifact");
    expect(html).toContain("artifact_saved");
    expect(html).toContain("promotion.patch");
    expect(html).toContain("manifest.json");
    expect(html).toContain("proposed-target.content");
    expect(html).toContain("Proposed content sha256");
    expect(html).toContain(
      ".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml",
    );
  });

  it("renders an empty draft state", () => {
    const html = renderToStaticMarkup(
      <SelfEvolutionDraftBrowserView
        response={{
          rootDir: ".omiga/learning/self-evolution-drafts",
          batchCount: 0,
          batches: [],
          note: "review-only",
        }}
        selectedDraftDir={null}
        detail={null}
      />,
    );

    expect(html).toContain("0 batches");
    expect(html).toContain("0 drafts");
    expect(html).toContain("暂无 `.omiga/learning/self-evolution-drafts/` 草稿");
  });

  it("renders blocked readiness when an artifact still targets the inert holding path", () => {
    const html = renderToStaticMarkup(
      <SelfEvolutionDraftBrowserView
        response={{
          rootDir: ".omiga/learning/self-evolution-drafts",
          batchCount: 0,
          batches: [],
          note: "review-only",
        }}
        selectedDraftDir={null}
        detail={null}
        promotionApplyPlan={{
          status: "blocked",
          safetyNote:
            "Apply readiness plan only. This command never writes the proposed target.",
          artifactDir:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo",
          patchPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/promotion.patch",
          manifestPath:
            ".omiga/review/self-evolution-promotions/artifacts/promotion-1-candidate-template-demo/manifest.json",
          proposedTargetPath:
            ".omiga/review/self-evolution-promotions/templates/candidate-template-demo/template.yaml",
          candidateId: "candidate-template-demo",
          kind: "template_candidate",
          title: "Reusable DE workflow",
          patchSha256: "sha256:patch",
          targetExists: false,
          targetCurrentSha256: null,
          checks: [
            {
              id: "manifest_readable",
              label: "Artifact manifest is readable",
              status: "passed",
              required: true,
              detail: "manifest.json parsed successfully",
            },
            {
              id: "target_not_review_holding_path",
              label: "Proposed target is not the inert review holding path",
              status: "blocked",
              required: true,
              detail:
                "target points under .omiga/review/self-evolution-promotions; save a new artifact with an explicit active project target before applying",
            },
          ],
          requiredConfirmations: [
            "Type candidate id exactly: candidate-template-demo",
          ],
          suggestedVerification: [
            "Review promotion.patch and manifest.json in a separate branch.",
          ],
          applyCommandAvailable: false,
          wouldWrite: false,
          applied: false,
        }}
      />,
    );

    expect(html).toContain("Promotion apply readiness gate");
    expect(html).toContain("blocked");
    expect(html).toContain("1 blocked");
    expect(html).toContain("Blocked reason");
    expect(html).toContain("explicit active project target");
    expect(html).toContain("apply unavailable");
    expect(html).toContain("wouldWrite=false");
  });
});
