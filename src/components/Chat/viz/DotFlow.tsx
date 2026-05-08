/**
 * DotFlow — parses a subset of Graphviz DOT language and renders it
 * with React Flow + dagre layout.
 *
 * Supported:
 *   digraph / graph / subgraph (subgraph grouping ignored)
 *   node definitions:  id [label="…" shape=box/diamond/ellipse/circle]
 *   edges:             A -> B [label="…"]   A -- B
 *   comment:           // …   /* … *\/   # …
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

// ─── Parser ───────────────────────────────────────────────────────────────────

type DotShape = "rect" | "round" | "diamond" | "circle";

interface DotNode {
  id: string;
  label: string;
  shape: DotShape;
}

interface DotEdge {
  source: string;
  target: string;
  label?: string;
  dashed?: boolean;
}

interface DotGraph {
  directed: boolean;
  nodes: Map<string, DotNode>;
  edges: DotEdge[];
}

function stripComments(src: string): string {
  src = src.replace(/\/\*[\s\S]*?\*\//g, " ");
  src = src.replace(/(?:\/\/|#)[^\n]*/g, " ");
  return src;
}

function parseAttrs(attrStr: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  const re = /(\w+)\s*=\s*(?:"([^"]*?)"|([^,\]\s]+))/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(attrStr)) !== null) {
    attrs[m[1]] = m[2] ?? m[3];
  }
  return attrs;
}

function dotShapeFromAttr(shape?: string): DotShape {
  switch ((shape ?? "").toLowerCase()) {
    case "diamond":      return "diamond";
    case "ellipse":
    case "oval":         return "round";
    case "circle":
    case "doublecircle": return "circle";
    default:             return "rect";
  }
}

function escapeRegex(s: string) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function parseDot(src: string): DotGraph {
  const clean = stripComments(src);
  const directed = /\bdigraph\b/i.test(clean);

  const nodeMap = new Map<string, DotNode>();
  const edges: DotEdge[] = [];

  const ensureNode = (id: string, label?: string, shape?: DotShape) => {
    const existing = nodeMap.get(id);
    if (!existing) {
      nodeMap.set(id, { id, label: label ?? id, shape: shape ?? "rect" });
    } else if (label && label !== id) {
      nodeMap.set(id, { ...existing, label, shape: shape ?? existing.shape });
    }
  };

  const body = clean.replace(/^[^{]*\{/, "").replace(/\}[^}]*$/, "");

  const edgeOp = directed ? "->" : "--";
  const edgeRe = new RegExp(
    `"?([\\w./\\-]+)"?\\s*${escapeRegex(edgeOp)}\\s*"?([\\w./\\-]+)"?\\s*(\\[[^\\]]*\\])?`,
    "g",
  );
  const nodeRe = /^\s*"?([\w./\-]+)"?\s*(\[[^\]]*\])?\s*;?\s*$/;

  for (const rawLine of body.split(/[;\n]/)) {
    const line = rawLine.trim();
    if (!line) continue;
    if (/^(?:graph|digraph|subgraph|node|edge)\b/i.test(line)) continue;
    if (line === "{" || line === "}") continue;

    if (line.includes(edgeOp)) {
      edgeRe.lastIndex = 0;
      let m: RegExpExecArray | null;
      while ((m = edgeRe.exec(line)) !== null) {
        const src2 = m[1];
        const tgt = m[2];
        const attrStr = m[3] ?? "";
        const attrs = parseAttrs(attrStr);
        ensureNode(src2);
        ensureNode(tgt);
        edges.push({
          source: src2,
          target: tgt,
          label: attrs.label,
          dashed: (attrs.style ?? "").includes("dash"),
        });
      }
      continue;
    }

    const nm = nodeRe.exec(line);
    if (nm) {
      const id = nm[1];
      if (id === "graph" || id === "node" || id === "edge") continue;
      const attrStr = nm[2] ?? "";
      const attrs = parseAttrs(attrStr);
      ensureNode(id, attrs.label ?? id, dotShapeFromAttr(attrs.shape));
    }
  }

  return { directed, nodes: nodeMap, edges };
}

// ─── React Flow node ─────────────────────────────────────────────────────────

interface DotNodeData extends Record<string, unknown> {
  label: string;
  shape: DotShape;
  onClick: () => void;
}

function shapeStyle(shape: DotShape): React.CSSProperties {
  switch (shape) {
    case "diamond": return { transform: "rotate(45deg)", borderRadius: 4 };
    case "circle":  return { borderRadius: "50%" };
    case "round":   return { borderRadius: 24 };
    default:        return { borderRadius: 8 };
  }
}

function DotNodeView({ data }: NodeProps<Node<DotNodeData>>) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  return (
    <>
      <Handle type="target" position={Position.Top}  style={{ opacity: 0.5 }} />
      <Handle type="target" position={Position.Left} style={{ opacity: 0.5 }} />
      <Box
        component="button"
        type="button"
        onClick={data.onClick}
        sx={{
          width: GRAPH_NODE_W,
          minHeight: GRAPH_NODE_H,
          px: 1.25,
          py: 0.75,
          border: `1.5px solid ${isDark ? "#555" : "#ccc"}`,
          bgcolor: isDark
            ? alpha(theme.palette.background.paper, 0.88)
            : alpha(theme.palette.background.paper, 0.96),
          cursor: "pointer",
          textAlign: "center",
          font: "inherit",
          color: "inherit",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
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
        <Typography sx={{
          fontWeight: 600,
          fontSize: 12,
          lineHeight: 1.4,
          color: "text.primary",
          pointerEvents: "none",
          overflowWrap: "break-word",
          ...(data.shape === "diamond" ? { transform: "rotate(-45deg)" } : {}),
        }}>
          {data.label}
        </Typography>
      </Box>
      <Handle type="source" position={Position.Bottom} style={{ opacity: 0.5 }} />
      <Handle type="source" position={Position.Right}  style={{ opacity: 0.5 }} />
    </>
  );
}

const dotNodeTypes = { dotNode: DotNodeView };

// ─── Main component ───────────────────────────────────────────────────────────

interface DotFlowProps {
  dot: string;
  onNodeClick?: (text: string) => void;
}

export function DotFlow({ dot, onNodeClick }: DotFlowProps) {
  const theme  = useTheme();
  const isDark = theme.palette.mode === "dark";

  const parsed   = useMemo(() => parseDot(dot), [dot]);
  const nodeList = useMemo(() => Array.from(parsed.nodes.values()), [parsed.nodes]);

  const direction = parsed.directed ? "LR" : "TB";

  const { positions } = useMemo(() => computeDagreLayout({
    nodes: nodeList,
    edges: parsed.edges,
    direction: toRankdir(direction),
  }), [nodeList, parsed.edges, direction]);

  const initialNodes: Node<DotNodeData>[] = useMemo(() => nodeList.map((n) => ({
    id: n.id,
    type: "dotNode",
    position: positions.get(n.id) ?? { x: 0, y: 0 },
    data: { label: n.label, shape: n.shape, onClick: () => onNodeClick?.(n.label) },
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
        <Typography variant="caption" color="text.secondary">无法解析图结构</Typography>
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
          nodeTypes={dotNodeTypes}
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
