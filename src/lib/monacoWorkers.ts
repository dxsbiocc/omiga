/**
 * Configure Monaco Editor to use locally-bundled web workers.
 *
 * By default @monaco-editor/react loads workers from a CDN. In a Tauri
 * desktop app the CDN request may silently fail, causing every language
 * service (JSON parsing, TypeScript type-checking, HTML/CSS tokenization)
 * to run on the MAIN thread — that is what makes large files feel slow.
 *
 * This file must be imported BEFORE any Monaco or @monaco-editor/react code
 * so that `self.MonacoEnvironment` is in place when the editor initialises.
 */

import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";

// Vite's `?worker` suffix produces a Worker constructor that references a
// locally-bundled chunk — no CDN, no network, works fully offline.
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import JsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import CssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import HtmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import TsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

// Tell Monaco which Worker to spawn for each language label.
// This MUST be set before any editor instance is created.
self.MonacoEnvironment = {
  getWorker(_moduleId: string, label: string): Worker {
    if (label === "json") return new JsonWorker();
    if (label === "css" || label === "scss" || label === "less") return new CssWorker();
    if (label === "html" || label === "handlebars" || label === "razor") return new HtmlWorker();
    if (label === "typescript" || label === "javascript") return new TsWorker();
    return new EditorWorker();
  },
};

// Point @monaco-editor/react at the same locally-imported monaco instance
// so it never tries to fetch anything from a CDN at runtime.
loader.config({ monaco });
