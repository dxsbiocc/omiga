/**
 * DagFlow — renders an `omiga-dag` fenced code block as an interactive React Flow graph.
 *
 * Input format (JSON inside the fenced block):
 * ```omiga-dag
 * {
 *   "title": "Research Workflow",
 *   "nodes": [
 *     { "id": "A", "label": "Literature Review", "description": "Search papers", "tone": "blue" },
 *     { "id": "B", "label": "Data Collection", "tone": "green" }
 *   ],
 *   "edges": [
 *     { "source": "A", "target": "B" }
 *   ]
 * }
 * ```
 *
 * Clicking a node appends its label + description to the composer input.
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
  GRAPH_NODE_W,
  GRAPH_NODE_H,
  SHARED_RF_PROPS,
  graphInnerSx,
} from "./viz/graphLayout";

// ─── Types ────────────────────────────────────────────────────────────────────

export type DagNodeTone = "grey" | "green" | "blue" | "purple" | "brown" | "amber";

export interface DagNodeData {
  id: string;
  label: string;
  description?: string;
  tone?: DagNodeTone;
}

export interface DagEdgeData {
  source: string;
  target: string;
  label?: string;
}

export interface OmigaDagPayload {
  title?: string;
  nodes: DagNodeData[];
  edges: DagEdgeData[];
}

// ─── Tone → color ─────────────────────────────────────────────────────────────

function useToneColors(tone: DagNodeTone) {
  const theme = useTheme();
  const d = theme.palette.mode === "dark";
  const a = d ? 0.28 : 0.18;

  switch (tone) {
    case "green":  return { bg: alpha(theme.palette.success.main,    a), border: alpha(theme.palette.success.main,    0.6) };
    case "blue":   return { bg: alpha(theme.palette.info.main,       a), border: alpha(theme.palette.info.main,       0.6) };
    case "purple": return { bg: alpha(theme.palette.secondary.main,  a), border: alpha(theme.palette.secondary.main,  0.6) };
    case "brown":  return { bg: alpha(theme.palette.error.light,     a), border: alpha(theme.palette.error.light,     0.6) };
    case "amber":  return { bg: alpha(theme.palette.warning.main,    a), border: alpha(theme.palette.warning.main,    0.6) };
    default:       return { bg: alpha(theme.palette.grey[d ? 700 : 300], d ? 0.4 : 0.5), border: alpha(theme.palette.grey[d ? 500 : 400], 0.7) };
  }
}

// ─── Custom node ──────────────────────────────────────────────────────────────

interface DagNodeCustomData extends Record<string, unknown> {
  label: string;
  description?: string;
  tone: DagNodeTone;
  onClick: () => void;
}

function DagNode({ data }: NodeProps<Node<DagNodeCustomData>>) {
  const theme = useTheme();
  const colors = useToneColors(data.tone);

  return (
    <>
      <Handle type="target" position={Position.Top}  style={{ opacity: 0.4 }} />
      <Handle type="target" position={Position.Left} style={{ opacity: 0.4 }} />
      <Box
        component="button"
        type="button"
        onClick={data.onClick}
        sx={{
          width: GRAPH_NODE_W,
          minHeight: GRAPH_NODE_H,
          px: 1.25,
          py: 0.875,
          borderRadius: 1.5,
          border: `1.5px solid ${colors.border}`,
          bgcolor: colors.bg,
          cursor: "pointer",
          textAlign: "left",
          font: "inherit",
          color: "inherit",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          gap: 0.25,
          transition: "box-shadow 120ms ease, transform 100ms ease",
          "&:hover": {
            boxShadow: `0 2px 8px ${alpha(theme.palette.primary.main, 0.25)}`,
            transform: "translateY(-1px)",
          },
          "&:active": { transform: "scale(0.97)" },
        }}
      >
        <Typography sx={{ fontWeight: 700, fontSize: 12, lineHeight: 1.35, color: "text.primary", pointerEvents: "none" }}>
          {data.label}
        </Typography>
        {data.description ? (
          <Typography sx={{ fontSize: 11, lineHeight: 1.35, color: "text.secondary", pointerEvents: "none" }}>
            {data.description}
          </Typography>
        ) : null}
      </Box>
      <Handle type="source" position={Position.Bottom} style={{ opacity: 0.4 }} />
      <Handle type="source" position={Position.Right}  style={{ opacity: 0.4 }} />
    </>
  );
}

const dagNodeTypes = { dagNode: DagNode };

// ─── Main component ───────────────────────────────────────────────────────────

export function buildDagNodeClickText(node: DagNodeData): string {
  const parts = [node.label.trim()];
  if (node.description?.trim()) parts.push(node.description.trim());
  return parts.join("\n");
}

interface DagFlowProps {
  data: OmigaDagPayload;
  onNodeClick?: (text: string) => void;
}

export function DagFlow({ data, onNodeClick }: DagFlowProps) {
  const theme  = useTheme();
  const isDark = theme.palette.mode === "dark";

  const { positions } = useMemo(() => computeDagreLayout({
    nodes: data.nodes,
    edges: data.edges,
    direction: "TB",
  }), [data.nodes, data.edges]);

  const initialNodes: Node<DagNodeCustomData>[] = useMemo(() => data.nodes.map((n) => ({
    id: n.id,
    type: "dagNode",
    position: positions.get(n.id) ?? { x: 0, y: 0 },
    data: {
      label: n.label,
      description: n.description,
      tone: n.tone ?? "grey",
      onClick: () => onNodeClick?.(buildDagNodeClickText(n)),
    },
  })), [data.nodes, positions, onNodeClick]);

  const initialEdges: Edge[] = useMemo(() => data.edges.map((e, i) => buildFlowEdge({
    id: `e-${i}`,
    source: e.source,
    target: e.target,
    label: e.label,
    isDark,
    paperBg:   theme.palette.background.paper,
    defaultBg: theme.palette.background.default,
  })), [data.edges, isDark, theme]);

  const [nodes, , onNodesChange] = useNodesState(initialNodes);
  const [edges, , onEdgesChange] = useEdgesState(initialEdges);
  const onNodeClickRF = useCallback(() => {}, []);

  return (
    <Box sx={{
      my: 1.25,
      borderRadius: 2,
      border: 1,
      borderColor: "divider",
      overflow: "hidden",
      bgcolor: isDark
        ? alpha(theme.palette.background.paper, 0.5)
        : alpha(theme.palette.background.default, 0.8),
    }}>
      {data.title?.trim() ? (
        <Typography sx={{
          fontWeight: 700,
          fontSize: 13,
          px: 2,
          pt: 1.5,
          pb: 0.75,
          textAlign: "center",
          borderBottom: 1,
          borderColor: "divider",
          color: "text.primary",
        }}>
          {data.title.trim()}
        </Typography>
      ) : null}

      <Box sx={graphInnerSx(isDark)}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClickRF}
          nodeTypes={dagNodeTypes}
          style={{ width: "100%", height: "100%" }}
          {...SHARED_RF_PROPS}
        >
          <Background variant={BackgroundVariant.Dots} gap={20} size={1} color={isDark ? "#333" : "#ddd"} />
          <Controls showInteractive={false} />
        </ReactFlow>
      </Box>

      <Typography sx={{
        fontSize: 10,
        color: "text.disabled",
        textAlign: "center",
        py: 0.5,
        borderTop: 1,
        borderColor: "divider",
        userSelect: "none",
      }}>
        点击节点可将内容追加到输入框
      </Typography>
    </Box>
  );
}
