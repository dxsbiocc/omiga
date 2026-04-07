// Must be the very first import so Monaco workers are registered before any
// editor instance is created. See src/lib/monacoWorkers.ts for details.
import "./lib/monacoWorkers";
// Configure pdf.js worker once at app startup.
import "./lib/pdfWorker";

import React from "react";
import ReactDOM from "react-dom/client";
import { ThemeProvider } from "@mui/material/styles";
import CssBaseline from "@mui/material/CssBaseline";
import App from "./App";
import { theme } from "./theme";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { installGlobalDebugHandlers } from "./debug/globalHandlers";
import "./index.css";

installGlobalDebugHandlers();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <ErrorBoundary label="App root">
        <App />
      </ErrorBoundary>
    </ThemeProvider>
  </React.StrictMode>
);
