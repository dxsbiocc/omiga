export const WEB_SEARCH_KEYS_STORAGE = "omiga_web_search_api_keys";

const DEFAULT_PUBMED_EMAIL = "omiga@example.invalid";
const DEFAULT_PUBMED_TOOL_NAME = "omiga";

const QUERY_DATASET_TYPE_IDS = [
  "expression",
  "sequencing",
  "genomics",
  "sample_metadata",
  "multi_omics",
] as const;
const QUERY_DATASET_SOURCE_IDS = [
  "geo",
  "ena",
  "cbioportal",
  "gtex",
  "arrayexpress",
  "biosample",
] as const;
const QUERY_KNOWLEDGE_SOURCE_IDS = ["ncbi_gene", "ensembl", "uniprot"] as const;

const DEFAULT_QUERY_DATASET_TYPES = [
  "expression",
  "sequencing",
  "genomics",
  "sample_metadata",
];
const DEFAULT_QUERY_DATASET_SOURCES = ["geo", "ena"];
const DEFAULT_QUERY_KNOWLEDGE_SOURCES = ["ncbi_gene"];

export interface StoredWebSearchPayload {
  tavily: string;
  exa: string;
  parallel: string;
  firecrawl: string;
  firecrawlUrl: string;
  semanticScholarEnabled: boolean;
  semanticScholarApiKey: string;
  wechatSearchEnabled: boolean;
  pubmedApiKey: string;
  pubmedEmail: string;
  pubmedToolName: string;
  queryDatasetTypes: string[];
  queryDatasetSources: string[];
  queryKnowledgeSources: string[];
}

function parseSettingBool(value: unknown, fallback: boolean): boolean {
  if (typeof value === "boolean") return value;
  if (typeof value === "string") {
    if (value.toLowerCase() === "true") return true;
    if (value.toLowerCase() === "false") return false;
  }
  return fallback;
}

function normalizeQuerySelection(
  raw: unknown,
  allowed: readonly string[],
  fallback: string[],
): string[] {
  if (!Array.isArray(raw)) return [...fallback];
  const selected = new Set(
    raw
      .map((value) => String(value).trim().toLowerCase().replace(/[-\s]+/gu, "_"))
      .filter(Boolean),
  );
  return allowed.filter((id) => selected.has(id));
}

export function defaultWebSearchQuerySettings(): Pick<
  StoredWebSearchPayload,
  "queryDatasetTypes" | "queryDatasetSources" | "queryKnowledgeSources"
> {
  return {
    queryDatasetTypes: [...DEFAULT_QUERY_DATASET_TYPES],
    queryDatasetSources: [...DEFAULT_QUERY_DATASET_SOURCES],
    queryKnowledgeSources: [...DEFAULT_QUERY_KNOWLEDGE_SOURCES],
  };
}

export function parseStoredWebSearchSettings(raw: string): StoredWebSearchPayload | null {
  try {
    const j = JSON.parse(raw) as Record<string, unknown>;
    return {
      tavily: String(j.tavily ?? "").trim(),
      exa: String(j.exa ?? "").trim(),
      parallel: String(j.parallel ?? "").trim(),
      firecrawl: String(j.firecrawl ?? "").trim(),
      firecrawlUrl: String(j.firecrawlUrl ?? "").trim(),
      semanticScholarEnabled: parseSettingBool(j.semanticScholarEnabled, false),
      semanticScholarApiKey: String(j.semanticScholarApiKey ?? "").trim(),
      wechatSearchEnabled: parseSettingBool(j.wechatSearchEnabled, false),
      pubmedApiKey: String(j.pubmedApiKey ?? "").trim(),
      pubmedEmail: String(j.pubmedEmail ?? DEFAULT_PUBMED_EMAIL).trim(),
      pubmedToolName: String(j.pubmedToolName ?? DEFAULT_PUBMED_TOOL_NAME).trim(),
      queryDatasetTypes: normalizeQuerySelection(
        j.queryDatasetTypes,
        QUERY_DATASET_TYPE_IDS,
        DEFAULT_QUERY_DATASET_TYPES,
      ),
      queryDatasetSources: normalizeQuerySelection(
        j.queryDatasetSources,
        QUERY_DATASET_SOURCE_IDS,
        DEFAULT_QUERY_DATASET_SOURCES,
      ),
      queryKnowledgeSources: normalizeQuerySelection(
        j.queryKnowledgeSources,
        QUERY_KNOWLEDGE_SOURCE_IDS,
        DEFAULT_QUERY_KNOWLEDGE_SOURCES,
      ),
    };
  } catch {
    return null;
  }
}
