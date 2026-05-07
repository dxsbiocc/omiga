import { describe, expect, it, vi } from "vitest";
import {
  NotebookExecutionController,
  applyNotebookCellExecutionResult,
  createNotebookCellExecutionRequest,
  runnableNotebookCellIndices,
  type ExecuteNotebookCellRequest,
} from "./notebookExecution";
import { createNotebookCell, type NotebookDocument } from "./notebookPlugin";

function notebook(cells = [
  createNotebookCell("code", "import os\n"),
  createNotebookCell("markdown", "notes"),
  createNotebookCell("code", "os.listdir('.')\n"),
]): NotebookDocument {
  return {
    cells,
    metadata: {},
    nbformat: 4,
    nbformat_minor: 5,
  };
}

describe("notebook execution controller helpers", () => {
  it("selects runnable code cell indices without leaking UI details", () => {
    expect(runnableNotebookCellIndices(notebook())).toEqual([0, 2]);
  });

  it("builds execution requests with source and earlier-code prelude", () => {
    expect(
      createNotebookCellExecutionRequest(notebook(), 2, {
        notebookPath: "/tmp/demo.ipynb",
        language: "python",
        shellMagic: true,
      }),
    ).toEqual({
      notebookPath: "/tmp/demo.ipynb",
      language: "python",
      shellMagic: true,
      cellIndex: 2,
      source: "os.listdir('.')\n",
      prelude: "import os",
    });
  });

  it("does not create execution requests for markdown cells", () => {
    expect(
      createNotebookCellExecutionRequest(notebook(), 1, {
        notebookPath: "/tmp/demo.ipynb",
        language: "python",
        shellMagic: true,
      }),
    ).toBeNull();
  });

  it("applies execution results to only the target code cell", () => {
    const nb = notebook();
    const updated = applyNotebookCellExecutionResult(
      nb,
      2,
      { stdout: "ok\n", stderr: "", exit_code: 0 },
      7,
    );

    expect(updated?.cells[0]).toBe(nb.cells[0]);
    expect(updated?.cells[2]).toMatchObject({
      execution_count: 7,
      outputs: [{ output_type: "stream", name: "stdout", text: "ok\n" }],
    });
  });

  it("keeps the executor adapter independent from Tauri or React", async () => {
    const execute: ExecuteNotebookCellRequest = vi.fn(async () => ({
      stdout: "42\n",
      stderr: "",
      exit_code: 0,
    }));
    const request = createNotebookCellExecutionRequest(notebook(), 0, {
      notebookPath: "/tmp/demo.ipynb",
      language: "python",
      shellMagic: false,
    });

    expect(request).not.toBeNull();
    if (!request) return;
    await expect(execute(request)).resolves.toMatchObject({ stdout: "42\n" });
    expect(execute).toHaveBeenCalledWith(request);
  });

  it("serializes run-all through a controller host", async () => {
    let current = notebook();
    const status: unknown[] = [];
    const execute: ExecuteNotebookCellRequest = vi.fn(async (request) => ({
      stdout: `cell:${request.cellIndex}\n`,
      stderr: "",
      exit_code: 0,
    }));
    const controller = new NotebookExecutionController({
      getNotebook: () => current,
      getOptions: () => ({
        notebookPath: "/tmp/demo.ipynb",
        language: "python",
        shellMagic: true,
      }),
      execute,
      commit: (nb) => {
        current = nb;
      },
      onStatus: (next) => status.push(next),
    });

    await expect(controller.runAll()).resolves.toBe(true);

    expect(execute).toHaveBeenCalledTimes(2);
    expect(current.cells[0]).toMatchObject({ execution_count: 1 });
    expect(current.cells[2]).toMatchObject({ execution_count: 2 });
    expect(status).toContainEqual({ runningCellIndex: 0, runningAll: true, error: null });
    expect(status).toContainEqual({ runningCellIndex: 2, runningAll: true, error: null });
    expect(status[status.length - 1]).toEqual({ runningCellIndex: null, runningAll: false, error: null });
  });

  it("keeps execution errors in final controller status", async () => {
    let current = notebook();
    const status: unknown[] = [];
    const controller = new NotebookExecutionController({
      getNotebook: () => current,
      getOptions: () => ({
        notebookPath: "/tmp/demo.ipynb",
        language: "python",
        shellMagic: true,
      }),
      execute: async () => {
        throw new Error("boom");
      },
      commit: (nb) => {
        current = nb;
      },
      onStatus: (next) => status.push(next),
      formatError: (error) => error instanceof Error ? error.message : String(error),
    });

    await expect(controller.runCell(0)).resolves.toBe(false);

    expect(status[status.length - 1]).toEqual({
      runningCellIndex: null,
      runningAll: false,
      error: "boom",
    });
    expect(current.cells[0]).toMatchObject({ execution_count: null, outputs: [] });
  });
});
