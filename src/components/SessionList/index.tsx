import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import {
  Box,
  Typography,
  IconButton,
  Menu,
  MenuItem,
  ListItemIcon,
  ListItemText,
  Divider,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  TextField,
  Button,
  Stack,
  InputBase,
  alpha,
  useTheme,
  Alert,
} from "@mui/material";
import {
  MoreVert,
  Delete,
  Edit,
  Add,
  Search,
  BusinessCenterOutlined,
  FolderOutlined,
  UnfoldMore,
  Settings as SettingsIcon,
  Language as LanguageIcon,
  Palette as PaletteIcon,
  Extension as ExtensionIcon,
  Link as LinkIcon,
  Storage as StorageIcon,
  AutoAwesome as AutoAwesomeIcon,
  HelpOutline as HelpOutlineIcon,
  MenuBook as MenuBookIcon,
  Logout as LogoutIcon,
  GitHub as GitHubIcon,
  ChevronRight,
} from "@mui/icons-material";
import { openUrl } from "@tauri-apps/plugin-opener";
import { OMIGA_GITHUB_RELEASES_URL } from "../../constants/appLinks";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  useSessionStore,
  UNUSED_SESSION_LABEL,
  shouldShowNewSessionPlaceholder,
  type Session,
} from "../../state/sessionStore";
import { useLocaleStore } from "../../state";
import {
  tSessionList,
  type SessionListStringKey,
} from "../../i18n/sessionListStrings";
import { invokeIfTauri } from "../../utils/tauriRuntime";
import { OmigaLogo } from "../OmigaLogo";
import { useActivityStore } from "../../state/activityStore";
import {
  useStreamRegistryVersion,
  isBackgroundSessionRunning,
} from "../../state/sessionStreamRegistry";

interface SessionListProps {
  onSelectSession?: () => void;
}

/** External links — adjust when Omiga has public docs */
const HELP_CENTER_URL = "https://support.anthropic.com/";
const LEARN_MORE_URL = "https://www.anthropic.com/claude";
const LANGUAGE_SUBMENU_BRIDGE_MS = 90;

function sessionProjectLabel(session: {
  workingDirectory?: string;
  projectPath?: string;
}): string {
  const raw = (session.workingDirectory ?? session.projectPath ?? "").trim();
  if (!raw || raw === ".") return "";
  const normalized = raw.replace(/\\/g, "/").replace(/\/+$/u, "");
  const last = normalized.split("/").filter(Boolean).pop();
  return last ?? raw;
}

interface SessionSearchSummary {
  id: string;
  name: string;
  project_path: string;
  message_count: number;
  updated_at: string;
  match_snippet?: string | null;
}

interface SessionSearchRow {
  session: Session;
  isPlaceholder: boolean;
  matchSnippet?: string | null;
}

export function SessionList({ onSelectSession }: SessionListProps) {
  const theme = useTheme();
  const locale = useLocaleStore((s) => s.locale);
  const setLocale = useLocaleStore((s) => s.setLocale);
  const t = (key: SessionListStringKey) => tSessionList(locale, key);

  // ── Selective subscriptions ───────────────────────────────────────────────
  // Subscribe to storeMessages.length (a primitive) instead of the full array.
  // During streaming, storeMessages grows with every chunk; subscribing to the
  // full array would re-render SessionList (and re-run filteredSessions useMemo)
  // on every token — completely unnecessary since SessionList only needs to know
  // whether the current session has messages (for placeholder detection).
  const sessions = useSessionStore((s) => s.sessions);
  const currentSession = useSessionStore((s) => s.currentSession);
  const isLoading = useSessionStore((s) => s.isLoading);
  const storeMessagesLength = useSessionStore((s) => s.storeMessages.length);
  const setCurrentSession = useSessionStore((s) => s.setCurrentSession);
  const loadSessions = useSessionStore((s) => s.loadSessions);
  const loadSession = useSessionStore((s) => s.loadSession);
  const deleteSession = useSessionStore((s) => s.deleteSession);
  const renameSession = useSessionStore((s) => s.renameSession);
  const createSessionQuick = useSessionStore((s) => s.createSessionQuick);

  // ── Running-session indicators ────────────────────────────────────────────
  // Current session: subscribe to activityStore (updates on every stream event).
  // Background sessions: subscribe to the registry version counter — it bumps
  // whenever a background snapshot is saved/cleared, which is much less frequent.
  // This avoids re-rendering the sidebar on every streaming token.
  const currentIsConnecting = useActivityStore((s) => s.isConnecting);
  const currentIsStreaming = useActivityStore((s) => s.isStreaming);
  const currentSessionRunning = currentIsConnecting || currentIsStreaming;
  useStreamRegistryVersion(); // subscribe — re-renders when any background session changes

  const isSessionRunning = (sessionId: string): boolean => {
    if (sessionId === currentSession?.id) return currentSessionRunning;
    return isBackgroundSessionRunning(sessionId);
  };

  // ── Hover / mousedown prefetch ───────────────────────────────────────────
  // Three complementary strategies to warm the message cache before a click:
  //
  // 1. Idle prefetch (sequential): After 250 ms of inactivity, load the top 20
  //    sessions one at a time. Sequential avoids flooding the macOS WKWebView IPC
  //    queue (concurrent invoke() calls serialise during renders, so batching them
  //    would stall all 20 — sequential gives the first result in ~200-800 ms and
  //    the rest follow as fast as the bridge can drain).
  //
  // 2. Hover prefetch: On mouseenter, start a prefetch after 30 ms. Filters out
  //    accidental passes.
  //
  // 3. Mousedown prefetch: On mousedown fire an immediate prefetch. Even if the
  //    IPC is already in-flight from hover, mousedown is a no-op (prefetchingRef
  //    guard). When combined with the hover prefetch it buys an extra ~100-200 ms
  //    headstart over the click handler.
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const prefetchingRef = useRef<Set<string>>(new Set());
  const idlePrefetchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const idlePrefetchAbortRef = useRef<{ aborted: boolean } | null>(null);

  const prefetchSession = (sessionId: string) => {
    if (sessionId === currentSession?.id) return;
    if (prefetchingRef.current.has(sessionId)) return;
    prefetchingRef.current.add(sessionId);
    loadSession(sessionId, { silent: true })
      .catch(() => {})
      .finally(() => {
        prefetchingRef.current.delete(sessionId);
      });
  };

  const handleSessionMouseEnter = (sessionId: string) => {
    if (sessionId === currentSession?.id) return;
    if (hoverTimerRef.current !== null) {
      clearTimeout(hoverTimerRef.current);
    }
    if (prefetchingRef.current.has(sessionId)) return;
    hoverTimerRef.current = setTimeout(() => {
      prefetchSession(sessionId);
    }, 30);
  };

  const handleSessionMouseLeave = () => {
    if (hoverTimerRef.current !== null) {
      clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
    }
  };

  const handleSessionMouseDown = (sessionId: string) => {
    prefetchSession(sessionId);
  };

  const [menuAnchorEl, setMenuAnchorEl] = useState<null | HTMLElement>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(
    null,
  );
  const [userMenuAnchorEl, setUserMenuAnchorEl] = useState<null | HTMLElement>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [selectError, setSelectError] = useState<string | null>(null);
  /** Anchor for Language → English / 中文 submenu */
  const [languageSubmenuAnchor, setLanguageSubmenuAnchor] =
    useState<null | HTMLElement>(null);
  const languageSubmenuLeaveTimerRef = useRef<ReturnType<
    typeof setTimeout
  > | null>(null);

  // Load sessions on mount
  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

  useEffect(() => {
    // Abort any in-progress sequential prefetch from a previous run.
    if (idlePrefetchAbortRef.current) {
      idlePrefetchAbortRef.current.aborted = true;
      idlePrefetchAbortRef.current = null;
    }
    if (idlePrefetchTimerRef.current !== null) {
      clearTimeout(idlePrefetchTimerRef.current);
      idlePrefetchTimerRef.current = null;
    }
    if (sessions.length <= 1) return;
    // Prefetch up to 20 sessions (most-recently-used first) sequentially so we
    // don't flood the macOS WKWebView IPC queue with concurrent invoke() calls.
    const targetIds = sessions
      .filter((s) => s.id !== currentSession?.id)
      .slice(0, 20)
      .map((s) => s.id);
    if (targetIds.length === 0) return;

    const abort = { aborted: false };
    idlePrefetchAbortRef.current = abort;

    const runSequential = async () => {
      for (const id of targetIds) {
        if (abort.aborted) return;
        // Skip if already cached (prefetchingRef check is inside prefetchSession).
        await loadSession(id, { silent: true }).catch(() => {});
      }
    };

    idlePrefetchTimerRef.current = setTimeout(() => {
      idlePrefetchTimerRef.current = null;
      void runSequential();
    }, 250);

    return () => {
      abort.aborted = true;
      idlePrefetchAbortRef.current = null;
      if (idlePrefetchTimerRef.current !== null) {
        clearTimeout(idlePrefetchTimerRef.current);
        idlePrefetchTimerRef.current = null;
      }
    };
  }, [sessions, currentSession?.id, loadSession]);

  useEffect(() => {
    document.documentElement.lang = locale === "zh-CN" ? "zh-CN" : "en";
  }, [locale]);

  const handleMenuOpen = (
    event: React.MouseEvent<HTMLElement>,
    sessionId: string,
  ) => {
    event.stopPropagation();
    setMenuAnchorEl(event.currentTarget);
    setSelectedSessionId(sessionId);
  };

  const handleMenuClose = () => {
    setMenuAnchorEl(null);
    setSelectedSessionId(null);
  };

  const handleSelectSession = async (sessionId: string) => {
    setSelectError(null);
    // T0: user click
    performance.mark("sw:click");
    (window as unknown as { __swClickAt?: number }).__swClickAt = performance.now();
    console.debug("[OmigaDebug][SessionList] click session", sessionId);
    try {
      await setCurrentSession(sessionId);
      console.debug(
        "[OmigaDebug][SessionList] setCurrentSession finished",
        sessionId,
      );
      onSelectSession?.();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.error("[OmigaDebug][SessionList] setCurrentSession failed", e);
      setSelectError(msg);
    }
  };

  const handleDelete = async () => {
    if (!selectedSessionId) return;
    setIsDeleting(true);
    try {
      await deleteSession(selectedSessionId);
    } finally {
      setIsDeleting(false);
      handleMenuClose();
    }
  };

  const handleRenameClick = () => {
    const session = sessions.find((s) => s.id === selectedSessionId);
    if (session) {
      setNewName(session.name);
      setRenameDialogOpen(true);
    }
    handleMenuClose();
  };

  const handleRenameConfirm = async () => {
    if (!selectedSessionId || !newName.trim()) return;
    await renameSession(selectedSessionId, newName.trim());
    setRenameDialogOpen(false);
    setNewName("");
  };

  const handleUserMenuOpen = (event: React.MouseEvent<HTMLElement>) => {
    setUserMenuAnchorEl(event.currentTarget);
  };

  const clearLanguageSubmenuLeaveTimer = useCallback(() => {
    if (languageSubmenuLeaveTimerRef.current) {
      clearTimeout(languageSubmenuLeaveTimerRef.current);
      languageSubmenuLeaveTimerRef.current = null;
    }
  }, []);

  const handleUserMenuClose = useCallback(() => {
    clearLanguageSubmenuLeaveTimer();
    setLanguageSubmenuAnchor(null);
    setUserMenuAnchorEl(null);
  }, [clearLanguageSubmenuLeaveTimer]);

  const closeLanguageSubmenuNow = useCallback(() => {
    clearLanguageSubmenuLeaveTimer();
    setLanguageSubmenuAnchor(null);
  }, [clearLanguageSubmenuLeaveTimer]);

  useEffect(() => {
    if (!userMenuAnchorEl && languageSubmenuAnchor) {
      closeLanguageSubmenuNow();
    }
  }, [closeLanguageSubmenuNow, languageSubmenuAnchor, userMenuAnchorEl]);

  useEffect(() => {
    if (!userMenuAnchorEl && !languageSubmenuAnchor) return;

    const closeMenus = () => handleUserMenuClose();
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (
        target?.closest(
          '[data-omiga-floating-menu="user"], [data-omiga-floating-menu="language"]',
        )
      ) {
        return;
      }
      closeMenus();
    };

    window.addEventListener("openSettings", closeMenus);
    window.addEventListener("blur", closeMenus);
    document.addEventListener("pointerdown", handlePointerDown, true);
    return () => {
      window.removeEventListener("openSettings", closeMenus);
      window.removeEventListener("blur", closeMenus);
      document.removeEventListener("pointerdown", handlePointerDown, true);
    };
  }, [handleUserMenuClose, languageSubmenuAnchor, userMenuAnchorEl]);

  const handleOpenSettings = (tab?: string) => {
    handleUserMenuClose();
    window.dispatchEvent(new CustomEvent("openSettings", { detail: { tab } }));
  };

  const openLanguageSubmenu = (el: HTMLElement) => {
    clearLanguageSubmenuLeaveTimer();
    setLanguageSubmenuAnchor(el);
  };

  const scheduleCloseLanguageSubmenu = () => {
    clearLanguageSubmenuLeaveTimer();
    languageSubmenuLeaveTimerRef.current = setTimeout(() => {
      setLanguageSubmenuAnchor(null);
    }, LANGUAGE_SUBMENU_BRIDGE_MS);
  };

  const handleOpenHelp = async () => {
    handleUserMenuClose();
    try {
      await openUrl(HELP_CENTER_URL);
    } catch (e) {
      console.error("[SessionList] open help URL failed", e);
    }
  };

  const handleLearnMore = async () => {
    handleUserMenuClose();
    try {
      await openUrl(LEARN_MORE_URL);
    } catch (e) {
      console.error("[SessionList] open learn more URL failed", e);
    }
  };

  const handleOpenGithubUpdates = async () => {
    handleUserMenuClose();
    try {
      await openUrl(OMIGA_GITHUB_RELEASES_URL);
    } catch (e) {
      console.error("[SessionList] open GitHub releases URL failed", e);
    }
  };

  const handleLogOut = async () => {
    handleUserMenuClose();
    try {
      await getCurrentWindow().close();
    } catch (e) {
      console.error("[SessionList] close window failed", e);
      try {
        window.close();
      } catch {
        /* ignore */
      }
    }
  };

  const handleCreateClick = async () => {
    setSelectError(null);
    try {
      await createSessionQuick();
      onSelectSession?.();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.error("[SessionList] createSessionQuick failed", e);
      setSelectError(msg);
    }
  };

  const [searchDialogOpen, setSearchDialogOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [contentSearchRows, setContentSearchRows] = useState<SessionSearchRow[] | null>(
    null,
  );
  const [contentSearchLoading, setContentSearchLoading] = useState(false);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const searchSeqRef = useRef(0);

  useEffect(() => {
    if (!searchDialogOpen) return;
    const id = window.requestAnimationFrame(() => {
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(id);
  }, [searchDialogOpen]);

  const handleOpenSearchDialog = () => {
    setSearchDialogOpen(true);
  };

  const handleCloseSearchDialog = () => {
    setSearchDialogOpen(false);
  };

  // Compute isPlaceholder once per session per render, then derive the modal
  // search results from that stable row list.
  const sessionRows = useMemo<SessionSearchRow[]>(() => {
    const currentId = currentSession?.id;
    const result: SessionSearchRow[] = [];
    for (const s of sessions) {
      const isCurrent = s.id === currentId;
      const isPlaceholder = shouldShowNewSessionPlaceholder(s, {
        isCurrentSession: isCurrent,
        storeMessageCount: isCurrent ? storeMessagesLength : undefined,
      });
      result.push({ session: s, isPlaceholder });
    }
    return result;
  }, [sessions, currentSession?.id, storeMessagesLength]);

  const sessionRowsById = useMemo(() => {
    const rowsById = new Map<string, SessionSearchRow>();
    for (const row of sessionRows) rowsById.set(row.session.id, row);
    return rowsById;
  }, [sessionRows]);

  const filteredSessions = useMemo(() => {
    const q = searchQuery.toLowerCase().trim();
    if (!q) return sessionRows;
    return sessionRows.filter(({ session, isPlaceholder }) => {
      const label = (isPlaceholder ? UNUSED_SESSION_LABEL : session.name).toLowerCase();
      const project = sessionProjectLabel(session).toLowerCase();
      return (
        session.name.toLowerCase().includes(q) ||
        label.includes(q) ||
        project.includes(q)
      );
    });
  }, [sessionRows, searchQuery]);

  useEffect(() => {
    const seq = searchSeqRef.current + 1;
    searchSeqRef.current = seq;

    if (!searchDialogOpen) {
      setContentSearchLoading(false);
      return;
    }

    const q = searchQuery.trim();

    if (!q) {
      setContentSearchRows(null);
      setContentSearchLoading(false);
      return;
    }

    setContentSearchLoading(true);
    const timer = window.setTimeout(() => {
      void invokeIfTauri<SessionSearchSummary[]>("search_sessions", {
        query: q,
        limit: 50,
      })
        .then((rows) => {
          if (searchSeqRef.current !== seq) return;

          if (!rows) {
            setContentSearchRows(null);
            return;
          }

          setContentSearchRows(
            rows.map((row) => {
              const existing = sessionRowsById.get(row.id);
              if (existing) {
                return {
                  ...existing,
                  matchSnippet: row.match_snippet ?? null,
                };
              }

              const projectPath = row.project_path || ".";
              const session: Session = {
                id: row.id,
                name: row.name,
                projectPath,
                workingDirectory: projectPath,
                createdAt: row.updated_at,
                updatedAt: row.updated_at,
                messageCount: row.message_count,
              };
              const isCurrent = session.id === currentSession?.id;

              return {
                session,
                isPlaceholder: shouldShowNewSessionPlaceholder(session, {
                  isCurrentSession: isCurrent,
                  storeMessageCount: isCurrent ? storeMessagesLength : undefined,
                }),
                matchSnippet: row.match_snippet ?? null,
              };
            }),
          );
        })
        .catch((err) => {
          if (searchSeqRef.current !== seq) return;
          console.error("[SessionList] search_sessions failed", err);
          setContentSearchRows(null);
        })
        .finally(() => {
          if (searchSeqRef.current === seq) setContentSearchLoading(false);
        });
    }, 140);

    return () => window.clearTimeout(timer);
  }, [
    searchDialogOpen,
    searchQuery,
    sessionRowsById,
    currentSession?.id,
    storeMessagesLength,
  ]);

  const searchResults =
    searchQuery.trim() && contentSearchRows ? contentSearchRows : filteredSessions;

  const handleSearchResultClick = (sessionId: string) => {
    setSearchDialogOpen(false);
    void handleSelectSession(sessionId);
  };

  const handleSearchKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    const ne = e.nativeEvent;
    if (ne.isComposing || ne.keyCode === 229) return;
    if (e.key === "Enter" && searchResults.length > 0) {
      e.preventDefault();
      handleSearchResultClick(searchResults[0].session.id);
    }
  };

  const navTextSx = useMemo(
    () => ({
      fontSize: 14,
      fontWeight: 500,
      color: theme.palette.text.primary,
      lineHeight: 1.3,
    }),
    [theme.palette.text.primary],
  );

  const navRowSx = useMemo(
    () => ({
      display: "flex",
      alignItems: "center",
      gap: 1.25,
      px: 1.5,
      py: 1,
      borderRadius: 1,
      cursor: "pointer",
      color: theme.palette.text.primary,
      "&:hover": {
        bgcolor: "action.hover",
      },
    }),
    [theme.palette.text.primary],
  );

  // Only block the whole panel on the initial list fetch — not when switching sessions
  if (isLoading && sessions.length === 0) {
    return (
      <Box sx={{ p: 2, flex: 1 }}>
        {[1, 2, 3, 4].map((i) => (
          <Box
            key={i}
            sx={{
              height: 64,
              mb: 1,
              borderRadius: 2,
              bgcolor: alpha(theme.palette.divider, 0.5),
            }}
          />
        ))}
      </Box>
    );
  }

  return (
    <Box
      sx={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        bgcolor: "transparent",
      }}
    >
      {selectError && (
        <Alert
          severity="error"
          onClose={() => setSelectError(null)}
          sx={{ m: 1, mb: 0 }}
        >
          {t("sessionSwitchErrorPrefix")} {selectError}
        </Alert>
      )}

      {/* App logo */}
      <Box sx={{ px: 2, pt: 2, pb: 1, display: "flex", alignItems: "center", gap: 1 }}>
        <OmigaLogo size={18} />
        <Typography variant="subtitle1" fontWeight={700} sx={{ letterSpacing: -0.3, color: "text.primary" }}>
          Omiga
        </Typography>
      </Box>

      {/* Top nav: icon + label (reference layout) */}
      <Stack spacing={0} sx={{ p: 1.5, pb: 1 }}>
        <Box
          sx={navRowSx}
          onClick={() => {
            handleCreateClick();
          }}
        >
          <Add sx={{ fontSize: 20, color: "text.secondary" }} />
          <Typography sx={navTextSx}>{t("newSession")}</Typography>
        </Box>
        <Box
          sx={navRowSx}
          onClick={handleOpenSearchDialog}
        >
          <Search sx={{ fontSize: 20, color: "text.secondary" }} />
          <Typography sx={navTextSx}>{t("search")}</Typography>
        </Box>
        <Box
          sx={navRowSx}
          onClick={() => {
            window.dispatchEvent(
              new CustomEvent("openSettings", { detail: { tab: "plugins" } }),
            );
          }}
        >
          <BusinessCenterOutlined sx={{ fontSize: 20, color: "text.secondary" }} />
          <Typography sx={navTextSx}>{t("customize")}</Typography>
        </Box>
        <Box sx={navRowSx} onClick={() => {}}>
          <FolderOutlined sx={{ fontSize: 20, color: "text.secondary" }} />
          <Typography sx={navTextSx}>{t("projects")}</Typography>
        </Box>
      </Stack>

      {/* Recents */}
      <Box sx={{ px: 1.5, pt: 0.5, pb: 0.75 }}>
        <Typography
          variant="caption"
          sx={{
            display: "block",
            px: 0.5,
            color: "text.secondary",
            fontSize: 12,
            fontWeight: 500,
          }}
        >
          {t("recents")}
        </Typography>
      </Box>

      {/* Session list */}
      <Box sx={{ flex: 1, overflow: "auto", px: 1, pb: 1, minHeight: 0 }}>
        <Stack spacing={0.5}>
          {sessionRows.length === 0 ? (
            <Box
              sx={{
                p: 3,
                textAlign: "center",
                color: "text.secondary",
              }}
            >
              <Typography variant="body2">{t("noSessions")}</Typography>
            </Box>
          ) : (
            sessionRows.map(({ session, isPlaceholder }) => (
              <Box
                key={session.id}
                onClick={() => handleSelectSession(session.id)}
                onMouseEnter={() => handleSessionMouseEnter(session.id)}
                onMouseLeave={handleSessionMouseLeave}
                onMouseDown={() => handleSessionMouseDown(session.id)}
                sx={{
                  px: 1.25,
                  py: 1,
                  borderRadius: 1.5,
                  cursor: "pointer",
                  position: "relative",
                  overflow: "hidden",
                  bgcolor:
                    currentSession?.id === session.id
                      ? alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.14 : 0.1)
                      : "transparent",
                  border: "1px solid",
                  borderColor:
                    currentSession?.id === session.id
                      ? alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.25 : 0.18)
                      : "transparent",
                  transition: "background-color 120ms ease, border-color 120ms ease",
                  "&:hover": {
                    bgcolor:
                      currentSession?.id === session.id
                        ? alpha(theme.palette.primary.main, theme.palette.mode === "dark" ? 0.2 : 0.14)
                        : "action.hover",
                  },
                  // Radial bloom on click — expands from press point outward.
                  // Uses a pseudo-element so it never affects layout.
                  // prefers-reduced-motion: animation is suppressed entirely.
                  "@media (prefers-reduced-motion: no-preference)": {
                    "&:active::after": {
                      content: '""',
                      position: "absolute",
                      inset: 0,
                      borderRadius: "inherit",
                      background: alpha(theme.palette.primary.main, 0.18),
                      "@keyframes omigaBloom": {
                        from: { opacity: 1, transform: "scale(0.6)" },
                        to:   { opacity: 0, transform: "scale(1.4)" },
                      },
                      animation: "omigaBloom 320ms cubic-bezier(0.2, 0, 0.6, 1) forwards",
                      pointerEvents: "none",
                    },
                  },
                }}
              >
                <Stack direction="row" alignItems="center" spacing={0.5}>
                  {/* Running indicator — pulsing dot + left glow strip */}
                  {isSessionRunning(session.id) && (
                    <Box
                      aria-label="running"
                      sx={{
                        flexShrink: 0,
                        width: 7,
                        height: 7,
                        borderRadius: "50%",
                        bgcolor: "primary.main",
                        boxShadow: (t) =>
                          `0 0 6px 1px ${alpha(t.palette.primary.main, 0.55)}`,
                        "@media (prefers-reduced-motion: no-preference)": {
                          "@keyframes omigaRunPulse": {
                            "0%, 100%": { opacity: 1, transform: "scale(1)" },
                            "50%":       { opacity: 0.45, transform: "scale(0.65)" },
                          },
                          animation: "omigaRunPulse 1.2s ease-in-out infinite",
                        },
                      }}
                    />
                  )}
                  <Typography
                    variant="body2"
                    fontWeight={500}
                    noWrap
                    sx={{
                      flex: 1,
                      minWidth: 0,
                      ...(isPlaceholder
                        ? {
                            color: "text.secondary",
                            fontStyle: "italic",
                            fontWeight: 400,
                          }
                        : { color: "text.primary" }),
                    }}
                  >
                    {isPlaceholder ? UNUSED_SESSION_LABEL : session.name}
                  </Typography>
                  <IconButton
                    size="small"
                    aria-label={t("sessionActions")}
                    onClick={(e) => {
                      e.stopPropagation();
                      handleMenuOpen(e, session.id);
                    }}
                    sx={{
                      p: 0.25,
                      flexShrink: 0,
                      color: "text.secondary",
                      "&:hover": {
                        bgcolor: "action.hover",
                      },
                    }}
                  >
                    <MoreVert fontSize="small" />
                  </IconButton>
                </Stack>
              </Box>
            ))
          )}
        </Stack>
      </Box>

      <Dialog
        open={searchDialogOpen}
        onClose={handleCloseSearchDialog}
        fullWidth
        maxWidth="md"
        keepMounted
        BackdropProps={{
          sx: {
            backgroundColor: alpha(
              theme.palette.background.default,
              theme.palette.mode === "dark" ? 0.38 : 0.18,
            ),
            backdropFilter: "blur(2px)",
          },
        }}
        PaperProps={{
          sx: {
            width: "min(760px, calc(100vw - 32px))",
            maxHeight: "min(620px, calc(100vh - 64px))",
            borderRadius: 3,
            overflow: "hidden",
            border: "1px solid",
            borderColor:
              theme.palette.mode === "dark"
                ? alpha(theme.palette.common.white, 0.12)
                : alpha(theme.palette.common.black, 0.12),
            bgcolor:
              theme.palette.mode === "dark"
                ? alpha(theme.palette.background.paper, 0.96)
                : alpha(theme.palette.background.paper, 0.98),
            backgroundImage: "none",
            boxShadow:
              theme.palette.mode === "dark"
                ? "0 24px 80px rgba(0,0,0,0.65)"
                : "0 24px 80px rgba(15,23,42,0.22)",
          },
        }}
      >
        <DialogContent sx={{ p: 0 }}>
          <Box sx={{ px: 2.25, pt: 2, pb: 1.25 }}>
            <Typography
              variant="body2"
              sx={{ color: "text.secondary", fontWeight: 500, mb: 1 }}
            >
              {t("searchDialogTitle")}
            </Typography>
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                gap: 1,
                px: 1.75,
                py: 1.15,
                borderRadius: 2.5,
                bgcolor:
                  theme.palette.mode === "dark"
                    ? alpha(theme.palette.common.white, 0.08)
                    : alpha(theme.palette.common.black, 0.04),
                border: "1px solid",
                borderColor:
                  theme.palette.mode === "dark"
                    ? alpha(theme.palette.common.white, 0.14)
                    : alpha(theme.palette.common.black, 0.1),
                transition:
                  "background-color 140ms ease, border-color 140ms ease, box-shadow 140ms ease",
                "&:focus-within": {
                  bgcolor:
                    theme.palette.mode === "dark"
                      ? alpha(theme.palette.common.white, 0.11)
                      : theme.palette.common.white,
                  borderColor: alpha(theme.palette.primary.main, 0.45),
                  boxShadow: `0 0 0 3px ${alpha(theme.palette.primary.main, 0.14)}`,
                },
              }}
            >
              <Search
                sx={{
                  color:
                    theme.palette.mode === "dark"
                      ? alpha(theme.palette.common.white, 0.58)
                      : "text.secondary",
                  fontSize: 22,
                }}
              />
              <InputBase
                inputRef={searchInputRef}
                placeholder={t("searchPlaceholder")}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={handleSearchKeyDown}
                inputProps={{ "aria-label": t("searchDialogTitle") }}
                sx={{
                  flex: 1,
                  fontSize: 16,
                  fontWeight: 500,
                  color: "text.primary",
                  "& input::placeholder": {
                    color: "text.secondary",
                    opacity: theme.palette.mode === "dark" ? 0.68 : 0.58,
                  },
                }}
              />
            </Box>
          </Box>
          <Divider />
          <Box sx={{ px: 1.5, py: 1.25, maxHeight: 460, overflow: "auto" }}>
            <Typography
              variant="caption"
              sx={{
                display: "block",
                color: "text.secondary",
                fontWeight: 500,
                px: 1,
                mb: 0.75,
              }}
            >
              {searchQuery.trim()
                ? contentSearchLoading
                  ? t("searchingSessions")
                  : `${t("searchDialogTitle")} (${searchResults.length})`
                : t("recentConversations")}
            </Typography>
            <Stack spacing={0.5}>
              {searchResults.length === 0 ? (
                <Box sx={{ py: 4, textAlign: "center", color: "text.secondary" }}>
                  <Typography variant="body2">{t("noSearchResults")}</Typography>
                </Box>
              ) : (
                searchResults.map(({ session, isPlaceholder, matchSnippet }, index) => {
                  const projectLabel = sessionProjectLabel(session);
                  const selected = currentSession?.id === session.id;
                  return (
                    <Box
                      key={session.id}
                      role="button"
                      tabIndex={0}
                      onClick={() => handleSearchResultClick(session.id)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          handleSearchResultClick(session.id);
                        }
                      }}
                      onMouseEnter={() => handleSessionMouseEnter(session.id)}
                      onMouseLeave={handleSessionMouseLeave}
                      onMouseDown={() => handleSessionMouseDown(session.id)}
                      sx={{
                        display: "flex",
                        alignItems: "center",
                        gap: 1.25,
                        px: 1.25,
                        py: 1,
                        minHeight: matchSnippet ? 58 : 44,
                        borderRadius: 2,
                        cursor: "pointer",
                        bgcolor:
                          selected
                            ? alpha(
                                theme.palette.primary.main,
                                theme.palette.mode === "dark" ? 0.18 : 0.1,
                              )
                            : index === 0
                              ? "action.hover"
                              : "transparent",
                        outline: "none",
                        "&:hover, &:focus-visible": {
                          bgcolor: "action.hover",
                        },
                      }}
                    >
                      <Box
                        sx={{
                          width: 18,
                          height: 14,
                          flexShrink: 0,
                          borderRadius: 0.75,
                          border: "1.5px solid",
                          borderColor: "text.secondary",
                          opacity: 0.82,
                          position: "relative",
                          "&::after": {
                            content: '""',
                            position: "absolute",
                            left: "50%",
                            bottom: -4,
                            width: 8,
                            height: 2,
                            borderRadius: 1,
                            transform: "translateX(-50%)",
                            bgcolor: "text.secondary",
                            opacity: 0.75,
                          },
                        }}
                      />
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Typography
                          variant="body2"
                          noWrap
                          sx={{
                            color: isPlaceholder ? "text.secondary" : "text.primary",
                            fontStyle: isPlaceholder ? "italic" : "normal",
                            fontWeight: selected ? 600 : 500,
                          }}
                        >
                          {isPlaceholder ? UNUSED_SESSION_LABEL : session.name}
                        </Typography>
                        {matchSnippet && (
                          <Typography
                            variant="caption"
                            noWrap
                            sx={{
                              display: "block",
                              color: "text.secondary",
                              mt: 0.25,
                            }}
                          >
                            {matchSnippet}
                          </Typography>
                        )}
                      </Box>
                      {projectLabel && (
                        <Typography
                          variant="body2"
                          noWrap
                          sx={{
                            maxWidth: 120,
                            color: "text.secondary",
                            flexShrink: 0,
                          }}
                        >
                          {projectLabel}
                        </Typography>
                      )}
                    </Box>
                  );
                })
              )}
            </Stack>
          </Box>
        </DialogContent>
      </Dialog>

      {/* Bottom profile (reference layout) */}
      <Box
        onClick={handleUserMenuOpen}
        sx={{
          borderTop: 1,
          borderColor: "divider",
          p: 1.5,
          display: "flex",
          alignItems: "center",
          gap: 1,
          flexShrink: 0,
          cursor: "pointer",
          "&:hover": {
            bgcolor: "action.hover",
          },
        }}
      >
        <OmigaLogo size={18} animated={false} />
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" fontWeight={600} color="text.primary" noWrap>
            dengxsh
          </Typography>
          <Typography variant="caption" color="text.secondary" display="block" noWrap>
            {t("proPlan")}
          </Typography>
        </Box>
        <UnfoldMore sx={{ fontSize: 18, color: "text.secondary", flexShrink: 0 }} />
      </Box>

      {/* User Menu */}
      <Menu
        anchorEl={userMenuAnchorEl}
        open={Boolean(userMenuAnchorEl)}
        onClose={handleUserMenuClose}
        anchorOrigin={{ vertical: "top", horizontal: "center" }}
        transformOrigin={{ vertical: "bottom", horizontal: "center" }}
        disableAutoFocusItem
        PaperProps={{
          sx: { width: 240, borderRadius: 2 },
          "data-omiga-floating-menu": "user",
        }}
      >
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => handleOpenSettings()}
        >
          <ListItemIcon>
            <SettingsIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("settings")}</ListItemText>
        </MenuItem>
        <MenuItem
          onMouseEnter={(e) => openLanguageSubmenu(e.currentTarget)}
          onMouseLeave={scheduleCloseLanguageSubmenu}
        >
          <ListItemIcon>
            <LanguageIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("language")}</ListItemText>
          <ChevronRight
            fontSize="small"
            sx={{ ml: "auto", color: "text.secondary" }}
          />
        </MenuItem>
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => handleOpenSettings("theme")}
        >
          <ListItemIcon>
            <PaletteIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("theme")}</ListItemText>
        </MenuItem>
        <Divider />
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => handleOpenSettings("plugins")}
        >
          <ListItemIcon>
            <ExtensionIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("plugins")}</ListItemText>
        </MenuItem>
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => handleOpenSettings("connectors")}
        >
          <ListItemIcon>
            <LinkIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("connectors")}</ListItemText>
        </MenuItem>
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => handleOpenSettings("mcp")}
        >
          <ListItemIcon>
            <StorageIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("mcp")}</ListItemText>
        </MenuItem>
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => handleOpenSettings("skills")}
        >
          <ListItemIcon>
            <AutoAwesomeIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("skills")}</ListItemText>
        </MenuItem>
        <Divider />
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => void handleOpenHelp()}
        >
          <ListItemIcon>
            <HelpOutlineIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("getHelp")}</ListItemText>
        </MenuItem>
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => void handleLearnMore()}
        >
          <ListItemIcon>
            <MenuBookIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("learnMore")}</ListItemText>
        </MenuItem>
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => void handleOpenGithubUpdates()}
        >
          <ListItemIcon>
            <GitHubIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("githubUpdates")}</ListItemText>
        </MenuItem>
        <Divider />
        <MenuItem
          onMouseEnter={closeLanguageSubmenuNow}
          onClick={() => void handleLogOut()}
        >
          <ListItemIcon>
            <LogoutIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("logOut")}</ListItemText>
        </MenuItem>
      </Menu>

      <Menu
        anchorEl={languageSubmenuAnchor}
        open={Boolean(userMenuAnchorEl && languageSubmenuAnchor)}
        onClose={closeLanguageSubmenuNow}
        anchorOrigin={{ vertical: "top", horizontal: "right" }}
        transformOrigin={{ vertical: "top", horizontal: "left" }}
        sx={{ pointerEvents: "none" }}
        PaperProps={{
          sx: {
            pointerEvents: "auto",
            borderRadius: 2,
            minWidth: 160,
          },
          "data-omiga-floating-menu": "language",
        }}
        MenuListProps={{
          onMouseEnter: clearLanguageSubmenuLeaveTimer,
          onMouseLeave: () => setLanguageSubmenuAnchor(null),
        }}
        disableAutoFocus
      >
        <MenuItem
          selected={locale === "en"}
          onClick={() => {
            setLocale("en");
            handleUserMenuClose();
          }}
        >
          {t("english")}
        </MenuItem>
        <MenuItem
          selected={locale === "zh-CN"}
          onClick={() => {
            setLocale("zh-CN");
            handleUserMenuClose();
          }}
        >
          {t("chinese")}
        </MenuItem>
      </Menu>

      {/* Context Menu */}
      <Menu
        anchorEl={menuAnchorEl}
        open={Boolean(menuAnchorEl)}
        onClose={handleMenuClose}
        anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
        transformOrigin={{ vertical: "top", horizontal: "right" }}
        PaperProps={{
          sx: { minWidth: 140 },
        }}
      >
        <MenuItem onClick={handleRenameClick}>
          <Edit fontSize="small" sx={{ mr: 1 }} />
          {t("rename")}
        </MenuItem>
        <MenuItem
          onClick={handleDelete}
          disabled={isDeleting}
          sx={{ color: "error.main" }}
        >
          <Delete fontSize="small" sx={{ mr: 1 }} />
          {isDeleting ? t("deleting") : t("delete")}
        </MenuItem>
      </Menu>

      {/* Rename Dialog */}
      <Dialog
        open={renameDialogOpen}
        onClose={() => setRenameDialogOpen(false)}
        maxWidth="xs"
        fullWidth
        PaperProps={{ sx: { borderRadius: 3 } }}
      >
        <DialogTitle>{t("renameSession")}</DialogTitle>
        <DialogContent>
          <TextField
            autoFocus
            fullWidth
            label={t("sessionName")}
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              const ne = e.nativeEvent;
              if (ne.isComposing || ne.keyCode === 229) return;
              if (e.key === "Enter" && newName.trim()) {
                handleRenameConfirm();
              }
            }}
            sx={{ mt: 1 }}
          />
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2 }}>
          <Button onClick={() => setRenameDialogOpen(false)}>
            {t("cancel")}
          </Button>
          <Button
            onClick={handleRenameConfirm}
            variant="contained"
            disabled={!newName.trim()}
          >
            {t("rename")}
          </Button>
        </DialogActions>
      </Dialog>
    </Box>
  );
}
