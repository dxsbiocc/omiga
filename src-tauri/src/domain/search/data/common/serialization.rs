use super::parsing::{displayed_link_for_url, truncate_chars};
use super::types::{DataRecord, DataSearchResponse};
use serde_json::{json, Value as Json};

pub fn search_response_to_json(response: &DataSearchResponse) -> Json {
    let results: Vec<Json> = response
        .results
        .iter()
        .enumerate()
        .map(|(idx, item)| record_to_serp_result(item, idx + 1))
        .collect();
    json!({
        "query": response.query,
        "category": "data",
        "source": response.source,
        "effective_source": response.source,
        "total": response.total,
        "route_notes": response.notes,
        "results": results,
    })
}

pub fn detail_to_json(record: &DataRecord) -> Json {
    json!({
        "category": "data",
        "source": record.source.as_str(),
        "effective_source": record.source.as_str(),
        "id": record.id,
        "accession": record.accession,
        "title": record.title,
        "name": record.title,
        "link": record.url,
        "url": record.url,
        "displayed_link": displayed_link_for_url(&record.url),
        "favicon": record.source.favicon(),
        "snippet": data_record_snippet(record),
        "content": data_record_content(record),
        "metadata": record_metadata(record),
    })
}

fn record_to_serp_result(record: &DataRecord, position: usize) -> Json {
    json!({
        "position": position,
        "category": "data",
        "source": record.source.as_str(),
        "title": record.title,
        "name": record.title,
        "link": record.url,
        "url": record.url,
        "displayed_link": displayed_link_for_url(&record.url),
        "favicon": record.source.favicon(),
        "snippet": data_record_snippet(record),
        "id": record.id,
        "accession": record.accession,
        "metadata": record_metadata(record),
    })
}

fn record_metadata(record: &DataRecord) -> Json {
    json!({
        "accession": record.accession,
        "source_label": record.source.label(),
        "record_type": record.record_type,
        "organism": record.organism,
        "published_date": record.published_date,
        "updated_date": record.updated_date,
        "sample_count": record.sample_count,
        "platform": record.platform,
        "files": record.files,
        "source_specific": record.extra,
    })
}

fn data_record_snippet(record: &DataRecord) -> String {
    let mut pieces = Vec::new();
    if let Some(record_type) = record
        .record_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        pieces.push(record_type.to_string());
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        pieces.push(organism.to_string());
    }
    if let Some(samples) = record.sample_count {
        pieces.push(format!("{samples} samples"));
    }
    if !record.summary.trim().is_empty() {
        pieces.push(truncate_chars(&record.summary, 280));
    }
    pieces.join(" | ")
}

fn data_record_content(record: &DataRecord) -> String {
    let mut out = String::new();
    out.push_str(&record.title);
    out.push_str("\n\n");
    out.push_str("Source: ");
    out.push_str(record.source.label());
    out.push('\n');
    if !record.accession.trim().is_empty() {
        out.push_str("Accession: ");
        out.push_str(&record.accession);
        out.push('\n');
    }
    if let Some(record_type) = record
        .record_type
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Record type: ");
        out.push_str(record_type);
        out.push('\n');
    }
    if let Some(organism) = record.organism.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Organism: ");
        out.push_str(organism);
        out.push('\n');
    }
    if let Some(samples) = record.sample_count {
        out.push_str("Samples: ");
        out.push_str(&samples.to_string());
        out.push('\n');
    }
    if let Some(platform) = record.platform.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("Platform: ");
        out.push_str(platform);
        out.push('\n');
    }
    if let Some(date) = record
        .published_date
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str("Published: ");
        out.push_str(date);
        out.push('\n');
    }
    out.push_str("Link: ");
    out.push_str(&record.url);
    if !record.files.is_empty() {
        out.push_str("\nFiles:\n");
        for file in &record.files {
            out.push_str("- ");
            out.push_str(file);
            out.push('\n');
        }
    }
    if !record.summary.trim().is_empty() {
        out.push_str("\nSummary\n");
        out.push_str(record.summary.trim());
    }
    out.trim().to_string()
}
