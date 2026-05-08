use super::*;
use serde_json::json;

#[test]
fn parses_arxiv_atom_fixture() {
    let xml = r#"
    <feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
      <entry>
        <id>http://arxiv.org/abs/2401.01234v1</id>
        <updated>2024-01-02T00:00:00Z</updated>
        <published>2024-01-01T00:00:00Z</published>
        <title> Test &amp; Paper </title>
        <summary> A useful abstract. </summary>
        <author><name>Alice Smith</name></author>
        <author><name>Bob Jones</name></author>
        <link title="pdf" href="http://arxiv.org/pdf/2401.01234v1" type="application/pdf"/>
        <category term="cs.CL" />
        <arxiv:doi>10.1000/example</arxiv:doi>
      </entry>
    </feed>
    "#;
    let parsed = parse_arxiv_atom(xml);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id, "2401.01234v1");
    assert_eq!(parsed[0].title, "Test & Paper");
    assert_eq!(parsed[0].authors, vec!["Alice Smith", "Bob Jones"]);
    assert_eq!(parsed[0].doi.as_deref(), Some("10.1000/example"));
    assert_eq!(parsed[0].categories, vec!["cs.CL"]);
}

#[test]
fn parses_crossref_json_fixture() {
    let value = json!({
        "message": {
            "total-results": 1,
            "items": [{
                "DOI": "10.1000/example",
                "title": ["<i>Crossref</i> Paper"],
                "author": [{"given": "Alice", "family": "Smith"}],
                "abstract": "<jats:p>Abstract text.</jats:p>",
                "URL": "https://doi.org/10.1000/example",
                "published": {"date-parts": [[2023, 5, 2]]},
                "container-title": ["Journal"],
                "subject": ["AI"],
                "is-referenced-by-count": 7
            }]
        }
    });
    let parsed = parse_crossref_json(&value);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].title, "Crossref Paper");
    assert_eq!(parsed[0].authors, vec!["Alice Smith"]);
    assert_eq!(parsed[0].published_date.as_deref(), Some("2023-05-02"));
    assert_eq!(parsed[0].citation_count, Some(7));
}

#[test]
fn parses_openalex_json_fixture() {
    let value = json!({
        "meta": {"count": 1},
        "results": [{
            "id": "https://openalex.org/W123",
            "display_name": "OpenAlex Paper",
            "doi": "https://doi.org/10.1000/openalex",
            "abstract_inverted_index": {"hello": [0], "world": [1]},
            "authorships": [{"author": {"display_name": "Jane Doe"}}],
            "publication_date": "2022-03-04",
            "primary_location": {
                "landing_page_url": "https://example.org/paper",
                "pdf_url": "https://example.org/paper.pdf",
                "source": {"display_name": "Venue"}
            },
            "concepts": [{"display_name": "Biology"}],
            "cited_by_count": 11
        }]
    });
    let parsed = parse_openalex_json(&value);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id, "W123");
    assert_eq!(parsed[0].abstract_text.as_deref(), Some("hello world"));
    assert_eq!(parsed[0].venue.as_deref(), Some("Venue"));
}

#[test]
fn parses_preprint_json_fixture() {
    let value = json!({
        "collection": [{
            "doi": "10.1101/2024.01.01.123456",
            "title": "CRISPR screen",
            "authors": "Alice Smith; Bob Jones",
            "abstract": "A CRISPR abstract",
            "date": "2024-01-01",
            "version": "2",
            "category": "genomics",
            "server": "biorxiv"
        }]
    });
    let parsed = parse_preprint_json(PublicLiteratureSource::Biorxiv, &value, "crispr");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].authors, vec!["Alice Smith", "Bob Jones"]);
    assert_eq!(
        parsed[0].pdf_url.as_deref(),
        Some("https://www.biorxiv.org/content/10.1101/2024.01.01.123456v2.full.pdf")
    );
}

#[test]
fn literature_json_uses_serpapi_shape() {
    let response = LiteratureSearchResponse {
        query: "test".to_string(),
        source: PublicLiteratureSource::Arxiv,
        total: Some(1),
        notes: vec!["note".to_string()],
        results: vec![LiteraturePaper {
            id: "2401.01234".to_string(),
            source: PublicLiteratureSource::Arxiv,
            title: "Title".to_string(),
            authors: vec!["Alice".to_string()],
            abstract_text: Some("Abstract".to_string()),
            url: "https://arxiv.org/abs/2401.01234".to_string(),
            pdf_url: None,
            doi: None,
            published_date: Some("2024-01-01".to_string()),
            updated_date: None,
            venue: None,
            categories: vec!["cs.CL".to_string()],
            citation_count: None,
            extra: JsonMap::new(),
        }],
    };
    let json = search_response_to_json(&response);
    assert_eq!(json["category"], "literature");
    assert_eq!(json["source"], "arxiv");
    assert_eq!(json["results"][0]["metadata"]["authors"][0], "Alice");
    assert_eq!(
        json["results"][0]["favicon"],
        "https://arxiv.org/favicon.ico"
    );
}

#[test]
fn literature_detail_json_preserves_fetch_fields() {
    let item = LiteraturePaper {
        id: "10.1000/example".to_string(),
        source: PublicLiteratureSource::Crossref,
        title: "Fetched Paper".to_string(),
        authors: vec!["Alice".to_string(), "Bob".to_string()],
        abstract_text: Some("Detailed abstract".to_string()),
        url: "https://doi.org/10.1000/example".to_string(),
        pdf_url: Some("https://example.org/paper.pdf".to_string()),
        doi: Some("10.1000/example".to_string()),
        published_date: Some("2024-01-02".to_string()),
        updated_date: None,
        venue: Some("Journal".to_string()),
        categories: vec!["AI".to_string()],
        citation_count: Some(12),
        extra: JsonMap::new(),
    };

    let json = paper_to_detail_json(&item);
    assert_eq!(json["category"], "literature");
    assert_eq!(json["source"], "crossref");
    assert_eq!(json["metadata"]["doi"], "10.1000/example");
    assert_eq!(json["authors"][0], "Alice");
    assert!(json["content"]
        .as_str()
        .unwrap()
        .contains("Detailed abstract"));
    assert_eq!(json["favicon"], "https://www.crossref.org/favicon.ico");
}

#[test]
fn normalizes_fetch_identifiers() {
    assert_eq!(
        normalize_arxiv_identifier("https://arxiv.org/pdf/2401.01234v2.pdf").as_deref(),
        Some("2401.01234v2")
    );
    assert_eq!(
        normalize_arxiv_identifier("ARXIV:2401.01234").as_deref(),
        Some("2401.01234")
    );
    assert_eq!(
        normalize_openalex_identifier("https://openalex.org/W123").as_deref(),
        Some("W123")
    );
    assert_eq!(
        normalize_openalex_identifier("https://doi.org/10.1000/example").as_deref(),
        Some("doi:10.1000/example")
    );
    assert_eq!(normalize_doi("DOI:10.1000/example"), "10.1000/example");
}
