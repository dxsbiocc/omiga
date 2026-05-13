//! Built-in retrieval source registry.
//!
//! This registry is the single backend source of truth for retrieval categories,
//! subcategories, source metadata, defaults, and UI discovery. Runtime adapters
//! still live in `domain::search` / `domain::tools`; the registry intentionally
//! stays declarative so new sources can be added one at a time.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalSourceStatus {
    Available,
    RequiresApiKey,
    OptIn,
    Planned,
    Extension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalCapability {
    Search,
    Fetch,
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalAutoStrategy {
    Fallback,
    Merge,
    RankedMerge,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalOrigin {
    Builtin,
    Extension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalParameterType {
    String,
    Boolean,
    Integer,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalCategoryDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalSubcategoryDefinition {
    pub id: &'static str,
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub default_enabled: bool,
    pub available: bool,
    pub status: RetrievalSourceStatus,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalParameterDefinition {
    pub name: &'static str,
    pub param_type: RetrievalParameterType,
    pub description: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalSourceDefinition {
    pub id: &'static str,
    pub category: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub aliases: &'static [&'static str],
    pub subcategories: &'static [&'static str],
    pub capabilities: &'static [RetrievalCapability],
    pub status: RetrievalSourceStatus,
    pub origin: RetrievalOrigin,
    pub available: bool,
    pub default_enabled: bool,
    pub requires_api_key: bool,
    pub requires_opt_in: bool,
    pub required_credential_refs: &'static [&'static str],
    pub optional_credential_refs: &'static [&'static str],
    pub priority: i32,
    pub auto_strategy: RetrievalAutoStrategy,
    pub parameters: &'static [RetrievalParameterDefinition],
    pub risk_level: RetrievalRiskLevel,
    pub risk_notes: &'static [&'static str],
    pub homepage_url: Option<&'static str>,
    pub docs_url: Option<&'static str>,
}

impl RetrievalSourceDefinition {
    pub fn supports(&self, capability: RetrievalCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn can_execute(&self) -> bool {
        matches!(
            self.status,
            RetrievalSourceStatus::Available
                | RetrievalSourceStatus::RequiresApiKey
                | RetrievalSourceStatus::OptIn
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalSourceRegistry {
    pub categories: Vec<RetrievalCategoryDefinition>,
    pub subcategories: Vec<RetrievalSubcategoryDefinition>,
    pub sources: Vec<RetrievalSourceDefinition>,
}

pub fn registry() -> RetrievalSourceRegistry {
    RetrievalSourceRegistry {
        categories: categories(),
        subcategories: subcategories(),
        sources: sources(),
    }
}

pub fn category_ids() -> Vec<&'static str> {
    categories().into_iter().map(|item| item.id).collect()
}

pub fn subcategory_ids(category: &str) -> Vec<&'static str> {
    let category = normalize_id(category);
    subcategories()
        .into_iter()
        .filter(|item| item.category == category)
        .map(|item| item.id)
        .collect()
}

pub fn source_ids(category: &str) -> Vec<&'static str> {
    let category = normalize_id(category);
    sources()
        .into_iter()
        .filter(|item| item.category == category)
        .map(|item| item.id)
        .collect()
}

pub fn default_subcategory_ids(category: &str) -> Vec<&'static str> {
    let category = normalize_id(category);
    subcategories()
        .into_iter()
        .filter(|item| item.category == category && item.default_enabled && item.available)
        .map(|item| item.id)
        .collect()
}

pub fn default_source_ids(category: &str) -> Vec<&'static str> {
    let category = normalize_id(category);
    sources()
        .into_iter()
        .filter(|item| item.category == category && item.default_enabled && item.can_execute())
        .map(|item| item.id)
        .collect()
}

pub fn canonical_source_id(category: &str, source: &str) -> Option<&'static str> {
    let category = normalize_id(category);
    let source = normalize_id(source);
    sources()
        .into_iter()
        .find(|item| {
            item.category == category
                && (item.id == source
                    || item
                        .aliases
                        .iter()
                        .any(|alias| normalize_id(alias) == source))
        })
        .map(|item| item.id)
}

pub fn find_source(category: &str, source: &str) -> Option<RetrievalSourceDefinition> {
    let category = normalize_id(category);
    let source = normalize_id(source);
    sources().into_iter().find(|item| {
        item.category == category
            && (item.id == source
                || item
                    .aliases
                    .iter()
                    .any(|alias| normalize_id(alias) == source))
    })
}

pub fn normalize_enabled_ids(
    category: &str,
    values: &[String],
    kind: RegistryEntryKind,
    include_unavailable: bool,
) -> Vec<String> {
    let allowed = match kind {
        RegistryEntryKind::Source => source_ids(category),
        RegistryEntryKind::Subcategory => subcategory_ids(category),
    };
    let available: HashSet<&'static str> = match kind {
        RegistryEntryKind::Source => sources()
            .into_iter()
            .filter(|item| item.category == normalize_id(category) && item.can_execute())
            .map(|item| item.id)
            .collect(),
        RegistryEntryKind::Subcategory => subcategories()
            .into_iter()
            .filter(|item| item.category == normalize_id(category) && item.available)
            .map(|item| item.id)
            .collect(),
    };
    let mut out = Vec::new();
    for value in values {
        let normalized = normalize_id(value);
        let canonical = match kind {
            RegistryEntryKind::Source => canonical_source_id(category, &normalized),
            RegistryEntryKind::Subcategory => allowed.iter().copied().find(|id| *id == normalized),
        };
        let Some(id) = canonical else {
            continue;
        };
        if !include_unavailable && !available.contains(id) {
            continue;
        }
        if !out.iter().any(|item| item == id) {
            out.push(id.to_string());
        }
    }
    out
}

pub fn normalize_enabled_map(
    values: HashMap<String, Vec<String>>,
    kind: RegistryEntryKind,
) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let categories = category_ids();
    let known = categories.iter().copied().collect::<HashSet<_>>();
    for (category, items) in values {
        let category = normalize_id(&category);
        if category.is_empty() {
            continue;
        }
        if known.contains(category.as_str()) {
            out.insert(
                category.clone(),
                normalize_enabled_ids(&category, &items, kind, false),
            );
        } else {
            out.insert(category, normalize_unregistered_enabled_ids(&items));
        }
    }
    out
}

pub fn normalize_unregistered_enabled_ids(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let id = normalize_id(value);
        if !id.is_empty() && !out.iter().any(|item| item == &id) {
            out.push(id);
        }
    }
    out
}

pub fn configured_extra_enabled_categories(
    values: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    let known = category_ids().iter().copied().collect::<HashSet<_>>();
    let mut out = HashMap::new();
    for (category, items) in values {
        let category = normalize_id(category);
        if category.is_empty() || known.contains(category.as_str()) {
            continue;
        }
        out.insert(category, normalize_unregistered_enabled_ids(items));
    }
    out
}

pub fn normalize_builtin_enabled_map(
    values: HashMap<String, Vec<String>>,
    kind: RegistryEntryKind,
) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for category in category_ids() {
        if let Some(items) = values.get(category) {
            out.insert(
                category.to_string(),
                normalize_enabled_ids(category, items, kind, false),
            );
        }
    }
    out
}

pub fn defaults_by_category(kind: RegistryEntryKind) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for category in category_ids() {
        let values = match kind {
            RegistryEntryKind::Source => default_source_ids(category),
            RegistryEntryKind::Subcategory => default_subcategory_ids(category),
        };
        out.insert(
            category.to_string(),
            values.into_iter().map(str::to_string).collect(),
        );
    }
    out
}

pub fn normalize_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryEntryKind {
    Source,
    Subcategory,
}

fn categories() -> Vec<RetrievalCategoryDefinition> {
    vec![
        RetrievalCategoryDefinition {
            id: "literature",
            label: "文献",
            description: "论文 / 预印本",
            priority: 10,
        },
        RetrievalCategoryDefinition {
            id: "dataset",
            label: "数据集",
            description: "表达 / 测序",
            priority: 20,
        },
        RetrievalCategoryDefinition {
            id: "knowledge",
            label: "知识库",
            description: "Gene / UniProt / Pathway",
            priority: 30,
        },
        RetrievalCategoryDefinition {
            id: "web",
            label: "通用网页",
            description: "网页搜索",
            priority: 40,
        },
        RetrievalCategoryDefinition {
            id: "social",
            label: "社交内容",
            description: "公众号等",
            priority: 50,
        },
    ]
}

fn subcategories() -> Vec<RetrievalSubcategoryDefinition> {
    use RetrievalSourceStatus::{Available, Planned};
    vec![
        subcategory(
            "literature",
            "paper",
            "Paper",
            "同行评审论文",
            true,
            Available,
            10,
        ),
        subcategory(
            "literature",
            "preprint",
            "Preprint",
            "预印本文献",
            true,
            Available,
            20,
        ),
        subcategory(
            "dataset",
            "expression",
            "Expression",
            "表达矩阵 / 芯片 / RNA-seq 数据集",
            true,
            Available,
            10,
        ),
        subcategory(
            "dataset",
            "sequencing",
            "Sequencing",
            "原始 reads / run / experiment",
            true,
            Available,
            20,
        ),
        subcategory(
            "dataset",
            "genomics",
            "Genomics",
            "assembly / sequence / annotation 元数据",
            true,
            Available,
            30,
        ),
        subcategory(
            "dataset",
            "sample_metadata",
            "Sample metadata",
            "样本、组织、物种、采样地点等元数据",
            true,
            Available,
            40,
        ),
        subcategory(
            "dataset",
            "multi_omics",
            "Multi-omics / Projects",
            "癌症多组学项目级数据",
            false,
            Available,
            50,
        ),
        subcategory(
            "knowledge",
            "local",
            "Local",
            "项目知识库、记忆与来源",
            true,
            Available,
            10,
        ),
        subcategory(
            "knowledge",
            "gene",
            "Gene",
            "基因、symbol、别名与基因组位置",
            true,
            Available,
            20,
        ),
        subcategory(
            "knowledge",
            "protein",
            "Protein",
            "蛋白功能、序列与交叉引用",
            false,
            Available,
            30,
        ),
        subcategory(
            "knowledge",
            "pathway",
            "Pathway",
            "Reactome、GO、MSigDB、KEGG 等通路与 gene-set 知识库",
            false,
            Available,
            40,
        ),
        subcategory(
            "knowledge",
            "disease",
            "Disease",
            "疾病、表型与基因关联",
            false,
            Planned,
            50,
        ),
        subcategory(
            "knowledge",
            "variant",
            "Variant",
            "变异与临床解释",
            false,
            Planned,
            60,
        ),
        subcategory(
            "knowledge",
            "drug",
            "Drug",
            "药物、化合物与靶点",
            false,
            Planned,
            70,
        ),
        subcategory(
            "knowledge",
            "interaction",
            "Interaction",
            "互作网络与调控关系",
            false,
            Planned,
            80,
        ),
        subcategory(
            "web",
            "web_page",
            "Web page",
            "通用网页搜索与抓取",
            true,
            Available,
            10,
        ),
        subcategory(
            "social",
            "public_account",
            "Public account",
            "公众号等社交内容",
            false,
            Available,
            10,
        ),
    ]
}

fn sources() -> Vec<RetrievalSourceDefinition> {
    use RetrievalAutoStrategy::{Fallback, Merge, Single};
    use RetrievalCapability::{Fetch, Query, Search};
    use RetrievalOrigin::{Builtin, Extension};
    use RetrievalRiskLevel::{Low, Medium};
    use RetrievalSourceStatus::{Available, OptIn, Planned, RequiresApiKey};

    vec![
        source(
            "pubmed",
            "literature",
            "PubMed",
            "官方 NCBI E-utilities；API key 可选。",
            &["pmid", "ncbi_pubmed"],
            &["paper"],
            &[Search, Fetch],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &["pubmed_api_key", "pubmed_email", "pubmed_tool_name"],
            10,
            Merge,
            PUBMED_PARAMS,
            Low,
            &["官方公共 API；建议保留 email/tool。"],
            Some("https://pubmed.ncbi.nlm.nih.gov/"),
            Some("https://www.ncbi.nlm.nih.gov/books/NBK25501/"),
        ),
        source(
            "arxiv",
            "literature",
            "arXiv",
            "开放预印本元数据。",
            &["ar_xiv"],
            &["preprint"],
            &[Search, Fetch],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            20,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["官方公共 API。"],
            Some("https://arxiv.org/"),
            Some("https://info.arxiv.org/help/api/index.html"),
        ),
        source(
            "crossref",
            "literature",
            "Crossref",
            "DOI 与出版物元数据。",
            &["cross_ref"],
            &["paper"],
            &[Search, Fetch],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            30,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["公共元数据 API。"],
            Some("https://www.crossref.org/"),
            Some("https://api.crossref.org/swagger-ui/index.html"),
        ),
        source(
            "openalex",
            "literature",
            "OpenAlex",
            "开放学术图谱和论文元数据。",
            &["open_alex"],
            &["paper"],
            &[Search, Fetch],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            40,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["公共元数据 API。"],
            Some("https://openalex.org/"),
            Some("https://docs.openalex.org/"),
        ),
        source(
            "biorxiv",
            "literature",
            "bioRxiv",
            "生命科学预印本。",
            &["bio_rxiv"],
            &["preprint"],
            &[Search, Fetch],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            50,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["公共 API。"],
            Some("https://www.biorxiv.org/"),
            Some("https://api.biorxiv.org/"),
        ),
        source(
            "medrxiv",
            "literature",
            "medRxiv",
            "医学预印本。",
            &["med_rxiv"],
            &["preprint"],
            &[Search, Fetch],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            60,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["公共 API。"],
            Some("https://www.medrxiv.org/"),
            Some("https://api.biorxiv.org/"),
        ),
        source(
            "semantic_scholar",
            "literature",
            "Semantic Scholar",
            "Academic Graph API；需要用户 API key。",
            &["semanticscholar", "s2"],
            &["paper"],
            &[Search, Fetch],
            RequiresApiKey,
            Builtin,
            false,
            true,
            true,
            &["semantic_scholar_api_key"],
            &[],
            70,
            Merge,
            BASIC_QUERY_PARAMS,
            Medium,
            &["查询会发送给 Semantic Scholar 第三方 API。"],
            Some("https://www.semanticscholar.org/"),
            Some("https://api.semanticscholar.org/api-docs/"),
        ),
        source(
            "geo",
            "dataset",
            "GEO",
            "Expression / NCBI GEO DataSets。",
            &["gds", "ncbi_geo", "ncbi_gds"],
            &["expression"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &["pubmed_api_key", "pubmed_email", "pubmed_tool_name"],
            10,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["官方 NCBI E-utilities。"],
            Some("https://www.ncbi.nlm.nih.gov/geo/"),
            Some("https://www.ncbi.nlm.nih.gov/books/NBK25501/"),
        ),
        source(
            "ena",
            "dataset",
            "ENA",
            "Sequencing / Genomics / Sample metadata。",
            &[
                "ena_study",
                "ena_run",
                "ena_experiment",
                "ena_sample",
                "ena_analysis",
                "ena_assembly",
                "ena_sequence",
                "european_nucleotide_archive",
            ],
            &["sequencing", "genomics", "sample_metadata"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            20,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["官方 ENA Portal / Browser API。"],
            Some("https://www.ebi.ac.uk/ena/browser/home"),
            Some("https://ena-docs.readthedocs.io/en/latest/retrieval/programmatic-access.html"),
        ),
        source(
            "cbioportal",
            "dataset",
            "cBioPortal",
            "Cancer genomics / TCGA studies。",
            &[
                "cbio_portal",
                "cbio",
                "cancer_genomics",
                "multi_omics",
                "multiomics",
                "projects",
                "tcga",
            ],
            &["multi_omics"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            false,
            false,
            false,
            &[],
            &[],
            30,
            Single,
            BASIC_QUERY_PARAMS,
            Low,
            &["公共 REST API；当前为 study 级元数据。"],
            Some("https://www.cbioportal.org/"),
            Some("https://docs.cbioportal.org/web-api-and-clients/"),
        ),
        source(
            "gtex",
            "dataset",
            "GTEx",
            "组织特异表达、组织清单与 top expressed gene。",
            &["genotype_tissue_expression", "tissue_expression"],
            &["expression"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            false,
            false,
            false,
            &[],
            &[],
            40,
            Merge,
            GTEX_PARAMS,
            Low,
            &["官方 GTEx Portal API v2；公共免 key。"],
            Some("https://gtexportal.org/"),
            Some("https://gtexportal.org/api/v2/redoc"),
        ),
        source(
            "ncbi_datasets",
            "dataset",
            "NCBI Datasets",
            "Genome assemblies via official Datasets v2 REST API。",
            &[
                "ncbi_dataset",
                "ncbi_genome",
                "ncbi_genomes",
                "ncbi_assembly",
                "ncbi_assemblies",
                "genome_dataset",
                "genome_datasets",
            ],
            &["genomics"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            false,
            false,
            false,
            &[],
            &["pubmed_api_key"],
            45,
            Merge,
            NCBI_DATASETS_PARAMS,
            Low,
            &["官方 NCBI Datasets v2 REST API；只返回元数据与下载链接，不自动下载 genome package。"],
            Some("https://www.ncbi.nlm.nih.gov/datasets/genomes/"),
            Some("https://www.ncbi.nlm.nih.gov/datasets/docs/v2/api/rest-api/"),
        ),
        source(
            "arrayexpress",
            "dataset",
            "ArrayExpress",
            "Functional genomics studies via EMBL-EBI BioStudies ArrayExpress collection。",
            &[
                "array_express",
                "ae",
                "ebi_arrayexpress",
                "biostudies_arrayexpress",
                "functional_genomics",
            ],
            &["expression"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            false,
            false,
            false,
            &[],
            &[],
            50,
            Merge,
            ARRAYEXPRESS_PARAMS,
            Low,
            &["Public BioStudies API；ArrayExpress accessions are preserved after migration to BioStudies。"],
            Some("https://www.ebi.ac.uk/biostudies/arrayexpress"),
            Some("https://www.ebi.ac.uk/biostudies/arrayexpress-in-biostudies"),
        ),
        source(
            "biosample",
            "dataset",
            "BioSample",
            "Sample metadata via NCBI BioSample。",
            &[
                "bio_sample",
                "biosamples",
                "ncbi_biosample",
                "ncbi_biosamples",
                "ncbi_sample",
                "ncbi_samples",
                "sample_metadata",
            ],
            &["sample_metadata"],
            &[Search, Fetch, Query],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &["pubmed_api_key", "pubmed_email", "pubmed_tool_name"],
            60,
            Merge,
            BIOSAMPLE_PARAMS,
            Low,
            &[
                "Search uses official NCBI E-utilities db=biosample; fetch uses NCBI Datasets v2 BioSample reports。",
                "NCBI API key optional；email/tool 使用设置中的 PubMed/NCBI 配置。",
            ],
            Some("https://www.ncbi.nlm.nih.gov/biosample"),
            Some("https://www.ncbi.nlm.nih.gov/datasets/docs/v2/api/rest-api/"),
        ),
        source(
            "project_wiki",
            "knowledge",
            "Project wiki",
            "项目知识库与文档化笔记。",
            &["wiki"],
            &["local"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            10,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["本地知识检索。"],
            None,
            None,
        ),
        source(
            "session_memory",
            "knowledge",
            "Session memory",
            "历史会话与隐式记忆。",
            &["implicit", "memory"],
            &["local"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            20,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["本地知识检索。"],
            None,
            None,
        ),
        source(
            "long_term",
            "knowledge",
            "Long-term",
            "沉淀后的长期偏好、决策和经验。",
            &["permanent"],
            &["local"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            30,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["本地知识检索。"],
            None,
            None,
        ),
        source(
            "sources",
            "knowledge",
            "Sources",
            "过去记录过的网页、论文与数据来源。",
            &["source_history"],
            &["local"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            40,
            Merge,
            BASIC_QUERY_PARAMS,
            Low,
            &["本地知识检索。"],
            None,
            None,
        ),
        source(
            "ncbi_gene",
            "knowledge",
            "NCBI Gene",
            "Gene ID / symbol / organism；官方 E-utilities。",
            &["gene"],
            &["gene"],
            &[Query],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &["pubmed_api_key", "pubmed_email", "pubmed_tool_name"],
            50,
            Single,
            GENE_PARAMS,
            Low,
            &["官方 NCBI E-utilities。"],
            Some("https://www.ncbi.nlm.nih.gov/gene/"),
            Some("https://www.ncbi.nlm.nih.gov/books/NBK25501/"),
        ),
        source(
            "uniprot",
            "knowledge",
            "UniProt",
            "蛋白功能、序列、GO 与交叉引用。",
            &["uni_prot", "uniprotkb", "uniprot_kb", "protein", "proteins"],
            &["protein"],
            &[Query],
            Available,
            Builtin,
            false,
            false,
            false,
            &[],
            &[],
            60,
            Single,
            UNIPROT_PARAMS,
            Low,
            &["官方 UniProt REST API。"],
            Some("https://www.uniprot.org/"),
            Some("https://www.uniprot.org/help/programmatic_access"),
        ),
        source(
            "ensembl",
            "knowledge",
            "Ensembl",
            "基因、转录本、变异与物种注释。",
            &[
                "ensembl_gene",
                "ensembl_transcript",
                "gene_annotation",
                "transcript",
                "transcripts",
                "variation",
                "variant",
                "variants",
            ],
            &["gene", "variant"],
            &[Query],
            Available,
            Builtin,
            false,
            false,
            false,
            &[],
            &[],
            70,
            Single,
            ENSEMBL_PARAMS,
            Low,
            &["官方 Ensembl REST API；无需 API key。"],
            Some("https://www.ensembl.org/"),
            Some("https://rest.ensembl.org/"),
        ),
        source(
            "reactome",
            "knowledge",
            "Reactome",
            "人工整理的生物通路、反应网络与富集分析。",
            &["reactome_pathway", "reactome_pathways", "reaction_network"],
            &["pathway"],
            &[Search, Fetch, Query],
            Available,
            Extension,
            false,
            false,
            false,
            &[],
            &[],
            80,
            Merge,
            PATHWAY_QUERY_PARAMS,
            Low,
            &["由 Pathway Databases 插件接入 Reactome Content/Analysis Service。"],
            Some("https://reactome.org/"),
            Some("https://reactome.org/dev/content-service"),
        ),
        source(
            "gene_ontology",
            "knowledge",
            "Gene Ontology",
            "GO biological process、molecular function 与 cellular component 术语。",
            &["go", "quickgo", "go_terms", "gene_ontology_terms"],
            &["pathway"],
            &[Search, Fetch, Query],
            Available,
            Extension,
            false,
            false,
            false,
            &[],
            &[],
            82,
            Merge,
            PATHWAY_QUERY_PARAMS,
            Low,
            &["由 Pathway Databases 插件接入 EMBL-EBI QuickGO API。"],
            Some("https://geneontology.org/"),
            Some("https://www.ebi.ac.uk/QuickGO/api/index.html"),
        ),
        source(
            "msigdb",
            "knowledge",
            "MSigDB",
            "Molecular Signatures Database gene-set collections for GSEA。",
            &["molecular_signatures_database", "gsea_msigdb", "gene_sets", "gmt"],
            &["pathway"],
            &[Search, Fetch, Query],
            Available,
            Extension,
            false,
            false,
            false,
            &[],
            &[],
            84,
            Merge,
            MSIGDB_PARAMS,
            Low,
            &["由 Pathway Databases 插件接入 MSigDB 公共页面/JSON；请遵守 MSigDB 再分发条款。"],
            Some("https://www.gsea-msigdb.org/gsea/msigdb"),
            Some("https://www.gsea-msigdb.org/gsea/msigdb"),
        ),
        source(
            "kegg",
            "knowledge",
            "KEGG",
            "KEGG pathway、module、compound、reaction、enzyme、disease 与 drug REST API。",
            &[
                "kegg_pathway",
                "kegg_rest",
                "kyoto_encyclopedia_of_genes_and_genomes",
            ],
            &["pathway"],
            &[Search, Fetch, Query],
            Available,
            Extension,
            false,
            false,
            false,
            &[],
            &[],
            86,
            Merge,
            KEGG_PARAMS,
            Low,
            &["由 Pathway Databases 插件接入 KEGG REST；KEGG API 仅供学术用户学术使用，商业使用需确认授权。"],
            Some("https://www.kegg.jp/"),
            Some("https://www.kegg.jp/kegg/rest/keggapi.html"),
        ),
        source(
            "clinvar",
            "knowledge",
            "ClinVar",
            "变异与临床解释，待接入。",
            &[],
            &["variant"],
            &[Query],
            Planned,
            Builtin,
            false,
            false,
            false,
            &[],
            &[],
            90,
            Merge,
            &[],
            Low,
            &["计划接入；当前不可执行。"],
            Some("https://www.ncbi.nlm.nih.gov/clinvar/"),
            Some("https://www.ncbi.nlm.nih.gov/books/NBK25501/"),
        ),
        source(
            "tavily",
            "web",
            "Tavily",
            "通用网页 API 搜索。",
            &[],
            &["web_page"],
            &[Search],
            RequiresApiKey,
            Builtin,
            false,
            true,
            false,
            &["tavily_api_key"],
            &[],
            10,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["查询会发送给 Tavily 第三方 API。"],
            Some("https://tavily.com/"),
            Some("https://docs.tavily.com/"),
        ),
        source(
            "ddg",
            "web",
            "DuckDuckGo",
            "公共 Instant Answer + HTML 搜索回退。",
            &["duckduckgo"],
            &["web_page"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            20,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["公共 HTML/Instant Answer 端点可能限流或结构变化。"],
            Some("https://duckduckgo.com/"),
            Some("https://duckduckgo.com/duckduckgo-help-pages/settings/params/"),
        ),
        source(
            "google",
            "web",
            "Google",
            "公共 HTML 搜索回退。",
            &[],
            &["web_page"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            30,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["公共 HTML 页面可能限流或结构变化。"],
            Some("https://www.google.com/"),
            None,
        ),
        source(
            "bing",
            "web",
            "Bing",
            "公共 HTML 搜索回退。",
            &[],
            &["web_page"],
            &[Search],
            Available,
            Builtin,
            true,
            false,
            false,
            &[],
            &[],
            40,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["公共 HTML 页面可能限流或结构变化。"],
            Some("https://www.bing.com/"),
            None,
        ),
        source(
            "exa",
            "web",
            "Exa",
            "语义网页检索和内容提取 API。",
            &[],
            &["web_page"],
            &[Search],
            RequiresApiKey,
            Builtin,
            false,
            true,
            false,
            &["exa_api_key"],
            &[],
            50,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["查询会发送给 Exa 第三方 API。"],
            Some("https://exa.ai/"),
            Some("https://docs.exa.ai/"),
        ),
        source(
            "firecrawl",
            "web",
            "Firecrawl",
            "网页搜索/抓取 API，可自定义 base URL。",
            &[],
            &["web_page"],
            &[Search],
            RequiresApiKey,
            Builtin,
            false,
            true,
            false,
            &["firecrawl_api_key"],
            &["firecrawl_url"],
            60,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["查询会发送给 Firecrawl 或自定义 Firecrawl 服务。"],
            Some("https://www.firecrawl.dev/"),
            Some("https://docs.firecrawl.dev/"),
        ),
        source(
            "parallel",
            "web",
            "Parallel",
            "网页搜索 API。",
            &[],
            &["web_page"],
            &[Search],
            RequiresApiKey,
            Builtin,
            false,
            true,
            false,
            &["parallel_api_key"],
            &[],
            70,
            Fallback,
            BASIC_QUERY_PARAMS,
            Medium,
            &["查询会发送给 Parallel 第三方 API。"],
            Some("https://parallel.ai/"),
            Some("https://docs.parallel.ai/"),
        ),
        source(
            "agent_browser",
            "web",
            "agent-browser",
            "浏览器自动化扩展，不作为内置执行。",
            &["browser", "agentbrowser"],
            &["web_page"],
            &[Search, Fetch],
            RetrievalSourceStatus::Extension,
            RetrievalOrigin::Extension,
            false,
            false,
            true,
            &[],
            &[],
            80,
            Single,
            &[],
            RetrievalRiskLevel::High,
            &["需要扩展安装；自动化浏览器权限更高。"],
            None,
            None,
        ),
        source(
            "wechat",
            "social",
            "微信公众号搜索",
            "Sogou 微信公开 HTML 搜索；默认关闭。",
            &["weixin", "sogou_wechat", "sogou_weixin"],
            &["public_account"],
            &[Search, Fetch],
            OptIn,
            Builtin,
            false,
            false,
            true,
            &[],
            &[],
            10,
            Single,
            BASIC_QUERY_PARAMS,
            Medium,
            &["依赖公开 HTML 页面，可能被验证码、限流或页面结构变化影响。"],
            Some("https://weixin.sogou.com/"),
            None,
        ),
    ]
}

fn subcategory(
    category: &'static str,
    id: &'static str,
    label: &'static str,
    description: &'static str,
    default_enabled: bool,
    status: RetrievalSourceStatus,
    priority: i32,
) -> RetrievalSubcategoryDefinition {
    RetrievalSubcategoryDefinition {
        id,
        category,
        label,
        description,
        default_enabled,
        available: matches!(status, RetrievalSourceStatus::Available),
        status,
        priority,
    }
}

#[allow(clippy::too_many_arguments)]
fn source(
    id: &'static str,
    category: &'static str,
    label: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    subcategories: &'static [&'static str],
    capabilities: &'static [RetrievalCapability],
    status: RetrievalSourceStatus,
    origin: RetrievalOrigin,
    default_enabled: bool,
    requires_api_key: bool,
    requires_opt_in: bool,
    required_credential_refs: &'static [&'static str],
    optional_credential_refs: &'static [&'static str],
    priority: i32,
    auto_strategy: RetrievalAutoStrategy,
    parameters: &'static [RetrievalParameterDefinition],
    risk_level: RetrievalRiskLevel,
    risk_notes: &'static [&'static str],
    homepage_url: Option<&'static str>,
    docs_url: Option<&'static str>,
) -> RetrievalSourceDefinition {
    RetrievalSourceDefinition {
        id,
        category,
        label,
        description,
        aliases,
        subcategories,
        capabilities,
        status,
        origin,
        available: matches!(
            status,
            RetrievalSourceStatus::Available
                | RetrievalSourceStatus::RequiresApiKey
                | RetrievalSourceStatus::OptIn
        ),
        default_enabled,
        requires_api_key,
        requires_opt_in,
        required_credential_refs,
        optional_credential_refs,
        priority,
        auto_strategy,
        parameters,
        risk_level,
        risk_notes,
        homepage_url,
        docs_url,
    }
}

const fn param(
    name: &'static str,
    param_type: RetrievalParameterType,
    description: &'static str,
    required: bool,
) -> RetrievalParameterDefinition {
    RetrievalParameterDefinition {
        name,
        param_type,
        description,
        required,
    }
}

const BASIC_QUERY_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "关键词或源原生查询语句。",
        true,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "返回记录上限。",
        false,
    ),
];

const PUBMED_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "PubMed / Entrez 查询语句。",
        true,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "返回记录上限。",
        false,
    ),
    param(
        "sort",
        RetrievalParameterType::String,
        "NCBI ESearch 排序。",
        false,
    ),
    param(
        "mindate",
        RetrievalParameterType::String,
        "起始日期过滤。",
        false,
    ),
    param(
        "maxdate",
        RetrievalParameterType::String,
        "结束日期过滤。",
        false,
    ),
];

const GENE_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "Gene symbol、Gene ID 或 Entrez 查询语句。",
        true,
    ),
    param(
        "organism",
        RetrievalParameterType::String,
        "物种名称，例如 Homo sapiens。",
        false,
    ),
    param(
        "taxon_id",
        RetrievalParameterType::String,
        "NCBI taxonomy id。",
        false,
    ),
    param(
        "ret_start",
        RetrievalParameterType::Integer,
        "分页偏移。",
        false,
    ),
    param(
        "sort",
        RetrievalParameterType::String,
        "NCBI ESearch 排序。",
        false,
    ),
];

const UNIPROT_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "UniProt 查询语句，例如 gene_exact:BRCA1。",
        true,
    ),
    param(
        "organism",
        RetrievalParameterType::String,
        "物种名称。",
        false,
    ),
    param(
        "taxon_id",
        RetrievalParameterType::String,
        "NCBI taxonomy id。",
        false,
    ),
    param(
        "reviewed",
        RetrievalParameterType::Boolean,
        "仅返回 Swiss-Prot reviewed 条目。",
        false,
    ),
];

const ENSEMBL_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "Gene symbol、Ensembl stable ID、rsID 或 Ensembl URL。",
        true,
    ),
    param(
        "species",
        RetrievalParameterType::String,
        "Ensembl species name，例如 homo_sapiens。",
        false,
    ),
    param(
        "object_type",
        RetrievalParameterType::String,
        "symbol lookup 类型：gene、transcript 或 translation。",
        false,
    ),
];

const PATHWAY_QUERY_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "通路 / GO / gene-set 关键词、稳定 ID 或源原生查询语句。",
        true,
    ),
    param(
        "species",
        RetrievalParameterType::String,
        "物种过滤，例如 Homo sapiens 或 human。",
        false,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "返回记录上限。",
        false,
    ),
];

const MSIGDB_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "MSigDB gene-set 名称关键词或精确 gene-set ID。",
        true,
    ),
    param(
        "collection",
        RetrievalParameterType::String,
        "可选 collection 过滤，例如 H、GO:BP、CP:REACTOME。",
        false,
    ),
    param(
        "species",
        RetrievalParameterType::String,
        "MSigDB namespace，默认 human。",
        false,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "返回 gene set 上限。",
        false,
    ),
];

const KEGG_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "KEGG ID、关键词或 link/conv 源 ID。",
        true,
    ),
    param(
        "organism",
        RetrievalParameterType::String,
        "KEGG organism code，例如 hsa、mmu。",
        false,
    ),
    param(
        "database",
        RetrievalParameterType::String,
        "KEGG database，默认 pathway；也可为 module、compound、drug 等。",
        false,
    ),
    param(
        "mode",
        RetrievalParameterType::String,
        "KEGG REST mode：list、find、link、conv、info。",
        false,
    ),
    param(
        "target",
        RetrievalParameterType::String,
        "link/conv 的目标库，例如 pathway、genes、uniprot、ncbi-geneid。",
        false,
    ),
    param(
        "option",
        RetrievalParameterType::String,
        "可选格式/搜索选项，例如 kgml、json、formula、exact_mass。",
        false,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "返回记录上限。",
        false,
    ),
];

const NCBI_DATASETS_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "Taxon/organism、GCA_/GCF_ assembly accession、BioProject、BioSample 或 assembly name。",
        true,
    ),
    param(
        "mode",
        RetrievalParameterType::String,
        "accession、taxon、bioproject、biosample、wgs 或 assembly_name；未设置时自动推断。",
        false,
    ),
    param(
        "reference_only",
        RetrievalParameterType::Boolean,
        "仅返回 reference genome assemblies。",
        false,
    ),
    param(
        "assembly_source",
        RetrievalParameterType::String,
        "refseq、genbank 或 all。",
        false,
    ),
    param(
        "assembly_level",
        RetrievalParameterType::String,
        "complete_genome、chromosome、scaffold 或 contig；可用逗号分隔。",
        false,
    ),
    param(
        "search_text",
        RetrievalParameterType::String,
        "按 submitter、assembly name、strain 或 organism name 缩小结果。",
        false,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "返回 genome assembly report 上限。",
        false,
    ),
    param(
        "include",
        RetrievalParameterType::String,
        "download_summary 包含文件类型，例如 genome,gff3,protein,sequence_report。",
        false,
    ),
    param(
        "chromosomes",
        RetrievalParameterType::String,
        "download_summary 只预览指定 chromosomes，可用逗号分隔。",
        false,
    ),
];

const ARRAYEXPRESS_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "ArrayExpress/BioStudies keyword query。",
        true,
    ),
    param(
        "id",
        RetrievalParameterType::String,
        "ArrayExpress accession such as E-MTAB-1234 for fetch/get。",
        false,
    ),
    param(
        "organism",
        RetrievalParameterType::String,
        "Optional organism keyword appended to the search query。",
        false,
    ),
    param(
        "study_type",
        RetrievalParameterType::String,
        "Optional study type keyword such as RNA-seq。",
        false,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "Maximum ArrayExpress studies to return。",
        false,
    ),
];

const BIOSAMPLE_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "BioSample keyword query for E-utilities db=biosample。",
        true,
    ),
    param(
        "id",
        RetrievalParameterType::String,
        "BioSample accession such as SAMN15960293 for fetch/get。",
        false,
    ),
    param(
        "organism",
        RetrievalParameterType::String,
        "Optional organism name filter。",
        false,
    ),
    param(
        "taxon_id",
        RetrievalParameterType::String,
        "Optional NCBI taxonomy id filter。",
        false,
    ),
    param(
        "max_results",
        RetrievalParameterType::Integer,
        "Maximum BioSample records to return。",
        false,
    ),
];

const GTEX_PARAMS: &[RetrievalParameterDefinition] = &[
    param(
        "query",
        RetrievalParameterType::String,
        "Gene symbol、GENCODE ID、tissue 关键词或 tissueSiteDetailId。",
        true,
    ),
    param(
        "endpoint",
        RetrievalParameterType::String,
        "gene、median_expression、tissues 或 top_expressed。",
        false,
    ),
    param(
        "datasetId",
        RetrievalParameterType::String,
        "GTEx datasetId，默认 gtex_v8。",
        false,
    ),
    param(
        "gencodeId",
        RetrievalParameterType::String,
        "Versioned GENCODE gene ID。",
        false,
    ),
    param(
        "tissueSiteDetailId",
        RetrievalParameterType::String,
        "GTEx tissueSiteDetailId，例如 Whole_Blood。",
        false,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_within_entry_types() {
        let registry = registry();
        let mut categories = HashSet::new();
        for item in registry.categories {
            assert!(categories.insert(item.id));
        }
        let mut subcategories = HashSet::new();
        for item in registry.subcategories {
            assert!(subcategories.insert((item.category, item.id)));
        }
        let mut sources = HashSet::new();
        for item in registry.sources {
            assert!(sources.insert((item.category, item.id)));
        }
    }

    #[test]
    fn available_sources_have_capabilities_and_valid_categories() {
        let registry = registry();
        let categories: HashSet<_> = registry.categories.iter().map(|item| item.id).collect();
        let subcategories: HashSet<_> = registry
            .subcategories
            .iter()
            .map(|item| (item.category, item.id))
            .collect();
        for source in registry.sources {
            assert!(categories.contains(source.category), "{}", source.id);
            assert!(!source.capabilities.is_empty(), "{}", source.id);
            for subcategory in source.subcategories {
                assert!(
                    subcategories.contains(&(source.category, *subcategory)),
                    "{} -> {}",
                    source.id,
                    subcategory
                );
            }
            if source.default_enabled {
                assert!(source.can_execute(), "{}", source.id);
            }
        }
    }

    #[test]
    fn canonicalizes_source_aliases() {
        assert_eq!(canonical_source_id("dataset", "ena_run"), Some("ena"));
        assert_eq!(canonical_source_id("knowledge", "protein"), Some("uniprot"));
        assert_eq!(
            canonical_source_id("literature", "s2"),
            Some("semantic_scholar")
        );
        assert_eq!(
            canonical_source_id("dataset", "ncbi_genome"),
            Some("ncbi_datasets")
        );
        assert_eq!(
            canonical_source_id("dataset", "ncbi_biosample"),
            Some("biosample")
        );
        assert_eq!(
            canonical_source_id("dataset", "array_express"),
            Some("arrayexpress")
        );
    }

    #[test]
    fn source_aliases_are_unambiguous_within_each_category() {
        let mut seen: HashMap<(String, String), &'static str> = HashMap::new();
        for source in registry().sources {
            let source_key = (source.category.to_string(), normalize_id(source.id));
            assert!(
                seen.insert(source_key, source.id).is_none(),
                "duplicate source id `{}` in category `{}`",
                source.id,
                source.category
            );
            for alias in source.aliases {
                let key = (source.category.to_string(), normalize_id(alias));
                let previous = seen.insert(key, source.id);
                assert!(
                    previous.is_none() || previous == Some(source.id),
                    "source alias `{alias}` in category `{}` is ambiguous between `{}` and `{}`",
                    source.category,
                    previous.unwrap(),
                    source.id
                );
            }
        }
    }

    #[test]
    fn source_status_origin_and_flags_are_coherent() {
        for source in registry().sources {
            assert_eq!(
                source.available,
                source.can_execute(),
                "source `{}` available flag must be derived from status",
                source.id
            );
            if source.default_enabled {
                assert!(
                    source.can_execute(),
                    "source `{}` cannot be default-enabled unless executable",
                    source.id
                );
                assert_eq!(
                    source.origin,
                    RetrievalOrigin::Builtin,
                    "source `{}` default-enabled sources must be built in",
                    source.id
                );
            }
            match source.status {
                RetrievalSourceStatus::Extension => {
                    assert_eq!(
                        source.origin,
                        RetrievalOrigin::Extension,
                        "extension source `{}` must have Extension origin",
                        source.id
                    );
                    assert!(
                        !source.default_enabled,
                        "extension source `{}` must not be default-enabled",
                        source.id
                    );
                }
                RetrievalSourceStatus::Planned => {
                    assert!(
                        !source.default_enabled && !source.available,
                        "planned source `{}` must not be enabled/available",
                        source.id
                    );
                }
                RetrievalSourceStatus::RequiresApiKey => {
                    assert!(
                        source.requires_api_key,
                        "source `{}` status RequiresApiKey must set requires_api_key",
                        source.id
                    );
                    assert!(
                        !source.required_credential_refs.is_empty(),
                        "source `{}` requires an API key but has no credential refs",
                        source.id
                    );
                }
                RetrievalSourceStatus::OptIn => {
                    assert!(
                        source.requires_opt_in && !source.default_enabled,
                        "opt-in source `{}` must require opt-in and stay default-off",
                        source.id
                    );
                }
                RetrievalSourceStatus::Available => {}
            }
        }
    }

    #[test]
    fn enabled_source_normalization_preserves_plugin_categories() {
        let normalized = normalize_enabled_map(
            HashMap::from([
                (
                    "drug".to_string(),
                    vec![
                        "ChEMBL".to_string(),
                        "chembl".to_string(),
                        "Drug Bank".to_string(),
                    ],
                ),
                (
                    "dataset".to_string(),
                    vec!["ncbi-biosample".to_string(), "unknown".to_string()],
                ),
            ]),
            RegistryEntryKind::Source,
        );

        assert_eq!(
            normalized.get("drug"),
            Some(&vec!["chembl".to_string(), "drug_bank".to_string()])
        );
        assert_eq!(
            normalized.get("dataset"),
            Some(&vec!["biosample".to_string()])
        );
    }

    #[test]
    fn defaults_include_current_query_sources() {
        assert_eq!(
            default_subcategory_ids("dataset"),
            vec!["expression", "sequencing", "genomics", "sample_metadata"]
        );
        assert_eq!(
            default_source_ids("dataset"),
            vec!["geo", "ena", "biosample"]
        );
        assert_eq!(
            default_source_ids("knowledge"),
            vec![
                "project_wiki",
                "session_memory",
                "long_term",
                "sources",
                "ncbi_gene"
            ]
        );
        assert_eq!(default_source_ids("web"), vec!["ddg", "google", "bing"]);
        assert_eq!(default_source_ids("social"), Vec::<&str>::new());
    }
}
