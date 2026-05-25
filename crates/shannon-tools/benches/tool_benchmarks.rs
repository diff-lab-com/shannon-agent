//! Benchmarks for shannon-tools operations.
//!
//! Measures file read/write/edit patterns, grep search, and glob matching
//! using underlying operations (std::fs, regex, globset).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::fs;
use std::io::Write;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp directory with test files of the given sizes.
fn create_test_files(
    dir: &std::path::Path,
    file_count: usize,
    size_bytes: usize,
) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::with_capacity(file_count);
    for i in 0..file_count {
        let path = dir.join(format!("test_file_{i:04}.txt"));
        let content = generate_content(size_bytes, i);
        let mut file = fs::File::create(&path).expect("create test file");
        file.write_all(content.as_bytes()).expect("write test file");
        paths.push(path);
    }
    paths
}

/// Generate deterministic content of approximately `size_bytes`.
fn generate_content(size_bytes: usize, seed: usize) -> String {
    let line = format!(
        "// Line from seed {seed}: Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor."
    );
    let line_len = line.len() + 1; // +1 for newline
    let line_count = size_bytes / line_len;
    let mut content = String::with_capacity(size_bytes);
    for i in 0..line_count {
        content.push_str(&format!("L{i:06}: {line}\n"));
    }
    // Pad to approximate target size
    if content.len() < size_bytes {
        content.push_str(&line);
    }
    content
}

fn file_size_label(size: usize) -> &'static str {
    match size {
        0..=1024 => "1KB",
        1025..=102_400 => "100KB",
        _ => "1MB",
    }
}

// ---------------------------------------------------------------------------
// File read benchmarks
// ---------------------------------------------------------------------------

fn bench_file_read(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut group = c.benchmark_group("file_read");

    for &size in &[1_024, 102_400, 1_048_576] {
        let paths = create_test_files(dir.path(), 1, size);
        let path = &paths[0];
        let label = file_size_label(size);

        group.bench_with_input(BenchmarkId::new("std_fs_read", label), path, |b, path| {
            b.iter(|| fs::read(path));
        });

        group.bench_with_input(
            BenchmarkId::new("std_fs_read_to_string", label),
            path,
            |b, path| {
                b.iter(|| fs::read_to_string(path));
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// File edit (search + replace) benchmarks
// ---------------------------------------------------------------------------

fn bench_file_edit(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut group = c.benchmark_group("file_edit");

    for &size in &[1_024, 102_400] {
        let paths = create_test_files(dir.path(), 1, size);
        let path = &paths[0];
        let label = file_size_label(size);

        // Benchmark: read, do string replace, write back
        let original = fs::read_to_string(path).expect("read file");
        let search = "Lorem ipsum";
        let replace = "REPLACED TEXT";

        group.bench_with_input(
            BenchmarkId::new("string_replace", label),
            &original,
            |b, content| {
                b.iter(|| {
                    let modified = content.replace(search, replace);
                    let _ = modified.len(); // prevent optimization
                });
            },
        );

        // Full edit cycle: read → replace → write
        let edit_path = dir.path().join(format!("edit_target_{label}.txt"));
        fs::write(&edit_path, &original).expect("write edit target");

        group.bench_with_input(
            BenchmarkId::new("full_edit_cycle", label),
            &edit_path,
            |b, path| {
                b.iter(|| {
                    let content = fs::read_to_string(path).expect("read");
                    let modified = content.replace(search, replace);
                    fs::write(path, &modified).expect("write");
                });
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Grep (regex search) benchmarks
// ---------------------------------------------------------------------------

fn bench_grep_search(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut group = c.benchmark_group("grep_search");

    for &file_count in &[10, 100] {
        let paths = create_test_files(dir.path(), file_count, 10_000);
        let pattern = regex::Regex::new(r"L\d{6}").expect("compile regex");

        // Read all files into memory (simulating file traversal)
        let file_contents: Vec<(std::path::PathBuf, String)> = paths
            .iter()
            .map(|p| (p.clone(), fs::read_to_string(p).expect("read file")))
            .collect();

        group.bench_with_input(
            BenchmarkId::new("regex_match", file_count),
            &file_contents,
            |b, files| {
                b.iter(|| {
                    let mut match_count = 0usize;
                    for (_path, content) in files {
                        for _mat in pattern.find_iter(content) {
                            match_count += 1;
                        }
                    }
                    assert!(match_count > 0);
                });
            },
        );

        // Benchmark regex compilation
        group.bench_with_input(
            BenchmarkId::new("regex_compile", file_count),
            &file_count,
            |b, _| {
                b.iter(|| {
                    let _ = regex::Regex::new(r"L\d{6}").expect("compile");
                });
            },
        );

        // Multi-pattern search
        let patterns = vec![
            regex::Regex::new(r"L\d{6}").unwrap(),
            regex::Regex::new(r"seed \d+").unwrap(),
            regex::Regex::new(r"Lorem").unwrap(),
            regex::Regex::new(r"dolor").unwrap(),
            regex::Regex::new(r"adipiscing").unwrap(),
        ];

        let patterns_ref = &patterns;
        let file_contents_ref = &file_contents;
        group.bench_with_input(
            BenchmarkId::new("multi_pattern_5_regex", file_count),
            &(file_contents_ref, patterns_ref),
            |b, (files, pats): &(&Vec<(std::path::PathBuf, String)>, &Vec<regex::Regex>)| {
                b.iter(|| {
                    let mut total = 0usize;
                    for (_path, content) in *files {
                        for pat in (*pats).iter() {
                            for _ in pat.find_iter(content) {
                                total += 1;
                            }
                        }
                    }
                    let _ = total;
                });
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Glob pattern matching benchmarks
// ---------------------------------------------------------------------------

fn bench_glob_pattern(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_pattern");

    // Build globset matchers
    let patterns: Vec<&str> = vec![
        "*.rs",
        "**/*.rs",
        "src/**/*.rs",
        "*.{rs,toml}",
        "crates/shannon-*/src/**/*.rs",
        "**/tests/*.rs",
    ];

    let test_paths: Vec<&str> = vec![
        "src/main.rs",
        "crates/shannon-core/src/lib.rs",
        "crates/shannon-core/src/api/types.rs",
        "Cargo.toml",
        "crates/shannon-tools/src/tools/read.rs",
        "tests/integration_test.rs",
        "README.md",
        "crates/shannon-core/benches/core_benchmarks.rs",
    ];

    for &pattern in &patterns {
        let glob = globset::GlobBuilder::new(pattern)
            .build()
            .expect("build glob")
            .compile_matcher();

        group.bench_with_input(
            BenchmarkId::new("globset_match", pattern),
            &glob,
            |b, matcher| {
                b.iter(|| {
                    for path in &test_paths {
                        let _ = matcher.is_match(*path);
                    }
                });
            },
        );
    }

    // Benchmark globset compilation
    group.bench_function("globset_compile", |b| {
        b.iter(|| {
            globset::GlobBuilder::new("**/*.rs")
                .build()
                .expect("build")
                .compile_matcher()
        })
    });

    // Benchmark multi-pattern globset
    let mut builder = globset::GlobSetBuilder::new();
    for &p in &patterns {
        builder.add(globset::Glob::new(p).expect("parse glob"));
    }
    let globset = builder.build().expect("build globset");

    group.bench_function("globset_multi_match_8_paths", |b| {
        b.iter(|| {
            for path in &test_paths {
                let _ = globset.is_match(*path);
            }
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// File traversal benchmark
// ---------------------------------------------------------------------------

fn bench_file_traversal(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");

    // Create a nested directory structure with files
    for i in 0..5 {
        let sub_dir = dir.path().join(format!("subdir_{i}"));
        fs::create_dir_all(&sub_dir).expect("create subdir");
        for j in 0..20 {
            let path = sub_dir.join(format!("file_{j}.rs"));
            fs::write(&path, generate_content(500, i * 20 + j)).expect("write file");
        }
        // Add some non-rs files
        for j in 0..5 {
            let path = sub_dir.join(format!("data_{j}.json"));
            fs::write(&path, generate_content(200, i * 5 + j + 100)).expect("write file");
        }
    }

    c.bench_function("walkdir_100_rs_files", |b| {
        b.iter(|| {
            let mut count = 0usize;
            let walker = ignore::WalkBuilder::new(dir.path()).build();
            for entry in walker {
                let entry = entry.expect("walk entry");
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        if ext == "rs" {
                            count += 1;
                        }
                    }
                }
            }
            assert_eq!(count, 100, "should find 100 .rs files");
        })
    });
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .noise_threshold(0.03)
        .confidence_level(0.98)
        .significance_level(0.02)
        .sample_size(50)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_file_read,
        bench_file_edit,
        bench_grep_search,
        bench_glob_pattern,
        bench_file_traversal
}
criterion_main!(benches);
