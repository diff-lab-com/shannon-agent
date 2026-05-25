//! Benchmarks for repository map generation.
//!
//! Measures performance of RepoMap generation across directories
//! with 10, 100, and 1000 source files of various types.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::io::Write;

use shannon_codegen::generate_repomap;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp directory tree with `n_files` Rust source files.
/// Each file contains a handful of functions and structs to exercise
/// the symbol extraction pipeline.
fn create_rust_project(n_files: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("create src dir");

    for i in 0..n_files {
        let path = src.join(format!("module_{i:04}.rs"));
        let mut file = std::fs::File::create(&path).expect("create file");
        writeln!(file, "//! Module {i}").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "use std::collections::HashMap;").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "pub struct Item{i} {{").unwrap();
        writeln!(file, "    id: usize,").unwrap();
        writeln!(file, "    name: String,").unwrap();
        writeln!(file, "}}").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "impl Item{i} {{").unwrap();
        writeln!(file, "    pub fn new(id: usize, name: String) -> Self {{").unwrap();
        writeln!(file, "        Self {{ id, name }}").unwrap();
        writeln!(file, "    }}").unwrap();
        writeln!(file).unwrap();
        writeln!(
            file,
            "    pub fn process(&self) -> Result<(), Box<dyn std::error::Error>> {{"
        )
        .unwrap();
        writeln!(file, "        Ok(())").unwrap();
        writeln!(file, "    }}").unwrap();
        writeln!(file, "}}").unwrap();
        writeln!(file).unwrap();
        writeln!(
            file,
            "fn helper_{i}(data: &HashMap<String, String>) -> Vec<String> {{"
        )
        .unwrap();
        writeln!(file, "    data.keys().cloned().collect()").unwrap();
        writeln!(file, "}}").unwrap();
    }

    dir
}

/// Create a temp directory with mixed Rust and Python files.
fn create_mixed_project(n_files: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("create src dir");

    for i in 0..n_files {
        if i % 3 == 0 {
            // Python file
            let path = src.join(format!("module_{i:04}.py"));
            let mut file = std::fs::File::create(&path).expect("create file");
            writeln!(file, "# Module {i}").unwrap();
            writeln!(file).unwrap();
            writeln!(file, "class Handler{i}:").unwrap();
            writeln!(file, "    def __init__(self, name):").unwrap();
            writeln!(file, "        self.name = name").unwrap();
            writeln!(file).unwrap();
            writeln!(file, "    def process(self):").unwrap();
            writeln!(file, "        return self.name.upper()").unwrap();
            writeln!(file).unwrap();
            writeln!(file, "def helper_{i}(items):").unwrap();
            writeln!(file, "    return [x for x in items if x]").unwrap();
        } else {
            // Rust file
            let path = src.join(format!("module_{i:04}.rs"));
            let mut file = std::fs::File::create(&path).expect("create file");
            writeln!(file, "pub fn process_{i}() -> String {{").unwrap();
            writeln!(file, "    format!(\"result {i}\")").unwrap();
            writeln!(file, "}}").unwrap();
        }
    }

    dir
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_generate_repomap_rust(c: &mut Criterion) {
    let mut group = c.benchmark_group("repomap/generate_rust");
    for &n_files in &[10, 100, 1000] {
        let dir = create_rust_project(n_files);
        group.bench_with_input(BenchmarkId::new("rust_files", n_files), &n_files, |b, _| {
            b.iter(|| generate_repomap(dir.path(), n_files).unwrap());
        });
    }
    group.finish();
}

fn bench_generate_repomap_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("repomap/generate_mixed");
    for &n_files in &[10, 100, 1000] {
        let dir = create_mixed_project(n_files);
        group.bench_with_input(
            BenchmarkId::new("mixed_rs_py", n_files),
            &n_files,
            |b, _| {
                b.iter(|| generate_repomap(dir.path(), n_files + 10).unwrap());
            },
        );
    }
    group.finish();
}

fn bench_generate_repomap_filtered(c: &mut Criterion) {
    let mut group = c.benchmark_group("repomap/generate_filtered");
    for &n_files in &[10, 100, 1000] {
        let dir = create_mixed_project(n_files);
        group.bench_with_input(BenchmarkId::new("rs_only", n_files), &n_files, |b, _| {
            b.iter(|| {
                shannon_codegen::generate_repomap_filtered(dir.path(), &["rs"], n_files + 10)
                    .unwrap()
            });
        });
    }
    group.finish();
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
    targets = bench_generate_repomap_rust,
        bench_generate_repomap_mixed,
        bench_generate_repomap_filtered
}
criterion_main!(benches);
