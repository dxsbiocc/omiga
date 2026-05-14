import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { PlanTodoList, planTodoRuntimeLabel } from "./PlanTodoList";

describe("PlanTodoList timing", () => {
  it("formats running and completed todo execution time", () => {
    vi.spyOn(Date, "now").mockReturnValue(6_500);
    try {
      expect(
        planTodoRuntimeLabel({
          status: "running",
          startedAt: 1_000,
        }),
      ).toBe("5s");
      expect(
        planTodoRuntimeLabel({
          status: "completed",
          startedAt: 1_000,
          completedAt: 3_400,
        }),
      ).toBe("2s");
    } finally {
      vi.restoreAllMocks();
    }
  });

  it("renders per-todo execution time in the list", () => {
    const html = renderToStaticMarkup(
      <PlanTodoList
        items={[
          {
            id: "todo-1",
            name: "筛选显著基因",
            status: "completed",
            startedAt: 1_000,
            completedAt: 4_200,
          },
        ]}
      />,
    );

    expect(html).toContain("筛选显著基因");
    expect(html).toContain("3s");
  });
});
