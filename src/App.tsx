import { lazy, Suspense, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Box, Paper, Stack, useTheme, alpha } from "@mui/material";
import { Layout } from "./components/Layout";
import { Chat } from "./components/Chat";
import { FileTree } from "./components/FileTree";
import { SessionList } from "./components/SessionList";
import { OPEN_SETTINGS_TAB_DETAIL } from "./components/Settings/openSettingsTabMap";
import { TaskStatus } from "./components/TaskStatus";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ResizeHandle } from "./components/ResizeHandle";
import { OnboardingWizard } from "./components/Onboarding";
import { ConfirmationDialog } from "./components/AgentSchedule/AgentScheduleLauncher";
import { useAgentStore } from "./state/agentStore";
import {
  useSessionStore,
  useWorkspaceStore,
  useUiStore,
  usePermissionStore,
  useExtensionStore,
  LAYOUT_LEFT_MIN,
  LAYOUT_LEFT_MAX,
  LAYOUT_RIGHT_MIN,
  LAYOUT_RIGHT_MAX,
  LAYOUT_PANEL_MIN,
} from "./state";
import { listenTauriEvent } from "./utils/tauriEvents";
import {
  defaultWebSearchQuerySettings,
  parseStoredWebSearchSettings,
  WEB_SEARCH_KEYS_STORAGE,
} from "./utils/webSearchSettings";
import { getLocalStorageItem } from "./utils/browserStorage";

const CodeWorkspace = lazy(() =>
  import("./components/CodeWorkspace").then((mod) => ({
    default: mod.CodeWorkspace,
  })),
);

const Settings = lazy(() =>
  import("./components/Settings").then((mod) => ({
    default: mod.Settings,
  })),
);

function clamp(n: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, n));
}

function PanelLoadingFallback({ label }: { label: string }) {
  return (
    <Box
      sx={{
        flex: 1,
        minHeight: 0,
        display: "grid",
        placeItems: "center",
        color: "text.secondary",
        fontSize: 13,
      }}
    >
      {label}
    </Box>
  );
}

export default function App() {
  const theme = useTheme();
  const { currentSession, loadSessions } = useSessionStore();

  const onboardingCompleted = useUiStore((s) => s.onboardingCompleted);
  const setOnboardingCompleted = useUiStore((s) => s.setOnboardingCompleted);
  const setSettingsOpen = useUiStore((s) => s.setSettingsOpen);
  const settingsTabIndex = useUiStore((s) => s.settingsTabIndex);
  const setSettingsTabIndex = useUiStore((s) => s.setSettingsTabIndex);
  const settingsExecutionSubTab = useUiStore((s) => s.settingsExecutionSubTab);
  const setSettingsExecutionSubTab = useUiStore((s) => s.setSettingsExecutionSubTab);
  const rightPanelMode = useUiStore((s) => s.rightPanelMode);
  const setRightPanelMode = useUiStore((s) => s.setRightPanelMode);
  const leftW = useUiStore((s) => s.leftPanelWidth);
  const rightW = useUiStore((s) => s.rightPanelWidth);
  const codeH = useUiStore((s) => s.codePanelHeight);
  const tasksH = useUiStore((s) => s.tasksPanelHeight);
  const setLeftWidth = useUiStore((s) => s.setLeftWidth);
  const setRightWidth = useUiStore((s) => s.setRightWidth);
  const setCodeHeight = useUiStore((s) => s.setCodeHeight);
  const setTasksHeight = useUiStore((s) => s.setTasksHeight);
  const ensureCodePanelMin = useUiStore((s) => s.ensureCodePanelMin);

  const leftRef = useRef<HTMLDivElement>(null);
  const centerRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const codePanelRef = useRef<HTMLDivElement>(null);
  const tasksPanelRef = useRef<HTMLDivElement>(null);
  const leftWidthRef = useRef(leftW);
  const rightWidthRef = useRef(rightW);
  const codeHeightRef = useRef(codeH);
  const tasksHeightRef = useRef(tasksH);

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
    leftWidthRef.current = leftW;
    if (leftRef.current) leftRef.current.style.width = `${leftW}px`;
  }, [leftW]);

  useEffect(() => {
    rightWidthRef.current = rightW;
    if (rightRef.current) rightRef.current.style.width = `${rightW}px`;
  }, [rightW]);

  useEffect(() => {
    codeHeightRef.current = codeH;
    if (codePanelRef.current) codePanelRef.current.style.height = `${codeH}px`;
  }, [codeH]);

  useEffect(() => {
    tasksHeightRef.current = tasksH;
    if (tasksPanelRef.current) tasksPanelRef.current.style.height = `${tasksH}px`;
  }, [tasksH]);

  const previewLeftResize = useCallback((delta: number) => {
    const next = clamp(
      leftWidthRef.current + delta,
      LAYOUT_LEFT_MIN,
      LAYOUT_LEFT_MAX,
    );
    if (next === leftWidthRef.current) return;
    leftWidthRef.current = next;
    if (leftRef.current) leftRef.current.style.width = `${next}px`;
  }, []);

  const commitLeftResize = useCallback(() => {
    setLeftWidth(leftWidthRef.current);
  }, [setLeftWidth]);

  const previewRightResize = useCallback((delta: number) => {
    const next = clamp(
      rightWidthRef.current - delta,
      LAYOUT_RIGHT_MIN,
      LAYOUT_RIGHT_MAX,
    );
    if (next === rightWidthRef.current) return;
    rightWidthRef.current = next;
    if (rightRef.current) rightRef.current.style.width = `${next}px`;
  }, []);

  const commitRightResize = useCallback(() => {
    setRightWidth(rightWidthRef.current);
  }, [setRightWidth]);

  const previewCodeResize = useCallback((delta: number) => {
    const next = clampCodeH(codeHeightRef.current + delta);
    if (next === codeHeightRef.current) return;
    codeHeightRef.current = next;
    if (codePanelRef.current) codePanelRef.current.style.height = `${next}px`;
  }, [clampCodeH]);

  const commitCodeResize = useCallback(() => {
    setCodeHeight(codeHeightRef.current);
  }, [setCodeHeight]);

  const previewTasksResize = useCallback((delta: number) => {
    const next = clampTasksH(tasksHeightRef.current + delta);
    if (next === tasksHeightRef.current) return;
    tasksHeightRef.current = next;
    if (tasksPanelRef.current) tasksPanelRef.current.style.height = `${next}px`;
  }, [clampTasksH]);

  const commitTasksResize = useCallback(() => {
    setTasksHeight(tasksHeightRef.current);
  }, [setTasksHeight]);

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

  // Initialize agent event listeners at app level so background-agent-update,
  // background-agent-complete, agent-schedule-complete, and
  // agent-schedule-confirmation-required are always active regardless of panel visibility.
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    useAgentStore.getState().initEventListeners().then((fn) => {
      cleanup = fn;
    });
    return () => cleanup?.();
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      await loadSessions();
      if (cancelled) return;
      // Auto-select the most recent session if one exists, but never auto-create.
      // New sessions are created only when the user explicitly clicks "New session"
      // or sends their first message.
      const { currentSession, sessions, setCurrentSession } =
        useSessionStore.getState();
      if (!currentSession && sessions.length > 0) {
        try {
          await setCurrentSession(sessions[0].id);
        } catch (e) {
          console.error("[App] auto-select most recent session failed", e);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [loadSessions]);

  useEffect(() => {
    void useExtensionStore.getState().loadExtensions();
  }, []);

  // Listen for cron job fired events — create an AI session and send the task
  useEffect(() => {
    const unlistenPromise = listenTauriEvent<{
      id: string;
      task: string;
      firedAt: string;
    }>("cron-job-fired", async (event) => {
      const { task, id } = event.payload;
      try {
        const sessionTitle = `[定时任务] ${task.slice(0, 40)}${task.length > 40 ? "…" : ""}`;
        await useSessionStore.getState().createSession(sessionTitle, ".");
        const sessionId = useSessionStore.getState().currentSession?.id;
        if (!sessionId) throw new Error("session id unavailable after createSession");
        await useSessionStore.getState().sendMessage({
          content: `[定时任务触发 ID: ${id}]\n\n${task}`,
          session_id: sessionId,
          use_tools: true,
        });
      } catch (e) {
        console.warn("[cron] failed to create AI session:", e);
        try {
          new Notification("Omiga — 定时任务", {
            body: task.length > 100 ? task.slice(0, 97) + "…" : task,
            silent: false,
          });
        } catch {
          console.info("[cron] job fired:", task);
        }
      }
    });
    return () => { void unlistenPromise.then((f) => f()); };
  }, []);

  useEffect(() => {
    const raw = getLocalStorageItem(WEB_SEARCH_KEYS_STORAGE);
    if (raw) {
      const payload = parseStoredWebSearchSettings(raw);
      if (payload) void invoke("set_web_search_api_keys", { ...payload }).catch(() => {});
      return;
    }
    const tavily = getLocalStorageItem("omiga_tavily_search_api_key");
    const legacy = getLocalStorageItem("omiga_brave_search_api_key") ?? undefined;
    const legacyTavily = (tavily?.trim() ? tavily : legacy)?.trim();
    if (legacyTavily) {
      void invoke("set_web_search_api_keys", {
        tavily: legacyTavily,
        exa: "",
        parallel: "",
        firecrawl: "",
        firecrawlUrl: "",
        semanticScholarEnabled: false,
        semanticScholarApiKey: "",
        wechatSearchEnabled: false,
        pubmedApiKey: "",
        pubmedEmail: "omiga@example.invalid",
        pubmedToolName: "omiga",
        ...defaultWebSearchQuerySettings(),
      }).catch(() => {});
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
      const raw = getLocalStorageItem("omiga_llm_config");
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
        request: {
          provider: parsed.provider,
          apiKey: parsed.apiKey.trim(),
          secretKey: parsed.secretKey,
          appId: parsed.appId,
          model: parsed.model?.trim() || undefined,
          baseUrl: parsed.baseUrl,
        },
      }).catch(() => {});
    })();
  }, []);

  useEffect(() => {
    const open = (e: Event) => {
      const detail = (e as CustomEvent<{ tab?: string; executionSubTab?: number }>)
        .detail;
      const key = detail?.tab;
      const idx =
        key != null && OPEN_SETTINGS_TAB_DETAIL[key] !== undefined
          ? OPEN_SETTINGS_TAB_DETAIL[key]
          : 0;
      setSettingsTabIndex(idx);
      if (
        detail?.executionSubTab != null &&
        Number.isFinite(detail.executionSubTab)
      ) {
        setSettingsExecutionSubTab(detail.executionSubTab);
      }
      setSettingsOpen(true);
      setRightPanelMode("settings");
    };
    window.addEventListener("openSettings", open);
    return () => window.removeEventListener("openSettings", open);
  }, [
    setSettingsOpen,
    setRightPanelMode,
    setSettingsTabIndex,
    setSettingsExecutionSubTab,
  ]);

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
        const unlisten = await listenTauriEvent<{
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
          project_root?: string | null;
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
            const projectRootFromEvent = (event.payload as { project_root?: string | null }).project_root;
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
              project_root: projectRootFromEvent ?? undefined,
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
      {!onboardingCompleted && (
        <OnboardingWizard onComplete={() => setOnboardingCompleted(true)} />
      )}
      {/* Orchestration confirmation dialog — mounted at root so it shows regardless of projectRoot */}
      <ConfirmationDialog />
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
            ref={leftRef}
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
            onResize={previewLeftResize}
            onResizeEnd={commitLeftResize}
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
                <Suspense fallback={<PanelLoadingFallback label="正在加载设置…" />}>
                  <Settings
                    open={true}
                    initialTab={settingsTabIndex}
                    initialExecutionSubTab={settingsExecutionSubTab}
                    onClose={() => {
                      setSettingsOpen(false);
                      setRightPanelMode("default");
                      setSettingsTabIndex(0);
                      setSettingsExecutionSubTab(0);
                    }}
                  />
                </Suspense>
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
                      ref={codePanelRef}
                      sx={{
                        height: codeH,
                        minHeight: LAYOUT_PANEL_MIN,
                        flexShrink: 0,
                        display: "flex",
                        flexDirection: "column",
                        overflow: "hidden",
                      }}
                    >
                      <Suspense fallback={<PanelLoadingFallback label="正在加载编辑器…" />}>
                        <CodeWorkspace />
                      </Suspense>
                    </Box>

                    <ResizeHandle
                      direction="vertical"
                      onResize={previewCodeResize}
                      onResizeEnd={commitCodeResize}
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
                onResize={previewRightResize}
                onResizeEnd={commitRightResize}
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
                  position: "relative",
                }}
              >
                <Box
                  ref={tasksPanelRef}
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
                  onResize={previewTasksResize}
                  onResizeEnd={commitTasksResize}
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
