//! Drift guard (§P2-7): the checked-in `gateway/src/engine/types.gen.ts`
//! must be byte-identical to what `gen-ts` emits from the current Rust
//! protocol types.
//!
//! If this test fails, the generated file is stale: run
//! `cargo run -p shannon-api-protocol --bin gen-ts` and commit the result
//! together with the Rust-side protocol change.
//!
//! Note this only guards regeneration drift. The type *list* inside
//! `gen_ts.rs::collect_entries()` is still hand-maintained: a newly added
//! protocol type that is forgotten there produces no drift and must be
//! caught in review.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generated_ts_matches_codegen_output() {
    // Same resolution as the generator: workspace root two levels above the
    // crate manifest, then the checked-in generated module.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root must have a parent two levels above the manifest dir");
    let dest = workspace_root.join("gateway/src/engine/types.gen.ts");
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
