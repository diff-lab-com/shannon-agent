//! /pdf command - Process PDF documents

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// PDF processing prompt template
///
/// Instructs the AI to use poppler-utils (`pdftotext`, `pdfinfo`, `pdfimages`)
/// to extract content, then produce a structured analysis using the same
/// categories defined in [`PdfContent`], [`PdfPage`], and [`PdfTable`].
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
- Title, Author, Pages, Creation date, whether encrypted

### Content Summary
- Purpose and main topics of the document
- Section structure with headings

### Key Findings
- Important data, figures, tables, or quotes (bullet points)
- If tables found, format them as markdown tables

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
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Tiff,
    Pnm,
    Pdf,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
}
