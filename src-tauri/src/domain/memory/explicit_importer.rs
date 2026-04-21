//! Explicit Memory Importer
//!
//! Imports files, folders, and text content directly into explicit memory (wiki)
//! using PageIndex parsing algorithms - without LLM compression.
//!
//! ## Workflow
//! 1. Parse source files using PageIndex parsers
//! 2. Extract hierarchical structure (headings, sections)
//! 3. Generate wiki pages with structured content
//! 4. Update wiki index

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};

use crate::domain::pageindex::{DocumentParser, SectionNode};
use crate::errors::AppError;

/// Source type for import
#[derive(Debug, Clone)]
pub enum ImportSource {
    /// Single file
    File(PathBuf),
    /// Directory (recursive)
    Directory(PathBuf),
    /// Raw text content
    Text { title: String, content: String },
}

/// Import options
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Whether to include file content or just structure
    pub include_content: bool,
    /// Max content length per section (0 = unlimited)
    pub max_section_length: usize,
    /// Whether to create index page for directories
    pub create_index_pages: bool,
    /// Custom tags to add to imported pages
    pub tags: Vec<String>,
    /// Source reference to add to pages
    pub source_ref: Option<String>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            include_content: true,
            max_section_length: 5000,
            create_index_pages: true,
            tags: vec![],
            source_ref: None,
        }
    }
}

/// Import result
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub imported_count: usize,
    pub skipped_count: usize,
    pub errors: Vec<String>,
    pub created_pages: Vec<String>,
}

/// Importer for explicit memory
pub struct ExplicitImporter {
    project_root: PathBuf,
    wiki_dir: PathBuf,
    /// Directory where original raw files are copied on import.
    raw_dir: PathBuf,
    parser: DocumentParser,
    options: ImportOptions,
}

impl ExplicitImporter {
    pub fn new(
        project_root: impl AsRef<Path>,
        wiki_dir: impl AsRef<Path>,
        raw_dir: impl AsRef<Path>,
        options: ImportOptions,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            wiki_dir: wiki_dir.as_ref().to_path_buf(),
            raw_dir: raw_dir.as_ref().to_path_buf(),
            parser: DocumentParser::new(6), // max 6 levels of headings
            options,
        }
    }

    /// Import from source
    pub async fn import(&self, source: ImportSource) -> Result<ImportResult, AppError> {
        match source {
            ImportSource::File(path) => self.import_file(&path).await,
            ImportSource::Directory(path) => self.import_directory(&path).await,
            ImportSource::Text { title, content } => self.import_text(&title, &content).await,
        }
    }

    /// Import a single file
    async fn import_file(&self, path: &Path) -> Result<ImportResult, AppError> {
        let mut result = ImportResult {
            imported_count: 0,
            skipped_count: 0,
            errors: vec![],
            created_pages: vec![],
        };

        // Check if file is readable
        if !path.exists() {
            result
                .errors
                .push(format!("File not found: {}", path.display()));
            return Ok(result);
        }

        // Get relative path for reference
        let relative_path = path
            .strip_prefix(&self.project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // Read file content
        let content = match fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to read {}: {}", path.display(), e));
                return Ok(result);
            }
        };

        // Parse using PageIndex parser
        let parse_result = match self.parser.parse(&relative_path, &content) {
            Ok(r) => r,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to parse {}: {}", path.display(), e));
                return Ok(result);
            }
        };

        // Generate wiki page slug
        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled");
        let slug = slugify(file_name);

        // Copy raw original file to raw_dir so it survives directory restructuring.
        let raw_ref = self.copy_raw_file(path, &slug).await;

        let wiki_content = self.generate_wiki_content(
            &parse_result.title,
            &relative_path,
            raw_ref.as_deref(),
            &parse_result,
        );

        // Write wiki page
        let wiki_path = self.wiki_dir.join(format!("{}.md", slug));
        if let Err(e) = fs::write(&wiki_path, &wiki_content).await {
            result
                .errors
                .push(format!("Failed to write wiki page: {}", e));
            return Ok(result);
        }

        // Update wiki index
        if let Err(e) = self
            .update_wiki_index(&slug, &parse_result.title, &relative_path)
            .await
        {
            result.errors.push(format!("Failed to update index: {}", e));
        }

        result.imported_count = 1;
        result.created_pages.push(slug.clone());

        info!("Imported file {} to wiki page {}", path.display(), slug);

        Ok(result)
    }

    /// Copy the original file into `raw_dir/<slug>.<ext>` and return the destination path string.
    /// Logs a warning but does not fail the import if the copy cannot be completed.
    async fn copy_raw_file(&self, src: &Path, slug: &str) -> Option<String> {
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("bin");
        let dest_name = format!("{}.{}", slug, ext);
        let dest = self.raw_dir.join(&dest_name);

        if let Err(e) = fs::create_dir_all(&self.raw_dir).await {
            warn!("Cannot create raw_dir {}: {}", self.raw_dir.display(), e);
            return None;
        }

        if let Err(e) = fs::copy(src, &dest).await {
            warn!("Cannot copy raw file to {}: {}", dest.display(), e);
            return None;
        }

        Some(dest.to_string_lossy().into_owned())
    }

    /// Import directory recursively
    async fn import_directory(&self, path: &Path) -> Result<ImportResult, AppError> {
        let mut result = ImportResult {
            imported_count: 0,
            skipped_count: 0,
            errors: vec![],
            created_pages: vec![],
        };

        // Use a stack to avoid recursion in async
        let mut dirs_to_process: Vec<PathBuf> = vec![path.to_path_buf()];
        let mut all_created_pages: Vec<String> = vec![];

        while let Some(current_dir) = dirs_to_process.pop() {
            let mut entries = match fs::read_dir(&current_dir).await {
                Ok(e) => e,
                Err(e) => {
                    result
                        .errors
                        .push(format!("Failed to read directory: {}", e));
                    continue;
                }
            };

            let mut files_to_import = vec![];

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files and common ignore patterns
                if file_name.starts_with('.') || file_name == "node_modules" {
                    continue;
                }

                if path.is_dir() {
                    // Add to stack for later processing
                    dirs_to_process.push(path);
                } else if path.is_file() {
                    // Check if file extension is supported
                    if let Some(ext) = path.extension() {
                        let ext = ext.to_string_lossy().to_lowercase();
                        if is_supported_extension(&ext) {
                            files_to_import.push(path);
                        } else {
                            result.skipped_count += 1;
                        }
                    }
                }
            }

            // Import files in this directory
            for file_path in files_to_import {
                let file_result = self.import_file(&file_path).await?;
                result.imported_count += file_result.imported_count;
                result.skipped_count += file_result.skipped_count;
                result.errors.extend(file_result.errors);
                all_created_pages.extend(file_result.created_pages);
            }
        }

        result.created_pages = all_created_pages.clone();

        // Create directory index page if enabled
        if self.options.create_index_pages && !all_created_pages.is_empty() {
            let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("index");
            let index_slug = slugify(dir_name);

            if let Err(e) = self
                .create_directory_index(&index_slug, path, &all_created_pages)
                .await
            {
                warn!("Failed to create directory index: {}", e);
            }
        }

        Ok(result)
    }

    /// Import raw text
    async fn import_text(&self, title: &str, content: &str) -> Result<ImportResult, AppError> {
        let mut result = ImportResult {
            imported_count: 0,
            skipped_count: 0,
            errors: vec![],
            created_pages: vec![],
        };

        let slug = slugify(title);

        // Parse as markdown
        let parse_result = match self.parser.parse(&format!("{}.md", slug), content) {
            Ok(r) => r,
            Err(_e) => {
                // If parsing fails, create simple page with raw content
                let wiki_content = format!(
                    "# {}\n\n{}\n\n---\n\n*Imported from text*\n",
                    title, content
                );

                let wiki_path = self.wiki_dir.join(format!("{}.md", slug));
                fs::write(&wiki_path, &wiki_content)
                    .await
                    .map_err(|e| AppError::Unknown(e.to_string()))?;

                self.update_wiki_index(&slug, title, "text import").await?;

                result.imported_count = 1;
                result.created_pages.push(slug);
                return Ok(result);
            }
        };

        let wiki_content = self.generate_wiki_content(title, "text import", None, &parse_result);

        let wiki_path = self.wiki_dir.join(format!("{}.md", slug));
        fs::write(&wiki_path, &wiki_content)
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?;

        self.update_wiki_index(&slug, title, "text import").await?;

        result.imported_count = 1;
        result.created_pages.push(slug.clone());

        info!("Imported text '{}' to wiki page {}", title, slug);

        Ok(result)
    }

    /// Generate wiki content from parsed document
    fn generate_wiki_content(
        &self,
        title: &str,
        source_path: &str,
        raw_path: Option<&str>,
        parse_result: &crate::domain::pageindex::ParseResult,
    ) -> String {
        let mut content = String::new();

        // Header
        content.push_str(&format!("# {}\n\n", title));

        // Source reference (original import path)
        content.push_str(&format!("> **Source**: `{}`\n\n", source_path));

        // Raw file reference (stable copy, survives directory moves)
        if let Some(raw) = raw_path {
            content.push_str(&format!("> **Raw**: `{}`\n\n", raw));
        }

        // Tags if any
        if !self.options.tags.is_empty() {
            content.push_str(&format!("> **Tags**: {}\n\n", self.options.tags.join(", ")));
        }

        // Main content
        if !parse_result.sections.is_empty() {
            // Generate table of contents
            content.push_str("## Contents\n\n");
            for (i, section) in parse_result.sections.iter().enumerate() {
                content.push_str(&format!(
                    "{}. [{}](#{})\n",
                    i + 1,
                    section.title,
                    slugify(&section.title)
                ));
            }
            content.push_str("\n---\n\n");

            // Generate sections
            for section in &parse_result.sections {
                content.push_str(&self.format_section(section, 2));
            }
        } else {
            // No sections, just add content
            let body = if self.options.max_section_length > 0
                && parse_result.content.len() > self.options.max_section_length
            {
                format!(
                    "{}\n\n*[Content truncated...]*",
                    &parse_result.content[..self.options.max_section_length]
                )
            } else {
                parse_result.content.clone()
            };
            content.push_str(&body);
        }

        // Footer
        content.push_str("\n\n---\n\n");
        content.push_str(&format!(
            "*Imported on {} from {}*\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M"),
            source_path
        ));

        content
    }

    /// Format a section recursively
    fn format_section(&self, section: &SectionNode, level: usize) -> String {
        let mut result = String::new();

        // Section heading
        let heading_prefix = "#".repeat(level);
        result.push_str(&format!("{} {}\n\n", heading_prefix, section.title));

        // Section content
        if !section.content.is_empty() {
            let body = if self.options.max_section_length > 0
                && section.content.len() > self.options.max_section_length
            {
                format!(
                    "{}\n\n*[Content truncated...]*",
                    &section.content[..self.options.max_section_length]
                )
            } else {
                section.content.clone()
            };
            result.push_str(&body);
            result.push_str("\n\n");
        }

        // Subsections
        for child in &section.children {
            result.push_str(&self.format_section(child, level + 1));
        }

        result
    }

    /// Update wiki index with new page
    async fn update_wiki_index(
        &self,
        slug: &str,
        title: &str,
        source: &str,
    ) -> Result<(), AppError> {
        let index_path = self.wiki_dir.join("index.md");

        // Read existing index or create new
        let mut index_content = if index_path.exists() {
            fs::read_to_string(&index_path).await.unwrap_or_default()
        } else {
            "# Wiki Index\n\n".to_string()
        };

        // Check if entry already exists
        let entry_pattern = format!("({}.md)", slug);
        if !index_content.contains(&entry_pattern) {
            let entry = format!("- [{}]({}.md) — Imported from `{}`\n", title, slug, source);
            index_content.push_str(&entry);

            fs::write(&index_path, index_content)
                .await
                .map_err(|e| AppError::Unknown(e.to_string()))?;
        }

        Ok(())
    }

    /// Create directory index page
    async fn create_directory_index(
        &self,
        slug: &str,
        dir_path: &Path,
        pages: &[String],
    ) -> Result<(), AppError> {
        let dir_name = dir_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Index");

        let mut content = format!("# {} Index\n\n", dir_name);
        content.push_str(&format!(
            "> Directory index for `{}`\n\n",
            dir_path.display()
        ));
        content.push_str("## Pages\n\n");

        for page_slug in pages {
            content.push_str(&format!("- [[{}]]\n", page_slug));
        }

        content.push_str("\n---\n\n");
        content.push_str(&format!(
            "*Auto-generated on {}*\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        ));

        let index_path = self.wiki_dir.join(format!("{}.md", slug));
        fs::write(&index_path, content)
            .await
            .map_err(|e| AppError::Unknown(e.to_string()))?;

        // Add to main index
        let _ = self
            .update_wiki_index(
                slug,
                &format!("{} Index", dir_name),
                &dir_path.to_string_lossy(),
            )
            .await;

        Ok(())
    }
}

/// Convert string to URL-friendly slug
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .replace(" ", "-")
        .replace("_", "-")
        .replace("/", "-")
        .replace("\\", "-")
        .replace(".", "-")
        .replace(",", "")
        .replace("'", "")
        .replace('"', "")
        .replace("(", "")
        .replace(")", "")
        .replace("[", "")
        .replace("]", "")
        .replace("{", "")
        .replace("}", "")
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

/// Check if file extension is supported for explicit memory import
///
/// Explicit memory focuses on text-based content files:
/// - Documents: Markdown, plain text, rich text
/// - Structured data: JSON, YAML, TOML (for configuration knowledge)
/// - Web content: HTML
/// - Office documents: PDF (requires special handling)
///
/// Note: Source code files should be indexed via implicit memory (PageIndex)
/// rather than imported into explicit memory.
fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext,
        // Document formats
        "md" | "txt" | "rtf" |
        // Data/Config formats (for knowledge, not code)
        "json" | "yaml" | "yml" | "toml" |
        // Web content
        "html" | "htm" |
        // Documentation formats
        "pdf"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("File_Name"), "file-name");
        assert_eq!(slugify("path/to/file"), "path-to-file");
        assert_eq!(slugify("  trim-me  "), "trim-me");
    }

    #[test]
    fn test_is_supported_extension() {
        // Document formats
        assert!(is_supported_extension("md"));
        assert!(is_supported_extension("txt"));
        assert!(is_supported_extension("rtf"));
        assert!(is_supported_extension("pdf"));

        // Data/Config formats
        assert!(is_supported_extension("json"));
        assert!(is_supported_extension("yaml"));
        assert!(is_supported_extension("yml"));
        assert!(is_supported_extension("toml"));

        // Web content
        assert!(is_supported_extension("html"));
        assert!(is_supported_extension("htm"));

        // Code files should NOT be supported (use implicit memory instead)
        assert!(!is_supported_extension("rs"));
        assert!(!is_supported_extension("py"));
        assert!(!is_supported_extension("js"));
        assert!(!is_supported_extension("ts"));

        // Binary files should NOT be supported
        assert!(!is_supported_extension("exe"));
        assert!(!is_supported_extension("dll"));
        assert!(!is_supported_extension("bin"));
    }
}
