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
  Route as RouteIcon,
  Extension as ExtensionIcon,
  Replay as ReplayIcon,
  Edit as EditIcon,
  ContentCopy as ContentCopyIcon,
  InfoOutlined as InfoOutlinedIcon,
} from "@mui/icons-material";
import type { ChatTokenSet } from "./chatTokens";
import {
  WORKFLOW_SLASH_COMMANDS,
  type WorkflowSlashCommandDefinition,
} from "../../utils/workflowCommands";

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
  selectedPluginIds?: string[];
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

interface UserMessageInlineCommand {
  command: WorkflowSlashCommandDefinition;
  body: string;
}

export function splitUserMessageInlineCommand(
  displayText: string,
): UserMessageInlineCommand | null {
  const match = displayText.match(
    /^\/(plan|schedule|team|autopilot|research|goal)(?:\s+([\s\S]*))?$/iu,
  );
  if (!match) return null;
  const command = WORKFLOW_SLASH_COMMANDS.find(
    (item) => item.id === match[1].toLowerCase(),
  );
  if (!command) return null;
  return {
    command,
    body: match[2] ?? "",
  };
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
  selectedPluginIds = [],
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
  const inlineCommand = isEditing
    ? null
    : splitUserMessageInlineCommand(displayText);
  const inlineText = inlineCommand ? inlineCommand.body : displayText;
  const isDark = theme.palette.mode === "dark";
  const commandTone = theme.palette.success.main;
  const fileTone = theme.palette.info.main;
  const pluginTone = theme.palette.warning.main;
  const agentTone = chat.accent;

  const semanticChipSx = (tone: string) => ({
    flexShrink: 0,
    maxWidth: "min(100%, 220px)",
    height: 22,
    display: "inline-flex",
    verticalAlign: "middle",
    fontSize: 11,
    fontWeight: 600,
    bgcolor: alpha(tone, isDark ? 0.18 : 0.11),
    borderColor: alpha(tone, isDark ? 0.62 : 0.45),
    color: tone,
    boxShadow: `0 1px 2px ${alpha(tone, isDark ? 0.2 : 0.16)}`,
    "& .MuiChip-icon": {
      color: tone,
      marginLeft: "6px",
    },
    "& .MuiChip-label": {
      px: 0.5,
      overflow: "hidden",
      textOverflow: "ellipsis",
      color: tone,
    },
  } as const);

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
            display: isEditing ? "flex" : "block",
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
          {isEditing ? (
            <>
              {composerAgentType || attachedPaths.length > 0 || selectedPluginIds.length > 0 ? (
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
                      className="user-msg-agent-chip"
                      size="small"
                      variant="outlined"
                      icon={<SmartToy sx={{ fontSize: 14, opacity: 0.9 }} />}
                      label={`/${composerAgentType}`}
                      sx={semanticChipSx(agentTone)}
                    />
                  ) : null}
                  {selectedPluginIds.map((pluginId) => (
                    <Tooltip key={pluginId} title={pluginId} placement="top">
                      <Chip
                        className="user-msg-plugin-chip"
                        size="small"
                        variant="outlined"
                        icon={<ExtensionIcon sx={{ fontSize: 14, opacity: 0.9 }} />}
                        label={`@${pluginId}`}
                        sx={semanticChipSx(pluginTone)}
                      />
                    </Tooltip>
                  ))}
                  {attachedPaths.map((p) => (
                    <Tooltip key={p} title={p} placement="top">
                      <Chip
                        className="user-msg-file-chip"
                        size="small"
                        variant="outlined"
                        icon={<InsertDriveFileIcon sx={{ fontSize: 14, opacity: 0.9 }} />}
                        label={`@${p}`}
                        sx={semanticChipSx(fileTone)}
                      />
                    </Tooltip>
                  ))}
                </Box>
              ) : null}
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
          ) : inlineText ||
            inlineCommand ||
            composerAgentType ||
            selectedPluginIds.length > 0 ||
            attachedPaths.length > 0 ? (
            <Typography
              component="div"
              className="user-msg-inline-flow"
              sx={{
                fontSize: 13,
                lineHeight: 1.45,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                overflowWrap: "anywhere",
                minWidth: 0,
                textAlign: "left",
              }}
            >
              {inlineCommand ? (
                <Tooltip
                  placement="top"
                  enterDelay={180}
                  title={inlineCommand.command.description}
                >
                  <Box
                    component="span"
                    className="user-msg-inline-chip"
                    sx={{
                      display: "inline-flex",
                      verticalAlign: "middle",
                      mr: 0.5,
                      mb: 0.2,
                    }}
                  >
                    <Chip
                      size="small"
                      className="user-msg-command-chip"
                      variant="outlined"
                      icon={<RouteIcon sx={{ fontSize: 14, opacity: 0.9 }} />}
                      label={inlineCommand.command.label}
                      sx={semanticChipSx(commandTone)}
                    />
                  </Box>
                </Tooltip>
              ) : null}
              {composerAgentType ? (
                <Box
                  component="span"
                  className="user-msg-inline-chip"
                  sx={{
                    display: "inline-flex",
                    verticalAlign: "middle",
                    mr: 0.5,
                    mb: 0.2,
                  }}
                >
                  <Chip
                    className="user-msg-agent-chip"
                    size="small"
                    variant="outlined"
                    icon={<SmartToy sx={{ fontSize: 14, opacity: 0.9 }} />}
                    label={`/${composerAgentType}`}
                    sx={semanticChipSx(agentTone)}
                  />
                </Box>
              ) : null}
              {selectedPluginIds.map((pluginId) => (
                <Tooltip key={pluginId} title={pluginId} placement="top">
                  <Box
                    component="span"
                    className="user-msg-inline-chip"
                    sx={{
                      display: "inline-flex",
                      verticalAlign: "middle",
                      mr: 0.5,
                      mb: 0.2,
                    }}
                  >
                    <Chip
                      className="user-msg-plugin-chip"
                      size="small"
                      variant="outlined"
                      icon={<ExtensionIcon sx={{ fontSize: 14, opacity: 0.9 }} />}
                      label={`@${pluginId}`}
                      sx={semanticChipSx(pluginTone)}
                    />
                  </Box>
                </Tooltip>
              ))}
              {attachedPaths.map((p) => (
                <Tooltip key={p} title={p} placement="top">
                  <Box
                    component="span"
                    className="user-msg-inline-chip"
                    sx={{
                      display: "inline-flex",
                      verticalAlign: "middle",
                      mr: 0.5,
                      mb: 0.2,
                    }}
                  >
                    <Chip
                      className="user-msg-file-chip"
                      size="small"
                      variant="outlined"
                      icon={<InsertDriveFileIcon sx={{ fontSize: 14, opacity: 0.9 }} />}
                      label={`@${p}`}
                      sx={semanticChipSx(fileTone)}
                    />
                  </Box>
                </Tooltip>
              ))}
              {inlineText ? (
                <Box
                  component="span"
                  className="user-msg-body-text"
                  sx={{ color: chat.userBubbleText }}
                >
                  {inlineText}
                </Box>
              ) : null}
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
