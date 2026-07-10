import { afterEach, describe, expect, it, vi } from "vitest";
import {
  compareProjectGroupsForSidebar,
  formatSidebarRelativeTime,
} from "./index";
import { normalizeAppLocale } from "../../state/localeStore";

describe("SessionList sidebar helpers", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("formats relative time using Chinese locale variants", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-03T12:00:00.000Z"));

    const oneWeekAgo = "2026-05-27T12:00:00.000Z";

    expect(formatSidebarRelativeTime(oneWeekAgo, "zh-CN")).toBe("1 周");
    expect(formatSidebarRelativeTime(oneWeekAgo, "zh-Hans")).toBe("1 周");
    expect(formatSidebarRelativeTime(oneWeekAgo, "en")).toBe("1w");
  });

  it("normalizes persisted or browser locale variants", () => {
    expect(normalizeAppLocale("zh")).toBe("zh-CN");
    expect(normalizeAppLocale("zh_CN")).toBe("zh-CN");
    expect(normalizeAppLocale("en-US")).toBe("en");
  });

  it("does not reorder projects just because a project is current", () => {
    const olderCurrent = {
      key: "/older-current",
      label: "Older current",
      latestUpdatedAt: "2026-05-01T00:00:00.000Z",
      isCurrent: true,
    };
    const newerInactive = {
      key: "/newer-inactive",
      label: "Newer inactive",
      latestUpdatedAt: "2026-06-01T00:00:00.000Z",
      isCurrent: false,
    };

    const sorted = [olderCurrent, newerInactive].sort(compareProjectGroupsForSidebar);

    expect(sorted.map((item) => item.key)).toEqual([
      "/newer-inactive",
      "/older-current",
    ]);
  });
});
