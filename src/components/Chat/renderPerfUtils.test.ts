import { describe, expect, it } from "vitest";
import {
  formatProgressiveRenderPerf,
  progressiveOlderItemCount,
  shouldLogProgressiveRenderPerf,
} from "./renderPerfUtils";

describe("renderPerfUtils", () => {
  it("logs only sessions that exceed the instant render window", () => {
    expect(shouldLogProgressiveRenderPerf(30, 30)).toBe(false);
    expect(shouldLogProgressiveRenderPerf(31, 30)).toBe(true);
  });

  it("counts hidden older items defensively", () => {
    expect(progressiveOlderItemCount({ totalItems: 80, renderedItems: 30 })).toBe(50);
    expect(progressiveOlderItemCount({ totalItems: 10, renderedItems: 30 })).toBe(0);
  });

  it("formats phase render performance messages", () => {
    expect(
      formatProgressiveRenderPerf({
        phase: "phase-1",
        sessionId: "abcdefgh1234",
        totalItems: 80,
        renderedItems: 30,
        instantRenderCount: 30,
        durationMs: 12.4,
      }),
    ).toBe("[RenderPerf] abcdefgh | phase-1 | rendered 30/80 items | deferred 50 | duration 12ms");

    expect(
      formatProgressiveRenderPerf({
        phase: "phase-2",
        totalItems: 80,
        renderedItems: 80,
        instantRenderCount: 30,
        durationMs: 42.6,
        addedHeightPx: 1234.2,
      }),
    ).toBe("[RenderPerf] phase-2 | rendered 80/80 items | restored 50 | duration 43ms | added-height 1234px");
  });
});
