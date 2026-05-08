/**
 * Shared dagre-based layout utility for MermaidFlow, DotFlow, and DagFlow.
 *
 * Dagre handles cycles correctly by internally reversing back-edges during
 * layout and then restoring them, so cyclic graphs (G→A) render cleanly.
 */
import dagre from "@dagrejs/dagre";
import type { Edge } from "@xyflow/react";
import { MarkerType } from "@xyflow/react";

export const GRAPH_NODE_W = 180;
export const GRAPH_NODE_H = 56;
export const GRAPH_FIXED_HEIGHT = 380;

export type GraphDirection = "TB" | "BT" | "LR" | "RL";

/** Map mermaid direction strings to dagre rankdir. */
export function toRankdir(dir: string): GraphDirection {
  switch (dir.toUpperCase()) {
    case "LR": return "LR";
    case "RL": return "RL";
    case "BT": return "BT";
    default:   return "TB"; // TD / TB
  }
}

export interface LayoutNode {
  id: string;
  width?: number;
  height?: number;
}

interface LayoutInput {
  nodes: LayoutNode[];
  edges: Array<{ source: string; target: string }>;
  direction?: GraphDirection;
  nodeW?: number;
  nodeH?: number;
}

interface LayoutOutput {
  positions: Map<string, { x: number; y: number }>;
}

export function computeDagreLayout({
  nodes,
  edges,
  direction = "TB",
  nodeW = GRAPH_NODE_W,
  nodeH = GRAPH_NODE_H,
}: LayoutInput): LayoutOutput {
  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({
    rankdir: direction,
    nodesep: 36,
    ranksep: 60,
    marginx: 20,
    marginy: 20,
  });

  for (const n of nodes) {
    const w = n.width ?? nodeW;
    const h = n.height ?? nodeH;
    g.setNode(n.id, { width: w, height: h });
  }
  for (const e of edges) {
    // dagre handles cycles internally — no need to filter them
    g.setEdge(e.source, e.target);
  }

  dagre.layout(g);

  const positions = new Map<string, { x: number; y: number }>();
  for (const n of nodes) {
    const w = n.width ?? nodeW;
    const h = n.height ?? nodeH;
    const { x, y } = g.node(n.id);
    // dagre returns center coordinates; React Flow uses top-left
    positions.set(n.id, { x: x - w / 2, y: y - h / 2 });
  }

  return { positions };
}

// ─── Per-node color ───────────────────────────────────────────────────────────

function hashStr(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  }
  return Math.abs(h);
}

/** Stable background color derived from a node's id, theme-aware. */
export function nodeIdBg(id: string, isDark: boolean): string {
  const hue = hashStr(id) % 360;
  return isDark
    ? `hsl(${hue}, 22%, 20%)`
    : `hsl(${hue}, 30%, 93%)`;
}

/** Matching border color for nodeIdBg. */
export function nodeIdBorder(id: string, isDark: boolean): string {
  const hue = hashStr(id) % 360;
  return isDark
    ? `hsl(${hue}, 28%, 38%)`
    : `hsl(${hue}, 35%, 72%)`;
}

/** Standard directed arrow marker for all graph edges. */
export const ARROW_MARKER = {
  type: MarkerType.ArrowClosed,
  width: 16,
  height: 16,
} as const;

/** Build React Flow edge from a parsed edge definition. */
export function buildFlowEdge(opts: {
  id: string;
  source: string;
  target: string;
  label?: string;
  dashed?: boolean;
  isDark: boolean;
  paperBg: string;
  defaultBg: string;
}): Edge {
  const { id, source, target, label, dashed, isDark, paperBg, defaultBg } = opts;
  return {
    id,
    source,
    target,
    label,
    animated: false,
    markerEnd: ARROW_MARKER,
    style: {
      stroke: isDark ? "#888" : "#999",
      strokeWidth: 1.5,
      strokeDasharray: dashed ? "5,3" : undefined,
    },
    labelStyle: { fontSize: 10, fill: isDark ? "#aaa" : "#666" },
    labelBgStyle: {
      fill: isDark ? paperBg : defaultBg,
      fillOpacity: 0.85,
    },
  };
}

/** Shared React Flow props to reduce boilerplate across all graph components. */
export const SHARED_RF_PROPS = {
  nodesDraggable: true,
  nodesConnectable: false,
  elementsSelectable: false,
  // Enable zoom — users can scroll inside the fixed-height canvas
  zoomOnScroll: true,
  panOnScroll: false,
  panOnDrag: true,
  preventScrolling: true,
  proOptions: { hideAttribution: true },
  fitView: true,
  fitViewOptions: { padding: 0.15 },
} as const;

/**
 * sx overrides for the inner Box that contains a ReactFlow instance.
 * Targets the Controls buttons so they respect the current MUI theme.
 */
export function graphInnerSx(isDark: boolean) {
  const btnBg     = isDark ? "#2a2a2a" : "#fff";
  const btnColor  = isDark ? "#ccc"    : "#333";
  const btnBorder = isDark ? "#444"    : "#ddd";
  const btnHover  = isDark ? "#3a3a3a" : "#f0f0f0";
  return {
    width: "100%",
    height: GRAPH_FIXED_HEIGHT,
    "& .react-flow__controls": {
      boxShadow: "none",
      border: `1px solid ${btnBorder}`,
      borderRadius: 1,
      overflow: "hidden",
    },
    "& .react-flow__controls-button": {
      background: btnBg,
      borderBottom: `1px solid ${btnBorder}`,
      color: btnColor,
      fill: btnColor,
      "&:hover": { background: btnHover },
      "& svg": { fill: btnColor },
    },
  } as const;
}

/** Shared ReactFlow wrapper props for the outer Box. */
export function graphBoxSx(isDark: boolean, paperAlpha: number, defaultAlpha: number) {
  return {
    my: 1.25,
    borderRadius: 2,
    border: 1,
    borderColor: "divider",
    overflow: "hidden",
    bgcolor: isDark
      ? `rgba(0,0,0,${paperAlpha})`
      : `rgba(255,255,255,${defaultAlpha})`,
  } as const;
}
