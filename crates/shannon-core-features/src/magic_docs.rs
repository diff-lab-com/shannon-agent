//! # Magic Docs
//!
//! Automatic documentation generation with template rendering.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

// ============================================================================
// Error
// ============================================================================

/// Errors that can occur during documentation generation.
#[derive(Debug, Error)]
pub enum MagicDocsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Template error: {0}")]
    Template(String),

    #[error("Rendering error: {0}")]
    Rendering(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Invalid output format: {0}")]
    InvalidFormat(String),
}

// ============================================================================
// Documentation Request
// ============================================================================

/// A request to generate documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocRequest {
    /// Unique request identifier.
    pub id: Uuid,

    /// Title or topic of the documentation.
    pub title: String,

    /// Context about the code or subject.
    pub context: Vec<String>,

    /// Target programming language.
    pub language: String,

    /// Output format (markdown, html, etc.).
    pub format: DocFormat,

    /// Additional variables for template rendering.
    pub variables: HashMap<String, String>,
}

impl DocRequest {
    /// Create a new documentation request.
    pub fn new(title: String, language: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            title,
            context: Vec::new(),
            language,
            format: DocFormat::Markdown,
            variables: HashMap::new(),
        }
    }

    /// Add context lines.
    pub fn with_context(mut self, context: Vec<String>) -> Self {
        self.context = context;
        self
    }

    /// Set the output format.
    pub fn with_format(mut self, format: DocFormat) -> Self {
        self.format = format;
        self
    }

    /// Add a template variable.
    pub fn with_variable(mut self, key: String, value: String) -> Self {
        self.variables.insert(key, value);
        self
    }
}

// ============================================================================
// Output Format
// ============================================================================

/// Supported documentation output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocFormat {
    Markdown,
    Html,
    PlainText,
}

impl DocFormat {
    /// Get the file extension for this format.
    pub fn extension(&self) -> &str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::PlainText => "txt",
        }
    }
}

// ============================================================================
// Documentation Response
// ============================================================================

/// A generated documentation response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocResponse {
    /// Unique response identifier.
    pub id: Uuid,

    /// The request that generated this response.
    pub request_id: Uuid,

    /// Generated content.
    pub content: String,

    /// Sources or references used.
    pub sources: Vec<String>,

    /// Confidence score (0.0 - 1.0).
    pub confidence: f64,

    /// Output format used.
    pub format: DocFormat,

    /// Time taken to generate (milliseconds).
    pub generation_time_ms: u64,
}

// ============================================================================
// Template
// ============================================================================

/// A documentation template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// Template name.
    pub name: String,

    /// Template content with placeholders.
    pub content: String,

    /// Default output format.
    pub format: DocFormat,
}

impl Template {
    /// Create a new template.
    pub fn new(name: String, content: String, format: DocFormat) -> Self {
        Self {
            name,
            content,
            format,
        }
    }

    /// Render the template with the given variables.
    pub fn render(&self, variables: &HashMap<String, String>) -> Result<String, MagicDocsError> {
        let mut result = self.content.clone();

        // Replace placeholders in the format {{variable}}
        for (key, value) in variables {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }

        // Check for unreplaced placeholders
        if result.contains("{{") {
            return Err(MagicDocsError::Rendering(
                "Unreplaced placeholders in template".to_string(),
            ));
        }

        Ok(result)
    }
}

// ============================================================================
// Magic Docs Service
// ============================================================================

/// Service for generating documentation from templates.
pub struct MagicDocsService {
    templates: HashMap<String, Template>,
    output_dir: Option<PathBuf>,
}

impl MagicDocsService {
    /// Create a new magic docs service.
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
            output_dir: None,
        }
    }

    /// Set the output directory for generated documentation.
    pub fn with_output_dir(mut self, dir: PathBuf) -> Self {
        self.output_dir = Some(dir);
        self
    }

    /// Register a template.
    pub fn register_template(&mut self, template: Template) {
        self.templates.insert(template.name.clone(), template);
    }

    /// Get a template by name.
    pub fn get_template(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }

    /// Generate documentation from a request.
    pub fn generate(&mut self, request: DocRequest) -> Result<DocResponse, MagicDocsError> {
        let start = std::time::Instant::now();

        // Select template based on language
        let template_name = format!("{}_doc", request.language);
        let template = self
            .templates
            .get(&template_name)
            .or_else(|| self.templates.get("default"))
            .ok_or_else(|| MagicDocsError::TemplateNotFound(template_name))?;

        // Build template variables
        let mut variables = request.variables.clone();
        variables.insert("title".to_string(), request.title.clone());
        variables.insert("language".to_string(), request.language.clone());

        // Add context as a code block
        if !request.context.is_empty() {
            let context_text = request.context.join("\n");
            variables.insert("context".to_string(), context_text);
        }

        // Render template
        let content = template.render(&variables)?;

        let generation_time_ms = start.elapsed().as_millis() as u64;

        Ok(DocResponse {
            id: Uuid::new_v4(),
            request_id: request.id,
            content,
            sources: vec![],
            confidence: 1.0,
            format: request.format,
            generation_time_ms,
        })
    }

    /// Generate and save documentation to a file.
    pub fn generate_and_save(
        &mut self,
        request: DocRequest,
        file_name: &str,
    ) -> Result<PathBuf, MagicDocsError> {
        let response = self.generate(request)?;

        let output_dir = self
            .output_dir
            .as_ref()
            .ok_or_else(|| MagicDocsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Output directory not set",
            )))?;

        // Ensure directory exists
        fs::create_dir_all(output_dir)?;

        let file_path = output_dir.join(format!(
            "{}.{}",
            file_name,
            response.format.extension()
        ));

        fs::write(&file_path, &response.content)?;

        Ok(file_path)
    }

    /// Load templates from a directory.
    pub fn load_templates_from_dir(&mut self, dir: &PathBuf) -> Result<(), MagicDocsError> {
        if !dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("tmpl") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| MagicDocsError::Template(path.to_string_lossy().to_string()))?
                    .to_string();

                let content = fs::read_to_string(&path)?;

                let format = if name.ends_with("_md") {
                    DocFormat::Markdown
                } else if name.ends_with("_html") {
                    DocFormat::Html
                } else {
                    DocFormat::PlainText
                };

                let template = Template::new(name.clone(), content, format);
                self.templates.insert(name, template);
            }
        }

        Ok(())
    }

    /// Get all registered template names.
    pub fn template_names(&self) -> Vec<&String> {
        self.templates.keys().collect()
    }
}

impl Default for MagicDocsService {
    fn default() -> Self {
        let mut service = Self::new();

        // Register default templates
        service.register_template(Template::new(
            "default".to_string(),
            "# {{title}}\n\n{{context}}".to_string(),
            DocFormat::Markdown,
        ));

        service.register_template(Template::new(
            "rust_doc".to_string(),
            "//! # {{title}}\n//!\n//! {{context}}\n".to_string(),
            DocFormat::PlainText,
        ));

        service.register_template(Template::new(
            "python_doc".to_string(),
            "\"\"\"\n{{title}}\n\n{{context}}\n\"\"\"\n".to_string(),
            DocFormat::PlainText,
        ));

        service.register_template(Template::new(
            "js_doc".to_string(),
            "/**\n * {{title}}\n *\n * {{context}}\n */\n".to_string(),
            DocFormat::PlainText,
        ));

        service
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_request_creation() {
        let request = DocRequest::new("Test API".to_string(), "rust".to_string())
            .with_context(vec!["Line 1".to_string(), "Line 2".to_string()])
            .with_format(DocFormat::Html)
            .with_variable("author".to_string(), "Test".to_string());

        assert_eq!(request.title, "Test API");
        assert_eq!(request.context.len(), 2);
        assert_eq!(request.format, DocFormat::Html);
        assert_eq!(request.variables.get("author"), Some(&"Test".to_string()));
    }

    #[test]
    fn test_template_render() {
        let template = Template::new(
            "test".to_string(),
            "Hello {{name}}, welcome to {{place}}!".to_string(),
            DocFormat::PlainText,
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());
        vars.insert("place".to_string(), "Here".to_string());

        let result = template.render(&vars).unwrap();
        assert_eq!(result, "Hello World, welcome to Here!");
    }

    #[test]
    fn test_template_unreplaced_placeholder() {
        let template = Template::new(
            "test".to_string(),
            "Hello {{name}}, unreplaced: {{missing}}!".to_string(),
            DocFormat::PlainText,
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "World".to_string());

        let result = template.render(&vars);
        assert!(result.is_err());
    }

    #[test]
    fn test_magic_docs_generate() {
        let mut service = MagicDocsService::default();

        let request = DocRequest::new("Test Function".to_string(), "rust".to_string())
            .with_context(vec!["fn test() {}".to_string()]);

        let response = service.generate(request).unwrap();

        assert_eq!(response.format, DocFormat::Markdown);
        assert!(response.content.contains("# Test Function"));
        assert!(response.content.contains("fn test() {}"));
    }

    #[test]
    fn test_magic_docs_language_specific_template() {
        let mut service = MagicDocsService::default();

        // Test with Python
        let request = DocRequest::new("MyClass".to_string(), "python".to_string())
            .with_context(vec!["class MyClass:".to_string()]);

        let response = service.generate(request).unwrap();

        // Should use python_doc template
        assert!(response.content.contains("\"\"\""));
        assert!(response.content.contains("MyClass"));
    }

    #[test]
    fn test_doc_format_extensions() {
        assert_eq!(DocFormat::Markdown.extension(), "md");
        assert_eq!(DocFormat::Html.extension(), "html");
        assert_eq!(DocFormat::PlainText.extension(), "txt");
    }

    #[test]
    fn test_magic_docs_template_management() {
        let mut service = MagicDocsService::new();

        assert_eq!(service.template_names().len(), 0);

        let template = Template::new("custom".to_string(), "Custom: {{var}}".to_string(), DocFormat::PlainText);
        service.register_template(template);

        assert_eq!(service.template_names().len(), 1);
        assert!(service.get_template("custom").is_some());
    }

    #[test]
    fn test_doc_response_serialization() {
        let response = DocResponse {
            id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            content: "Test content".to_string(),
            sources: vec!["source1".to_string()],
            confidence: 0.9,
            format: DocFormat::Markdown,
            generation_time_ms: 100,
        };

        let json = serde_json::to_string(&response).unwrap();
        let decoded: DocResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.content, "Test content");
        assert_eq!(decoded.confidence, 0.9);
        assert_eq!(decoded.format, DocFormat::Markdown);
    }

    #[test]
    fn test_magic_docs_generate_and_save() {
        let temp_dir = std::env::temp_dir();
        let mut service = MagicDocsService::default().with_output_dir(temp_dir.clone());

        let request = DocRequest::new("Test".to_string(), "rust".to_string());
        let file_path = service.generate_and_save(request, "test_doc").unwrap();

        assert!(file_path.exists());
        assert!(file_path.to_string_lossy().ends_with(".md"));

        // Cleanup
        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn test_magic_docs_custom_variables_in_context() {
        let mut service = MagicDocsService::default();

        let request = DocRequest::new("Custom".to_string(), "rust".to_string())
            .with_variable("custom_field".to_string(), "custom_value".to_string());

        // Default template doesn't have {{custom_field}}, so it should still work
        let response = service.generate(request).unwrap();
        assert!(response.content.contains("# Custom"));
    }
}
