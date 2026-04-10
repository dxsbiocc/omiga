import { useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Box, Paper, Stack, useTheme, alpha } from "@mui/material";
import { Layout } from "./components/Layout";
import { Chat } from "./components/Chat";
import { FileTree } from "./components/FileTree";
import { SessionList } from "./components/SessionList";
import { Settings } from "./components/Settings";
import { OPEN_SETTINGS_TAB_DETAIL } from "./components/Settings/openSettingsTabMap";
import { TaskStatus } from "./components/TaskStatus";
import { CodeWorkspace } from "./components/CodeWorkspace";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ResizeHandle } from "./components/ResizeHandle";
import {
  useSessionStore,
  useWorkspaceStore,
  useUiStore,
  usePermissionStore,
  LAYOUT_PANEL_MIN,
} from "./state";

export default function App() {
  const theme = useTheme();
  const { currentSession, loadSessions } = useSessionStore();

  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const settingsTabIndex = useUiStore((s) => s.settingsTabIndex);
  const setSettingsTabIndex = useUiStore((s) => s.setSettingsTabIndex);
  const rightPanelMode = useUiStore((s) => s.rightPanelMode);
  const setRightPanelMode = useUiStore((s) => s.setRightPanelMode);
  const leftW = useUiStore((s) => s.leftPanelWidth);
  const rightW = useUiStore((s) => s.rightPanelWidth);
  const codeH = useUiStore((s) => s.codePanelHeight);
  const tasksH = useUiStore((s) => s.tasksPanelHeight);
  const resizeLeftBy = useUiStore((s) => s.resizeLeftBy);
  const resizeRightBy = useUiStore((s) => s.resizeRightBy);
  const resizeCodeBy = useUiStore((s) => s.resizeCodeBy);
  const resizeTasksBy = useUiStore((s) => s.resizeTasksBy);
  const ensureCodePanelMin = useUiStore((s) => s.ensureCodePanelMin);

  const centerRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);

  const clampCodeH = useCallback((h: number) => {
    const el = centerRef.current;
    const codeMin = LAYOUT_PANEL_MIN;
    if (!el) return Math.max(codeMin, Math.min(600, h));
    const max = Math.max(codeMin, el.clientHeight - codeMin);
    return Math.max(codeMin, Math.min(max, h));
  }, []);

  const clampTasksH = useCallback((h: number) => {
    const el = rightRef.current;
    if (!el) return Math.max(LAYOUT_PANEL_MIN, Math.min(500, h));
    const max = Math.max(LAYOUT_PANEL_MIN, el.clientHeight - LAYOUT_PANEL_MIN);
    return Math.max(LAYOUT_PANEL_MIN, Math.min(max, h));
  }, []);

  useEffect(() => {
    const onWinResize = () => {
      const {
        codePanelHeight,
        tasksPanelHeight,
        setCodeHeight,
        setTasksHeight,
      } = useUiStore.getState();
      setCodeHeight(clampCodeH(codePanelHeight));
      setTasksHeight(clampTasksH(tasksPanelHeight));
    };
    window.addEventListener("resize", onWinResize);
    return () => window.removeEventListener("resize", onWinResize);
  }, [clampCodeH, clampTasksH]);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  useEffect(() => {
    const raw = localStorage.getItem("omiga_brave_search_api_key");
    const t = raw?.trim();
    if (t) {
      void invoke("set_brave_search_api_key", { apiKey: t }).catch(() => {});
    }
  }, []);

  // If omiga.yaml did not load at startup, migrate legacy localStorage into Rust once.
  useEffect(() => {
    void (async () => {
      try {
        const st = await invoke<{ provider?: string; apiKeyPreview?: string } | null>(
          "get_llm_config_state",
          {},
        );
        if (st?.provider?.trim() && st.apiKeyPreview?.trim()) {
          return;
        }
      } catch {
        /* fall through */
      }
      const raw = localStorage.getItem("omiga_llm_config");
      if (!raw?.trim()) return;
      let parsed: {
        provider: string;
        apiKey: string;
        secretKey?: string;
        appId?: string;
        model?: string;
        baseUrl?: string;
      };
      try {
        parsed = JSON.parse(raw);
      } catch {
        return;
      }
      if (!parsed.provider || !parsed.apiKey?.trim()) return;
      void invoke("set_llm_config", {
        provider: parsed.provider,
        apiKey: parsed.apiKey.trim(),
        secretKey: parsed.secretKey,
        appId: parsed.appId,
        model: parsed.model?.trim() || undefined,
        baseUrl: parsed.baseUrl,
      }).catch(() => {});
    })();
  }, []);

  useEffect(() => {
    const open = (e: Event) => {
      const detail = (e as CustomEvent<{ tab?: string }>).detail;
      const key = detail?.tab;
      const idx =
        key != null && OPEN_SETTINGS_TAB_DETAIL[key] !== undefined
          ? OPEN_SETTINGS_TAB_DETAIL[key]
          : 0;
      setSettingsTabIndex(idx);
      setSettingsOpen(true);
      setRightPanelMode("settings");
    };
    window.addEventListener("openSettings", open);
    return () => window.removeEventListener("openSettings", open);
  }, [setSettingsOpen, setRightPanelMode, setSettingsTabIndex]);

  const sessionId = currentSession?.id ?? "";
  const filePath = useWorkspaceStore((s) => s.filePath);
  const hasCodeWorkspace = Boolean(filePath);

  useEffect(() => {
    if (!hasCodeWorkspace) return;
    ensureCodePanelMin();
  }, [hasCodeWorkspace, ensureCodePanelMin]);

  // Listen for permission requests from backend
  useEffect(() => {
    const setupListener = async () => {
      try {
        const unlisten = await listen<{
          type: string;
          request_id: string;
          tool_name: string;
          risk_level: string;
          risk_description: string;
          detected_risks?: Array<{
            category: string;
            severity: string;
            description: string;
            mitigation?: string;
          }>;
          recommendations?: string[];
          session_id?: string;
        }>("permission-request", (event) => {
          try {
            console.log("Permission request received:", event.payload);

            // Validate risk_level is one of the expected values
            const validRiskLevels = ["safe", "low", "medium", "high", "critical"];
            const riskLevel = event.payload.risk_level;
            if (!validRiskLevels.includes(riskLevel)) {
              console.warn("Invalid risk level received:", riskLevel, "- defaulting to 'medium'");
            }

            console.log("[Permission] Setting pending request for:", event.payload.tool_name);
            const { setPendingRequest } = usePermissionStore.getState();
            const detectedRisks = (event.payload.detected_risks || []).map(r => ({
              category: r.category,
              severity: (validRiskLevels.includes(r.severity) ? r.severity : "medium") as import("./state/permissionStore").RiskLevel,
              description: r.description,
              mitigation: r.mitigation,
            }));
            const rawArgs = (event.payload as { arguments?: Record<string, unknown> }).arguments;
            const sessionFromEvent = (event.payload as { session_id?: string }).session_id;
            setPendingRequest({
              allowed: false,
              requires_approval: true,
              request_id: event.payload.request_id,
              tool_name: event.payload.tool_name,
              risk_level: (validRiskLevels.includes(riskLevel) ? riskLevel : "medium") as import("./state/permissionStore").RiskLevel,
              risk_description: event.payload.risk_description,
              detected_risks: detectedRisks,
              recommendations: event.payload.recommendations || [],
              arguments: rawArgs,
              session_id: sessionFromEvent,
            });
            console.log("[Permission] Pending request set");
          } catch (error) {
            console.error("Error handling permission request:", error);
          }
        });
        return unlisten;
      } catch (error) {
        console.error("Error setting up permission listener:", error);
        return () => {};
      }
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    }).catch((error) => {
      console.error("Failed to setup permission listener:", error);
    });

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, []);

  return (
    <>
      <Layout>
        <Stack
          direction="row"
          sx={{
            flex: 1,
            minHeight: 0,
            minWidth: 0,
            width: "100%",
          }}
        >
          {/* Left: conversations */}
          <Paper
            id="omiga-session-panel"
            component="aside"
            elevation={0}
            square
            sx={{
              width: leftW,
              flexShrink: 0,
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
              borderRadius: 0,
              borderRight: 1,
              borderColor: "divider",
              bgcolor: "background.paper",
            }}
          >
            <ErrorBoundary label="Session list">
              <SessionList
                onSelectSession={() => {
                  setRightPanelMode("default");
                  setSettingsOpen(false);
                }}
              />
            </ErrorBoundary>
          </Paper>

          <ResizeHandle
            direction="horizontal"
            onResize={(d) => resizeLeftBy(d)}
          />

          {rightPanelMode === "settings" ? (
            /* Settings covers center + right: code/chat + file tree area */
            <Paper
              component="section"
              elevation={0}
              square
              sx={{
                flex: 1,
                minWidth: 0,
                minHeight: 0,
                display: "flex",
                flexDirection: "column",
                overflow: "hidden",
                borderRadius: 0,
                borderLeft: 1,
                borderColor: "divider",
                bgcolor: "background.paper",
              }}
            >
              <Box
                sx={{
                  flex: 1,
                  minHeight: 0,
                  display: "flex",
                  flexDirection: "column",
                  overflow: "hidden",
                }}
              >
                <Settings
                  open={true}
                  initialTab={settingsTabIndex}
                  onClose={() => {
                    setSettingsOpen(false);
                    setRightPanelMode("default");
                    setSettingsTabIndex(0);
                  }}
                />
              </Box>
            </Paper>
          ) : (
            <>
              {/* Center: code + chat */}
              <Paper
                ref={centerRef}
                component="section"
                elevation={0}
                square
                sx={{
                  flex: 1,
                  minWidth: 0,
                  minHeight: 0,
                  display: "flex",
                  flexDirection: "column",
                  overflow: "hidden",
                  borderRadius: 0,
                  bgcolor: "background.default",
                  boxShadow: `inset 0 1px 0 ${alpha(theme.palette.common.black, 0.04)}`,
                }}
              >
                {hasCodeWorkspace && (
                  <>
                    <Box
                      sx={{
                        height: codeH,
                        minHeight: LAYOUT_PANEL_MIN,
                        flexShrink: 0,
                        display: "flex",
                        flexDirection: "column",
                        overflow: "hidden",
                      }}
                    >
                      <CodeWorkspace />
                    </Box>

                    <ResizeHandle
                      direction="vertical"
                      onResize={(d) => {
                        const el = centerRef.current;
                        const codeMin = LAYOUT_PANEL_MIN;
                        const max = el
                          ? Math.max(codeMin, el.clientHeight - codeMin)
                          : 600;
                        resizeCodeBy(d, max);
                      }}
                    />
                  </>
                )}

                <Box
                  sx={{
                    flex: 1,
                    minHeight: 0,
                    minWidth: 0,
                    display: "flex",
                    flexDirection: "column",
                    bgcolor: "background.paper",
                  }}
                >
                  <ErrorBoundary label="Chat">
                    <Chat sessionId={sessionId} />
                  </ErrorBoundary>
                </Box>
              </Paper>

              <ResizeHandle
                direction="horizontal"
                onResize={(d) => resizeRightBy(d)}
              />

              <Paper
                ref={rightRef}
                component="aside"
                elevation={0}
                square
                sx={{
                  width: rightW,
                  flexShrink: 0,
                  display: "flex",
                  flexDirection: "column",
                  overflow: "hidden",
                  borderRadius: 0,
                  borderLeft: 1,
                  borderColor: "divider",
                  bgcolor: "background.paper",
                }}
              >
                <Box
                  sx={{
                    height: tasksH,
                    minHeight: LAYOUT_PANEL_MIN,
                    flexShrink: 0,
                    overflow: "auto",
                    display: "flex",
                    flexDirection: "column",
                  }}
                >
                  <TaskStatus />
                </Box>

                <ResizeHandle
                  direction="vertical"
                  onResize={(d) => {
                    const el = rightRef.current;
                    const max = el
                      ? Math.max(
                          LAYOUT_PANEL_MIN,
                          el.clientHeight - LAYOUT_PANEL_MIN,
                        )
                      : 500;
                    resizeTasksBy(d, max);
                  }}
                />

                <Box
                  sx={{
                    flex: 1,
                    minHeight: 0,
                    display: "flex",
                    flexDirection: "column",
                  }}
                >
                  <FileTree />
                </Box>
              </Paper>
            </>
          )}
        </Stack>
      </Layout>
    </>
  );
}
