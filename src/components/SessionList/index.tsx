import { useState, useEffect, useRef } from "react";
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
  Storage as StorageIcon,
  AutoAwesome as AutoAwesomeIcon,
  HelpOutline as HelpOutlineIcon,
  MenuBook as MenuBookIcon,
  Logout as LogoutIcon,
  ChevronRight,
} from "@mui/icons-material";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  useSessionStore,
  UNUSED_SESSION_LABEL,
  shouldShowNewSessionPlaceholder,
} from "../../state/sessionStore";
import { useLocaleStore } from "../../state";
import {
  tSessionList,
  type SessionListStringKey,
} from "../../i18n/sessionListStrings";

interface SessionListProps {
  onSelectSession?: () => void;
}

/** External links — adjust when Omiga has public docs */
const HELP_CENTER_URL = "https://support.anthropic.com/";
const LEARN_MORE_URL = "https://www.anthropic.com/claude";

export function SessionList({ onSelectSession }: SessionListProps) {
  const theme = useTheme();
  const locale = useLocaleStore((s) => s.locale);
  const setLocale = useLocaleStore((s) => s.setLocale);
  const t = (key: SessionListStringKey) => tSessionList(locale, key);
  const {
    sessions,
    currentSession,
    setCurrentSession,
    loadSessions,
    deleteSession,
    renameSession,
    createSessionQuick,
    isLoading,
    storeMessages,
  } = useSessionStore();

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

  const clearLanguageSubmenuLeaveTimer = () => {
    if (languageSubmenuLeaveTimerRef.current) {
      clearTimeout(languageSubmenuLeaveTimerRef.current);
      languageSubmenuLeaveTimerRef.current = null;
    }
  };

  const handleUserMenuClose = () => {
    clearLanguageSubmenuLeaveTimer();
    setLanguageSubmenuAnchor(null);
    setUserMenuAnchorEl(null);
  };

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
    }, 200);
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

  const [searchQuery, setSearchQuery] = useState("");
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  const filteredSessions = sessions.filter((s) => {
    const q = searchQuery.toLowerCase().trim();
    if (!q) return true;
    const listLabel = shouldShowNewSessionPlaceholder(s, {
      isCurrentSession: currentSession?.id === s.id,
      storeMessageCount:
        currentSession?.id === s.id ? storeMessages.length : undefined,
    })
      ? UNUSED_SESSION_LABEL
      : s.name;
    return (
      s.name.toLowerCase().includes(q) ||
      listLabel.toLowerCase().includes(q)
    );
  });

  const navTextSx = {
    fontSize: 14,
    fontWeight: 500,
    color: "#1C1C1E",
    lineHeight: 1.3,
  } as const;

  const navRowSx = {
    display: "flex",
    alignItems: "center",
    gap: 1.25,
    px: 1.5,
    py: 1,
    borderRadius: 1,
    cursor: "pointer",
    color: "#1C1C1E",
    "&:hover": {
      bgcolor: alpha(theme.palette.common.black, 0.04),
    },
  } as const;

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
        bgcolor: "#FAFAFA",
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

      {/* Top nav: icon + label (reference layout) */}
      <Stack spacing={0} sx={{ p: 1.5, pb: 1 }}>
        <Box
          sx={navRowSx}
          onClick={() => {
            handleCreateClick();
          }}
        >
          <Add sx={{ fontSize: 20, color: "#3C3C43" }} />
          <Typography sx={navTextSx}>{t("newSession")}</Typography>
        </Box>
        <Box
          sx={navRowSx}
          onClick={() => {
            searchInputRef.current?.focus();
          }}
        >
          <Search sx={{ fontSize: 20, color: "#3C3C43" }} />
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
          <BusinessCenterOutlined sx={{ fontSize: 20, color: "#3C3C43" }} />
          <Typography sx={navTextSx}>{t("customize")}</Typography>
        </Box>
        <Box sx={navRowSx} onClick={() => {}}>
          <FolderOutlined sx={{ fontSize: 20, color: "#3C3C43" }} />
          <Typography sx={navTextSx}>{t("projects")}</Typography>
        </Box>
      </Stack>

      {/* Recents + filter */}
      <Box sx={{ px: 1.5, pt: 0.5, pb: 1 }}>
        <Typography
          variant="caption"
          sx={{
            display: "block",
            mb: 1,
            px: 0.5,
            color: "#8E8E93",
            fontSize: 12,
            fontWeight: 500,
          }}
        >
          {t("recents")}
        </Typography>
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            gap: 0.75,
            px: 1.5,
            py: 0.75,
            borderRadius: 2,
            bgcolor: "#EBEBEB",
          }}
        >
          <Search fontSize="small" sx={{ color: "#AEAEB2", fontSize: 16 }} />
          <InputBase
            inputRef={searchInputRef}
            placeholder={t("searchPlaceholder")}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            sx={{
              flex: 1,
              fontSize: 13,
              "& input::placeholder": { color: "#AEAEB2", opacity: 1 },
            }}
          />
        </Box>
      </Box>

      {/* Session list */}
      <Box sx={{ flex: 1, overflow: "auto", px: 1, pb: 1, minHeight: 0 }}>
        <Stack spacing={0.5}>
          {filteredSessions.length === 0 ? (
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
            filteredSessions.map((session) => {
              const isPlaceholder = shouldShowNewSessionPlaceholder(session, {
                isCurrentSession: currentSession?.id === session.id,
                storeMessageCount:
                  currentSession?.id === session.id
                    ? storeMessages.length
                    : undefined,
              });
              return (
              <Box
                key={session.id}
                onClick={() => handleSelectSession(session.id)}
                sx={{
                  px: 1.25,
                  py: 1,
                  borderRadius: 1.5,
                  cursor: "pointer",
                  bgcolor:
                    currentSession?.id === session.id
                      ? alpha(theme.palette.common.black, 0.06)
                      : "transparent",
                  border: "1px solid transparent",
                  "&:hover": {
                    bgcolor:
                      currentSession?.id === session.id
                        ? alpha(theme.palette.common.black, 0.06)
                        : alpha(theme.palette.common.black, 0.04),
                  },
                }}
              >
                <Stack direction="row" alignItems="center" spacing={0.5}>
                  <Typography
                    variant="body2"
                    fontWeight={500}
                    noWrap
                    sx={{
                      flex: 1,
                      minWidth: 0,
                      ...(isPlaceholder
                        ? {
                            color: "#8E8E93",
                            fontStyle: "italic",
                            fontWeight: 400,
                          }
                        : { color: "#1C1C1E" }),
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
                      color: "#8E8E93",
                      "&:hover": {
                        bgcolor: alpha(theme.palette.common.black, 0.06),
                      },
                    }}
                  >
                    <MoreVert fontSize="small" />
                  </IconButton>
                </Stack>
              </Box>
            );
            })
          )}
        </Stack>
      </Box>

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
            bgcolor: alpha(theme.palette.common.black, 0.04),
          },
        }}
      >
        <Box
          sx={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            bgcolor: "#F48FB1",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: "#fff",
            fontSize: 18,
          }}
        >
          ✿
        </Box>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" fontWeight={600} color="#1C1C1E" noWrap>
            dengxsh
          </Typography>
          <Typography variant="caption" color="#8E8E93" display="block" noWrap>
            {t("proPlan")}
          </Typography>
        </Box>
        <UnfoldMore sx={{ fontSize: 18, color: "#8E8E93", flexShrink: 0 }} />
      </Box>

      {/* User Menu */}
      <Menu
        anchorEl={userMenuAnchorEl}
        open={Boolean(userMenuAnchorEl)}
        onClose={handleUserMenuClose}
        anchorOrigin={{ vertical: "top", horizontal: "center" }}
        transformOrigin={{ vertical: "bottom", horizontal: "center" }}
        PaperProps={{
          sx: { width: 240, borderRadius: 2 },
        }}
      >
        <MenuItem onClick={() => handleOpenSettings()}>
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
        <MenuItem onClick={() => handleOpenSettings("theme")}>
          <ListItemIcon>
            <PaletteIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("theme")}</ListItemText>
        </MenuItem>
        <Divider />
        <MenuItem onClick={() => handleOpenSettings("plugins")}>
          <ListItemIcon>
            <ExtensionIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("plugins")}</ListItemText>
        </MenuItem>
        <MenuItem onClick={() => handleOpenSettings("mcp")}>
          <ListItemIcon>
            <StorageIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("mcp")}</ListItemText>
        </MenuItem>
        <MenuItem onClick={() => handleOpenSettings("skills")}>
          <ListItemIcon>
            <AutoAwesomeIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("skills")}</ListItemText>
        </MenuItem>
        <Divider />
        <MenuItem onClick={() => void handleOpenHelp()}>
          <ListItemIcon>
            <HelpOutlineIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("getHelp")}</ListItemText>
        </MenuItem>
        <MenuItem onClick={() => void handleLearnMore()}>
          <ListItemIcon>
            <MenuBookIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("learnMore")}</ListItemText>
        </MenuItem>
        <Divider />
        <MenuItem onClick={() => void handleLogOut()}>
          <ListItemIcon>
            <LogoutIcon fontSize="small" />
          </ListItemIcon>
          <ListItemText>{t("logOut")}</ListItemText>
        </MenuItem>
      </Menu>

      <Menu
        anchorEl={languageSubmenuAnchor}
        open={Boolean(languageSubmenuAnchor)}
        onClose={() => setLanguageSubmenuAnchor(null)}
        anchorOrigin={{ vertical: "top", horizontal: "right" }}
        transformOrigin={{ vertical: "top", horizontal: "left" }}
        sx={{ pointerEvents: "none" }}
        PaperProps={{
          sx: {
            pointerEvents: "auto",
            borderRadius: 2,
            minWidth: 160,
          },
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
