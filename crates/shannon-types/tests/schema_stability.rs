//! T5 — schema-stability gate for the Φ0 `ProviderModelConfig` contract.
//!
//! Regenerates the JSON Schema from the **source-of-truth** Rust type
//! (`src/provider_config.rs`) and asserts it is structurally identical to the
//! committed file at `schema/provider-model-config.schema.json`.
//!
//! # What this guards (structural contract)
//! The comparison strips `description` keys (see below) and then requires
//! equality, so it catches the drift that actually breaks consumers:
//! - **Uncommitted drift** — `src/` changed but the committed schema was not
//!   regenerated (`cargo build -p shannon-types` rewrites it).
//! - **`build.rs` structural drift** — the committed file is produced by the
//!   redeclared stubs in `build.rs`; if a stub's field type / enum variant /
//!   serde attribute (`rename`, `tag`, `deny_unknown_fields`, …) falls out of
//!   sync with `src/`, the structural schemas diverge and this fails.
//! - **`schemars` version churn** that changes emitted structure.
//!
//! # Why `description` is excluded
//! `schemars` derives `description` from `///` doc comments. The `build.rs`
//! redeclaration block (ADR-0004) intentionally omits prose doc comments, so the
//! committed file is description-poor relative to `src/`. Prose is documentation,
//! not wire contract, so it is intentionally outside this gate's scope.
//!
//! # Permanent fix (follow-up, not in scope here)
//! The redeclaration that produces this description gap can be eliminated by
//! sharing one type file between `src/` and `build.rs` via `include!` — at which
//! point `description` would re-enter the gate and ADR-0004's discipline retires.

use shannon_types::provider_config::ProviderModelConfig;
use std::fs;

/// Recursively drop every `"description"` key so the comparison is over the
/// structural contract only (see module docs for rationale).
fn strip_descriptions(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            map.remove("description");
            for child in map.values_mut() {
                strip_descriptions(child);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_descriptions(item);
            }
        }
        _ => {}
    }
}

/// The committed schema must equal what `schemars` generates from the real type
/// (structurally — `description` excluded).
#[test]
fn provider_model_config_schema_matches_committed() {
    // Regenerate from the source-of-truth type in `src/` (NOT the build.rs stubs).
    let regenerated = schemars::schema_for!(ProviderModelConfig);
    let mut regen: serde_json::Value =
        serde_json::to_value(&regenerated).expect("regenerated schema -> Value");

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/schema/provider-model-config.schema.json"
    );
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read committed schema {path:?}: {e}"));
    let mut committed: serde_json::Value =
        serde_json::from_str(&text).expect("committed schema is valid JSON");

    strip_descriptions(&mut regen);
    strip_descriptions(&mut committed);

    assert_eq!(
        regen, committed,
        "ProviderModelConfig schema STRUCTURE drifted from the committed file.\n\
         Likely causes:\n  \
         (a) src/provider_config.rs changed without regenerating — run `cargo build -p shannon-types`;\n  \
         (b) the build.rs redeclaration block drifted structurally from src/ — sync it;\n  \
         (c) schemars was bumped and emits differently — regenerate and review.\n\
         See docs/adr/0004-build-schema-redeclaration.md."
    );
}
