//! arXiv Atom response parser.

use super::{
    arxiv_id_from_url, attr_value, clean_xml_text, first_xml_tag, normalize_date_string,
    normalize_doi, LiteraturePaper, PublicLiteratureSource,
};
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{json, Map as JsonMap};

pub fn parse_arxiv_atom(xml: &str) -> Vec<LiteraturePaper> {
    lazy_static! {
        static ref RE_ENTRY: Regex = Regex::new(r#"(?is)<entry\b[^>]*>(.*?)</entry>"#).unwrap();
        static ref RE_AUTHOR: Regex = Regex::new(r#"(?is)<author\b[^>]*>(.*?)</author>"#).unwrap();
        static ref RE_LINK: Regex = Regex::new(r#"(?is)<link\b([^>]*)/?>"#).unwrap();
        static ref RE_CATEGORY: Regex = Regex::new(r#"(?is)<category\b([^>]*)/?>"#).unwrap();
    }

    let mut out = Vec::new();
    for entry in RE_ENTRY.captures_iter(xml) {
        let block = entry.get(1).map(|m| m.as_str()).unwrap_or_default();
        let title = first_xml_tag(block, "title").unwrap_or_default();
        let title = clean_xml_text(&title);
        if title.is_empty() {
            continue;
        }
        let url = clean_xml_text(&first_xml_tag(block, "id").unwrap_or_default());
        let id = arxiv_id_from_url(&url).unwrap_or_else(|| url.clone());
        let abstract_text = clean_xml_text(&first_xml_tag(block, "summary").unwrap_or_default());
        let published_date = normalize_date_string(&clean_xml_text(
            &first_xml_tag(block, "published").unwrap_or_default(),
        ));
        let updated_date = normalize_date_string(&clean_xml_text(
            &first_xml_tag(block, "updated").unwrap_or_default(),
        ));
        let doi = first_xml_tag(block, "doi")
            .map(|s| normalize_doi(&clean_xml_text(&s)))
            .filter(|s| !s.is_empty());
        let authors = RE_AUTHOR
            .captures_iter(block)
            .filter_map(|cap| first_xml_tag(cap.get(1)?.as_str(), "name"))
            .map(|s| clean_xml_text(&s))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let mut pdf_url = None;
        for link in RE_LINK.captures_iter(block) {
            let attrs = link.get(1).map(|m| m.as_str()).unwrap_or_default();
            let href = attr_value(attrs, "href").unwrap_or_default();
            let title_attr = attr_value(attrs, "title").unwrap_or_default();
            let type_attr = attr_value(attrs, "type").unwrap_or_default();
            if title_attr.eq_ignore_ascii_case("pdf")
                || type_attr.to_ascii_lowercase().contains("pdf")
                || href.contains("/pdf/")
            {
                pdf_url = Some(href);
                break;
            }
        }
        let categories = RE_CATEGORY
            .captures_iter(block)
            .filter_map(|cap| attr_value(cap.get(1)?.as_str(), "term"))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let mut extra = JsonMap::new();
        if let Some(comment) = first_xml_tag(block, "comment").map(|s| clean_xml_text(&s)) {
            if !comment.is_empty() {
                extra.insert("comment".to_string(), json!(comment));
            }
        }
        if let Some(journal_ref) = first_xml_tag(block, "journal_ref").map(|s| clean_xml_text(&s)) {
            if !journal_ref.is_empty() {
                extra.insert("journal_ref".to_string(), json!(journal_ref));
            }
        }
        out.push(LiteraturePaper {
            id,
            source: PublicLiteratureSource::Arxiv,
            title,
            authors,
            abstract_text: if abstract_text.is_empty() {
                None
            } else {
                Some(abstract_text)
            },
            url,
            pdf_url,
            doi,
            published_date,
            updated_date,
            venue: None,
            categories,
            citation_count: None,
            extra,
        });
    }
    out
}
