import { memo } from "react";
import type { Components } from "react-markdown";
import { Box, Chip, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import type { getChatTokens } from "./chatTokens";
import { ChatMarkdownContent } from "./ChatMarkdownContent";

type ChatTokens = ReturnType<typeof getChatTokens>;

interface TraceMarkdownProps {
  content: string;
  chat: ChatTokens;
  components: Components;
}

function TraceMarkdown({ content, chat, components }: TraceMarkdownProps) {
  return (
    <ChatMarkdownContent
      content={content}
      tone="agent"
      components={components}
      chat={chat}
    />
  );
}

export interface AssistantTraceItemProps {
  content: string;
  intermediate?: boolean;
  chat: ChatTokens;
  components: Components;
}

export const AssistantTraceItem = memo(function AssistantTraceItem({
  content,
  intermediate = false,
  chat,
  components,
}: AssistantTraceItemProps) {
  if (!content.trim()) return null;

  return (
    <Box
      sx={{
        pb: 0.25,
        ...(intermediate
          ? {
              px: 1,
              py: 0.75,
              borderRadius: "8px",
              bgcolor: alpha(chat.agentBubbleBg, 0.62),
              border: `1px dashed ${alpha(chat.agentBubbleBorder, 0.95)}`,
            }
          : {}),
      }}
    >
      {intermediate && (
        <Stack
          direction="row"
          alignItems="center"
          spacing={0.75}
          sx={{ mb: 0.5 }}
        >
          <Chip
            size="small"
            label="思考"
            sx={{ height: 18, fontSize: 9, color: chat.textMuted }}
          />
        </Stack>
      )}
      <Box
        sx={{
          fontSize: 12,
          color: chat.textMuted,
          lineHeight: 1.45,
          "& p": { m: 0 },
        }}
      >
        <TraceMarkdown content={content} chat={chat} components={components} />
      </Box>
    </Box>
  );
});

export interface LiveIntermediateTraceProps {
  foldId: string;
  content: string;
  chat: ChatTokens;
  components: Components;
}

export const LiveIntermediateTrace = memo(function LiveIntermediateTrace({
  foldId,
  content,
  chat,
  components,
}: LiveIntermediateTraceProps) {
  if (!content.trim()) return null;

  return (
    <Box
      key={`${foldId}-live-assistant-segment`}
      sx={{
        pb: 0.25,
        px: 1,
        py: 0.75,
        borderRadius: "8px",
        bgcolor: alpha(chat.agentBubbleBg, 0.62),
        border: `1px dashed ${alpha(chat.agentBubbleBorder, 0.95)}`,
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={0.75}
        sx={{ mb: 0.5 }}
      >
        <Chip
          size="small"
          label="思考中"
          sx={{ height: 18, fontSize: 9, color: chat.textMuted }}
        />
        <Typography sx={{ fontSize: 10, color: chat.labelMuted }}>
          流式中；下一次行动会接在这里
        </Typography>
      </Stack>
      <Box
        sx={{
          fontSize: 12,
          color: chat.textMuted,
          lineHeight: 1.45,
          "& p": { m: 0 },
        }}
      >
        <TraceMarkdown content={content} chat={chat} components={components} />
        <Box
          component="span"
          sx={{
            display: "inline-block",
            width: 7,
            height: 14,
            bgcolor: chat.accent,
            ml: 0.5,
            verticalAlign: "text-bottom",
            animation: "pulse 1s ease-in-out infinite",
            "@keyframes pulse": {
              "0%, 100%": { opacity: 1 },
              "50%": { opacity: 0.3 },
            },
          }}
        />
      </Box>
    </Box>
  );
});
