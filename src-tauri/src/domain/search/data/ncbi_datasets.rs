//! NCBI Datasets v2 genome dataset-report adapter.
//!
//! This source uses the official NCBI Datasets v2 REST API for genome assembly
//! metadata. It intentionally returns metadata and download links only; it does
//! not download genome packages inside the tool call.

use super::common::*;
use super::PublicDataClient;
use serde_json::{json, Map as JsonMap, Value as Json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenomeLookupMode {
    Accession,
    Taxon,
    BioProject,
    BioSample,
    Wgs,
    AssemblyName,
}

impl GenomeLookupMode {
    fn parse(value: &str) -> Option<Self> {
        match normalize_param_id(value).as_str() {
            "accession" | "accessions" | "assembly_accession" | "assembly_accessions" => {
                Some(Self::Accession)
            }
            "taxon" | "taxons" | "taxonomy" | "organism" | "taxid" | "tax_id" => Some(Self::Taxon),
            "bioproject" | "bio_project" | "bioprojects" | "project" => Some(Self::BioProject),
            "biosample" | "bio_sample" | "biosample_id" | "biosample_ids" | "sample" => {
                Some(Self::BioSample)
            }
            "wgs" | "wgs_accession" | "wgs_accessions" => Some(Self::Wgs),
            "assembly_name" | "assembly_names" | "name" => Some(Self::AssemblyName),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Accession => "accession",
            Self::Taxon => "taxon",
            Self::BioProject => "bioproject",
            Self::BioSample => "biosample",
            Self::Wgs => "wgs",
            Self::AssemblyName => "assembly_name",
        }
    }

    fn endpoint(self, value: &str) -> String {
        let value = encode_path_segment(value);
        match self {
            Self::Accession => format!("genome/accession/{value}/dataset_report"),
            Self::Taxon => format!("genome/taxon/{value}/dataset_report"),
            Self::BioProject => format!("genome/bioproject/{value}/dataset_report"),
            Self::BioSample => format!("genome/biosample/{value}/dataset_report"),
            Self::Wgs => format!("genome/wgs/{value}/dataset_report"),
            Self::AssemblyName => format!("genome/assembly_name/{value}/dataset_report"),
        }
    }
}

impl PublicDataClient {
    pub(in crate::domain::search::data) async fn search_ncbi_datasets(
        &self,
        args: DataSearchArgs,
    ) -> Result<DataSearchResponse, String> {
        let mode = ncbi_datasets_mode(&args).unwrap_or_else(|| infer_mode_from_query(&args.query));
        let lookup = ncbi_datasets_lookup_value(&args, mode)
            .ok_or_else(|| format!("NCBI Datasets {} search requires a query", mode.as_str()))?;
        let mut params = ncbi_datasets_query_params(&args);
        params.push((
            "page_size".to_string(),
            args.normalized_max_results().to_string(),
        ));
        let json = self
            .get_ncbi_datasets_json(&mode.endpoint(&lookup), &params)
            .await?;
        let results = parse_ncbi_datasets_report_page(&json);
        let mut notes = vec![format!(
            "NCBI Datasets v2 REST API genome dataset_report ({})",
            mode.as_str()
        )];
        if self.settings.api_key.is_some() {
            notes.push(
                "Using the configured NCBI API key as an api-key header; it is the same My NCBI key used by E-utilities."
                    .to_string(),
            );
        } else {
            notes.push(
                "No NCBI API key configured; Datasets v2 and E-utilities share the same optional My NCBI key for higher rate limits."
                    .to_string(),
            );
        }
        if !params.iter().any(|(key, _)| key == "filters.search_text")
            && matches!(mode, GenomeLookupMode::Taxon)
        {
            notes.push(
                "For narrower organism searches, pass params.search_text, reference_only, assembly_level, or assembly_source."
                    .to_string(),
            );
        }
        Ok(DataSearchResponse {
            query: args.query.trim().to_string(),
            source: PublicDataSource::NcbiDatasets.as_str().to_string(),
            total: json
                .get("total_count")
                .and_then(json_u64_from_string_or_number),
            results,
            notes,
        })
    }

    pub(in crate::domain::search::data) async fn fetch_ncbi_datasets(
        &self,
        identifier: &str,
    ) -> Result<DataRecord, String> {
        let accession = normalize_ncbi_datasets_accession(identifier).ok_or_else(|| {
            format!(
                "NCBI Datasets fetch requires a genome assembly accession (GCA_/GCF_), got `{identifier}`"
            )
        })?;
        let params = vec![("page_size".to_string(), "1".to_string())];
        let json = self
            .get_ncbi_datasets_json(&GenomeLookupMode::Accession.endpoint(&accession), &params)
            .await?;
        parse_ncbi_datasets_report_page(&json)
            .into_iter()
            .next()
            .ok_or_else(|| format!("NCBI Datasets returned no genome report for `{accession}`"))
    }

    pub(crate) async fn ncbi_datasets_download_summary(
        &self,
        identifier: &str,
        params: Option<&Json>,
    ) -> Result<Json, String> {
        let accession = normalize_ncbi_datasets_accession(identifier).ok_or_else(|| {
            format!(
                "NCBI Datasets download_summary requires a genome assembly accession (GCA_/GCF_), got `{identifier}`"
            )
        })?;
        let query_params = ncbi_datasets_download_summary_params(params);
        let json = self
            .get_ncbi_datasets_json(
                &format!(
                    "genome/accession/{}/download_summary",
                    encode_path_segment(&accession)
                ),
                &query_params,
            )
            .await?;
        Ok(ncbi_datasets_download_summary_to_json(
            &accession,
            &json,
            &query_params,
        ))
    }

    async fn get_ncbi_datasets_json(
        &self,
        endpoint: &str,
        params: &[(String, String)],
    ) -> Result<Json, String> {
        #[cfg(debug_assertions)]
        if self.base_urls.ncbi_datasets == "mock://ncbi_datasets" {
            return mock_ncbi_datasets_json(endpoint).ok_or_else(|| {
                format!("debug NCBI Datasets mock has no fixture for endpoint `{endpoint}`")
            });
        }

        let mut request = self
            .http
            .get(format!(
                "{}/{}",
                self.base_urls.ncbi_datasets,
                endpoint.trim_start_matches('/')
            ))
            .header(reqwest::header::ACCEPT, "application/json")
            .query(params);
        if let Some(api_key) = &self.settings.api_key {
            request = request.header("api-key", api_key);
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("NCBI Datasets API {endpoint} request failed: {e}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("read NCBI Datasets API {endpoint} response: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "NCBI Datasets API {endpoint} returned HTTP {}: {}",
                status.as_u16(),
                truncate_for_error(&body)
            ));
        }
        let json: Json = serde_json::from_str(&body).map_err(|e| {
            format!(
                "parse NCBI Datasets API {endpoint} JSON: {e}; body: {}",
                truncate_for_error(&body)
            )
        })?;
        if let Some(error) = json.get("error").and_then(json_string) {
            return Err(format!("NCBI Datasets API {endpoint} error: {error}"));
        }
        if let Some(errors) = json.get("errors").and_then(Json::as_array) {
            if !errors.is_empty() {
                return Err(format!("NCBI Datasets API {endpoint} errors: {errors:?}"));
            }
        }
        Ok(json)
    }
}

pub fn looks_like_ncbi_datasets_accession(value: &str) -> bool {
    normalize_ncbi_datasets_accession(value).is_some()
}

pub(in crate::domain::search::data) fn parse_ncbi_datasets_report_page(
    value: &Json,
) -> Vec<DataRecord> {
    value
        .get("reports")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_ncbi_datasets_report)
        .collect()
}

fn parse_ncbi_datasets_report(report: &Json) -> Option<DataRecord> {
    let accession = json_path_string(report, &["accession"])
        .or_else(|| json_path_string(report, &["current_accession"]))?;
    let current_accession = json_path_string(report, &["current_accession"]);
    let organism = json_path_string(report, &["organism", "organism_name"]);
    let common_name = json_path_string(report, &["organism", "common_name"]);
    let tax_id = json_path_string(report, &["organism", "tax_id"]);
    let assembly_name = json_path_string(report, &["assembly_info", "assembly_name"]);
    let assembly_level = json_path_string(report, &["assembly_info", "assembly_level"]);
    let assembly_status = json_path_string(report, &["assembly_info", "assembly_status"]);
    let description = json_path_string(report, &["assembly_info", "description"]);
    let release_date = json_path_string(report, &["assembly_info", "release_date"]);
    let submitter = json_path_string(report, &["assembly_info", "submitter"]);
    let refseq_category = json_path_string(report, &["assembly_info", "refseq_category"]);
    let bioproject = json_path_string(report, &["assembly_info", "bioproject_accession"]);
    let annotation_release = json_path_string(report, &["annotation_info", "release_date"]);
    let annotation_provider = json_path_string(report, &["annotation_info", "provider"]);
    let sequence_length = json_path_string(report, &["assembly_stats", "total_sequence_length"]);

    let title = match (organism.as_deref(), assembly_name.as_deref()) {
        (Some(organism), Some(assembly)) => format!("{organism} {assembly}"),
        (Some(organism), None) => organism.to_string(),
        (None, Some(assembly)) => assembly.to_string(),
        (None, None) => accession.clone(),
    };
    let mut summary_pieces = Vec::new();
    push_piece(&mut summary_pieces, description.as_deref());
    push_piece_labeled(&mut summary_pieces, "Level", assembly_level.as_deref());
    push_piece_labeled(&mut summary_pieces, "Status", assembly_status.as_deref());
    push_piece_labeled(
        &mut summary_pieces,
        "RefSeq category",
        refseq_category.as_deref(),
    );
    push_piece_labeled(&mut summary_pieces, "BioProject", bioproject.as_deref());
    push_piece_labeled(&mut summary_pieces, "Submitter", submitter.as_deref());
    push_piece_labeled(
        &mut summary_pieces,
        "Sequence length",
        sequence_length.as_deref(),
    );
    push_piece_labeled(
        &mut summary_pieces,
        "Annotation",
        annotation_provider.as_deref(),
    );

    let url = ncbi_datasets_record_url(&accession);
    let download_url = ncbi_datasets_download_url(&accession);
    let download_summary_url = ncbi_datasets_download_summary_url(&accession);
    let mut files = Vec::new();
    if let Some(report_url) = json_path_string(report, &["annotation_info", "report_url"]) {
        files.push(report_url);
    }
    if let Some(blast_url) = json_path_string(report, &["assembly_info", "blast_url"]) {
        files.push(blast_url);
    }
    files.push(download_summary_url.clone());
    files.push(download_url.clone());

    let mut extra = JsonMap::new();
    insert_extra(&mut extra, "current_accession", current_accession);
    insert_extra(
        &mut extra,
        "paired_accession",
        json_path_string(report, &["paired_accession"]),
    );
    insert_extra(
        &mut extra,
        "source_database",
        json_path_string(report, &["source_database"]),
    );
    insert_extra(&mut extra, "tax_id", tax_id);
    insert_extra(&mut extra, "common_name", common_name);
    insert_extra(&mut extra, "assembly_name", assembly_name);
    insert_extra(&mut extra, "assembly_level", assembly_level);
    insert_extra(&mut extra, "assembly_status", assembly_status);
    insert_extra(
        &mut extra,
        "assembly_type",
        json_path_string(report, &["assembly_info", "assembly_type"]),
    );
    insert_extra(&mut extra, "bioproject_accession", bioproject);
    insert_extra(&mut extra, "refseq_category", refseq_category);
    insert_extra(
        &mut extra,
        "synonym",
        json_path_string(report, &["assembly_info", "synonym"]),
    );
    insert_extra(&mut extra, "submitter", submitter);
    insert_extra(&mut extra, "total_sequence_length", sequence_length);
    insert_extra(
        &mut extra,
        "total_number_of_chromosomes",
        json_path_string(report, &["assembly_stats", "total_number_of_chromosomes"]),
    );
    insert_extra(
        &mut extra,
        "number_of_scaffolds",
        json_path_string(report, &["assembly_stats", "number_of_scaffolds"]),
    );
    insert_extra(
        &mut extra,
        "annotation_name",
        json_path_string(report, &["annotation_info", "name"]),
    );
    insert_extra(&mut extra, "annotation_provider", annotation_provider);
    insert_extra(
        &mut extra,
        "annotation_release_date",
        annotation_release.clone(),
    );
    extra.insert(
        "download_summary_url".to_string(),
        Json::String(download_summary_url),
    );
    extra.insert(
        "download_package_url".to_string(),
        Json::String(download_url),
    );

    Some(DataRecord {
        id: accession.clone(),
        accession,
        source: PublicDataSource::NcbiDatasets,
        title,
        summary: summary_pieces.join(" | "),
        url,
        record_type: Some("genome_assembly".to_string()),
        organism,
        published_date: release_date,
        updated_date: annotation_release,
        sample_count: None,
        platform: None,
        files,
        extra,
    })
}

fn ncbi_datasets_mode(args: &DataSearchArgs) -> Option<GenomeLookupMode> {
    param_string(
        args.params.as_ref(),
        &["mode", "endpoint", "lookup", "by", "kind", "type"],
    )
    .and_then(|value| GenomeLookupMode::parse(&value))
}

fn infer_mode_from_query(query: &str) -> GenomeLookupMode {
    let value = query.trim();
    if looks_like_ncbi_datasets_accession(value) {
        return GenomeLookupMode::Accession;
    }
    let upper = value.to_ascii_uppercase();
    if upper.starts_with("PRJN") || upper.starts_with("PRJE") || upper.starts_with("PRJD") {
        return GenomeLookupMode::BioProject;
    }
    if upper.starts_with("SAMN") || upper.starts_with("SAMEA") || upper.starts_with("SAMD") {
        return GenomeLookupMode::BioSample;
    }
    GenomeLookupMode::Taxon
}

fn ncbi_datasets_lookup_value(args: &DataSearchArgs, mode: GenomeLookupMode) -> Option<String> {
    let keys = match mode {
        GenomeLookupMode::Accession => &["accession", "accessions", "assembly_accession", "id"][..],
        GenomeLookupMode::Taxon => &["taxon", "taxons", "taxid", "tax_id", "organism"],
        GenomeLookupMode::BioProject => &["bioproject", "bio_project", "project"],
        GenomeLookupMode::BioSample => &["biosample", "bio_sample", "biosample_id", "sample"],
        GenomeLookupMode::Wgs => &["wgs", "wgs_accession"],
        GenomeLookupMode::AssemblyName => &["assembly_name", "assembly_names", "name"],
    };
    let raw =
        param_string(args.params.as_ref(), keys).or_else(|| normalize_lookup_text(&args.query));
    if matches!(mode, GenomeLookupMode::Accession) {
        return raw.and_then(|value| normalize_ncbi_datasets_accession(&value));
    }
    raw
}

fn normalize_lookup_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn ncbi_datasets_query_params(args: &DataSearchArgs) -> Vec<(String, String)> {
    let mut params = Vec::new();
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.reference_only",
        &["reference_only", "referenceOnly"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.assembly_source",
        &["assembly_source", "assemblySource"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.has_annotation",
        &["has_annotation", "hasAnnotation", "annotated"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.exclude_paired_reports",
        &["exclude_paired_reports", "excludePairedReports"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.exclude_atypical",
        &["exclude_atypical", "excludeAtypical"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.assembly_version",
        &["assembly_version", "assemblyVersion"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.first_release_date",
        &["first_release_date", "firstReleaseDate", "released_after"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.last_release_date",
        &["last_release_date", "lastReleaseDate", "released_before"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.search_text",
        &["search_text", "searchText", "text_filter", "filter"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.is_metagenome_derived",
        &["is_metagenome_derived", "metagenome"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.is_type_material",
        &["is_type_material", "type_material"],
    );
    push_repeated_param_alias(
        &mut params,
        args.params.as_ref(),
        "filters.assembly_level",
        &["assembly_level", "assemblyLevel", "levels"],
    );
    push_repeated_param_alias(
        &mut params,
        args.params.as_ref(),
        "chromosomes",
        &["chromosomes", "chromosome"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "tax_exact_match",
        &["tax_exact_match", "taxExactMatch"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "returned_content",
        &["returned_content", "returnedContent"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "page_token",
        &["page_token", "pageToken"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "sort.field",
        &["sort_field", "sortField"],
    );
    push_param_alias(
        &mut params,
        args.params.as_ref(),
        "sort.direction",
        &["sort_direction", "sortDirection"],
    );
    params
}

fn ncbi_datasets_download_summary_params(params: Option<&Json>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for value in param_string_list(
        params,
        &[
            "include_annotation_type",
            "includeAnnotationType",
            "include",
            "files",
        ],
    ) {
        out.push((
            "include_annotation_type".to_string(),
            normalize_annotation_type(&value),
        ));
    }
    push_repeated_param_alias(
        &mut out,
        params,
        "chromosomes",
        &["chromosomes", "chromosome"],
    );
    out
}

fn ncbi_datasets_download_summary_to_json(
    accession: &str,
    summary: &Json,
    query_params: &[(String, String)],
) -> Json {
    let record_url = ncbi_datasets_record_url(accession);
    let available_files = available_file_rows(summary);
    let include_annotation_type = query_params
        .iter()
        .filter(|(key, _)| key == "include_annotation_type")
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    let chromosomes = query_params
        .iter()
        .filter(|(key, _)| key == "chromosomes")
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    json!({
        "category": "data",
        "source": PublicDataSource::NcbiDatasets.as_str(),
        "effective_source": PublicDataSource::NcbiDatasets.as_str(),
        "operation": "download_summary",
        "id": accession,
        "accession": accession,
        "title": format!("NCBI Datasets download summary for {accession}"),
        "name": format!("NCBI Datasets download summary for {accession}"),
        "link": record_url,
        "url": record_url,
        "displayed_link": displayed_link_for_url(&record_url),
        "favicon": PublicDataSource::NcbiDatasets.favicon(),
        "record_count": summary.get("record_count").and_then(json_u64_from_string_or_number),
        "resource_updated_on": json_path_string(summary, &["resource_updated_on"]),
        "hydrated": summary.get("hydrated").cloned().unwrap_or(Json::Null),
        "dehydrated": summary.get("dehydrated").cloned().unwrap_or(Json::Null),
        "available_files": available_files,
        "requested": {
            "include_annotation_type": include_annotation_type,
            "chromosomes": chromosomes,
        },
        "download_summary_url": ncbi_datasets_download_summary_url(accession),
        "content": download_summary_content(accession, summary),
        "metadata": {
            "source_label": PublicDataSource::NcbiDatasets.label(),
            "api": "NCBI Datasets v2 REST API",
            "api_key": "Uses the same optional My NCBI API key as E-utilities when configured.",
            "does_not_download": true,
            "source_specific": summary,
        }
    })
}

fn available_file_rows(summary: &Json) -> Vec<Json> {
    let Some(files) = summary.get("available_files").and_then(Json::as_object) else {
        return Vec::new();
    };
    let mut rows = files
        .iter()
        .map(|(name, value)| {
            json!({
                "name": name,
                "file_count": value.get("file_count").and_then(json_u64_from_string_or_number),
                "size_mb": value.get("size_mb").and_then(Json::as_f64),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        a.get("name")
            .and_then(Json::as_str)
            .cmp(&b.get("name").and_then(Json::as_str))
    });
    rows
}

fn download_summary_content(accession: &str, summary: &Json) -> String {
    let mut out = Vec::new();
    out.push(format!("NCBI Datasets download summary for {accession}"));
    if let Some(count) = summary
        .get("record_count")
        .and_then(json_u64_from_string_or_number)
    {
        out.push(format!("Record count: {count}"));
    }
    if let Some(updated) = json_path_string(summary, &["resource_updated_on"]) {
        out.push(format!("Resource updated: {updated}"));
    }
    if let Some(size) = json_path_string(summary, &["hydrated", "estimated_file_size_mb"]) {
        out.push(format!("Hydrated estimate: {size} MB"));
    }
    if let Some(command) = json_path_string(summary, &["hydrated", "cli_download_command_line"]) {
        out.push(format!("Hydrated CLI: {command}"));
    }
    if let Some(size) = json_path_string(summary, &["dehydrated", "estimated_file_size_mb"]) {
        out.push(format!("Dehydrated estimate: {size} MB"));
    }
    if let Some(command) = json_path_string(summary, &["dehydrated", "cli_download_command_line"]) {
        out.push(format!("Dehydrated CLI: {command}"));
    }
    let files = available_file_rows(summary);
    if !files.is_empty() {
        out.push("Available files:".to_string());
        for file in files {
            let name = file.get("name").and_then(Json::as_str).unwrap_or("unknown");
            let count = file
                .get("file_count")
                .and_then(Json::as_u64)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string());
            let size = file
                .get("size_mb")
                .and_then(Json::as_f64)
                .map(|v| format!("{v:.2} MB"))
                .unwrap_or_else(|| "? MB".to_string());
            out.push(format!("- {name}: {count} files, {size}"));
        }
    }
    out.join("\n")
}

fn normalize_annotation_type(value: &str) -> String {
    let normalized = normalize_param_id(value);
    let mapped = match normalized.as_str() {
        "genome" | "genome_fasta" | "fasta" | "all_genomic_fasta" => "GENOME_FASTA",
        "gff" | "gff3" | "genome_gff" => "GENOME_GFF",
        "gbff" | "genbank" | "genome_gbff" => "GENOME_GBFF",
        "gtf" | "genome_gtf" => "GENOME_GTF",
        "rna" | "rna_fasta" => "RNA_FASTA",
        "protein" | "proteins" | "prot" | "prot_fasta" | "protein_fasta" => "PROT_FASTA",
        "cds" | "cds_fasta" => "CDS_FASTA",
        "sequence_report" | "seq_report" => "SEQUENCE_REPORT",
        other => {
            if other.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return other.to_ascii_uppercase();
            }
            return value.trim().to_string();
        }
    };
    mapped.to_string()
}

fn push_param_alias(
    out: &mut Vec<(String, String)>,
    params: Option<&Json>,
    target: &str,
    keys: &[&str],
) {
    if let Some(value) = param_string(params, keys) {
        out.push((target.to_string(), value));
    }
}

fn push_repeated_param_alias(
    out: &mut Vec<(String, String)>,
    params: Option<&Json>,
    target: &str,
    keys: &[&str],
) {
    for value in param_string_list(params, keys) {
        out.push((target.to_string(), value));
    }
}

fn param_string(params: Option<&Json>, keys: &[&str]) -> Option<String> {
    let object = params?.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(json_param_string))
}

fn param_string_list(params: Option<&Json>, keys: &[&str]) -> Vec<String> {
    let Some(object) = params.and_then(Json::as_object) else {
        return Vec::new();
    };
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        if let Some(array) = value.as_array() {
            let values = array
                .iter()
                .filter_map(json_param_string)
                .collect::<Vec<_>>();
            if !values.is_empty() {
                return values;
            }
        }
        if let Some(value) = json_param_string(value) {
            let values = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !values.is_empty() {
                return values;
            }
        }
    }
    Vec::new()
}

fn json_param_string(value: &Json) -> Option<String> {
    match value {
        Json::Bool(value) => Some(value.to_string()),
        _ => json_string(value),
    }
}

fn normalize_ncbi_datasets_accession(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(value) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.contains("ncbi.nlm.nih.gov") || host.contains("api.ncbi.nlm.nih.gov") {
            if let Some(accession) = parsed
                .path_segments()
                .into_iter()
                .flatten()
                .find_map(assembly_accession_from_text)
            {
                return Some(accession);
            }
            for (_, val) in parsed.query_pairs() {
                if let Some(accession) = assembly_accession_from_text(&val) {
                    return Some(accession);
                }
            }
        }
    }
    assembly_accession_from_text(value)
}

fn assembly_accession_from_text(value: &str) -> Option<String> {
    lazy_static::lazy_static! {
        static ref RE_ASSEMBLY: regex::Regex = regex::Regex::new(r#"(?i)\bGC[AF]_\d+(?:\.\d+)?\b"#).unwrap();
    }
    RE_ASSEMBLY
        .find(value)
        .map(|m| m.as_str().to_ascii_uppercase())
}

fn json_path_string(value: &Json, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    json_string(current)
}

fn push_piece(out: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(value.to_string());
    }
}

fn push_piece_labeled(out: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|s| !s.is_empty()) {
        out.push(format!("{label}: {value}"));
    }
}

fn insert_extra(map: &mut JsonMap<String, Json>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
        map.insert(key.to_string(), Json::String(value));
    }
}

fn normalize_param_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn ncbi_datasets_record_url(accession: &str) -> String {
    format!("https://www.ncbi.nlm.nih.gov/datasets/genome/{accession}/")
}

fn ncbi_datasets_download_summary_url(accession: &str) -> String {
    format!(
        "https://api.ncbi.nlm.nih.gov/datasets/v2/genome/accession/{}/download_summary",
        encode_path_segment(accession)
    )
}

fn ncbi_datasets_download_url(accession: &str) -> String {
    format!(
        "https://api.ncbi.nlm.nih.gov/datasets/v2/genome/accession/{}/download?hydrated=DATA_REPORT_ONLY&filename=ncbi_dataset.zip",
        encode_path_segment(accession)
    )
}

#[cfg(debug_assertions)]
fn mock_ncbi_datasets_json(endpoint: &str) -> Option<Json> {
    if endpoint.contains("GCF_000001405.40") && endpoint.ends_with("/download_summary") {
        return Some(json!({
            "record_count": 1,
            "resource_updated_on": "2026-05-01T05:15:00Z",
            "hydrated": {
                "estimated_file_size_mb": 1002,
                "url": "https://api.ncbi.nlm.nih.gov/datasets/v2/genome/accession/GCF_000001405.40/download?include_annotation_type=GENOME_FASTA&include_annotation_type=GENOME_GFF&hydrated=FULLY_HYDRATED",
                "cli_download_command_line": "datasets download genome accession GCF_000001405.40 --include gff3,genome"
            },
            "dehydrated": {
                "estimated_file_size_mb": 1,
                "url": "https://api.ncbi.nlm.nih.gov/datasets/v2/genome/accession/GCF_000001405.40/download?include_annotation_type=GENOME_FASTA&include_annotation_type=GENOME_GFF&hydrated=DATA_REPORT_ONLY",
                "cli_download_command_line": "datasets download genome accession GCF_000001405.40 --include gff3,genome --dehydrated",
                "cli_rehydrate_command_line": "datasets rehydrate --help"
            },
            "available_files": {
                "all_genomic_fasta": {"file_count": 1, "size_mb": 927.82837},
                "genome_gff": {"file_count": 1, "size_mb": 74.56825},
                "sequence_report": {"file_count": 1, "size_mb": 0.07672691}
            }
        }));
    }
    if endpoint.contains("GCF_000001405.40") || endpoint.contains("/taxon/9606/") {
        return Some(json!({
            "reports": [{
                "accession": "GCF_000001405.40",
                "current_accession": "GCF_000001405.40",
                "paired_accession": "GCA_000001405.29",
                "source_database": "SOURCE_DATABASE_REFSEQ",
                "organism": {
                    "tax_id": 9606,
                    "organism_name": "Homo sapiens",
                    "common_name": "human"
                },
                "assembly_info": {
                    "assembly_level": "Chromosome",
                    "assembly_status": "current",
                    "assembly_name": "GRCh38.p14",
                    "assembly_type": "haploid-with-alt-loci",
                    "bioproject_accession": "PRJNA31257",
                    "release_date": "2022-02-03",
                    "description": "Genome Reference Consortium Human Build 38 patch release 14 (GRCh38.p14)",
                    "submitter": "Genome Reference Consortium",
                    "refseq_category": "reference genome",
                    "synonym": "hg38",
                    "blast_url": "https://blast.ncbi.nlm.nih.gov/Blast.cgi?BLAST_SPEC=GDH_GCF_000001405.40"
                },
                "assembly_stats": {
                    "total_number_of_chromosomes": 24,
                    "total_sequence_length": "3099441038",
                    "number_of_scaffolds": 470
                },
                "annotation_info": {
                    "name": "GCF_000001405.40-RS_2025_08",
                    "provider": "NCBI RefSeq",
                    "release_date": "2025-08-01",
                    "report_url": "https://www.ncbi.nlm.nih.gov/genome/annotation_euk/Homo_sapiens/GCF_000001405.40-RS_2025_08.html"
                }
            }],
            "total_count": 1
        }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_genome_dataset_report() {
        let json = json!({
            "reports": [{
                "accession": "GCF_000001405.40",
                "organism": {"tax_id": 9606, "organism_name": "Homo sapiens"},
                "assembly_info": {
                    "assembly_name": "GRCh38.p14",
                    "assembly_level": "Chromosome",
                    "release_date": "2022-02-03",
                    "bioproject_accession": "PRJNA31257",
                    "description": "Human reference genome"
                },
                "assembly_stats": {"total_sequence_length": "3099441038"},
                "annotation_info": {"provider": "NCBI RefSeq", "release_date": "2025-08-01"}
            }],
            "total_count": 1
        });
        let records = parse_ncbi_datasets_report_page(&json);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, PublicDataSource::NcbiDatasets);
        assert_eq!(records[0].accession, "GCF_000001405.40");
        assert!(records[0].title.contains("GRCh38.p14"));
        assert!(records[0].summary.contains("BioProject: PRJNA31257"));
        assert!(records[0]
            .extra
            .get("download_package_url")
            .and_then(Json::as_str)
            .is_some_and(|url| url.contains("hydrated=DATA_REPORT_ONLY")));
    }

    #[test]
    fn recognizes_ncbi_datasets_accessions_and_urls() {
        assert!(looks_like_ncbi_datasets_accession("GCF_000001405.40"));
        assert!(looks_like_ncbi_datasets_accession(
            "https://www.ncbi.nlm.nih.gov/datasets/genome/GCA_000001405.29/"
        ));
        assert_eq!(
            normalize_ncbi_datasets_accession(
                "https://api.ncbi.nlm.nih.gov/datasets/v2/genome/accession/GCF_000001405.40/dataset_report"
            )
            .as_deref(),
            Some("GCF_000001405.40")
        );
    }

    #[test]
    fn infers_lookup_modes() {
        assert_eq!(
            infer_mode_from_query("GCF_000001405.40"),
            GenomeLookupMode::Accession
        );
        assert_eq!(
            infer_mode_from_query("PRJNA31257"),
            GenomeLookupMode::BioProject
        );
        assert_eq!(
            infer_mode_from_query("SAMN123"),
            GenomeLookupMode::BioSample
        );
        assert_eq!(
            infer_mode_from_query("Homo sapiens"),
            GenomeLookupMode::Taxon
        );
    }
}
