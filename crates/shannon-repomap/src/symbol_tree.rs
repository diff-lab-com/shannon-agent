//! Symbol tree types for the repo map.
//!
//! A `SymbolNode` is the unit emitted by the parser. The tree is recursive:
//! a `Module` contains `Function`/`Struct`/`Enum`/etc. children, and an `Impl`
//! contains the methods it declares.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Source span (zero-indexed lines/columns) for a symbol declaration.
///
/// Coordinates follow tree-sitter's convention: `start_line` is the line where
/// the declaration begins, `end_line` is the last line of the declaration
/// (closing brace included for items that have one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// Coarse classification of the declarations we surface to the LLM.
///
/// Tree-sitter Rust has many more node kinds (e.g., `function_signature_item`,
/// `macro_invocation`, `static_item`). We collapse them into the categories
/// that map cleanly to "things a developer would point at in a symbol map".
///
/// `Class` and `Interface` are Phase B additions for TS / Python / Go; they
/// fit cleanly into the same `SymbolNode` shape (one declaration, optional
/// children for nested methods), so the budget / markdown layers don't need
/// to special-case them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    TypeAlias,
    Const,
    /// TS `class_declaration` / `abstract_class_declaration`,
    /// Python `class_definition`, Go `type_declaration` with `struct_type`.
    Class,
    /// TS `interface_declaration`,
    /// Go `type_declaration` with `interface_type`.
    Interface,
}

impl SymbolKind {
    /// Short label used in markdown rendering.
    pub fn label(&self) -> &'static str {
        match self {
            SymbolKind::Function => "fn",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::Module => "mod",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Const => "const",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
        }
    }
}

/// A single symbol declaration in a Rust source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolNode {
    pub kind: SymbolKind,
    pub name: String,
    pub span: Span,
    /// First line of the declaration (e.g. `pub fn add(a: i32, b: i32) -> i32`),
    /// or a compact summary for items without a single-line signature.
    pub signature: String,
    /// Nested symbols. For example, methods inside an `impl`, items inside a
    /// nested `mod`, or variants inside an `enum` body.
    pub children: Vec<SymbolNode>,
}

impl SymbolNode {
    /// Approximate character count of this node's signature plus all
    /// descendants' signatures. Used by the budget slicer.
    pub fn chars_recursive(&self) -> usize {
        self.signature.len()
            + self
                .children
                .iter()
                .map(SymbolNode::chars_recursive)
                .sum::<usize>()
    }
}

/// A full symbol map for a workspace tree.
///
/// `root` is the directory that was walked; `files` are all discovered `.rs`
/// files paired with their top-level symbol list (children included).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMap {
    pub root: PathBuf,
    pub files: Vec<(PathBuf, Vec<SymbolNode>)>,
}

impl SymbolMap {
    /// Total characters across every signature in every file (recursive).
    pub fn total_chars(&self) -> usize {
        self.files
            .iter()
            .flat_map(|(_, syms)| syms.iter())
            .map(SymbolNode::chars_recursive)
            .sum()
    }
}
