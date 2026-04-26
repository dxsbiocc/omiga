import type { CSSProperties } from "react";
import { Box, Typography } from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import {
  oneDark,
  oneLight,
} from "react-syntax-highlighter/dist/esm/styles/prism";
import type { getChatTokens } from "./chatTokens";

type ChatTokens = ReturnType<typeof getChatTokens>;

const MD_BLOCK_RADIUS_PX = 1;
const PRISM_CODE_SEL = 'code[class*="language-"]';
const PRISM_PRE_SEL = 'pre[class*="language-"]';

/** Prism oneLight/oneDark set a fill on `code`/`pre`; only keep the outer chat box background. */
function prismStyleTransparentCodeSurface(
  style: Record<string, CSSProperties>,
): Record<string, CSSProperties> {
  return {
    ...style,
    [PRISM_CODE_SEL]: {
      ...(style[PRISM_CODE_SEL] ?? {}),
      background: "transparent",
      backgroundColor: "transparent",
    },
    [PRISM_PRE_SEL]: {
      ...(style[PRISM_PRE_SEL] ?? {}),
      background: "transparent",
      backgroundColor: "transparent",
    },
  };
}

interface FencedCodeBlockProps {
  code: string;
  lang: string;
  isAgent: boolean;
  chat: ChatTokens;
}

export function FencedCodeBlock({
  code,
  lang,
  isAgent,
  chat,
}: FencedCodeBlockProps) {
  const theme = useTheme();
  const prismStyleRaw = theme.palette.mode === "dark" ? oneDark : oneLight;
  const prismStyleFenced = prismStyleTransparentCodeSurface(
    prismStyleRaw as Record<string, CSSProperties>,
  );
  const fencedScrollStyle = {
    margin: 0,
    borderRadius: 0,
    background: "transparent",
    whiteSpace: "pre" as const,
    wordBreak: "normal" as const,
    overflowWrap: "normal" as const,
    minWidth: "min-content",
  };

  if (isAgent) {
    return (
      <Box sx={{ my: 1.25 }}>
        <Typography
          sx={{
            fontSize: 10,
            color: chat.labelMuted,
            fontWeight: 400,
            mb: 0.5,
            display: "block",
          }}
        >
          {lang}
        </Typography>
        <Box
          sx={{
            borderRadius: `${MD_BLOCK_RADIUS_PX}px`,
            border: `1px solid ${chat.agentBubbleBorder}`,
            bgcolor: chat.codeBg,
            maxHeight: 320,
            maxWidth: "100%",
            overflow: "auto",
            [`& ${PRISM_PRE_SEL}, & ${PRISM_CODE_SEL}`]: {
              background: "transparent !important",
              backgroundColor: "transparent !important",
            },
          }}
        >
          <SyntaxHighlighter
            style={prismStyleFenced}
            language={lang}
            PreTag="div"
            customStyle={{
              ...fencedScrollStyle,
              padding: "8px 12px",
              fontSize: 11,
              lineHeight: 1.45,
            }}
          >
            {code}
          </SyntaxHighlighter>
        </Box>
      </Box>
    );
  }

  return (
    <Box
      component="div"
      sx={{
        my: 1.5,
        borderRadius: `${MD_BLOCK_RADIUS_PX}px`,
        overflow: "hidden",
        border: 1,
        borderColor: alpha(theme.palette.divider, 0.5),
      }}
    >
      <Box
        sx={{
          px: 2,
          py: 0.5,
          bgcolor: alpha(theme.palette.background.paper, 0.5),
          borderBottom: 1,
          borderColor: alpha(theme.palette.divider, 0.3),
        }}
      >
        <Typography variant="caption" color="text.secondary">
          {lang}
        </Typography>
      </Box>
      <Box
        sx={{
          bgcolor: chat.codeBg,
          maxHeight: 360,
          maxWidth: "100%",
          overflow: "auto",
          [`& ${PRISM_PRE_SEL}, & ${PRISM_CODE_SEL}`]: {
            background: "transparent !important",
            backgroundColor: "transparent !important",
          },
        }}
      >
        <SyntaxHighlighter
          style={prismStyleFenced}
          language={lang}
          PreTag="div"
          customStyle={{
            ...fencedScrollStyle,
            padding: "12px 16px",
            fontSize: "0.8125rem",
            lineHeight: 1.6,
          }}
        >
          {code}
        </SyntaxHighlighter>
      </Box>
    </Box>
  );
}

export default FencedCodeBlock;
