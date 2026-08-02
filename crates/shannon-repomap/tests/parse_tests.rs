//! Integration tests for the parser + budget + repo-map wiring.
//!
//! These tests live in `tests/` (Cargo's per-crate integration-test slot) so
//! they exercise the public API only — no `#[cfg(test)]` reach-around.

use shannon_repomap::{
    RepoMap,
    budget::trim_to_budget,
    parser::parse_file,
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

fn first_matching<'a>(
    syms: &'a [shannon_repomap::SymbolNode],
    name: &str,
) -> Option<&'a shannon_repomap::SymbolNode> {
    syms.iter().find(|s| s.name == name)
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

    let syms = parse_file(&tmp).expect("parse inline fixture");
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
    let syms = parse_file(&path).expect("parse tiny_struct.rs");

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
    let syms = parse_file(&path).expect("parse mod_with_nested.rs");

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

// ---------------------------------------------------------------------------
// Phase B — TypeScript / Python / Go
// ---------------------------------------------------------------------------

#[test]
fn parse_fixture_tiny_ts_surfaces_function_class_interface() {
    let path = fixtures_dir().join("tiny_ts.ts");
    let syms = parse_file(&path).expect("parse tiny_ts.ts");

    // Function: add
    assert!(
        names_matching(&syms, "add")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Function)),
        "expected `add` Function, got: {syms:#?}"
    );
    // Class: Counter, with methods nested under it
    let counter = first_matching(&syms, "Counter")
        .filter(|s| matches!(s.kind, SymbolKind::Class))
        .expect("Counter class missing");
    let method_names: Vec<&str> = counter.children.iter().map(|c| c.name.as_str()).collect();
    assert!(
        method_names.contains(&"increment"),
        "Counter should expose `increment` method, got: {method_names:#?}"
    );
    assert!(
        method_names.contains(&"greet"),
        "Counter should expose `greet` method, got: {method_names:#?}"
    );
    // Interface: Greeter
    assert!(
        names_matching(&syms, "Greeter")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Interface)),
        "Greeter interface missing, got: {syms:#?}"
    );
    // Type alias: Pair
    assert!(
        names_matching(&syms, "Pair")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::TypeAlias)),
        "Pair type alias missing, got: {syms:#?}"
    );
    // Top-level const: DEFAULT_LIMIT
    assert!(
        names_matching(&syms, "DEFAULT_LIMIT")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Const)),
        "DEFAULT_LIMIT const missing, got: {syms:#?}"
    );
}

#[test]
fn parse_fixture_tiny_python_surfaces_class_with_methods_and_decorated_fn() {
    let path = fixtures_dir().join("tiny_python.py");
    let syms = parse_file(&path).expect("parse tiny_python.py");

    // Function: add
    assert!(
        names_matching(&syms, "add")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Function)),
        "expected `add` Function, got: {syms:#?}"
    );
    // Class: Counter with methods nested.
    let counter = first_matching(&syms, "Counter")
        .filter(|s| matches!(s.kind, SymbolKind::Class))
        .expect("Counter class missing");
    let method_names: Vec<&str> = counter.children.iter().map(|c| c.name.as_str()).collect();
    assert!(
        method_names.contains(&"increment"),
        "Counter should expose `increment`, got: {method_names:#?}"
    );
    assert!(
        method_names.contains(&"_double"),
        "Counter should expose `_double`, got: {method_names:#?}"
    );
    // Decorated function: cached_add (the @lru_cache decorator must be
    // unwrapped so the inner `def` surfaces as Function).
    let cached = first_matching(&syms, "cached_add")
        .filter(|s| matches!(s.kind, SymbolKind::Function))
        .expect("cached_add (decorated def) missing");
    assert!(
        !cached.signature.is_empty(),
        "cached_add signature was empty"
    );
}

#[test]
fn parse_fixture_tiny_go_surfaces_function_struct_interface_method() {
    let path = fixtures_dir().join("tiny_go.go");
    let syms = parse_file(&path).expect("parse tiny_go.go");

    // Function: Add
    let add = first_matching(&syms, "Add")
        .filter(|s| matches!(s.kind, SymbolKind::Function))
        .expect("Add function missing");
    // Methods on Counter (Increment, Value) are top-level Functions in Go
    // because the receiver sits on the method_declaration, not inside a
    // type_declaration. The symbol name is just the method name.
    assert!(
        names_matching(&syms, "Increment")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Function)),
        "Increment method missing, got: {syms:#?}"
    );
    assert!(
        names_matching(&syms, "Value")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Function)),
        "Value method missing, got: {syms:#?}"
    );
    // Struct: Counter
    assert!(
        names_matching(&syms, "Counter")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Struct)),
        "Counter struct missing, got: {syms:#?}"
    );
    // Interface: Greeter
    assert!(
        names_matching(&syms, "Greeter")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Interface)),
        "Greeter interface missing, got: {syms:#?}"
    );
    // Generic struct: Pair (declared as `type Pair[T any] struct { ... }`,
    // so it surfaces as a Struct in our classifier — that's intentional,
    // a generic alias would surface as TypeAlias instead).
    assert!(
        names_matching(&syms, "Pair")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Struct)),
        "Pair struct missing, got: {syms:#?}"
    );
    // Const block: DefaultLimit
    assert!(
        names_matching(&syms, "DefaultLimit")
            .iter()
            .any(|s| matches!(s.kind, SymbolKind::Const)),
        "DefaultLimit const missing, got: {syms:#?}"
    );
    // Sanity: the headline signature mentions `Add`.
    assert!(
        add.signature.contains("Add"),
        "Add signature should mention its name, got: {}",
        add.signature
    );
}

#[test]
fn parse_file_rejects_unsupported_extension() {
    // .txt isn't supported; we should get a clean UnsupportedLanguage
    // error, not a panic.
    let tmp = std::env::temp_dir().join("shannon_repomap_unsupported.txt");
    std::fs::write(&tmp, "hello world\n").expect("write tmp");
    let err = parse_file(&tmp).expect_err("txt should be unsupported");
    let msg = format!("{err}");
    assert!(
        msg.contains("unsupported language"),
        "expected unsupported-language error, got: {msg}"
    );
}

#[test]
fn from_path_parses_single_non_rust_file() {
    let path = fixtures_dir().join("tiny_python.py");
    let repo = RepoMap::from_path(&path).expect("from_path(python)");
    // The single-file map should contain exactly one file entry.
    assert_eq!(repo.map.files.len(), 1, "expected one file in map");
    let (parsed_path, syms) = &repo.map.files[0];
    assert_eq!(parsed_path, &path);
    assert!(
        !syms.is_empty(),
        "expected non-empty symbol list for tiny_python.py, got: {syms:#?}"
    );
}

#[test]
fn from_path_rejects_unsupported_extension() {
    let path = std::env::temp_dir().join("shannon_repomap_md_test.md");
    std::fs::write(&path, "# heading\n").expect("write tmp");
    let err = RepoMap::from_path(&path).expect_err("md should be unsupported");
    let msg = format!("{err}");
    assert!(
        msg.contains("unsupported language"),
        "expected unsupported-language error, got: {msg}"
    );
}

#[test]
fn from_dir_walks_mixed_language_fixtures() {
    let dir = fixtures_dir();
    let repo = RepoMap::from_dir(&dir).expect("from_dir(fixtures)");
    let names: Vec<String> = repo
        .map
        .files
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    // Phase A fixtures (Rust).
    assert!(names.iter().any(|n| n == "tiny_struct.rs"));
    assert!(names.iter().any(|n| n == "mod_with_nested.rs"));
    // Phase B fixtures.
    assert!(names.iter().any(|n| n == "tiny_ts.ts"));
    assert!(names.iter().any(|n| n == "tiny_python.py"));
    assert!(names.iter().any(|n| n == "tiny_go.go"));

    // The markdown should mention every fixture by name and contain the
    // language-specific kind labels. This is the human-readability check
    // the Phase B scope explicitly asked for.
    let md = repo.to_system_prompt_markdown();
    assert!(md.contains("tiny_ts.ts"), "md missing tiny_ts.ts");
    assert!(md.contains("tiny_python.py"), "md missing tiny_python.py");
    assert!(md.contains("tiny_go.go"), "md missing tiny_go.go");
    // Kind labels should appear (one for each language).
    assert!(md.contains("**class**"), "md missing class label");
    assert!(md.contains("**interface**"), "md missing interface label");
    assert!(md.contains("**fn**"), "md missing fn label");
}

#[test]
fn markdown_for_python_class_is_human_readable() {
    // Snapshot-style check: the rendered markdown for a Python class
    // should put indented methods under their parent class. If the
    // extractor loses the parent/child structure the rendering collapses
    // to flat bullets and this test fails.
    let path = fixtures_dir().join("tiny_python.py");
    let repo = RepoMap::from_path(&path).expect("from_path");
    let md = repo.to_system_prompt_markdown();

    // Header
    assert!(md.starts_with("# Repo Map:"));
    // Class declaration
    assert!(md.contains("**class** `class Counter:`"), "md: {md}");
    // Methods appear nested (indented) under the class.
    // Look for the indented `  - **fn**` pattern.
    let indented_fn = md
        .lines()
        .find(|l| l.starts_with("  - **fn**") && l.contains("increment"))
        .unwrap_or_else(|| panic!("expected indented `increment` fn under class, md:\n{md}"));
    assert!(indented_fn.contains("increment"));
    // Top-level `add` function should NOT be indented (it's a module-level def).
    let top_level_fn = md
        .lines()
        .find(|l| l.starts_with("- **fn**") && l.contains("add"))
        .unwrap_or_else(|| panic!("expected top-level `add` fn, md:\n{md}"));
    assert!(top_level_fn.contains("add"));
}

#[test]
fn budget_trim_handles_non_rust_symbols() {
    // Build a synthetic mixed-kind map, then trim it. The budget slicer
    // doesn't care which language produced the symbols; it just walks
    // signatures recursively.
    let mut files = Vec::new();
    for kind_idx in 0..3 {
        let mut syms = Vec::new();
        for i in 0..5 {
            let kind = match kind_idx {
                0 => SymbolKind::Function,
                1 => SymbolKind::Class,
                _ => SymbolKind::Interface,
            };
            syms.push(shannon_repomap::SymbolNode {
                kind,
                name: format!("sym_{kind_idx}_{i}"),
                span: shannon_repomap::Span {
                    start_line: 0,
                    start_col: 0,
                    end_line: 0,
                    end_col: 0,
                },
                signature: "x".repeat(400),
                children: vec![],
            });
        }
        files.push((
            PathBuf::from(format!("/synthetic/file_{kind_idx}.ts")),
            syms,
        ));
    }
    let mut map = SymbolMap {
        root: PathBuf::from("/synthetic"),
        files,
    };
    let before = shannon_repomap::budget::total_tokens(&map);
    assert!(before > 500, "map should exceed budget before trim");
    trim_to_budget(&mut map, 500);
    let after = shannon_repomap::budget::total_tokens(&map);
    assert!(after <= 500, "post-trim tokens {after} exceeds budget 500");
}
