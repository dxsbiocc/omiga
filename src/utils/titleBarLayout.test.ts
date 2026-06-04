import { describe, expect, it } from "vitest";
import { computeTitleBarContentLeft } from "./titleBarLayout";

const baseLayout = {
  buttonRailEnd: 224,
  chatRailInset: 18,
  leftPanelWidth: 260,
  resizeHandleWidth: 6,
  showSettingsPanel: false,
};

describe("computeTitleBarContentLeft", () => {
  it("keeps the session summary anchored when the left sidebar is collapsed", () => {
    const expanded = computeTitleBarContentLeft({
      ...baseLayout,
      leftPanelCollapsed: false,
    });
    const collapsed = computeTitleBarContentLeft({
      ...baseLayout,
      leftPanelCollapsed: true,
    });

    expect(collapsed).toBe(expanded);
    expect(collapsed).toBe(284);
  });

  it("falls back to the titlebar button rail in settings mode", () => {
    expect(
      computeTitleBarContentLeft({
        ...baseLayout,
        leftPanelCollapsed: true,
        showSettingsPanel: true,
      }),
    ).toBe(baseLayout.buttonRailEnd);
  });
});
