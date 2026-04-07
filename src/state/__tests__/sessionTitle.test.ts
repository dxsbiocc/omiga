import { describe, it, expect } from "vitest";
import type { Session } from "../sessionStore";
import {
  isPlaceholderSessionTitle,
  titleFromFirstUserMessage,
  UNUSED_SESSION_LABEL,
  shouldShowNewSessionPlaceholder,
  isUnsetWorkspacePath,
} from "../sessionStore";

describe("session title from first message", () => {
  it("detects placeholder New chat titles", () => {
    expect(isPlaceholderSessionTitle("New chat · Apr 3, 1:36 PM")).toBe(true);
    expect(isPlaceholderSessionTitle("  New chat · 4月3日 13:36")).toBe(true);
    expect(isPlaceholderSessionTitle(undefined)).toBe(false);
    expect(isPlaceholderSessionTitle("")).toBe(false);
    expect(isPlaceholderSessionTitle("Refactor auth module")).toBe(false);
  });

  it("treats New session labels as placeholder", () => {
    expect(isPlaceholderSessionTitle(UNUSED_SESSION_LABEL)).toBe(true);
    expect(isPlaceholderSessionTitle("New Session")).toBe(true);
  });

  it("shouldShowNewSessionPlaceholder respects DB and local counts", () => {
    const empty: Session = {
      id: "1",
      name: UNUSED_SESSION_LABEL,
      projectPath: ".",
      createdAt: "",
      updatedAt: "",
      messageCount: 0,
    };
    expect(shouldShowNewSessionPlaceholder(empty)).toBe(true);
    expect(
      shouldShowNewSessionPlaceholder({ ...empty, messageCount: 1 }),
    ).toBe(false);
    expect(
      shouldShowNewSessionPlaceholder(empty, {
        isCurrentSession: true,
        storeMessageCount: 1,
      }),
    ).toBe(false);
  });

  it("uses first line and truncates long titles", () => {
    expect(titleFromFirstUserMessage("Hello world")).toBe("Hello world");
    expect(titleFromFirstUserMessage("Line one\nLine two")).toBe("Line one");
    expect(titleFromFirstUserMessage("  a   b  c  ")).toBe("a b c");
    const long = "x".repeat(60);
    const out = titleFromFirstUserMessage(long);
    expect(out.length).toBe(48);
    expect(out.endsWith("…")).toBe(true);
  });

  it("handles empty first line", () => {
    expect(titleFromFirstUserMessage("\n\nonly second")).toBe("only second");
  });
});

describe("isUnsetWorkspacePath", () => {
  it("treats empty and dot as unset", () => {
    expect(isUnsetWorkspacePath(undefined)).toBe(true);
    expect(isUnsetWorkspacePath("")).toBe(true);
    expect(isUnsetWorkspacePath("  .  ")).toBe(true);
    expect(isUnsetWorkspacePath("/Users/foo/proj")).toBe(false);
  });
});
