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

// ─── Parser ───────────────────────────────────────────────────────────────────

type Direction = "TD" | "TB" | "LR" | "RL" | "BT";
type NodeShape = "rect" | "round" | "diamond" | "circle" | "stadium" | "flag" | "card";

interface ParsedNode {
  id: string;
  label: string;
  shape: NodeShape;
  group?: string;
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
  groups: Map<string, string[]>;      // groupName -> nodeIds
  groupTitles: Map<string, string>;   // groupName -> display title
}

/** Join lines where a `[` bracket is opened but not yet closed. */
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
    const opens  = (buf.match(/\[/g) ?? []).length;
    const closes = (buf.match(/]/g)  ?? []).length;
    if (opens <= closes) { joined.push(buf); buf = ""; }
  }
  if (buf) joined.push(buf);
  return joined;
}

function parseNodeDef(token: string): { id: string; label: string; shape: NodeShape } | null {
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
    const m = re.exec(token);
    if (m) return { id: m[1], label: m[2].trim(), shape };
  }
  if (/^[\w-]+$/.test(token)) return { id: token, label: token, shape: "rect" };
  return null;
}

function splitEdgeLine(line: string): { source: string; target: string; edgeLabel?: string; dashed: boolean } | null {
  // With label: A -->|label| B  or  A -- label --> B
  const withLabel = [
    /^(.+?)\s*-{1,2}\.?-{0,2}>?\|(.+?)\|\s*(.+)$/,
    /^(.+?)\s*--\s*(.+?)\s*-->\s*(.+)$/,
  ];
  for (const pat of withLabel) {
    const m = pat.exec(line);
    if (m) return { source: m[1].trim(), target: m[3].trim(), edgeLabel: m[2].trim() || undefined, dashed: line.includes("-.") };
  }
  // Plain: --> --- -.-> ==>
  const plain = /^(.+?)\s*(={2,}>|(?:\.?-{1,}\.?-{0,2}>)|(?:-{2,}))\s*(.+)$/.exec(line);
  if (plain) return { source: plain[1].trim(), target: plain[3].trim(), dashed: plain[2].includes(".") };
  return null;
}

function groupColor(name: string): { bg: string; border: string; text: string } {
  // Stable hash-based hue
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (Math.imul(31, h) + name.charCodeAt(i)) | 0;
  }
  const hue = Math.abs(h) % 360;
  return {
    bg: `hsl(${hue}, 55%, 22%)`,
    border: `hsl(${hue}, 60%, 45%)`,
    text: `hsl(${hue}, 20%, 95%)`,
  };
}

/** Split label by <br> tags or literal \\n into lines. */
function splitLabel(label: string): string[] {
  return label
    .split(/\s*(?:<br\s*\/?>|\\n)\s*/i)
    .map((l) => l.trim())
    .filter(Boolean);
}

/** Estimate card height from number of text lines. */
function cardHeight(lineCount: number): number {
  const headerH = 28;      // title line
  const lineH = 18;        // each body line
  const padY = 14;         // total vertical padding
  return headerH + lineCount * lineH + padY;
}

export function parseMermaid(source: string): ParsedGraph {
  const lines = joinContinuationLines(source);
  let direction: Direction = "TD";
  const nodeMap = new Map<string, ParsedNode>();
  const edges: ParsedEdge[] = [];
  const groups = new Map<string, string[]>();
  const groupTitles = new Map<string, string>();
  let currentGroup: string | undefined;

  const ensureNode = (id: string, label?: string, shape?: NodeShape) => {
    const hasMultiline = label ? /(<br\s*\/?>|\\n)/i.test(label) : false;
    const resolvedShape: NodeShape = shape ?? (hasMultiline ? "card" : "rect");
    if (!nodeMap.has(id)) {
      nodeMap.set(id, {
        id,
        label: label ?? id,
        shape: resolvedShape,
        group: currentGroup,
      });
      if (currentGroup) {
        groups.set(currentGroup, [...(groups.get(currentGroup) ?? []), id]);
      }
    } else if (label && label !== id) {
      const ex = nodeMap.get(id)!;
      nodeMap.set(id, {
        ...ex,
        label,
        shape: shape ?? (hasMultiline ? "card" : ex.shape),
        group: ex.group ?? currentGroup,
      });
      if (currentGroup && !groups.get(currentGroup)?.includes(id)) {
        groups.set(currentGroup, [...(groups.get(currentGroup) ?? []), id]);
      }
    }
  };

  for (const rawLine of lines) {
    const line = rawLine.replace(/%%.*$/, "").trim();
    if (!line) continue;

    const dirMatch = /^(?:graph|flowchart)\s+(TD|TB|LR|RL|BT)\b/i.exec(line);
    if (dirMatch) { direction = dirMatch[1].toUpperCase() as Direction; continue; }

    // Subgraph start
    const subMatch = /^subgraph\s+([\w-]+)(?:\s*\[\s*(.+?)\s*\])?\s*$/i.exec(line);
    if (subMatch) {
      currentGroup = subMatch[1];
      groupTitles.set(currentGroup, subMatch[2]?.trim() ?? currentGroup);
      if (!groups.has(currentGroup)) groups.set(currentGroup, []);
      continue;
    }

    if (/^end\b/i.test(line)) {
      currentGroup = undefined;
      continue;
    }

    if (/^(?:style|classDef|class|linkStyle)\b/i.test(line)) continue;

    const ep = splitEdgeLine(line);
    if (ep) {
      const srcDef = parseNodeDef(ep.source);
      const tgtDef = parseNodeDef(ep.target);
      const srcId = srcDef?.id ?? ep.source;
      const tgtId = tgtDef?.id ?? ep.target;
      if (srcDef) ensureNode(srcDef.id, srcDef.label, srcDef.shape); else ensureNode(srcId);
      if (tgtDef) ensureNode(tgtDef.id, tgtDef.label, tgtDef.shape); else ensureNode(tgtId);
      edges.push({ source: srcId, target: tgtId, label: ep.edgeLabel, dashed: ep.dashed });
      continue;
    }

    const nd = parseNodeDef(line);
    if (nd) ensureNode(nd.id, nd.label, nd.shape);
  }

  return { direction, nodes: nodeMap, edges, groups, groupTitles };
}

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
