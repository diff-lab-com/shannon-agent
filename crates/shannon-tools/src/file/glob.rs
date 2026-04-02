//! Glob tool implementation

use super::super::ToolError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobInput {
    /// Glob pattern to match files
    pub pattern: String,

    /// Optional directory to search in
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GlobOutput {
    /// Matching file paths
    pub files: Vec<String>,

    /// Number of matches found
    pub count: usize,

    /// Pattern that was searched
    pub pattern: String,
}

// Stub glob iterator to match the expected API
struct GlobIterator {
    results: Vec<Result<PathBuf, glob::GlobError>>,
    index: usize,
}

impl Iterator for GlobIterator {
    type Item = Result<PathBuf, glob::GlobError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.results.len() {
            let result = match &self.results[self.index] {
                Ok(path) => Ok(path.clone()),
                Err(e) => Err(glob::GlobError),
            };
            self.index += 1;
            Some(result)
        } else {
            None
        }
    }
}

// Stub glob function that returns an iterator
fn glob(_pattern: &str) -> Result<GlobIterator, glob::GlobError> {
    // Stub implementation - returns empty iterator
    Ok(GlobIterator {
        results: Vec::new(),
        index: 0,
    })
}

// Stub glob error type
mod glob {
    use std::io;
    #[derive(Debug)]
    pub struct GlobError;
}

pub async fn execute(input: GlobInput) -> Result<serde_json::Value, ToolError> {
    let base_path = input.path.unwrap_or_else(|| ".".to_string());
    let full_pattern = if base_path == "." {
        input.pattern.clone()
    } else {
        format!("{}/{}", base_path, input.pattern)
    };

    let mut files = Vec::new();

    for entry in glob(&full_pattern)
        .map_err(|e| ToolError::FileError(format!("Invalid glob pattern: {:?}", e)))?
    {
        match entry {
            Ok(path) => {
                if let Some(path_str) = path.to_str() {
                    files.push(path_str.to_string());
                }
            }
            Err(e) => {
                // Log error but continue processing other files
                eprintln!("Error reading path: {:?}", e);
            }
        }
    }

    // Sort files by modification time (if possible) or name
    files.sort();

    let output = GlobOutput {
        count: files.len(),
        files,
        pattern: input.pattern,
    };

    serde_json::to_value(output).map_err(ToolError::from)
}

