import {
  buildNotebookExecutionPrelude,
  buildOutputsFromRun,
  getCellSource,
  nextGlobalExecutionCount,
  type ExecuteResult,
  type NotebookDocument,
} from "./notebookPlugin";

export interface NotebookExecutionOptions {
  notebookPath: string;
  language: "python" | "r";
  shellMagic: boolean;
}

export interface NotebookCellExecutionRequest extends NotebookExecutionOptions {
  cellIndex: number;
  source: string;
  prelude: string;
}

export type ExecuteNotebookCellRequest = (
  request: NotebookCellExecutionRequest,
) => Promise<ExecuteResult>;

export interface NotebookExecutionStatus {
  runningCellIndex: number | null;
  runningAll: boolean;
  error: string | null;
}

export interface NotebookExecutionControllerHost {
  getNotebook: () => NotebookDocument | null;
  getOptions: () => NotebookExecutionOptions;
  execute: ExecuteNotebookCellRequest;
  commit: (nb: NotebookDocument) => void;
  onStatus: (status: NotebookExecutionStatus) => void;
  formatError?: (error: unknown) => string;
}

export function runnableNotebookCellIndices(nb: NotebookDocument): number[] {
  return nb.cells
    .map((cell, index) => (cell.cell_type === "code" ? index : -1))
    .filter((index) => index >= 0);
}

export function createNotebookCellExecutionRequest(
  nb: NotebookDocument,
  cellIndex: number,
  options: NotebookExecutionOptions,
): NotebookCellExecutionRequest | null {
  const cell = nb.cells[cellIndex];
  if (!cell || cell.cell_type !== "code") return null;
  return {
    ...options,
    cellIndex,
    source: getCellSource(cell),
    prelude: buildNotebookExecutionPrelude(nb.cells, cellIndex),
  };
}

export function applyNotebookCellExecutionResult(
  nb: NotebookDocument,
  cellIndex: number,
  result: ExecuteResult,
  executionCount = nextGlobalExecutionCount(nb),
): NotebookDocument | null {
  const originalCell = nb.cells[cellIndex];
  if (!originalCell || originalCell.cell_type !== "code") return null;
  const cells = nb.cells.slice();
  cells[cellIndex] = {
    ...originalCell,
    outputs: buildOutputsFromRun(result),
    execution_count: executionCount,
  };
  return { ...nb, cells };
}

export class NotebookExecutionController {
  private busy = false;

  constructor(private readonly host: NotebookExecutionControllerHost) {}

  get isBusy(): boolean {
    return this.busy;
  }

  async runCell(cellIndex: number): Promise<boolean> {
    if (this.busy) return false;
    const nb = this.host.getNotebook();
    if (!nb) return false;
    const request = createNotebookCellExecutionRequest(
      nb,
      cellIndex,
      this.host.getOptions(),
    );
    if (!request) return false;

    this.busy = true;
    this.host.onStatus({ runningCellIndex: cellIndex, runningAll: false, error: null });
    let errorMessage: string | null = null;
    try {
      const result = await this.host.execute(request);
      const latest = this.host.getNotebook();
      if (latest) {
        const updated = applyNotebookCellExecutionResult(latest, cellIndex, result);
        if (updated) this.host.commit(updated);
      }
      return true;
    } catch (error) {
      errorMessage = this.formatError(error);
      return false;
    } finally {
      this.busy = false;
      this.host.onStatus({
        runningCellIndex: null,
        runningAll: false,
        error: errorMessage,
      });
    }
  }

  async runAll(): Promise<boolean> {
    if (this.busy) return false;
    let working = this.host.getNotebook();
    if (!working) return false;

    this.busy = true;
    this.host.onStatus({ runningCellIndex: null, runningAll: true, error: null });
    let errorMessage: string | null = null;
    try {
      let sequence = 1;
      const cellIndices = runnableNotebookCellIndices(working);
      for (const cellIndex of cellIndices) {
        this.host.onStatus({ runningCellIndex: cellIndex, runningAll: true, error: null });
        const request = createNotebookCellExecutionRequest(
          working,
          cellIndex,
          this.host.getOptions(),
        );
        if (!request) continue;
        const result = await this.host.execute(request);
        const latest = this.host.getNotebook() ?? working;
        const updated = applyNotebookCellExecutionResult(
          latest,
          cellIndex,
          result,
          sequence,
        );
        if (!updated) continue;
        sequence += 1;
        working = updated;
        this.host.commit(updated);
      }
      return true;
    } catch (error) {
      errorMessage = this.formatError(error);
      return false;
    } finally {
      this.busy = false;
      this.host.onStatus({
        runningCellIndex: null,
        runningAll: false,
        error: errorMessage,
      });
    }
  }

  private formatError(error: unknown): string {
    return this.host.formatError?.(error) ?? String(error);
  }
}
