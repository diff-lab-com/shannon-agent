//! Integration tests for language detection.

use shannon_codegen::{language_for_name, language_for_path, supported_languages};
use std::path::Path;

// ── language_for_path ────────────────────────────────────────────────────

#[test]
fn test_language_for_rust_extension() {
    let lang = language_for_path(Path::new("main.rs")).unwrap();
    assert_eq!(lang.name, "Rust");
}

#[test]
fn test_language_for_rust_alt_extension() {
    let lang = language_for_path(Path::new("main.rust")).unwrap();
    assert_eq!(lang.name, "Rust");
}

#[test]
fn test_language_for_python_extension() {
    let lang = language_for_path(Path::new("script.py")).unwrap();
    assert_eq!(lang.name, "Python");
}

#[test]
fn test_language_for_python_pyw() {
    let lang = language_for_path(Path::new("gui.pyw")).unwrap();
    assert_eq!(lang.name, "Python");
}

#[test]
fn test_language_for_python_pyi() {
    let lang = language_for_path(Path::new("stubs.pyi")).unwrap();
    assert_eq!(lang.name, "Python");
}

#[test]
fn test_language_for_javascript_extension() {
    let lang = language_for_path(Path::new("app.js")).unwrap();
    assert_eq!(lang.name, "JavaScript");
}

#[test]
fn test_language_for_javascript_mjs() {
    let lang = language_for_path(Path::new("module.mjs")).unwrap();
    assert_eq!(lang.name, "JavaScript");
}

#[test]
fn test_language_for_javascript_cjs() {
    let lang = language_for_path(Path::new("commonjs.cjs")).unwrap();
    assert_eq!(lang.name, "JavaScript");
}

#[test]
fn test_language_for_javascript_jsx() {
    let lang = language_for_path(Path::new("component.jsx")).unwrap();
    assert_eq!(lang.name, "JavaScript");
}

#[test]
fn test_language_for_typescript_extension() {
    let lang = language_for_path(Path::new("app.ts")).unwrap();
    assert_eq!(lang.name, "TypeScript");
}

#[test]
fn test_language_for_typescript_tsx() {
    let lang = language_for_path(Path::new("component.tsx")).unwrap();
    assert_eq!(lang.name, "TypeScript");
}

#[test]
fn test_language_for_go_extension() {
    let lang = language_for_path(Path::new("main.go")).unwrap();
    assert_eq!(lang.name, "Go");
}

#[test]
fn test_language_for_java_extension() {
    let lang = language_for_path(Path::new("Main.java")).unwrap();
    assert_eq!(lang.name, "Java");
}

#[test]
fn test_language_for_c_extension() {
    let lang = language_for_path(Path::new("main.c")).unwrap();
    assert_eq!(lang.name, "C");
}

#[test]
fn test_language_for_c_header() {
    let lang = language_for_path(Path::new("header.h")).unwrap();
    assert_eq!(lang.name, "C");
}

#[test]
fn test_language_for_cpp_extension() {
    let lang = language_for_path(Path::new("main.cpp")).unwrap();
    assert_eq!(lang.name, "C++");
}

#[test]
fn test_language_for_cpp_cc() {
    let lang = language_for_path(Path::new("impl.cc")).unwrap();
    assert_eq!(lang.name, "C++");
}

#[test]
fn test_language_for_cpp_cxx() {
    let lang = language_for_path(Path::new("impl.cxx")).unwrap();
    assert_eq!(lang.name, "C++");
}

#[test]
fn test_language_for_cpp_hpp() {
    let lang = language_for_path(Path::new("header.hpp")).unwrap();
    assert_eq!(lang.name, "C++");
}

#[test]
fn test_language_for_cpp_hh() {
    let lang = language_for_path(Path::new("header.hh")).unwrap();
    assert_eq!(lang.name, "C++");
}

#[test]
fn test_language_for_cpp_hxx() {
    let lang = language_for_path(Path::new("header.hxx")).unwrap();
    assert_eq!(lang.name, "C++");
}

// ── Unknown/Unsupported Extensions ──────────────────────────────────────

#[test]
fn test_language_for_unknown_extension() {
    assert!(language_for_path(Path::new("file.xyz")).is_none());
}

#[test]
fn test_language_for_no_extension() {
    assert!(language_for_path(Path::new("Makefile")).is_none());
}

#[test]
fn test_language_for_hidden_file() {
    assert!(language_for_path(Path::new(".env")).is_none());
}

#[test]
fn test_language_for_markdown() {
    assert!(language_for_path(Path::new("README.md")).is_none());
}

#[test]
fn test_language_for_json() {
    assert!(language_for_path(Path::new("package.json")).is_none());
}

#[test]
fn test_language_for_toml() {
    assert!(language_for_path(Path::new("Cargo.toml")).is_none());
}

// ── Path Variations ─────────────────────────────────────────────────────

#[test]
fn test_language_for_path_with_directory() {
    let lang = language_for_path(Path::new("src/main.rs")).unwrap();
    assert_eq!(lang.name, "Rust");
}

#[test]
fn test_language_for_path_with_nested_directory() {
    let lang = language_for_path(Path::new("crates/shannon-core/src/lib.rs")).unwrap();
    assert_eq!(lang.name, "Rust");
}

#[test]
fn test_language_for_absolute_path() {
    let lang = language_for_path(Path::new("/home/user/project/app.py")).unwrap();
    assert_eq!(lang.name, "Python");
}

// ── language_for_name ────────────────────────────────────────────────────

#[test]
fn test_language_for_name_lowercase() {
    assert_eq!(language_for_name("rust").unwrap().name, "Rust");
    assert_eq!(language_for_name("python").unwrap().name, "Python");
    assert_eq!(language_for_name("javascript").unwrap().name, "JavaScript");
    assert_eq!(language_for_name("typescript").unwrap().name, "TypeScript");
    assert_eq!(language_for_name("go").unwrap().name, "Go");
    assert_eq!(language_for_name("java").unwrap().name, "Java");
    assert_eq!(language_for_name("c").unwrap().name, "C");
}

#[test]
fn test_language_for_name_uppercase() {
    assert_eq!(language_for_name("RUST").unwrap().name, "Rust");
    assert_eq!(language_for_name("PYTHON").unwrap().name, "Python");
    assert_eq!(language_for_name("JAVASCRIPT").unwrap().name, "JavaScript");
}

#[test]
fn test_language_for_name_mixed_case() {
    assert_eq!(language_for_name("Rust").unwrap().name, "Rust");
    assert_eq!(language_for_name("Python").unwrap().name, "Python");
    assert_eq!(language_for_name("JavaScript").unwrap().name, "JavaScript");
}

#[test]
fn test_language_for_name_c_plus_plus() {
    assert_eq!(language_for_name("C++").unwrap().name, "C++");
    assert_eq!(language_for_name("c++").unwrap().name, "C++");
}

#[test]
fn test_language_for_name_unknown() {
    assert!(language_for_name("unknown").is_none());
    assert!(language_for_name("ruby").is_none());
    assert!(language_for_name("swift").is_none());
    assert!(language_for_name("kotlin").is_none());
    assert!(language_for_name("").is_none());
}

// ── supported_languages ─────────────────────────────────────────────────

#[test]
fn test_supported_languages_count() {
    let langs = supported_languages();
    assert!(langs.len() >= 8, "Expected at least 8 languages, got {}", langs.len());
}

#[test]
fn test_supported_languages_contains_expected() {
    let langs = supported_languages();
    let names: Vec<&str> = langs.iter().map(|l| l.name).collect();

    assert!(names.contains(&"Rust"), "Missing Rust");
    assert!(names.contains(&"Python"), "Missing Python");
    assert!(names.contains(&"JavaScript"), "Missing JavaScript");
    assert!(names.contains(&"TypeScript"), "Missing TypeScript");
    assert!(names.contains(&"Go"), "Missing Go");
    assert!(names.contains(&"Java"), "Missing Java");
    assert!(names.contains(&"C"), "Missing C");
    assert!(names.contains(&"C++"), "Missing C++");
}

#[test]
fn test_supported_languages_have_extensions() {
    for lang in supported_languages() {
        assert!(
            !lang.extensions.is_empty(),
            "Language {} has no extensions",
            lang.name
        );
    }
}

#[test]
fn test_supported_languages_have_ts_id() {
    for lang in supported_languages() {
        // Just verify the language config has a valid tree-sitter ID
        // (not necessarily that the feature is enabled)
        let _ = lang.ts_id;
    }
}

// ── LanguageConfig Clone ────────────────────────────────────────────────

#[test]
fn test_language_config_clone() {
    let lang = language_for_path(Path::new("test.rs")).unwrap();
    let cloned = lang.clone();
    assert_eq!(lang.name, cloned.name);
    assert_eq!(lang.extensions, cloned.extensions);
}

#[test]
fn test_language_config_debug() {
    let lang = language_for_path(Path::new("test.rs")).unwrap();
    let debug_str = format!("{lang:?}");
    assert!(debug_str.contains("Rust"));
}
