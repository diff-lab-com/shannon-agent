//! /pdf command - Process PDF documents
//!
//! Provides the `/pdf` slash command which instructs the AI to extract and
//! analyse content from a PDF file using `poppler-utils`. The types defined
//! here (`PdfContent`, `PdfPage`, `PdfTable`, `PdfImage`, `PdfMetadata`,
//! `ImageFormat`, `PdfOptions`) describe the expected output structure so
//! that callers can parse the AI response back into structured data.

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// PDF processing prompt template
///
/// Instructs the AI to use poppler-utils (`pdftotext`, `pdfinfo`, `pdfimages`)
/// to extract content, then produce a structured analysis. The output sections
/// correspond to the fields of [`PdfMetadata`], [`PdfPage`], and [`PdfTable`].
const PDF_PROMPT: &str = r##"
Process and analyze a PDF document.

Arguments: {args}
- The first argument should be the path to a PDF file (required)
- Optional flags: --pages <range> (e.g., 1-3,5), --ocr, --images, --tables

## Steps

1. Run `pdfinfo {args}` to get metadata (title, author, pages, encrypted status).
2. Run `pdftotext -layout {args} -` to extract full text with layout preserved.
   - If --pages is specified, use `pdftotext -f <first> -l <last> -layout <file> -`
3. If --images is specified, run `pdfimages -list <file>` to list embedded images.
4. If --tables is specified, look for structured table patterns in the extracted text.
5. If --ocr is specified and the text is sparse/empty, note that OCR (e.g., tesseract) would be needed.

## Output Format

### Document Metadata
- **Title**: document title
- **Author**: document author
- **Subject**: document subject (if available)
- **Keywords**: keywords (if available)
- **Creator**: creating application
- **Producer**: PDF producer
- **Creation date**: when created
- **Modification date**: when last modified
- **Pages**: total page count
- **Encrypted**: yes/no

### Content Summary
- Purpose and main topics of the document
- Section structure with headings

### Key Findings
- Important data, figures, tables, or quotes (bullet points)
- If tables found, format them as markdown tables with headers and rows

### Quality Notes
- OCR errors, missing content, formatting issues
- Whether the extraction appears complete

If the file does not exist or is not a PDF, report the error clearly.
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
        prompt_template: Some(PDF_PROMPT.to_string()),
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

impl PdfContent {
    /// Parse AI-generated markdown output into structured PDF content.
    ///
    /// The AI prompt produces sections like "### Document Metadata" with
    /// key-value pairs and bullet lists. This method extracts the known
    /// fields into the typed structures.
    pub fn from_ai_output(source_path: &str, ai_output: &str) -> Self {
        let mut content = Self {
            source_path: source_path.to_string(),
            ..Default::default()
        };

        let mut in_metadata = false;
        let mut in_findings = false;

        for line in ai_output.lines() {
            let trimmed = line.trim();

            // Track which section we're in
            if trimmed.starts_with("### Document Metadata") {
                in_metadata = true;
                in_findings = false;
                continue;
            } else if trimmed.starts_with("### Key Findings") || trimmed.starts_with("### Content Summary") {
                in_metadata = false;
                in_findings = true;
                continue;
            } else if trimmed.starts_with("###") {
                in_metadata = false;
                in_findings = false;
                continue;
            }

            if in_metadata {
                // Parse both "**Key**: value" and "- **Key**: value" patterns
                let candidate = if let Some(stripped) = trimmed.strip_prefix("- ") {
                    stripped.trim()
                } else if let Some(stripped) = trimmed.strip_prefix("* ") {
                    stripped.trim()
                } else {
                    trimmed
                };

                if let Some(rest) = candidate.strip_prefix("**") {
                    if let Some(end) = rest.find("**:") {
                        let key = rest[..end].trim();
                        let value = rest[end + 3..].trim();
                        match key {
                            "Title" => content.metadata.title = Some(value.to_string()),
                            "Author" => content.metadata.author = Some(value.to_string()),
                            "Subject" => content.metadata.subject = Some(value.to_string()),
                            "Keywords" => content.metadata.keywords = Some(value.to_string()),
                            "Creator" => content.metadata.creator = Some(value.to_string()),
                            "Producer" => content.metadata.producer = Some(value.to_string()),
                            "Creation date" => content.metadata.creation_date = Some(value.to_string()),
                            "Modification date" => content.metadata.modification_date = Some(value.to_string()),
                            "Pages" => {
                                if let Ok(n) = value.parse::<usize>() {
                                    content.metadata.page_count = n;
                                    content.total_pages = n;
                                }
                            }
                            "Encrypted" => {
                                content.metadata.encrypted = value.to_lowercase().starts_with('y');
                            }
                            _ => {}
                        }
                    }
                }
            }

            if in_findings {
                // Capture bullet points as page content (simplified)
                if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                    let text = trimmed[2..].to_string();
                    if !text.is_empty() {
                        let page_num = content.pages.len() + 1;
                        content.pages.push(PdfPage::new(page_num, text));
                    }
                }
            }
        }

        content
    }
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
            output.push_str(&format!(" {header:width$} |"));
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
                output.push_str(&format!(" {cell:width$} |"));
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

/// Get PDF analysis prompt with file path and options context
pub fn get_pdf_prompt(file_path: &str, options: &PdfOptions) -> String {
    let mut args = file_path.to_string();

    if let Some(pages) = &options.pages {
        args.push_str(&format!(" --pages {pages:?}"));
    }
    if options.use_ocr {
        args.push_str(" --ocr");
    }
    if options.extract_images {
        args.push_str(" --images");
    }
    if options.extract_tables {
        args.push_str(" --tables");
    }

    PDF_PROMPT.replace("{args}", &args)
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
        assert!(prompt.contains("--ocr"));
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

    #[test]
    fn test_pdf_content_from_ai_output() {
        let ai_output = r#"
### Document Metadata
- **Title**: Research Paper on Rust
- **Author**: Jane Doe
- **Subject**: Programming Languages
- **Keywords**: rust, systems, safety
- **Creator**: LaTeX
- **Producer**: pdfTeX
- **Creation date**: 2024-01-15
- **Modification date**: 2024-02-20
- **Pages**: 42
- **Encrypted**: no

### Content Summary
This paper explores ownership semantics in Rust.

### Key Findings
- Rust provides memory safety without garbage collection
- The borrow checker enforces ownership rules at compile time
- Zero-cost abstractions enable high-level patterns with low-level performance

### Quality Notes
- Text extraction appears complete
"#;
        let content = PdfContent::from_ai_output("paper.pdf", ai_output);

        assert_eq!(content.source_path, "paper.pdf");
        assert_eq!(content.total_pages, 42);
        assert_eq!(content.metadata.title.as_deref(), Some("Research Paper on Rust"));
        assert_eq!(content.metadata.author.as_deref(), Some("Jane Doe"));
        assert_eq!(content.metadata.subject.as_deref(), Some("Programming Languages"));
        assert_eq!(content.metadata.keywords.as_deref(), Some("rust, systems, safety"));
        assert_eq!(content.metadata.creator.as_deref(), Some("LaTeX"));
        assert_eq!(content.metadata.producer.as_deref(), Some("pdfTeX"));
        assert!(!content.metadata.encrypted);
        // Bullet points from Key Findings should become pages
        assert!(!content.pages.is_empty());
        assert!(content.pages[0].text.contains("memory safety"));
    }

    #[test]
    fn test_pdf_content_from_ai_output_encrypted() {
        let ai_output = r#"
### Document Metadata
- **Pages**: 10
- **Encrypted**: yes

### Key Findings
"#;
        let content = PdfContent::from_ai_output("secure.pdf", ai_output);
        assert_eq!(content.total_pages, 10);
        assert!(content.metadata.encrypted);
    }

    #[test]
    fn test_pdf_content_from_ai_output_empty() {
        let content = PdfContent::from_ai_output("empty.pdf", "No PDF content found.");
        assert_eq!(content.source_path, "empty.pdf");
        assert_eq!(content.total_pages, 0);
        assert!(content.pages.is_empty());
    }
}
