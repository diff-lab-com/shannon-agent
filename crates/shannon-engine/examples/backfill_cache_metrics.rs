//! One-shot script: backfill top-level `cache_creation_input_tokens` and
//! `cache_read_input_tokens` fields in JSONL fixtures.
//!
//! # Why this exists
//!
//! Two classes of fixtures ended up without correct cache metrics:
//!
//! 1. **MiniMax / OpenAI-compatible** recordings (29 fixtures): `extract_cache_metrics`
//!    didn't recognize the OpenAI `usage.prompt_tokens_details.cached_tokens` shape,
//!    so `cache_*` was recorded as 0 even when the upstream API reported cache hits.
//!    Fixed in commit `3a9f280`; this script reapplies the fixed extractor.
//!
//! 2. **Zhipu-Coding / Anthropic-compatible** recordings (30 fixtures): committed
//!    on 2026-06-02, *before* `cache_*` fields were added to `RecordedExchange`
//!    on 2026-06-24. The fields are absent (not zero). The response bodies
//!    contain the data; this script extracts and backfills the top-level fields.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example backfill_cache_metrics -p shannon-engine -- \
//!     tests/fixtures/real_tasks
//! ```
//!
//! Default path is `tests/fixtures/real_tasks` if no argument is given.
//! Pass `--dry-run` to print stats without writing.

use shannon_engine::testing::record_replay::RecordedExchange;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let args = env::args().skip(1);
    let mut dry_run = false;
    let mut target: Option<PathBuf> = None;

    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            other => target = Some(PathBuf::from(other)),
        }
    }

    let target = target.unwrap_or_else(|| PathBuf::from("tests/fixtures/real_tasks"));

    if !target.is_dir() {
        eprintln!("error: {} is not a directory", target.display());
        std::process::exit(2);
    }

    let mut total_files = 0usize;
    let mut total_lines = 0usize;
    let mut updated_lines = 0usize;
    let mut already_correct = 0usize;
    let mut missing_response_body = 0usize;
    let mut total_read_added: u64 = 0;
    let mut total_create_added: u64 = 0;

    let entries: Vec<PathBuf> = match fs::read_dir(&target) {
        Ok(e) => e.filter_map(|e| e.ok().map(|d| d.path())).collect(),
        Err(e) => {
            eprintln!("error: read_dir({}): {e}", target.display());
            std::process::exit(2);
        }
    };

    for path in entries {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        total_files += 1;
        match backfill_file(&path, dry_run) {
            Ok(stats) => {
                total_lines += stats.total;
                updated_lines += stats.updated;
                already_correct += stats.already_correct;
                missing_response_body += stats.missing_body;
                total_read_added += stats.read_added;
                total_create_added += stats.create_added;
            }
            Err(e) => eprintln!("warn: {}: {e}", path.display()),
        }
    }

    println!();
    println!("=== Backfill summary ===");
    println!("target:            {}", target.display());
    println!(
        "mode:              {}",
        if dry_run { "dry-run" } else { "write" }
    );
    println!("files scanned:     {total_files}");
    println!("lines scanned:     {total_lines}");
    println!("lines updated:     {updated_lines}");
    println!("already correct:   {already_correct}");
    println!("missing body:      {missing_response_body}");
    println!("read tokens added: {total_read_added}");
    println!("create tokens added: {total_create_added}");
}

struct FileStats {
    total: usize,
    updated: usize,
    already_correct: usize,
    missing_body: usize,
    read_added: u64,
    create_added: u64,
}

fn backfill_file(path: &Path, dry_run: bool) -> Result<FileStats, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;

    let mut stats = FileStats {
        total: 0,
        updated: 0,
        already_correct: 0,
        missing_body: 0,
        read_added: 0,
        create_added: 0,
    };

    let mut new_lines: Vec<String> = Vec::with_capacity(64);
    let mut any_change = false;

    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            new_lines.push(line.to_string());
            continue;
        }
        stats.total += 1;

        let mut exchange: RecordedExchange =
            serde_json::from_str(line).map_err(|e| format!("line {}: parse: {e}", idx + 1))?;

        // Extract from response body using the (now-fixed) extractor.
        let body = exchange.response.body.as_str();
        if body.is_empty() {
            stats.missing_body += 1;
            new_lines.push(line.to_string());
            continue;
        }

        let (new_created, new_read) = RecordedExchange::extract_cache_metrics(body);

        let old_created = exchange.cache_creation_input_tokens;
        let old_read = exchange.cache_read_input_tokens;

        // "Already correct" = top-level fields already match what the
        // extractor computes from response.body. This is the steady state
        // for any fixture recorded after the fix.
        if old_created == new_created && old_read == new_read && old_created != 0 {
            stats.already_correct += 1;
        } else if old_created == new_created && old_read == new_read {
            // Both old and new are zero — genuinely no cache to record.
            stats.already_correct += 1;
        } else {
            stats.updated += 1;
            stats.read_added += new_read as u64;
            stats.create_added += new_created as u64;
            exchange.cache_creation_input_tokens = new_created;
            exchange.cache_read_input_tokens = new_read;
            any_change = true;
            let new_line = serde_json::to_string(&exchange)
                .map_err(|e| format!("line {}: serialize: {e}", idx + 1))?;
            new_lines.push(new_line);
            continue;
        }

        new_lines.push(line.to_string());
    }

    if any_change && !dry_run {
        let mut new_content = new_lines.join("\n");
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        fs::write(path, new_content).map_err(|e| format!("write: {e}"))?;
    }

    Ok(stats)
}
