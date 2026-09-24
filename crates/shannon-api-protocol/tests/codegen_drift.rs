//! Drift guard (§P2-7): the checked-in `gateway/src/engine/types.gen.ts`
//! must be byte-identical to what `gen-ts` emits from the current Rust
//! protocol types.
//!
//! If this test fails, the generated file is stale: run
//! `cargo run -p shannon-api-protocol --bin gen-ts` and commit the result
//! together with the Rust-side protocol change.
//!
//! Regeneration drift alone cannot catch a type that was never *added* to
//! `gen_ts.rs::collect_entries()` in the first place, so
//! [`every_pub_protocol_type_is_emitted`] also guards the type list itself.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    // Same resolution as the generator: workspace root two levels above the
    // crate manifest.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root must have a parent two levels above the manifest dir")
        .to_path_buf()
}

fn generated_ts_path() -> PathBuf {
    workspace_root().join("gateway/src/engine/types.gen.ts")
}

#[test]
fn generated_ts_matches_codegen_output() {
    let dest = generated_ts_path();
    let checked_in = std::fs::read(&dest).expect(
        "gateway/src/engine/types.gen.ts must exist (it is the checked-in codegen artifact)",
    );

    // The test binary's own package builds the `gen-ts` bin first and exposes
    // its path, so no nested `cargo run` is needed.
    let gen_bin = env!("CARGO_BIN_EXE_gen-ts");
    let status = Command::new(gen_bin)
        .status()
        .expect("spawn gen-ts codegen binary");
    assert!(status.success(), "gen-ts exited with failure: {status}");

    let regenerated = std::fs::read(&dest).expect("gen-ts wrote types.gen.ts");
    if regenerated != checked_in {
        // Restore the checked-in bytes so the test never leaves the working
        // tree dirtier than it found it.
        std::fs::write(&dest, &checked_in).expect("restore checked-in types.gen.ts");
        panic!(
            "gateway/src/engine/types.gen.ts is stale relative to shannon-api-protocol; \
             run `cargo run -p shannon-api-protocol --bin gen-ts` and commit the regenerated file"
        );
    }
}

/// Types deliberately withheld from the generated module. Every entry needs a
/// justification here; the point of the guard is that the default for a new
/// `pub` type is to be generated.
const GENERATION_SKIPPED: &[&str] = &[];

/// §P2-7 list-drift guard: every `pub struct` / `pub enum` declared in the
/// protocol crate root must be emitted into `types.gen.ts`. A type forgotten
/// in `gen_ts.rs::collect_entries()` produces no *regeneration* drift, so it
/// would otherwise reach gateway clients only by luck.
#[test]
fn every_pub_protocol_type_is_emitted() {
    let lib =
        std::fs::read_to_string(workspace_root().join("crates/shannon-api-protocol/src/lib.rs"))
            .expect("protocol crate root must exist");

    let mut declared: Vec<&str> = lib
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            for keyword in ["pub struct ", "pub enum "] {
                if let Some(rest) = line.strip_prefix(keyword) {
                    let name: &str = rest
                        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .next()
                        .unwrap_or("");
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
            None
        })
        .collect();
    declared.sort_unstable();
    declared.dedup();
    assert!(
        !declared.is_empty(),
        "scan found no pub types; the scan itself is broken"
    );

    let generated =
        std::fs::read_to_string(generated_ts_path()).expect("checked-in types.gen.ts must exist");
    let missing: Vec<&str> = declared
        .iter()
        .filter(|name| {
            !GENERATION_SKIPPED.contains(name)
                && !(generated.contains(&format!("export interface {name} {{"))
                    || generated.contains(&format!("export type {name} =")))
        })
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "pub protocol types missing from the generated TS contract: {missing:?}; \
         add them to gen_ts.rs::collect_entries() and regenerate \
         (`cargo run -p shannon-api-protocol --bin gen-ts`), or justify the skip in \
         GENERATION_SKIPPED"
    );
}
