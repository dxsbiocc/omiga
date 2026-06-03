import { memo } from "react";
import { Alert, Box, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { Components } from "react-markdown";
import type { ChatTokenSet } from "./chatTokens";
import { ChatMarkdownContent } from "./ChatMarkdownContent";

export interface AssistantTokenUsage {
  input: number;
  output: number;
  total?: number;
  provider?: string;
}

export function formatAssistantTokenUsage(tokenUsage: AssistantTokenUsage): string {
  const parts = [
    `输入 ${tokenUsage.input.toLocaleString()}`,
    `输出 ${tokenUsage.output.toLocaleString()}`,
  ];
  if (
    tokenUsage.total != null &&
    tokenUsage.total !== tokenUsage.input + tokenUsage.output
  ) {
    parts.push(`Σ ${tokenUsage.total.toLocaleString()}`);
  }
  if (tokenUsage.provider) parts.push(tokenUsage.provider);
  return parts.join(" · ");
}

type AssistantInterruptionKind = "cancelled" | "tool-limit";

export interface AssistantInterruptionNotice {
  kind: AssistantInterruptionKind;
  title: string;
  detail: string;
}

export interface ParsedAssistantInterruption {
  visibleContent: string;
  notice: AssistantInterruptionNotice | null;
}

const CANCELLED_MARKER_RE = /\s*\[(?:Cancelled|Canceled)(?:\s+by\s+user)?\]\s*$/i;
const TOOL_ROUND_LIMIT_RE = /\s*\[Stopped:\s*exceeded\s+(\d+)\s+tool rounds\]\s*$/i;

export function parseAssistantInterruptionNotice(
  content: string,
): ParsedAssistantInterruption {
  let visibleContent = content;
  const toolLimitMatch = visibleContent.match(TOOL_ROUND_LIMIT_RE);
  if (toolLimitMatch) {
    const matchIndex = toolLimitMatch.index ?? 0;
    visibleContent = visibleContent.slice(0, matchIndex).trimEnd();
    return {
      visibleContent,
      notice: {
        kind: "tool-limit",
        title: `已达到工具调用上限（${toolLimitMatch[1]} 轮）`,
        detail:
          "系统已停止自动工具调用，避免在同一类错误里无限循环。请先整理当前进展，或选择一个更小的下一步继续。",
      },
    };
  }

  let cancelled = false;
  while (CANCELLED_MARKER_RE.test(visibleContent)) {
    visibleContent = visibleContent.replace(CANCELLED_MARKER_RE, "").trimEnd();
    cancelled = true;
  }

  if (cancelled) {
    return {
      visibleContent,
      notice: {
        kind: "cancelled",
        title: "本轮已中断",
        detail:
          "工具调用已经停止，已产生的输出仍保留在上方。可以直接断点继续，或补充约束后重新发送。",
      },
    };
  }

  return { visibleContent, notice: null };
}

interface AssistantMessageBubbleProps {
  content: string;
  tokenUsage?: AssistantTokenUsage;
  components: Components;
  chat: ChatTokenSet;
  bubbleRadiusPx: number;
}

/**
 * Memoized completed assistant bubble. Parent Chat state changes frequently
 * during streaming and panel interactions; completed assistant rows should not
 * rebuild their Markdown subtree unless content/theme/components actually change.
 */
export const AssistantMessageBubble = memo(function AssistantMessageBubble({
  content,
  tokenUsage,
  components,
  chat,
  bubbleRadiusPx,
}: AssistantMessageBubbleProps) {
  const parsed = parseAssistantInterruptionNotice(content);
  const hasVisibleContent = parsed.visibleContent.trim().length > 0;

  return (
    <Box
      sx={{
        position: "relative",
        width: "100%",
        minWidth: 0,
        maxWidth: "100%",
        px: 1.75,
        py: 1.25,
        pb: tokenUsage ? 2.25 : 1.25,
        borderRadius: `${bubbleRadiusPx}px`,
        bgcolor: chat.agentBubbleBg,
        border: `1px solid ${chat.agentBubbleBorder}`,
        fontFamily: chat.font,
        overflow: "visible",
      }}
    >
      {hasVisibleContent ? (
        <ChatMarkdownContent
          content={parsed.visibleContent}
          tone="agent"
          components={components}
          chat={chat}
        />
      ) : null}
      {parsed.notice ? (
        <Alert
          severity="warning"
          variant="outlined"
          sx={{
            mt: hasVisibleContent ? 1.25 : 0,
            borderRadius: 2,
            alignItems: "center",
            bgcolor:
              parsed.notice.kind === "tool-limit"
                ? alpha(chat.accent, 0.045)
                : alpha(chat.doneGreen, 0.055),
            borderColor:
              parsed.notice.kind === "tool-limit"
                ? alpha(chat.accent, 0.24)
                : alpha(chat.doneGreen, 0.28),
            color: chat.textPrimary,
            "& .MuiAlert-icon": {
              color:
                parsed.notice.kind === "tool-limit"
                  ? chat.accent
                  : chat.doneGreen,
              opacity: 0.92,
            },
            "& .MuiAlert-message": {
              width: "100%",
              minWidth: 0,
            },
          }}
        >
          <Typography
            component="div"
            sx={{ fontWeight: 700, fontSize: 13.5, lineHeight: 1.35 }}
          >
            {parsed.notice.title}
          </Typography>
          <Typography
            component="div"
            sx={{
              mt: 0.35,
              fontSize: 12.5,
              lineHeight: 1.55,
              color: chat.textMuted,
            }}
          >
            {parsed.notice.detail}
          </Typography>
        </Alert>
      ) : null}
      {tokenUsage ? (
        <Typography
          component="div"
          sx={{
            position: "absolute",
            right: 10,
            bottom: 6,
            fontSize: 10,
            lineHeight: 1.2,
            color: alpha(chat.toolIcon, 0.85),
            userSelect: "none",
            pointerEvents: "none",
            textAlign: "right",
            maxWidth: "calc(100% - 20px)",
          }}
        >
          {formatAssistantTokenUsage(tokenUsage)}
        </Typography>
      ) : null}
    </Box>
  );
});
