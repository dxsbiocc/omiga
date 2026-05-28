import { describe, expect, it } from "vitest";
import {
  OPEN_SETTINGS_TAB_DETAIL,
  resolveOpenSettingsTarget,
} from "./openSettingsTabMap";

describe("resolveOpenSettingsTarget", () => {
  it("routes plugin and Browser Operator deep links to their settings tabs", () => {
    expect(resolveOpenSettingsTarget({ tab: "plugins" })).toEqual({
      tabIndex: 4,
      executionSubTab: 0,
    });
    expect(resolveOpenSettingsTarget({ tab: "browser-operator" })).toEqual({
      tabIndex: 9,
      executionSubTab: 0,
    });
  });

  it("clamps execution subtab and resets stale values when omitted", () => {
    expect(
      resolveOpenSettingsTarget({ tab: "execution", executionSubTab: 9 }),
    ).toEqual({
      tabIndex: 9,
      executionSubTab: 2,
    });
    expect(resolveOpenSettingsTarget({ tab: "execution" })).toEqual({
      tabIndex: 9,
      executionSubTab: 0,
    });
    expect(resolveOpenSettingsTarget({ tab: "ssh" })).toEqual({
      tabIndex: 9,
      executionSubTab: 2,
    });
  });

  it("falls back to the model tab for unknown details", () => {
    expect(resolveOpenSettingsTarget({ tab: "missing", executionSubTab: -1 })).toEqual({
      tabIndex: 0,
      executionSubTab: 0,
    });
    expect(OPEN_SETTINGS_TAB_DETAIL.schedule).toBe(15);
  });
});
