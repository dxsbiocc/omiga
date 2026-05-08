/**
 * Mermaid flowchart parser — extracts nodes, edges, subgraph groups.
 */

export type Direction = "TD" | "TB" | "LR" | "RL" | "BT";
export type NodeShape = "rect" | "round" | "diamond" | "circle" | "stadium" | "flag" | "card";

export interface ParsedNode {
  id: string;
  label: string;
  shape: NodeShape;
  group?: string;
}

export interface ParsedEdge {
  source: string;
  target: string;
  label?: string;
  dashed?: boolean;
}

export interface ParsedGraph {
  direction: Direction;
  nodes: Map<string, ParsedNode>;
  edges: ParsedEdge[];
  groups: Map<string, string[]>;
  groupTitles: Map<string, string>;
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
    buf = buf ? `${buf}\\n${trimmed}` : trimmed;
    const opens  = (buf.match(/\[/g) ?? []).length;
    const closes = (buf.match(/]/g)  ?? []).length;
    if (opens <= closes) { joined.push(buf); buf = ""; }
  }
  if (buf) joined.push(buf);
  return joined;
}

function parseNodeDef(token: string): { id: string; label: string; shape: NodeShape } | null {
  const patterns: [RegExp, NodeShape][] = [
    [/^([\w-]+)\[\[(.+?)]]$/s, "stadium"],
    [/^([\w-]+)\(\((.+?)\)\)$/s, "circle"],
    [/^([\w-]+)\(\[(.+?)]$/s, "stadium"],
    [/^([\w-]+)\[(.+?)]$/s, "rect"],
    [/^([\w-]+)\((.+?)\)$/s, "round"],
    [/^([\w-]+)\{(.+?)\}$/s, "diamond"],
    [/^([\w-]+)>(.+?)]$/s, "flag"],
  ];
  for (const [re, shape] of patterns) {
    const m = re.exec(token);
    if (m) return { id: m[1], label: m[2].trim(), shape };
  }
  if (/^[\w-]+$/.test(token)) return { id: token, label: token, shape: "rect" };
  return null;
}

function splitEdgeLine(line: string): { source: string; target: string; edgeLabel?: string; dashed: boolean } | null {
  const withLabel = [
    /^(.+?)\s*-{1,2}\.?-{0,2}>?\|(.+?)\|\s*(.+)$/,
    /^(.+?)\s*--\s*(.+?)\s*-->\s*(.+)$/,
  ];
  for (const pat of withLabel) {
    const m = pat.exec(line);
    if (m) return { source: m[1].trim(), target: m[3].trim(), edgeLabel: m[2].trim() || undefined, dashed: line.includes("-.") };
  }
  const plain = /^(.+?)\s*(={2,}>|(?:\.?-{1,}\.?-{0,2}>)|(?:-{2,}))\s*(.+)$/.exec(line);
  if (plain) return { source: plain[1].trim(), target: plain[3].trim(), dashed: plain[2].includes(".") };
  return null;
}

export function groupColor(name: string): { bg: string; border: string; text: string } {
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

/** Split label by <br> tags or literal \n into lines. */
export function splitLabel(label: string): string[] {
  return label
    .split(/\s*(?:<br\s*\/?>|\\n)\s*/i)
    .map((l) => l.trim())
    .filter(Boolean);
}

/** Estimate card height from number of text lines. */
export function cardHeight(lineCount: number): number {
  const headerH = 28;
  const lineH = 18;
  const padY = 14;
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
