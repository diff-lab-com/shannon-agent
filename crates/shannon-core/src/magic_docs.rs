//! # Magic Documentation
//!
//! Automatic documentation generation service for Shannon Code. Produces
//! documentation stubs from module and source file names without requiring
//! external tools or language servers.
//!
//! ## Architecture
//!
//! - [`MagicDocsService`]: Orchestrates documentation generation
//! - [`DocSection`]: A single documentation section (module, function, or type)
//! - [`DocGenerationRequest`]: Input parameters for a generation run
//! - [`DocOutput`]: The full generated documentation with metadata
//!
//! The service works with source paths (real or synthetic) and produces
//! structured documentation stubs that can be further refined by an AI or
//! human editor.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors that can occur during documentation generation.
#[derive(Debug, Error)]
pub enum MagicDocsError {
    /// No source paths were provided.
    #[error("No source paths provided")]
    NoSourcePaths,

    /// A path could not be processed.
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// An output format is not supported.
    #[error("Unsupported output format: {0}")]
    UnsupportedFormat(String),
}

// ---------------------------------------------------------------------------
// DocSection
// ---------------------------------------------------------------------------

/// The granularity level of a documentation section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DocLevel {
    /// A top-level module or crate.
    Module,
    /// A function or method.
    Function,
    /// A type, struct, enum, or trait.
    Type,
}

impl std::fmt::Display for DocLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module => write!(f, "module"),
            Self::Function => write!(f, "function"),
            Self::Type => write!(f, "type"),
        }
    }
}

/// A single section of generated documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSection {
    /// Section title (derived from the source path or symbol name).
    pub title: String,

    /// Generated documentation content (markdown by default).
    pub content: String,

    /// Ordering index within the output.
    pub order: usize,

    /// Granularity level of this section.
    pub level: DocLevel,

    /// The source path this section was derived from.
    pub source_path: String,
}

impl DocSection {
    /// Create a new documentation section.
    pub fn new(
        title: impl Into<String>,
        content: impl Into<String>,
        order: usize,
        level: DocLevel,
        source_path: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
            order,
            level,
            source_path: source_path.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

/// Supported output formats for generated documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DocOutputFormat {
    /// Markdown output.
    Markdown,
    /// HTML output.
    Html,
    /// JSON output.
    Json,
}

impl std::fmt::Display for DocOutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Markdown => write!(f, "markdown"),
            Self::Html => write!(f, "html"),
            Self::Json => write!(f, "json"),
        }
    }
}

// ---------------------------------------------------------------------------
// DocGenerationRequest
// ---------------------------------------------------------------------------

/// Input parameters for a documentation generation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocGenerationRequest {
    /// Source paths (files or directories) to document.
    pub source_paths: Vec<String>,

    /// Desired output format.
    pub output_format: DocOutputFormat,

    /// Whether to include private / non-public items.
    pub include_private: bool,

    /// Optional title for the generated documentation.
    pub title: Option<String>,

    /// Maximum depth when processing directories.
    pub max_depth: usize,
}

impl DocGenerationRequest {
    /// Create a new generation request for the given source paths.
    pub fn new(source_paths: Vec<String>) -> Self {
        Self {
            source_paths,
            output_format: DocOutputFormat::Markdown,
            include_private: false,
            title: None,
            max_depth: 5,
        }
    }

    /// Set the output format.
    pub fn with_format(mut self, format: DocOutputFormat) -> Self {
        self.output_format = format;
        self
    }

    /// Include private items.
    pub fn include_private(mut self) -> Self {
        self.include_private = true;
        self
    }

    /// Set a title for the output.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

// ---------------------------------------------------------------------------
// DocOutput
// ---------------------------------------------------------------------------

/// The full result of a documentation generation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocOutput {
    /// Ordered list of documentation sections.
    pub sections: Vec<DocSection>,

    /// Metadata about the generation run.
    pub metadata: DocMetadata,
}

/// Metadata attached to a [`DocOutput`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocMetadata {
    /// Total number of sections generated.
    pub total_sections: usize,

    /// When the documentation was generated.
    pub generated_at: DateTime<Utc>,

    /// Number of source paths processed.
    pub source_count: usize,

    /// Output format used.
    pub output_format: DocOutputFormat,

    /// Title of the documentation, if provided.
    pub title: Option<String>,
}

impl DocOutput {
    /// Create a new documentation output.
    pub fn new(sections: Vec<DocSection>, metadata: DocMetadata) -> Self {
        Self { sections, metadata }
    }
}

// ---------------------------------------------------------------------------
// MagicDocsService
// ---------------------------------------------------------------------------

/// Service for automatic documentation generation.
///
/// Produces documentation stubs from source paths. The stubs include module
/// titles, placeholder descriptions, and structured section metadata that
/// can be further refined by AI or human editors.
pub struct MagicDocsService {
    /// Custom templates for different doc levels (keyed by level name).
    templates: HashMap<String, String>,
}

impl Default for MagicDocsService {
    fn default() -> Self {
        Self::new()
    }
}

impl MagicDocsService {
    /// Create a new magic docs service with default templates.
    pub fn new() -> Self {
        let mut templates = HashMap::new();
        templates.insert(
            "module".to_string(),
            "# {title}\n\n> Auto-generated documentation stub.\n\n## Overview\n\n\
             TODO: Add module overview.\n\n## Examples\n\n```rust\n// TODO: Add usage examples.\n```\n"
                .to_string(),
        );
        templates.insert(
            "function".to_string(),
            "## `{title}`\n\n```rust\n// TODO: Add function signature.\nfn {title}() {{}}\n```\n\n\
             **Description:** TODO\n\n**Parameters:** TODO\n\n**Returns:** TODO\n\n\
             **Examples:**\n\n```rust\n// TODO\n```\n"
                .to_string(),
        );
        templates.insert(
            "type".to_string(),
            "## `{title}`\n\n```rust\n// TODO: Add type definition.\nstruct {title};\n```\n\n\
             **Description:** TODO\n\n**Fields:** TODO\n\n**Implementations:** TODO\n"
                .to_string(),
        );
        Self { templates }
    }

    /// Register a custom template for a given doc level.
    ///
    /// Templates may contain `{title}` placeholders that will be replaced
    /// with the section title.
    pub fn set_template(&mut self, level: &str, template: impl Into<String>) {
        self.templates.insert(level.to_string(), template.into());
    }

    /// Generate documentation from the given request.
    pub fn generate_docs(&self, request: &DocGenerationRequest) -> Result<DocOutput, MagicDocsError> {
        if request.source_paths.is_empty() {
            return Err(MagicDocsError::NoSourcePaths);
        }

        let source_count = request.source_paths.len();
        let mut sections = Vec::new();
        let mut order = 0;

        for source_path in &request.source_paths {
            let path = Path::new(source_path);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(source_path);

            // Determine the doc level from the path
            let level = self.infer_level(path);

            // Skip private files if not requested
            let title = stem.to_string();
            if !request.include_private && self.is_private(&title) {
                continue;
            }

            // Select template and render content
            let template_key = level.to_string();
            let template = self
                .templates
                .get(&template_key)
                .cloned()
                .unwrap_or_else(|| format!("# {}\n\nTODO: Add documentation.\n", title));

            let content = template.replace("{title}", &title);

            sections.push(DocSection::new(
                &title,
                content,
                order,
                level,
                source_path,
            ));
            order += 1;
        }

        let metadata = DocMetadata {
            total_sections: sections.len(),
            generated_at: Utc::now(),
            source_count,
            output_format: request.output_format,
            title: request.title.clone(),
        };

        Ok(DocOutput::new(sections, metadata))
    }

    /// Render the documentation output into the requested format.
    pub fn render(&self, output: &DocOutput) -> String {
        match output.metadata.output_format {
            DocOutputFormat::Markdown => self.render_markdown(output),
            DocOutputFormat::Html => self.render_html(output),
            DocOutputFormat::Json => serde_json::to_string_pretty(output)
                .unwrap_or_else(|_| "[]".to_string()),
        }
    }

    /// Render as plain Markdown (concatenate all sections).
    fn render_markdown(&self, output: &DocOutput) -> String {
        let mut parts = Vec::new();

        if let Some(ref title) = output.metadata.title {
            parts.push(format!("# {}", title));
            parts.push(String::new());
        }

        for section in &output.sections {
            parts.push(section.content.clone());
            parts.push(String::new());
        }

        parts.join("\n")
    }

    /// Render as a basic HTML document.
    fn render_html(&self, output: &DocOutput) -> String {
        let mut body = String::new();

        if let Some(ref title) = output.metadata.title {
            body.push_str(&format!("<h1>{}</h1>\n", title));
        }

        for section in &output.sections {
            let heading = match section.level {
                DocLevel::Module => "h2",
                DocLevel::Function | DocLevel::Type => "h3",
            };
            body.push_str(&format!("<{}>{}</{}>\n", heading, section.title, heading));
            // Convert newlines to <br> for simple rendering
            let html_content = section.content.replace('\n', "<br>\n");
            body.push_str(&format!("<div class=\"doc-section\">{}</div>\n", html_content));
        }

        format!(
            "<!DOCTYPE html>\n<html><head><title>{}</title></head><body>\n{}</body></html>",
            output.metadata.title.as_deref().unwrap_or("Documentation"),
            body,
        )
    }

    /// Infer the documentation level from a file path.
    fn infer_level(&self, path: &Path) -> DocLevel {
        match path.extension().and_then(|e| e.to_str()) {
            // Module-level files (mod.rs, lib.rs, main.rs)
            Some("rs") => {
                let file_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("");
                if file_name == "mod.rs"
                    || file_name == "lib.rs"
                    || file_name == "main.rs"
                {
                    DocLevel::Module
                } else {
                    DocLevel::Module
                }
            }
            Some("py") => {
                let file_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("");
                if file_name == "__init__.py" || file_name == "__main__.py" {
                    DocLevel::Module
                } else {
                    DocLevel::Module
                }
            }
            _ => DocLevel::Module,
        }
    }

    /// Heuristic to determine if a source item is "private".
    fn is_private(&self, name: &str) -> bool {
        name.starts_with('_') || name.ends_with("_internal") || name.ends_with("_priv")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_docs_basic() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "src/utils.rs".to_string(),
        ]);

        let output = service.generate_docs(&request).unwrap();
        assert_eq!(output.sections.len(), 3);
        assert_eq!(output.metadata.source_count, 3);
        assert_eq!(output.metadata.total_sections, 3);
        assert_eq!(output.metadata.output_format, DocOutputFormat::Markdown);
    }

    #[test]
    fn test_generate_docs_empty_paths() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec![]);

        let err = service.generate_docs(&request).unwrap_err();
        assert!(matches!(err, MagicDocsError::NoSourcePaths));
    }

    #[test]
    fn test_generate_docs_private_filtering() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec![
            "src/public.rs".to_string(),
            "src/_internal.rs".to_string(),
            "src/utils_priv.rs".to_string(),
        ]);

        let output = service.generate_docs(&request).unwrap();
        assert_eq!(output.sections.len(), 1);
        assert_eq!(output.sections[0].title, "public");
    }

    #[test]
    fn test_generate_docs_include_private() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec![
            "src/public.rs".to_string(),
            "src/_internal.rs".to_string(),
        ])
        .include_private();

        let output = service.generate_docs(&request).unwrap();
        assert_eq!(output.sections.len(), 2);
    }

    #[test]
    fn test_generate_docs_with_title() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec!["src/lib.rs".to_string()])
            .with_title("My Crate");

        let output = service.generate_docs(&request).unwrap();
        assert_eq!(output.metadata.title, Some("My Crate".to_string()));
    }

    #[test]
    fn test_generate_docs_html_format() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec!["src/main.rs".to_string()])
            .with_format(DocOutputFormat::Html);

        let output = service.generate_docs(&request).unwrap();
        let rendered = service.render(&output);
        assert!(rendered.contains("<!DOCTYPE html>"));
        assert!(rendered.contains("<html>"));
        assert!(rendered.contains("<div class=\"doc-section\">"));
    }

    #[test]
    fn test_generate_docs_json_format() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec!["src/main.rs".to_string()])
            .with_format(DocOutputFormat::Json);

        let output = service.generate_docs(&request).unwrap();
        let rendered = service.render(&output);

        // Should parse as valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert!(parsed.get("sections").is_some());
        assert!(parsed.get("metadata").is_some());
    }

    #[test]
    fn test_render_markdown_with_title() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec!["src/lib.rs".to_string()])
            .with_title("Test Docs");

        let output = service.generate_docs(&request).unwrap();
        let rendered = service.render(&output);
        assert!(rendered.starts_with("# Test Docs"));
    }

    #[test]
    fn test_doc_section_ordering() {
        let service = MagicDocsService::new();
        let request = DocGenerationRequest::new(vec![
            "src/zebra.rs".to_string(),
            "src/alpha.rs".to_string(),
            "src/beta.rs".to_string(),
        ]);

        let output = service.generate_docs(&request).unwrap();
        // Sections should preserve input order, not alphabetical
        assert_eq!(output.sections[0].order, 0);
        assert_eq!(output.sections[1].order, 1);
        assert_eq!(output.sections[2].order, 2);
    }

    #[test]
    fn test_custom_template() {
        let mut service = MagicDocsService::new();
        service.set_template("module", "Custom: {title}\n");

        let request = DocGenerationRequest::new(vec!["src/lib.rs".to_string()]);
        let output = service.generate_docs(&request).unwrap();

        assert_eq!(output.sections[0].content, "Custom: lib\n");
    }

    #[test]
    fn test_doc_level_display() {
        assert_eq!(DocLevel::Module.to_string(), "module");
        assert_eq!(DocLevel::Function.to_string(), "function");
        assert_eq!(DocLevel::Type.to_string(), "type");
    }

    #[test]
    fn test_doc_output_format_display() {
        assert_eq!(DocOutputFormat::Markdown.to_string(), "markdown");
        assert_eq!(DocOutputFormat::Html.to_string(), "html");
        assert_eq!(DocOutputFormat::Json.to_string(), "json");
    }

    #[test]
    fn test_doc_serialization_round_trip() {
        let section = DocSection::new("my_module", "Content here", 0, DocLevel::Module, "src/my_module.rs");
        let json = serde_json::to_string(&section).unwrap();
        let decoded: DocSection = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.title, "my_module");
        assert_eq!(decoded.content, "Content here");
        assert_eq!(decoded.level, DocLevel::Module);
        assert_eq!(decoded.order, 0);
    }

    #[test]
    fn test_doc_metadata_fields() {
        let request = DocGenerationRequest::new(vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
        ])
        .with_format(DocOutputFormat::Html)
        .with_title("Test");

        let service = MagicDocsService::new();
        let output = service.generate_docs(&request).unwrap();
        let meta = &output.metadata;

        assert_eq!(meta.total_sections, 2);
        assert_eq!(meta.source_count, 2);
        assert_eq!(meta.output_format, DocOutputFormat::Html);
        assert_eq!(meta.title, Some("Test".to_string()));
    }
}
