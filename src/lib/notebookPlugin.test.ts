import { describe, expect, it } from "vitest";
import {
  buildNotebookExecutionPrelude,
  buildOutputsFromRun,
  createNotebookCell,
  executionLanguageForNotebook,
  getCellSource,
  monacoLanguageForNotebook,
  nextGlobalExecutionCount,
  notebookKernelLanguage,
  parseNotebookContent,
  renderableNotebookOutput,
  serializeNotebook,
  setCellSource,
  setNotebookCellType,
  setNotebookKernelLanguage,
  type NotebookDocument,
} from "./notebookPlugin";

describe("notebook plugin serializer", () => {
  it("initializes empty content as nbformat 4.5", () => {
    const parsed = parseNotebookContent("");

    expect(parsed).toMatchObject({
      ok: true,
      initialized: true,
      nb: {
        cells: [],
        nbformat: 4,
        nbformat_minor: 5,
      },
    });
  });

  it("keeps notebooks with no cells empty so the UI can show insert actions only", () => {
    const parsed = parseNotebookContent(JSON.stringify({ cells: [], metadata: {} }));

    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.nb.cells).toEqual([]);
  });

  it("normalizes source arrays and promotes legacy metadata ids", () => {
    const parsed = parseNotebookContent(
      JSON.stringify({
        cells: [
          {
            cell_type: "code",
            metadata: { id: "legacy-id" },
            source: ["print(", "1", ")\n"],
          },
        ],
        metadata: {},
      }),
    );

    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.nb.cells[0]).toMatchObject({
      id: "legacy-id",
      cell_type: "code",
      source: "print(1)\n",
      outputs: [],
      execution_count: null,
    });
  });

  it("serializes with normalized top-level cell ids", () => {
    const cell = createNotebookCell("markdown", "# demo");
    const raw = serializeNotebook({
      cells: [cell],
      metadata: {},
      nbformat: 4,
      nbformat_minor: 5,
    });
    const reparsed = JSON.parse(raw) as NotebookDocument;

    expect(reparsed.cells[0].id).toBe(cell.id);
    expect(reparsed.cells[0].source).toBe("# demo");
  });
});

describe("notebook plugin language model", () => {
  it("prefers language_info over kernelspec language", () => {
    const nb: NotebookDocument = {
      cells: [],
      metadata: {
        language_info: { name: "R" },
        kernelspec: { language: "python" },
      },
    };

    expect(notebookKernelLanguage(nb)).toBe("r");
    expect(monacoLanguageForNotebook("IR")).toBe("r");
    expect(executionLanguageForNotebook("IR")).toBe("r");
  });

  it("tracks execution count across code cells", () => {
    expect(
      nextGlobalExecutionCount({
        cells: [
          { cell_type: "code", source: "", execution_count: 4 },
          { cell_type: "markdown", source: "" },
          { cell_type: "code", source: "", execution_count: 2 },
        ],
      }),
    ).toBe(5);
  });

  it("updates kernelspec and language_info when selecting an executable kernel", () => {
    const nb: NotebookDocument = {
      cells: [],
      metadata: {
        kernelspec: { display_name: "Python 3", language: "python", name: "python3" },
        language_info: { name: "python", version: "3.11" },
      },
    };

    setNotebookKernelLanguage(nb, "r");

    expect(nb.metadata).toMatchObject({
      kernelspec: { display_name: "R", language: "r", name: "ir" },
      language_info: { name: "r", version: "3.11" },
    });
    expect(notebookKernelLanguage(nb)).toBe("r");
  });

  it("builds execution prelude from earlier code cells only", () => {
    const cells = [
      createNotebookCell("code", "import os\n"),
      createNotebookCell("markdown", "notes"),
      createNotebookCell("code", "\n"),
      createNotebookCell("code", "base = os.getcwd()\n"),
      createNotebookCell("code", "os.listdir(base)"),
    ];

    expect(buildNotebookExecutionPrelude(cells, 4)).toBe(
      "import os\n\nbase = os.getcwd()",
    );
    expect(buildNotebookExecutionPrelude(cells, 0)).toBe("");
  });
});

describe("notebook plugin output renderer", () => {
  it("chooses rich MIME output before plain text", () => {
    const output = renderableNotebookOutput({
      output_type: "execute_result",
      data: {
        "text/plain": "plain",
        "text/html": "<b>rich</b>",
      },
    });

    expect(output).toEqual({ kind: "html", html: "<b>rich</b>" });
  });

  it("keeps widget output as an explicit unsupported renderer state", () => {
    const output = renderableNotebookOutput({
      output_type: "display_data",
      data: {
        "application/vnd.jupyter.widget-view+json": { model_id: "abc" },
        "text/plain": "Widget(...)",
      },
    });

    expect(output.kind).toBe("widget");
  });

  it("builds stream and error outputs from local execution", () => {
    expect(buildOutputsFromRun({ stdout: "ok\n", stderr: "bad\n", exit_code: 2 }))
      .toMatchObject([
        { output_type: "stream", name: "stdout", text: "ok\n" },
        { output_type: "stream", name: "stderr", text: "bad\n" },
        { output_type: "error", ename: "ExitCode" },
      ]);
  });

  it("edits cell source through the notebook cell abstraction", () => {
    const cell = createNotebookCell("code", "a = 1");
    setCellSource(cell, "a = 2");

    expect(getCellSource(cell)).toBe("a = 2");
  });

  it("changes cell type while preserving source and normalizing code outputs", () => {
    const cell = createNotebookCell("code", "x = 1");
    cell.outputs = [{ output_type: "stream", name: "stdout", text: "old" }];
    cell.execution_count = 3;

    setNotebookCellType(cell, "markdown");
    expect(cell).toMatchObject({ cell_type: "markdown", source: "x = 1" });
    expect(cell.outputs).toBeUndefined();
    expect(cell.execution_count).toBeUndefined();

    setNotebookCellType(cell, "code");
    expect(cell).toMatchObject({
      cell_type: "code",
      source: "x = 1",
      outputs: [],
      execution_count: null,
    });
  });
});
