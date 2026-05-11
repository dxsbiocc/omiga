import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ExecutionRecordBrowserView } from "./ExecutionRecordBrowser";

describe("ExecutionRecordBrowserView", () => {
  it("renders read-only record list and selected detail insight", () => {
    const html = renderToStaticMarkup(
      <ExecutionRecordBrowserView
        response={{
          database: "/project/.omiga/execution/executions.sqlite",
          count: 2,
          note: "read-only",
          lineageSummary: {
            returnedRecords: 2,
            returnedRootRecords: 1,
            returnedRecordsWithParent: 1,
            includedChildRecords: 0,
            statusCounts: { success: 2 },
            kindCounts: { template: 1, operator: 1 },
            executionModeCounts: { renderedTemplate: 1 },
          },
          records: [
            {
              id: "execrec_parent",
              kind: "template",
              unitId: "bulk_de",
              status: "success",
              endedAt: "2026-05-10T00:01:00Z",
            },
            {
              id: "execrec_child",
              kind: "operator",
              unitId: "bulk_de_operator",
              status: "success",
              parentExecutionId: "execrec_parent",
            },
          ],
        }}
        selectedId="execrec_parent"
        detail={{
          found: true,
          recordId: "execrec_parent",
          database: "/project/.omiga/execution/executions.sqlite",
          note: "read-only",
          record: {
            id: "execrec_parent",
            kind: "template",
            unitId: "bulk_de",
            status: "success",
          },
          parsed: {
            metadata: {
              paramSources: { method: "user_preflight" },
              preflight: { answeredParams: [{ param: "method" }] },
            },
            outputSummary: {
              outputs: { table: ["de.tsv"] },
            },
          },
          lineage: {
            parentExecutionId: null,
            childCount: 1,
          },
          children: [
            {
              id: "execrec_child",
              kind: "operator",
              unitId: "bulk_de_operator",
              status: "success",
            },
          ],
        }}
      />,
    );

    expect(html).toContain("ExecutionRecord Browser");
    expect(html).toContain("只读运行历史");
    expect(html).toContain("2 records");
    expect(html).toContain("1 roots");
    expect(html).toContain("bulk_de");
    expect(html).toContain("child");
    expect(html).toContain("Execution record detail");
    expect(html).toContain("paramSources user_preflight: 1");
    expect(html).toContain("1 answered question");
    expect(html).toContain("table: 1");
    expect(html).toContain("Children:");
  });

  it("renders an empty read-only state", () => {
    const html = renderToStaticMarkup(
      <ExecutionRecordBrowserView
        response={{
          database: "/project/.omiga/execution/executions.sqlite",
          count: 0,
          records: [],
          note: "read-only",
          lineageSummary: {
            returnedRecords: 0,
            returnedRootRecords: 0,
            returnedRecordsWithParent: 0,
            includedChildRecords: 0,
            statusCounts: {},
            kindCounts: {},
            executionModeCounts: {},
          },
        }}
        selectedId={null}
        detail={null}
      />,
    );

    expect(html).toContain("0 records");
    expect(html).toContain("暂无 Operator / Template ExecutionRecord");
  });
});
