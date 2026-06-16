//! Benchmarks for VCR recording/replay and JSONL session recording.
//!
//! Measures write/read throughput for RecordingEntry items and VCR lookup.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use serde_json::json;

use shannon_core::recording::RecordingEntry;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a list of RecordingEntry items simulating a realistic session.
fn build_recording_entries(count: usize) -> Vec<RecordingEntry> {
    let mut entries = Vec::with_capacity(count);

    // SessionStart
    entries.push(RecordingEntry::SessionStart {
        session_id: "bench-session-001".to_string(),
        model: "claude-3-opus".to_string(),
        timestamp: "2026-05-22T10:00:00Z".to_string(),
    });

    for i in 1..count {
        match i % 6 {
            0 => entries.push(RecordingEntry::UserMessage {
                content: format!("Please help me implement feature {i}. I need a function that processes data efficiently."),
                turn: i / 6 + 1,
            }),
            1 => entries.push(RecordingEntry::LlmRequest {
                turn: i / 6 + 1,
                request_hash: format!("sha256:{i:040}"),
                body: json!({"model": "claude-3-opus", "max_tokens": 4096, "messages": []}),
            }),
            2 => entries.push(RecordingEntry::LlmResponse {
                turn: i / 6 + 1,
                body: json!({"id": format!("msg_{i}"), "content": [{"type": "text", "text": "Here is the implementation..."}]}),
            }),
            3 => entries.push(RecordingEntry::ToolCall {
                tool: "Read".to_string(),
                input: json!({"path": format!("src/module_{i}.rs")}),
                result: format!("fn process_{i}() -> bool {{ true }}"),
                is_error: false,
                duration_ms: 15 + (i as u64) % 50,
            }),
            4 => entries.push(RecordingEntry::QueryEvent {
                event: shannon_core::QueryEvent::Text {
                    query_id: uuid::Uuid::new_v4(),
                    content: format!("Processing module {i}..."),
                },
            }),
            5 => {
                if i == count - 1 {
                    entries.push(RecordingEntry::SessionEnd {
                        session_id: "bench-session-001".to_string(),
                        total_turns: count / 6,
                        total_tokens: count as u64 * 250,
                    });
                } else {
                    entries.push(RecordingEntry::ToolCall {
                        tool: "Bash".to_string(),
                        input: json!({"command": format!("cargo check --module {i}")}),
                        result: "Finished dev [unoptimized + debuginfo]".to_string(),
                        is_error: false,
                        duration_ms: 200 + (i as u64) % 500,
                    });
                }
            }
            _ => unreachable!(),
        }
    }

    entries
}

/// Write entries to a JSONL file and return the path.
fn write_jsonl(entries: &[RecordingEntry], dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("bench_recording.jsonl");
    let mut file = std::fs::File::create(&path).expect("create jsonl file");
    use std::io::Write;
    for entry in entries {
        let line = serde_json::to_string(entry).expect("serialize entry");
        writeln!(file, "{line}").expect("write entry");
    }
    path
}

/// Read entries back from a JSONL file.
fn read_jsonl(path: &std::path::Path) -> Vec<RecordingEntry> {
    let content = std::fs::read_to_string(path).expect("read jsonl file");
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("deserialize entry"))
        .collect()
}

// ---------------------------------------------------------------------------
// Benchmark groups
// ---------------------------------------------------------------------------

fn bench_recording_write(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let dir_path = dir.path().to_path_buf();

    let mut group = c.benchmark_group("recording_write");

    for &count in &[10, 50, 200] {
        let entries = build_recording_entries(count);
        group.bench_with_input(
            BenchmarkId::new("jsonl_write", count),
            &entries,
            |b, entries| {
                b.iter(|| {
                    let path = dir_path.join(format!("bench_write_{count}.jsonl"));
                    let mut file = std::fs::File::create(&path).expect("create file");
                    use std::io::Write;
                    for entry in entries {
                        let line = serde_json::to_string(entry).expect("serialize");
                        let _ = writeln!(file, "{line}");
                    }
                    let _ = std::fs::remove_file(&path);
                });
            },
        );
    }

    group.finish();
}

fn bench_recording_read(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut group = c.benchmark_group("recording_read");

    for &count in &[10, 50, 200] {
        // Pre-write the file so we can benchmark only the read
        let entries = build_recording_entries(count);
        let path = write_jsonl(&entries, dir.path());

        group.bench_with_input(BenchmarkId::new("jsonl_read", count), &path, |b, path| {
            b.iter(|| {
                let _entries: Vec<RecordingEntry> = read_jsonl(path);
            });
        });

        let _ = std::fs::remove_file(&path);
    }

    group.finish();
}

fn bench_recording_serialize(c: &mut Criterion) {
    let entries = build_recording_entries(50);

    let mut group = c.benchmark_group("recording_serialize");

    // Serialize single entry
    group.bench_function("single_entry", |b| {
        b.iter(|| serde_json::to_string(&entries[0]))
    });

    // Serialize batch
    group.bench_function("batch_50_entries", |b| {
        b.iter(|| {
            entries
                .iter()
                .map(serde_json::to_string)
                .collect::<Vec<_>>()
        })
    });

    // Deserialize single entry
    let serialized: Vec<String> = entries
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();

    group.bench_function("deserialize_single", |b| {
        b.iter(|| serde_json::from_str::<RecordingEntry>(&serialized[0]))
    });

    group.bench_function("deserialize_batch_50", |b| {
        b.iter(|| {
            serialized
                .iter()
                .map(|s| serde_json::from_str::<RecordingEntry>(s))
                .collect::<Vec<_>>()
        })
    });

    group.finish();
}

fn bench_vcr_lookup(c: &mut Criterion) {
    use shannon_core::vcr::{Vcr, VcrConfig};

    let dir = tempfile::tempdir().expect("tempdir");
    let dir_path = dir.path().to_path_buf();

    c.bench_function("vcr_create_empty", |b| {
        b.iter(|| {
            let config = VcrConfig::with_dir(&dir_path);
            Vcr::new(config)
        })
    });

    // VCR find_recording lookup benchmark
    let mut vcr = Vcr::for_recording(&dir_path);
    // Populate with some recordings
    for i in 0..20 {
        let request = json!({"model": "claude-3-opus", "messages": [{"role": "user", "content": format!("query {i}")}]});
        let response = json!({"content": [{"type": "text", "text": format!("response {i}")}]});
        let _ = vcr.record(request, response, vec![format!("tag_{i}")]);
    }

    c.bench_function("vcr_find_recording_20_loaded", |b| {
        b.iter(|| {
            vcr.find_recording("query 10");
        })
    });

    c.bench_function("vcr_list_recordings", |b| b.iter(|| vcr.list_recordings()));
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
    targets = bench_recording_write,
        bench_recording_read,
        bench_recording_serialize,
        bench_vcr_lookup
}
criterion_main!(benches);
