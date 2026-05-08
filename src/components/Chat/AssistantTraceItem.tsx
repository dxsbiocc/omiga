import { memo, useState } from "react";
import type { Components } from "react-markdown";
import { Box, Collapse, Stack, Typography } from "@mui/material";
import { alpha } from "@mui/material/styles";
import { ExpandMore } from "@mui/icons-material";
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

function thoughtPreviewText(content: string): string {
  return content
    .replace(/```[\s\S]*?```/g, " code block ")
    .replace(/[`*_#>\[\]()]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

export interface CollapsibleThoughtTraceProps {
  content: string;
  chat: ChatTokens;
  components: Components;
  live?: boolean;
  defaultExpanded?: boolean;
  sx?: object;
}

export const CollapsibleThoughtTrace = memo(function CollapsibleThoughtTrace({
  content,
  chat,
  components,
  live = false,
  defaultExpanded = false,
  sx,
}: CollapsibleThoughtTraceProps) {
  const trimmed = content.trim();
  const [expanded, setExpanded] = useState(defaultExpanded);
  if (!trimmed) return null;

  return (
    <Box
      sx={{
        borderRadius: "10px",
        border: `1px solid ${alpha(chat.agentBubbleBorder, 0.92)}`,
        bgcolor: alpha(chat.agentBubbleBg, 0.48),
        overflow: "hidden",
        transition: "border-color 180ms ease, background-color 180ms ease",
        "&:hover": {
          borderColor: alpha(chat.accent, 0.24),
          bgcolor: alpha(chat.agentBubbleBg, 0.62),
        },
        ...sx,
      }}
    >
      <Stack
        direction="row"
        alignItems="center"
        spacing={0.75}
        onClick={() => setExpanded((v) => !v)}
        sx={{
          cursor: "pointer",
          userSelect: "none",
          px: 1.25,
          py: 0.82,
          minWidth: 0,
        }}
      >
        <Typography
          component="span"
          sx={{
            fontSize: 12,
            fontWeight: 700,
            color: chat.textPrimary,
            flexShrink: 0,
          }}
        >
          Thoughts
        </Typography>
        <Typography
          component="span"
          title={thoughtPreviewText(trimmed)}
          sx={{
            fontSize: 12,
            color: chat.textMuted,
            lineHeight: 1.35,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            minWidth: 0,
            flex: 1,
          }}
        >
          {thoughtPreviewText(trimmed)}
          {live ? " …" : ""}
        </Typography>
        <ExpandMore
          sx={{
            fontSize: 17,
            color: chat.toolIcon,
            opacity: 0.68,
            flexShrink: 0,
            transform: expanded ? "rotate(0deg)" : "rotate(-90deg)",
            transition: "transform 0.18s ease",
          }}
        />
      </Stack>
      <Collapse in={expanded} unmountOnExit>
        <Box
          sx={{
            borderTop: `1px solid ${alpha(chat.agentBubbleBorder, 0.78)}`,
            px: 1.25,
            py: 0.95,
            fontSize: 12,
            color: chat.textMuted,
            lineHeight: 1.45,
            "& p": { m: 0 },
          }}
        >
          <TraceMarkdown content={trimmed} chat={chat} components={components} />
          {live && (
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
          )}
        </Box>
      </Collapse>
    </Box>
  );
});

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

  if (intermediate) {
    return (
      <CollapsibleThoughtTrace
        content={content}
        chat={chat}
        components={components}
      />
    );
  }

  return (
    <Box
      sx={{
        pb: 0.25,
      }}
    >
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
    <CollapsibleThoughtTrace
      key={`${foldId}-live-assistant-segment`}
      content={content}
      chat={chat}
      components={components}
      live
    />
  );
});
