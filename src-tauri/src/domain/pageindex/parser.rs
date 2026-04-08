//! Document parser for PageIndex.
//!
//! Supports multiple formats:
//! - Markdown: Parses headings into hierarchical sections
//! - Code files: Creates a single "Document" section with the full content
//! - Text files: Simple text parsing

use regex::Regex;
use std::collections::HashMap;

use super::tree::{DocumentMetadata, SectionNode};
use crate::errors::AppError;

/// Result of parsing a document.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Document title
    pub title: String,
    /// Full document content
    pub content: String,
    /// Parsed sections
    pub sections: Vec<SectionNode>,
    /// Document metadata
    pub metadata: DocumentMetadata,
}

/// Document parser supporting multiple formats.
pub struct DocumentParser {
    max_section_depth: usize,
    heading_regex: Regex,
    frontmatter_regex: Regex,
}

impl DocumentParser {
    /// Create a new parser with the specified max section depth.
    pub fn new(max_section_depth: usize) -> Self {
        Self {
            max_section_depth,
            heading_regex: Regex::new(r"^(#{1,6})\s+(.+)$").unwrap(),
            frontmatter_regex: Regex::new(r"^---\s*\n(.*?)\n---\s*\n(.*)$").unwrap(),
        }
    }

    /// Parse a document from content.
    pub fn parse(&self, path: &str, content: &str) -> Result<ParseResult, AppError> {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("txt");

        match extension.to_lowercase().as_str() {
            "md" | "markdown" => self.parse_markdown(path, content),
            "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "java" | "cpp" | "c" | "h"
            | "hpp" => self.parse_code_file(path, content),
            "json" | "yaml" | "yml" | "toml" => self.parse_config_file(path, content),
            _ => self.parse_text_file(path, content),
        }
    }

    /// Parse a Markdown file into hierarchical sections.
    fn parse_markdown(&self, path: &str, content: &str) -> Result<ParseResult, AppError> {
        let (frontmatter, body) = self.extract_frontmatter(content);
        let mut metadata = DocumentMetadata::from_path_and_content(path, content);
        metadata.frontmatter = frontmatter;

        let title = self.extract_title(path, body);
        let sections = self.parse_sections(body);

        Ok(ParseResult {
            title,
            content: body.to_string(),
            sections,
            metadata,
        })
    }

    /// Parse a code file (creates a single section with the content).
    fn parse_code_file(&self, path: &str, content: &str) -> Result<ParseResult, AppError> {
        let metadata = DocumentMetadata::from_path_and_content(path, content);
        let title = self.filename_to_title(path);

        // For code files, we might want to extract doc comments or function signatures
        // For now, treat as a single document node
        Ok(ParseResult {
            title,
            content: content.to_string(),
            sections: vec![], // Code files don't have hierarchical sections (for now)
            metadata,
        })
    }

    /// Parse a config file (JSON, YAML, TOML).
    fn parse_config_file(&self, path: &str, content: &str) -> Result<ParseResult, AppError> {
        let metadata = DocumentMetadata::from_path_and_content(path, content);
        let title = self.filename_to_title(path);

        Ok(ParseResult {
            title,
            content: content.to_string(),
            sections: vec![],
            metadata,
        })
    }

    /// Parse a plain text file.
    fn parse_text_file(&self, path: &str, content: &str) -> Result<ParseResult, AppError> {
        let metadata = DocumentMetadata::from_path_and_content(path, content);
        let title = self.filename_to_title(path);

        Ok(ParseResult {
            title,
            content: content.to_string(),
            sections: vec![],
            metadata,
        })
    }

    /// Extract YAML frontmatter from markdown content.
    fn extract_frontmatter<'a>(&self, content: &'a str) -> (Option<HashMap<String, serde_json::Value>>, &'a str) {
        if let Some(captures) = self.frontmatter_regex.captures(content) {
            let frontmatter_str = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let body = captures.get(2).map(|m| m.as_str()).unwrap_or("");

            // Try to parse as YAML
            match serde_yaml::from_str::<HashMap<String, serde_json::Value>>(frontmatter_str) {
                Ok(frontmatter) => (Some(frontmatter), body),
                Err(_) => (None, content),
            }
        } else {
            (None, content)
        }
    }

    /// Extract the title from the document (first H1 or filename).
    fn extract_title(&self, path: &str, content: &str) -> String {
        // Try to find first H1 heading
        for line in content.lines() {
            if let Some(captures) = self.heading_regex.captures(line.trim()) {
                let level = captures.get(1).unwrap().as_str().len();
                if level == 1 {
                    return captures.get(2).unwrap().as_str().trim().to_string();
                }
            }
        }

        // Fall back to filename
        self.filename_to_title(path)
    }

    /// Convert filename to title.
    fn filename_to_title(&self, path: &str) -> String {
        std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| {
                // Convert kebab-case or snake_case to Title Case
                s.split(&['-', '_', '.'][..])
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "Untitled".to_string())
    }

    /// Parse markdown content into hierarchical sections.
    fn parse_sections(&self, content: &str) -> Vec<SectionNode> {
        let mut sections: Vec<SectionNode> = Vec::new();
        let mut section_stack: Vec<SectionNode> = Vec::new();
        let mut current_content: Vec<String> = Vec::new();
        let mut current_line = 0;
        let _section_start_line = 0;

        for (line_num, line) in content.lines().enumerate() {
            current_line = line_num;

            if let Some(captures) = self.heading_regex.captures(line.trim()) {
                // Save content of current section before starting new one
                if !section_stack.is_empty() {
                    let content = current_content.join("\n").trim().to_string();
                    if let Some(section) = section_stack.last_mut() {
                        section.content = content;
                        section.line_range.1 = line_num;
                    }
                }
                current_content.clear();

                let level = captures.get(1).unwrap().as_str().len();
                let title = captures.get(2).unwrap().as_str().trim().to_string();
                let section_id = format!("sec_{}_{}", line_num, self.sanitize_id(&title));

                let new_section = SectionNode {
                    id: section_id,
                    title,
                    level,
                    content: String::new(),
                    children: Vec::new(),
                    line_range: (line_num, line_num),
                };

                // Pop sections that are at the same or deeper level
                while let Some(top) = section_stack.last() {
                    if top.level >= level {
                        let finished = section_stack.pop().unwrap();
                        if let Some(parent) = section_stack.last_mut() {
                            parent.add_child(finished);
                        } else {
                            sections.push(finished);
                        }
                    } else {
                        break;
                    }
                }

                section_stack.push(new_section);
                let _ = line_num;
            } else {
                current_content.push(line.to_string());
            }
        }

        // Flush remaining content and sections
        if !section_stack.is_empty() {
            let content = current_content.join("\n").trim().to_string();
            if let Some(section) = section_stack.last_mut() {
                section.content = content;
                section.line_range.1 = current_line + 1;
            }

            // Pop all remaining sections
            while let Some(section) = section_stack.pop() {
                if let Some(parent) = section_stack.last_mut() {
                    parent.add_child(section);
                } else {
                    sections.push(section);
                }
            }
        }

        // Filter out sections that are too deep
        self.filter_section_depth(&mut sections, 1);

        sections
    }

    /// Filter sections to max depth recursively.
    fn filter_section_depth(&self, sections: &mut Vec<SectionNode>, current_depth: usize) {
        if current_depth >= self.max_section_depth {
            sections.clear();
            return;
        }

        for section in sections.iter_mut() {
            self.filter_section_depth(&mut section.children, current_depth + 1);
        }
    }

    /// Sanitize a string for use in an ID.
    fn sanitize_id(&self, s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c.to_lowercase().to_string()
                } else {
                    "-".to_string()
                }
            })
            .collect::<String>()
            .replace("--", "-")
            .trim_matches('-')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_simple() {
        let parser = DocumentParser::new(6);
        let content = r#"# Title

This is the introduction.

## Section 1

Content of section 1.

### Subsection 1.1

Deep content.

## Section 2

Content of section 2.
"#;

        let result = parser.parse_markdown("test.md", content).unwrap();
        assert_eq!(result.title, "Title");
        assert_eq!(result.sections.len(), 2);
        assert_eq!(result.sections[0].title, "Section 1");
        assert_eq!(result.sections[0].children.len(), 1);
        assert_eq!(result.sections[0].children[0].title, "Subsection 1.1");
    }

    #[test]
    fn test_parse_markdown_with_frontmatter() {
        let parser = DocumentParser::new(6);
        let content = r#"---
title: My Document
description: A test document
---

# Actual Title

Content here.
"#;

        let result = parser.parse_markdown("test.md", content).unwrap();
        assert_eq!(result.title, "Actual Title");
        assert!(result.metadata.frontmatter.is_some());
        let fm = result.metadata.frontmatter.unwrap();
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "My Document");
    }

    #[test]
    fn test_filename_to_title() {
        let parser = DocumentParser::new(6);
        assert_eq!(parser.filename_to_title("my-file.rs"), "My File");
        assert_eq!(parser.filename_to_title("my_file.rs"), "My File");
        assert_eq!(parser.filename_to_title("README.md"), "Readme");
    }

    #[test]
    fn test_section_depth_limit() {
        let parser = DocumentParser::new(2); // Only allow 2 levels
        let content = r#"# Title

## Level 2

### Level 3

#### Level 4

Content.
"#;

        let result = parser.parse_markdown("test.md", content).unwrap();
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].children.len(), 1);
        // Level 3 and beyond should be filtered
        assert!(result.sections[0].children[0].children.is_empty());
    }

    #[test]
    fn test_code_file_parsing() {
        let parser = DocumentParser::new(6);
        let content = r#"// This is a Rust file
fn main() {
    println!("Hello");
}
"#;

        let result = parser.parse_code_file("main.rs", content).unwrap();
        assert_eq!(result.title, "Main");
        assert!(result.sections.is_empty());
    }

    #[test]
    fn test_keyword_search_in_sections() {
        let parser = DocumentParser::new(6);
        let content = r#"# Rust Guide

## Getting Started

Install Rust with rustup.

## Advanced Topics

Learn about lifetimes and borrowing.
"#;

        let result = parser.parse_markdown("guide.md", content).unwrap();
        assert!(result.sections[0].contains_keyword("rustup"));
        assert!(result.sections[1].contains_keyword("lifetimes"));
        assert!(!result.sections[0].contains_keyword("python"));
    }
}
