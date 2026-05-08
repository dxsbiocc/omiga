//! OpenAlex Works response parser.

use super::{clean_html_text, normalize_doi, LiteraturePaper, PublicLiteratureSource};
use serde_json::{json, Map as JsonMap, Value as Json};

pub fn parse_openalex_json(root: &Json) -> Vec<LiteraturePaper> {
    let items = root
        .get("results")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    items.iter().filter_map(parse_openalex_item).collect()
}

pub(in crate::domain::search::literature) fn parse_openalex_item(
    item: &Json,
) -> Option<LiteraturePaper> {
    let title = item
        .get("title")
        .or_else(|| item.get("display_name"))
        .and_then(Json::as_str)
        .map(clean_html_text)?;
    if title.is_empty() {
        return None;
    }
    let openalex_id = item.get("id").and_then(Json::as_str).unwrap_or_default();
    let id = openalex_id
        .trim_start_matches("https://openalex.org/")
        .to_string();
    let doi = item
        .get("doi")
        .and_then(Json::as_str)
        .map(normalize_doi)
        .filter(|s| !s.is_empty());
    let primary_location = item.get("primary_location");
    let url = primary_location
        .and_then(|v| v.get("landing_page_url"))
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| doi.as_ref().map(|d| format!("https://doi.org/{d}")))
        .unwrap_or_else(|| openalex_id.to_string());
    let pdf_url = primary_location
        .and_then(|v| v.get("pdf_url"))
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.pointer("/open_access/oa_url")
                .and_then(Json::as_str)
                .map(str::to_string)
        });
    let authors = item
        .get("authorships")
        .and_then(Json::as_array)
        .map(|authors| {
            authors
                .iter()
                .filter_map(|a| a.pointer("/author/display_name").and_then(Json::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let abstract_text = item
        .get("abstract_inverted_index")
        .and_then(reconstruct_openalex_abstract)
        .filter(|s| !s.is_empty());
    let published_date = item
        .get("publication_date")
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.get("publication_year")
                .and_then(Json::as_u64)
                .map(|year| year.to_string())
        });
    let venue = item
        .pointer("/primary_location/source/display_name")
        .and_then(Json::as_str)
        .map(str::to_string)
        .or_else(|| {
            item.get("host_venue")
                .and_then(|v| v.get("display_name"))
                .and_then(Json::as_str)
                .map(str::to_string)
        });
    let categories = item
        .get("concepts")
        .and_then(Json::as_array)
        .map(|concepts| {
            concepts
                .iter()
                .filter_map(|c| c.get("display_name").and_then(Json::as_str))
                .take(6)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut extra = JsonMap::new();
    if let Some(work_type) = item.get("type").and_then(Json::as_str) {
        extra.insert("type".to_string(), json!(work_type));
    }
    if let Some(is_oa) = item.pointer("/open_access/is_oa").and_then(Json::as_bool) {
        extra.insert("is_open_access".to_string(), json!(is_oa));
    }
    Some(LiteraturePaper {
        id,
        source: PublicLiteratureSource::OpenAlex,
        title,
        authors,
        abstract_text,
        url,
        pdf_url,
        doi,
        published_date,
        updated_date: item
            .get("updated_date")
            .and_then(Json::as_str)
            .map(str::to_string),
        venue,
        categories,
        citation_count: item.get("cited_by_count").and_then(Json::as_u64),
        extra,
    })
}

fn reconstruct_openalex_abstract(value: &Json) -> Option<String> {
    let map = value.as_object()?;
    let mut positions = Vec::<(usize, String)>::new();
    for (word, indexes) in map {
        let Some(indexes) = indexes.as_array() else {
            continue;
        };
        for index in indexes {
            if let Some(pos) = index.as_u64() {
                positions.push((pos as usize, word.clone()));
            }
        }
    }
    positions.sort_by_key(|(pos, _)| *pos);
    Some(
        positions
            .into_iter()
            .map(|(_, word)| word)
            .collect::<Vec<_>>()
            .join(" "),
    )
}
