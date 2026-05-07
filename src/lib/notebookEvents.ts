export type NotebookCellKind = "code" | "markdown" | "raw" | string;

export type NotebookEditorKey = "Enter" | "ArrowUp" | "ArrowDown" | "Other";

export type NotebookFocusPlacement = "start" | "end";

export interface NotebookEditorKeyEvent {
  key: NotebookEditorKey;
  shiftKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
}

export interface NotebookEditorCommandContext {
  shortcutsEnabled: boolean;
  isBusy: boolean;
  cellIndex: number;
  cellCount: number;
  cellType: NotebookCellKind;
  cursorLineNumber: number;
  lineCount: number;
}

export type NotebookEditorCommand =
  | { type: "none"; consume: false }
  | { type: "blocked"; consume: true }
  | { type: "run-cell"; consume: true; cellIndex: number }
  | {
      type: "run-and-focus-next";
      consume: true;
      cellIndex: number;
      createCodeCellIfLast: true;
    }
  | {
      type: "focus-relative-cell";
      consume: true;
      fromIndex: number;
      offset: -1 | 1;
      placement: NotebookFocusPlacement;
    };

const NONE: NotebookEditorCommand = { type: "none", consume: false };
const BLOCKED: NotebookEditorCommand = { type: "blocked", consume: true };

export function resolveNotebookEditorCommand(
  event: NotebookEditorKeyEvent,
  context: NotebookEditorCommandContext,
): NotebookEditorCommand {
  if (!context.shortcutsEnabled) return NONE;

  if (event.key === "Enter") {
    if (event.shiftKey) {
      if (context.isBusy) return BLOCKED;
      return {
        type: "run-and-focus-next",
        consume: true,
        cellIndex: context.cellIndex,
        createCodeCellIfLast: true,
      };
    }
    if (event.ctrlKey || event.metaKey) {
      if (context.isBusy) return BLOCKED;
      if (context.cellType !== "code") return BLOCKED;
      return { type: "run-cell", consume: true, cellIndex: context.cellIndex };
    }
    return NONE;
  }

  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return NONE;

  if (
    event.key === "ArrowUp" &&
    context.cursorLineNumber <= 1 &&
    context.cellIndex > 0
  ) {
    return {
      type: "focus-relative-cell",
      consume: true,
      fromIndex: context.cellIndex,
      offset: -1,
      placement: "end",
    };
  }

  if (
    event.key === "ArrowDown" &&
    context.cursorLineNumber >= Math.max(1, context.lineCount) &&
    context.cellIndex < context.cellCount - 1
  ) {
    return {
      type: "focus-relative-cell",
      consume: true,
      fromIndex: context.cellIndex,
      offset: 1,
      placement: "start",
    };
  }

  return NONE;
}

export function nextCellTargetAfterRun(
  cellIndex: number,
  cellCount: number,
): { targetIndex: number; createCodeCell: boolean } {
  const nextIndex = cellIndex + 1;
  if (nextIndex < cellCount) {
    return { targetIndex: nextIndex, createCodeCell: false };
  }
  return { targetIndex: nextIndex, createCodeCell: true };
}
