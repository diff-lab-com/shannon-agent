//! Offline consistency check: every tracked fixture's top-level
//! `cache_creation_input_tokens` and `cache_read_input_tokens` must equal
//! what `extract_cache_metrics(response.body)` computes.
//!
//! # Why this exists
//!
//! Two failure modes this test catches:
//!
//! 1. **Extractor regression** — if `extract_cache_metrics` is changed
//!    (e.g. a new provider format is misparsed), every fixture's top-level
//!    cache fields would silently drift away from what the response body
//!    says. This test fails immediately.
//!
//! 2. **Fixture tampering** — if someone hand-edits a fixture's cache
//!    fields or `response.body` out of sync with each other, this test
//!    fails with a precise `path:line (hash=...): top vs body` diff.
//!
//! # Why only tracked fixtures
//!
//! Per `.gitignore` line 88, `tests/fixtures/real_tasks/*.jsonl` is ignored
//! with 4 VCR-replay fixtures exception'd in PR #75. The 55 other fixtures
//! (29 MiniMax + 26 Zhipu-Coding) are local-only artifacts that won't exist
//! in CI, so testing them here would create a CI-vs-local false signal.
//! Local devs can run `just cache-stats-compare` to spot-check the rest.
//!
//! # Cost
//!
//! ~4 fixtures × ~10 exchanges × < 1µs per extract ≈ < 1ms total.
//! No API key, no network. Always runnable.

use shannon_engine::testing::record_replay::RecordedExchange;
use std::fs;
use std::path::PathBuf;

/// VCR-replay fixtures tracked in git (.gitignore exception list from PR #75).
/// Adding a fixture here means committing it; the test will then guard it
/// against silent drift forever.
///
/// Paths are relative to the workspace root (NOT this crate), because the
/// fixture directory is shared across all crates.
const TRACKED_FIXTURES: &[&str] = &[
    "tests/fixtures/real_tasks/minimax_MiniMax-M3_bash_command.jsonl",
    "tests/fixtures/real_tasks/minimax_MiniMax-M3_create_file.jsonl",
    "tests/fixtures/real_tasks/minimax_MiniMax-M3_overwrite_existing_file.jsonl",
    "tests/fixtures/real_tasks/minimax_MiniMax-M3_read_and_edit.jsonl",
];

fn workspace_root() -> PathBuf {
    // crates/shannon-engine -> ../.. = workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn tracked_fixtures_cache_fields_match_response_body() {
    let root = workspace_root();
    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for rel_path in TRACKED_FIXTURES {
        let path = root.join(rel_path);
        let display = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| rel_path.to_string());

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                mismatches.push(format!("{display}: read failed: {e}"));
                continue;
            }
        };

        for (idx, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            let exchange: RecordedExchange = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(e) => {
                    mismatches.push(format!("{display}:{}: parse failed: {e}", idx + 1));
                    continue;
                }
            };

            checked += 1;

            let (exp_created, exp_read) =
                RecordedExchange::extract_cache_metrics(&exchange.response.body);

            if exchange.cache_creation_input_tokens != exp_created
                || exchange.cache_read_input_tokens != exp_read
            {
                let hash_short: String = exchange.request_hash.chars().take(8).collect();
                mismatches.push(format!(
                    "{display}:{} (hash={hash_short}): \
                     top create={}, read={}; \
                     from body: create={exp_created}, read={exp_read}",
                    idx + 1,
                    exchange.cache_creation_input_tokens,
                    exchange.cache_read_input_tokens,
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "{checked} exchanges checked across {} fixtures, {} mismatch(es):\n  {}",
        TRACKED_FIXTURES.len(),
        mismatches.len(),
        mismatches.join("\n  "),
    );
}
