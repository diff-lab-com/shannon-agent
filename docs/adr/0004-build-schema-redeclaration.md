# ADR-0004: `build.rs` schema-stubs redeclaration pattern

**Date**: 2026-07-22
**Status**: Proposed (Φ0 retrospective — awaiting human partner confirmation)
**Sprint**: continuous

## Context

`crates/shannon-types/build.rs` generates `provider-model-config.schema.json`
(draft-07 JSON Schema) and `events.schema.json` at compile time via
`schemars::schema_for!()`. The macro requires a concrete Rust type reference.

`build.rs` runs **before** `src/` compiles — it cannot import the runtime
types due to a chicken-and-egg dependency (build script is a Cargo
prerequisite for compiling the library crate).

Current workaround: `build.rs` contains a `mod schema_stubs { ... }` block
that **redeclares the relevant subset** of types from
`src/provider_config.rs` (~200 lines as of Φ0 HEAD `fa13896`). This is
functionally correct but creates a real **drift risk**: any change to
`src/provider_config.rs` (new field, new type, type rename) must be
manually mirrored in `build.rs::schema_stubs`. Otherwise `schemars`
silently emits `null` for the missed piece.

### Observed drift in Φ0.6

The Φ0.6 implementer added a new `CredentialScope` field to
`ModelProfile`. After the first build, a `jq` self-check revealed
`CredentialScope` was emitted as `null` in the regenerated schema —
the `ModelProfile.credential_scope` field had been added to
`src/provider_config.rs` but the build.rs redeclaration block was
out-of-sync. The implementer caught and fixed it before commit. Without
the `jq` self-check, drift would have shipped silently.

The whole-branch reviewer (opus) flagged this as a recurring risk and
recommended "ADR + drift-detection discipline". This ADR is the
response.

## Decision

Adopt **Option A — redeclaration + manual sync discipline + CI gate** as
the build-time schema-derivation strategy.

### 1. `build.rs::schema_stubs` redeclaration block

- Lives at `crates/shannon-types/build.rs:42–56` (module decl) +
  `:130–340` (redeclaration). ~200 lines as of HEAD.
- This block is the **source of generated schema**, NOT a source of
  truth for runtime behavior. The two must match; drift is a bug.
- `// KEEP IN SYNC WITH: src/provider_config.rs` comment placed above
  the block to flag the constraint for future readers.

### 2. Manual sync discipline

When a PR modifies `src/provider_config.rs`:
- If the change adds/removes/renames any type or field, the PR **must**
  also update `build.rs::schema_stubs`.
- Reviewers **must** reject PRs that violate this rule.
- The Φ0 ledger guardrail
  (`shannon-code/.superpowers/sdd/progress.md`) already mandates this;
  this ADR formalizes the policy.

### 3. CI gate (schema-stability test)

Add `crates/shannon-types/tests/schema_stability.rs`:
- Invokes `cargo build -p shannon-types` to regenerate schema
- Reads the generated `schema/*.json` files
- Diffs against the committed versions
- Fails the build on any mismatch

This catches three failure modes:
- Drift between `src/` and `build.rs::schema_stubs`
- Accidental `src/` change without mirror update
- `schemars` version churn that changes output format

### 4. Brief template update

When extracting a task brief that adds to `src/provider_config.rs`,
the brief **must** explicitly list "mirror to `build.rs::schema_stubs`"
as a deliverable, with the new symbol named verbatim. Reviewers verify
this item is checked off in the implementer's self-review before
accepting the task report.

## Consequences

### Positive

- **Drift detected at CI time, not downstream** — caught before
  consumers (CLI / Gateway / IDE completions) see a broken schema
- **No new workspace member** — minimal surface change
- **Reviewer discipline is auditable** — PR diff shows the build.rs
  sync (or its absence) explicitly
- **Schema contract is locked** and protected against `schemars` version
  churn (test fails on any regenerated diff)

### Negative

- **~200 lines of duplication remain** in `build.rs`; intentional
  and documented. This is the price of build-time schema derivation.
- **CI runtime +30s** for the `cargo build -p shannon-types` invocation
  inside the test. Acceptable for a single-crate workspace member.
- **Reviewers must remember the sync requirement**. Mitigated by this
  ADR reference + brief-template mandate.

### Neutral

- One new test file (~30 LoC) in `tests/schema_stability.rs`
- Brief-extraction template gains one mandatory checklist item

## Alternatives Considered

### A. Separate schema-derivation crate (`shannon-types-schema`)

**Rejected**: out of proportion to the problem. Adds a new workspace
member to publish/version; `build.rs` would import precompiled types,
which still hits a chicken-and-egg unless the schema crate is
dependency-only (extra Cargo machinery). The ~200 LoC duplication is
cheaper than the indirection.

### B. Macro-based codegen (proc-macro derives schema in-place)

**Rejected**: requires writing a proc-macro crate; compile-time cost;
debugging surface. We are already using `schemars`'s derive macros —
adding a second codegen layer doubles the moving parts.

### C. Runtime schema export (`schema_for!()` at test time, write file)

**Rejected**: schema not available at build-time to downstream consumers
(CLI / Gateway / future migration tools that load it during compilation).
Test-time export forces regeneration on every test run (slow, noisy diffs
that drown real signals).

### D. Drop the schema artifact; types are the contract

**Rejected**: cross-sibling protocol contract (per
[[shannon-agent-consolidation]]) requires an out-of-language schema for
downstream validation, IDE completion, and migration tool development.
Types-only blocks these consumers.

## Implementation References

- `crates/shannon-types/build.rs` — current redeclaration block (~200
  lines; drift surface)
- `crates/shannon-types/src/provider_config.rs` — source of truth
- `crates/shannon-types/schema/provider-model-config.schema.json` —
  committed output (draft-07, 11 definitions)
- `crates/shannon-types/schema/events.schema.json` — also emitted from
  `build.rs` (events mirror; same drift surface)
- `crates/shannon-types/tests/provider_config_schema.rs` — existing
  integration tests (verify against committed schema)
- `shannon-code/.superpowers/sdd/progress.md` — Φ0.6 drift ledger entry
- `shannon-code/.superpowers/sdd/global-constraints.md` — guardrail
  for build.rs sync discipline

## Open Questions

- **OQ-1**: Should `build.rs::schema_stubs` types be wrapped with
  `#[allow(dead_code)]` to suppress warnings about fields the runtime
  doesn't read (the `schemars` derive reads them via the macro)? Proposed:
  yes, with `// KEEP: schemars derive reads these fields; build.rs
  runtime does not` comment per field.
- **OQ-2**: Should the schema-stability test also validate
  `schema/examples/*.toml` + `schema/examples/*.json` against the
  regenerated schema? Proposed: yes, fold into the same test
  (`ajv` already imported for cross-validation in
  `scripts/validate-provider-config.mjs`).
- **OQ-3**: Should drift detection run as a pre-commit hook in
  addition to CI? Proposed: defer until we see actual drift in CI
  (chicken-and-egg with build-time test running inside a hook).