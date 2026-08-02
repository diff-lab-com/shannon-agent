//! Integration tests for the parser + budget + repo-map wiring.
//!
//! These tests live in `tests/` (Cargo's per-crate integration-test slot) so
//! they exercise the public API only — no `#[cfg(test)]` reach-around.

use shannon_repomap::{
    RepoMap,
    budget::trim_to_budget,
    parser::parse_rust_file,
    symbol_tree::{SymbolKind, SymbolMap},
};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn names_matching<'a>(
    syms: &'a [shannon_repomap::SymbolNode],
    name: &str,
) -> Vec<&'a shannon_repomap::SymbolNode> {
    syms.iter().filter(|s| s.name == name).collect()
}

#[test]
fn parse_simple_function_and_struct_from_inline_source() {
    // Minimal end-to-end parse on a tiny inline string. Confirms both
    // function_item and struct_item are surfaced at file scope.
    let tmp = std::env::temp_dir().join("shannon_repomap_simple.rs");
    std::fs::write(
        &tmp,
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\
         pub struct Point { pub x: f64, pub y: f64 }\n",
    )
    .expect("write tmp");

    let syms = parse_rust_file(&tmp).expect("parse inline fixture");
    assert!(
        names_matching(&syms, "add")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Function)),
        "expected Function symbol named `add`, got: {syms:#?}"
    );
    assert!(
        names_matching(&syms, "Point")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Struct)),
        "expected Struct symbol named `Point`, got: {syms:#?}"
    );
}

#[test]
fn parse_fixture_tiny_struct_surfaces_all_kinds() {
    let path = fixtures_dir().join("tiny_struct.rs");
    let syms = parse_rust_file(&path).expect("parse tiny_struct.rs");

    // Every top-level declaration must surface with the right kind.
    assert!(
        names_matching(&syms, "Point")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Struct)),
        "Point struct missing"
    );
    assert!(
        names_matching(&syms, "Color")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Enum)),
        "Color enum missing"
    );
    assert!(
        names_matching(&syms, "Alias")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::TypeAlias)),
        "Alias type missing"
    );
    assert!(
        names_matching(&syms, "ORIGIN")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Const)),
        "ORIGIN const missing"
    );

    // Signatures should be non-empty (we read at least one source line).
    let point = names_matching(&syms, "Point")[0];
    assert!(!point.signature.is_empty(), "Point signature was empty");
}

#[test]
fn parse_fixture_nested_mod_groups_methods_under_impl() {
    let path = fixtures_dir().join("mod_with_nested.rs");
    let syms = parse_rust_file(&path).expect("parse mod_with_nested.rs");

    // The `inner` module should be present at file scope.
    let inner = names_matching(&syms, "inner")
        .into_iter()
        .find(|s| matches!(s.kind, SymbolKind::Module))
        .expect("inner module missing");

    // `helper` is defined *inside* the `inner` mod, so it should surface as a
    // child of the module — not as a free function at the file root.
    let file_root_names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        !file_root_names.contains(&"helper"),
        "`helper` should live under `inner` mod, not at the file root, got: {file_root_names:#?}"
    );
    let helper = inner
        .children
        .iter()
        .find(|c| c.name == "helper")
        .expect("helper should be a child of inner mod");
    assert!(
        matches!(helper.kind, SymbolKind::Function),
        "helper should be a Function, got {:?}",
        helper.kind
    );
    let _ = names_matching; // silence unused-import lint if assertion above gets simplified later

    // Methods on the Holder impl are children of the Holder struct's impl,
    // not free-floating at the file root. The exact tree layout depends on
    // parser order — we just assert they're not at the file root.
    let file_root_names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        !file_root_names.contains(&"new"),
        "`new` method should not appear at the file root, got: {file_root_names:#?}"
    );
    assert!(
        !file_root_names.contains(&"doubled"),
        "`doubled` method should not appear at the file root, got: {file_root_names:#?}"
    );

    // `Holder` is inside `inner`, so it's reachable through inner.children.
    let holder = inner
        .children
        .iter()
        .find(|c| c.name == "Holder")
        .expect("Holder struct missing under inner mod");
    assert!(
        matches!(holder.kind, SymbolKind::Struct),
        "Holder should be a Struct, got {:?}",
        holder.kind
    );

    // The `impl Holder` block is a sibling of the struct under `inner`,
    // and its methods are children of the impl — not of the struct itself.
    let impl_holder = inner
        .children
        .iter()
        .find(|c| c.name.contains("Holder") && matches!(c.kind, SymbolKind::Impl))
        .expect("impl Holder block missing under inner mod");
    let method_names: Vec<&str> = impl_holder
        .children
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        method_names.contains(&"new"),
        "impl Holder should have a `new` method, got: {method_names:#?}"
    );
    assert!(
        method_names.contains(&"doubled"),
        "impl Holder should have a `doubled` method, got: {method_names:#?}"
    );

    // Sanity: the `inner` module itself has children.
    assert!(
        !inner.children.is_empty(),
        "inner module should expose nested items as children"
    );
}

#[test]
fn budget_cap_keeps_token_estimate_under_limit() {
    // Build a synthetic map: 5 files × 10 functions, each with a long
    // signature. Without trimming this is ~5×10×1000 chars = ~12,500 tokens,
    // well over the 2k budget we ask the slicer to enforce.
    let mut files = Vec::new();
    for f in 0..5 {
        let mut syms = Vec::new();
        for g in 0..10 {
            syms.push(shannon_repomap::SymbolNode {
                kind: SymbolKind::Function,
                name: format!("f{f}_g{g}"),
                span: shannon_repomap::Span {
                    start_line: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 0,
                },
                signature: "x".repeat(1_000),
                children: vec![],
            });
        }
        files.push((PathBuf::from(format!("/synthetic/file{f}.rs")), syms));
    }
    let mut map = SymbolMap {
        root: PathBuf::from("/synthetic"),
        files,
    };

    let before = shannon_repomap::budget::total_tokens(&map);
    assert!(
        before > 2_000,
        "synthetic map should exceed budget before trim"
    );

    trim_to_budget(&mut map, 2_000);
    let after = shannon_repomap::budget::total_tokens(&map);
    assert!(
        after <= 2_000,
        "expected post-trim tokens <= 2000, got {after}"
    );
}

#[test]
fn for_workspace_walks_dir_and_finds_fixtures() {
    // End-to-end smoke test: walk the fixtures dir and confirm the two .rs
    // fixtures show up in the output map.
    let dir = fixtures_dir();
    let repo = RepoMap::for_workspace(&dir).expect("for_workspace(fixtures)");
    let names: Vec<String> = repo
        .map
        .files
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n == "tiny_struct.rs"),
        "tiny_struct.rs missing from walk, got: {names:#?}"
    );
    assert!(
        names.iter().any(|n| n == "mod_with_nested.rs"),
        "mod_with_nested.rs missing from walk, got: {names:#?}"
    );

    // The markdown view should mention both files in their relative form.
    let md = repo.to_system_prompt_markdown();
    assert!(md.contains("tiny_struct.rs"), "md missing tiny_struct.rs");
    assert!(
        md.contains("mod_with_nested.rs"),
        "md missing mod_with_nested.rs"
    );
}
