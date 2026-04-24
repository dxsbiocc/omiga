//! Query engine for PageIndex.
//!
//! Implements semantic-like retrieval without vectors by:
//! - Keyword matching with TF-IDF-like scoring
//! - Heading-aware context expansion
//! - Breadcrumb navigation for better context

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;

use super::tree::{DocumentNode, DocumentTree, SectionNode};
use crate::errors::AppError;

/// Query result containing matched content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Document ID
    pub doc_id: String,
    /// Document path
    pub path: String,
    /// Section ID (if matched a section)
    pub section_id: Option<String>,
    /// Title of the matched content
    pub title: String,
    /// Breadcrumb path (parent sections)
    pub breadcrumb: Vec<String>,
    /// Matched content excerpt
    pub excerpt: String,
    /// Full content of the section/document
    pub content: String,
    /// Relevance score (higher = more relevant)
    pub score: f64,
    /// Match type
    pub match_type: MatchType,
}

/// Type of match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchType {
    /// Matched document title
    Title,
    /// Matched section heading
    Heading,
    /// Matched content body
    Content,
    /// Matched file path
    Path,
}

/// Query engine for searching the document tree.
pub struct QueryEngine {
    /// Stop words to filter out
    stop_words: Vec<String>,
}

impl QueryEngine {
    /// Create a new query engine.
    pub fn new() -> Self {
        Self {
            stop_words: DEFAULT_STOP_WORDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Search the document tree for relevant content.
    pub async fn search(
        &self,
        tree: &DocumentTree,
        query: &str,
        limit: usize,
    ) -> Result<Vec<QueryResult>, AppError> {
        debug!("Searching for: {}", query);

        let keywords = self.extract_keywords(query);
        if keywords.is_empty() {
            return Ok(vec![]);
        }

        let mut results: Vec<QueryResult> = Vec::new();

        // Search all documents
        for doc in tree.iter_documents() {
            let doc_results = self.search_document(doc, &keywords);
            results.extend(doc_results);
        }

        // Sort by score (descending)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Deduplicate and limit
        results = self.deduplicate_results(results);
        results.truncate(limit);

        debug!("Found {} results", results.len());
        Ok(results)
    }

    /// Search within a single document.
    fn search_document(&self, doc: &DocumentNode, keywords: &[String]) -> Vec<QueryResult> {
        let mut results = Vec::new();

        // Check document title
        let title_score = self.score_text(&doc.title, keywords);
        if title_score > 0.0 {
            results.push(QueryResult {
                doc_id: doc.id.clone(),
                path: doc.path.clone(),
                section_id: None,
                title: doc.title.clone(),
                breadcrumb: vec![],
                excerpt: self.create_excerpt(&doc.content, keywords, 300),
                content: doc.summary(),
                score: title_score * 2.0, // Boost title matches
                match_type: MatchType::Title,
            });
        }

        // Check document path
        let path_score = self.score_text(&doc.path, keywords);
        if path_score > 0.0 {
            results.push(QueryResult {
                doc_id: doc.id.clone(),
                path: doc.path.clone(),
                section_id: None,
                title: doc.title.clone(),
                breadcrumb: vec![],
                excerpt: self.create_excerpt(&doc.content, keywords, 300),
                content: doc.summary(),
                score: path_score * 1.5, // Boost path matches
                match_type: MatchType::Path,
            });
        }

        // Check document content
        let content_score = self.score_text(&doc.content, keywords);
        if content_score > 0.0 && title_score == 0.0 && path_score == 0.0 {
            results.push(QueryResult {
                doc_id: doc.id.clone(),
                path: doc.path.clone(),
                section_id: None,
                title: doc.title.clone(),
                breadcrumb: vec![],
                excerpt: self.create_excerpt(&doc.content, keywords, 300),
                content: doc.summary(),
                score: content_score,
                match_type: MatchType::Content,
            });
        }

        // Search sections
        for section in &doc.sections {
            let section_results = self.search_section(doc, section, keywords, vec![]);
            results.extend(section_results);
        }

        results
    }

    /// Search within a section recursively.
    fn search_section(
        &self,
        doc: &DocumentNode,
        section: &SectionNode,
        keywords: &[String],
        parent_breadcrumb: Vec<String>,
    ) -> Vec<QueryResult> {
        let mut results = Vec::new();
        let mut breadcrumb = parent_breadcrumb.clone();
        breadcrumb.push(section.title.clone());

        // Check section title
        let title_score = self.score_text(&section.title, keywords);
        if title_score > 0.0 {
            results.push(QueryResult {
                doc_id: doc.id.clone(),
                path: doc.path.clone(),
                section_id: Some(section.id.clone()),
                title: section.title.clone(),
                breadcrumb: breadcrumb.clone(),
                excerpt: self.create_excerpt(&section.content, keywords, 300),
                content: section.full_text(),
                score: title_score * 1.8, // Boost section title matches
                match_type: MatchType::Heading,
            });
        }

        // Check section content
        let content_score = self.score_text(&section.content, keywords);
        if content_score > 0.0 && title_score == 0.0 {
            results.push(QueryResult {
                doc_id: doc.id.clone(),
                path: doc.path.clone(),
                section_id: Some(section.id.clone()),
                title: section.title.clone(),
                breadcrumb: breadcrumb.clone(),
                excerpt: self.create_excerpt(&section.content, keywords, 300),
                content: section.full_text(),
                score: content_score * 0.9, // Slight penalty for body matches
                match_type: MatchType::Content,
            });
        }

        // Search child sections
        for child in &section.children {
            let child_results = self.search_section(doc, child, keywords, breadcrumb.clone());
            results.extend(child_results);
        }

        results
    }

    /// Score how well text matches keywords.
    fn score_text(&self, text: &str, keywords: &[String]) -> f64 {
        score_terms_against_text(text, keywords)
    }

    /// Extract keywords from a query string.
    fn extract_keywords(&self, query: &str) -> Vec<String> {
        let mut keywords = derive_query_terms(query);
        keywords.retain(|w| !self.stop_words.contains(w));
        keywords
    }

    /// Create an excerpt around matched keywords.
    fn create_excerpt(&self, content: &str, keywords: &[String], max_length: usize) -> String {
        let content_lower = content.to_lowercase();
        let mut best_pos = 0;
        let mut best_score = 0;

        // Find the best position (most keyword matches in window)
        // Use character-based indexing to handle UTF-8 safely
        let char_count = content.chars().count();
        let window_chars = max_length.min(char_count);
        if window_chars >= char_count {
            return content.to_string();
        }

        // Collect char byte positions for safe slicing
        let char_positions: Vec<usize> = content.char_indices().map(|(i, _)| i).collect();
        let max_start_idx = char_positions.len().saturating_sub(window_chars);
        let step = (window_chars / 4).max(1);

        for idx in (0..max_start_idx).step_by(step) {
            let byte_start = char_positions[idx];
            let byte_end = char_positions
                .get(idx + window_chars)
                .copied()
                .unwrap_or(content.len());
            let window = &content_lower[byte_start..byte_end];
            let score = keywords
                .iter()
                .filter(|kw| window.contains(&kw.to_lowercase()))
                .count();
            if score > best_score {
                best_score = score;
                best_pos = byte_start;
            }
        }

        // Extract the window - ensure byte indices are at char boundaries for UTF-8 safety
        let start = best_pos;
        let window_size = content.floor_char_boundary(max_length).min(content.len());
        let end = (start + window_size).min(content.len());

        // Adjust to char boundaries to avoid panics on multi-byte UTF-8 characters
        let start = content.floor_char_boundary(start);
        let end = content.floor_char_boundary(end);

        // Adjust to word boundaries
        let adjusted_start = if start > 0 {
            content[start..]
                .find(|c: char| c.is_whitespace())
                .map(|i| start + i)
                .unwrap_or(start)
        } else {
            0
        };

        let adjusted_end = content[..end]
            .rfind(|c: char| c.is_whitespace())
            .unwrap_or(end);

        // Ensure final indices are at char boundaries
        let adjusted_start = content.floor_char_boundary(adjusted_start);
        let adjusted_end = content.floor_char_boundary(adjusted_end);

        let mut excerpt = content[adjusted_start..adjusted_end].to_string();

        // Add ellipsis if truncated
        if adjusted_start > 0 {
            excerpt = format!("...{}", excerpt.trim_start());
        }
        if adjusted_end < content.len() {
            excerpt = format!("{}...", excerpt.trim_end());
        }

        excerpt
    }

    /// Deduplicate results, keeping the highest scoring match for each document/section.
    fn deduplicate_results(&self, results: Vec<QueryResult>) -> Vec<QueryResult> {
        let mut seen: HashMap<String, QueryResult> = HashMap::new();

        for result in results {
            let key = if let Some(ref section_id) = result.section_id {
                format!("{}:{}", result.doc_id, section_id)
            } else {
                result.doc_id.clone()
            };

            match seen.get_mut(&key) {
                Some(existing) => {
                    if result.score > existing.score {
                        *existing = result;
                    }
                }
                None => {
                    seen.insert(key, result);
                }
            }
        }

        seen.into_values().collect()
    }

    /// Format query results as a context string for LLM prompting.
    pub fn format_results_as_context(&self, results: &[QueryResult]) -> String {
        if results.is_empty() {
            return String::new();
        }

        let mut context = String::from("## Relevant Context from Project Memory\n\n");

        for (i, result) in results.iter().enumerate() {
            context.push_str(&format!("### {}. {}", i + 1, result.title));

            if !result.breadcrumb.is_empty() {
                context.push_str(&format!(" (in: {})", result.breadcrumb.join(" > ")));
            }

            context.push_str(&format!("\n*Source: `{}`*\n\n", result.path));
            context.push_str(&result.excerpt);
            context.push_str("\n\n---\n\n");
        }

        context
    }
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub fn derive_query_terms(query: &str) -> Vec<String> {
    let normalized = query.trim().to_lowercase();
    if normalized.is_empty() {
        return vec![];
    }

    let mut terms = Vec::new();
    for raw_segment in normalized.split(|c: char| !c.is_alphanumeric()) {
        let cleaned = strip_query_wrappers(raw_segment);
        for segment in cleaned.split_whitespace() {
            push_query_terms(segment, &mut terms);
        }
    }

    if terms.is_empty() {
        let fallback = strip_query_wrappers(&normalized)
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>();
        push_term(&fallback, &mut terms);
    }

    terms.sort_by(|a, b| {
        b.chars()
            .count()
            .cmp(&a.chars().count())
            .then_with(|| a.cmp(b))
    });
    terms.truncate(12);
    terms
}

pub fn score_terms_against_text(text: &str, keywords: &[String]) -> f64 {
    if keywords.is_empty() {
        return 0.0;
    }

    let text_lower = text.to_lowercase();
    let token_count = estimated_token_count(&text_lower);

    let mut matched_keywords = 0usize;
    let mut total_matches = 0usize;

    for keyword in keywords {
        if keyword.is_empty() {
            continue;
        }
        if text_lower.contains(keyword) {
            matched_keywords += 1;
            total_matches += text_lower.matches(keyword).count();
        }
    }

    if matched_keywords == 0 {
        return 0.0;
    }

    let keyword_coverage = matched_keywords as f64 / keywords.len().max(1) as f64;
    let frequency_score = (total_matches as f64 / token_count as f64).min(1.0);
    keyword_coverage * 0.7 + frequency_score * 0.3
}

fn estimated_token_count(text: &str) -> usize {
    let whitespace_tokens = text.split_whitespace().count();
    if whitespace_tokens > 1 {
        return whitespace_tokens;
    }

    let run_tokens = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .count();
    if run_tokens > 1 {
        return run_tokens;
    }

    (text.chars().count() / 2).max(1)
}

fn push_query_terms(segment: &str, out: &mut Vec<String>) {
    let trimmed_segment = trim_cjk_particles(segment);
    let trimmed = trimmed_segment.trim();
    if trimmed.is_empty() {
        return;
    }

    let char_count = trimmed.chars().count();
    let has_cjk = trimmed.chars().any(is_cjk);

    if has_cjk {
        if char_count <= 1 {
            return;
        }

        if char_count <= 8 {
            push_term(trimmed, out);
        }

        if char_count >= 4 {
            for size in [4usize, 3, 2] {
                for gram in cjk_ngrams(trimmed, size) {
                    push_term(&gram, out);
                }
            }
        } else if char_count >= 2 {
            for gram in cjk_ngrams(trimmed, char_count) {
                push_term(&gram, out);
            }
        }
        return;
    }

    if char_count >= 3 {
        push_term(trimmed, out);
    }
}

fn push_term(term: &str, out: &mut Vec<String>) {
    let candidate = term.trim().to_lowercase();
    if candidate.chars().count() < 2 {
        return;
    }
    if SEARCH_NOISE_TERMS.contains(&candidate.as_str()) {
        return;
    }
    if !out.iter().any(|existing| existing == &candidate) {
        out.push(candidate);
    }
}

fn strip_query_wrappers(segment: &str) -> String {
    let mut cleaned = segment.to_lowercase();
    for wrapper in QUERY_WRAPPER_PHRASES {
        cleaned = cleaned.replace(wrapper, " ");
    }
    cleaned
}

fn trim_cjk_particles(segment: &str) -> String {
    let particles: &[char] = &[
        '的', '了', '和', '与', '及', '中', '里', '上', '下', '请', '把',
    ];
    segment.trim_matches(|c| particles.contains(&c)).to_string()
}

fn cjk_ngrams(segment: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = segment.chars().collect();
    if size == 0 || chars.len() < size {
        return vec![];
    }

    let mut out = Vec::new();
    for window in chars.windows(size) {
        out.push(window.iter().collect());
    }
    out
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
    )
}

/// Default stop words for keyword extraction.
const DEFAULT_STOP_WORDS: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "i", "it", "for", "not", "on",
    "with", "he", "as", "you", "do", "at", "this", "but", "his", "by", "from", "they", "we", "say",
    "her", "she", "or", "an", "will", "my", "one", "all", "would", "there", "their", "what", "so",
    "up", "out", "if", "about", "who", "get", "which", "go", "me", "when", "make", "can", "like",
    "time", "no", "just", "him", "know", "take", "people", "into", "year", "your", "good", "some",
    "could", "them", "see", "other", "than", "then", "now", "look", "only", "come", "its", "over",
    "think", "also", "back", "after", "use", "two", "how", "our", "first", "well", "way", "even",
    "new", "want", "because", "any", "these", "give", "day", "most", "us", "was", "were", "been",
    "has", "had", "did", "does", "doing", "done",
];

const QUERY_WRAPPER_PHRASES: &[&str] = &[
    "获取",
    "查找",
    "搜索",
    "检索",
    "查询",
    "请问",
    "请帮我",
    "帮我",
    "看看",
    "查看",
    "相关内容",
    "相关资料",
    "相关信息",
    "全局记忆",
    "项目记忆",
    "会话记忆",
    "聊天记录",
    "知识库",
    "global memory",
    "project memory",
    "session history",
    "chat history",
    "knowledge base",
    "find me",
    "show me",
];

const SEARCH_NOISE_TERMS: &[&str] = &[
    "获取", "查找", "搜索", "检索", "查询", "相关", "内容", "资料", "信息", "show", "find",
    "search", "retrieve", "query", "related", "content",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_keywords() {
        let engine = QueryEngine::new();
        let keywords = engine.extract_keywords("How does authentication work in Rust?");

        assert!(keywords.contains(&"authentication".to_string()));
        assert!(keywords.contains(&"work".to_string()));
        assert!(keywords.contains(&"rust".to_string()));
        assert!(!keywords.contains(&"how".to_string())); // Stop word
        assert!(!keywords.contains(&"does".to_string())); // Stop word
        assert!(!keywords.contains(&"in".to_string())); // Stop word
    }

    #[test]
    fn test_score_text() {
        let engine = QueryEngine::new();

        let score1 = engine.score_text(
            "Rust programming language",
            &["rust".to_string(), "programming".to_string()],
        );
        assert!(score1 > 0.0);

        let score2 = engine.score_text(
            "Python programming",
            &["rust".to_string(), "programming".to_string()],
        );
        assert!(score2 > 0.0);
        assert!(score2 < score1); // Should score lower since "rust" is missing

        let score3 = engine.score_text(
            "completely unrelated",
            &["rust".to_string(), "programming".to_string()],
        );
        assert_eq!(score3, 0.0);
    }

    #[test]
    fn test_create_excerpt() {
        let engine = QueryEngine::new();
        let content = "This is a long document about Rust programming. \
                      Rust is a systems programming language. \
                      It has many features like ownership and borrowing. \
                      The language is designed for safety and performance.";

        let excerpt = engine.create_excerpt(
            content,
            &["rust".to_string(), "programming".to_string()],
            100,
        );

        assert!(excerpt.to_lowercase().contains("rust"));
        assert!(excerpt.len() <= 150); // Allow some margin
    }

    #[test]
    fn test_derive_query_terms_handles_cjk_natural_language_queries() {
        let terms = derive_query_terms("获取全局记忆中与氧化还原节律相关内容");

        assert!(terms.iter().any(|term| term == "氧化还原节律"));
        assert!(terms
            .iter()
            .any(|term| term == "氧化还原" || term == "还原节律"));
        assert!(!terms
            .iter()
            .any(|term| term == "获取" || term == "相关内容"));
    }

    #[test]
    fn test_format_results_as_context() {
        let engine = QueryEngine::new();
        let results = vec![QueryResult {
            doc_id: "doc1".to_string(),
            path: "guide.md".to_string(),
            section_id: None,
            title: "Getting Started".to_string(),
            breadcrumb: vec![],
            excerpt: "Install Rust...".to_string(),
            content: "Full content".to_string(),
            score: 0.9,
            match_type: MatchType::Title,
        }];

        let context = engine.format_results_as_context(&results);
        assert!(context.contains("Relevant Context from Project Memory"));
        assert!(context.contains("Getting Started"));
        assert!(context.contains("guide.md"));
    }
}
