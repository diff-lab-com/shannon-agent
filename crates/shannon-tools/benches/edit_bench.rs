//! Benchmarks for the file edit tool.
//!
//! Measures performance of string replacement, unique-match finding,
//! and diff computation across files of 100, 1000, and 10000 lines.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::io::Write;

use shannon_tools::file::edit::{compute_diff_hunks, perform_edit};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp directory and write a file with `n_lines` of Rust-like code.
/// Returns (temp_dir, file_path) so the temp dir lives for the benchmark.
fn create_temp_file(n_lines: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(format!("bench_edit_{n_lines}.rs"));
    let mut file = std::fs::File::create(&path).expect("create file");
    for i in 0..n_lines {
        writeln!(
            file,
            "fn function_{i:06}() -> Result<String, Box<dyn std::error::Error>> {{"
        )
        .expect("write line");
        writeln!(file, "    let data = format!(\"processing item {i}\");").expect("write line");
        writeln!(file, "    Ok(data)").expect("write line");
        writeln!(file, "}}").expect("write line");
    }
    (dir, path)
}

/// Read a file's content into a string.
fn read_content(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("read file")
}

/// Generate a unique needle present exactly once in the file at a specific line.
fn unique_needle(line_index: usize) -> String {
    format!("processing item {line_index}")
}

/// Generate replacement text for the needle.
fn replacement_text(line_index: usize) -> String {
    format!("processed item {line_index} [updated]")
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_perform_edit_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit/perform_edit_single");
    for &n_lines in &[100, 1_000, 10_000] {
        let (_dir, path) = create_temp_file(n_lines);
        let content = read_content(&path);
        // Pick a needle near the middle for realistic search cost
        let target = n_lines / 2;
        let old = unique_needle(target);
        let new = replacement_text(target);
        group.throughput(criterion::Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("single_replace", n_lines),
            &n_lines,
            |b, _| {
                b.iter(|| perform_edit(&content, &old, &new, false).unwrap());
            },
        );
    }
    group.finish();
}

fn bench_perform_edit_replace_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit/perform_edit_replace_all");
    for &n_lines in &[100, 1_000, 10_000] {
        let (_dir, path) = create_temp_file(n_lines);
        let content = read_content(&path);
        // "Ok(data)" appears on every function, so replace_all has real work
        let old = "Ok(data)";
        let new = "Ok(data.into())";
        let expected_count = content.matches(old).count();
        assert!(expected_count > 0, "needle must appear in file");
        group.throughput(criterion::Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("replace_all", n_lines),
            &n_lines,
            |b, _| {
                b.iter(|| {
                    let result = perform_edit(&content, &old, &new, true).unwrap();
                    assert_eq!(result.1, expected_count);
                    result
                });
            },
        );
    }
    group.finish();
}

fn bench_perform_edit_not_found(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit/perform_edit_not_found");
    for &n_lines in &[100, 1_000, 10_000] {
        let (_dir, path) = create_temp_file(n_lines);
        let content = read_content(&path);
        group.throughput(criterion::Throughput::Bytes(content.len() as u64));
        group.bench_with_input(BenchmarkId::new("not_found", n_lines), &n_lines, |b, _| {
            b.iter(|| {
                let result = perform_edit(&content, "ZZZ_NOT_PRESENT_ANYWHERE", "replaced", false);
                assert!(result.is_err());
                result
            });
        });
    }
    group.finish();
}

fn bench_compute_diff_hunks(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit/compute_diff_hunks");
    for &n_lines in &[100, 1_000, 10_000] {
        let (_dir, path) = create_temp_file(n_lines);
        let old_content = read_content(&path);
        // Replace a single occurrence to produce a small diff
        let target = n_lines / 2;
        let needle = unique_needle(target);
        let new_content = old_content.replacen(&needle, &replacement_text(target), 1);
        group.throughput(criterion::Throughput::Bytes(old_content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("single_change", n_lines),
            &n_lines,
            |b, _| {
                b.iter(|| compute_diff_hunks(&old_content, &new_content));
            },
        );
    }
    group.finish();
}

fn bench_compute_diff_hunks_many_changes(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit/compute_diff_hunks_many_changes");
    for &n_lines in &[100, 1_000, 10_000] {
        let (_dir, path) = create_temp_file(n_lines);
        let old_content = read_content(&path);
        // Change every 10th function to produce many scattered diffs
        let mut new_content = old_content.clone();
        for i in (0..n_lines).step_by(10) {
            let needle = unique_needle(i);
            let repl = replacement_text(i);
            new_content = new_content.replacen(&needle, &repl, 1);
        }
        group.throughput(criterion::Throughput::Bytes(old_content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("many_changes", n_lines),
            &n_lines,
            |b, _| {
                b.iter(|| compute_diff_hunks(&old_content, &new_content));
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_perform_edit_single,
    bench_perform_edit_replace_all,
    bench_perform_edit_not_found,
    bench_compute_diff_hunks,
    bench_compute_diff_hunks_many_changes,
);
criterion_main!(benches);
