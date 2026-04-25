import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppThemeProvider } from "./components/AppThemeProvider";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { installGlobalDebugHandlers } from "./debug/globalHandlers";
import { initNotifications } from "./utils/notifications";
import "./index.css";

installGlobalDebugHandlers();

// Initialize system notifications
void initNotifications();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppThemeProvider>
      <ErrorBoundary label="App root">
        <App />
      </ErrorBoundary>
    </AppThemeProvider>
  </React.StrictMode>
);
