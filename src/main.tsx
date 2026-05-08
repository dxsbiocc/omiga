import React from "react";
import ReactDOM from "react-dom/client";
import { DndProvider } from "react-dnd";
import { TouchBackend } from "react-dnd-touch-backend";
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
  <DndProvider
    backend={TouchBackend}
    options={{ enableMouseEvents: true, touchSlop: 4 }}
  >
    <React.StrictMode>
      <AppThemeProvider>
        <ErrorBoundary label="App root">
          <App />
        </ErrorBoundary>
      </AppThemeProvider>
    </React.StrictMode>
  </DndProvider>,
);
