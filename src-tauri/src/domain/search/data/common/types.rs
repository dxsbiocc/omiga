use super::parsing::clean_optional;
use crate::domain::tools::WebSearchApiKeys;
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as Json};

const DEFAULT_EUTILS_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const ENA_PORTAL_SEARCH_URL: &str = "https://www.ebi.ac.uk/ena/portal/api/search";
const ENA_BROWSER_XML_BASE_URL: &str = "https://www.ebi.ac.uk/ena/browser/api/xml";
const CBIOPORTAL_API_BASE_URL: &str = "https://www.cbioportal.org/api";
const GTEX_API_BASE_URL: &str = "https://gtexportal.org/api/v2";
const NCBI_DATASETS_API_BASE_URL: &str = "https://api.ncbi.nlm.nih.gov/datasets/v2";
const DEFAULT_MAX_RESULTS: u32 = 10;
pub(in crate::domain::search::data) const MAX_RESULTS_CAP: u32 = 25;
const DEFAULT_EMAIL: &str = "omiga@example.invalid";
const DEFAULT_TOOL: &str = "omiga";
const GEO_FAVICON: &str = "https://www.ncbi.nlm.nih.gov/favicon.ico";
const ENA_FAVICON: &str = "https://www.ebi.ac.uk/favicon.ico";
const CBIOPORTAL_FAVICON: &str = "https://www.cbioportal.org/favicon.ico";
const GTEX_FAVICON: &str = "https://gtexportal.org/favicon.ico";
const NCBI_FAVICON: &str = "https://www.ncbi.nlm.nih.gov/favicon.ico";

#[derive(Clone, Debug)]
pub struct DataApiBaseUrls {
    pub entrez: String,
    pub ena_portal_search: String,
    pub ena_browser_xml: String,
    pub cbioportal: String,
    pub gtex: String,
    pub ncbi_datasets: String,
}

impl Default for DataApiBaseUrls {
    fn default() -> Self {
        Self {
            entrez: DEFAULT_EUTILS_BASE_URL.to_string(),
            ena_portal_search: ENA_PORTAL_SEARCH_URL.to_string(),
            ena_browser_xml: ENA_BROWSER_XML_BASE_URL.to_string(),
            cbioportal: CBIOPORTAL_API_BASE_URL.to_string(),
            gtex: GTEX_API_BASE_URL.to_string(),
            ncbi_datasets: NCBI_DATASETS_API_BASE_URL.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicDataSource {
    Geo,
    EnaStudy,
    EnaRun,
    EnaExperiment,
    EnaSample,
    EnaAnalysis,
    EnaAssembly,
    EnaSequence,
    CbioPortal,
    Gtex,
    NcbiDatasets,
}

impl PublicDataSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "geo" | "gds" | "ncbi_geo" | "ncbi_gds" => Some(Self::Geo),
            "ena" | "ena_study" | "study" | "read_study" | "european_nucleotide_archive" => {
                Some(Self::EnaStudy)
            }
            "ena_run" | "run" | "read_run" => Some(Self::EnaRun),
            "ena_experiment" | "experiment" | "read_experiment" => Some(Self::EnaExperiment),
            "ena_sample" | "sample" | "read_sample" => Some(Self::EnaSample),
            "ena_analysis" | "analysis" => Some(Self::EnaAnalysis),
            "ena_assembly" | "assembly" => Some(Self::EnaAssembly),
            "ena_sequence" | "sequence" => Some(Self::EnaSequence),
            "cbioportal" | "cbio_portal" | "cbio" | "cancer_genomics" | "multi_omics"
            | "multiomics" | "projects" | "tcga" => Some(Self::CbioPortal),
            "gtex" | "genotype_tissue_expression" | "tissue_expression" => Some(Self::Gtex),
            "ncbi_datasets" | "ncbi_dataset" | "ncbi_genome" | "ncbi_genomes" | "ncbi_assembly"
            | "ncbi_assemblies" | "genome_datasets" | "genome_dataset" => Some(Self::NcbiDatasets),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Geo => "geo",
            Self::EnaStudy => "ena",
            Self::EnaRun => "ena_run",
            Self::EnaExperiment => "ena_experiment",
            Self::EnaSample => "ena_sample",
            Self::EnaAnalysis => "ena_analysis",
            Self::EnaAssembly => "ena_assembly",
            Self::EnaSequence => "ena_sequence",
            Self::CbioPortal => "cbioportal",
            Self::Gtex => "gtex",
            Self::NcbiDatasets => "ncbi_datasets",
        }
    }

    pub(in crate::domain::search::data) fn label(self) -> &'static str {
        match self {
            Self::Geo => "NCBI GEO DataSets",
            Self::EnaStudy => "ENA read studies",
            Self::EnaRun => "ENA raw read runs",
            Self::EnaExperiment => "ENA read experiments",
            Self::EnaSample => "ENA samples",
            Self::EnaAnalysis => "ENA analyses",
            Self::EnaAssembly => "ENA assemblies",
            Self::EnaSequence => "ENA nucleotide sequences",
            Self::CbioPortal => "cBioPortal cancer genomics studies",
            Self::Gtex => "GTEx tissue expression",
            Self::NcbiDatasets => "NCBI Datasets genome assemblies",
        }
    }

    pub(in crate::domain::search::data) fn favicon(self) -> &'static str {
        match self {
            Self::Geo => GEO_FAVICON,
            Self::EnaStudy
            | Self::EnaRun
            | Self::EnaExperiment
            | Self::EnaSample
            | Self::EnaAnalysis
            | Self::EnaAssembly
            | Self::EnaSequence => ENA_FAVICON,
            Self::CbioPortal => CBIOPORTAL_FAVICON,
            Self::Gtex => GTEX_FAVICON,
            Self::NcbiDatasets => NCBI_FAVICON,
        }
    }

    pub(in crate::domain::search::data) fn ena_result(self) -> Option<&'static str> {
        match self {
            Self::Geo => None,
            Self::EnaStudy => Some("read_study"),
            Self::EnaRun => Some("read_run"),
            Self::EnaExperiment => Some("read_experiment"),
            Self::EnaSample => Some("sample"),
            Self::EnaAnalysis => Some("analysis"),
            Self::EnaAssembly => Some("assembly"),
            Self::EnaSequence => Some("sequence"),
            Self::CbioPortal | Self::Gtex | Self::NcbiDatasets => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DataSearchArgs {
    #[serde(alias = "term", alias = "q")]
    pub query: String,
    #[serde(default, alias = "maxResults", alias = "limit", alias = "retmax")]
    pub max_results: Option<u32>,
    #[serde(default)]
    pub params: Option<Json>,
}

impl DataSearchArgs {
    pub fn normalized_max_results(&self) -> u32 {
        self.max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRecord {
    pub id: String,
    pub accession: String,
    pub source: PublicDataSource,
    pub title: String,
    pub summary: String,
    pub url: String,
    pub record_type: Option<String>,
    pub organism: Option<String>,
    pub published_date: Option<String>,
    pub updated_date: Option<String>,
    pub sample_count: Option<u64>,
    pub platform: Option<String>,
    pub files: Vec<String>,
    pub extra: JsonMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSearchResponse {
    pub query: String,
    pub source: String,
    pub total: Option<u64>,
    pub results: Vec<DataRecord>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub(in crate::domain::search::data) struct EntrezSettings {
    pub(in crate::domain::search::data) api_key: Option<String>,
    pub(in crate::domain::search::data) email: String,
    pub(in crate::domain::search::data) tool: String,
}

impl EntrezSettings {
    pub(in crate::domain::search::data) fn from_keys(keys: &WebSearchApiKeys) -> Self {
        Self {
            api_key: clean_optional(&keys.pubmed_api_key),
            email: clean_optional(&keys.pubmed_email).unwrap_or_else(|| DEFAULT_EMAIL.to_string()),
            tool: clean_optional(&keys.pubmed_tool_name)
                .unwrap_or_else(|| DEFAULT_TOOL.to_string()),
        }
    }
}
