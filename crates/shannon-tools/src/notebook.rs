//! Notebook editing tools
//!
//! Provides implementations for:
//! - NotebookEdit: Edit Jupyter notebook (.ipynb) cells
//!
//! This tool allows for:
//! - Replacing cell content
//! - Inserting new cells
//! - Deleting cells
//! - Changing cell types (code/markdown)

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

/// Jupyter notebook cell structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookCell {
    /// Cell type
    #[serde(rename = "cell_type")]
    pub cell_type: CellType,

    /// Unique cell ID (optional in older formats)
    pub id: Option<String>,

    /// Cell source content
    pub source: CellSource,

    /// Cell metadata
    #[serde(default)]
    pub metadata: serde_json::Value,

    /// Execution count (code cells only)
    #[serde(rename = "execution_count", default)]
    pub execution_count: Option<u32>,

    /// Cell outputs (code cells only)
    #[serde(default)]
    pub outputs: Vec<serde_json::Value>,
}

/// Cell type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CellType {
    Code,
    Markdown,
}

/// Cell source - can be a string or array of strings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellSource {
    Single(String),
    Multiple(Vec<String>),
}

impl fmt::Display for CellSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CellSource::Single(s) => write!(f, "{s}"),
            CellSource::Multiple(lines) => write!(f, "{}", lines.join("\n")),
        }
    }
}

impl CellSource {
    /// Create from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        CellSource::Single(s.to_string())
    }
}

/// Jupyter notebook structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookContent {
    /// Notebook format version
    pub nbformat: u32,

    /// Notebook format minor version
    #[serde(rename = "nbformat_minor")]
    pub nbformat_minor: u32,

    /// Notebook metadata
    pub metadata: NotebookMetadata,

    /// Notebook cells
    pub cells: Vec<NotebookCell>,
}

/// Notebook metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookMetadata {
    /// Language information
    #[serde(default)]
    pub language_info: Option<LanguageInfo>,

    /// Additional metadata
    #[serde(flatten)]
    pub other: serde_json::Value,
}

/// Language information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    /// Language name
    pub name: String,

    /// File extension
    #[serde(rename = "file_extension")]
    pub file_extension: Option<String>,

    /// Mimetype
    pub mimetype: Option<String>,
}

/// Edit mode for notebook operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EditMode {
    Replace,
    Insert,
    Delete,
}

/// Input for notebook edit operations
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotebookEditInput {
    /// Absolute path to the notebook file
    pub notebook_path: String,

    /// Cell ID to edit (for replace/delete) or insert after
    pub cell_id: Option<String>,

    /// New source content for the cell
    pub new_source: String,

    /// Cell type (required for insert)
    pub cell_type: Option<CellType>,

    /// Edit mode (replace, insert, delete)
    pub edit_mode: Option<EditMode>,
}

/// Output from notebook edit operations
#[derive(Debug, Serialize)]
pub struct NotebookEditOutput {
    /// The new source code that was written
    pub new_source: String,

    /// ID of the cell that was edited
    pub cell_id: Option<String>,

    /// Type of the cell
    pub cell_type: String,

    /// Programming language of the notebook
    pub language: String,

    /// Edit mode that was used
    pub edit_mode: String,

    /// Error message if operation failed
    pub error: Option<String>,

    /// Path to the notebook file
    pub notebook_path: String,

    /// Original notebook content
    pub original_file: String,

    /// Updated notebook content
    pub updated_file: String,
}

/// Notebook edit tool
pub struct NotebookEditTool {
    description: String,
}

impl Default for NotebookEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl NotebookEditTool {
    pub fn new() -> Self {
        Self {
            description: "Edit Jupyter notebook (.ipynb) cells - replace, insert, or delete cells".to_string(),
        }
    }

    /// Load notebook from file
    fn load_notebook(path: &str) -> Result<NotebookContent, ToolError> {
        let content = fs::read_to_string(path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read notebook: {e}")))?;

        let notebook: NotebookContent = serde_json::from_str(&content)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse notebook JSON: {e}")))?;

        Ok(notebook)
    }

    /// Save notebook to file
    fn save_notebook(path: &str, notebook: &NotebookContent) -> Result<(), ToolError> {
        let json = serde_json::to_string_pretty(notebook)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to serialize notebook: {e}")))?;

        fs::write(path, json)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write notebook: {e}")))?;

        Ok(())
    }

    /// Generate a new cell ID
    fn generate_cell_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{timestamp:x}")
    }

    /// Find cell index by ID or numeric index
    fn find_cell_index(notebook: &NotebookContent, cell_id: &str) -> Result<usize, ToolError> {
        // First try to find by actual ID
        for (i, cell) in notebook.cells.iter().enumerate() {
            if let Some(ref id) = cell.id {
                if id == cell_id {
                    return Ok(i);
                }
            }
        }

        // Try to parse as numeric index (cell-N format)
        if let Some(index_str) = cell_id.strip_prefix("cell-") {
            if let Ok(index) = index_str.parse::<usize>() {
                if index < notebook.cells.len() {
                    return Ok(index);
                }
            }
        }

        // Try direct numeric parse
        if let Ok(index) = cell_id.parse::<usize>() {
            if index < notebook.cells.len() {
                return Ok(index);
            }
        }

        Err(ToolError::ExecutionFailed(format!(
            "Cell with ID '{cell_id}' not found in notebook"
        )))
    }

    /// Execute notebook edit operation
    async fn execute_edit(&self, input: NotebookEditInput) -> Result<NotebookEditOutput, ToolError> {
        let notebook_path = &input.notebook_path;

        // Validate file extension
        if !notebook_path.ends_with(".ipynb") {
            return Ok(NotebookEditOutput {
                new_source: input.new_source,
                cell_id: input.cell_id,
                cell_type: "code".to_string(),
                language: "python".to_string(),
                edit_mode: "replace".to_string(),
                error: Some("File must be a Jupyter notebook (.ipynb file)".to_string()),
                notebook_path: notebook_path.clone(),
                original_file: String::new(),
                updated_file: String::new(),
            });
        }

        // Check file exists
        if !Path::new(notebook_path).exists() {
            return Ok(NotebookEditOutput {
                new_source: input.new_source,
                cell_id: input.cell_id,
                cell_type: "code".to_string(),
                language: "python".to_string(),
                edit_mode: "replace".to_string(),
                error: Some("Notebook file does not exist".to_string()),
                notebook_path: notebook_path.clone(),
                original_file: String::new(),
                updated_file: String::new(),
            });
        }

        // Read original file
        let original_content = fs::read_to_string(notebook_path)
            .unwrap_or_default();

        // Load notebook
        let mut notebook = Self::load_notebook(notebook_path)?;

        // Get edit mode (default to replace)
        let edit_mode = input.edit_mode.unwrap_or(EditMode::Replace);

        // Get language from metadata
        let language = notebook
            .metadata
            .language_info
            .as_ref()
            .map(|info| info.name.clone())
            .unwrap_or_else(|| "python".to_string());

        // Save cell_type for output before we potentially move it
        let output_cell_type = input.cell_type.clone();

        let cell_id = match edit_mode {
            EditMode::Replace => {
                // Find target cell
                let cell_index = if let Some(ref cell_id) = input.cell_id {
                    Self::find_cell_index(&notebook, cell_id)?
                } else {
                    // Default to last cell if no ID provided
                    notebook.cells.len().saturating_sub(1)
                };

                // Check if we should append instead
                if cell_index >= notebook.cells.len() {
                    // Insert new cell at end
                    let cell_type = input.cell_type.clone().unwrap_or(CellType::Code);
                    let new_cell = NotebookCell {
                        cell_type: cell_type.clone(),
                        id: if notebook.nbformat > 4
                            || (notebook.nbformat == 4 && notebook.nbformat_minor >= 5)
                        {
                            Some(Self::generate_cell_id())
                        } else {
                            None
                        },
                        source: CellSource::from_str(&input.new_source),
                        metadata: serde_json::json!({}),
                        execution_count: None,
                        outputs: Vec::new(),
                    };
                    notebook.cells.push(new_cell);
                    None
                } else {
                    let cell = &mut notebook.cells[cell_index];
                    cell.source = CellSource::from_str(&input.new_source);

                    // Update cell type if specified
                    if let Some(ref cell_type) = input.cell_type {
                        cell.cell_type = cell_type.clone();
                    }

                    // Reset execution state for code cells
                    if cell.cell_type == CellType::Code {
                        cell.execution_count = None;
                        cell.outputs.clear();
                    }

                    cell.id.clone()
                }
            }
            EditMode::Insert => {
                let cell_type = input.cell_type
                    .as_ref()
                    .ok_or_else(|| ToolError::ExecutionFailed("Cell type is required when using edit_mode=insert".to_string()))?
                    .clone();

                let insert_index = if let Some(ref cell_id) = input.cell_id {
                    Self::find_cell_index(&notebook, cell_id)? + 1
                } else {
                    0
                };

                let new_cell_id = if notebook.nbformat > 4
                    || (notebook.nbformat == 4 && notebook.nbformat_minor >= 5)
                {
                    Some(Self::generate_cell_id())
                } else {
                    None
                };

                let new_cell = NotebookCell {
                    cell_type,
                    id: new_cell_id.clone(),
                    source: CellSource::from_str(&input.new_source),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: Vec::new(),
                };

                notebook.cells.insert(insert_index, new_cell);
                new_cell_id
            }
            EditMode::Delete => {
                let cell_index = if let Some(ref cell_id) = input.cell_id {
                    Self::find_cell_index(&notebook, cell_id)?
                } else {
                    return Ok(NotebookEditOutput {
                        new_source: String::new(),
                        cell_id: None,
                        cell_type: "code".to_string(),
                        language,
                        edit_mode: "delete".to_string(),
                        error: Some("Cell ID must be specified for delete operation".to_string()),
                        notebook_path: notebook_path.clone(),
                        original_file: original_content,
                        updated_file: String::new(),
                    });
                };

                if cell_index >= notebook.cells.len() {
                    return Ok(NotebookEditOutput {
                        new_source: String::new(),
                        cell_id: input.cell_id,
                        cell_type: "code".to_string(),
                        language,
                        edit_mode: "delete".to_string(),
                        error: Some("Cell index out of bounds".to_string()),
                        notebook_path: notebook_path.clone(),
                        original_file: original_content,
                        updated_file: String::new(),
                    });
                }

                notebook.cells.remove(cell_index);
                input.cell_id
            }
        };

        // Save updated notebook
        Self::save_notebook(notebook_path, &notebook)?;

        // Get updated content
        let updated_content = fs::read_to_string(notebook_path).unwrap_or_default();

        // Determine cell type for output
        let cell_type_str = output_cell_type
            .map(|ct| format!("{ct:?}").to_lowercase())
            .unwrap_or_else(|| "code".to_string());

        Ok(NotebookEditOutput {
            new_source: input.new_source,
            cell_id,
            cell_type: cell_type_str,
            language,
            edit_mode: format!("{edit_mode:?}").to_lowercase(),
            error: None,
            notebook_path: notebook_path.clone(),
            original_file: original_content,
            updated_file: updated_content,
        })
    }
}

#[async_trait]
impl Tool for NotebookEditTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let edit_input: NotebookEditInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid notebook edit input: {e}")))?;
        let output = self.execute_edit(edit_input).await?;

        let notebook_path = output.notebook_path.clone();
        let is_error = output.error.is_some();
        let content = if let Some(err) = &output.error {
            format!("Failed to edit notebook: {err}")
        } else {
            format!("Successfully edited notebook cell in {notebook_path}")
        };

        Ok(ToolOutput {
            content,
            is_error,
            metadata: {
                let mut map = HashMap::new();
                map.insert("notebook_path".to_string(), json!(notebook_path));
                map.insert("cell_id".to_string(), json!(output.cell_id));
                map.insert("cell_type".to_string(), json!(output.cell_type));
                map.insert("edit_mode".to_string(), json!(output.edit_mode));
                if let Some(err) = &output.error {
                    map.insert("error".to_string(), json!(err));
                }
                map
            },
        })
    }

    fn name(&self) -> &str {
        "NotebookEdit"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "Path to .ipynb file"
                },
                "cell_id": {
                    "type": "string",
                    "description": "Cell ID to edit"
                },
                "new_source": {
                    "type": "string",
                    "description": "New cell content"
                },
                "cell_type": {
                    "type": "string",
                    "description": "Cell type (code or markdown)"
                }
            },
            "required": ["notebook_path"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use tempfile::NamedTempFile;

    #[test]
    fn test_cell_source_conversion() {
        let single = CellSource::Single("line1".to_string());
        assert_eq!(single.to_string(), "line1");

        let multiple = CellSource::Multiple(vec!["line1".to_string(), "line2".to_string()]);
        assert_eq!(multiple.to_string(), "line1\nline2");
    }

    #[test]
    fn test_generate_cell_id() {
        let id1 = NotebookEditTool::generate_cell_id();
        let id2 = NotebookEditTool::generate_cell_id();
        assert_ne!(id1, id2);
        // Verify hex format
        assert!(!id1.is_empty());
        assert!(!id2.is_empty());
    }

    #[test]
    fn test_cell_source_from_str() {
        let source = CellSource::from_str("hello world");
        assert_eq!(source.to_string(), "hello world");
    }

    #[test]
    fn test_cell_type_serialization() {
        let code = CellType::Code;
        let markdown = CellType::Markdown;

        // Serialize to lowercase
        let code_json = serde_json::to_string(&code).unwrap();
        assert_eq!(code_json, "\"code\"");

        let markdown_json = serde_json::to_string(&markdown).unwrap();
        assert_eq!(markdown_json, "\"markdown\"");

        // Deserialize from lowercase
        let code_deser: CellType = serde_json::from_str("\"code\"").unwrap();
        assert_eq!(code_deser, CellType::Code);

        let markdown_deser: CellType = serde_json::from_str("\"markdown\"").unwrap();
        assert_eq!(markdown_deser, CellType::Markdown);
    }

    #[test]
    fn test_edit_mode_serialization() {
        let replace = EditMode::Replace;
        let insert = EditMode::Insert;
        let delete = EditMode::Delete;

        let replace_json = serde_json::to_string(&replace).unwrap();
        assert_eq!(replace_json, "\"replace\"");

        let insert_json = serde_json::to_string(&insert).unwrap();
        assert_eq!(insert_json, "\"insert\"");

        let delete_json = serde_json::to_string(&delete).unwrap();
        assert_eq!(delete_json, "\"delete\"");
    }

    #[test]
    fn test_notebook_content_roundtrip() {
        let notebook = NotebookContent {
            nbformat: 4,
            nbformat_minor: 2,
            metadata: NotebookMetadata {
                language_info: Some(LanguageInfo {
                    name: "python".to_string(),
                    file_extension: Some(".py".to_string()),
                    mimetype: Some("text/x-python".to_string()),
                }),
                other: serde_json::json!({}),
            },
            cells: vec![
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("cell-1".to_string()),
                    source: CellSource::Single("print('hello')".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: Some(1),
                    outputs: vec![],
                },
                NotebookCell {
                    cell_type: CellType::Markdown,
                    id: Some("cell-2".to_string()),
                    source: CellSource::Multiple(vec!["# Title".to_string(), "Text".to_string()]),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
            ],
        };

        let json = serde_json::to_string_pretty(&notebook).unwrap();
        let deserialized: NotebookContent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.nbformat, 4);
        assert_eq!(deserialized.nbformat_minor, 2);
        assert_eq!(deserialized.cells.len(), 2);
        assert_eq!(deserialized.cells[0].cell_type, CellType::Code);
        assert_eq!(deserialized.cells[1].cell_type, CellType::Markdown);
        assert_eq!(deserialized.cells[0].source.to_string(), "print('hello')");
        assert_eq!(deserialized.cells[1].source.to_string(), "# Title\nText");
    }

    #[test]
    fn test_notebook_save_load() {
        let _tool = NotebookEditTool::new();

        // Create a temporary file
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        // Create a notebook
        let notebook = NotebookContent {
            nbformat: 4,
            nbformat_minor: 2,
            metadata: NotebookMetadata {
                language_info: Some(LanguageInfo {
                    name: "python".to_string(),
                    file_extension: Some(".py".to_string()),
                    mimetype: Some("text/x-python".to_string()),
                }),
                other: serde_json::json!({}),
            },
            cells: vec![
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("test-cell-1".to_string()),
                    source: CellSource::Single("x = 42".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
            ],
        };

        // Save notebook
        NotebookEditTool::save_notebook(temp_path, &notebook).unwrap();

        // Load notebook
        let loaded = NotebookEditTool::load_notebook(temp_path).unwrap();

        assert_eq!(loaded.nbformat, 4);
        assert_eq!(loaded.cells.len(), 1);
        assert_eq!(loaded.cells[0].source.to_string(), "x = 42");
    }

    #[test]
    fn test_find_cell_by_id() {
        let notebook = NotebookContent {
            nbformat: 4,
            nbformat_minor: 2,
            metadata: NotebookMetadata {
                language_info: None,
                other: serde_json::json!({}),
            },
            cells: vec![
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("abc123".to_string()),
                    source: CellSource::Single("code1".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
                NotebookCell {
                    cell_type: CellType::Markdown,
                    id: Some("def456".to_string()),
                    source: CellSource::Single("markdown1".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
            ],
        };

        // Find by actual ID
        let idx = NotebookEditTool::find_cell_index(&notebook, "abc123").unwrap();
        assert_eq!(idx, 0);

        let idx = NotebookEditTool::find_cell_index(&notebook, "def456").unwrap();
        assert_eq!(idx, 1);

        // Find by numeric index (0-based)
        let idx = NotebookEditTool::find_cell_index(&notebook, "0").unwrap();
        assert_eq!(idx, 0);

        let idx = NotebookEditTool::find_cell_index(&notebook, "1").unwrap();
        assert_eq!(idx, 1);

        // Find by cell-N format
        let idx = NotebookEditTool::find_cell_index(&notebook, "cell-0").unwrap();
        assert_eq!(idx, 0);

        let idx = NotebookEditTool::find_cell_index(&notebook, "cell-1").unwrap();
        assert_eq!(idx, 1);

        // Not found
        let result = NotebookEditTool::find_cell_index(&notebook, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cell_insertion() {
        let mut notebook = NotebookContent {
            nbformat: 4,
            nbformat_minor: 5, // Supports cell IDs
            metadata: NotebookMetadata {
                language_info: None,
                other: serde_json::json!({}),
            },
            cells: vec![
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("cell-1".to_string()),
                    source: CellSource::Single("original".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
            ],
        };

        // Insert at index 0 (beginning)
        let new_cell = NotebookCell {
            cell_type: CellType::Markdown,
            id: Some(NotebookEditTool::generate_cell_id()),
            source: CellSource::Single("# New cell".to_string()),
            metadata: serde_json::json!({}),
            execution_count: None,
            outputs: vec![],
        };
        notebook.cells.insert(0, new_cell);

        assert_eq!(notebook.cells.len(), 2);
        assert_eq!(notebook.cells[0].source.to_string(), "# New cell");
        assert_eq!(notebook.cells[1].source.to_string(), "original");
    }

    #[test]
    fn test_cell_deletion() {
        let mut notebook = NotebookContent {
            nbformat: 4,
            nbformat_minor: 2,
            metadata: NotebookMetadata {
                language_info: None,
                other: serde_json::json!({}),
            },
            cells: vec![
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("cell-1".to_string()),
                    source: CellSource::Single("first".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("cell-2".to_string()),
                    source: CellSource::Single("second".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("cell-3".to_string()),
                    source: CellSource::Single("third".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: None,
                    outputs: vec![],
                },
            ],
        };

        // Delete middle cell (index 1)
        notebook.cells.remove(1);

        assert_eq!(notebook.cells.len(), 2);
        assert_eq!(notebook.cells[0].source.to_string(), "first");
        assert_eq!(notebook.cells[1].source.to_string(), "third");
    }

    #[test]
    fn test_cell_replacement() {
        let mut notebook = NotebookContent {
            nbformat: 4,
            nbformat_minor: 2,
            metadata: NotebookMetadata {
                language_info: None,
                other: serde_json::json!({}),
            },
            cells: vec![
                NotebookCell {
                    cell_type: CellType::Code,
                    id: Some("cell-1".to_string()),
                    source: CellSource::Single("old code".to_string()),
                    metadata: serde_json::json!({}),
                    execution_count: Some(1),
                    outputs: vec![serde_json::json!({"output": "result"})],
                },
            ],
        };

        let cell = &mut notebook.cells[0];
        cell.source = CellSource::Single("new code".to_string());
        cell.cell_type = CellType::Markdown;
        cell.execution_count = None;
        cell.outputs.clear();

        assert_eq!(cell.source.to_string(), "new code");
        assert_eq!(cell.cell_type, CellType::Markdown);
        assert_eq!(cell.execution_count, None);
        assert!(cell.outputs.is_empty());
    }

    #[test]
    fn test_notebook_edit_input_serialization() {
        let input = NotebookEditInput {
            notebook_path: "/path/to/notebook.ipynb".to_string(),
            cell_id: Some("cell-1".to_string()),
            new_source: "print('test')".to_string(),
            cell_type: Some(CellType::Code),
            edit_mode: Some(EditMode::Replace),
        };

        let json = serde_json::to_string(&input).unwrap();
        let deserialized: NotebookEditInput = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.notebook_path, "/path/to/notebook.ipynb");
        assert_eq!(deserialized.cell_id, Some("cell-1".to_string()));
        assert_eq!(deserialized.new_source, "print('test')");
        assert_eq!(deserialized.cell_type, Some(CellType::Code));
        assert_eq!(deserialized.edit_mode, Some(EditMode::Replace));
    }

    #[test]
    fn test_load_invalid_json() {
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        // Write invalid JSON
        std::fs::write(temp_path, "not valid json").unwrap();

        let result = NotebookEditTool::load_notebook(temp_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = NotebookEditTool::load_notebook("/nonexistent/file.ipynb");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiline_source_normalization() {
        // Test that multiline sources are preserved
        let source = CellSource::Multiple(vec![
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
        ]);

        assert_eq!(source.to_string(), "line 1\nline 2\nline 3");

        // Create from multiline string
        let from_str = CellSource::from_str("line 1\nline 2\nline 3");
        assert_eq!(from_str.to_string(), "line 1\nline 2\nline 3");
    }
}
