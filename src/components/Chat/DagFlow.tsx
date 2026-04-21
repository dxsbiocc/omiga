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

// ─── Types ────────────────────────────────────────────────────────────────────

export type DagNodeTone =
  | "grey"
  | "green"
  | "blue"
  | "purple"
  | "brown"
  | "amber";

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

// ─── Layout (topological BFS column layout) ───────────────────────────────────

const NODE_W = 180;
const NODE_H = 64;
const COL_GAP = 60;
const ROW_GAP = 24;

function computeLayout(
  nodes: DagNodeData[],
  edges: DagEdgeData[],
): Map<string, { x: number; y: number }> {
  // Build adjacency and in-degree maps
  const inDegree = new Map<string, number>(nodes.map((n) => [n.id, 0]));
  const adj = new Map<string, string[]>(nodes.map((n) => [n.id, []]));
  for (const e of edges) {
    inDegree.set(e.target, (inDegree.get(e.target) ?? 0) + 1);
    adj.get(e.source)?.push(e.target);
  }

  // Kahn's BFS to assign levels
  const level = new Map<string, number>(nodes.map((n) => [n.id, 0]));
  const queue: string[] = [];
  for (const [id, deg] of inDegree) {
    if (deg === 0) queue.push(id);
  }
  const tempIn = new Map(inDegree);
  while (queue.length) {
    const cur = queue.shift()!;
    const curLevel = level.get(cur) ?? 0;
    for (const nb of adj.get(cur) ?? []) {
      const next = (tempIn.get(nb) ?? 1) - 1;
      tempIn.set(nb, next);
      level.set(nb, Math.max(level.get(nb) ?? 0, curLevel + 1));
      if (next === 0) queue.push(nb);
    }
  }

  // Group by level
  const byLevel = new Map<number, string[]>();
  for (const [id, lv] of level) {
    if (!byLevel.has(lv)) byLevel.set(lv, []);
    byLevel.get(lv)!.push(id);
  }

  // Assign positions (columns = levels, rows within each column)
  const positions = new Map<string, { x: number; y: number }>();
  for (const [lv, ids] of byLevel) {
    const totalH = ids.length * (NODE_H + ROW_GAP) - ROW_GAP;
    ids.forEach((id, row) => {
      positions.set(id, {
        x: lv * (NODE_W + COL_GAP),
        y: row * (NODE_H + ROW_GAP) - totalH / 2,
      });
    });
  }
  return positions;
}

// ─── Tone → color ─────────────────────────────────────────────────────────────

function useToneColors(tone: DagNodeTone, clicked: boolean) {
  const theme = useTheme();
  const d = theme.palette.mode === "dark";
  const baseAlpha = clicked ? 0.42 : d ? 0.28 : 0.18;

  switch (tone) {
    case "green":
      return {
        bg: alpha(theme.palette.success.main, baseAlpha),
        border: alpha(theme.palette.success.main, 0.6),
      };
    case "blue":
      return {
        bg: alpha(theme.palette.info.main, baseAlpha),
        border: alpha(theme.palette.info.main, 0.6),
      };
    case "purple":
      return {
        bg: alpha(theme.palette.secondary.main, baseAlpha),
        border: alpha(theme.palette.secondary.main, 0.6),
      };
    case "brown":
      return {
        bg: alpha(theme.palette.error.light, baseAlpha),
        border: alpha(theme.palette.error.light, 0.6),
      };
    case "amber":
      return {
        bg: alpha(theme.palette.warning.main, baseAlpha),
        border: alpha(theme.palette.warning.main, 0.6),
      };
    default: // grey
      return {
        bg: alpha(theme.palette.grey[d ? 700 : 300], d ? 0.4 : 0.5),
        border: alpha(theme.palette.grey[d ? 500 : 400], 0.7),
      };
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
  const colors = useToneColors(data.tone, false);

  return (
    <>
      <Handle type="target" position={Position.Left} style={{ opacity: 0.4 }} />
      <Box
        component="button"
        type="button"
        onClick={data.onClick}
        sx={{
          width: NODE_W,
          minHeight: NODE_H,
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
        <Typography
          sx={{
            fontWeight: 700,
            fontSize: 12,
            lineHeight: 1.35,
            color: "text.primary",
            pointerEvents: "none",
          }}
        >
          {data.label}
        </Typography>
        {data.description ? (
          <Typography
            sx={{
              fontSize: 11,
              lineHeight: 1.35,
              color: "text.secondary",
              pointerEvents: "none",
            }}
          >
            {data.description}
          </Typography>
        ) : null}
      </Box>
      <Handle
        type="source"
        position={Position.Right}
        style={{ opacity: 0.4 }}
      />
    </>
  );
}

const nodeTypes = { dagNode: DagNode };

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
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";

  const positions = useMemo(
    () => computeLayout(data.nodes, data.edges),
    [data.nodes, data.edges],
  );

  // Compute canvas size
  const maxRows = useMemo(() => {
    const rowCounts = data.nodes.reduce<Map<number, number>>((acc, n) => {
      const pos = positions.get(n.id);
      if (!pos) return acc;
      const col = Math.round(pos.x / (NODE_W + COL_GAP));
      acc.set(col, (acc.get(col) ?? 0) + 1);
      return acc;
    }, new Map());
    return Math.max(1, ...rowCounts.values());
  }, [data.nodes, positions]);

  const canvasH = Math.max(120, maxRows * (NODE_H + ROW_GAP) - ROW_GAP + 80);

  const initialNodes: Node<DagNodeCustomData>[] = useMemo(
    () =>
      data.nodes.map((n) => ({
        id: n.id,
        type: "dagNode",
        position: positions.get(n.id) ?? { x: 0, y: 0 },
        data: {
          label: n.label,
          description: n.description,
          tone: n.tone ?? "grey",
          onClick: () => onNodeClick?.(buildDagNodeClickText(n)),
        },
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data.nodes, positions],
  );

  const initialEdges: Edge[] = useMemo(
    () =>
      data.edges.map((e, i) => ({
        id: `e-${i}`,
        source: e.source,
        target: e.target,
        label: e.label,
        animated: false,
        style: { stroke: isDark ? "#666" : "#bbb", strokeWidth: 1.5 },
        labelStyle: { fontSize: 10, fill: isDark ? "#aaa" : "#777" },
        labelBgStyle: {
          fill: isDark
            ? theme.palette.background.paper
            : theme.palette.background.default,
          fillOpacity: 0.8,
        },
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [data.edges, isDark],
  );

  const [nodes, , onNodesChange] = useNodesState(initialNodes);
  const [edges, , onEdgesChange] = useEdgesState(initialEdges);

  const onNodeClickRF = useCallback(() => {
    // Node clicks handled inside each node's button onClick — no-op here
  }, []);

  return (
    <Box
      sx={{
        my: 1.25,
        borderRadius: 2,
        border: 1,
        borderColor: "divider",
        overflow: "hidden",
        bgcolor: isDark
          ? alpha(theme.palette.background.paper, 0.5)
          : alpha(theme.palette.background.default, 0.8),
      }}
    >
      {data.title?.trim() ? (
        <Typography
          sx={{
            fontWeight: 700,
            fontSize: 13,
            px: 2,
            pt: 1.5,
            pb: 0.75,
            textAlign: "center",
            borderBottom: 1,
            borderColor: "divider",
            color: "text.primary",
          }}
        >
          {data.title.trim()}
        </Typography>
      ) : null}

      <Box sx={{ width: "100%", height: canvasH, minHeight: 120 }}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClickRF}
          nodeTypes={nodeTypes}
          fitView
          fitViewOptions={{ padding: 0.2 }}
          proOptions={{ hideAttribution: true }}
          nodesDraggable
          nodesConnectable={false}
          elementsSelectable={false}
          zoomOnScroll={false}
          panOnScroll={false}
          preventScrolling={false}
          style={{ width: "100%", height: "100%" }}
        >
          <Background
            variant={BackgroundVariant.Dots}
            gap={20}
            size={1}
            color={isDark ? "#333" : "#ddd"}
          />
        </ReactFlow>
      </Box>

      <Typography
        sx={{
          fontSize: 10,
          color: "text.disabled",
          textAlign: "center",
          py: 0.5,
          borderTop: 1,
          borderColor: "divider",
          userSelect: "none",
        }}
      >
        点击节点可将内容追加到输入框
      </Typography>
    </Box>
  );
}
