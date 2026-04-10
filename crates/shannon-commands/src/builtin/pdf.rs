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
#[derive(Debug, Clone, Default)]
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

impl PdfPage {
    /// Create a new page with just text content
    pub fn new(number: usize, text: String) -> Self {
        Self {
            number,
            text,
            images: Vec::new(),
            tables: Vec::new(),
        }
    }

    /// Get word count of the page text
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    /// Check if the page has any content
    pub fn has_content(&self) -> bool {
        !self.text.trim().is_empty() || !self.images.is_empty() || !self.tables.is_empty()
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Tiff,
    Pnm,
    Pdf,
}

impl ImageFormat {
    /// Get the file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Png => "png",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Pnm => "pnm",
            ImageFormat::Pdf => "pdf",
        }
    }

    /// Get the MIME type
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Png => "image/png",
            ImageFormat::Tiff => "image/tiff",
            ImageFormat::Pnm => "image/x-portable-anymap",
            ImageFormat::Pdf => "application/pdf",
        }
    }

    /// Parse from extension string
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
            "png" => Some(ImageFormat::Png),
            "tiff" | "tif" => Some(ImageFormat::Tiff),
            "pnm" => Some(ImageFormat::Pnm),
            "pdf" => Some(ImageFormat::Pdf),
            _ => None,
        }
    }
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

impl PdfTable {
    /// Create a new table
    pub fn new(index: usize, page: usize, headers: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { index, page, headers, rows }
    }

    /// Get the number of data rows (excluding header)
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Get column count from headers
    pub fn column_count(&self) -> usize {
        self.headers.len()
    }

    /// Format as a simple text table
    pub fn to_text(&self) -> String {
        let mut output = String::new();
        let col_widths: Vec<usize> = self.headers.iter().enumerate().map(|(i, h)| {
            let data_width = self.rows.iter()
                .filter_map(|r| r.get(i).map(|c| c.len()))
                .max()
                .unwrap_or(0);
            h.len().max(data_width).max(4)
        }).collect();

        // Header
        for (i, header) in self.headers.iter().enumerate() {
            let width = col_widths.get(i).copied().unwrap_or(4);
            output.push_str(&format!(" {:width$} |", header, width = width));
        }
        output.push('\n');

        // Separator
        for width in &col_widths {
            output.push_str(&format!(" {} |", "-".repeat(*width)));
        }
        output.push('\n');

        // Rows
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                let width = col_widths.get(i).copied().unwrap_or(4);
                output.push_str(&format!(" {:width$} |", cell, width = width));
            }
            output.push('\n');
        }

        output
    }
}

/// PDF metadata
#[derive(Debug, Clone, Default)]
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

    #[test]
    fn test_image_format_extension() {
        assert_eq!(ImageFormat::Jpeg.extension(), "jpg");
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert_eq!(ImageFormat::Tiff.extension(), "tiff");
        assert_eq!(ImageFormat::Pnm.extension(), "pnm");
        assert_eq!(ImageFormat::Pdf.extension(), "pdf");
    }

    #[test]
    fn test_image_format_mime_type() {
        assert_eq!(ImageFormat::Jpeg.mime_type(), "image/jpeg");
        assert_eq!(ImageFormat::Png.mime_type(), "image/png");
        assert_eq!(ImageFormat::Pdf.mime_type(), "application/pdf");
    }

    #[test]
    fn test_image_format_from_extension() {
        assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("jpeg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("PNG"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("tiff"), Some(ImageFormat::Tiff));
        assert_eq!(ImageFormat::from_extension("xyz"), None);
    }

    #[test]
    fn test_pdf_metadata_default() {
        let meta = PdfMetadata::default();
        assert!(meta.title.is_none());
        assert!(meta.author.is_none());
        assert_eq!(meta.page_count, 0);
        assert!(!meta.encrypted);
    }

    #[test]
    fn test_pdf_page_new() {
        let page = PdfPage::new(1, "Hello world this is page one".to_string());
        assert_eq!(page.number, 1);
        assert_eq!(page.word_count(), 6);
        assert!(page.has_content());
        assert!(page.images.is_empty());
        assert!(page.tables.is_empty());
    }

    #[test]
    fn test_pdf_page_empty() {
        let page = PdfPage::new(1, String::new());
        assert!(!page.has_content());
        assert_eq!(page.word_count(), 0);
    }

    #[test]
    fn test_pdf_table_new() {
        let table = PdfTable::new(
            0,
            1,
            vec!["Name".to_string(), "Age".to_string()],
            vec![
                vec!["Alice".to_string(), "30".to_string()],
                vec!["Bob".to_string(), "25".to_string()],
            ],
        );
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.column_count(), 2);
    }

    #[test]
    fn test_pdf_table_to_text() {
        let table = PdfTable::new(
            0,
            1,
            vec!["Name".to_string(), "Age".to_string()],
            vec![vec!["Alice".to_string(), "30".to_string()]],
        );
        let text = table.to_text();
        assert!(text.contains("Name"));
        assert!(text.contains("Alice"));
        assert!(text.contains("30"));
    }

    #[test]
    fn test_pdf_content_default() {
        let content = PdfContent::default();
        assert!(content.source_path.is_empty());
        assert_eq!(content.total_pages, 0);
        assert!(content.pages.is_empty());
    }
}
