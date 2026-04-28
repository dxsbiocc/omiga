import { memo } from "react";
import type { Components } from "react-markdown";
import { Box, Chip, Collapse, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  Article,
  Assignment as AssignmentIcon,
  Checklist as ChecklistIcon,
  Construction,
  ExpandMore,
  FolderOpen,
  ForumOutlined,
  Link as LinkIcon,
  MenuBook as MenuBookIcon,
  Search as SearchIcon,
  Send as SendIcon,
  SmartToy,
  Terminal as TerminalIcon,
  TravelExplore as TravelExploreIcon,
} from "@mui/icons-material";
import type { getChatTokens } from "./chatTokens";
import { CollapsibleThoughtTrace } from "./AssistantTraceItem";
import {
  formatToolDuration,
  toolCallPanelTitle,
  toolDisplayOutputText,
  type ToolCallLike,
} from "./ToolFoldSummary";

type ChatTokens = ReturnType<typeof getChatTokens>;

function toolRowIcon(toolName: string) {
  const n = toolName.toLowerCase();
  if (n.includes("ask_user") || n.includes("askuserquestion"))
    return ForumOutlined;
  if (n === "Agent" || n === "Task") return SmartToy;
  if (
    n.includes("send_user_message") ||
    n.includes("sendusermessage") ||
    n.includes("brief")
  )
    return SendIcon;
  if (n.includes("todo_write") || n.includes("todowrite")) return ChecklistIcon;
  if (n.includes("notebook_edit") || n.includes("notebookedit"))
    return MenuBookIcon;
  if (n === "skill" || n === "skilltool") return MenuBookIcon;
  if (n === "recall") return SearchIcon;
  if (n.includes("web_search") || n.includes("websearch"))
    return TravelExploreIcon;
  if (n.includes("web_fetch") || n.includes("fetch")) return LinkIcon;
  if (n.includes("bash") || n.includes("shell")) return TerminalIcon;
  if (n.includes("glob") || n.includes("file")) return FolderOpen;
  if (n.includes("ripgrep") || n.includes("grep")) return SearchIcon;
  if (n.includes("toolsearch")) return SearchIcon;
  if (n.includes("exitplan") || n.includes("enterplan")) return MenuBookIcon;
  if (
    n === "taskcreate" ||
    n === "taskget" ||
    n === "tasklist" ||
    n === "taskupdate"
  )
    return AssignmentIcon;
  if (n.includes("taskstop") || n.includes("taskoutput")) return TerminalIcon;
  if (n.includes("read")) return Article;
  return Construction;
}

export interface ToolCallCardProps {
  foldId: string;
  messageId: string;
  content: string;
  timestamp?: number;
  prefaceBeforeTools?: string;
  toolCall: ToolCallLike;
  previousAssistantHasText: boolean;
  nestedOpen: boolean;
  showAskUserPanel: boolean;
  chat: ChatTokens;
  components: Components;
  onToggle: (foldId: string, messageId: string, toolCall: ToolCallLike) => void;
}

export const ToolCallCard = memo(function ToolCallCard({
  foldId,
  messageId,
  content,
  timestamp,
  prefaceBeforeTools,
  toolCall,
  previousAssistantHasText,
  nestedOpen,
  showAskUserPanel,
  chat,
  components,
  onToggle,
}: ToolCallCardProps) {
  const StepIcon = toolRowIcon(toolCall.name);
  const displayOutput = toolDisplayOutputText(
    { role: "tool", content, toolCall },
    toolCall,
  );
  const hasInput = Boolean(toolCall.input && toolCall.input.trim());
  const hasOutput = Boolean(displayOutput);
  const isBash =
    toolCall.name === "bash" || toolCall.name.toLowerCase().includes("bash");
  const commandSectionLabel = isBash ? "Command" : toolCall.name;
  const panelTitle = toolCallPanelTitle(toolCall.input, toolCall.name);
  const prefaceThought = prefaceBeforeTools?.trim() ?? "";
  const toolDurationLabel = formatToolDuration(timestamp, toolCall.completedAt);
  const thoughtRow =
    prefaceThought && !previousAssistantHasText ? (
      <CollapsibleThoughtTrace
        content={prefaceThought}
        chat={chat}
        components={components}
        sx={{ mb: 0.75 }}
      />
    ) : null;

  if (!hasInput && !hasOutput && !showAskUserPanel) {
    return thoughtRow ? <Box>{thoughtRow}</Box> : null;
  }

  return (
    <Box>
      {thoughtRow}

      <Box
        sx={{
          borderRadius: "10px",
          border: `1px solid ${chat.agentBubbleBorder}`,
          bgcolor: chat.toolCallCardBg,
          overflow: "hidden",
          transition: "border-color 200ms ease",
          "&:hover": {
            borderColor: alpha(chat.accent, 0.22),
          },
        }}
      >
        <Stack
          direction="row"
          alignItems="center"
          spacing={1}
          onClick={() => onToggle(foldId, messageId, toolCall)}
          sx={{
            cursor: "pointer",
            userSelect: "none",
            px: 1.25,
            py: 0.85,
            transition: "background-color 150ms ease",
            "&:hover": {
              bgcolor: alpha(chat.accent, 0.06),
              "& > svg:first-of-type": {
                color: chat.accent,
                opacity: 1,
              },
              "& > svg:nth-of-type(2)": {
                color: chat.accent,
              },
              "& > .MuiTypography-root": {
                color: chat.textPrimary,
              },
            },
          }}
        >
          <ExpandMore
            sx={{
              fontSize: 18,
              color: chat.toolIcon,
              opacity: 0.65,
              flexShrink: 0,
              transform: nestedOpen ? "rotate(0deg)" : "rotate(-90deg)",
              transition: "transform 0.2s ease, color 150ms ease, opacity 150ms ease",
            }}
          />
          <Chip
            size="small"
            label="行动"
            sx={{
              height: 18,
              fontSize: 9,
              color: chat.textMuted,
              flexShrink: 0,
            }}
          />
          <StepIcon
            sx={{
              fontSize: 16,
              color: chat.toolIcon,
              flexShrink: 0,
              transition: "color 150ms ease",
            }}
          />
          <Typography
            sx={{
              fontSize: 12,
              fontWeight: 600,
              color: chat.textPrimary,
              flex: 1,
              lineHeight: 1.35,
              wordBreak: "break-word",
              transition: "color 150ms ease",
            }}
          >
            {panelTitle}
          </Typography>
          {toolCall.status === "running" && (
            <Chip
              size="small"
              label={showAskUserPanel ? "等待你的回答" : "Running"}
              sx={{ height: 22, fontSize: 11, flexShrink: 0 }}
            />
          )}
          {toolCall.status === "error" && (
            <Chip
              size="small"
              label="Error"
              color="error"
              variant="outlined"
              sx={{ height: 22, fontSize: 11, flexShrink: 0 }}
            />
          )}
          {toolDurationLabel && (
            <Typography
              sx={{
                fontSize: 10,
                color: chat.labelMuted,
                flexShrink: 0,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {toolDurationLabel}
            </Typography>
          )}
        </Stack>

        <Collapse in={nestedOpen}>
          <Box
            sx={{
              px: 1.25,
              pb: 1.25,
              pt: 0,
              borderTop: `1px solid ${alpha(chat.agentBubbleBorder, 0.85)}`,
            }}
          >
            {panelTitle !== toolCall.name && (
              <Typography
                sx={{
                  fontSize: 10,
                  color: chat.labelMuted,
                  mb: 0.75,
                  fontWeight: 500,
                }}
              >
                {toolCall.name}
              </Typography>
            )}

            {(hasInput || hasOutput) && (
              <Stack direction="column" spacing={1} sx={{ width: "100%" }}>
                {hasInput && (
                  <Box>
                    <Typography
                      sx={{
                        fontSize: 10,
                        color: chat.labelMuted,
                        mb: 0.5,
                        fontWeight: 400,
                      }}
                    >
                      {isBash ? commandSectionLabel : "Input"}
                    </Typography>
                    <Box
                      sx={{
                        borderRadius: "6px",
                        border: `1px solid ${chat.agentBubbleBorder}`,
                        bgcolor: "transparent",
                        p: 1,
                        maxHeight: 200,
                        maxWidth: "100%",
                        overflowY: "auto",
                        overflowX: "hidden",
                      }}
                    >
                      <Typography
                        component="pre"
                        sx={{
                          m: 0,
                          fontFamily: "Menlo, Monaco, Consolas, monospace",
                          fontSize: 11,
                          lineHeight: 1.45,
                          color: chat.textPrimary,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-word",
                        }}
                      >
                        {toolCall.input}
                      </Typography>
                    </Box>
                  </Box>
                )}
                {hasOutput && (
                  <Box>
                    <Typography
                      sx={{
                        fontSize: 10,
                        color: chat.labelMuted,
                        mb: 0.5,
                        fontWeight: 400,
                      }}
                    >
                      Output
                    </Typography>
                    <Box
                      sx={{
                        borderRadius: "6px",
                        border: `1px solid ${chat.agentBubbleBorder}`,
                        bgcolor: chat.outputBg,
                        p: 1,
                        maxHeight: 320,
                        maxWidth: "100%",
                        overflowY: "auto",
                        overflowX: "hidden",
                      }}
                    >
                      <Typography
                        component="pre"
                        sx={{
                          m: 0,
                          fontFamily: "Inter, system-ui, sans-serif",
                          fontSize: 10,
                          lineHeight: 1.35,
                          color: chat.textMuted,
                          whiteSpace: "pre-wrap",
                          wordBreak: "break-word",
                        }}
                      >
                        {displayOutput}
                      </Typography>
                    </Box>
                  </Box>
                )}
              </Stack>
            )}

          </Box>
        </Collapse>
      </Box>
    </Box>
  );
});
