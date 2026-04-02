//! /pdf command - Process PDF documents

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// PDF processing prompt template
const PDF_PROMPT: &str = r##"
You are analyzing a PDF document. The content has been extracted using OCR/text extraction.

## Analysis Guidelines

1. **Content Overview**: Summarize the document's purpose and main topics
2. **Key Information**: Extract important data, figures, tables, or quotes
3. **Structure**: Identify sections, headings, and organization
4. **Quality Assessment**: Note any OCR errors, missing content, or formatting issues
5. **Relevance**: Assess how this document relates to the user's task

## Output Format

Provide your analysis in clear sections with bullet points for key findings.
"##;

/// Create the /pdf command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "pdf".to_string(),
            aliases: vec!["read-pdf".to_string(), "analyze-pdf".to_string()],
            description: "Extract and analyze content from PDF files".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("<file.pdf>".to_string()),
            when_to_use: Some(
                "Use when you need to read or analyze PDF documents such as research papers, documentation, or reports".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Processing PDF...".to_string(),
        content_length: 1800,
        arg_names: vec!["file_path".to_string(), "pages".to_string()],
        allowed_tools: vec![
            "Bash(pdftotext:*)".to_string(),
            "Bash(pdfinfo:*)".to_string(),
            "Bash(pdfimages:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec!["*.pdf".to_string()],
    })
}

/// PDF processing options
#[derive(Debug, Clone, Default)]
pub struct PdfOptions {
    /// Specific page numbers to extract (1-indexed)
    pub pages: Option<Vec<usize>>,

    /// Extract images
    pub extract_images: bool,

    /// Use OCR for scanned documents
    pub use_ocr: bool,

    /// OCR language
    pub ocr_language: Option<String>,

    /// Preserve layout
    pub preserve_layout: bool,

    /// Extract tables
    pub extract_tables: bool,
}

impl PdfOptions {
    /// Create new default options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set specific pages
    pub fn with_pages(mut self, pages: Vec<usize>) -> Self {
        self.pages = Some(pages);
        self
    }

    /// Enable image extraction
    pub fn with_images(mut self) -> Self {
        self.extract_images = true;
        self
    }

    /// Enable OCR
    pub fn with_ocr(mut self, language: Option<String>) -> Self {
        self.use_ocr = true;
        self.ocr_language = language;
        self
    }

    /// Enable table extraction
    pub fn with_tables(mut self) -> Self {
        self.extract_tables = true;
        self
    }
}

/// Extracted PDF content
#[derive(Debug, Clone)]
pub struct PdfContent {
    /// File path
    pub source_path: String,

    /// Total pages
    pub total_pages: usize,

    /// Extracted pages
    pub pages: Vec<PdfPage>,

    /// Metadata
    pub metadata: PdfMetadata,
}

/// Single page from PDF
#[derive(Debug, Clone)]
pub struct PdfPage {
    /// Page number (1-indexed)
    pub number: usize,

    /// Text content
    pub text: String,

    /// Extracted images
    pub images: Vec<PdfImage>,

    /// Tables (if extracted)
    pub tables: Vec<PdfTable>,
}

/// Image extracted from PDF
#[derive(Debug, Clone)]
pub struct PdfImage {
    /// Image index
    pub index: usize,

    /// Page number
    pub page: usize,

    /// Path to extracted image
    pub path: String,

    /// Image format
    pub format: ImageFormat,
}

/// Image format
#[derive(Debug, Clone, Copy)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Tiff,
    Pnm,
    Pdf,
}

/// Table extracted from PDF
#[derive(Debug, Clone)]
pub struct PdfTable {
    /// Table index
    pub index: usize,

    /// Page number
    pub page: usize,

    /// Table headers
    pub headers: Vec<String>,

    /// Table rows
    pub rows: Vec<Vec<String>>,
}

/// PDF metadata
#[derive(Debug, Clone)]
pub struct PdfMetadata {
    /// Title
    pub title: Option<String>,

    /// Author
    pub author: Option<String>,

    /// Subject
    pub subject: Option<String>,

    /// Keywords
    pub keywords: Option<String>,

    /// Creator
    pub creator: Option<String>,

    /// Producer
    pub producer: Option<String>,

    /// Creation date
    pub creation_date: Option<String>,

    /// Modification date
    pub modification_date: Option<String>,

    /// Page count
    pub page_count: usize,

    /// Is encrypted
    pub encrypted: bool,
}

/// Get PDF analysis prompt
pub fn get_pdf_prompt(file_path: &str, options: &PdfOptions) -> String {
    let mut prompt = format!(
        "## PDF Analysis Request\n\nFile: {}\n",
        file_path
    );

    if let Some(pages) = &options.pages {
        prompt.push_str(&format!("Pages: {:?}\n", pages));
    } else {
        prompt.push_str("Pages: All\n");
    }

    if options.use_ocr {
        prompt.push_str(&format!(
            "OCR: Enabled (language: {})\n",
            options.ocr_language.as_deref().unwrap_or("auto")
        ));
    }

    if options.extract_images {
        prompt.push_str("Image extraction: Enabled\n");
    }

    if options.extract_tables {
        prompt.push_str("Table extraction: Enabled\n");
    }

    prompt.push_str(&format!("\n{}\n", PDF_PROMPT));
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "pdf");
        assert!(cmd.aliases().contains(&"read-pdf".to_string()));
    }

    #[test]
    fn test_pdf_options_builder() {
        let options = PdfOptions::new()
            .with_pages(vec![1, 2, 3])
            .with_images()
            .with_ocr(Some("eng".to_string()));

        assert_eq!(options.pages, Some(vec![1, 2, 3]));
        assert!(options.extract_images);
        assert!(options.use_ocr);
        assert_eq!(options.ocr_language, Some("eng".to_string()));
    }

    #[test]
    fn test_get_pdf_prompt() {
        let options = PdfOptions::new().with_ocr(Some("eng".to_string()));
        let prompt = get_pdf_prompt("test.pdf", &options);

        assert!(prompt.contains("test.pdf"));
        assert!(prompt.contains("OCR: Enabled"));
    }
}
