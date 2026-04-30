import { describe, expect, it } from "vitest";
import {
  normalizeTerminalWorkspacePath,
  terminalWorkspaceDisplayName,
} from "./systemTerminal";

describe("embedded terminal workspace helpers", () => {
  it("treats empty and dot paths as unset workspaces", () => {
    expect(normalizeTerminalWorkspacePath("")).toBeNull();
    expect(normalizeTerminalWorkspacePath("   ")).toBeNull();
    expect(normalizeTerminalWorkspacePath(".")).toBeNull();
    expect(normalizeTerminalWorkspacePath(undefined)).toBeNull();
  });

  it("preserves real workspace paths after trimming", () => {
    expect(normalizeTerminalWorkspacePath(" /tmp/project ")).toBe("/tmp/project");
  });

  it("shows a compact basename plus full path", () => {
    expect(terminalWorkspaceDisplayName("/Users/me/work/omiga")).toBe(
      "omiga · /Users/me/work/omiga",
    );
    expect(terminalWorkspaceDisplayName("C:\\work\\omiga")).toBe(
      "omiga · C:\\work\\omiga",
    );
  });
});
