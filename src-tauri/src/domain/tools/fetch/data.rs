use super::common::{
    clean_nonempty, json_stream, metadata_string_from_result, normalized_source,
    normalized_subcategory, resolve_url, string_from_result,
};
use super::FetchArgs;
use crate::domain::tools::{ToolContext, ToolError};

pub(super) async fn fetch_public_data(
    ctx: &ToolContext,
    args: &FetchArgs,
    source: &str,
) -> Result<crate::infrastructure::streaming::StreamOutputBox, ToolError> {
    let source = crate::domain::search::data::PublicDataSource::parse(source).ok_or_else(|| {
        ToolError::InvalidArguments {
            message: format!("Unsupported public data source: {source}"),
        }
    })?;
    let identifier = resolve_data_identifier(args).ok_or_else(|| ToolError::InvalidArguments {
        message: format!(
            "fetch(category=data, source={}) requires `id`, `url`, accession, or a search `result`",
            source.as_str()
        ),
    })?;
    let client = crate::domain::search::data::PublicDataClient::from_tool_context(ctx)
        .map_err(|message| ToolError::ExecutionFailed { message })?;
    let record = tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
        r = client.fetch(source, &identifier) => r.map_err(|message| ToolError::ExecutionFailed { message })?,
    };
    Ok(json_stream(crate::domain::search::data::detail_to_json(
        &record,
    )))
}

fn data_source_for_subcategory(subcategory: Option<&str>) -> Option<&'static str> {
    match subcategory? {
        "expression" | "gene_expression" | "transcriptomics" | "transcriptome" => Some("geo"),
        "sequencing" | "sequence_reads" | "raw_reads" | "reads" | "sra" => Some("ena_run"),
        "genomics" | "genome" | "genomes" | "assembly" | "assemblies" => Some("ena_assembly"),
        "sample_metadata" | "sample" | "samples" | "metadata" => Some("ena_sample"),
        "multi_omics" | "multiomics" | "projects" | "project" => Some("cbioportal"),
        _ => None,
    }
}

pub(super) fn resolve_data_source(args: &FetchArgs, requested_source: &str) -> String {
    if requested_source != "auto" {
        return requested_source.to_string();
    }
    if let Some(source) = string_from_result(args, &["source", "effective_source"])
        .map(|s| normalized_source(Some(&s)))
        .filter(|s| s != "auto")
    {
        if crate::domain::search::data::PublicDataSource::parse(&source).is_some() {
            return source;
        }
    }
    if let Some(value) = resolve_data_identifier(args) {
        if crate::domain::search::data::looks_like_geo_accession(&value) {
            return "geo".to_string();
        }
        if let Some(source) = crate::domain::search::data::inferred_ena_source_key(&value) {
            return source.to_string();
        }
        if crate::domain::search::data::looks_like_gtex_identifier(&value) {
            return "gtex".to_string();
        }
    }
    if let Some(url) = resolve_url(args) {
        let lower = url.to_ascii_lowercase();
        if lower.contains("ncbi.nlm.nih.gov/geo") || lower.contains("ncbi.nlm.nih.gov/gds") {
            return "geo".to_string();
        }
        if lower.contains("ebi.ac.uk/ena") {
            return "ena".to_string();
        }
        if lower.contains("gtexportal.org") {
            return "gtex".to_string();
        }
    }
    if let Some(source) =
        data_source_for_subcategory(normalized_subcategory(args.subcategory.as_deref()).as_deref())
    {
        return source.to_string();
    }
    "geo".to_string()
}

fn resolve_data_identifier(args: &FetchArgs) -> Option<String> {
    args.id
        .as_deref()
        .and_then(clean_nonempty)
        .or_else(|| {
            metadata_string_from_result(
                args,
                &[
                    "accession",
                    "geo_accession",
                    "ena_accession",
                    "gencodeId",
                    "gencode_id",
                ],
            )
        })
        .or_else(|| string_from_result(args, &["accession", "id"]))
        .or_else(|| resolve_url(args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_data_source_and_identifier() {
        let from_geo_result = FetchArgs {
            category: "data".into(),
            source: None,
            subcategory: None,
            url: None,
            id: None,
            result: Some(json!({
                "source": "geo",
                "accession": "GSE123",
                "metadata": {"accession": "GSE123"}
            })),
            prompt: None,
        };
        assert_eq!(resolve_data_source(&from_geo_result, "auto"), "geo");
        assert_eq!(
            resolve_data_identifier(&from_geo_result).as_deref(),
            Some("GSE123")
        );

        let from_ena_url = FetchArgs {
            category: "data".into(),
            source: None,
            subcategory: None,
            url: Some("https://www.ebi.ac.uk/ena/browser/view/PRJEB123".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_data_source(&from_ena_url, "auto"), "ena");

        let from_geo_url = FetchArgs {
            category: "data".into(),
            source: None,
            subcategory: None,
            url: Some("https://www.ncbi.nlm.nih.gov/geo/query/acc.cgi?acc=GSM575".into()),
            id: None,
            result: None,
            prompt: None,
        };
        assert_eq!(resolve_data_source(&from_geo_url, "auto"), "geo");
    }
}
