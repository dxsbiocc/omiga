import { memo } from "react";
import type { Components } from "react-markdown";
import { Box, Chip, Collapse, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import {
  Article,
  Assignment as AssignmentIcon,
  Checklist as ChecklistIcon,
  Computer as ComputerIcon,
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
  WarningAmber,
} from "@mui/icons-material";
import type { getChatTokens } from "./chatTokens";
import { CollapsibleThoughtTrace } from "./AssistantTraceItem";
import {
  formatToolDuration,
  parseStructuredToolErrorHint,
  toolCallPanelTitle,
  toolDisplayOutputText,
  type ToolCallLike,
} from "./ToolFoldSummary";

type ChatTokens = ReturnType<typeof getChatTokens>;

function toolRowIcon(toolName: string) {
  const n = toolName.toLowerCase();
  if (n.startsWith("computer_")) return ComputerIcon;
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
  if (n === "query") return TravelExploreIcon;
  if (n === "search" || n === "websearch") return TravelExploreIcon;
  if (n === "fetch") return LinkIcon;
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

function isComputerUseTool(toolName: string): boolean {
  return toolName.toLowerCase().startsWith("computer_");
}

function parseToolInput(input: string | undefined): Record<string, unknown> | null {
  if (!input?.trim()) return null;
  try {
    const parsed = JSON.parse(input) as unknown;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
    /* not JSON */
  }
  return null;
}

function compactString(value: unknown, max = 80): string | undefined {
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  return trimmed.length > max ? `${trimmed.slice(0, max)}…` : trimmed;
}

function computerUseInputSummary(toolName: string, input: string | undefined): string {
  const parsed = parseToolInput(input);
  if (!parsed) return input?.trim() ? "[Computer Use input hidden]" : "";

  const summary: Record<string, unknown> = {};
  for (const key of [
    "observationId",
    "targetWindowId",
    "x",
    "y",
    "button",
    "elementId",
    "reason",
    "targetHint",
    "appName",
    "bundleId",
    "windowTitle",
  ]) {
    if (!(key in parsed)) continue;
    const value = parsed[key];
    summary[key] = typeof value === "string" ? compactString(value) ?? value : value;
  }

  if (typeof parsed.text === "string") {
    summary.text = `[hidden ${parsed.text.length} chars]`;
  }

  if (Object.keys(summary).length === 0) {
    summary.kind = toolName;
  }
  return JSON.stringify(summary, null, 2);
}

function computerUsePanelTitle(toolName: string, input: string | undefined): string {
  const parsed = parseToolInput(input);
  if (toolName === "computer_type" && typeof parsed?.text === "string") {
    return `computer_type · text hidden (${parsed.text.length} chars)`;
  }
  if (toolName === "computer_click") {
    const x = parsed?.x;
    const y = parsed?.y;
    if (typeof x === "number" && typeof y === "number") {
      return `computer_click · (${x}, ${y})`;
    }
  }
  if (toolName === "computer_click_element") {
    const elementId = compactString(parsed?.elementId, 48);
    if (elementId) return `computer_click_element · ${elementId}`;
  }
  return toolName;
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
  const structuredError = parseStructuredToolErrorHint(displayOutput);
  const isComputerTool = isComputerUseTool(toolCall.name);
  const displayInput = isComputerTool
    ? computerUseInputSummary(toolCall.name, toolCall.input)
    : toolCall.input;
  const hasInput = Boolean(displayInput && displayInput.trim());
  const hasOutput = Boolean(displayOutput);
  const isBash =
    toolCall.name === "bash" || toolCall.name.toLowerCase().includes("bash");
  const commandSectionLabel = isBash
    ? "Command"
    : isComputerTool
      ? "Computer Use request"
      : toolCall.name;
  const panelTitle = isComputerTool
    ? computerUsePanelTitle(toolCall.name, toolCall.input)
    : toolCallPanelTitle(toolCall.input, toolCall.name);
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
              transition: "transform 0.2s ease",
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
          {toolCall.status !== "error" && structuredError && (
            <Chip
              size="small"
              label={structuredError.recoverable === false ? "Blocked" : "Needs action"}
              color={structuredError.recoverable === false ? "error" : "warning"}
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
                        {displayInput}
                      </Typography>
                    </Box>
                  </Box>
                )}
                {hasOutput && (
                  <Box>
                    {structuredError && (
                      <Box
                        sx={{
                          mb: 1,
                          borderRadius: "8px",
                          border: `1px solid ${alpha(
                            structuredError.recoverable === false
                              ? "#d32f2f"
                              : "#ed6c02",
                            0.32,
                          )}`,
                          bgcolor: alpha(
                            structuredError.recoverable === false
                              ? "#d32f2f"
                              : "#ed6c02",
                            0.07,
                          ),
                          p: 1,
                        }}
                      >
                        <Stack direction="column" spacing={0.65}>
                          <Stack
                            direction="row"
                            alignItems="center"
                            spacing={0.75}
                            sx={{ minWidth: 0 }}
                          >
                            <WarningAmber
                              sx={{
                                fontSize: 15,
                                color:
                                  structuredError.recoverable === false
                                    ? "error.main"
                                    : "warning.main",
                                flexShrink: 0,
                              }}
                            />
                            <Typography
                              sx={{
                                fontSize: 11,
                                fontWeight: 700,
                                color: chat.textPrimary,
                                flex: 1,
                                minWidth: 0,
                              }}
                            >
                              {structuredError.message ?? structuredError.error}
                            </Typography>
                            <Chip
                              size="small"
                              label={structuredError.error}
                              variant="outlined"
                              sx={{ height: 20, fontSize: 10, flexShrink: 0 }}
                            />
                          </Stack>

                          {structuredError.route && (
                            <Typography
                              sx={{
                                fontSize: 10,
                                color: chat.textMuted,
                                overflowWrap: "anywhere",
                              }}
                            >
                              Route: {structuredError.route}
                            </Typography>
                          )}

                          {structuredError.nextAction && (
                            <Typography
                              sx={{
                                fontSize: 10,
                                color: chat.textPrimary,
                                overflowWrap: "anywhere",
                              }}
                            >
                              Next step: {structuredError.nextAction}
                            </Typography>
                          )}

                          {structuredError.diagnosticsHint && (
                            <Typography
                              sx={{
                                fontSize: 10,
                                color: chat.textMuted,
                                overflowWrap: "anywhere",
                              }}
                            >
                              Diagnostics: {structuredError.diagnosticsHint}
                            </Typography>
                          )}
                        </Stack>
                      </Box>
                    )}

                    <Typography
                      sx={{
                        fontSize: 10,
                        color: chat.labelMuted,
                        mb: 0.5,
                        fontWeight: 400,
                      }}
                    >
                      {structuredError ? "Raw output" : "Output"}
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
