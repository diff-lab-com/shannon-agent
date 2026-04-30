//! Language detection and grammar registry
//!
//! Provides language detection from file extensions and tree-sitter grammar lookup.

use crate::{CodegenError, Result};
use std::path::Path;
use std::sync::OnceLock;

/// Language configuration with file extensions and grammar function
#[derive(Debug, Clone)]
pub struct LanguageConfig {
    /// Language name (e.g., "Rust", "Python")
    pub name: &'static str,
    /// File extensions (e.g., ["rs", "rust"])
    pub extensions: &'static [&'static str],
    /// Tree-sitter language identifier
    pub ts_id: TreeSitterLanguage,
}

/// Tree-sitter language identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeSitterLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
}

impl TreeSitterLanguage {
    /// Get the tree-sitter Language for this identifier
    ///
    /// Returns None if the language feature is not enabled
    #[allow(unreachable_code)]
    pub fn to_language(self) -> Option<tree_sitter::Language> {
        match self {
            TreeSitterLanguage::Rust => {
                #[cfg(feature = "rust")]
                return Some(tree_sitter_rust::LANGUAGE.into());
                None
            }
            TreeSitterLanguage::Python => {
                #[cfg(feature = "python")]
                return Some(tree_sitter_python::LANGUAGE.into());
                None
            }
            TreeSitterLanguage::JavaScript => {
                #[cfg(feature = "javascript")]
                return Some(tree_sitter_javascript::LANGUAGE.into());
                None
            }
            TreeSitterLanguage::TypeScript => {
                #[cfg(feature = "typescript")]
                return Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into());
                None
            }
            TreeSitterLanguage::Go => {
                #[cfg(feature = "go")]
                return Some(tree_sitter_go::LANGUAGE.into());
                None
            }
            TreeSitterLanguage::Java => {
                #[cfg(feature = "java")]
                return Some(tree_sitter_java::LANGUAGE.into());
                None
            }
            TreeSitterLanguage::C => {
                #[cfg(feature = "c")]
                return Some(tree_sitter_c::LANGUAGE.into());
                None
            }
            TreeSitterLanguage::Cpp => {
                #[cfg(feature = "cpp")]
                return Some(tree_sitter_cpp::LANGUAGE.into());
                None
            }
        }
    }
}

/// Get all supported language configurations
fn supported_languages_list() -> &'static [LanguageConfig] {
    static LANGUAGES: OnceLock<Vec<LanguageConfig>> = OnceLock::new();

    LANGUAGES.get_or_init(|| {
        vec![
            LanguageConfig {
                name: "Rust",
                extensions: &["rs", "rust"],
                ts_id: TreeSitterLanguage::Rust,
            },
            LanguageConfig {
                name: "Python",
                extensions: &["py", "pyi", "pyw"],
                ts_id: TreeSitterLanguage::Python,
            },
            LanguageConfig {
                name: "JavaScript",
                extensions: &["js", "jsx", "mjs", "cjs"],
                ts_id: TreeSitterLanguage::JavaScript,
            },
            LanguageConfig {
                name: "TypeScript",
                extensions: &["ts", "tsx"],
                ts_id: TreeSitterLanguage::TypeScript,
            },
            LanguageConfig {
                name: "Go",
                extensions: &["go"],
                ts_id: TreeSitterLanguage::Go,
            },
            LanguageConfig {
                name: "Java",
                extensions: &["java"],
                ts_id: TreeSitterLanguage::Java,
            },
            LanguageConfig {
                name: "C",
                extensions: &["c", "h"],
                ts_id: TreeSitterLanguage::C,
            },
            LanguageConfig {
                name: "C++",
                extensions: &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
                ts_id: TreeSitterLanguage::Cpp,
            },
        ]
    })
}

/// Detect language from file path
///
/// # Arguments
///
/// * `path` - File path to analyze
///
/// # Returns
///
/// Language configuration if supported, None otherwise
pub fn language_for_path(path: &Path) -> Option<LanguageConfig> {
    let extension = path.extension()?.to_str()?;

    for lang in supported_languages_list() {
        if lang.extensions.contains(&extension) {
            return Some(lang.clone());
        }
    }

    None
}

/// Get language configuration by name
///
/// # Arguments
///
/// * `name` - Language name (case-insensitive)
///
/// # Returns
///
/// Language configuration if found, None otherwise
pub fn language_for_name(name: &str) -> Option<LanguageConfig> {
    let name_lower = name.to_lowercase();

    for lang in supported_languages_list() {
        if lang.name.to_lowercase() == name_lower {
            return Some(lang.clone());
        }
    }

    None
}

/// Get all supported languages
///
/// # Returns
///
/// Vector of all language configurations
pub fn supported_languages() -> Vec<LanguageConfig> {
    supported_languages_list().to_vec()
}

/// Get tree-sitter language from path
///
/// # Arguments
///
/// * `path` - File path to analyze
///
/// # Returns
///
/// Tree-sitter Language if supported and feature enabled
///
/// # Errors
///
/// Returns CodegenError if language is not supported or feature not enabled
pub fn get_language_for_path(path: &Path) -> Result<tree_sitter::Language> {
    let config = language_for_path(path)
        .ok_or_else(|| CodegenError::UnsupportedLanguage(path.display().to_string()))?;

    config.ts_id.to_language()
        .ok_or_else(|| CodegenError::UnsupportedLanguage(
            format!("{} (feature not enabled)", config.name)
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_for_path() {
        assert_eq!(language_for_path(Path::new("test.rs")).unwrap().name, "Rust");
        assert_eq!(language_for_path(Path::new("test.py")).unwrap().name, "Python");
        assert_eq!(language_for_path(Path::new("test.js")).unwrap().name, "JavaScript");
        assert_eq!(language_for_path(Path::new("test.ts")).unwrap().name, "TypeScript");
        assert_eq!(language_for_path(Path::new("test.go")).unwrap().name, "Go");
        assert_eq!(language_for_path(Path::new("test.java")).unwrap().name, "Java");
        assert!(language_for_path(Path::new("test.unknown")).is_none());
    }

    #[test]
    fn test_language_for_name() {
        assert_eq!(language_for_name("rust").unwrap().name, "Rust");
        assert_eq!(language_for_name("RUST").unwrap().name, "Rust");
        assert_eq!(language_for_name("python").unwrap().name, "Python");
        assert!(language_for_name("unknown").is_none());
    }

    #[test]
    fn test_supported_languages() {
        let langs = supported_languages();
        assert!(!langs.is_empty());
        assert!(langs.iter().any(|l| l.name == "Rust"));
        assert!(langs.iter().any(|l| l.name == "Python"));
    }
}
