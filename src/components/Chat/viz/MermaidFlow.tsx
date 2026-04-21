/**
 * MermaidFlow — parses Mermaid flowchart/graph syntax and renders it
 * with React Flow + dagre layout.
 *
 * Supported:
 *   graph/flowchart  directions: TD TB LR RL BT
 *   nodes: A[rect]  A(round)  A{diamond}  A((circle))  A([stadium])
 *   card: labels with <br> become card-shaped multi-line nodes
 *   edges: -->  ---  -.->  with optional |label|
 *   subgraph: grouped nodes get per-group color coding
 *   cyclic graphs: handled correctly by dagre
 */

import { useCallback, useMemo } from "react";
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  Position,
  useNodesState,
  useEdgesState,
  type NodeProps,
  type Node,
  type Edge,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Box, Typography } from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  computeDagreLayout,
  buildFlowEdge,
  toRankdir,
  GRAPH_NODE_W,
  GRAPH_NODE_H,
  SHARED_RF_PROPS,
  graphInnerSx,
} from "./graphLayout";
import {
  parseMermaid,
  splitLabel,
  cardHeight,
  groupColor,
  type NodeShape,
} from "./mermaidParser";

// ─── React Flow custom node ───────────────────────────────────────────────────

interface MermaidNodeData extends Record<string, unknown> {
  label: string;
  shape: NodeShape;
  group?: string;
  onClick: () => void;
}

function shapeStyle(shape: NodeShape): React.CSSProperties {
  switch (shape) {
    case "diamond":  return { transform: "rotate(45deg)", borderRadius: 4 };
    case "circle":   return { borderRadius: "50%" };
    case "round":
    case "stadium":  return { borderRadius: 24 };
    case "card":     return { borderRadius: 8, textAlign: "left" };
    default:         return { borderRadius: 8 };
  }
}

function MermaidNode({ data }: NodeProps<Node<MermaidNodeData>>) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const lines = splitLabel(data.label);
  const isCard = data.shape === "card";
  const group = data.group;

  // Group-aware coloring
  let bgColor: string;
  let borderColor: string;
  let textColor: string;
  if (group) {
    const gc = groupColor(group);
    bgColor = gc.bg;
    borderColor = gc.border;
    textColor = gc.text;
  } else {
    bgColor = isDark
      ? alpha(theme.palette.background.paper, 0.88)
      : alpha(theme.palette.background.paper, 0.96);
    borderColor = isDark ? "#555" : "#ccc";
    textColor = theme.palette.text.primary;
  }

  return (
    <>
      <Handle type="target" position={Position.Top}   style={{ opacity: 0.5 }} />
      <Handle type="target" position={Position.Left}  style={{ opacity: 0.5 }} />
      <Box
        component="button"
        type="button"
        onClick={data.onClick}
        sx={{
          width: GRAPH_NODE_W,
          minHeight: GRAPH_NODE_H,
          px: isCard ? 1.5 : 1.25,
          py: isCard ? 1.25 : 0.75,
          border: `1.5px solid ${borderColor}`,
          bgcolor: bgColor,
          cursor: "pointer",
          font: "inherit",
          color: textColor,
          display: "flex",
          flexDirection: "column",
          alignItems: isCard ? "flex-start" : "center",
          justifyContent: isCard ? "flex-start" : "center",
          transition: "box-shadow 120ms, transform 100ms, border-color 120ms",
          "&:hover": {
            boxShadow: `0 2px 10px ${alpha(theme.palette.primary.main, 0.35)}`,
            borderColor: theme.palette.primary.main,
            transform: "translateY(-1px)",
          },
          "&:active": { transform: "scale(0.97)" },
          ...shapeStyle(data.shape),
        }}
      >
        {isCard && lines.length > 0 ? (
          <>
            {/* Title / header */}
            <Typography sx={{
              fontWeight: 700,
              fontSize: 12,
              lineHeight: 1.3,
              color: textColor,
              pointerEvents: "none",
              overflowWrap: "break-word",
              width: "100%",
              mb: 0.5,
              borderBottom: `1px solid ${alpha(textColor, 0.2)}`,
              pb: 0.25,
            }}>
              {lines[0]}
            </Typography>
            {/* Body lines */}
            {lines.slice(1).map((l, i) => (
              <Typography key={i} sx={{
                fontWeight: 400,
                fontSize: 10.5,
                lineHeight: 1.5,
                color: alpha(textColor, 0.85),
                pointerEvents: "none",
                overflowWrap: "break-word",
                width: "100%",
              }}>
                {l}
              </Typography>
            ))}
          </>
        ) : data.shape === "diamond" ? (
          <Typography sx={{
            fontWeight: 600,
            fontSize: 12,
            lineHeight: 1.4,
            color: textColor,
            pointerEvents: "none",
            overflowWrap: "break-word",
            transform: "rotate(-45deg)",
          }}>
            {data.label}
          </Typography>
        ) : (
          <Typography sx={{
            fontWeight: 600,
            fontSize: 12,
            lineHeight: 1.4,
            color: textColor,
            pointerEvents: "none",
            overflowWrap: "break-word",
          }}>
            {data.label}
          </Typography>
        )}
      </Box>
      <Handle type="source" position={Position.Bottom} style={{ opacity: 0.5 }} />
      <Handle type="source" position={Position.Right}  style={{ opacity: 0.5 }} />
    </>
  );
}

const mermaidNodeTypes = { mermaidNode: MermaidNode };

// ─── Main component ───────────────────────────────────────────────────────────

interface MermaidFlowProps {
  source: string;
  onNodeClick?: (text: string) => void;
}

export function MermaidFlow({ source, onNodeClick }: MermaidFlowProps) {
  const theme  = useTheme();
  const isDark = theme.palette.mode === "dark";

  const parsed   = useMemo(() => parseMermaid(source), [source]);
  const nodeList = useMemo(() => Array.from(parsed.nodes.values()), [parsed.nodes]);

  // Compute per-node heights for card shapes before dagre layout
  const nodesForLayout = useMemo(() =>
    nodeList.map((n) => {
      if (n.shape === "card") {
        const lines = splitLabel(n.label);
        const h = cardHeight(Math.max(1, lines.length));
        return { id: n.id, width: GRAPH_NODE_W, height: h };
      }
      return { id: n.id, width: GRAPH_NODE_W, height: GRAPH_NODE_H };
    }), [nodeList]);

  const { positions } = useMemo(() => computeDagreLayout({
    nodes: nodesForLayout,
    edges: parsed.edges,
    direction: toRankdir(parsed.direction),
  }), [nodesForLayout, parsed.edges, parsed.direction]);

  const initialNodes: Node<MermaidNodeData>[] = useMemo(() => nodeList.map((n) => ({
    id: n.id,
    type: "mermaidNode",
    position: positions.get(n.id) ?? { x: 0, y: 0 },
    data: {
      label: n.label,
      shape: n.shape,
      group: n.group,
      onClick: () => onNodeClick?.(n.label),
    },
  })), [nodeList, positions, onNodeClick]);

  const initialEdges: Edge[] = useMemo(() => parsed.edges.map((e, i) => buildFlowEdge({
    id: `e-${i}`,
    source: e.source,
    target: e.target,
    label: e.label,
    dashed: e.dashed,
    isDark,
    paperBg:   theme.palette.background.paper,
    defaultBg: theme.palette.background.default,
  })), [parsed.edges, isDark, theme]);

  const [nodes, , onNodesChange] = useNodesState(initialNodes);
  const [edges, , onEdgesChange] = useEdgesState(initialEdges);
  const onNodeClickRF = useCallback(() => {}, []);

  if (nodeList.length === 0) {
    return (
      <Box sx={{ my: 1, p: 1.5, borderRadius: 1, border: 1, borderColor: "divider" }}>
        <Typography variant="caption" color="text.secondary">无法解析流程图</Typography>
      </Box>
    );
  }

  return (
    <Box sx={{
      my: 1.25,
      borderRadius: 2,
      border: 1,
      borderColor: "divider",
      overflow: "hidden",
      bgcolor: isDark
        ? alpha(theme.palette.background.paper, 0.5)
        : alpha(theme.palette.background.default, 0.85),
    }}>
      <Box sx={graphInnerSx(isDark)}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClickRF}
          nodeTypes={mermaidNodeTypes}
          style={{ width: "100%", height: "100%" }}
          {...SHARED_RF_PROPS}
        >
          <Background variant={BackgroundVariant.Dots} gap={20} size={1} color={isDark ? "#333" : "#ddd"} />
          <Controls showInteractive={false} />
        </ReactFlow>
      </Box>
      {onNodeClick && (
        <Typography sx={{ fontSize: 10, color: "text.disabled", textAlign: "center", py: 0.5, borderTop: 1, borderColor: "divider", userSelect: "none" }}>
          点击节点可将内容追加到输入框
        </Typography>
      )}
    </Box>
  );
}
