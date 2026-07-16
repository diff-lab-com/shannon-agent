//! Standalone benchmark for `lsp_code_actions` against a real Rust file.
//! Spawns rust-analyzer, measures full request latency.
//!
//! ```sh
//! cargo run --manifest-path <repo>/Cargo.toml \
//!     --example bench_lsp --features tauri -q -- <file.rs> <line> <char>
//! ```

use std::time::Instant;

#[tokio::main]
async fn main() {
    let file_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "src/scheduled_commands.rs".to_string());
    let abs = std::fs::canonicalize(&file_path)
        .unwrap_or_else(|e| panic!("canonicalize {file_path}: {e}"));

    let req = shannon_desktop::lsp_commands::CodeActionRequest {
        file_path: abs.to_string_lossy().into_owned(),
        server_cmd: "rust-analyzer".into(),
        server_args: vec![],
        start_line: 0,
        start_character: 0,
        end_line: 0,
        end_character: 1,
        language_id: "rust".into(),
        diagnostic_messages: vec![],
    };

    let working_dir = abs.parent().unwrap_or_else(|| std::path::Path::new("."));
    let start = Instant::now();
    let result = shannon_desktop::lsp_commands::lsp_code_actions_inner(working_dir, req).await;
    let elapsed = start.elapsed();

    match result {
        Ok(res) => {
            println!("lsp_code_actions: {elapsed:?}");
            println!("  actions: {}", res.actions.len());
        }
        Err(e) => {
            println!("lsp_code_actions failed after {elapsed:?}");
            println!("  error: {e}");
        }
    }
}
