//! Citation metadata fetching
//!
//! Fetches structured metadata for cited URLs:
//!   - DOI / CrossRef → journal, authors, title, year
//!   - PubMed          → journal, authors, title, year, abstract snippet
//!   - arXiv           → authors, title, year, abstract snippet
//!   - Generic URLs    → OG title + description via lightweight HTML regex

use std::collections::HashMap;
use std::sync::Mutex;

use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};

// ── In-process cache ──────────────────────────────────────────────────────────

lazy_static! {
    static ref CACHE: Mutex<HashMap<String, CitationMeta>> = Mutex::new(HashMap::new());
}

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationMeta {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub journal: Option<String>,
    pub year: Option<u32>,
    pub doi: Option<String>,
    pub kind: CitationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CitationKind {
    Academic,
    Web,
    Unknown,
}

// ── Tauri command ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn fetch_citation_metadata(url: String) -> Result<CitationMeta, String> {
    // Fast path: cache hit
    {
        let cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(meta) = cache.get(&url) {
            return Ok(meta.clone());
        }
    }

    let meta = fetch_meta_inner(&url)
        .await
        .unwrap_or_else(|_| CitationMeta {
            url: url.clone(),
            title: None,
            description: None,
            authors: vec![],
            journal: None,
            year: None,
            doi: None,
            kind: CitationKind::Unknown,
        });

    {
        let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
        cache.insert(url, meta.clone());
    }

    Ok(meta)
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

async fn fetch_meta_inner(url: &str) -> Result<CitationMeta, String> {
    let lower = url.to_lowercase();

    if lower.contains("doi.org/") || lower.starts_with("https://dx.doi.org") {
        fetch_crossref(url).await
    } else if lower.contains("pubmed.ncbi.nlm.nih.gov") || lower.contains("ncbi.nlm.nih.gov/pubmed")
    {
        fetch_pubmed(url).await
    } else if lower.contains("arxiv.org/abs/") || lower.contains("arxiv.org/pdf/") {
        fetch_arxiv(url).await
    } else {
        fetch_og_tags(url).await
    }
}

// ── CrossRef (DOI) ────────────────────────────────────────────────────────────

async fn fetch_crossref(url: &str) -> Result<CitationMeta, String> {
    let doi = extract_doi(url).ok_or("cannot extract DOI")?;
    // Percent-encode each path segment to avoid query/fragment injection from DOIs
    // containing '?', '#', or '&' characters (valid per DOI spec).
    let doi_encoded: String = doi
        .split('/')
        .map(|seg| {
            seg.chars()
                .flat_map(|c| {
                    if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | ':') {
                        vec![c]
                    } else {
                        format!("%{:02X}", c as u32).chars().collect()
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/");
    let api = format!("https://api.crossref.org/works/{}", doi_encoded);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Omiga/1.0 (mailto:support@omiga.app)")
        .build()
        .map_err(|e| e.to_string())?;

    let resp: serde_json::Value = client
        .get(&api)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let work = &resp["message"];

    let title = work["title"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let authors: Vec<String> = work["author"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|a| {
            let given = a["given"].as_str().unwrap_or("");
            let family = a["family"].as_str()?;
            Some(if given.is_empty() {
                family.to_string()
            } else {
                format!("{} {}", given, family)
            })
        })
        .collect();

    let journal = work["container-title"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let year = work["published"]["date-parts"]
        .as_array()
        .and_then(|outer| outer.first())
        .and_then(|parts| parts.as_array())
        .and_then(|parts| parts.first())
        .and_then(|y| y.as_u64())
        .map(|y| y as u32);

    let description = work["abstract"]
        .as_str()
        .map(|s| truncate_str(strip_html(s), 300));

    Ok(CitationMeta {
        url: url.to_string(),
        title,
        description,
        authors,
        journal,
        year,
        doi: Some(doi),
        kind: CitationKind::Academic,
    })
}

fn extract_doi(url: &str) -> Option<String> {
    // https://doi.org/10.xxxx/...  or  doi:10.xxxx/...
    let lower = url.to_lowercase();
    let after = if let Some(pos) = lower.find("doi.org/") {
        &url[pos + 8..]
    } else if let Some(pos) = lower.find("doi:") {
        &url[pos + 4..]
    } else {
        return None;
    };
    let doi = after.trim_start_matches('/');
    if doi.starts_with("10.") {
        Some(doi.to_string())
    } else {
        None
    }
}

// ── PubMed ───────────────────────────────────────────────────────────────────

async fn fetch_pubmed(url: &str) -> Result<CitationMeta, String> {
    let pmid = extract_pmid(url).ok_or("cannot extract PMID")?;
    // Use NCBI esummary JSON API (no key needed for low-rate access)
    let api = format!(
        "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi?db=pubmed&id={}&retmode=json",
        pmid
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Omiga/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let resp: serde_json::Value = client
        .get(&api)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let doc = &resp["result"][&pmid];

    let title = doc["title"].as_str().map(str::to_string);

    let authors: Vec<String> = doc["authors"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|a| a["name"].as_str().map(str::to_string))
        .collect();

    let journal = doc["source"].as_str().map(str::to_string);

    let year = doc["pubdate"]
        .as_str()
        .and_then(|s| s.split_whitespace().next())
        .and_then(|y| y.parse::<u32>().ok());

    let doi = doc["articleids"]
        .as_array()
        .and_then(|ids| {
            ids.iter()
                .find(|id| id["idtype"].as_str().map(|t| t == "doi").unwrap_or(false))
        })
        .and_then(|id| id["value"].as_str())
        .map(str::to_string);

    Ok(CitationMeta {
        url: url.to_string(),
        title,
        description: None,
        authors,
        journal,
        year,
        doi,
        kind: CitationKind::Academic,
    })
}

fn extract_pmid(url: &str) -> Option<String> {
    // https://pubmed.ncbi.nlm.nih.gov/12345678/
    let re = regex::Regex::new(r"/(\d{5,10})/?$").ok()?;
    re.captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

// ── arXiv ────────────────────────────────────────────────────────────────────

async fn fetch_arxiv(url: &str) -> Result<CitationMeta, String> {
    let arxiv_id = extract_arxiv_id(url).ok_or("cannot extract arXiv ID")?;
    // arXiv Atom API
    let api = format!(
        "https://export.arxiv.org/api/query?id_list={}&max_results=1",
        arxiv_id
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Omiga/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let text = client
        .get(&api)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let title = extract_xml_tag(&text, "title")
        .into_iter()
        .find(|t| !t.to_lowercase().contains("arxiv"))
        .map(|t| t.trim().to_string());

    let authors: Vec<String> = extract_xml_tag(&text, "name");

    let description = extract_xml_tag(&text, "summary")
        .into_iter()
        .next()
        .map(|s| truncate_str(s.trim().to_string(), 300));

    let year = extract_xml_tag(&text, "published")
        .into_iter()
        .next()
        .and_then(|s| s.get(..4).and_then(|y| y.parse::<u32>().ok()));

    Ok(CitationMeta {
        url: url.to_string(),
        title,
        description,
        authors,
        journal: Some("arXiv".to_string()),
        year,
        doi: None,
        kind: CitationKind::Academic,
    })
}

fn extract_arxiv_id(url: &str) -> Option<String> {
    // https://arxiv.org/abs/2301.00001  or  /pdf/2301.00001
    let re = regex::Regex::new(r"arxiv\.org/(?:abs|pdf)/(\d{4}\.\d{4,5}(?:v\d+)?)").ok()?;
    re.captures(url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

fn extract_xml_tag(xml: &str, tag: &str) -> Vec<String> {
    let re = regex::Regex::new(&format!(r"(?s)<{tag}[^>]*>(.*?)</{tag}>")).unwrap();
    re.captures_iter(xml)
        .filter_map(|c| c.get(1))
        .map(|m| strip_html(m.as_str()).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ── Generic OG meta tags ──────────────────────────────────────────────────────

async fn fetch_og_tags(url: &str) -> Result<CitationMeta, String> {
    // SSRF guard: reject non-http(s) and private/loopback addresses
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err("unsupported scheme".to_string()),
    }
    if let Some(host) = parsed.host_str() {
        if is_private_host(host) {
            return Err("private address".to_string());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent("Mozilla/5.0 (compatible; Omiga/1.0)")
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;

    // Only process HTML; avoid downloading binary files
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !ct.contains("html") {
        return Err("not an HTML page".to_string());
    }

    // Read at most 32 KB (enough for <head>)
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let head_html = {
        let raw = &bytes[..bytes.len().min(32_768)];
        String::from_utf8_lossy(raw).into_owned()
    };

    let title = og_meta(&head_html, "og:title").or_else(|| html_tag(&head_html, "title"));

    let description = og_meta(&head_html, "og:description")
        .or_else(|| og_meta(&head_html, "description"))
        .map(|s| truncate_str(s, 300));

    Ok(CitationMeta {
        url: url.to_string(),
        title,
        description,
        authors: vec![],
        journal: None,
        year: None,
        doi: None,
        kind: CitationKind::Web,
    })
}

fn is_private_host(host: &str) -> bool {
    use std::net::IpAddr;
    if host == "localhost" || host.ends_with(".local") {
        return true;
    }
    if let Ok(addr) = host.parse::<IpAddr>() {
        return addr.is_loopback()
            || addr.is_unspecified()
            || matches!(addr, IpAddr::V4(v4) if
                v4.is_private() || v4.is_link_local() || v4.is_broadcast()
            )
            || matches!(addr, IpAddr::V6(v6) if is_private_ipv6(&v6));
    }
    false
}

fn is_private_ipv6(addr: &std::net::Ipv6Addr) -> bool {
    let segments = addr.segments();
    // Unique-local: fc00::/7
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // Link-local: fe80::/10
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    false
}

fn og_meta(html: &str, property: &str) -> Option<String> {
    // <meta property="og:title" content="…">  or  <meta name="description" content="…">
    let pattern = format!(
        r#"(?i)<meta[^>]*(?:property|name)\s*=\s*["']?{}["']?[^>]*content\s*=\s*["']([^"']+)["']"#,
        regex::escape(property)
    );
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| decode_html_entities(m.as_str()))
        // Also try reversed attribute order: content first, then property
        .or_else(|| {
            let pattern2 = format!(
                r#"(?i)<meta[^>]*content\s*=\s*["']([^"']+)["'][^>]*(?:property|name)\s*=\s*["']?{}["']?"#,
                regex::escape(property)
            );
            let re2 = regex::Regex::new(&pattern2).ok()?;
            re2.captures(html)
                .and_then(|c| c.get(1))
                .map(|m| decode_html_entities(m.as_str()))
        })
}

fn html_tag(html: &str, tag: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"(?is)<{tag}[^>]*>(.*?)</{tag}>")).ok()?;
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| decode_html_entities(&strip_html(m.as_str())))
        .filter(|s| !s.is_empty())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn strip_html(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(s, "").into_owned()
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn truncate_str(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{}…", truncated.trim_end())
}
