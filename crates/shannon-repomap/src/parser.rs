//! Tree-sitter driven parser for `.rs` files.
//!
//! Walks the syntax tree, surfaces the declarations that are useful to a
//! developer looking at a repo map (functions, types, traits, impls, modules,
//! type aliases, consts) and drops everything else (expressions, statements,
//! attributes, etc.) on the floor.

use crate::symbol_tree::{Span, SymbolKind, SymbolNode};
use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Node, Parser};

/// Parse a single Rust file and return its top-level symbol list.
///
/// Returns an error if the file can't be read, the grammar fails to load, or
/// tree-sitter returns `None` for the parse.
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
    extract_symbols(root, &source, &mut out);
    Ok(out)
}

/// Walk a tree-sitter subtree, collecting top-level symbols.
///
/// `ancestor_kinds` tracks the kind chain from the file root down to (but not
/// including) the current node. We use it to decide whether a `function_item`
/// we encounter is a free function or a method inside an `impl`/`trait` body.
fn extract_symbols(node: Node, source: &str, out: &mut Vec<SymbolNode>) {
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
                        extract_symbols(body, source, &mut sym.children);
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
                        extract_symbols(body, source, &mut sym.children);
                    }
                    out.push(sym);
                }
            }
            // Recurse into `source_file` and other container nodes so
            // declarations nested inside modules or impl bodies surface
            // through the recursion above. Items we don't classify (e.g.
            // `use`, `extern`, macros) are simply skipped.
            _ => {
                if child.is_named() {
                    let mut scratch = Vec::new();
                    extract_symbols(child, source, &mut scratch);
                    out.extend(scratch);
                }
            }
        }
    }
}

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
/// type/const). The caller handles recursion for `impl_item` and `mod_item`.
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
/// tree-sitter-rust exposes the name as a `field: "name"` on most items. As a
/// fallback we scan direct children for the first `identifier` node, which
/// handles a few cases where the field name differs (e.g. tuple-struct fields
/// of interest are not the struct name itself).
fn find_child_identifier(node: Node, source: &str) -> Option<String> {
    if let Some(named) = node.child_by_field_name("name") {
        return Some(slice_text(named, source));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "type_identifier" {
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
