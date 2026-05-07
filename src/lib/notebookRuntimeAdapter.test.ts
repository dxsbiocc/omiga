import { describe, expect, it } from "vitest";
import { createTauriNotebookCellExecutionPayload } from "./notebookRuntimeAdapter";
import type { NotebookCellExecutionRequest } from "./notebookExecution";

describe("notebook runtime adapter", () => {
  it("maps execution requests to the Tauri command payload without UI state", () => {
    const request: NotebookCellExecutionRequest = {
      notebookPath: "/tmp/demo.ipynb",
      cellIndex: 3,
      source: "print(value)\n",
      prelude: "value = 42",
      language: "python",
      shellMagic: true,
    };

    expect(createTauriNotebookCellExecutionPayload(request)).toEqual({
      notebookPath: "/tmp/demo.ipynb",
      cellIndex: 3,
      source: "print(value)\n",
      prelude: "value = 42",
      language: "python",
      shellMagic: true,
    });
  });
});
