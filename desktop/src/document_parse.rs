//! Office Wave A2' — minimal text extraction for office-document attachments
//! (`docx` / `pptx` / `xlsx` / `ods` / `csv`) plus the send_message injection
//! block builder.
//!
//! Design (reviewed & frozen for A2'):
//! - Formats are parsed in-process, not shelled out: OOXML (`docx`/`pptx`) is
//!   a zip container of XML parts read with `zip` + `roxmltree`;
//!   `xlsx`/`ods` go through `calamine` (which reads cached formula *values*);
//!   `csv` through the `csv` crate.
//! - Extracted text is sectioned (docx: heading/paragraph runs, pptx: slides,
//!   xlsx: worksheets, csv: row blocks) so the model can page through it.
//! - The full sectioned text is written to
//!   `~/.shannon/cache/extracted/<sha256(path+mtime)>.txt`; the inline
//!   injection block carries the leading sections up to a 16 KiB budget plus
//!   the cache path, so the model can `Read`/`Grep` the rest with its
//!   existing tools (no new tool is introduced).
//! - Every parser path is guard-railed (entry count, decompressed size and
//!   ratio, XML part size, CSV rows/bytes) and returns `Result` — a hostile
//!   document must produce a clear error string, never a panic.
//!
//! Nothing here talks to Tauri; everything is a pure function over paths and
//! bytes so it is unit-testable without the feature gate.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use zip::read::ZipArchive;

// ── Limits (the safety contract of this module) ─────────────────────────────

/// Attachments with these extensions get office text injection in
/// `send_message`. Legacy binary formats (`doc`/`xls`/`ppt`) and `odt`/`rtf`
/// are deliberately out of A2' scope.
pub(crate) const OFFICE_EXTENSIONS: [&str; 5] = ["docx", "pptx", "xlsx", "ods", "csv"];

/// Inline injection budget per office attachment. Deliberately smaller than
/// the PDF 50 KiB (`PDF_TEXT_INJECT_LIMIT`): office extraction is unbounded
/// in principle (a 200k-row CSV would dwarf it), and the cache file + Read
/// tool is the designated escape hatch.
pub(crate) const OFFICE_INLINE_INJECT_LIMIT: usize = 16 * 1024;

/// Maximum number of entries in a zip container. Real OOXML documents have
/// dozens; thousands mean a crafted archive.
const MAX_ZIP_ENTRIES: usize = 2000;

/// Maximum total decompressed size across all zip entries.
const MAX_ZIP_TOTAL_DECOMPRESSED: u64 = 256 * 1024 * 1024;

/// Maximum single-entry decompression ratio (uncompressed : compressed).
/// Legitimate XML parts compress ~10:1; anything beyond 500:1 is a bomb.
const MAX_ZIP_RATIO: u64 = 500;

/// Maximum compressed size of a zip container accepted for preflight
/// (nothing is even read into memory beyond this).
const MAX_ZIP_INPUT_BYTES: usize = 256 * 1024 * 1024;

/// Maximum bytes of a single XML part handed to `roxmltree`.
const MAX_XML_PART_BYTES: usize = 64 * 1024 * 1024;

/// Maximum CSV attachment size accepted for parsing.
const MAX_CSV_BYTES: usize = 50 * 1024 * 1024;

/// Maximum data rows parsed from one CSV.
const MAX_CSV_ROWS: usize = 200_000;

/// Rows per CSV section (the "row block" sectioning unit).
const MAX_ROWS_PER_CSV_SECTION: usize = 500;

/// Rows rendered per worksheet before the section is honestly truncated.
const MAX_ROWS_PER_SHEET: usize = 20_000;

/// Non-heading docx paragraphs per section (chunking unit for heading-less
/// documents).
const MAX_PARAGRAPHS_PER_SECTION: usize = 200;

// ── Small pure helpers ──────────────────────────────────────────────────────

/// Lowercased extension of `path`, if any.
pub(crate) fn extension_lowercase(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// Whether `path` is an office document this module can extract
/// (extension-based, case-insensitive).
pub(crate) fn is_office_document(path: &Path) -> bool {
    extension_lowercase(path).is_some_and(|ext| OFFICE_EXTENSIONS.contains(&ext.as_str()))
}

/// Guard helper: ratio check for one zip entry. Factored out so tests can
/// exercise it without building a real bomb.
fn check_entry_ratio(uncompressed: u64, compressed: u64) -> Result<(), String> {
    if uncompressed == 0 || compressed == 0 {
        return Ok(());
    }
    if uncompressed > compressed.saturating_mul(MAX_ZIP_RATIO) {
        return Err(format!(
            "zip entry decompresses to {uncompressed} bytes from {compressed} compressed \
             (ratio over {MAX_ZIP_RATIO}:1) — refusing a possible zip bomb"
        ));
    }
    Ok(())
}

/// Guard helper: running total of decompressed bytes. Factored out so tests
/// can exercise it without building a 256 MB archive.
fn check_total_decompressed(total: u64) -> Result<(), String> {
    if total > MAX_ZIP_TOTAL_DECOMPRESSED {
        return Err(format!(
            "zip decompresses to over {} MB (limit {}) — refusing a possible zip bomb",
            MAX_ZIP_TOTAL_DECOMPRESSED / (1024 * 1024),
            MAX_ZIP_TOTAL_DECOMPRESSED / (1024 * 1024)
        ));
    }
    Ok(())
}

/// Guard helper: XML part size cap. Factored out so tests can exercise it
/// without building a 64 MB part.
fn check_xml_part_size(part_len: usize) -> Result<(), String> {
    if part_len > MAX_XML_PART_BYTES {
        return Err(format!(
            "XML part is {part_len} bytes (limit {MAX_XML_PART_BYTES}) — refusing to parse"
        ));
    }
    Ok(())
}

/// Guard helper: compressed container size cap (factored out for tests).
fn check_container_size(len: u64) -> Result<(), String> {
    if len > MAX_ZIP_INPUT_BYTES as u64 {
        return Err(format!(
            "document is {len} bytes (limit {MAX_ZIP_INPUT_BYTES}) — refusing to parse"
        ));
    }
    Ok(())
}

/// Truncate a label to `max` chars with an ASCII ellipsis.
fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push_str("...");
        out
    }
}

/// Byte-limit cut that respects UTF-8 char boundaries (mirrors the PDF
/// injection's boundary walk in `commands.rs`).
fn cut_at_char_boundary(s: &str, mut end: usize) -> &str {
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ── Extracted document model ────────────────────────────────────────────────

/// A parsed office document: an ordered list of `(label, body)` sections.
/// Markers (`[Section i/N] label`) are composed at render time so the total
/// count is always consistent with the list length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedDocument {
    pub sections: Vec<(String, String)>,
}

impl ExtractedDocument {
    pub(crate) fn from_sections(sections: Vec<(String, String)>) -> Self {
        Self { sections }
    }

    /// Full sectioned text, one `[Section i/N] label` marker per section,
    /// body on the following lines. This is what lands in the cache file.
    pub(crate) fn full_text(&self) -> String {
        let total = self.sections.len();
        let mut out = String::new();
        for (i, (label, body)) in self.sections.iter().enumerate() {
            out.push_str(&format!("[Section {}/{}] {}\n", i + 1, total, label));
            out.push_str(body.trim_end());
            out.push('\n');
        }
        out
    }
}

/// Render the `[Section i/N] label` marker (test-visible format lock).
fn section_marker(index: usize, total: usize, label: &str) -> String {
    format!("[Section {index}/{total}] {label}")
}

// ── Dispatch ────────────────────────────────────────────────────────────────

/// Extract text from an office document at `path`. Total-function discipline:
/// every failure is a `String` reason, never a panic. The size guards run on
/// `fs::metadata` BEFORE any read (mirrors the `MAX_PDF_BYTES` gate in
/// `send_message`) so a giant file never gets read into memory just to be
/// rejected.
pub(crate) fn extract_document(path: &Path) -> Result<ExtractedDocument, String> {
    let ext = extension_lowercase(path)
        .ok_or_else(|| "file has no extension to identify its office format".to_string())?;
    let is_csv = ext == "csv";
    let file_len = std::fs::metadata(path)
        .map_err(|e| format!("failed to stat document for parsing: {e}"))?
        .len();
    if is_csv {
        check_csv_size(file_len)?;
    } else {
        check_container_size(file_len)?;
    }
    match ext.as_str() {
        "csv" => extract_csv(path),
        "docx" => extract_ooxml(path, OoxmlKind::Docx),
        "pptx" => extract_ooxml(path, OoxmlKind::Pptx),
        "xlsx" | "ods" => extract_spreadsheet(path),
        other => Err(format!("unsupported office extension '{other}'")),
    }
}

// ── Zip preflight guards ────────────────────────────────────────────────────

/// Open `bytes` as a zip container and enforce the archive-level guards:
/// entry count, per-entry compression ratio, and total decompressed size
/// (declared sizes — the `zip` crate validates actual-vs-declared on read,
/// so a lying central directory fails the later reads too).
fn preflight_zip(bytes: &[u8]) -> Result<(), String> {
    check_container_size(bytes.len() as u64)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("not a readable zip container: {e}"))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(format!(
            "zip has {} entries (limit {MAX_ZIP_ENTRIES}) — refusing a possible zip bomb",
            archive.len()
        ));
    }
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        // Metadata-only access — no decompression happens here.
        let file = archive
            .by_index_raw(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let (uncompressed, compressed) = (file.size(), file.compressed_size());
        check_entry_ratio(uncompressed, compressed)?;
        total = total.saturating_add(uncompressed);
        check_total_decompressed(total)?;
    }
    Ok(())
}

/// Read one entry by name, enforcing the XML part cap on both the declared
/// and the actually-read size.
fn read_zip_entry_capped(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|e| format!("zip entry '{name}': {e}"))?;
    check_xml_part_size(entry.size() as usize)?;
    let mut buf = Vec::with_capacity((entry.size() as usize).min(MAX_XML_PART_BYTES));
    entry
        .read_to_end(&mut buf)
        .map_err(|e| format!("zip entry '{name}' read failed: {e}"))?;
    check_xml_part_size(buf.len())?;
    Ok(buf)
}

// ── OOXML (docx / pptx) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OoxmlKind {
    Docx,
    Pptx,
}

fn extract_ooxml(path: &Path, kind: OoxmlKind) -> Result<ExtractedDocument, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("failed to read document for parsing: {e}"))?;
    preflight_zip(&bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(&bytes[..]))
        .map_err(|e| format!("not a readable zip container: {e}"))?;
    match kind {
        OoxmlKind::Docx => {
            let xml = read_zip_entry_capped(&mut archive, "word/document.xml")?;
            sections_from_document_xml(&xml)
        }
        OoxmlKind::Pptx => {
            // Slide parts: ppt/slides/slide<N>.xml — collected, sorted
            // numerically (slide2 before slide10), one section per slide.
            let mut slides: Vec<(u32, String)> = archive
                .file_names()
                .filter_map(|name| {
                    let rest = name.strip_prefix("ppt/slides/slide")?;
                    let num = rest.strip_suffix(".xml")?;
                    if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) {
                        num.parse::<u32>().ok().map(|n| (n, name.to_string()))
                    } else {
                        None
                    }
                })
                .collect();
            slides.sort_by_key(|(num, _)| *num);
            if slides.is_empty() {
                return Err("no ppt/slides/slideN.xml parts found in the presentation".into());
            }
            let mut sections = Vec::with_capacity(slides.len());
            for (num, name) in slides {
                let xml = read_zip_entry_capped(&mut archive, &name)?;
                let lines = text_lines_from_xml(&xml, |n| n == "p", |n| n == "t")
                    .map_err(|e| format!("failed to parse {name}: {e}"))?;
                let label = match lines.first() {
                    Some(first) => format!("Slide {num}: {}", shorten(first, 80)),
                    None => format!("Slide {num} (no text)"),
                };
                sections.push((label, lines.join("\n")));
            }
            Ok(ExtractedDocument::from_sections(sections))
        }
    }
}

/// Parse an XML part and collect the text of every element matching
/// `paragraph_tag` (one output line each) by concatenating the text of all
/// `text_tag` descendants. Namespace-agnostic by local name — OOXML parts use
/// a single primary namespace (`w:` in docx, `a:`/`p:` in pptx).
fn text_lines_from_xml(
    xml: &[u8],
    paragraph_tag: impl Fn(&str) -> bool,
    text_tag: impl Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    let text = String::from_utf8_lossy(xml);
    let doc =
        roxmltree::Document::parse(text.as_ref()).map_err(|e| format!("XML parse error: {e}"))?;
    let mut lines = Vec::new();
    for p in doc
        .root()
        .descendants()
        .filter(|n| paragraph_tag(n.tag_name().name()))
    {
        let mut line = String::new();
        for t in p.descendants().filter(|n| {
            let name = n.tag_name().name();
            text_tag(name) || name == "tab"
        }) {
            if t.tag_name().name() == "tab" {
                line.push(' ');
            } else if let Some(s) = t.text() {
                line.push_str(s);
            }
        }
        if !line.trim().is_empty() {
            lines.push(line.trim_end().to_string());
        }
    }
    Ok(lines)
}

/// Build docx sections from `word/document.xml`: a new section starts at
/// every heading-styled paragraph (`w:pStyle` val `Heading*`/`Title`);
/// consecutive body paragraphs join the current section; heading-less
/// documents are chunked every [`MAX_PARAGRAPHS_PER_SECTION`] paragraphs.
fn sections_from_document_xml(xml: &[u8]) -> Result<ExtractedDocument, String> {
    let lines_with_styles = paragraph_lines_with_styles(xml)?;
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for (line, is_heading) in lines_with_styles {
        if is_heading {
            // The heading text lives in the marker label; the body carries
            // only the paragraphs under it.
            sections.push((format!("Heading: {}", shorten(&line, 80)), vec![]));
        } else {
            match sections.last_mut() {
                Some((_, lines)) if lines.len() < MAX_PARAGRAPHS_PER_SECTION => lines.push(line),
                _ => {
                    let label = format!("Paragraphs: {}", shorten(&line, 60));
                    sections.push((label, vec![line]));
                }
            }
        }
    }
    if sections.is_empty() {
        return Err("word/document.xml has no extractable paragraphs".into());
    }
    Ok(ExtractedDocument::from_sections(
        sections
            .into_iter()
            .map(|(label, lines)| (label, lines.join("\n")))
            .collect(),
    ))
}

/// One `(text, is_heading)` pair per non-empty docx paragraph.
fn paragraph_lines_with_styles(xml: &[u8]) -> Result<Vec<(String, bool)>, String> {
    let text = String::from_utf8_lossy(xml);
    let doc = roxmltree::Document::parse(text.as_ref())
        .map_err(|e| format!("failed to parse word/document.xml: {e}"))?;
    let mut out = Vec::new();
    for p in doc
        .root()
        .descendants()
        .filter(|n| n.tag_name().name() == "p")
    {
        let mut line = String::new();
        for t in p.descendants().filter(|n| {
            let name = n.tag_name().name();
            name == "t" || name == "tab"
        }) {
            if t.tag_name().name() == "tab" {
                line.push(' ');
            } else if let Some(s) = t.text() {
                line.push_str(s);
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        let is_heading = p
            .descendants()
            .find(|n| n.tag_name().name() == "pStyle")
            .and_then(|style| style.attributes().find(|a| a.name() == "val"))
            .map(|a| {
                let v = a.value();
                v.eq_ignore_ascii_case("Title") || v.to_ascii_lowercase().starts_with("heading")
            })
            .unwrap_or(false);
        out.push((line.trim_end().to_string(), is_heading));
    }
    Ok(out)
}

// ── Spreadsheets (xlsx / ods) via calamine ──────────────────────────────────

fn extract_spreadsheet(path: &Path) -> Result<ExtractedDocument, String> {
    // Preflight the container guards ourselves — calamine's own reader does
    // not enforce archive-level limits.
    let bytes =
        std::fs::read(path).map_err(|e| format!("failed to read spreadsheet for parsing: {e}"))?;
    preflight_zip(&bytes)?;

    let mut workbook = calamine::open_workbook_auto(path)
        .map_err(|e| format!("failed to open spreadsheet: {e}"))?;
    use calamine::Reader as _;
    let sheet_names: Vec<String> = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err("spreadsheet has no worksheets".into());
    }
    let mut sections = Vec::with_capacity(sheet_names.len());
    for name in &sheet_names {
        let range = workbook
            .worksheet_range(name.as_str())
            .map_err(|e| format!("failed to read sheet '{name}': {e}"))?;
        let mut lines: Vec<String> = Vec::new();
        let mut truncated = false;
        for row in range.rows() {
            if lines.len() >= MAX_ROWS_PER_SHEET {
                truncated = true;
                break;
            }
            lines.push(sheet_row_line(row));
        }
        if lines.is_empty() {
            continue;
        }
        let mut body = lines.join("\n");
        if truncated {
            body.push_str(&format!(
                "\n[sheet '{name}' truncated at {MAX_ROWS_PER_SHEET} rows — the rest was not read]"
            ));
        }
        sections.push((format!("Sheet: {name}"), body));
    }
    if sections.is_empty() {
        return Err("spreadsheet has no non-empty worksheets".into());
    }
    Ok(ExtractedDocument::from_sections(sections))
}

/// Render one worksheet row: cells joined with `" | "`, trailing empty cells
/// trimmed, fully-empty rows skipped (empty string return).
fn sheet_row_line(row: &[calamine::Data]) -> String {
    let mut cells: Vec<String> = row
        .iter()
        .map(|cell| match cell {
            calamine::Data::Empty => String::new(),
            other => other.to_string(),
        })
        .collect();
    while cells.last().is_some_and(|c| c.is_empty()) {
        cells.pop();
    }
    cells.join(" | ")
}

// ── CSV ─────────────────────────────────────────────────────────────────────

fn extract_csv(path: &Path) -> Result<ExtractedDocument, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("failed to read CSV: {e}"))?;
    check_csv_size(bytes.len() as u64)?;
    extract_csv_bytes(&bytes)
}

/// Guard helper: CSV byte-size cap (factored out for tests).
fn check_csv_size(len: u64) -> Result<(), String> {
    if len > MAX_CSV_BYTES as u64 {
        return Err(format!(
            "CSV is {len} bytes (limit {MAX_CSV_BYTES}) — refusing to parse"
        ));
    }
    Ok(())
}

fn extract_csv_bytes(bytes: &[u8]) -> Result<ExtractedDocument, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    let mut record = csv::ByteRecord::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut block: Vec<String> = Vec::with_capacity(MAX_ROWS_PER_CSV_SECTION);
    let mut block_start_row: usize = 1;
    let mut total_rows: usize = 0;
    loop {
        match reader.read_byte_record(&mut record) {
            Ok(true) => {
                total_rows += 1;
                check_csv_rows(total_rows)?;
                block.push(csv_record_line(&record));
                if block.len() == MAX_ROWS_PER_CSV_SECTION {
                    let label = format!(
                        "Rows {}-{}",
                        block_start_row,
                        block_start_row + block.len() - 1
                    );
                    sections.push((label, block.join("\n")));
                    block.clear();
                    block_start_row = total_rows + 1;
                }
            }
            Ok(false) => break,
            Err(e) => return Err(format!("CSV parse failed at row {}: {e}", total_rows + 1)),
        }
    }
    if !block.is_empty() {
        let label = format!(
            "Rows {}-{}",
            block_start_row,
            block_start_row + block.len() - 1
        );
        sections.push((label, block.join("\n")));
    }
    if sections.is_empty() {
        return Err("CSV has no data rows".into());
    }
    Ok(ExtractedDocument::from_sections(sections))
}

/// Guard helper: CSV row cap (factored out for tests).
fn check_csv_rows(rows: usize) -> Result<(), String> {
    if rows > MAX_CSV_ROWS {
        return Err(format!(
            "CSV has more than {MAX_CSV_ROWS} data rows (limit {MAX_CSV_ROWS}) — refusing to parse"
        ));
    }
    Ok(())
}

/// Render one CSV record: fields lossily UTF-8'd (hostile bytes become
/// replacement chars, never an error), minimally re-quoted so the line stays
/// machine-readable. A UTF-8 BOM on the first field is stripped.
fn csv_record_line(record: &csv::ByteRecord) -> String {
    record
        .iter()
        .map(|field| {
            let text = String::from_utf8_lossy(field);
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            if text.contains(',')
                || text.contains('"')
                || text.contains('\n')
                || text.contains('\r')
            {
                format!("\"{}\"", text.replace('"', "\"\""))
            } else {
                text.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

// ── Cache (`~/.shannon/cache/extracted/`) ───────────────────────────────────

/// Cache directory honoring `SHANNON_HOME` (same convention as the feedback
/// store in `commands_feedback.rs`). `None` when no home can be resolved —
/// caching then degrades to "unavailable" in the injection block.
fn extracted_cache_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("SHANNON_HOME") {
        return Some(PathBuf::from(home).join("cache").join("extracted"));
    }
    Some(
        dirs::home_dir()?
            .join(".shannon")
            .join("cache")
            .join("extracted"),
    )
}

/// Cache key: sha256 of "<canonical path>:<mtime nanos>", hex-encoded — a
/// rewrite of the source file changes the name, so stale cache entries are
/// never read again.
fn cache_key(source_path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let meta = std::fs::metadata(source_path)?;
    let mtime_nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(source_path.to_string_lossy().as_bytes());
    hasher.update(b":");
    hasher.update(mtime_nanos.to_string().as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Write the full extracted text to `dir` under the hash key of
/// `source_path`. Separate from [`cache_extracted_text`] so tests can inject
/// a temp base dir instead of the real `~/.shannon`.
fn write_extracted_cache(
    dir: &Path,
    source_path: &Path,
    full_text: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(format!("{}.txt", cache_key(source_path)?));
    std::fs::write(&target, full_text)?;
    Ok(target)
}

/// Best-effort cache write: `None` on any failure (no home dir, unwritable
/// dir, ...) — the injection block then says the cache is unavailable rather
/// than failing the whole send.
fn cache_extracted_text(source_path: &Path, full_text: &str) -> Option<PathBuf> {
    let dir = extracted_cache_dir()?;
    write_extracted_cache(&dir, source_path, full_text).ok()
}

// ── Injection block builder ─────────────────────────────────────────────────

/// Build the text block injected into the model context for one office
/// attachment. Shape (spec example wording preserved):
///
/// ```text
/// Attached Office document "report.docx" (12345 bytes). Extracted text: 23 section(s).
/// Showing sections 1-8 of 23. Full extracted text: /home/u/.shannon/cache/extracted/<hash>.txt — use Read/Grep on it for the rest.
/// [Section 1/23] Heading: Executive Summary
/// ...
/// ```
///
/// Sections are included whole, oldest first, up to `inline_limit` bytes; the
/// first section is never skipped (if it alone exceeds the budget it is cut
/// at the byte limit with an explicit note). `cache_path: None` (cache write
/// failed) degrades honestly instead of pretending a path exists.
pub(crate) fn build_office_injection_block(
    file_name: &str,
    size: u64,
    doc: &ExtractedDocument,
    cache_path: Option<&str>,
    inline_limit: usize,
) -> String {
    let total = doc.sections.len();
    if total == 0 {
        return format!(
            "Attached Office document \"{file_name}\" ({size} bytes). No extractable text found."
        );
    }
    let cut_notice = "\n[Section 1 cut off at the inline byte limit — read the full extracted text file above for the rest.]\n";
    let mut body = String::new();
    let mut shown = 0usize;
    let mut first_section_cut = false;
    for (i, (label, section_body)) in doc.sections.iter().enumerate() {
        let candidate = format!(
            "{}\n{}\n",
            section_marker(i + 1, total, label),
            section_body.trim_end()
        );
        if body.len() + candidate.len() > inline_limit {
            if i == 0 {
                // The first section alone busts the budget: include a cut
                // preview so the model still sees something real.
                let budget = inline_limit.saturating_sub(cut_notice.len());
                body.push_str(cut_at_char_boundary(&candidate, budget));
                body.push_str(cut_notice);
                shown = 1;
                first_section_cut = true;
            }
            break;
        }
        body.push_str(&candidate);
        shown += 1;
    }
    let range_line = if first_section_cut {
        format!(
            "Showing sections 1-1 of {total} (first section cut off at the {inline_limit}-byte inline limit). "
        )
    } else if shown < total {
        format!("Showing sections 1-{shown} of {total}. ")
    } else {
        format!("Showing all {total} sections. ")
    };
    let cache_line = match cache_path {
        Some(path) => format!("Full extracted text: {path} — use Read/Grep on it for the rest."),
        None => "Full extracted text: unavailable (cache write failed).".to_string(),
    };
    format!(
        "Attached Office document \"{file_name}\" ({size} bytes). Extracted text: {total} section(s).\n\
         {range_line}{cache_line}\n{body}"
    )
    .trim_end()
    .to_string()
}

/// Placeholder block injected when extraction fails (unreadable file,
/// guard trip, malformed container). Failure is surfaced into the context —
/// the model can tell the user exactly why, instead of the file vanishing.
pub(crate) fn office_extraction_error_block(file_name: &str, size: u64, reason: &str) -> String {
    format!(
        "Attached Office document \"{file_name}\" ({size} bytes). Text extraction failed: {reason}"
    )
}

/// Full per-attachment pipeline used by `send_message` (runs inside
/// `spawn_blocking` there): extract → cache → build the injection block, or
/// produce the failure placeholder. Never panics.
pub(crate) fn office_block_for_file(path: &Path, file_name: &str, size: u64) -> String {
    match extract_document(path) {
        Ok(doc) => {
            let cache = cache_extracted_text(path, &doc.full_text())
                .map(|p| p.to_string_lossy().into_owned());
            build_office_injection_block(
                file_name,
                size,
                &doc,
                cache.as_deref(),
                OFFICE_INLINE_INJECT_LIMIT,
            )
        }
        Err(reason) => office_extraction_error_block(file_name, size, &reason),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;

    /// Build an in-memory zip with `deflate`-compressed entries (the same
    /// compression real OOXML containers use).
    fn zip_bytes(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            for (name, data) in entries {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .expect("start_file");
                writer.write_all(data).expect("write entry");
            }
            writer.finish().expect("finish zip");
        }
        buffer.into_inner()
    }

    fn temp_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write temp file");
        path
    }

    /// Set a file's mtime (File::set_times — `fs::set_modified` is not in
    /// the 1.88 toolchain).
    fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
        use std::fs::FileTimes;
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open for mtime");
        file.set_times(FileTimes::new().set_modified(mtime))
            .expect("set mtime");
    }

    // ── Extension detection ─────────────────────────────────────────────

    #[test]
    fn office_detection_matches_supported_extensions_only() {
        for name in ["a.docx", "b.PPTX", "c.xlsx", "d.ods", "e.csv"] {
            assert!(is_office_document(Path::new(name)), "{name} should match");
        }
        for name in [
            "a.doc", "b.xls", "c.ppt", "d.odt", "e.rtf", "f.pdf", "g.png", "noext",
        ] {
            assert!(
                !is_office_document(Path::new(name)),
                "{name} must not match"
            );
        }
    }

    // ── docx ────────────────────────────────────────────────────────────

    fn docx_document_xml(body: &str) -> Vec<u8> {
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             <w:body>{body}</w:body></w:document>"
        );
        zip_bytes(&[("word/document.xml", xml.into_bytes())])
    }

    fn w_p(paragraph_inner: &str) -> String {
        format!("<w:p>{paragraph_inner}</w:p>")
    }

    fn w_run_text(text: &str) -> String {
        format!("<w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r>")
    }

    fn w_heading(style: &str, text: &str) -> String {
        w_p(&format!(
            "<w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>{}",
            w_run_text(text)
        ))
    }

    #[test]
    fn docx_headings_start_sections_and_paragraphs_join_them() {
        let body = [
            w_heading("Heading1", "Executive Summary"),
            w_p(&w_run_text("First paragraph.")),
            w_p(&w_run_text("Second paragraph.")),
            w_heading("Heading2", "Details"),
            w_p(&w_run_text("Detail paragraph.")),
        ]
        .join("");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "doc.docx", &docx_document_xml(&body));
        let doc = extract_document(&path).expect("parse docx");
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].0, "Heading: Executive Summary");
        // Body carries only the paragraphs under the heading; the heading
        // text itself lives in the marker label.
        assert_eq!(doc.sections[0].1, "First paragraph.\nSecond paragraph.");
        assert_eq!(doc.sections[1].0, "Heading: Details");
        assert_eq!(doc.sections[1].1, "Detail paragraph.");
        let full = doc.full_text();
        assert!(full.contains("[Section 1/2] Heading: Executive Summary\n"));
        assert!(full.contains("[Section 2/2] Heading: Details\n"));
    }

    #[test]
    fn docx_title_style_and_case_insensitive_heading_detection() {
        let body = [w_heading("Title", "Cover"), w_p(&w_run_text("Body line"))].join("");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "doc.docx", &docx_document_xml(&body));
        let doc = extract_document(&path).expect("parse docx");
        assert_eq!(doc.sections[0].0, "Heading: Cover");
    }

    #[test]
    fn docx_without_headings_chunks_paragraphs_into_sections() {
        let mut body = String::new();
        for i in 0..450 {
            body.push_str(&w_p(&w_run_text(&format!("Paragraph number {i}"))));
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "doc.docx", &docx_document_xml(&body));
        let doc = extract_document(&path).expect("parse docx");
        // 450 paragraphs at 200 per section -> 3 sections.
        assert_eq!(doc.sections.len(), 3);
        assert!(
            doc.sections[0]
                .0
                .starts_with("Paragraphs: Paragraph number 0")
        );
        assert_eq!(doc.sections[0].1.lines().count(), 200);
        assert_eq!(doc.sections[2].1.lines().count(), 50);
    }

    #[test]
    fn docx_empty_body_is_a_clear_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "doc.docx", &docx_document_xml(""));
        let err = extract_document(&path).expect_err("must error");
        assert!(err.contains("no extractable paragraphs"), "{err}");
    }

    #[test]
    fn docx_missing_document_xml_is_a_clear_error() {
        let bytes = zip_bytes(&[("other.xml", b"<x/>".to_vec())]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "broken.docx", &bytes);
        let err = extract_document(&path).expect_err("must error");
        assert!(err.contains("word/document.xml"), "{err}");
    }

    #[test]
    fn docx_malformed_xml_is_a_clear_error_not_a_panic() {
        let bytes = zip_bytes(&[("word/document.xml", b"<w:document><w:body>".to_vec())]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "bad.docx", &bytes);
        let err = extract_document(&path).expect_err("must error");
        assert!(err.contains("parse"), "{err}");
    }

    // ── pptx ────────────────────────────────────────────────────────────

    fn slide_xml(paragraphs: &[&str]) -> Vec<u8> {
        let paras: String = paragraphs
            .iter()
            .map(|p| format!("<a:p><a:r><a:t>{p}</a:t></a:r></a:p>"))
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
             xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
             <p:cSld><p:spTree>{paras}</p:spTree></p:cSld></p:sld>"
        )
        .into_bytes()
    }

    #[test]
    fn pptx_slides_are_extracted_in_numeric_order() {
        let bytes = zip_bytes(&[
            ("ppt/slides/slide10.xml", slide_xml(&["Slide ten"])),
            (
                "ppt/slides/slide2.xml",
                slide_xml(&["Second slide title", "Bullet"]),
            ),
            (
                "ppt/slides/_rels/slide2.xml.rels",
                b"<Relationships/>".to_vec(),
            ),
        ]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "deck.pptx", &bytes);
        let doc = extract_document(&path).expect("parse pptx");
        assert_eq!(doc.sections.len(), 2, "rels entries must be ignored");
        assert_eq!(doc.sections[0].0, "Slide 2: Second slide title");
        assert_eq!(doc.sections[0].1, "Second slide title\nBullet");
        assert_eq!(doc.sections[1].0, "Slide 10: Slide ten");
        let full = doc.full_text();
        assert!(full.contains("[Section 1/2] Slide 2: Second slide title\n"));
        assert!(full.contains("[Section 2/2] Slide 10: Slide ten\n"));
    }

    #[test]
    fn pptx_without_slides_is_a_clear_error() {
        let bytes = zip_bytes(&[("docProps/app.xml", b"<x/>".to_vec())]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "empty.pptx", &bytes);
        let err = extract_document(&path).expect_err("must error");
        assert!(err.contains("no ppt/slides"), "{err}");
    }

    // ── xlsx / ods via calamine ─────────────────────────────────────────

    /// Minimal real xlsx container calamine accepts: workbook + rels + two
    /// sheets (inline strings, plus a cached formula value `<v>42</v>` to
    /// lock the "formula results are read" requirement).
    fn xlsx_bytes() -> Vec<u8> {
        let workbook = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
             xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
             <sheets><sheet name=\"Alpha\" sheetId=\"1\" r:id=\"rId1\"/>\
             <sheet name=\"Beta\" sheetId=\"2\" r:id=\"rId2\"/></sheets></workbook>"
            .as_bytes();
        let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
             <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet2.xml\"/>\
             </Relationships>"
            .as_bytes();
        let sheet1 = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
             <sheetData>\
             <row r=\"1\"><c t=\"inlineStr\"><is><t>City</t></is></c><c t=\"inlineStr\"><is><t>Population</t></is></c></row>\
             <row r=\"2\"><c t=\"inlineStr\"><is><t>Shanghai</t></is></c><c><v>26000000</v></c></row>\
             </sheetData></worksheet>"
            .as_bytes();
        let sheet2 = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
             <sheetData>\
             <row r=\"1\"><c t=\"inlineStr\"><is><t>Total</t></is></c></row>\
             <row r=\"2\"><c><f>SUM(A1)</f><v>42</v></c></row>\
             </sheetData></worksheet>"
            .as_bytes();
        zip_bytes(&[
            ("xl/workbook.xml", workbook.to_vec()),
            ("xl/_rels/workbook.xml.rels", rels.to_vec()),
            ("xl/worksheets/sheet1.xml", sheet1.to_vec()),
            ("xl/worksheets/sheet2.xml", sheet2.to_vec()),
        ])
    }

    #[test]
    fn xlsx_sheets_become_sections_with_formula_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "book.xlsx", &xlsx_bytes());
        let doc = extract_document(&path).expect("parse xlsx");
        assert_eq!(doc.sections.len(), 2);
        assert_eq!(doc.sections[0].0, "Sheet: Alpha");
        assert_eq!(doc.sections[0].1, "City | Population\nShanghai | 26000000");
        assert_eq!(doc.sections[1].0, "Sheet: Beta");
        // The formula cell renders its cached value, not the formula.
        assert_eq!(doc.sections[1].1, "Total\n42");
        let full = doc.full_text();
        assert!(full.contains("[Section 1/2] Sheet: Alpha\n"));
        assert!(full.contains("[Section 2/2] Sheet: Beta\n"));
    }

    #[test]
    fn ods_open_workbook_auto_path_works() {
        // Minimal ODS container: mimetype + content.xml with one table.
        let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <office:document-content \
             xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
             xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
             xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
             office:version=\"1.2\"><office:body><office:spreadsheet>\
             <table:table table:name=\"Notes\">\
             <table:table-row><table:table-cell office:value-type=\"string\">\
             <text:p>Hello ODS</text:p></table:table-cell></table:table-row>\
             </table:table></office:spreadsheet></office:body></office:document-content>"
            .as_bytes();
        let manifest = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" \
             manifest:version=\"1.2\">\
             <manifest:file-entry manifest:full-path=\"/\" \
             manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
             <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
             </manifest:manifest>"
            .as_bytes();
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            // ODS spec: the mimetype entry comes first and is stored
            // uncompressed. calamine tolerates deviations, real tools don't.
            let stored =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("mimetype", stored)
                .expect("mimetype entry");
            writer
                .write_all(b"application/vnd.oasis.opendocument.spreadsheet")
                .expect("mimetype");
            // calamine's Ods reader requires the package manifest.
            writer
                .start_file("META-INF/manifest.xml", SimpleFileOptions::default())
                .expect("manifest entry");
            writer.write_all(manifest).expect("manifest");
            writer
                .start_file("content.xml", SimpleFileOptions::default())
                .expect("content entry");
            writer.write_all(content).expect("content");
            writer.finish().expect("finish");
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "notes.ods", &buffer.into_inner());
        let doc = extract_document(&path).expect("parse ods");
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].0, "Sheet: Notes");
        assert_eq!(doc.sections[0].1, "Hello ODS");
    }

    // ── csv ─────────────────────────────────────────────────────────────

    #[test]
    fn csv_rows_chunk_into_block_sections() {
        let mut csv = String::from("id,name\n");
        for i in 0..1200 {
            csv.push_str(&format!("{i},name{i}\n"));
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "data.csv", csv.as_bytes());
        let doc = extract_document(&path).expect("parse csv");
        // 1 header + 1200 data rows = 1201 rows -> 500/500/201.
        assert_eq!(doc.sections.len(), 3);
        assert_eq!(doc.sections[0].0, "Rows 1-500");
        assert!(doc.sections[0].1.starts_with("id,name\n"));
        assert_eq!(doc.sections[1].0, "Rows 501-1000");
        assert_eq!(doc.sections[2].0, "Rows 1001-1201");
        let full = doc.full_text();
        assert!(full.contains("[Section 1/3] Rows 1-500\n"));
    }

    #[test]
    fn csv_fields_with_commas_and_quotes_are_requoted() {
        let csv = b"name,note\n\"Smith, John\",\"said \"\"hi\"\"\"\n";
        let doc = extract_csv_bytes(csv).expect("parse csv");
        assert_eq!(doc.sections.len(), 1);
        let data_row = doc.sections[0].1.lines().nth(1).expect("data row");
        assert_eq!(data_row, "\"Smith, John\",\"said \"\"hi\"\"\"");
    }

    #[test]
    fn csv_utf8_bom_is_stripped_from_first_field() {
        let mut csv = vec![0xef, 0xbb, 0xbf];
        csv.extend_from_slice(b"col\nval\n");
        let doc = extract_csv_bytes(&csv).expect("parse csv");
        assert!(
            doc.sections[0].1.starts_with("col\n"),
            "{}",
            doc.sections[0].1
        );
    }

    #[test]
    fn csv_row_guard_rejects_over_limit() {
        check_csv_rows(MAX_CSV_ROWS).expect("at limit ok");
        let err = check_csv_rows(MAX_CSV_ROWS + 1).expect_err("over limit");
        assert!(err.contains(&MAX_CSV_ROWS.to_string()), "{err}");
    }

    #[test]
    fn csv_size_guard_rejects_over_limit() {
        check_csv_size(10).expect("small ok");
        let err = check_csv_size(MAX_CSV_BYTES as u64 + 1).expect_err("over limit");
        assert!(err.contains("bytes (limit"), "{err}");
    }

    #[test]
    fn csv_lossy_on_invalid_utf8() {
        let csv = b"a,b\n\xff\xfe,val\n";
        let doc = extract_csv_bytes(csv).expect("parse csv");
        assert!(doc.sections[0].1.contains('\u{fffd}'));
    }

    // ── zip guards ──────────────────────────────────────────────────────

    #[test]
    fn ratio_guard_rejects_zip_bomb_entry() {
        // 300 KB of one repeated byte deflates to a few hundred bytes —
        // a genuine >500:1 ratio without lying about metadata.
        let bytes = zip_bytes(&[("word/document.xml", vec![b'0'; 300_000])]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "bomb.docx", &bytes);
        let err = extract_document(&path).expect_err("guard must trip");
        assert!(err.contains("zip bomb"), "{err}");
    }

    #[test]
    fn entry_count_guard_rejects_over_limit() {
        let entries: Vec<(String, Vec<u8>)> = (0..=MAX_ZIP_ENTRIES)
            .map(|i| (format!("f/{i}.xml"), b"<x/>".to_vec()))
            .collect();
        let refs: Vec<(&str, Vec<u8>)> = entries
            .iter()
            .map(|(n, d)| (n.as_str(), d.clone()))
            .collect();
        let bytes = zip_bytes(&refs);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "many.docx", &bytes);
        let err = extract_document(&path).expect_err("guard must trip");
        assert!(err.contains("entries (limit"), "{err}");
    }

    #[test]
    fn ratio_and_total_guards_direct() {
        check_entry_ratio(1000, 100).expect("10:1 ok");
        check_entry_ratio(0, 100).expect("empty entry skipped");
        let err = check_entry_ratio(1000, 1).expect_err("1000:1 rejected");
        assert!(err.contains("zip bomb"), "{err}");
        check_total_decompressed(MAX_ZIP_TOTAL_DECOMPRESSED).expect("at limit ok");
        let err = check_total_decompressed(MAX_ZIP_TOTAL_DECOMPRESSED + 1).expect_err("over limit");
        assert!(err.contains("zip bomb"), "{err}");
    }

    #[test]
    fn xml_part_size_guard_direct() {
        check_xml_part_size(10).expect("small ok");
        let err = check_xml_part_size(MAX_XML_PART_BYTES + 1).expect_err("over limit");
        assert!(err.contains("XML part"), "{err}");
    }

    #[test]
    fn container_size_guard_direct() {
        check_container_size(10).expect("small ok");
        let err = check_container_size(MAX_ZIP_INPUT_BYTES as u64 + 1).expect_err("over limit");
        assert!(err.contains("bytes (limit"), "{err}");
    }

    #[test]
    fn non_zip_file_is_a_clear_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "fake.docx", b"this is not a zip");
        let err = extract_document(&path).expect_err("must error");
        assert!(err.contains("zip"), "{err}");
    }

    // ── cache ───────────────────────────────────────────────────────────

    #[test]
    fn cache_write_uses_sha256_hash_name_and_round_trips() {
        let source_dir = tempfile::tempdir().expect("tempdir");
        let source = temp_file(source_dir.path(), "report.docx", b"payload");
        let cache_dir = tempfile::tempdir().expect("tempdir");

        let target = write_extracted_cache(cache_dir.path(), &source, "[Section 1/1] X\nbody\n")
            .expect("cache write");
        assert_eq!(target.extension().and_then(|e| e.to_str()), Some("txt"));
        let stem = target
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("stem")
            .to_string();
        assert_eq!(stem.len(), 64, "sha256 hex: {stem}");
        assert!(
            stem.bytes().all(|b| b.is_ascii_hexdigit()),
            "hex only: {stem}"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("read cache"),
            "[Section 1/1] X\nbody\n"
        );

        // Deterministic: same source (path+mtime) -> same file name.
        let again = write_extracted_cache(cache_dir.path(), &source, "[Section 1/1] X\nbody\n")
            .expect("cache write again");
        assert_eq!(target, again);
    }

    #[test]
    fn cache_key_changes_when_mtime_changes() {
        let source_dir = tempfile::tempdir().expect("tempdir");
        let source = temp_file(source_dir.path(), "doc.docx", b"v1");
        let key1 = cache_key(&source).expect("key1");
        let new_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        set_mtime(&source, new_mtime);
        let key2 = cache_key(&source).expect("key2");
        assert_ne!(key1, key2, "rewrite must rotate the cache name");
    }

    // ── injection block ─────────────────────────────────────────────────

    fn doc_with_n_sections(n: usize, body_per_section: usize) -> ExtractedDocument {
        let sections = (0..n)
            .map(|i| {
                (
                    format!("Heading: Section {i}"),
                    "word ".repeat(body_per_section).trim_end().to_string(),
                )
            })
            .collect();
        ExtractedDocument::from_sections(sections)
    }

    #[test]
    fn injection_block_truncates_with_ranges_and_cache_hint() {
        // ~40 sections x ~1 KiB = far over the 16 KiB budget.
        let doc = doc_with_n_sections(40, 200);
        let block = build_office_injection_block(
            "report.docx",
            90_000,
            &doc,
            Some("/home/u/.shannon/cache/extracted/abc123.txt"),
            OFFICE_INLINE_INJECT_LIMIT,
        );
        assert!(
            block.starts_with(
                "Attached Office document \"report.docx\" (90000 bytes). Extracted text: 40 section(s)."
            ),
            "{block}"
        );
        let shown: usize = {
            let line = block.lines().nth(1).expect("range line");
            let tail = line
                .strip_prefix("Showing sections 1-")
                .expect("partial-range wording")
                .to_string();
            tail.split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .expect("parsed shown count")
        };
        assert!(shown > 1 && shown < 40, "partial window, got {shown}");
        assert!(
            block.lines().nth(1).expect("line").contains(
                "Full extracted text: /home/u/.shannon/cache/extracted/abc123.txt — use Read/Grep on it for the rest."
            ),
            "{block}"
        );
        assert!(block.contains(&section_marker(1, 40, "Heading: Section 0")));
        assert!(block.contains(&section_marker(
            shown,
            40,
            &format!("Heading: Section {}", shown - 1)
        )));
        assert!(
            !block.contains(&section_marker(
                shown + 1,
                40,
                &format!("Heading: Section {shown}")
            )),
            "section {shown} must not be inlined"
        );
        assert!(
            block.len() <= OFFICE_INLINE_INJECT_LIMIT + 512,
            "block is {} bytes",
            block.len()
        );
    }

    #[test]
    fn injection_block_all_sections_fit() {
        let doc = doc_with_n_sections(2, 5);
        let block = build_office_injection_block("a.docx", 10, &doc, Some("/tmp/x.txt"), 16_384);
        assert!(block.contains("Showing all 2 sections. "), "{block}");
        assert!(block.contains("[Section 2/2] Heading: Section 1"));
        assert!(
            block.contains("Full extracted text: /tmp/x.txt — use Read/Grep on it for the rest.")
        );
    }

    #[test]
    fn injection_block_first_section_over_limit_is_cut_with_note() {
        let sections = vec![("Heading: Huge".to_string(), "x".repeat(100_000))];
        let doc = ExtractedDocument::from_sections(sections);
        let block = build_office_injection_block("big.docx", 200_000, &doc, None, 16_384);
        assert!(
            block.contains("first section cut off at the 16384-byte inline limit"),
            "{block}"
        );
        assert!(block.contains("Full extracted text: unavailable (cache write failed)."));
        let body_start =
            block.find("[Section 1/1]").expect("marker") + "[Section 1/1] Heading: Huge\n".len();
        let included = block.len() - body_start;
        assert!(included <= 16_384 + 256, "included {included} bytes");
    }

    #[test]
    fn injection_block_empty_document() {
        let doc = ExtractedDocument::from_sections(vec![]);
        let block = build_office_injection_block("e.docx", 5, &doc, None, 1024);
        assert_eq!(
            block,
            "Attached Office document \"e.docx\" (5 bytes). No extractable text found."
        );
    }

    #[test]
    fn injection_block_markers_are_byte_budgeted_not_char_broken() {
        // Multi-byte content: the cut path must not split a UTF-8 char.
        let sections = vec![("Heading: 中文".to_string(), "中文内容".repeat(10_000))];
        let doc = ExtractedDocument::from_sections(sections);
        let block = build_office_injection_block("cjk.docx", 1, &doc, None, 1024);
        // Would panic on slice if boundaries were ignored.
        assert!(block.contains("[Section 1/1]"));
    }

    // ── end-to-end per-file pipeline ────────────────────────────────────

    #[test]
    fn office_block_for_file_end_to_end_with_real_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes = docx_document_xml(
            &(w_heading("Heading1", "Plan") + &w_p(&w_run_text("Do the thing."))),
        );
        let path = temp_file(dir.path(), "plan.docx", &bytes);
        // Redirect the cache at a temp SHANNON_HOME so the test never writes
        // the real ~/.shannon (same save/restore pattern as commands_feedback
        // tests). SAFETY: unique tempdir per test run; restored below.
        let cache_home = tempfile::tempdir().expect("cache tempdir");
        let prev_home = std::env::var("SHANNON_HOME").ok();
        unsafe { std::env::set_var("SHANNON_HOME", cache_home.path()) };
        let block = office_block_for_file(&path, "plan.docx", bytes.len() as u64);
        match prev_home {
            Some(prev) => unsafe { std::env::set_var("SHANNON_HOME", prev) },
            None => unsafe { std::env::remove_var("SHANNON_HOME") },
        }
        assert!(
            block.starts_with("Attached Office document \"plan.docx\" ("),
            "{block}"
        );
        assert!(block.contains("Showing all 1 sections. "), "{block}");
        assert!(block.contains("[Section 1/1] Heading: Plan\n"));
        assert!(block.contains("Do the thing."));
        // The cache file named in the block really exists with the full text.
        let cache_line = block
            .lines()
            .find(|l| l.contains("Full extracted text: "))
            .expect("cache line");
        let cache_path = cache_line
            .trim_start_matches("Showing all 1 sections. Full extracted text: ")
            .trim_end_matches(" — use Read/Grep on it for the rest.");
        let cached = std::fs::read_to_string(cache_path).expect("cache file readable");
        assert_eq!(cached, "[Section 1/1] Heading: Plan\nDo the thing.\n");
    }

    #[test]
    fn office_block_for_file_failure_is_a_placeholder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = temp_file(dir.path(), "junk.docx", b"not a zip at all");
        let block = office_block_for_file(&path, "junk.docx", 16);
        assert_eq!(
            block,
            format!(
                "Attached Office document \"junk.docx\" (16 bytes). Text extraction failed: {}",
                extract_document(&path).expect_err("same reason")
            )
        );
    }

    // ── section marker format lock ──────────────────────────────────────

    #[test]
    fn section_marker_format_lock() {
        assert_eq!(
            section_marker(1, 12, "Heading: Intro"),
            "[Section 1/12] Heading: Intro"
        );
    }

    #[test]
    fn shorten_appends_ascii_ellipsis() {
        assert_eq!(shorten("short", 10), "short");
        let long = "a".repeat(100);
        assert_eq!(shorten(&long, 10), format!("{}...", "a".repeat(10)));
    }
}
