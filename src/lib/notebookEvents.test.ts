import { describe, expect, it } from "vitest";
import {
  nextCellTargetAfterRun,
  resolveNotebookEditorCommand,
  type NotebookEditorCommandContext,
} from "./notebookEvents";

const baseContext: NotebookEditorCommandContext = {
  shortcutsEnabled: true,
  isBusy: false,
  cellIndex: 1,
  cellCount: 3,
  cellType: "code",
  cursorLineNumber: 1,
  lineCount: 2,
};

describe("notebook editor event controller", () => {
  it("maps Shift+Enter to run-and-focus-next without caring about rendering details", () => {
    expect(
      resolveNotebookEditorCommand(
        { key: "Enter", shiftKey: true },
        baseContext,
      ),
    ).toEqual({
      type: "run-and-focus-next",
      consume: true,
      cellIndex: 1,
      createCodeCellIfLast: true,
    });
  });

  it("maps Ctrl/Cmd+Enter to run-cell only for code cells", () => {
    expect(
      resolveNotebookEditorCommand({ key: "Enter", ctrlKey: true }, baseContext),
    ).toEqual({ type: "run-cell", consume: true, cellIndex: 1 });

    expect(
      resolveNotebookEditorCommand(
        { key: "Enter", metaKey: true },
        { ...baseContext, cellType: "markdown" },
      ),
    ).toEqual({ type: "blocked", consume: true });
  });

  it("blocks execution shortcuts while the kernel is busy", () => {
    expect(
      resolveNotebookEditorCommand(
        { key: "Enter", shiftKey: true },
        { ...baseContext, isBusy: true },
      ),
    ).toEqual({ type: "blocked", consume: true });
  });

  it("maps boundary arrow keys to cross-cell focus commands", () => {
    expect(
      resolveNotebookEditorCommand({ key: "ArrowUp" }, baseContext),
    ).toEqual({
      type: "focus-relative-cell",
      consume: true,
      fromIndex: 1,
      offset: -1,
      placement: "end",
    });

    expect(
      resolveNotebookEditorCommand(
        { key: "ArrowDown" },
        { ...baseContext, cursorLineNumber: 2 },
      ),
    ).toEqual({
      type: "focus-relative-cell",
      consume: true,
      fromIndex: 1,
      offset: 1,
      placement: "start",
    });
  });

  it("lets Monaco handle non-boundary or modified arrow keys", () => {
    expect(
      resolveNotebookEditorCommand(
        { key: "ArrowUp" },
        { ...baseContext, cursorLineNumber: 2 },
      ),
    ).toEqual({ type: "none", consume: false });
    expect(
      resolveNotebookEditorCommand(
        { key: "ArrowDown", altKey: true },
        { ...baseContext, cursorLineNumber: 2 },
      ),
    ).toEqual({ type: "none", consume: false });
  });

  it("plans whether Shift+Enter needs to create a new code cell", () => {
    expect(nextCellTargetAfterRun(0, 3)).toEqual({
      targetIndex: 1,
      createCodeCell: false,
    });
    expect(nextCellTargetAfterRun(2, 3)).toEqual({
      targetIndex: 3,
      createCodeCell: true,
    });
  });
});
