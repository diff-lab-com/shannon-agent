//! Token-budget aware trimming of the symbol map.
//!
//! The estimator here is intentionally rough: `chars / 4`. That's the same
//! shorthand OpenAI/Anthropic docs use when they talk about a "token is
//! roughly 4 characters" and it's plenty accurate for symbol-map line items,
//! which are short ASCII strings. If we ever need to render prose bodies, swap
//! in a real BPE counter — but for signatures and labels, chars/4 is fine.

use crate::symbol_tree::{SymbolMap, SymbolNode};

/// Rough token estimator. Uses `div_ceil` so a 1-char string still counts as
/// 1 token, matching the typical "≥1 token per non-empty fragment" floor.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Compute the token estimate for the entire map (signatures, recursive).
pub fn total_tokens(map: &SymbolMap) -> usize {
    estimate_tokens_recursive(&map.files)
}

/// Greedy trim: walk every file and shrink its symbol tree until the total
/// estimated tokens fit within `token_budget`. The shrink strategy is, in
/// order:
///
/// 1. Drop all `children` from every node (the in-memory cost is signatures,
///    not bodies).
/// 2. If still too big, drop whole top-level symbols starting from the bottom
///    of the file (least-important decls first, since file order is source
///    order and the top of the file usually contains the entry points).
///
/// If even step 2 can't fit the budget, the function still returns a (possibly
/// empty) trimmed map; the caller is responsible for noting that the budget
/// was unattainable.
pub fn trim_to_budget(map: &mut SymbolMap, token_budget: usize) {
    if token_budget == 0 {
        // Pathological caller; produce an empty map rather than spinning.
        for (_, syms) in &mut map.files {
            syms.clear();
        }
        return;
    }

    // Step 1: drop children across the board.
    for (_, syms) in &mut map.files {
        for sym in syms.iter_mut() {
            drop_children_deep(sym);
        }
    }

    if total_tokens(map) <= token_budget {
        return;
    }

    // Step 2: drop whole top-level symbols from the bottom of each file until
    // we fit. We do per-file trimming in reverse source order so each file's
    // entry-point symbols (functions/types near the top) survive.
    //
    // We snapshot the file count up front so we can release the mutable
    // borrow on `map.files` between pop iterations and let `total_tokens`
    // re-scan immutably each time. This keeps the borrow checker happy and
    // keeps the loop body simple.
    let file_count = map.files.len();
    for i in 0..file_count {
        loop {
            let needs_trim = {
                let syms = &map.files[i].1;
                if syms.is_empty() {
                    break;
                }
                total_tokens(map) > token_budget
            };
            if !needs_trim {
                break;
            }
            map.files[i].1.pop();
        }
        if total_tokens(map) <= token_budget {
            break;
        }
    }
}

/// Recursively empty `children` to free signature-only budget.
fn drop_children_deep(node: &mut SymbolNode) {
    for child in node.children.iter_mut() {
        drop_children_deep(child);
    }
    node.children.clear();
}

fn estimate_tokens_recursive(files: &[(std::path::PathBuf, Vec<SymbolNode>)]) -> usize {
    files
        .iter()
        .flat_map(|(_, syms)| syms.iter())
        .map(node_tokens)
        .sum()
}

fn node_tokens(node: &SymbolNode) -> usize {
    estimate_tokens(&node.signature) + node.children.iter().map(node_tokens).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_rounds_up_short_strings() {
        // 1 char → 1 token (div_ceil floors at 1 for non-empty text).
        assert_eq!(estimate_tokens("x"), 1);
        // 4 chars → 1 token.
        assert_eq!(estimate_tokens("abcd"), 1);
        // 5 chars → 2 tokens.
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn trim_clears_children_first_then_drops_whole_symbols() {
        let mut map = SymbolMap {
            root: std::path::PathBuf::from("/"),
            files: vec![(
                std::path::PathBuf::from("/x.rs"),
                vec![
                    SymbolNode {
                        kind: crate::symbol_tree::SymbolKind::Function,
                        name: "outer".into(),
                        span: crate::symbol_tree::Span {
                            start_line: 0,
                            start_col: 0,
                            end_line: 0,
                            end_col: 0,
                        },
                        // 120 chars → 30 tokens of signature; keep this one.
                        signature: "a".repeat(120),
                        children: vec![SymbolNode {
                            kind: crate::symbol_tree::SymbolKind::Function,
                            name: "inner".into(),
                            span: crate::symbol_tree::Span {
                                start_line: 0,
                                start_col: 0,
                                end_line: 0,
                                end_col: 0,
                            },
                            // 400 chars → 100 tokens of child; should be dropped.
                            signature: "b".repeat(400),
                            children: vec![],
                        }],
                    },
                    SymbolNode {
                        kind: crate::symbol_tree::SymbolKind::Function,
                        name: "tail".into(),
                        span: crate::symbol_tree::Span {
                            start_line: 0,
                            start_col: 0,
                            end_line: 0,
                            end_col: 0,
                        },
                        // 400 chars → 100 tokens; should be dropped.
                        signature: "c".repeat(400),
                        children: vec![],
                    },
                ],
            )],
        };

        trim_to_budget(&mut map, 50);

        let file = &map.files[0].1;
        // "outer" stays; "tail" got popped; the inner child of "outer" was
        // dropped in step 1.
        assert_eq!(file.len(), 1);
        assert_eq!(file[0].name, "outer");
        assert!(file[0].children.is_empty());
    }
}
