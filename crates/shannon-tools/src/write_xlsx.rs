//! Engine-native xlsx generation tool (`write_xlsx`) — office Wave 2 (B1).
//!
//! Companion of the `Write` tool for spreadsheet artifacts: the model hands
//! over rows of plain strings, and the engine produces a real `.xlsx` binary
//! via `rust_xlsxwriter` — no host `python3`/`openpyxl` involved. Cell values
//! are smart-typed:
//!
//! - strings starting with `=` are written as spreadsheet **formulas**;
//! - numeric-looking strings are written as **numbers**;
//! - everything else (including the empty string) is written as **text**.
//!
//! Path scoping is identical to `Write`: the target must pass the injected
//! [`crate::file::sandbox::PathSandbox`] (`validate_for_write` semantics — the file may not exist
//! yet, but it must canonicalize inside an allowed root). The bytes are
//! produced in memory and committed through the injected filesystem world
//! with the same atomic temp-file + rename dance as `Write`, so sandboxed
//! and remote assemblies behave exactly like the plain text writer.

use crate::{ToolError, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use shannon_tool_interface::FileSystemProvider;
use std::collections::HashMap;
use std::path::Path;

/// Upper bound on total cells per invocation (all sheets combined). A safety
/// net against pathological inputs wedging the engine on one tool call —
/// 2M cells already produces workbook-sized files, and real tables are
/// orders of magnitude below this.
const MAX_TOTAL_CELLS: usize = 2_000_000;

/// One worksheet: a name plus rows of string cells.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct XlsxSheet {
    /// Sheet name as it should appear in Excel (validated: non-empty, ≤31
    /// chars, no `[]:*?/\`).
    pub name: String,
    /// Row-major cell values. All cells are strings; smart typing decides
    /// the concrete Excel cell type (formula / number / text).
    pub rows: Vec<Vec<String>>,
}

/// Input for the `write_xlsx` tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WriteXlsxInput {
    /// Absolute path of the `.xlsx` file to write (sandbox-scoped like Write).
    pub path: String,
    /// Worksheets to create, in order. At least one is required.
    pub sheets: Vec<XlsxSheet>,
}

/// Smart-type one cell: `=...` → formula, numeric → number, else text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Formula,
    Number,
    Text,
}

fn classify_cell(raw: &str) -> CellKind {
    let trimmed = raw.trim();
    if trimmed.starts_with('=') && trimmed.len() > 1 {
        return CellKind::Formula;
    }
    // Strict numeric typing: the trimmed spelling must itself parse and must
    // be finite (`NaN`/`inf` are not spreadsheet numbers — they stay text).
    if trimmed == raw {
        if let Ok(v) = raw.parse::<f64>() {
            if v.is_finite() {
                return CellKind::Number;
            }
        }
    }
    CellKind::Text
}

/// Validate a sheet name against Excel's hard limits, with tool-shaped
/// `InvalidInput` errors (same style as the file tools).
fn validate_sheet_name(name: &str) -> Result<(), ToolError> {
    if name.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "sheet name must not be empty".to_string(),
        ));
    }
    if name.chars().count() > 31 {
        return Err(ToolError::InvalidInput(format!(
            "sheet name exceeds Excel's 31-character limit: {name}"
        )));
    }
    if name
        .chars()
        .any(|c| matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
    {
        return Err(ToolError::InvalidInput(format!(
            "sheet name must not contain any of []:*?/\\ : {name}"
        )));
    }
    Ok(())
}

/// Render the workbook into an in-memory xlsx package.
fn render_xlsx(input: &WriteXlsxInput) -> Result<Vec<u8>, ToolError> {
    if input.sheets.is_empty() {
        return Err(ToolError::InvalidInput(
            "at least one sheet is required".to_string(),
        ));
    }
    let mut seen_names: Vec<&str> = Vec::with_capacity(input.sheets.len());
    let mut total_cells = 0usize;
    for sheet in &input.sheets {
        validate_sheet_name(&sheet.name)?;
        if seen_names.contains(&sheet.name.as_str()) {
            return Err(ToolError::InvalidInput(format!(
                "duplicate sheet name: {}",
                sheet.name
            )));
        }
        seen_names.push(&sheet.name);
        total_cells += sheet.rows.iter().map(|r| r.len()).sum::<usize>();
    }
    if total_cells > MAX_TOTAL_CELLS {
        return Err(ToolError::InvalidInput(format!(
            "too many cells: {total_cells} (max {MAX_TOTAL_CELLS})"
        )));
    }

    let mut workbook = rust_xlsxwriter::Workbook::new();
    for sheet in &input.sheets {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(&sheet.name).map_err(|e| {
            ToolError::InvalidInput(format!("bad sheet name {:?}: {e}", sheet.name))
        })?;
        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let result = match classify_cell(cell) {
                    CellKind::Formula => worksheet
                        .write_formula(
                            row_idx as u32,
                            col_idx as u16,
                            rust_xlsxwriter::Formula::new(cell),
                        )
                        .map(|_| ()),
                    // `classify_cell` only returns Number for parseable,
                    // finite values — a parse failure here would be an
                    // internal bug, so surface it as an error instead of
                    // guessing.
                    CellKind::Number => {
                        let value = cell.trim().parse::<f64>().map_err(|e| {
                            ToolError::ExecutionFailed(format!(
                                "numeric cell {cell:?} failed to parse: {e}"
                            ))
                        })?;
                        worksheet
                            .write_number(row_idx as u32, col_idx as u16, value)
                            .map(|_| ())
                    }
                    CellKind::Text => worksheet
                        .write_string(row_idx as u32, col_idx as u16, cell)
                        .map(|_| ()),
                };
                result.map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "failed to write sheet {:?} cell ({row_idx},{col_idx}): {e}",
                        sheet.name
                    ))
                })?;
            }
        }
    }
    workbook
        .save_to_buffer()
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to render xlsx: {e}")))
}

pub async fn execute(input: WriteXlsxInput) -> Result<ToolOutput, ToolError> {
    execute_with(input, crate::defaults::fs().as_ref()).await
}

/// Provider-injected entry point (§4.11): the rendered package is committed
/// through the injected filesystem world with the same atomic temp + rename
/// pattern as the `Write` tool.
pub async fn execute_with(
    input: WriteXlsxInput,
    fs: &dyn FileSystemProvider,
) -> Result<ToolOutput, ToolError> {
    let bytes = render_xlsx(&input)?;
    let target = input.path.clone();

    // Create missing parent directories up front — same Write-tool rationale:
    // `validate_for_write` approved the full path, so this cannot escape the
    // sandbox, and it saves a failed save when `output/` does not exist yet.
    if let Some(parent) = Path::new(&target).parent() {
        fs.create_dir_all(parent)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create directory: {e}")))?;
    }

    // Atomic commit: UUID-suffixed temp file next to the target (no symlink
    // race on predictable temp names), then rename into place.
    let temp_path = format!(
        "{}.shannon-tmp-{}",
        target,
        uuid::Uuid::new_v4().as_simple()
    );
    fs.write_bytes(Path::new(&temp_path), &bytes)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;
    fs.rename(Path::new(&temp_path), Path::new(&target))
        .await
        .map_err(|e| {
            let _ = fs.remove_file_blocking(Path::new(&temp_path));
            ToolError::ExecutionFailed(format!("Failed to rename temp file: {e}"))
        })?;

    let cell_count: usize = input
        .sheets
        .iter()
        .map(|s| s.rows.iter().map(|r| r.len()).sum::<usize>())
        .sum();
    Ok(ToolOutput {
        content: format!(
            "Successfully wrote {} ({} sheet{}, {cell_count} cells, {} bytes)",
            input.path,
            input.sheets.len(),
            if input.sheets.len() == 1 { "" } else { "s" },
            bytes.len(),
        ),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("file_path".to_string(), json!(input.path));
            map.insert("bytes".to_string(), json!(bytes.len()));
            map.insert("sheets".to_string(), json!(input.sheets.len()));
            map.insert("cells".to_string(), json!(cell_count));
            map
        },
    })
}

/// `write_xlsx` tool implementation. Scope rules mirror [`crate::file::WriteTool`].
pub struct WriteXlsxTool {
    description: String,
    sandbox: crate::file::sandbox::PathSandbox,
    /// Filesystem world backing the atomic commit (§4.11; `LocalFs` default).
    fs: std::sync::Arc<dyn FileSystemProvider>,
}

impl Default for WriteXlsxTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteXlsxTool {
    const DESCRIPTION: &'static str = "Writes a native Microsoft Excel .xlsx file (real binary \
workbook — no Python or external tooling involved).\n\
\n\
`path` is the target file (same scope rules as Write; missing parent\n\
directories are created). `sheets` lists the worksheets in order; every cell\n\
is a string and is smart-typed: a value starting with `=` becomes a live\n\
spreadsheet formula, a numeric value becomes a real number cell, everything\n\
else is stored as text. Sheet names must be unique, at most 31 characters,\n\
and free of []:*?/\\ characters. Styling, merged cells and charts are not\n\
supported — keep tables plain.";

    pub fn new() -> Self {
        Self {
            description: Self::DESCRIPTION.to_string(),
            sandbox: crate::file::sandbox::PathSandbox::new(),
            fs: crate::defaults::fs(),
        }
    }

    /// Create the tool with a custom sandbox configuration.
    pub fn with_sandbox(sandbox: crate::file::sandbox::PathSandbox) -> Self {
        Self {
            description: Self::DESCRIPTION.to_string(),
            sandbox,
            fs: crate::defaults::fs(),
        }
    }

    /// Inject a filesystem world override (sandbox/remote assemblies).
    pub fn with_fs(mut self, fs: std::sync::Arc<dyn FileSystemProvider>) -> Self {
        self.fs = fs;
        self
    }
}

#[async_trait::async_trait]
impl crate::Tool for WriteXlsxTool {
    fn name(&self) -> &str {
        "write_xlsx"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path of the .xlsx file to write — absolute, or relative to the working directory"
                },
                "sheets": {
                    "type": "array",
                    "description": "Worksheets to create, in order (at least one)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Sheet name (unique, ≤31 chars, no []:*?/\\ characters)"
                            },
                            "rows": {
                                "type": "array",
                                "description": "Row-major cell values. Strings starting with '=' become formulas; numeric strings become numbers; everything else stays text.",
                                "items": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            }
                        },
                        "required": ["name", "rows"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "sheets"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: serde_json::Value) -> crate::ToolResult<ToolOutput> {
        let mut xlsx_input: WriteXlsxInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid write_xlsx input: {e}")))?;

        // Same scope contract as Write: the target may be new, but it must
        // canonicalize inside an allowed root (nearest-existing-ancestor
        // canonicalization covers the not-yet-existing tail).
        let canonical = self
            .sandbox
            .validate_for_write(Path::new(&xlsx_input.path))
            .await
            .map_err(|e| ToolError::InvalidInput(format!("Path sandbox: {e}")))?;
        xlsx_input.path = canonical.to_string_lossy().to_string();

        let mut output = execute_with(xlsx_input, self.fs.as_ref()).await?;
        self.sandbox.remap_tool_output(&mut output);
        Ok(output)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Tool as _;
    use calamine::{DataType, Reader};

    fn sandbox_for(dir: &Path) -> crate::file::sandbox::PathSandbox {
        crate::file::sandbox::PathSandbox::with_config(crate::file::sandbox::SandboxConfig {
            allowed_roots: vec![dir.to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        })
    }

    fn tool_for(dir: &Path) -> WriteXlsxTool {
        WriteXlsxTool::with_sandbox(sandbox_for(dir))
    }

    fn sample_input(path: &Path) -> WriteXlsxInput {
        WriteXlsxInput {
            path: path.to_string_lossy().to_string(),
            sheets: vec![XlsxSheet {
                name: "Data".to_string(),
                rows: vec![
                    vec!["Region".to_string(), "Revenue".to_string()],
                    vec!["North".to_string(), "125000".to_string()],
                ],
            }],
        }
    }

    // ── Cell classification ─────────────────────────────────────────────

    #[test]
    fn classify_cells_by_content() {
        assert_eq!(classify_cell("=SUM(A1:A2)"), CellKind::Formula);
        assert_eq!(classify_cell("=B2*2"), CellKind::Formula);
        assert_eq!(classify_cell("125000"), CellKind::Number);
        assert_eq!(classify_cell("-3.14"), CellKind::Number);
        assert_eq!(classify_cell("1e3"), CellKind::Number);
        assert_eq!(classify_cell("0"), CellKind::Number);
        assert_eq!(classify_cell("North"), CellKind::Text);
        assert_eq!(classify_cell(""), CellKind::Text);
        assert_eq!(classify_cell("1,000"), CellKind::Text);
        assert_eq!(classify_cell("NaN"), CellKind::Text);
        assert_eq!(classify_cell("inf"), CellKind::Text);
        assert_eq!(classify_cell("42 "), CellKind::Text, "padded stays text");
        assert_eq!(
            classify_cell("="),
            CellKind::Text,
            "lone '=' is not a formula"
        );
    }

    // ── Input validation ────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_sheets_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = sample_input(&dir.path().join("out.xlsx"));
        input.sheets.clear();
        let err = execute(input).await.unwrap_err();
        assert!(err.to_string().contains("at least one sheet"), "{err}");
    }

    #[tokio::test]
    async fn duplicate_sheet_names_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let input = WriteXlsxInput {
            path: dir.path().join("out.xlsx").to_string_lossy().to_string(),
            sheets: vec![
                XlsxSheet {
                    name: "Data".into(),
                    rows: vec![vec!["a".into()]],
                },
                XlsxSheet {
                    name: "Data".into(),
                    rows: vec![vec!["b".into()]],
                },
            ],
        };
        let err = execute(input).await.unwrap_err();
        assert!(err.to_string().contains("duplicate sheet name"), "{err}");
    }

    #[tokio::test]
    async fn invalid_sheet_names_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "",
            "a/very/long-name-over-thirty-one-characters-x",
            "bad[name",
            "bad:name",
        ] {
            let input = WriteXlsxInput {
                path: dir.path().join("out.xlsx").to_string_lossy().to_string(),
                sheets: vec![XlsxSheet {
                    name: bad.to_string(),
                    rows: vec![vec!["a".into()]],
                }],
            };
            let err = execute(input).await.unwrap_err();
            assert!(err.to_string().contains("sheet name"), "{bad:?}: {err}");
        }
        // Nothing may have been written by the failed validations.
        assert!(!dir.path().join("out.xlsx").exists());
    }

    // ── Path scope ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn rejects_path_outside_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let tool = tool_for(dir.path());
        let outside = std::env::temp_dir().join("outside_sandbox_test.xlsx");
        let result = tool
            .execute(serde_json::json!({
                "path": outside.to_string_lossy(),
                "sheets": [{ "name": "Data", "rows": [["a"]] }]
            }))
            .await;
        assert!(
            result.is_err(),
            "write outside the sandbox must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("sandbox") || err.contains("allowed"),
            "error should mention the sandbox: {err}"
        );
    }

    #[tokio::test]
    async fn rejects_relative_and_traversal_paths() {
        let dir = tempfile::tempdir().unwrap();
        let tool = tool_for(dir.path());
        for bad in ["relative/out.xlsx", "/etc/shannon-denied.xlsx"] {
            let result = tool
                .execute(serde_json::json!({
                    "path": bad,
                    "sheets": [{ "name": "Data", "rows": [["a"]] }]
                }))
                .await;
            assert!(result.is_err(), "{bad} must be rejected");
        }
    }

    // ── Real file generation ────────────────────────────────────────────

    #[tokio::test]
    async fn writes_readable_workbook_with_typed_cells() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.xlsx");
        let mut input = sample_input(&path);
        input.sheets[0]
            .rows
            .push(vec!["=SUM(B2:B2)".to_string(), "Total".to_string()]);
        let output = execute(input).await.unwrap();
        assert!(!output.is_error);
        assert_eq!(output.metadata["sheets"], 1);
        assert!(path.exists(), "file must exist after a successful write");
        // No temp residue (same invariant as the Write tool tests).
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_str().unwrap().contains("shannon-tmp"),
                "temp file left behind: {name:?}"
            );
        }

        // Type-read the cells back through calamine (the same reader the
        // desktop's injection pipeline uses).
        let mut workbook = calamine::open_workbook::<calamine::Xlsx<_>, _>(&path)
            .unwrap_or_else(|e| panic!("output must open as xlsx: {e}"));
        let range = workbook
            .worksheet_range("Data")
            .unwrap_or_else(|e| panic!("sheet Data must exist: {e}"));
        let text = |r: u32, c: u32| -> String {
            match range.get((r as usize, c as usize)) {
                Some(v) => v
                    .as_string()
                    .unwrap_or_else(|| panic!("expected text at ({r},{c}), got {v:?}")),
                None => panic!("cell ({r},{c}) missing"),
            }
        };
        let num = |r: u32, c: u32| -> f64 {
            match range.get((r as usize, c as usize)) {
                Some(v) => v
                    .as_f64()
                    .unwrap_or_else(|| panic!("expected number at ({r},{c}), got {v:?}")),
                None => panic!("cell ({r},{c}) missing"),
            }
        };
        assert_eq!(text(0, 0), "Region");
        assert_eq!(num(1, 1), 125000.0);
        assert_eq!(text(2, 1), "Total");
    }

    #[tokio::test]
    async fn formula_cells_are_written_as_formulas() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("formulas.xlsx");
        let input = WriteXlsxInput {
            path: path.to_string_lossy().to_string(),
            sheets: vec![XlsxSheet {
                name: "Sheet1".to_string(),
                rows: vec![
                    vec!["1".to_string(), "2".to_string()],
                    vec!["=SUM(A1:B1)".to_string(), "=A1*10".to_string()],
                ],
            }],
        };
        execute(input).await.unwrap();

        // Read the sheet XML out of the package and assert the two cells
        // carry <f> formula parts (a formula cell is never a literal value).
        let file = std::fs::File::open(&path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let sheet = {
            let mut entry = zip.by_name("xl/worksheets/sheet1.xml").unwrap();
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut entry, &mut buf).unwrap();
            buf
        };
        assert!(
            sheet.contains("<f>SUM(A1:B1)</f>") || sheet.contains("<f>A1*10</f>"),
            "expected formula parts in sheet XML: {sheet}"
        );
        assert!(sheet.contains("<f>A1*10</f>"), "got: {sheet}");
    }

    #[tokio::test]
    async fn multi_sheet_workbook_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.xlsx");
        let input = WriteXlsxInput {
            path: path.to_string_lossy().to_string(),
            sheets: vec![
                XlsxSheet {
                    name: "一覧".to_string(),
                    rows: vec![vec!["名前".to_string(), "数".to_string()]],
                },
                XlsxSheet {
                    name: "Notes".to_string(),
                    rows: vec![vec!["hello 🌍".to_string()]],
                },
            ],
        };
        let output = execute(input).await.unwrap();
        assert_eq!(output.metadata["sheets"], 2);

        let mut workbook = calamine::open_workbook::<calamine::Xlsx<_>, _>(&path).unwrap();
        let names = workbook.sheet_names().to_vec();
        assert_eq!(names, vec!["一覧".to_string(), "Notes".to_string()]);
        let range = workbook.worksheet_range("Notes").unwrap();
        assert_eq!(
            range.get((0, 0)).and_then(|v| v.as_string()),
            Some("hello 🌍".to_string())
        );
    }

    #[tokio::test]
    async fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output/2026/report.xlsx");
        execute(sample_input(&path)).await.unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn overwrites_existing_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.xlsx");
        execute(sample_input(&path)).await.unwrap();
        let first = std::fs::read(&path).unwrap();

        let mut input = sample_input(&path);
        input.sheets[0].name = "Replaced".into();
        execute(input).await.unwrap();
        let second = std::fs::read(&path).unwrap();
        assert_ne!(first, second, "the second write must replace the file");
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(!name.to_str().unwrap().contains("shannon-tmp"));
        }
    }

    // ── Tool surface ────────────────────────────────────────────────────

    #[test]
    fn tool_name_and_schema() {
        let tool = WriteXlsxTool::new();
        assert_eq!(tool.name(), "write_xlsx");
        let schema = tool.input_schema();
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["sheets"]["items"]["properties"]["rows"].is_object());
        assert_eq!(schema["required"], serde_json::json!(["path", "sheets"]));
    }

    #[test]
    fn tool_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WriteXlsxTool>();
    }
}
