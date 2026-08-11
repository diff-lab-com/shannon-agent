//! Tree-sitter driven parser for `.rs`, `.ts`, `.tsx`, `.py`, and `.go` files.
//!
//! Each language gets its own [`LanguageParser`] variant that owns the right
//! `tree_sitter::Language` and a top-level walker. The shared dispatch entry
//! point is [`parse_file`], which detects the language from the file extension
//! and routes accordingly. The Rust branch is the original Phase A logic; TS,
//! Python, and Go are new in Phase B and follow the same conservative
//! "function / class / type" surface area.
//!
//! Symbol extraction per language is conservative: we surface the headline
//! declarations a developer would point at in a repo map (functions, classes,
//! types) and drop everything else (expressions, statements, imports, etc.) on
//! the floor.

use crate::symbol_tree::{Span, SymbolKind, SymbolNode};
use anyhow::{Context, Result};
use std::path::Path;
use thiserror::Error;
use tree_sitter::{Language, Node, Parser};

/// Errors specific to the repo-map public surface.
///
/// `parse_file` (the new Phase B entry) returns this directly so callers can
/// distinguish "unknown extension" from "the file is broken Rust". The
/// existing `parse_rust_file` keeps its `anyhow::Result` signature for
/// backwards compatibility with Phase A call sites.
#[derive(Debug, Error)]
pub enum RepoMapError {
    #[error("unsupported language for file: {0} (supported: .rs .ts .tsx .py .go)")]
    UnsupportedLanguage(String),
    #[error("io error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("tree-sitter failed to load grammar for {language}")]
    GrammarLoad { language: &'static str },
    #[error("tree-sitter parse returned None for {path}")]
    Parse { path: String },
}

/// A language-specific tree-sitter binding paired with an extractor.
///
/// `Rust` is the original Phase A path. `TypeScript` / `Python` / `Go` are
/// new in Phase B and produce the same `SymbolNode` shape so the budget and
/// markdown layers don't need to care which language the symbols came from.
#[derive(Debug, Clone, Copy)]
pub enum LanguageParser {
    Rust,
    TypeScript,
    Python,
    Go,
}

impl LanguageParser {
    /// Resolve a `LanguageParser` from a file extension.
    ///
    /// Returns `Err(RepoMapError::UnsupportedLanguage)` for unknown
    /// extensions — this is what the public `RepoMap::from_path` will hand
    /// back to the caller.
    pub fn from_extension(ext: &str) -> Result<Self, RepoMapError> {
        match ext {
            "rs" => Ok(LanguageParser::Rust),
            "ts" | "tsx" | "mts" | "cts" | "jsx" => Ok(LanguageParser::TypeScript),
            "py" | "pyi" => Ok(LanguageParser::Python),
            "go" => Ok(LanguageParser::Go),
            other => Err(RepoMapError::UnsupportedLanguage(other.to_string())),
        }
    }

    /// Resolve a `LanguageParser` from a full path by looking at the
    /// extension. Returns the same error as [`Self::from_extension`].
    pub fn from_path(path: &Path) -> Result<Self, RepoMapError> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        Self::from_extension(ext)
    }

    /// Short, lower-case label used in error messages and (optionally) the
    /// markdown header line.
    pub fn label(&self) -> &'static str {
        match self {
            LanguageParser::Rust => "rust",
            LanguageParser::TypeScript => "typescript",
            LanguageParser::Python => "python",
            LanguageParser::Go => "go",
        }
    }

    /// The tree-sitter `Language` for this variant.
    ///
    /// All four grammar crates expose a `LanguageFn` that gets wrapped in
    /// `tree_sitter::Language` via `.into()` at the call site (matches the
    /// existing Phase A pattern for `tree_sitter_rust`).
    pub fn language(&self) -> Language {
        match self {
            LanguageParser::Rust => tree_sitter_rust::LANGUAGE.into(),
            // `.tsx` is JSX-flavored; for Phase B we route both `.ts` and
            // `.tsx` through the typescript grammar. The plan §Phase B
            // acknowledges this as an acceptable simplification for the
            // symbol-map use case — JSX-aware extraction is a follow-up.
            LanguageParser::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            LanguageParser::Python => tree_sitter_python::LANGUAGE.into(),
            LanguageParser::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    /// Dispatch to the right language-specific symbol extractor.
    fn extract(&self, root: Node, source: &str, out: &mut Vec<SymbolNode>) {
        match self {
            LanguageParser::Rust => extract_rust_symbols(root, source, out),
            LanguageParser::TypeScript => extract_typescript_symbols(root, source, out),
            LanguageParser::Python => extract_python_symbols(root, source, out),
            LanguageParser::Go => extract_go_symbols(root, source, out),
        }
    }
}

/// Parse a single Rust file and return its top-level symbol list.
///
/// Kept verbatim from Phase A so existing callers don't need to migrate. New
/// code should use [`parse_file`] for language-agnostic dispatch.
pub fn parse_rust_file(path: &Path) -> Result<Vec<SymbolNode>> {
    let source =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;

    let mut parser = Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    parser
        .set_language(&language.into())
        .context("load tree-sitter-rust grammar")?;

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter parse returned None"))?;

    let root = tree.root_node();
    let mut out = Vec::new();
    extract_rust_symbols(root, &source, &mut out);
    Ok(out)
}

/// Phase B entry point: parse any supported file, dispatching on extension.
///
/// Returns `Err(RepoMapError::UnsupportedLanguage)` for files whose extension
/// we don't recognize; other errors wrap the underlying io/grammar issue.
pub fn parse_file(path: &Path) -> Result<Vec<SymbolNode>, RepoMapError> {
    let language = LanguageParser::from_path(path)?;
    let source = std::fs::read_to_string(path).map_err(|e| RepoMapError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let mut parser = Parser::new();
    parser
        .set_language(&language.language())
        .map_err(|_| RepoMapError::GrammarLoad {
            language: language.label(),
        })?;

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| RepoMapError::Parse {
            path: path.display().to_string(),
        })?;

    let mut out = Vec::new();
    language.extract(tree.root_node(), &source, &mut out);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Rust — original Phase A extraction. Doc comments inline.
// ---------------------------------------------------------------------------

/// Walk a tree-sitter subtree, collecting top-level symbols.
fn extract_rust_symbols(node: Node, source: &str, out: &mut Vec<SymbolNode>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(sym) = build_function(child, source) {
                    out.push(sym);
                }
            }
            "struct_item" => {
                if let Some(sym) = build_type(child, source, SymbolKind::Struct) {
                    out.push(sym);
                }
            }
            "enum_item" => {
                if let Some(sym) = build_type(child, source, SymbolKind::Enum) {
                    out.push(sym);
                }
            }
            "trait_item" => {
                if let Some(sym) = build_type(child, source, SymbolKind::Trait) {
                    out.push(sym);
                }
            }
            "impl_item" => {
                if let Some(mut sym) = build_type(child, source, SymbolKind::Impl) {
                    if let Some(body) = child.child_by_field_name("body") {
                        extract_rust_symbols(body, source, &mut sym.children);
                    }
                    out.push(sym);
                }
            }
            "type_item" => {
                if let Some(sym) = build_type(child, source, SymbolKind::TypeAlias) {
                    out.push(sym);
                }
            }
            "const_item" | "static_item" => {
                if let Some(sym) = build_type(child, source, SymbolKind::Const) {
                    out.push(sym);
                }
            }
            "mod_item" => {
                if let Some(mut sym) = build_type(child, source, SymbolKind::Module) {
                    if let Some(body) = child.child_by_field_name("body") {
                        extract_rust_symbols(body, source, &mut sym.children);
                    }
                    out.push(sym);
                }
            }
            _ => {
                if child.is_named() {
                    let mut scratch = Vec::new();
                    extract_rust_symbols(child, source, &mut scratch);
                    out.extend(scratch);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TypeScript / TSX — Phase B extraction.
//
// We map:
//   function_declaration            → Function
//   generator_function_declaration  → Function
//   class_declaration               → Class (with method_definition children)
//   interface_declaration           → Interface
//   type_alias_declaration          → TypeAlias
//   lexical_declaration (export const/let/var at top level) → Const
//
// We deliberately stop short of `enum_declaration` (TS enums are noisy and
// not a common repo-map signal). The plan §Phase B §symbol-extraction
// explicitly lists the conservative set above.
// ---------------------------------------------------------------------------

fn extract_typescript_symbols(node: Node, source: &str, out: &mut Vec<SymbolNode>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(sym) = build_function(child, source) {
                    out.push(sym);
                }
            }
            "method_definition" => {
                // `method foo()` / `public foo()` inside a class body.
                // tree-sitter-typescript uses `method_definition` for both
                // methods and constructors; we surface them all as Function.
                if let Some(sym) = build_function(child, source) {
                    out.push(sym);
                }
            }
            "class_declaration" => {
                if let Some(mut sym) = build_type(child, source, SymbolKind::Class) {
                    if let Some(body) = child.child_by_field_name("body") {
                        extract_typescript_symbols(body, source, &mut sym.children);
                    }
                    out.push(sym);
                }
            }
            "interface_declaration" => {
                if let Some(sym) = build_type(child, source, SymbolKind::Interface) {
                    out.push(sym);
                }
            }
            "type_alias_declaration" => {
                if let Some(sym) = build_type(child, source, SymbolKind::TypeAlias) {
                    out.push(sym);
                }
            }
            "lexical_declaration" => {
                // `export const FOO = ...;` / `let bar = ...;` at module scope.
                if let Some(sym) = build_lexical(child, source) {
                    out.push(sym);
                }
            }
            "abstract_class_declaration" => {
                if let Some(mut sym) = build_type(child, source, SymbolKind::Class) {
                    if let Some(body) = child.child_by_field_name("body") {
                        extract_typescript_symbols(body, source, &mut sym.children);
                    }
                    out.push(sym);
                }
            }
            // `class_body` and other container nodes are recursed into via
            // the fallthrough case so that nested method/public_field
            // definitions are picked up.
            _ => {
                if child.is_named() {
                    let mut scratch = Vec::new();
                    extract_typescript_symbols(child, source, &mut scratch);
                    out.extend(scratch);
                }
            }
        }
    }
}

/// Build a `SymbolNode` for a `lexical_declaration` (TS top-level const/let).
///
/// The declaration is `const FOO = ...;` or `let bar;`. We surface the
/// declaration's first source line, which is plenty for a repo map.
fn build_lexical(node: Node, source: &str) -> Option<SymbolNode> {
    // The pattern is `lexical_declaration` containing a `variable_declarator`
    // whose `name` field is the identifier. If we can't find that, fall back
    // to the first source line so the symbol still surfaces.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = slice_text(name_node, source);
                if !name.is_empty() {
                    return Some(SymbolNode {
                        kind: SymbolKind::Const,
                        name,
                        span: span_of(node),
                        signature: first_source_line(node, source),
                        children: Vec::new(),
                    });
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Python — Phase B extraction.
//
// We map:
//   function_definition     → Function
//   class_definition        → Class (with nested function_definition children)
//   decorated_definition    → unwrap; extract the inner def
//
// Python is indentation-sensitive but tree-sitter handles that for us; the
// block-as-statement pattern means `class C: def m(self): ...` is naturally
// nested once we recurse into the class body.
// ---------------------------------------------------------------------------

fn extract_python_symbols(node: Node, source: &str, out: &mut Vec<SymbolNode>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(sym) = build_function(child, source) {
                    out.push(sym);
                }
            }
            "class_definition" => {
                if let Some(mut sym) = build_type(child, source, SymbolKind::Class) {
                    if let Some(body) = child.child_by_field_name("body") {
                        extract_python_symbols(body, source, &mut sym.children);
                    }
                    out.push(sym);
                }
            }
            "decorated_definition" => {
                // `@decorator\ndef foo(): ...` — the actual def is one of
                // the children. Recurse so the unwrapped node gets matched.
                let mut scratch = Vec::new();
                extract_python_symbols(child, source, &mut scratch);
                out.extend(scratch);
            }
            _ => {
                if child.is_named() {
                    let mut scratch = Vec::new();
                    extract_python_symbols(child, source, &mut scratch);
                    out.extend(scratch);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Go — Phase B extraction.
//
// We map:
//   function_declaration        → Function
//   method_declaration          → Function
//   type_declaration (struct)   → Struct
//   type_declaration (interface)→ Interface
//   type_declaration (other)    → TypeAlias
//   const_declaration / var_declaration → Const
//
// Go's `type_declaration` covers structs, interfaces, and named-type
// aliases. We sniff the inner kind via `child_by_field_name("type")` to
// classify correctly.
// ---------------------------------------------------------------------------

fn extract_go_symbols(node: Node, source: &str, out: &mut Vec<SymbolNode>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "method_declaration" => {
                if let Some(sym) = build_function(child, source) {
                    out.push(sym);
                }
            }
            "type_declaration" => {
                if let Some(sym) = build_go_type(child, source) {
                    out.push(sym);
                }
            }
            "const_declaration" | "var_declaration" => {
                if let Some(sym) = build_go_const_block(child, source) {
                    out.push(sym);
                }
            }
            _ => {
                if child.is_named() {
                    let mut scratch = Vec::new();
                    extract_go_symbols(child, source, &mut scratch);
                    out.extend(scratch);
                }
            }
        }
    }
}

/// Classify a Go `type_declaration` (struct / interface / other alias).
fn build_go_type(node: Node, source: &str) -> Option<SymbolNode> {
    // `type_declaration` wraps a `type_spec` (or several in a parenthesised
    // block). We surface the first spec — most Go files use one type per
    // declaration. Compound blocks fall back to a generic TypeAlias.
    let mut cursor = node.walk();
    let mut first_name: Option<String> = None;
    let mut first_kind: Option<SymbolKind> = None;
    for child in node.children(&mut cursor) {
        if child.kind() == "type_spec" || child.kind() == "type_alias" {
            if let Some(name_node) = child.child_by_field_name("name") {
                if first_name.is_none() {
                    first_name = Some(slice_text(name_node, source));
                }
            }
            if first_kind.is_none() {
                if let Some(t) = child.child_by_field_name("type") {
                    first_kind = Some(match t.kind() {
                        "struct_type" => SymbolKind::Struct,
                        "interface_type" => SymbolKind::Interface,
                        _ => SymbolKind::TypeAlias,
                    });
                }
            }
        }
    }
    let name = first_name?;
    let kind = first_kind.unwrap_or(SymbolKind::TypeAlias);
    Some(SymbolNode {
        kind,
        name,
        span: span_of(node),
        signature: first_source_line(node, source),
        children: Vec::new(),
    })
}

/// Surface a Go `const`/`var` block as a single `Const` symbol using the
/// first declarator's name. The block-level summary is plenty for a repo
/// map; we don't currently split multi-symbol blocks.
fn build_go_const_block(node: Node, source: &str) -> Option<SymbolNode> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "const_spec" || child.kind() == "var_spec" {
            if let Some(name_node) = child.child_by_field_name("name") {
                let name = slice_text(name_node, source);
                if !name.is_empty() {
                    return Some(SymbolNode {
                        kind: SymbolKind::Const,
                        name,
                        span: span_of(node),
                        signature: first_source_line(node, source),
                        children: Vec::new(),
                    });
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Shared tree-sitter → SymbolNode helpers. Mirrors the Phase A originals
// so the per-language extractors can share them without re-implementing.
// ---------------------------------------------------------------------------

/// Build a `SymbolNode` for a function item.
fn build_function(node: Node, source: &str) -> Option<SymbolNode> {
    let name = find_child_identifier(node, source)?;
    let span = span_of(node);
    let signature = first_source_line(node, source);
    Some(SymbolNode {
        kind: SymbolKind::Function,
        name,
        span,
        signature,
        children: Vec::new(),
    })
}

/// Build a `SymbolNode` for a non-recursive item (struct/enum/trait/impl/mod/
/// type/const). The caller handles recursion for container items.
fn build_type(node: Node, source: &str, kind: SymbolKind) -> Option<SymbolNode> {
    let name = find_child_identifier(node, source)?;
    let span = span_of(node);
    let signature = first_source_line(node, source);
    Some(SymbolNode {
        kind,
        name,
        span,
        signature,
        children: Vec::new(),
    })
}

/// Locate the identifier child that names this declaration.
///
/// Most grammar nodes expose the name via a `"name"` field. As a fallback we
/// scan direct children for an identifier-shaped node, which handles edge
/// cases (tuple-struct fields, `class C(Base): ...` in Python, etc.).
fn find_child_identifier(node: Node, source: &str) -> Option<String> {
    if let Some(named) = node.child_by_field_name("name") {
        return Some(slice_text(named, source));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier"
            || child.kind() == "type_identifier"
            || child.kind() == "property_identifier"
        {
            return Some(slice_text(child, source));
        }
    }
    None
}

/// Text of a tree-sitter node, derived from the source buffer.
///
/// `node.utf8_text(source)` would do the same thing but it does UTF-8
/// validation on every call; we trust the source came from `read_to_string`
/// and slice the bytes directly, which is what tree-sitter would do
/// internally anyway.
fn slice_text(child: Node, source: &str) -> String {
    let start = child.byte_range().start;
    let end = child.byte_range().end;
    if end <= source.len() {
        source[start..end].to_string()
    } else {
        String::new()
    }
}

/// Convert a tree-sitter `Node` into our `Span`.
fn span_of(node: Node) -> Span {
    let r = node.range();
    Span {
        start_line: r.start_point.row as u32,
        start_col: r.start_point.column as u32,
        end_line: r.end_point.row as u32,
        end_col: r.end_point.column as u32,
    }
}

/// First source line of the declaration, trimmed.
///
/// We pick the line from the start row. For items that span many lines
/// (multi-line struct literals, big enums) this captures the headline and
/// drops the body, which is what the budget slicer wants.
fn first_source_line(node: Node, source: &str) -> String {
    let start = node.range().start_point.row;
    let mut line = source.lines().nth(start).unwrap_or("").to_string();
    if line.len() > 160 {
        line.truncate(160);
        line.push('…');
    }
    line.trim().to_string()
}
