/**
 * MermaidFlow — parses Mermaid flowchart/graph syntax and renders it
 * with React Flow instead of the mermaid library's SVG output.
 *
 * Supported syntax:
 *   graph TD / flowchart LR  (direction: TD TB LR RL BT)
 *   A[label]  A(label)  A{label}  A((label))  A([label])  A>label]
 *   A --> B   A --- B   A -.-> B  A ==> B
 *   A -->|edge label| B
 *   subgraph (parsed, grouping ignored)
 *   %% comments
 *   style / classDef / class directives (skipped)
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

type Direction = "TD" | "TB" | "LR" | "RL" | "BT";
type NodeShape = "rect" | "round" | "diamond" | "circle" | "stadium" | "flag";

interface ParsedNode {
  id: string;
  label: string;
  shape: NodeShape;
}

interface ParsedEdge {
  source: string;
  target: string;
  label?: string;
  dashed?: boolean;
}

interface ParsedGraph {
  direction: Direction;
  nodes: Map<string, ParsedNode>;
  edges: ParsedEdge[];
}

function parseNodeDef(token: string): { id: string; label: string; shape: NodeShape } | null {
  // Try all bracket forms: id[label] id(label) id{label} id((label)) id([label]) id>label]
  const patterns: [RegExp, NodeShape][] = [
    [/^([\w-]+)\[\[(.+?)]]$/, "stadium"],
    [/^([\w-]+)\(\((.+?)\)\)$/, "circle"],
    [/^([\w-]+)\(\[(.+?)]$/, "stadium"],
    [/^([\w-]+)\[(.+?)]$/, "rect"],
    [/^([\w-]+)\((.+?)\)$/, "round"],
    [/^([\w-]+)\{(.+?)\}$/, "diamond"],
    [/^([\w-]+)>(.+?)]$/, "flag"],
  ];
  for (const [re, shape] of patterns) {
    const match = re.exec(token);
    if (match) return { id: match[1], label: match[2].trim(), shape };
  }
  // Plain id (no brackets)
  if (/^[\w\-]+$/.test(token)) return { id: token, label: token, shape: "rect" };
  return null;
}

/** Split an edge line like `A -->|label| B` into source/target/edgeLabel. */
function splitEdgeLine(line: string): { source: string; target: string; edgeLabel?: string; dashed: boolean } | null {
  // Supported: --> --- -.-> ==> --label-->
  // Supported: --> --- -.-> ==> --label-->
  const patterns = [
    // A -->|label| B
    /^(.+?)\s*-{1,2}\.?-{0,2}>?\|(.+?)\|\s*(.+)$/,
    // A -- label --> B
    /^(.+?)\s*--\s*(.+?)\s*-->\s*(.+)$/,
  ];

  for (const pat of patterns) {
    const m = pat.exec(line);
    if (m) {
      return {
        source: m[1].trim(),
        target: m[3].trim(),
        edgeLabel: m[2].trim() || undefined,
        dashed: line.includes("-."),
      };
    }
  }

  // Plain edges without labels: --> --- -.-> ==>
  const plain = /^(.+?)\s*(={2,}>|(?:\.?-{1,}\.?-{0,2}>)|(?:-{2,}))\s*(.+)$/.exec(line);
  if (plain) {
    return {
      source: plain[1].trim(),
      target: plain[3].trim(),
      dashed: plain[2].includes("."),
    };
  }
  return null;
}

/** Join lines where a bracket `[` is opened but not closed on the same line. */
function joinContinuationLines(source: string): string[] {
  const raw = source.split("\n");
  const joined: string[] = [];
  let buf = "";
  for (const line of raw) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (buf) { joined.push(buf); buf = ""; }
      continue;
    }
    buf = buf ? `${buf} ${trimmed}` : trimmed;
    // Count unmatched brackets in the accumulated buffer
    const opens = (buf.match(/\[/g) ?? []).length;
    const closes = (buf.match(/]/g) ?? []).length;
    if (opens <= closes) {
      joined.push(buf);
      buf = "";
    }
  }
  if (buf) joined.push(buf);
  return joined;
}

export function parseMermaid(source: string): ParsedGraph {
  const lines = joinContinuationLines(source);

  let direction: Direction = "TD";
  const nodeMap = new Map<string, ParsedNode>();
  const edges: ParsedEdge[] = [];

  const ensureNode = (id: string, label?: string, shape?: NodeShape) => {
    if (!nodeMap.has(id)) {
      nodeMap.set(id, { id, label: label ?? id, shape: shape ?? "rect" });
    } else if (label && label !== id) {
      // Update label if we get a richer definition
      const existing = nodeMap.get(id)!;
      nodeMap.set(id, { ...existing, label, shape: shape ?? existing.shape });
    }
  };

  for (const rawLine of lines) {
    // Strip comments
    const line = rawLine.replace(/%%.*$/, "").trim();
    if (!line) continue;

    // Direction declaration
    const dirMatch = /^(?:graph|flowchart)\s+(TD|TB|LR|RL|BT)\b/i.exec(line);
    if (dirMatch) {
      direction = dirMatch[1].toUpperCase() as Direction;
      continue;
    }

    // Skip subgraph / end / style / classDef / class / linkStyle
    if (/^(?:subgraph|end|style|classDef|class|linkStyle)\b/i.test(line)) continue;

    // Edge line — try to parse
    const edgeParsed = splitEdgeLine(line);
    if (edgeParsed) {
      // Source and target may themselves be node definitions
      const srcDef = parseNodeDef(edgeParsed.source);
      const tgtDef = parseNodeDef(edgeParsed.target);

      const srcId = srcDef?.id ?? edgeParsed.source;
      const tgtId = tgtDef?.id ?? edgeParsed.target;

      if (srcDef) ensureNode(srcDef.id, srcDef.label, srcDef.shape);
      else ensureNode(srcId);

      if (tgtDef) ensureNode(tgtDef.id, tgtDef.label, tgtDef.shape);
      else ensureNode(tgtId);

      edges.push({
        source: srcId,
        target: tgtId,
        label: edgeParsed.edgeLabel,
        dashed: edgeParsed.dashed,
      });
      continue;
    }

    // Standalone node definition
    const nodeDef = parseNodeDef(line);
    if (nodeDef) {
      ensureNode(nodeDef.id, nodeDef.label, nodeDef.shape);
    }
  }

  return { direction, nodes: nodeMap, edges };
}

// ─── Layout ───────────────────────────────────────────────────────────────────

const NODE_W = 160;
const NODE_H = 52;

function computeLayout(
  nodes: ParsedNode[],
  edges: ParsedEdge[],
  direction: Direction,
): Map<string, { x: number; y: number }> {
  const COL_GAP = direction === "LR" || direction === "RL" ? 70 : 56;
  const ROW_GAP = direction === "LR" || direction === "RL" ? 20 : 24;

  const ids = nodes.map((n) => n.id);
  const inDegree = new Map<string, number>(ids.map((id) => [id, 0]));
  const adj = new Map<string, string[]>(ids.map((id) => [id, []]));

  for (const e of edges) {
    if (inDegree.has(e.target)) inDegree.set(e.target, (inDegree.get(e.target) ?? 0) + 1);
    adj.get(e.source)?.push(e.target);
  }

  // BFS topological level assignment
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

  // Group by level
  const byLevel = new Map<number, string[]>();
  for (const [id, lv] of level) {
    if (!byLevel.has(lv)) byLevel.set(lv, []);
    byLevel.get(lv)!.push(id);
  }

  const positions = new Map<string, { x: number; y: number }>();
  const isHorizontal = direction === "LR" || direction === "RL";

  for (const [lv, lvIds] of byLevel) {
    const count = lvIds.length;
    lvIds.forEach((id, row) => {
      const offset = (row - (count - 1) / 2) * (NODE_H + ROW_GAP);
      if (isHorizontal) {
        const x = lv * (NODE_W + COL_GAP);
        positions.set(id, {
          x: direction === "RL" ? -x : x,
          y: offset,
        });
      } else {
        const y = lv * (NODE_H + COL_GAP);
        positions.set(id, {
          x: offset * (NODE_W / NODE_H), // scale x offset proportionally
          y: direction === "BT" ? -y : y,
        });
      }
    });
  }

  return positions;
}

// ─── React Flow node ─────────────────────────────────────────────────────────

interface FlowNodeData extends Record<string, unknown> {
  label: string;
  shape: NodeShape;
  onClick: () => void;
}

function shapeStyle(shape: NodeShape): React.CSSProperties {
  switch (shape) {
    case "diamond":
      return { transform: "rotate(45deg)", borderRadius: 4 };
    case "circle":
      return { borderRadius: "50%" };
    case "round":
      return { borderRadius: 24 };
    case "stadium":
      return { borderRadius: 24 };
    default:
      return { borderRadius: 8 };
  }
}

function MermaidNode({ data }: NodeProps<Node<FlowNodeData>>) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";
  const style = shapeStyle(data.shape);

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
          ...style,
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

const mermaidNodeTypes = { mermaidNode: MermaidNode };

// ─── Main component ───────────────────────────────────────────────────────────

interface MermaidFlowProps {
  source: string;
  onNodeClick?: (text: string) => void;
}

export function MermaidFlow({ source, onNodeClick }: MermaidFlowProps) {
  const theme = useTheme();
  const isDark = theme.palette.mode === "dark";

  const parsed = useMemo(() => parseMermaid(source), [source]);
  const nodeList = useMemo(() => Array.from(parsed.nodes.values()), [parsed.nodes]);
  const positions = useMemo(
    () => computeLayout(nodeList, parsed.edges, parsed.direction),
    [nodeList, parsed.edges, parsed.direction],
  );

  const isHorizontal = parsed.direction === "LR" || parsed.direction === "RL";

  // Canvas height: enough rows × node height
  const maxRows = useMemo(() => {
    const byLevel = new Map<number, number>();
    for (const n of nodeList) {
      const pos = positions.get(n.id);
      if (!pos) continue;
      const axis = isHorizontal ? pos.x : pos.y;
      const step = isHorizontal ? NODE_W + 70 : NODE_H + 56;
      const lv = Math.round(Math.abs(axis) / step);
      byLevel.set(lv, (byLevel.get(lv) ?? 0) + 1);
    }
    return Math.max(1, ...byLevel.values());
  }, [nodeList, positions, isHorizontal]);

  const canvasH = Math.max(140, maxRows * (NODE_H + 28) + 80);

  const initialNodes: Node<FlowNodeData>[] = useMemo(
    () =>
      nodeList.map((n) => ({
        id: n.id,
        type: "mermaidNode",
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
          无法解析流程图
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
          nodeTypes={mermaidNodeTypes}
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
