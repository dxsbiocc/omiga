/**
 * Configure pdf.js to use a locally-bundled worker so PDF rendering works
 * fully offline inside Tauri (no CDN requests at runtime).
 *
 * Import this file once, before any <Document> component is rendered.
 * We use Vite's `?url` suffix to get the resolved asset URL of the
 * bundled worker script; pdf.js spawns the worker internally using that URL.
 */
import { pdfjs } from "react-pdf";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
