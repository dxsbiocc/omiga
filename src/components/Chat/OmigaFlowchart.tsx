import { Box, Typography } from "@mui/material";
import type { Theme } from "@mui/material/styles";
import { alpha, useTheme } from "@mui/material/styles";

/** 与助手气泡内 Markdown 代码块语言 `omiga-flowchart` 配套的 JSON 结构 */
export interface OmigaFlowchartNode {
  id: string;
  title: string;
  lines?: string[];
  tone?: "grey" | "green" | "blue" | "purple" | "brown" | "amber";
}

export type OmigaFlowchartStage =
  | { node: OmigaFlowchartNode }
  | { parallel: OmigaFlowchartNode[] }
  | { pair: [OmigaFlowchartNode, OmigaFlowchartNode] };

export interface OmigaFlowchartPayload {
  title?: string;
  stages: OmigaFlowchartStage[];
}

export function buildOmigaFlowchartFillText(node: OmigaFlowchartNode): string {
  const parts = [node.title.trim()];
  if (node.lines?.length) {
    for (const line of node.lines) {
      const t = line.trim();
      if (t) parts.push(t);
    }
  }
  return parts.join("\n");
}

function toneSurface(theme: Theme, tone: NonNullable<OmigaFlowchartNode["tone"]>) {
  const d = theme.palette.mode === "dark";
  switch (tone) {
    case "grey":
      return alpha(theme.palette.grey[d ? 700 : 300], d ? 0.35 : 0.45);
    case "green":
      return alpha(theme.palette.success.main, d ? 0.22 : 0.18);
    case "blue":
      return alpha(theme.palette.info.main, d ? 0.22 : 0.16);
    case "purple":
      return alpha(theme.palette.secondary.main, d ? 0.28 : 0.14);
    case "brown":
      return alpha(theme.palette.error.light, d ? 0.2 : 0.12);
    case "amber":
      return alpha(theme.palette.warning.main, d ? 0.22 : 0.18);
    default:
      return alpha(theme.palette.grey[d ? 700 : 300], d ? 0.35 : 0.45);
  }
}

function FlowNodeBox({
  node,
  isAgent,
  onClick,
}: {
  node: OmigaFlowchartNode;
  isAgent: boolean;
  onClick?: () => void;
}) {
  const theme = useTheme();
  const tone = node.tone ?? "grey";
  const bg = toneSurface(theme, tone);
  const clickable = Boolean(onClick);

  return (
    <Box
      component={clickable ? "button" : "div"}
      type={clickable ? "button" : undefined}
      onClick={clickable ? onClick : undefined}
      sx={{
        width: "100%",
        textAlign: "left",
        borderRadius: 2,
        border: 1,
        borderColor: "divider",
        bgcolor: bg,
        px: 1.25,
        py: 1,
        cursor: clickable ? "pointer" : "default",
        transition: "box-shadow 120ms ease, transform 120ms ease",
        font: "inherit",
        color: "inherit",
        display: "block",
        ...(clickable
          ? {
              "&:hover": {
                boxShadow: 1,
              },
              "&:active": {
                transform: "scale(0.99)",
              },
            }
          : {}),
      }}
    >
      <Typography
        sx={{
          fontWeight: 700,
          fontSize: isAgent ? 12 : 13,
          lineHeight: 1.35,
          color: isAgent ? "text.primary" : "text.primary",
        }}
      >
        {node.title}
      </Typography>
      {node.lines?.map((line, i) => (
        <Typography
          key={`${node.id}-L${i}`}
          sx={{
            mt: 0.35,
            fontSize: isAgent ? 11 : 12,
            color: "text.secondary",
            lineHeight: 1.4,
          }}
        >
          {line}
        </Typography>
      ))}
    </Box>
  );
}

function DownArrow() {
  return (
    <Typography
      component="div"
      sx={{
        textAlign: "center",
        color: "text.disabled",
        fontSize: 16,
        lineHeight: 1,
        py: 0.5,
        userSelect: "none",
      }}
    >
      ↓
    </Typography>
  );
}

export function OmigaFlowchart({
  data,
  isAgent,
  onStepClick,
}: {
  data: OmigaFlowchartPayload;
  isAgent: boolean;
  onStepClick?: (text: string) => void;
}) {
  const stages = data.stages ?? [];

  return (
    <Box
      sx={{
        my: 1.25,
        p: 1.5,
        borderRadius: 2,
        border: 1,
        borderColor: "divider",
        bgcolor: (t) => alpha(t.palette.background.paper, 0.6),
        maxWidth: "100%",
      }}
    >
      {data.title?.trim() ? (
        <Typography
          sx={{
            fontWeight: 700,
            fontSize: isAgent ? 13 : 14,
            mb: 1.25,
            textAlign: "center",
            color: "text.primary",
          }}
        >
          {data.title.trim()}
        </Typography>
      ) : null}

      {stages.map((stage, idx) => {
        const key = `stage-${idx}`;
        const showArrowDown = idx < stages.length - 1;

        if ("node" in stage) {
          return (
            <Box key={key}>
              <FlowNodeBox
                node={stage.node}
                isAgent={isAgent}
                onClick={
                  onStepClick
                    ? () => onStepClick(buildOmigaFlowchartFillText(stage.node))
                    : undefined
                }
              />
              {showArrowDown ? <DownArrow /> : null}
            </Box>
          );
        }

        if ("parallel" in stage) {
          return (
            <Box key={key}>
              <Box
                sx={{
                  display: "flex",
                  flexDirection: { xs: "column", sm: "row" },
                  gap: 1,
                  alignItems: "stretch",
                  width: "100%",
                }}
              >
                {stage.parallel.map((n) => (
                  <Box key={n.id} sx={{ flex: 1, minWidth: 0 }}>
                    <FlowNodeBox
                      node={n}
                      isAgent={isAgent}
                      onClick={
                        onStepClick
                          ? () => onStepClick(buildOmigaFlowchartFillText(n))
                          : undefined
                      }
                    />
                  </Box>
                ))}
              </Box>
              {showArrowDown ? <DownArrow /> : null}
            </Box>
          );
        }

        if ("pair" in stage) {
          const [a, b] = stage.pair;
          return (
            <Box key={key}>
              <Box
                sx={{
                  display: "flex",
                  flexDirection: { xs: "column", sm: "row" },
                  gap: 1,
                  alignItems: "stretch",
                }}
              >
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <FlowNodeBox
                    node={a}
                    isAgent={isAgent}
                    onClick={
                      onStepClick
                        ? () => onStepClick(buildOmigaFlowchartFillText(a))
                        : undefined
                    }
                  />
                </Box>
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <FlowNodeBox
                    node={b}
                    isAgent={isAgent}
                    onClick={
                      onStepClick
                        ? () => onStepClick(buildOmigaFlowchartFillText(b))
                        : undefined
                    }
                  />
                </Box>
              </Box>
              {showArrowDown ? <DownArrow /> : null}
            </Box>
          );
        }

        return null;
      })}
    </Box>
  );
}
