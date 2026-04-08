//! Chat Content Indexer — Auto-index chat messages into implicit memory
//!
//! This module provides functionality to automatically index chat conversations
//! into the PageIndex system, enabling semantic search over chat history.
//!
//! ## Design
//!
//! Chat messages are indexed as virtual "documents" in the implicit memory:
//! - Each session becomes a document
//! - Messages are structured with timestamps and roles
//! - Incremental updates: only new messages are indexed
//!
//! ## Storage Structure
//!
//! ```text
//! .omiga/memory/
//! └── implicit/
//!     ├── tree.json          # Document tree with chat sessions
//!     ├── cache/             # Content hash cache
//!     └── content/           # Processed chat content
//!         └── chat_{session_id}.md
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::domain::pageindex::{CacheEntry, DocumentNode, DocumentParser, DocumentTree, IndexConfig, IndexStorage};
use crate::errors::AppError;

/// Chat message for indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: ChatRole,
    pub content: String,
    pub timestamp: i64,
    pub tool_calls: Option<Vec<ToolCallInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Chat content indexer
pub struct ChatIndexer {
    storage: IndexStorage,
    config: IndexConfig,
    tree: DocumentTree,
    cache: HashMap<String, CacheEntry>,
}

impl ChatIndexer {
    /// Create a new chat indexer
    pub fn new(memory_dir: impl Into<PathBuf>) -> Self {
        let memory_dir = memory_dir.into();
        let storage = IndexStorage::new(&memory_dir);
        let config = IndexConfig::default();
        let tree = DocumentTree::new();
        let cache = HashMap::new();

        Self {
            storage,
            config,
            tree,
            cache,
        }
    }

    /// Initialize the indexer
    pub async fn init(&self) -> std::io::Result<()> {
        // Storage::save_tree and save_cache will create directories automatically
        Ok(())
    }

    /// Load existing index data
    pub async fn load(&mut self) -> Result<(), AppError> {
        // Load tree if exists
        if let Some(tree) = self.storage.load_tree().await? {
            self.tree = tree;
        }

        // Load cache if exists
        if self.config.enable_cache {
            self.cache = self.storage.load_cache().await.unwrap_or_default();
        }

        Ok(())
    }

    /// Index a batch of chat messages from a session
    pub async fn index_session(
        &mut self,
        session_id: &str,
        session_name: &str,
        messages: &[ChatMessage],
    ) -> Result<(), AppError> {
        if messages.is_empty() {
            debug!("No messages to index for session {}", session_id);
            return Ok(());
        }

        // Generate document ID
        let doc_id = format!("chat_{}", session_id);
        let doc_path = format!("chat/{}.md", session_id);

        // Build content
        let content = self.format_chat_content(session_name, messages);

        // Calculate hash for change detection
        let hash = calculate_hash(&content);

        // Check if content changed
        if let Some(cached) = self.cache.get(&doc_path) {
            if cached.hash == hash {
                debug!("Chat session {} unchanged, skipping indexing", session_id);
                return Ok(());
            }
        }

        // Parse content structure
        let parser = DocumentParser::new(self.config.max_section_depth);
        let parse_result = parser.parse(&doc_path, &content)?;

        // Create document node
        let doc_node = DocumentNode {
            id: doc_id.clone(),
            path: doc_path.clone(),
            title: format!("Chat: {}", session_name),
            content: parse_result.content,
            sections: parse_result.sections,
            hash: hash.clone(),
            metadata: parse_result.metadata,
        };

        // Update tree
        self.tree.add_document(doc_node);

        // Update cache
        self.cache.insert(
            doc_path,
            CacheEntry {
                hash,
                last_indexed: chrono::Utc::now().timestamp(),
                doc_id,
            },
        );

        // Persist
        self.storage.save_tree(&self.tree).await?;
        if self.config.enable_cache {
            self.storage.save_cache(&self.cache).await?;
        }

        info!("Indexed chat session {} with {} messages", session_id, messages.len());
        Ok(())
    }

    /// Index a single new message (incremental update)
    pub async fn index_message(
        &mut self,
        session_id: &str,
        session_name: &str,
        message: &ChatMessage,
    ) -> Result<(), AppError> {
        // Get existing messages for this session from the document
        let doc_id = format!("chat_{}", session_id);
        
        // For incremental single-message updates, we need to:
        // 1. Check if document exists
        // 2. Append the new message content
        // 3. Re-parse and update
        
        let doc_path = format!("chat/{}.md", session_id);
        
        // Build the message content
        let message_content = format_single_message(message);
        
        // Get existing content if available
        let existing_content = if let Some(doc) = self.tree.get_document(&doc_id) {
            doc.content.clone()
        } else {
            format!("# Chat Session: {}\n\n", session_name)
        };
        
        // Append new message
        let new_content = format!("{}\n{}", existing_content, message_content);
        
        // Calculate hash
        let hash = calculate_hash(&new_content);
        
        // Check if changed
        if let Some(cached) = self.cache.get(&doc_path) {
            if cached.hash == hash {
                return Ok(());
            }
        }
        
        // Parse and update
        let parser = DocumentParser::new(self.config.max_section_depth);
        let parse_result = parser.parse(&doc_path, &new_content)?;
        
        let doc_node = DocumentNode {
            id: doc_id.clone(),
            path: doc_path.clone(),
            title: format!("Chat: {}", session_name),
            content: parse_result.content,
            sections: parse_result.sections,
            hash: hash.clone(),
            metadata: parse_result.metadata,
        };
        
        self.tree.add_document(doc_node);
        self.cache.insert(
            doc_path,
            CacheEntry {
                hash,
                last_indexed: chrono::Utc::now().timestamp(),
                doc_id,
            },
        );
        
        // Persist
        self.storage.save_tree(&self.tree).await?;
        if self.config.enable_cache {
            self.storage.save_cache(&self.cache).await?;
        }
        
        debug!("Indexed message {} for session {}", message.id, session_id);
        Ok(())
    }

    /// Format chat messages into markdown content
    fn format_chat_content(&self, session_name: &str, messages: &[ChatMessage]) -> String {
        let mut content = format!("# Chat Session: {}\n\n", session_name);
        content.push_str(&format!("> **Session ID**: {}\n\n",
            messages.first().map(|m| m.session_id.clone()).unwrap_or_default()
        ));
        content.push_str("---\n\n");

        for msg in messages {
            content.push_str(&format_single_message(msg));
        }

        content.push_str("\n---\n\n");
        content.push_str(&format!(
            "*Indexed on {}*\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        ));

        content
    }

    /// Get indexed document count
    pub fn document_count(&self) -> usize {
        self.tree.document_count()
    }

    /// Get tree reference
    pub fn tree(&self) -> &DocumentTree {
        &self.tree
    }
}

/// Format a single message
fn format_single_message(msg: &ChatMessage) -> String {
    let timestamp = chrono::DateTime::from_timestamp(msg.timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let (role_label, role_emoji) = match msg.role {
        ChatRole::User => ("User", "👤"),
        ChatRole::Assistant => ("Assistant", "🤖"),
        ChatRole::Tool => ("Tool", "🔧"),
    };

    let mut formatted = format!(
        "## {} {} ({}\n\n",
        role_emoji, role_label, timestamp
    );

    // Add content
    formatted.push_str(&msg.content);
    formatted.push_str("\n\n");

    // Add tool calls if present
    if let Some(ref calls) = msg.tool_calls {
        if !calls.is_empty() {
            formatted.push_str("**Tool Calls:**\n");
            for call in calls {
                formatted.push_str(&format!("- `{}` ({}\n", call.name, call.id));
            }
            formatted.push_str("\n");
        }
    }

    formatted
}

/// Calculate content hash
fn calculate_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_single_message() {
        let msg = ChatMessage {
            id: "msg_1".to_string(),
            session_id: "sess_1".to_string(),
            role: ChatRole::User,
            content: "Hello, how do I implement authentication?".to_string(),
            timestamp: 1704067200,
            tool_calls: None,
        };

        let formatted = format_single_message(&msg);
        assert!(formatted.contains("User"));
        assert!(formatted.contains("Hello, how do I implement authentication?"));
    }

    #[test]
    fn test_calculate_hash() {
        let content = "test content";
        let hash1 = calculate_hash(content);
        let hash2 = calculate_hash(content);
        let hash3 = calculate_hash("different content");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}
