use crate::domain::search::data::common::{
    clean_html_text, json_number_string, json_u64_from_keys, string_field_any, DataRecord,
    PublicDataSource,
};
use serde_json::{Map as JsonMap, Value as Json};

pub(super) fn parse_gtex_genes_json(value: &Json) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(parse_gtex_gene_item)
        .collect()
}

fn parse_gtex_gene_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let gencode_id = string_field_any(map, &["gencodeId", "gencode_id", "geneId", "gene_id"])?;
    let gene_symbol =
        string_field_any(map, &["geneSymbol", "gene_symbol"]).unwrap_or_else(|| gencode_id.clone());
    let description =
        string_field_any(map, &["description", "geneDescription"]).unwrap_or_default();
    let gene_type = string_field_any(map, &["geneType", "gene_type"]);
    let chromosome = string_field_any(map, &["chromosome"]);
    let start = json_u64_from_keys(map, &["start"]);
    let end = json_u64_from_keys(map, &["end"]);
    let title = if gene_symbol == gencode_id {
        gencode_id.clone()
    } else {
        format!("{gene_symbol} ({gencode_id})")
    };
    let mut summary_parts = Vec::new();
    if let Some(gene_type) = gene_type.as_deref().filter(|s| !s.is_empty()) {
        summary_parts.push(gene_type.to_string());
    }
    if let Some(chromosome) = chromosome.as_deref().filter(|s| !s.is_empty()) {
        let location = match (start, end) {
            (Some(start), Some(end)) => format!("{chromosome}:{start}-{end}"),
            _ => chromosome.to_string(),
        };
        summary_parts.push(location);
    }
    if !description.trim().is_empty() {
        summary_parts.push(description.clone());
    }

    Some(DataRecord {
        id: gencode_id.clone(),
        accession: gencode_id.clone(),
        source: PublicDataSource::Gtex,
        title: clean_html_text(&title),
        summary: clean_html_text(&summary_parts.join(" | ")),
        url: gtex_gene_url(&gencode_id),
        record_type: Some("gene".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: None,
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

pub(super) fn parse_gtex_median_expression_json(value: &Json) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(parse_gtex_median_expression_item)
        .collect()
}

fn parse_gtex_median_expression_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let gencode_id = string_field_any(map, &["gencodeId", "gencode_id"])?;
    let gene_symbol =
        string_field_any(map, &["geneSymbol", "gene_symbol"]).unwrap_or_else(|| gencode_id.clone());
    let tissue = string_field_any(map, &["tissueSiteDetailId", "tissue_site_detail_id"])
        .unwrap_or_else(|| "all_tissues".to_string());
    let median = json_number_string(map.get("median")?).unwrap_or_else(|| "NA".to_string());
    let unit = string_field_any(map, &["unit"]).unwrap_or_else(|| "TPM".to_string());
    Some(DataRecord {
        id: format!("{gencode_id}:{tissue}"),
        accession: gencode_id.clone(),
        source: PublicDataSource::Gtex,
        title: format!("{gene_symbol} median expression in {tissue}"),
        summary: format!("median {median} {unit}"),
        url: gtex_gene_url(&gencode_id),
        record_type: Some("median_gene_expression".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: Some(unit),
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

pub(super) fn parse_gtex_tissues_json(value: &Json) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(parse_gtex_tissue_item)
        .collect()
}

fn parse_gtex_tissue_item(item: &Json) -> Option<DataRecord> {
    let map = item.as_object()?;
    let tissue_id = string_field_any(map, &["tissueSiteDetailId", "tissue_site_detail_id"])?;
    let tissue_name = string_field_any(map, &["tissueSiteDetail", "tissue_site_detail"])
        .unwrap_or_else(|| tissue_id.clone());
    let tissue_site = string_field_any(map, &["tissueSite", "tissue_site"]);
    let sample_count = json_u64_from_keys(
        map,
        &[
            "rnaSeqSampleCount",
            "rna_seq_sample_count",
            "sampleCount",
            "sample_count",
        ],
    );
    Some(DataRecord {
        id: tissue_id.clone(),
        accession: tissue_id.clone(),
        source: PublicDataSource::Gtex,
        title: tissue_name,
        summary: tissue_site.unwrap_or_else(|| "GTEx tissue".to_string()),
        url: gtex_tissue_url(&tissue_id),
        record_type: Some("tissue".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count,
        platform: None,
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

pub(super) fn parse_gtex_top_expressed_json(value: &Json, tissue: &str) -> Vec<DataRecord> {
    gtex_data_items(value)
        .into_iter()
        .filter_map(|item| parse_gtex_top_expressed_item(item, tissue))
        .collect()
}

fn parse_gtex_top_expressed_item(item: &Json, fallback_tissue: &str) -> Option<DataRecord> {
    let map = item.as_object()?;
    let gencode_id = string_field_any(map, &["gencodeId", "gencode_id", "geneId", "gene_id"])?;
    let gene_symbol =
        string_field_any(map, &["geneSymbol", "gene_symbol"]).unwrap_or_else(|| gencode_id.clone());
    let tissue = string_field_any(map, &["tissueSiteDetailId", "tissue_site_detail_id"])
        .unwrap_or_else(|| fallback_tissue.to_string());
    let median = map
        .get("median")
        .and_then(json_number_string)
        .unwrap_or_else(|| "NA".to_string());
    let unit = string_field_any(map, &["unit"]).unwrap_or_else(|| "TPM".to_string());
    Some(DataRecord {
        id: format!("{gencode_id}:{tissue}"),
        accession: gencode_id.clone(),
        source: PublicDataSource::Gtex,
        title: format!("{gene_symbol} top expression in {tissue}"),
        summary: format!("median {median} {unit}"),
        url: gtex_gene_url(&gencode_id),
        record_type: Some("top_expressed_gene".to_string()),
        organism: Some("Homo sapiens".to_string()),
        published_date: None,
        updated_date: None,
        sample_count: None,
        platform: Some(unit),
        files: Vec::new(),
        extra: gtex_extra_from_map(map),
    })
}

fn gtex_data_items(value: &Json) -> Vec<&Json> {
    if let Some(items) = value.get("data").and_then(Json::as_array) {
        return items.iter().collect();
    }
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    Vec::new()
}

pub(super) fn gtex_total(value: &Json) -> Option<u64> {
    let paging = value
        .get("paging_info")
        .or_else(|| value.get("pagingInfo"))
        .and_then(Json::as_object)?;
    json_u64_from_keys(
        paging,
        &["totalNumberOfItems", "total_number_of_items", "total"],
    )
}

fn gtex_extra_from_map(map: &JsonMap<String, Json>) -> JsonMap<String, Json> {
    let mut extra = JsonMap::new();
    for key in [
        "chromosome",
        "dataSource",
        "description",
        "end",
        "entrezGeneId",
        "gencodeId",
        "gencodeVersion",
        "geneStatus",
        "geneSymbol",
        "geneSymbolUpper",
        "geneType",
        "genomeBuild",
        "median",
        "ontologyId",
        "start",
        "strand",
        "tissueSite",
        "tissueSiteDetail",
        "tissueSiteDetailId",
        "tss",
        "unit",
    ] {
        if let Some(value) = map.get(key) {
            extra.insert(key.to_string(), value.clone());
        }
    }
    extra
}

pub(super) fn gtex_gene_url(gencode_id: &str) -> String {
    format!("https://gtexportal.org/home/gene/{gencode_id}")
}

pub(super) fn gtex_tissue_url(tissue_id: &str) -> String {
    format!("https://gtexportal.org/home/tissue/{tissue_id}")
}
