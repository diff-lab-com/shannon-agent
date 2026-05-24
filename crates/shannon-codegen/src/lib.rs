//! shannon-codegen - Tree-sitter based code understanding for Shannon Code
//!
//! cargo fmt expands `if let A && B` into nested `if let` blocks that
//! clippy flags as collapsible. Suppress project-wide since the formatting
//! is intentional.
#![allow(clippy::collapsible_if, clippy::collapsible_match)]
//!
//! This crate provides code analysis capabilities using tree-sitter grammars:
//! - Language detection from file extensions
//! - Symbol outline extraction (functions, classes, etc.)
//! - Repository map generation
//!
//! ## Example
//!
//! ```rust,no_run
//! use shannon_codegen::{file_outline, generate_repomap};
//! use std::path::Path;
//!
//! // Get outline for a single file
//! let symbols = file_outline(Path::new("src/main.rs")).unwrap();
//!
//! // Generate repository map
//! let repo_map = generate_repomap(Path::new("."), 100).unwrap();
//! ```

mod languages;
mod outline;
mod repomap;

pub use languages::{LanguageConfig, language_for_name, language_for_path, supported_languages};
pub use outline::{Symbol, SymbolKind, file_outline, file_outline_content};
pub use repomap::{FileSummary, RepoMap, generate_repomap, generate_repomap_filtered};

/// Error types for codegen operations
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("Language not supported for path: {0}")]
    UnsupportedLanguage(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Tree-sitter error: {0}")]
    TreeSitter(String),

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Result type for codegen operations
pub type Result<T> = std::result::Result<T, CodegenError>;
