/**
 * DotFlow — parses a subset of Graphviz DOT language and renders it
 * with React Flow instead of an external viz.js iframe.
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

/** Strip DOT comments from source. */
function stripComments(src: string): string {
  // Block comments /* … */
  src = src.replace(/\/\*[\s\S]*?\*\//g, " ");
  // Line comments // … and # …
  src = src.replace(/(?:\/\/|#)[^\n]*/g, " ");
  return src;
}

/** Parse attr list `[key=value key="value" …]` into a plain object. */
function parseAttrs(attrStr: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  // Match key=value or key="value"
  const re = /(\w+)\s*=\s*(?:"([^"]*?)"|([^,\]\s]+))/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(attrStr)) !== null) {
    attrs[m[1]] = m[2] ?? m[3];
  }
  return attrs;
}

function dotShapeFromAttr(shape?: string): DotShape {
  switch ((shape ?? "").toLowerCase()) {
    case "diamond":
      return "diamond";
    case "ellipse":
    case "oval":
      return "round";
    case "circle":
    case "doublecircle":
      return "circle";
    default:
      return "rect";
  }
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

  // Tokenise into statements (split by ; or newline outside quotes/braces)
  // Simpler: strip outer braces then process line-by-line
  const body = clean.replace(/^[^{]*\{/, "").replace(/\}[^}]*$/, "");

  // Edge pattern: A -> B [attrs] or A -- B [attrs], also compound A -> B -> C
  const edgeOp = directed ? "->" : "--";
  const edgeRe = new RegExp(
    `"?([\\w./\\-]+)"?\\s*${escapeRegex(edgeOp)}\\s*"?([\\w./\\-]+)"?\\s*(\\[[^\\]]*\\])?`,
    "g",
  );

  // Node pattern: id [attrs]  (no edge op on the line)
  const nodeRe = /^\s*"?([\w./\-]+)"?\s*(\[[^\]]*\])?\s*;?\s*$/;

  for (const rawLine of body.split(/[;\n]/)) {
    const line = rawLine.trim();
    if (!line) continue;
    if (/^(?:graph|digraph|subgraph|node|edge)\b/i.test(line)) continue;
    if (line === "{" || line === "}") continue;

    // Edge lines
    if (line.includes(edgeOp)) {
      edgeRe.lastIndex = 0;
      let m: RegExpExecArray | null;
      let prev: string | null = null;
      while ((m = edgeRe.exec(line)) !== null) {
        const src2 = m[1];
        const tgt = m[2];
        const attrStr = m[3] ?? "";
        const attrs = parseAttrs(attrStr);
        ensureNode(src2);
        ensureNode(tgt);
        if (prev && prev !== src2) {
          // chained edge: already captured by loop
        }
        edges.push({
          source: src2,
          target: tgt,
          label: attrs.label,
          dashed: (attrs.style ?? "").includes("dash"),
        });
        prev = tgt;
      }
      continue;
    }

    // Node definition
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

function escapeRegex(s: string) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// ─── Layout (same BFS as MermaidFlow) ────────────────────────────────────────

const NODE_W = 160;
const NODE_H = 52;
const COL_GAP = 70;
const ROW_GAP = 20;

function computeLayout(nodes: DotNode[], edges: DotEdge[]): Map<string, { x: number; y: number }> {
  const ids = nodes.map((n) => n.id);
  const inDegree = new Map<string, number>(ids.map((id) => [id, 0]));
  const adj = new Map<string, string[]>(ids.map((id) => [id, []]));

  for (const e of edges) {
    if (inDegree.has(e.target)) inDegree.set(e.target, (inDegree.get(e.target) ?? 0) + 1);
    adj.get(e.source)?.push(e.target);
  }

  const level = new Map<string, number>(ids.map((id) => [id, 0]));
  const queue: string[] = [];
  for (const [id, deg] of inDegree) if (deg === 0) queue.push(id);
  const tempIn = new Map(inDegree);

  while (queue.length) {
    const cur = queue.shift()!;
    const curLv = level.get(cur) ?? 0;
    for (const nb of adj.get(cur) ?? []) {
      const next = (tempIn.get(nb) ?? 1) - 1;
      tempIn.set(nb, next);
      level.set(nb, Math.max(level.get(nb) ?? 0, curLv + 1));
      if (next === 0) queue.push(nb);
    }
  }

  const byLevel = new Map<number, string[]>();
  for (const [id, lv] of level) {
    if (!byLevel.has(lv)) byLevel.set(lv, []);
    byLevel.get(lv)!.push(id);
  }

  const positions = new Map<string, { x: number; y: number }>();
  for (const [lv, lvIds] of byLevel) {
    lvIds.forEach((id, row) => {
      positions.set(id, {
        x: lv * (NODE_W + COL_GAP),
        y: (row - (lvIds.length - 1) / 2) * (NODE_H + ROW_GAP),
      });
    });
  }
  return positions;
}

// ─── React Flow node ─────────────────────────────────────────────────────────

interface DotFlowNodeData extends Record<string, unknown> {
  label: string;
  shape: DotShape;
  onClick: () => void;
}

function DotNode({ data }: NodeProps<Node<DotFlowNodeData>>) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";

  const shapeStyles: React.CSSProperties =
    data.shape === "diamond"
      ? { transform: "rotate(45deg)", borderRadius: 4 }
      : data.shape === "circle"
        ? { borderRadius: "50%" }
        : data.shape === "round"
          ? { borderRadius: 24 }
          : { borderRadius: 8 };

  return (
    <>
      <Handle type="target" position={Position.Left} style={{ opacity: 0.4 }} />
      <Handle type="target" position={Position.Top} style={{ opacity: 0.4 }} />
      <Box
        component="button"
        type="button"
        onClick={data.onClick}
        sx={{
          width: NODE_W,
          minHeight: NODE_H,
          px: 1.25,
          py: 0.75,
          border: `1.5px solid ${isDark ? "#555" : "#ccc"}`,
          bgcolor: isDark
            ? alpha(theme.palette.background.paper, 0.85)
            : alpha(theme.palette.background.paper, 0.95),
          cursor: "pointer",
          textAlign: "center",
          font: "inherit",
          color: "inherit",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          transition: "box-shadow 120ms ease, transform 100ms ease, border-color 120ms",
          "&:hover": {
            boxShadow: `0 2px 8px ${alpha(theme.palette.primary.main, 0.3)}`,
            borderColor: theme.palette.primary.main,
            transform: "translateY(-1px)",
          },
          "&:active": { transform: "scale(0.97)" },
          ...shapeStyles,
        }}
      >
        <Typography
          sx={{
            fontWeight: 600,
            fontSize: 12,
            lineHeight: 1.35,
            color: "text.primary",
            pointerEvents: "none",
            ...(data.shape === "diamond" ? { transform: "rotate(-45deg)" } : {}),
          }}
        >
          {data.label}
        </Typography>
      </Box>
      <Handle type="source" position={Position.Right} style={{ opacity: 0.4 }} />
      <Handle type="source" position={Position.Bottom} style={{ opacity: 0.4 }} />
    </>
  );
}

const dotNodeTypes = { dotNode: DotNode };

// ─── Main component ───────────────────────────────────────────────────────────

interface DotFlowProps {
  dot: string;
  onNodeClick?: (text: string) => void;
}

export function DotFlow({ dot, onNodeClick }: DotFlowProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";

  const parsed = useMemo(() => parseDot(dot), [dot]);
  const nodeList = useMemo(() => Array.from(parsed.nodes.values()), [parsed.nodes]);
  const positions = useMemo(() => computeLayout(nodeList, parsed.edges), [nodeList, parsed.edges]);

  const maxRows = useMemo(() => {
    const byLevel = new Map<number, number>();
    for (const n of nodeList) {
      const pos = positions.get(n.id);
      if (!pos) continue;
      const lv = Math.round(pos.x / (NODE_W + COL_GAP));
      byLevel.set(lv, (byLevel.get(lv) ?? 0) + 1);
    }
    return Math.max(1, ...byLevel.values());
  }, [nodeList, positions]);

  const canvasH = Math.max(140, maxRows * (NODE_H + ROW_GAP) + 80);

  const initialNodes: Node<DotFlowNodeData>[] = useMemo(
    () =>
      nodeList.map((n) => ({
        id: n.id,
        type: "dotNode",
        position: positions.get(n.id) ?? { x: 0, y: 0 },
        data: {
          label: n.label,
          shape: n.shape,
          onClick: () => onNodeClick?.(n.label),
        },
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [nodeList, positions],
  );

  const initialEdges: Edge[] = useMemo(
    () =>
      parsed.edges.map((e, i) => ({
        id: `e-${i}`,
        source: e.source,
        target: e.target,
        label: e.label,
        animated: false,
        style: {
          stroke: isDark ? "#666" : "#bbb",
          strokeWidth: 1.5,
          strokeDasharray: e.dashed ? "5,3" : undefined,
        },
        labelStyle: { fontSize: 10, fill: isDark ? "#aaa" : "#666" },
        labelBgStyle: {
          fill: isDark ? theme.palette.background.paper : theme.palette.background.default,
          fillOpacity: 0.85,
        },
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [parsed.edges, isDark],
  );

  const [nodes, , onNodesChange] = useNodesState(initialNodes);
  const [edges, , onEdgesChange] = useEdgesState(initialEdges);
  const onNodeClickRF = useCallback(() => {}, []);

  if (nodeList.length === 0) {
    return (
      <Box sx={{ my: 1, p: 1.5, borderRadius: 1, border: 1, borderColor: "divider" }}>
        <Typography variant="caption" color="text.secondary">
          无法解析图结构
        </Typography>
      </Box>
    );
  }

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
          : alpha(theme.palette.background.default, 0.85),
      }}
    >
      <Box sx={{ width: "100%", height: canvasH }}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onNodeClick={onNodeClickRF}
          nodeTypes={dotNodeTypes}
          fitView
          fitViewOptions={{ padding: 0.25 }}
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
      {onNodeClick && (
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
      )}
    </Box>
  );
}
