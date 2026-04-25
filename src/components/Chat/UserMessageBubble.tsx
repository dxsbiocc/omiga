import { memo } from "react";
import {
  Box,
  Button,
  Chip,
  IconButton,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  SmartToy,
  InsertDriveFile as InsertDriveFileIcon,
  Replay as ReplayIcon,
  Edit as EditIcon,
  ContentCopy as ContentCopyIcon,
  InfoOutlined as InfoOutlinedIcon,
} from "@mui/icons-material";
import type { ChatTokenSet } from "./chatTokens";

export function formatUserMessageTimestamp(ts: number | undefined): string {
  try {
    return new Date(ts ?? Date.now()).toLocaleString(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

interface UserMessageBubbleProps {
  content: string;
  displayText: string;
  timestamp?: number;
  composerAgentType?: string;
  attachedPaths: string[];
  isEditing: boolean;
  editDraft: string;
  chat: ChatTokenSet;
  bubbleRadiusPx: number;
  maxWidth: string;
  onRetry: () => void;
  onEdit: () => void;
  onCopy: () => void;
  onEditDraftChange: (draft: string) => void;
  onCancelEdit: () => void;
  onSaveEdit: () => void;
}

/**
 * Memoized user message bubble. It keeps historical user rows from rebuilding
 * their chip/edit/action chrome when Chat's streaming/activity state changes.
 */
export const UserMessageBubble = memo(function UserMessageBubble({
  displayText,
  timestamp,
  composerAgentType,
  attachedPaths,
  isEditing,
  editDraft,
  chat,
  bubbleRadiusPx,
  maxWidth,
  onRetry,
  onEdit,
  onCopy,
  onEditDraftChange,
  onCancelEdit,
  onSaveEdit,
}: UserMessageBubbleProps) {
  const theme = useTheme();

  const chipSx = {
    flexShrink: 0,
    maxWidth: "min(100%, 220px)",
    height: 22,
    fontSize: 11,
    fontWeight: 600,
    bgcolor: chat.userChipBg,
    borderColor: chat.userChipBorder,
    color: chat.userBubbleText,
    boxShadow: `0 1px 2px ${alpha(chat.userBubbleText, 0.12)}`,
    "& .MuiChip-icon": {
      color: chat.accent,
      marginLeft: "6px",
    },
    "& .MuiChip-label": {
      px: 0.5,
      overflow: "hidden",
      textOverflow: "ellipsis",
    },
  } as const;

  const iconButtonSx = {
    p: 0.35,
    color: chat.toolIcon,
    "&:hover": {
      color: chat.accent,
      bgcolor: alpha(chat.accent, 0.1),
    },
  } as const;

  return (
    <Box
      className="user-msg-wrap"
      sx={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        alignItems: isEditing ? "stretch" : "flex-end",
        minWidth: 0,
        width: "100%",
        maxWidth: "100%",
        alignSelf: "stretch",
        pb: 1,
        "&:hover .user-msg-hover-actions": {
          opacity: 1,
          pointerEvents: "auto",
        },
      }}
    >
      <Box
        sx={{
          position: "relative",
          display: "flex",
          flexDirection: "column",
          alignItems: isEditing ? "stretch" : "flex-end",
          width: "100%",
          minWidth: 0,
        }}
      >
        <Box
          sx={{
            minWidth: 0,
            width: isEditing ? "100%" : "fit-content",
            maxWidth: isEditing ? "100%" : maxWidth,
            px: isEditing ? 2 : 1.75,
            py: isEditing ? 2 : 1.25,
            borderRadius: `${bubbleRadiusPx}px`,
            border: `1px solid ${isEditing ? chat.agentBubbleBorder : chat.userBubbleBorder}`,
            background: isEditing ? theme.palette.background.paper : chat.userGrad,
            color: isEditing ? chat.textPrimary : chat.userBubbleText,
            fontFamily: chat.font,
            overflow: "hidden",
            display: "flex",
            flexDirection: isEditing ? "column" : "row",
            flexWrap: isEditing ? "nowrap" : "wrap",
            alignItems: isEditing ? "stretch" : "center",
            alignContent: isEditing ? "stretch" : "center",
            gap: 0.25,
            boxShadow: isEditing
              ? theme.palette.mode === "dark"
                ? `0 1px 4px ${alpha(theme.palette.common.black, 0.45)}`
                : `0 1px 3px ${alpha(theme.palette.common.black, 0.08)}`
              : undefined,
          }}
        >
          {composerAgentType || attachedPaths.length > 0 ? (
            <Box
              sx={{
                display: "flex",
                flexDirection: "row",
                flexWrap: "wrap",
                alignItems: "center",
                alignContent: "center",
                gap: 0.25,
                flexShrink: 0,
              }}
            >
              {composerAgentType ? (
                <Chip
                  size="small"
                  variant="outlined"
                  icon={<SmartToy sx={{ fontSize: 14, opacity: 0.9 }} />}
                  label={`/${composerAgentType}`}
                  sx={chipSx}
                />
              ) : null}
              {attachedPaths.map((p) => (
                <Tooltip key={p} title={p} placement="top">
                  <Chip
                    size="small"
                    variant="outlined"
                    icon={<InsertDriveFileIcon sx={{ fontSize: 14, opacity: 0.9 }} />}
                    label={`@${p}`}
                    sx={chipSx}
                  />
                </Tooltip>
              ))}
            </Box>
          ) : null}

          {isEditing ? (
            <>
              <TextField
                autoFocus
                multiline
                fullWidth
                minRows={4}
                maxRows={24}
                value={editDraft}
                onChange={(e) => onEditDraftChange(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.preventDefault();
                    onCancelEdit();
                  }
                  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                    e.preventDefault();
                    onSaveEdit();
                  }
                }}
                variant="outlined"
                placeholder="编辑消息内容…"
                sx={{
                  flex: "1 1 auto",
                  minWidth: 0,
                  width: "100%",
                  mt: 0.25,
                  "& .MuiOutlinedInput-root": {
                    fontSize: 13,
                    lineHeight: 1.45,
                    bgcolor: chat.codeBg,
                    color: chat.textPrimary,
                    alignItems: "flex-start",
                    borderRadius: `${bubbleRadiusPx}px`,
                  },
                  "& .MuiOutlinedInput-notchedOutline": {
                    borderColor: alpha(chat.agentBubbleBorder, 0.9),
                  },
                  "& .MuiInputBase-input": {
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                    overflowWrap: "anywhere",
                    px: 1,
                  },
                }}
              />
              <Stack direction="row" alignItems="flex-start" spacing={1} sx={{ mt: 1.5 }}>
                <InfoOutlinedIcon
                  sx={{
                    fontSize: 18,
                    color: chat.textMuted,
                    flexShrink: 0,
                    mt: 0.15,
                  }}
                />
                <Typography
                  variant="caption"
                  sx={{ color: chat.textMuted, lineHeight: 1.45 }}
                >
                  保存后将截断后续消息并重新分析。可按 Esc 取消，或使用 Ctrl/⌘ + Enter
                  保存并重发。
                </Typography>
              </Stack>
              <Stack direction="row" justifyContent="flex-end" spacing={1} sx={{ mt: 1, flexShrink: 0 }}>
                <Button size="small" variant="outlined" color="inherit" onClick={onCancelEdit}>
                  取消
                </Button>
                <Button size="small" variant="contained" onClick={onSaveEdit}>
                  保存
                </Button>
              </Stack>
            </>
          ) : displayText ? (
            <Typography
              component="span"
              sx={{
                fontSize: 13,
                lineHeight: 1.45,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                overflowWrap: "anywhere",
                flex: "1 1 0",
                minWidth: 0,
                textAlign: "left",
              }}
            >
              {displayText}
            </Typography>
          ) : null}
        </Box>

        <Stack
          className="user-msg-hover-actions"
          direction="row"
          alignItems="center"
          justifyContent="flex-end"
          flexWrap="nowrap"
          sx={{
            position: "absolute",
            left: 0,
            right: 0,
            top: "100%",
            mt: 0.5,
            width: "100%",
            maxWidth: "100%",
            boxSizing: "border-box",
            px: 0.25,
            py: 0,
            gap: 0.5,
            opacity: isEditing ? 1 : 0,
            pointerEvents: isEditing ? "auto" : "none",
            transition: "opacity 0.15s ease",
            zIndex: 2,
            minWidth: 0,
            overflowX: "auto",
            overflowY: "hidden",
            scrollbarWidth: "thin",
          }}
        >
          <Typography
            component="span"
            sx={{
              fontSize: 11,
              lineHeight: 1.2,
              color: chat.textMuted,
              whiteSpace: "nowrap",
              flexShrink: 0,
              userSelect: "none",
            }}
          >
            {formatUserMessageTimestamp(timestamp)}
          </Typography>
          {!isEditing ? (
            <Tooltip title="重试">
              <IconButton
                size="small"
                aria-label="重试"
                onClick={(e) => {
                  e.stopPropagation();
                  onRetry();
                }}
                sx={iconButtonSx}
              >
                <ReplayIcon sx={{ fontSize: 17 }} />
              </IconButton>
            </Tooltip>
          ) : null}
          {!isEditing ? (
            <Tooltip title="编辑">
              <IconButton
                size="small"
                aria-label="编辑"
                onClick={(e) => {
                  e.stopPropagation();
                  onEdit();
                }}
                sx={iconButtonSx}
              >
                <EditIcon sx={{ fontSize: 17 }} />
              </IconButton>
            </Tooltip>
          ) : null}
          <Tooltip title="复制">
            <IconButton
              size="small"
              aria-label="复制"
              onClick={(e) => {
                e.stopPropagation();
                onCopy();
              }}
              sx={iconButtonSx}
            >
              <ContentCopyIcon sx={{ fontSize: 17 }} />
            </IconButton>
          </Tooltip>
        </Stack>
      </Box>
    </Box>
  );
});
