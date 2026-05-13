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
  "ncbi_datasets",
  "arrayexpress",
  "biosample",
] as const;
const QUERY_KNOWLEDGE_SOURCE_IDS = [
  "ncbi_gene",
  "ensembl",
  "uniprot",
  "reactome",
  "gene_ontology",
  "msigdb",
  "kegg",
] as const;

const DEFAULT_QUERY_DATASET_TYPES = [
  "expression",
  "sequencing",
  "genomics",
  "sample_metadata",
];
const DEFAULT_QUERY_DATASET_SOURCES = ["geo", "ena"];
const DEFAULT_QUERY_KNOWLEDGE_SOURCES = ["ncbi_gene"];

const CATEGORY_SOURCE_IDS: Record<string, readonly string[]> = {
  literature: [
    "pubmed",
    "arxiv",
    "crossref",
    "openalex",
    "biorxiv",
    "medrxiv",
    "semantic_scholar",
  ],
  dataset: [
    "geo",
    "ena",
    "cbioportal",
    "gtex",
    "ncbi_datasets",
    "arrayexpress",
    "biosample",
  ],
  knowledge: [
    "project_wiki",
    "session_memory",
    "long_term",
    "sources",
    "ncbi_gene",
    "ensembl",
    "uniprot",
    "reactome",
    "gene_ontology",
    "msigdb",
    "kegg",
  ],
  web: ["tavily", "ddg", "google", "bing", "exa", "firecrawl", "parallel"],
  social: ["wechat"],
};

const CATEGORY_SUBCATEGORY_IDS: Record<string, readonly string[]> = {
  literature: ["paper", "preprint"],
  dataset: QUERY_DATASET_TYPE_IDS,
  knowledge: [
    "local",
    "gene",
    "protein",
    "pathway",
    "disease",
    "variant",
    "drug",
    "interaction",
  ],
  web: ["web_page"],
  social: ["public_account"],
};

const DEFAULT_ENABLED_SOURCES_BY_CATEGORY: Record<string, string[]> = {
  literature: ["pubmed", "arxiv", "crossref", "openalex", "biorxiv", "medrxiv"],
  dataset: [...DEFAULT_QUERY_DATASET_SOURCES],
  knowledge: ["project_wiki", "session_memory", "long_term", "sources", "ncbi_gene"],
  web: ["ddg", "google", "bing"],
  social: [],
};

const DEFAULT_ENABLED_SUBCATEGORIES_BY_CATEGORY: Record<string, string[]> = {
  literature: ["paper", "preprint"],
  dataset: [...DEFAULT_QUERY_DATASET_TYPES],
  knowledge: ["local", "gene"],
  web: ["web_page"],
  social: [],
};

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
  enabledSourcesByCategory: Record<string, string[]>;
  enabledSubcategoriesByCategory: Record<string, string[]>;
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

function normalizeCategorySelectionMap(
  raw: unknown,
  allowedByCategory: Record<string, readonly string[]>,
  fallbackByCategory: Record<string, string[]>,
): Record<string, string[]> {
  if (!raw || typeof raw !== "object") {
    return Object.fromEntries(
      Object.entries(fallbackByCategory).map(([key, value]) => [key, [...value]]),
    );
  }
  const input = raw as Record<string, unknown>;
  const out: Record<string, string[]> = {};
  for (const [category, allowed] of Object.entries(allowedByCategory)) {
    out[category] = normalizeQuerySelection(
      input[category],
      allowed,
      fallbackByCategory[category] ?? [],
    );
  }
  return out;
}

export function defaultWebSearchQuerySettings(): Pick<
  StoredWebSearchPayload,
  | "queryDatasetTypes"
  | "queryDatasetSources"
  | "queryKnowledgeSources"
  | "enabledSourcesByCategory"
  | "enabledSubcategoriesByCategory"
> {
  return {
    queryDatasetTypes: [...DEFAULT_QUERY_DATASET_TYPES],
    queryDatasetSources: [...DEFAULT_QUERY_DATASET_SOURCES],
    queryKnowledgeSources: [...DEFAULT_QUERY_KNOWLEDGE_SOURCES],
    enabledSourcesByCategory: normalizeCategorySelectionMap(
      null,
      CATEGORY_SOURCE_IDS,
      DEFAULT_ENABLED_SOURCES_BY_CATEGORY,
    ),
    enabledSubcategoriesByCategory: normalizeCategorySelectionMap(
      null,
      CATEGORY_SUBCATEGORY_IDS,
      DEFAULT_ENABLED_SUBCATEGORIES_BY_CATEGORY,
    ),
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
      enabledSourcesByCategory: normalizeCategorySelectionMap(
        j.enabledSourcesByCategory,
        CATEGORY_SOURCE_IDS,
        DEFAULT_ENABLED_SOURCES_BY_CATEGORY,
      ),
      enabledSubcategoriesByCategory: normalizeCategorySelectionMap(
        j.enabledSubcategoriesByCategory,
        CATEGORY_SUBCATEGORY_IDS,
        DEFAULT_ENABLED_SUBCATEGORIES_BY_CATEGORY,
      ),
    };
  } catch {
    return null;
  }
}
