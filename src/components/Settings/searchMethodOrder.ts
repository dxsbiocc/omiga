export type WebSearchEngine = "ddg" | "bing" | "google";

export type WebSearchMethod =
  | "tavily"
  | "exa"
  | "firecrawl"
  | "parallel"
  | "google"
  | "bing"
  | "ddg";

export const DEFAULT_WEB_SEARCH_METHODS: WebSearchMethod[] = [
  "ddg",
  "google",
  "bing",
];

const LEGACY_ALL_PROVIDER_DEFAULT: WebSearchMethod[] = [
  "tavily",
  "exa",
  "firecrawl",
  "parallel",
  "google",
  "bing",
  "ddg",
];

export function normalizeWebSearchMethod(raw: unknown): WebSearchMethod | null {
  const value = String(raw ?? "").trim().toLowerCase();
  if (value === "tavily") return "tavily";
  if (value === "exa") return "exa";
  if (value === "firecrawl") return "firecrawl";
  if (value === "parallel") return "parallel";
  if (value === "google") return "google";
  if (value === "bing") return "bing";
  if (value === "duckduckgo" || value === "duck-duck-go" || value === "ddg") {
    return "ddg";
  }
  return null;
}

export function normalizeWebSearchMethods(raw: unknown): WebSearchMethod[] {
  if (!Array.isArray(raw)) return [...DEFAULT_WEB_SEARCH_METHODS];
  const out: WebSearchMethod[] = [];
  for (const item of raw) {
    const method = normalizeWebSearchMethod(item);
    if (method && !out.includes(method)) out.push(method);
  }
  if (out.length === 0) return [...DEFAULT_WEB_SEARCH_METHODS];
  if (
    out.length === LEGACY_ALL_PROVIDER_DEFAULT.length &&
    out.every((method, index) => method === LEGACY_ALL_PROVIDER_DEFAULT[index])
  ) {
    return [...DEFAULT_WEB_SEARCH_METHODS];
  }
  return out;
}

export function primaryPublicSearchEngine(
  methods: WebSearchMethod[],
  fallback: WebSearchEngine = "ddg",
): WebSearchEngine {
  return (
    methods.find(
      (method): method is WebSearchEngine =>
        method === "google" || method === "bing" || method === "ddg",
    ) ?? fallback
  );
}

export function normalizeWebSearchEngine(raw: unknown): WebSearchEngine {
  const value = String(raw ?? "").trim().toLowerCase();
  if (value === "google") return "google";
  if (value === "bing") return "bing";
  if (value === "duckduckgo" || value === "duck-duck-go" || value === "ddg") {
    return "ddg";
  }
  return "ddg";
}

export function moveItemToIndex<T>(items: readonly T[], item: T, targetIndex: number): T[] {
  const fromIndex = items.indexOf(item);
  if (fromIndex < 0 || items.length === 0) return [...items];

  const boundedTarget = Math.min(Math.max(0, targetIndex), items.length - 1);
  if (fromIndex === boundedTarget) return [...items];

  const next = [...items];
  const [moved] = next.splice(fromIndex, 1);
  next.splice(boundedTarget, 0, moved);
  return next;
}
