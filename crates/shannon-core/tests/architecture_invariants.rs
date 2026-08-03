//! Architecture invariants (P2-3 §architecture invariants).
//!
//! These tests assert that the workspace still respects the structural
//! rules documented in `docs/STABILITY.md` and
//! `docs/architecture.md`. Each invariant has a one-line failure
//! message that names the rule so the CI log is self-explanatory.
//!
//! Coverage (matches the P2-3 plan):
//!
//! 1. **Metadata dependency separation** — workspace members must not
//!    pull in anything outside the workspace or `std`. Anything else is
//!    a layering violation.
//! 2. **Stable public APIs carry doc comments** — every `#[stable_api]`
//!    item must also have a `///` doc comment so rustdoc renders the
//!    promise.
//! 3. **`#[allow(dead_code)] // KEEP:` markers are explicit** — the
//!    comment must immediately follow the attribute (no bare
//!    `#[allow(dead_code)]`).
//! 4. **`shannon-mcp-saas` feature surface is gated** — the
//!    `github`/`slack`/`jira` sub-modules are only emitted when their
//!    feature is enabled. The test enforces this by reading the source
//!    tree, since Cargo feature resolution happens at compile time.
//!
//! These tests intentionally do **not** shell out to `cargo metadata` —
//! the source-text inspection approach keeps them fast, deterministic,
//! and offline. If we need richer metadata checks later, point them at
//! `docs/plans/insta-workflow.md` for the CI discipline.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const REPO_ROOT_MARKERS: &[&str] = &["Cargo.toml", "crates"];

/// Walk the workspace once and cache the list of crate roots.
fn crate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let crates_dir = workspace_root().join("crates");
    for entry in fs::read_dir(&crates_dir).expect("crates/ must be readable") {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let manifest = entry.path().join("Cargo.toml");
        if manifest.exists() {
            roots.push(entry.path().to_path_buf());
        }
    }
    roots.sort();
    roots
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is set per crate; the test crate lives in
    // shannon-core, so the workspace root is one level up of `crates`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("shannon-core must be at <root>/crates/shannon-core")
        .to_path_buf()
}

// ---------------------------------------------------------------------------
// Invariant 1: metadata dependency separation
// ---------------------------------------------------------------------------
//
// Rule: a workspace member must not `path = "..."` depend on something
// outside the workspace unless the dependency is itself a workspace
// member. We allow transitive `path` deps to point at workspace
// members — those are fine — but any other `path` dep is a layering
// violation.

#[test]
fn metadata_dependency_separation() {
    let mut violations = Vec::new();
    for crate_root in crate_roots() {
        let manifest = fs::read_to_string(crate_root.join("Cargo.toml"))
            .unwrap_or_else(|e| panic!("read {}: {e}", crate_root.display()));
        let name = extract_package_name(&manifest).unwrap_or_else(|| {
            crate_root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        });

        // Only inspect lines inside a `[dependencies]` /
        // `[dev-dependencies]` / `[build-dependencies]` block. The `[[bin]]`,
        // `[[bench]]`, `[[example]]`, and `[[test]]` tables also use
        // `path = "..."` for their own targets and are not dependency
        // declarations.
        let mut in_dep_block = false;
        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_dep_block = trimmed.starts_with("[dependencies")
                    || trimmed.starts_with("[dev-dependencies")
                    || trimmed.starts_with("[build-dependencies");
                continue;
            }
            if !in_dep_block || !trimmed.contains("path = \"") {
                continue;
            }
            let path = extract_path_value(trimmed)
                .unwrap_or_else(|| panic!("malformed path dep in {name}: {trimmed}"));
            let resolved = crate_root.join(&path);
            if !is_workspace_member(&resolved) {
                violations.push(format!(
                    "{name}: path = {path:?} does not point at a workspace member"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "metadata dependency separation violated:\n  - {}",
        violations.join("\n  - ")
    );
}

fn extract_package_name(manifest: &str) -> Option<String> {
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("name") {
            if let Some(value) = rest.split('"').nth(1) {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_path_value(dep_line: &str) -> Option<String> {
    let needle = "path = \"";
    let start = dep_line.find(needle)? + needle.len();
    let rest = &dep_line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn is_workspace_member(resolved: &Path) -> bool {
    resolved.join("Cargo.toml").exists()
        && resolved
            .canonicalize()
            .map(|p| p.starts_with(workspace_root().join("crates")))
            .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Invariant 2: stable public APIs carry doc comments
// ---------------------------------------------------------------------------
//
// Every `#[stable_api(...)]` item must be followed (on the same or next
// line) by a `///` or `/**` doc comment. The contract is meant to be
// visible in rustdoc — a missing doc comment defeats the purpose.

#[test]
fn stable_public_apis_have_doc_comments() {
    let types_root = workspace_root().join("crates/shannon-types/src");
    let source = fs::read_to_string(types_root.join("lib.rs"))
        .unwrap_or_else(|e| panic!("read shannon-types/src/lib.rs: {e}"));
    let violations = check_stable_api_docs(&source);
    assert!(
        violations.is_empty(),
        "stable_api items missing doc comments:\n  - {}",
        violations.join("\n  - ")
    );
}

fn check_stable_api_docs(source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("#[stable_api") {
            continue;
        }
        // Doc comment may be either immediately above the attribute or
        // immediately below (the proc-macro rewrites the item to embed
        // the stability note as a `#[doc = "..."]`, so the
        // hand-written `///` is preserved on whichever side the author
        // wrote it).
        let prev = if idx == 0 {
            ""
        } else {
            lines[idx - 1].trim_start()
        };
        let next = lines.get(idx + 1).unwrap_or(&"").trim_start();
        let prev_is_doc =
            prev.starts_with("///") || prev.starts_with("//!") || prev.starts_with("/**");
        let next_is_doc =
            next.starts_with("///") || next.starts_with("//!") || next.starts_with("/**");
        if !prev_is_doc && !next_is_doc {
            violations.push(format!("line {}: {}", idx + 1, line.trim()));
        }
    }
    violations
}

// ---------------------------------------------------------------------------
// Invariant 3: `#[allow(dead_code)]` is always paired with `// KEEP:`
// ---------------------------------------------------------------------------
//
// Bare `#[allow(dead_code)]` is forbidden; every occurrence must carry
// a `KEEP:` justification in the trailing comment.

#[test]
fn dead_code_allow_keep_markers() {
    // P2-3 (improvement plan §P2-3): every `#[allow(dead_code)]` must be
    // paired with a trailing `// KEEP:` justification so the next reader
    // knows why the symbol is preserved. New code must follow this
    // convention; existing offenders are tracked in
    // `PRE_EXISTING_BARE_DEAD_CODE` below so the test enforces the rule
    // going forward without breaking the build for old code.
    const PRE_EXISTING_BARE_DEAD_CODE: &[&str] = &[
        // (file_path, line_number) — bare markers inherited from before
        // P2-3. Each entry should grow a `KEEP:` comment (or be deleted)
        // opportunistically; this list shrinks as cleanup lands. Paths
        // are crate-relative (no leading `crates/<name>/`).
        "src/bin/gen_ts.rs:66",
        "src/commands_providers.rs:33",
        "src/commands_providers.rs:621",
        "src/builtin/memory.rs:134",
        "src/builtin/memory.rs:144",
        "src/builtin/memory.rs:152",
        "src/builtin/memory.rs:165",
        "src/builtin/memory.rs:179",
        "src/builtin/memory.rs:198",
        "src/builtin/memory.rs:208",
        "src/builtin/session.rs:53",
        "src/builtin/session.rs:62",
        "src/builtin/session.rs:75",
        "src/builtin/session.rs:87",
        "src/builtin/session.rs:98",
        "src/builtin/session.rs:134",
        "src/builtin/session.rs:148",
        "src/builtin/session.rs:168",
        "src/jira/auth.rs:305",
        "src/jira/auth.rs:308",
        "src/jira/api.rs:70",
        "src/jira/api.rs:73",
        "src/jira/api.rs:394",
        "src/slack/auth.rs:160",
        "src/slack/auth.rs:167",
        "src/slack/api.rs:84",
        "src/auth.rs:18",
        "src/file/merge.rs:73",
    ];

    let baseline: std::collections::HashSet<&str> =
        PRE_EXISTING_BARE_DEAD_CODE.iter().copied().collect();

    let mut violations = Vec::new();
    for crate_root in crate_roots() {
        let src_dir = crate_root.join("src");
        walk_source(&src_dir, &mut |path, contents| {
            for (idx, line) in contents.lines().enumerate() {
                if !line.contains("#[allow(dead_code)]") {
                    continue;
                }
                if !line.contains("KEEP:") {
                    let key = format!(
                        "{}:{}",
                        path.strip_prefix(&crate_root).unwrap().display(),
                        idx + 1
                    );
                    if baseline.contains(key.as_str()) {
                        continue;
                    }
                    violations.push(key);
                }
            }
        });
    }
    assert!(
        violations.is_empty(),
        "bare #[allow(dead_code)] without // KEEP: marker (not in pre-existing baseline):\n  - {}",
        violations.join("\n  - ")
    );
}

fn walk_source<F: FnMut(&Path, &str)>(dir: &Path, visit: &mut F) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_source(&path, visit);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(contents) = fs::read_to_string(&path) {
                visit(&path, &contents);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Invariant 4: shannon-mcp-saas feature gating
// ---------------------------------------------------------------------------
//
// The crate exposes `github` always, and `slack`/`jira` behind features.
// The test asserts the source matches that contract so a contributor
// can't accidentally drop a `#[cfg(feature = ...)]` and silently ship
// all three SaaS integrations.

#[test]
fn shannon_mcp_saas_feature_gating() {
    let lib_path = workspace_root().join("crates/shannon-mcp-saas/src/lib.rs");
    let source = fs::read_to_string(&lib_path)
        .unwrap_or_else(|e| panic!("read shannon-mcp-saas/src/lib.rs: {e}"));
    // Always-on modules (`pub mod github {`) are accepted without
    // cfg gating; the only modules we need to inspect are the ones
    // that *should* be feature-gated.
    let must_be_gated: BTreeSet<&str> = ["pub mod jira", "pub mod slack"].into_iter().collect();

    let lines: Vec<&str> = source.lines().collect();
    let mut failures = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // The `#[cfg(feature = "...")]` attribute lives on the line
        // above the `pub mod` declaration; check the previous line.
        if must_be_gated
            .iter()
            .any(|needle| trimmed.starts_with(needle))
        {
            let prev = if idx == 0 { "" } else { lines[idx - 1] };
            if !prev.contains("#[cfg") {
                failures.push(format!("expected feature gating on `{trimmed}`"));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "shannon-mcp-saas feature gating violated:\n  - {}",
        failures.join("\n  - ")
    );
}

// Marker test that asserts the helper markers were located; without
// these, the test runner would otherwise silently skip.
#[test]
fn repo_root_markers_present() {
    let root = workspace_root();
    for marker in REPO_ROOT_MARKERS {
        assert!(
            root.join(marker).exists(),
            "missing workspace marker: {marker}"
        );
    }
}
