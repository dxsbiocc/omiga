//! Hierarchical document tree structure for PageIndex.
//!
//! This module implements a unified tree architecture where:
//! - Each document is a root node
//! - Documents have sections as children
//! - Sections can have nested subsections
//!
//! Tree structure: Document → Section → Subsection → ...

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The complete document tree containing all indexed documents.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentTree {
    /// All documents in the tree, keyed by document ID
    documents: HashMap<String, DocumentNode>,
    /// Root-level document IDs for quick iteration
    root_document_ids: Vec<String>,
}

impl DocumentTree {
    /// Create a new empty document tree.
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            root_document_ids: Vec::new(),
        }
    }

    /// Add a document to the tree.
    pub fn add_document(&mut self, doc: DocumentNode) {
        let id = doc.id.clone();
        if !self.documents.contains_key(&id) {
            self.root_document_ids.push(id.clone());
        }
        self.documents.insert(id, doc);
    }

    /// Get a document by ID.
    pub fn get_document(&self, id: &str) -> Option<&DocumentNode> {
        self.documents.get(id)
    }

    /// Get a mutable reference to a document.
    pub fn get_document_mut(&mut self, id: &str) -> Option<&mut DocumentNode> {
        self.documents.get_mut(id)
    }

    /// Remove a document from the tree.
    pub fn remove_document(&mut self, id: &str) -> Option<DocumentNode> {
        self.root_document_ids.retain(|doc_id| doc_id != id);
        self.documents.remove(id)
    }

    /// Iterate over all documents.
    pub fn iter_documents(&self) -> impl Iterator<Item = &DocumentNode> {
        self.documents.values()
    }

    /// Get all document IDs.
    pub fn document_ids(&self) -> &[String] {
        &self.root_document_ids
    }

    /// Get the total number of documents.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Get the total number of sections across all documents.
    pub fn section_count(&self) -> usize {
        self.documents
            .values()
            .map(|doc| doc.total_section_count())
            .sum()
    }

    /// Get the total bytes of content across all documents.
    pub fn total_bytes(&self) -> usize {
        self.documents
            .values()
            .map(|doc| doc.content.len())
            .sum()
    }

    /// Flatten the tree into a list of all nodes (documents and sections).
    pub fn flatten(&self) -> Vec<FlatNode> {
        let mut nodes = Vec::new();
        for doc in self.documents.values() {
            nodes.push(FlatNode {
                id: doc.id.clone(),
                node_type: NodeType::Document,
                title: doc.title.clone(),
                path: doc.path.clone(),
                content: doc.content.clone(),
                parent_id: None,
                level: 0,
                metadata: doc.metadata.clone(),
            });
            for section in &doc.sections {
                self.flatten_section(&doc.id, section, 1, &mut nodes);
            }
        }
        nodes
    }

    fn flatten_section(
        &self,
        parent_id: &str,
        section: &SectionNode,
        level: usize,
        nodes: &mut Vec<FlatNode>,
    ) {
        nodes.push(FlatNode {
            id: section.id.clone(),
            node_type: NodeType::Section,
            title: section.title.clone(),
            path: format!("{}# {}", parent_id, section.title),
            content: section.content.clone(),
            parent_id: Some(parent_id.to_string()),
            level,
            metadata: DocumentMetadata::default(),
        });

        for child in &section.children {
            self.flatten_section(&section.id, child, level + 1, nodes);
        }
    }

    /// Build a path-based index for quick lookups.
    pub fn build_path_index(&self) -> HashMap<String, String> {
        let mut index = HashMap::new();
        for doc in self.documents.values() {
            index.insert(doc.path.clone(), doc.id.clone());
        }
        index
    }
}

/// A document node in the tree (root level).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentNode {
    /// Unique document ID
    pub id: String,
    /// Relative file path
    pub path: String,
    /// Document title (usually first heading or filename)
    pub title: String,
    /// Full document content
    pub content: String,
    /// Top-level sections
    pub sections: Vec<SectionNode>,
    /// Content hash for cache invalidation
    pub hash: String,
    /// Document metadata
    pub metadata: DocumentMetadata,
}

impl DocumentNode {
    /// Get the total count of all sections (including nested).
    pub fn total_section_count(&self) -> usize {
        self.sections
            .iter()
            .map(|s| 1 + s.total_child_count())
            .sum()
    }

    /// Find a section by ID within this document.
    pub fn find_section(&self, section_id: &str) -> Option<&SectionNode> {
        for section in &self.sections {
            if let found @ Some(_) = section.find_by_id(section_id) {
                return found;
            }
        }
        None
    }

    /// Get all text content (document + sections) as a single string.
    pub fn full_text(&self) -> String {
        let mut text = self.content.clone();
        for section in &self.sections {
            text.push_str(&section.full_text());
        }
        text
    }

    /// Get a summary of the document (first ~500 chars).
    pub fn summary(&self) -> String {
        let text = self.full_text();
        if text.len() > 500 {
            format!("{}...", &text[..500])
        } else {
            text
        }
    }
}

/// A section node within a document (can be nested).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionNode {
    /// Unique section ID
    pub id: String,
    /// Section heading/title
    pub title: String,
    /// Section level (1 = H1, 2 = H2, etc.)
    pub level: usize,
    /// Content under this section (excluding subsections)
    pub content: String,
    /// Child sections (subsections)
    pub children: Vec<SectionNode>,
    /// Line range in the original document [start, end)
    pub line_range: (usize, usize),
}

impl SectionNode {
    /// Create a new section node.
    pub fn new(id: String, title: String, level: usize) -> Self {
        Self {
            id,
            title,
            level,
            content: String::new(),
            children: Vec::new(),
            line_range: (0, 0),
        }
    }

    /// Add a child section.
    pub fn add_child(&mut self, child: SectionNode) {
        self.children.push(child);
    }

    /// Get the total count of all child sections (recursive).
    pub fn total_child_count(&self) -> usize {
        self.children
            .iter()
            .map(|c| 1 + c.total_child_count())
            .sum()
    }

    /// Find a section by ID (recursive).
    pub fn find_by_id(&self, id: &str) -> Option<&SectionNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &self.children {
            if let found @ Some(_) = child.find_by_id(id) {
                return found;
            }
        }
        None
    }

    /// Get all text content including children.
    pub fn full_text(&self) -> String {
        let mut text = format!("\n{}\n{}", self.title, self.content);
        for child in &self.children {
            text.push_str(&child.full_text());
        }
        text
    }

    /// Get the breadcrumb path (parent titles) for this section.
    pub fn breadcrumb(&self) -> Vec<String> {
        vec![self.title.clone()]
    }

    /// Check if this section contains a keyword (case-insensitive).
    pub fn contains_keyword(&self, keyword: &str) -> bool {
        let keyword = keyword.to_lowercase();
        self.title.to_lowercase().contains(&keyword)
            || self.content.to_lowercase().contains(&keyword)
            || self.children.iter().any(|c| c.contains_keyword(keyword.as_str()))
    }
}

/// Node type for flattened tree representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeType {
    Document,
    Section,
}

/// A flattened node representation for easy iteration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatNode {
    pub id: String,
    pub node_type: NodeType,
    pub title: String,
    pub path: String,
    pub content: String,
    pub parent_id: Option<String>,
    pub level: usize,
    pub metadata: DocumentMetadata,
}

/// Metadata for a document.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentMetadata {
    /// File extension
    pub extension: Option<String>,
    /// File size in bytes
    pub file_size: Option<usize>,
    /// Last modified timestamp
    pub last_modified: Option<i64>,
    /// Document language (if detectable)
    pub language: Option<String>,
    /// Number of lines
    pub line_count: Option<usize>,
    /// Frontmatter data (for Markdown)
    pub frontmatter: Option<HashMap<String, serde_json::Value>>,
}

impl DocumentMetadata {
    /// Create metadata from a file path and content.
    pub fn from_path_and_content(path: &str, content: &str) -> Self {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());

        let language = extension.as_ref().map(|ext| match ext.as_str() {
            "rs" => "rust",
            "py" => "python",
            "js" => "javascript",
            "ts" => "typescript",
            "tsx" => "typescript-react",
            "jsx" => "javascript-react",
            "go" => "go",
            "java" => "java",
            "cpp" | "hpp" | "cc" => "cpp",
            "c" | "h" => "c",
            "md" => "markdown",
            "json" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            _ => "text",
        }.to_string());

        Self {
            extension,
            file_size: Some(content.len()),
            last_modified: Some(chrono::Utc::now().timestamp()),
            language,
            line_count: Some(content.lines().count()),
            frontmatter: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_hierarchy() {
        let mut root = SectionNode::new("s1".to_string(), "Root".to_string(), 1);
        let child1 = SectionNode::new("s2".to_string(), "Child 1".to_string(), 2);
        let child2 = SectionNode::new("s3".to_string(), "Child 2".to_string(), 2);

        root.add_child(child1);
        root.add_child(child2);

        assert_eq!(root.total_child_count(), 2);
        assert!(root.find_by_id("s2").is_some());
        assert!(root.find_by_id("nonexistent").is_none());
    }

    #[test]
    fn test_document_tree() {
        let mut tree = DocumentTree::new();
        
        let doc = DocumentNode {
            id: "doc1".to_string(),
            path: "test.md".to_string(),
            title: "Test".to_string(),
            content: "Hello".to_string(),
            sections: vec![],
            hash: "abc".to_string(),
            metadata: DocumentMetadata::default(),
        };

        tree.add_document(doc);
        
        assert_eq!(tree.document_count(), 1);
        assert!(tree.get_document("doc1").is_some());
        assert!(tree.get_document("nonexistent").is_none());
    }

    #[test]
    fn test_flatten_tree() {
        let mut tree = DocumentTree::new();
        
        let mut doc = DocumentNode {
            id: "doc1".to_string(),
            path: "test.md".to_string(),
            title: "Test".to_string(),
            content: "Hello".to_string(),
            sections: vec![],
            hash: "abc".to_string(),
            metadata: DocumentMetadata::default(),
        };

        let section = SectionNode::new("s1".to_string(), "Section 1".to_string(), 1);
        doc.sections.push(section);
        
        tree.add_document(doc);
        
        let flat = tree.flatten();
        assert_eq!(flat.len(), 2); // Document + 1 section
    }

    #[test]
    fn test_keyword_search() {
        let mut section = SectionNode::new("s1".to_string(), "Introduction".to_string(), 1);
        section.content = "This is about Rust programming".to_string();

        assert!(section.contains_keyword("rust"));
        assert!(section.contains_keyword("programming"));
        assert!(!section.contains_keyword("python"));
    }
}
