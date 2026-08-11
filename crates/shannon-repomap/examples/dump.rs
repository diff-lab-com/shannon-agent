//! Demo: dump the first 50 lines of the system-prompt markdown for the
//! `shannon-types` crate. Run via:
//!
//! ```sh
//! cargo run -p shannon-repomap --example dump
//! ```

use shannon_repomap::RepoMap;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("shannon-repomap lives under crates/");
    let types_crate = workspace_root.join("shannon-types");

    let mut repo_map = RepoMap::for_workspace(&types_crate)?;

    let tokens_before = repo_map.token_estimate();
    repo_map.trim_to_budget(2_000);
    let tokens_after = repo_map.token_estimate();

    eprintln!(
        "walked {} files in {}; tokens before={} after trim={}",
        repo_map.map.files.len(),
        types_crate.display(),
        tokens_before,
        tokens_after,
    );

    let md = repo_map.to_system_prompt_markdown();
    for line in md.lines().take(50) {
        println!("{line}");
    }
    Ok(())
}
