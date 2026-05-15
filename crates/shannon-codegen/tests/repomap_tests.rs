//! Integration tests for repository map generation.

use shannon_codegen::{generate_repomap, generate_repomap_filtered, FileSummary, RepoMap};

// ── Basic RepoMap Generation ────────────────────────────────────────────

#[test]
fn test_repomap_from_temp_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(
        root.join("main.rs"),
        "pub fn main() { println!(\"hello\"); }\n",
    )
    .unwrap();

    std::fs::write(
        root.join("app.py"),
        "def main():\n    print('hello')\n",
    )
    .unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 2);
    assert!(repo_map.total_symbols >= 2);
    assert!(repo_map.total_lines >= 2);
}

#[test]
fn test_repomap_rust_file_symbols() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(
        root.join("lib.rs"),
        "pub struct Config;\npub fn load() -> Config { Config }\n",
    )
    .unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 1);

    let file = &repo_map.files[0];
    assert_eq!(file.language, "Rust");
    let names: Vec<&str> = file.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Config"), "Expected Config struct");
    assert!(names.contains(&"load"), "Expected load function");
}

#[test]
fn test_repomap_python_file_symbols() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(
        root.join("main.py"),
        "class App:\n    def run(self):\n        pass\n",
    )
    .unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 1);

    let file = &repo_map.files[0];
    assert_eq!(file.language, "Python");
    let names: Vec<&str> = file.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"App"), "Expected App class");
}

#[test]
fn test_repomap_multiple_languages() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("app.py"), "def app(): pass\n").unwrap();
    std::fs::write(root.join("index.js"), "function index() {}\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 3);

    let languages: Vec<&str> = repo_map.files.iter().map(|f| f.language.as_str()).collect();
    assert!(languages.contains(&"Rust"));
    assert!(languages.contains(&"Python"));
    assert!(languages.contains(&"JavaScript"));
}

// ── File Filtering ──────────────────────────────────────────────────────

#[test]
fn test_repomap_skips_unsupported_extensions() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("data.txt"), "some text data\n").unwrap();
    std::fs::write(root.join("config.yaml"), "key: value\n").unwrap();
    std::fs::write(root.join("image.png"), "not a real image\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 1);
    assert_eq!(repo_map.files[0].path, "main.rs");
}

#[test]
fn test_repomap_extension_filter() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("app.py"), "def app(): pass\n").unwrap();
    std::fs::write(root.join("index.js"), "function index() {}\n").unwrap();

    // Filter to only Python files
    let repo_map = generate_repomap_filtered(root, &["py"], 10).unwrap();
    assert_eq!(repo_map.files.len(), 1);
    assert_eq!(repo_map.files[0].language, "Python");
}

#[test]
fn test_repomap_extension_filter_multiple() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("app.py"), "def app(): pass\n").unwrap();
    std::fs::write(root.join("index.js"), "function index() {}\n").unwrap();

    let repo_map = generate_repomap_filtered(root, &["rs", "js"], 10).unwrap();
    assert_eq!(repo_map.files.len(), 2);
    let languages: Vec<&str> = repo_map.files.iter().map(|f| f.language.as_str()).collect();
    assert!(languages.contains(&"Rust"));
    assert!(languages.contains(&"JavaScript"));
    assert!(!languages.contains(&"Python"));
}

// ── max_files Budget ────────────────────────────────────────────────────

#[test]
fn test_repomap_max_files_limit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // Create 5 Rust files
    for i in 0..5 {
        std::fs::write(root.join(format!("file{i}.rs")), "fn func() {}\n").unwrap();
    }

    // Limit to 3 files
    let repo_map = generate_repomap(root, 3).unwrap();
    assert!(
        repo_map.files.len() <= 3,
        "Expected at most 3 files, got {}",
        repo_map.files.len()
    );
}

#[test]
fn test_repomap_max_files_one() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(root.join("b.rs"), "fn b() {}\n").unwrap();

    let repo_map = generate_repomap(root, 1).unwrap();
    assert!(
        repo_map.files.len() <= 1,
        "Expected at most 1 file, got {}",
        repo_map.files.len()
    );
}

// ── Empty Directory Handling ────────────────────────────────────────────

#[test]
fn test_repomap_empty_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert!(repo_map.files.is_empty());
    assert_eq!(repo_map.total_symbols, 0);
    assert_eq!(repo_map.total_lines, 0);
}

#[test]
fn test_repomap_directory_with_only_unsupported_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::write(root.join("readme.txt"), "Hello world\n").unwrap();
    std::fs::write(root.join("config.json"), "{}\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert!(repo_map.files.is_empty());
}

// ── Gitignore Handling ──────────────────────────────────────────────────

#[test]
fn test_repomap_respects_gitignore() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // Initialize a git repo so gitignore rules are respected by the ignore crate
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("git init should succeed");

    std::fs::write(root.join(".gitignore"), "ignored_dir/\n*.generated.rs\n").unwrap();

    std::fs::create_dir(root.join("ignored_dir")).unwrap();
    std::fs::write(root.join("ignored_dir/secret.rs"), "fn secret() {}\n").unwrap();

    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("code.generated.rs"), "fn generated() {}\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();

    let paths: Vec<&str> = repo_map.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"main.rs"), "Should include main.rs");
    // ignored_dir/secret.rs and code.generated.rs should be excluded
    assert!(
        !paths.iter().any(|p| p.contains("ignored_dir")),
        "Should not include gitignored directory, got {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("generated")),
        "Should not match *.generated.rs pattern, got {paths:?}"
    );
}

// ── Subdirectory Walking ────────────────────────────────────────────────

#[test]
fn test_repomap_walks_subdirectories() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn lib_func() {}\n").unwrap();

    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/test.rs"), "fn test_func() {}\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 2);

    let paths: Vec<&str> = repo_map.files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.iter().any(|p| p.contains("lib.rs")),
        "Should find src/lib.rs"
    );
    assert!(
        paths.iter().any(|p| p.contains("test.rs")),
        "Should find tests/test.rs"
    );
}

// ── File Summary Sorting ───────────────────────────────────────────────

#[test]
fn test_repomap_files_sorted_by_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    // Create files in non-alphabetical order
    std::fs::write(root.join("z_lib.rs"), "fn z() {}\n").unwrap();
    std::fs::write(root.join("a_main.rs"), "fn a() {}\n").unwrap();
    std::fs::write(root.join("m_mid.rs"), "fn m() {}\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    let paths: Vec<&str> = repo_map.files.iter().map(|f| f.path.as_str()).collect();

    let mut sorted = paths.to_vec();
    sorted.sort();
    assert_eq!(paths, sorted, "Files should be sorted alphabetically by path");
}

// ── Line Count ──────────────────────────────────────────────────────────

#[test]
fn test_repomap_counts_lines() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let content = "line 1\nline 2\nline 3\nline 4\nline 5\n";
    std::fs::write(root.join("five.rs"), content).unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files.len(), 1);
    assert_eq!(repo_map.files[0].lines, 5);
    assert_eq!(repo_map.total_lines, 5);
}

// ── Relative Paths ─────────────────────────────────────────────────────

#[test]
fn test_repomap_uses_relative_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    let repo_map = generate_repomap(root, 10).unwrap();
    assert_eq!(repo_map.files[0].path, "src/main.rs");
}

// ── RepoMap Serialization ──────────────────────────────────────────────

#[test]
fn test_repomap_json_serialization() {
    let repo_map = RepoMap {
        files: vec![FileSummary {
            path: "src/main.rs".to_string(),
            language: "Rust".to_string(),
            symbols: vec![],
            lines: 10,
        }],
        total_symbols: 0,
        total_lines: 10,
    };

    let json = serde_json::to_string(&repo_map).unwrap();
    assert!(json.contains("src/main.rs"));
    assert!(json.contains("Rust"));
}

#[test]
fn test_file_summary_json_serialization() {
    let summary = FileSummary {
        path: "lib.rs".to_string(),
        language: "Rust".to_string(),
        symbols: vec![shannon_codegen::Symbol {
            name: "main".to_string(),
            kind: shannon_codegen::SymbolKind::Function,
            start_line: 1,
            end_line: 5,
            children: vec![],
        }],
        lines: 100,
    };

    let json = serde_json::to_string_pretty(&summary).unwrap();
    assert!(json.contains("lib.rs"));
    assert!(json.contains("main"));
    assert!(json.contains("function"));
}
