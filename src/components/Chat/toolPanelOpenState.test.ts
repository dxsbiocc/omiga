import { describe, expect, it } from "vitest";
import {
  EMPTY_NESTED_TOOL_PANEL_OPEN,
  getNestedToolPanelOpenForFold,
  toggleNestedToolPanelOpenForFold,
} from "./toolPanelOpenState";

describe("toolPanelOpenState", () => {
  it("returns a shared empty override map for folds without overrides", () => {
    const state = { "rf-a": { "tool-a": true } };

    expect(getNestedToolPanelOpenForFold(state, "rf-missing")).toBe(
      EMPTY_NESTED_TOOL_PANEL_OPEN,
    );
    expect(getNestedToolPanelOpenForFold(state, "rf-a")).toEqual({
      "tool-a": true,
    });
  });

  it("updates only the target fold override map", () => {
    const preserved = { "tool-b": true };
    const state = { "rf-a": { "tool-a": true }, "rf-b": preserved };

    const next = toggleNestedToolPanelOpenForFold(
      state,
      "rf-a",
      "tool-a",
      true,
    );

    expect(next).not.toBe(state);
    expect(next["rf-a"]).toEqual({ "tool-a": false });
    expect(next["rf-a"]).not.toBe(state["rf-a"]);
    expect(next["rf-b"]).toBe(preserved);
  });
});
