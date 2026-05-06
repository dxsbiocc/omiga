const INACTIVE_FILE_MENTION = {
  active: false as const,
  kind: "mixed" as const,
  prefix: "" as const,
  query: "",
  directory: "",
  filter: "",
};

export interface ComposerFileMentionParse {
  active: boolean;
  /** Mention namespace. `@` is for workspace files, `#` is for Omiga plugins. */
  kind: "mixed" | "file" | "plugin";
  /** The normalized explicit prefix without trigger/`:`, empty for bare `@`. */
  prefix: "" | "file" | "plugin";
  /** Query after the trigger, normalized to `/` separators for file paths. */
  query: string;
  /** Workspace-relative directory currently being listed. Empty means workspace root. */
  directory: string;
  /** Basename filter within `directory`. */
  filter: string;
}

export interface ComposerMentionRow {
  path: string;
  is_file: boolean;
  size: number;
}

export function normalizeComposerMentionQuery(query: string): string {
  return query
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/\/{2,}/g, "/");
}

export function normalizeComposerMentionPath(path: string): string {
  return normalizeComposerMentionQuery(path.replace(/^@/, "")).replace(
    /\/+$/,
    "",
  );
}

export function parseComposerFileMentionInput(
  input: string,
): ComposerFileMentionParse {
  if (/^#[^\s]*$/.test(input)) {
    const raw = input.slice(1);
    const query = normalizeComposerMentionQuery(raw);
    return {
      active: true,
      kind: "plugin",
      prefix: "plugin",
      query,
      directory: "",
      filter: query,
    };
  }

  if (!/^@[^\s]*$/.test(input)) return INACTIVE_FILE_MENTION;
  const raw = input.slice(1);
  const lower = raw.toLowerCase();
  let kind: ComposerFileMentionParse["kind"] = "file";
  let prefix: ComposerFileMentionParse["prefix"] = "";
  let rawQuery = raw;
  if (lower.startsWith("file:")) {
    kind = "file";
    prefix = "file";
    rawQuery = raw.slice("file:".length);
  }
  const query = normalizeComposerMentionQuery(rawQuery);
  const slashIndex = query.lastIndexOf("/");
  if (slashIndex < 0) {
    return {
      active: true,
      kind,
      prefix,
      query,
      directory: "",
      filter: query,
    };
  }
  return {
    active: true,
    kind,
    prefix,
    query,
    directory: normalizeComposerMentionPath(query.slice(0, slashIndex)),
    filter: query.slice(slashIndex + 1),
  };
}

export function parentComposerMentionDirectory(directory: string): string {
  const safe = normalizeComposerMentionPath(directory);
  if (!safe) return "";
  const parts = safe.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

export function joinWorkspaceMentionDirectory(
  workspacePath: string,
  relativeDirectory: string,
): string {
  const root = workspacePath.trim().replace(/\\/g, "/");
  const dir = normalizeComposerMentionPath(relativeDirectory);
  if (!dir) return root;
  if (!root) return dir;
  const trimmedRoot = root === "/" ? "/" : root.replace(/\/+$/, "");
  return trimmedRoot === "/" ? `/${dir}` : `${trimmedRoot}/${dir}`;
}

export function buildComposerMentionChildPath(
  directory: string,
  entryName: string,
): string {
  const safeDirectory = normalizeComposerMentionPath(directory);
  const safeName = normalizeComposerMentionPath(entryName);
  if (!safeName) return safeDirectory;
  return safeDirectory ? `${safeDirectory}/${safeName}` : safeName;
}

export function sortComposerMentionRows<T extends ComposerMentionRow>(
  rows: T[],
): T[] {
  return [...rows].sort((a, b) => {
    if (a.is_file !== b.is_file) return a.is_file ? 1 : -1;
    return a.path.localeCompare(b.path);
  });
}

export function filterComposerMentionRows<T extends ComposerMentionRow>(
  rows: T[],
  filter: string,
  limit = 200,
): T[] {
  const q = filter.toLowerCase().trim();
  const filtered = q
    ? rows.filter((row) => {
        const path = normalizeComposerMentionPath(row.path).toLowerCase();
        const pathSegments = path.split("/").filter(Boolean);
        const basename = pathSegments[pathSegments.length - 1] ?? path;
        return (
          basename.startsWith(q) || basename.includes(q) || path.includes(q)
        );
      })
    : rows;
  return filtered.slice(0, limit);
}

function normalizedComposerPaths(paths: string[]): string[] {
  return paths
    .map(normalizeComposerMentionPath)
    .filter((path, index, arr) => path && arr.indexOf(path) === index);
}

export function formatComposerPathPreview(paths: string[]): string {
  return normalizedComposerPaths(paths)
    .map((path) => `@${path}`)
    .join(" ");
}

export function buildComposerPathInjection(paths: string[]): string {
  const normalized = normalizedComposerPaths(paths);
  if (normalized.length === 0) return "";
  return [
    "<omiga-selected-paths>",
    "The user selected these workspace-relative paths with the @ picker. Use these exact path strings when reading or editing files; do not infer or guess alternatives:",
    ...normalized.map((path) => `- ${path}`),
    "</omiga-selected-paths>",
  ].join("\n");
}

/** Build payload text with exact @ picker paths injected for the model. */
export function mergeComposerPathsAndBody(
  paths: string[],
  body: string,
): string {
  const pathBlock = buildComposerPathInjection(paths);
  if (pathBlock && body) return `${pathBlock}\n\n${body}`;
  return pathBlock || body;
}

function legacyComposerPathLine(paths: string[]): string {
  return formatComposerPathPreview(paths);
}

interface LeadingPathPrefixParse {
  paths: string[];
  body: string;
  hasPathPrefix: boolean;
}

function parseLeadingPathInjection(full: string): LeadingPathPrefixParse | null {
  const open = "<omiga-selected-paths>";
  const close = "</omiga-selected-paths>";
  if (!full.startsWith(open)) return null;

  const closeIndex = full.indexOf(close, open.length);
  if (closeIndex < 0) return null;

  const block = full.slice(open.length, closeIndex);
  const paths = normalizedComposerPaths(
    block
      .split(/\r?\n/u)
      .map((line) => line.match(/^\s*-\s+(.+?)\s*$/u)?.[1] ?? ""),
  );
  const body = full
    .slice(closeIndex + close.length)
    .replace(/^(?:\r?\n)+/u, "");

  return {
    paths,
    body,
    hasPathPrefix: true,
  };
}

/**
 * Split the model-facing selected-path prefix from user-visible text.
 *
 * New messages should carry `composerAttachedPaths` as structured metadata, but
 * older persisted rows may only contain the injected XML-like block.  This
 * helper keeps those rows friendly in the UI without making the backend parse
 * arbitrary prose.
 */
export function splitLeadingPathPrefixFromMerged(
  full: string,
  paths: string[] = [],
): LeadingPathPrefixParse {
  const normalizedPaths = normalizedComposerPaths(paths);
  const prefixes = normalizedPaths.length > 0
    ? [
        buildComposerPathInjection(normalizedPaths),
        legacyComposerPathLine(normalizedPaths),
      ].filter(Boolean)
    : [];

  for (const prefix of prefixes) {
    if (full.startsWith(`${prefix}\n\n`)) {
      return {
        paths: normalizedPaths,
        body: full.slice(prefix.length + 2),
        hasPathPrefix: true,
      };
    }
    if (full.trim() === prefix) {
      return {
        paths: normalizedPaths,
        body: "",
        hasPathPrefix: true,
      };
    }
  }

  const parsedInjection = parseLeadingPathInjection(full);
  if (parsedInjection) return parsedInjection;

  return {
    paths: normalizedPaths,
    body: full,
    hasPathPrefix: false,
  };
}

/** Remove the hidden model-facing path injection before showing/editing body text. */
export function stripLeadingPathPrefixFromMerged(
  full: string,
  paths: string[],
): string {
  return splitLeadingPathPrefixFromMerged(full, paths).body;
}

/** Whether merged content still carries the selected path prefix for this path set. */
export function pathsStillMatchMergedContent(
  paths: string[],
  content: string,
): boolean {
  if (paths.length === 0) return true;
  const prefixes = [
    buildComposerPathInjection(paths),
    legacyComposerPathLine(paths),
  ].filter(Boolean);
  return prefixes.some(
    (prefix) =>
      content.startsWith(`${prefix}\n\n`) || content.trim() === prefix,
  );
}
