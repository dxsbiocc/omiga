import { describe, expect, it } from "vitest";
import { summarizeExecutionInsight } from "./executionInsight";

describe("summarizeExecutionInsight", () => {
  it("summarizes operator/template run provenance", () => {
    const insight = summarizeExecutionInsight(
      "template_execute",
      JSON.stringify({
        status: "succeeded",
        runId: "oprun_1",
        runDir: "/tmp/run",
        operator: {
          alias: "bulk_de",
          id: "omics_differential_expression_basic",
          version: "0.1.0",
        },
        runContext: {
          kind: "template",
          parentExecutionId: "execrec_parent",
        },
        paramSources: {
          method: "user_preflight",
          fdr: "default",
        },
        preflight: {
          answeredParams: [{ param: "method", question: "Choose method" }],
          paramsBySource: {
            method: "user_preflight",
          },
        },
        selectedParams: {
          method: "DESeq2",
        },
        outputs: {
          table: [{ path: "/tmp/run/out/table.tsv" }],
        },
      }),
    );

    expect(insight?.title).toBe("Template execution");
    expect(insight?.chips).toContain("Template run");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "parent: execrec_parent",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "paramSources user_preflight: 1",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "selected: method=DESeq2",
    );
  });

  it("summarizes execution record lineage and recorded preflight metadata", () => {
    const insight = summarizeExecutionInsight(
      "execution_record_list",
      JSON.stringify({
        count: 1,
        records: [
          {
            id: "execrec_1",
            kind: "template",
            unitId: "bulk_de",
            status: "succeeded",
            metadataJson: JSON.stringify({
              paramSources: {
                method: "user_preflight",
                fdr: "default",
              },
              preflight: {
                answeredParams: [{ param: "method" }],
              },
              selectedParams: {
                method: "DESeq2",
              },
            }),
          },
        ],
        lineageSummary: {
          returnedRecords: 1,
          returnedRootRecords: 1,
          returnedRecordsWithParent: 0,
          includedChildRecords: 1,
          executionModeCounts: {
            renderedTemplate: 1,
          },
        },
      }),
    );

    expect(insight?.title).toBe("Execution record insight");
    expect(insight?.chips).toContain("1 returned");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "renderedTemplate: 1",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "bulk_de paramSources user_preflight: 1",
    );
  });

  it("summarizes execution record detail with parsed metadata", () => {
    const insight = summarizeExecutionInsight(
      "execution_record_detail",
      JSON.stringify({
        recordId: "execrec_1",
        found: true,
        record: {
          id: "execrec_1",
          kind: "template",
          unitId: "bulk_de",
          canonicalId: "plugin/template/bulk_de",
          status: "succeeded",
        },
        parsed: {
          metadata: {
            paramSources: {
              method: "user_preflight",
            },
            preflight: {
              answeredParams: [{ param: "method" }],
            },
            selectedParams: {
              method: "limma",
            },
          },
          outputSummary: {
            outputs: {
              report: { path: ".omiga/runs/report.md" },
            },
          },
        },
        children: [{ id: "execrec_child" }],
        lineage: {
          childCount: 1,
        },
      }),
    );

    expect(insight?.title).toBe("Execution record detail");
    expect(insight?.chips).toContain("template");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "canonical: plugin/template/bulk_de",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "paramSources user_preflight: 1",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "report: 1",
    );
  });

  it("summarizes archive advisor recommendations", () => {
    const insight = summarizeExecutionInsight(
      "execution_archive_advisor",
      JSON.stringify({
        summary: {
          recommendationCount: 2,
          highPriorityCount: 1,
          mediumPriorityCount: 1,
          actionCounts: {
            archive_result: 1,
          },
        },
        recommendations: [
          {
            recordId: "execrec_1",
            action: "archive_result",
            priority: "high",
            unitId: "viz_scatter_basic",
            reason: "Successful run has output artifacts.",
          },
        ],
      }),
    );

    expect(insight?.title).toBe("Archive recommendations");
    expect(insight?.chips).toContain("2 recommendations");
    expect(insight?.sections[0].items[0]).toContain(
      "high archive_result: viz_scatter_basic",
    );
  });

  it("summarizes written archive suggestion reports", () => {
    const insight = summarizeExecutionInsight(
      "execution_archive_suggestion_write",
      JSON.stringify({
        status: "succeeded",
        reportPath:
          ".omiga/execution/archive-suggestions/archive-suggestions-1.md",
        jsonPath:
          ".omiga/execution/archive-suggestions/archive-suggestions-1.json",
        recommendationCount: 2,
        highPriorityCount: 1,
        mediumPriorityCount: 1,
        markdownSummary: "Scanned 2 ExecutionRecords.",
        safetyNote: "No artifact mutation was performed.",
      }),
    );

    expect(insight?.title).toBe("Archive suggestion report");
    expect(insight?.chips).toContain("2 recommendations");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "markdown: .omiga/execution/archive-suggestions/archive-suggestions-1.md",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "No artifact mutation was performed.",
    );
  });

  it("summarizes environment preparation plans", () => {
    const insight = summarizeExecutionInsight(
      "environment_profile_prepare_plan",
      JSON.stringify({
        status: "planned",
        envRef: "r-base",
        resolution: {
          status: "resolved",
          canonicalId: "visualization-r@omiga-curated/environment/r-base",
          diagnostics: ["environment profile resolved"],
        },
        planPath: ".omiga/environments/prepare-plans/environment-prepare.md",
        jsonPath: ".omiga/environments/prepare-plans/environment-prepare.json",
        plan: {
          status: "planned",
          actions: [
            {
              kind: "rPackages",
              status: "manual",
              description: "Install or make these R packages available.",
            },
          ],
        },
        safetyNote: "Plan-only environment preparation.",
      }),
    );

    expect(insight?.title).toBe("Environment preparation plan");
    expect(insight?.chips).toContain("resolved");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "canonical: visualization-r@omiga-curated/environment/r-base",
    );
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "markdown: .omiga/environments/prepare-plans/environment-prepare.md",
    );
    expect(insight?.sections.flatMap((section) => section.items).join("\n")).toContain(
      "rPackages (manual)",
    );
  });

  it("summarizes learning self-evolution reports", () => {
    const insight = summarizeExecutionInsight(
      "learning_self_evolution_report",
      JSON.stringify({
        status: "succeeded",
        summary: {
          candidateCount: 3,
          templateCandidateCount: 1,
          preferenceCandidateCount: 1,
        },
        reportPath:
          ".omiga/learning/self-evolution-reports/self-evolution.md",
        jsonPath:
          ".omiga/learning/self-evolution-reports/self-evolution.json",
        candidates: [
          {
            priority: "high",
            kind: "template_candidate",
            title: "Crystallize lineage into a reusable Template",
            rationale: "Successful root execution has child executions.",
          },
        ],
        safetyNote: "Report-only self-evolution.",
      }),
    );

    expect(insight?.title).toBe("Learning self-evolution report");
    expect(insight?.chips).toContain("3 candidates");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "markdown: .omiga/learning/self-evolution-reports/self-evolution.md",
    );
    expect(insight?.sections.flatMap((section) => section.items).join("\n")).toContain(
      "high template_candidate",
    );
  });

  it("summarizes learning self-evolution draft scaffolds", () => {
    const insight = summarizeExecutionInsight(
      "learning_self_evolution_draft_write",
      JSON.stringify({
        status: "succeeded",
        batchDir:
          ".omiga/learning/self-evolution-drafts/draft-batch-1",
        indexPath:
          ".omiga/learning/self-evolution-drafts/draft-batch-1/README.md",
        draftCount: 2,
        drafts: [
          {
            candidateId: "evolve_1",
            kind: "template_candidate",
            title: "Crystallize lineage",
            draftDir:
              ".omiga/learning/self-evolution-drafts/draft-batch-1/01-template",
          },
        ],
        safetyNote: "Draft-only self-evolution.",
      }),
    );

    expect(insight?.title).toBe("Learning self-evolution drafts");
    expect(insight?.chips).toContain("2 drafts");
    expect(insight?.sections.flatMap((section) => section.items)).toContain(
      "batch: .omiga/learning/self-evolution-drafts/draft-batch-1",
    );
    expect(insight?.sections.flatMap((section) => section.items).join("\n")).toContain(
      "template_candidate: Crystallize lineage",
    );
  });
});
