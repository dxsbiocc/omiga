import { describe, expect, it } from "vitest";
import {
  messageEntranceDelayMs,
  messageRenderItemKey,
  shouldAnimateMessageItem,
} from "./renderItemUtils";

describe("renderItemUtils", () => {
  it("returns stable keys for row and fold render items", () => {
    expect(messageRenderItemKey({ kind: "react_fold", id: "rf-a" })).toBe("rf-a");
    expect(messageRenderItemKey({ kind: "row", message: { id: "msg-a" } })).toBe("msg-a");
  });

  it("caps entrance animation delay for long sessions", () => {
    expect(messageEntranceDelayMs(0)).toBe(0);
    expect(messageEntranceDelayMs(3)).toBe(105);
    expect(messageEntranceDelayMs(99)).toBe(280);
  });

  it("disables item animations while older history is being restored", () => {
    expect(shouldAnimateMessageItem({ restoringOlderItems: false })).toBe(true);
    expect(shouldAnimateMessageItem({ restoringOlderItems: true })).toBe(false);
  });
});
