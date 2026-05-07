import { invoke } from "@tauri-apps/api/core";
import type {
  ExecuteNotebookCellRequest,
  NotebookCellExecutionRequest,
} from "./notebookExecution";
import type { ExecuteResult } from "./notebookPlugin";

export interface NotebookRuntimeAdapter {
  executeCell: ExecuteNotebookCellRequest;
}

export type TauriNotebookCellExecutionPayload = Record<string, unknown> & {
  notebookPath: string;
  cellIndex: number;
  source: string;
  prelude: string;
  language: "python" | "r";
  shellMagic: boolean;
};

export function createTauriNotebookCellExecutionPayload(
  request: NotebookCellExecutionRequest,
): TauriNotebookCellExecutionPayload {
  return {
    notebookPath: request.notebookPath,
    cellIndex: request.cellIndex,
    source: request.source,
    prelude: request.prelude,
    language: request.language,
    shellMagic: request.shellMagic,
  };
}

export function createTauriNotebookRuntimeAdapter(): NotebookRuntimeAdapter {
  return {
    executeCell: (request) =>
      invoke<ExecuteResult>(
        "execute_ipynb_cell",
        createTauriNotebookCellExecutionPayload(request),
      ),
  };
}
