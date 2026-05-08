import { memo } from "react";
import { Box, Typography } from "@mui/material";
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
      <ChatMarkdownContent
        content={content}
        tone="agent"
        components={components}
        chat={chat}
      />
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
