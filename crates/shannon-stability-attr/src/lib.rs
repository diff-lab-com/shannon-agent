//! `#[stable_api]` and `#[unstable_api]` attribute macros for the Shannon workspace.
//!
//! These markers make the stability tier of a public API explicit in the
//! source. They compile to `#[doc = "..."]` attributes so the promise is
//! visible in rustdoc, and can be grepped to regenerate
//! `docs/STABILITY.md`'s stable surface list.
//!
//! The names `stable_api` / `unstable_api` are used because Rust reserves
//! `#[stable]` / `#[unstable]` for the standard library (E0734).
//!
//! See `docs/STABILITY.md` for the full policy.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Expr, Item, Lit, Meta, parse_macro_input};

#[proc_macro_attribute]
pub fn stable_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let since = parse_since_attribute(attr);
    let parsed = parse_macro_input!(item as Item);
    let doc_line = format!(
        "\n\n**Stability: stable** — guaranteed under cargo semver until the next major bump. Tagged in `docs/STABILITY.md` (since {since})."
    );
    let expanded = quote! {
        #[doc = #doc_line]
        #parsed
    };
    expanded.into()
}

#[proc_macro_attribute]
pub fn unstable_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let note = parse_since_attribute(attr);
    let parsed = parse_macro_input!(item as Item);
    let doc_line = format!(
        "\n\n**Stability: unstable** — may break in any minor bump ({note}). Pin the exact workspace version if you depend on it outside the Shannon workspace."
    );
    let expanded = quote! {
        #[doc = #doc_line]
        #parsed
    };
    expanded.into()
}

fn parse_since_attribute(attr: TokenStream) -> String {
    if attr.is_empty() {
        return "unspecified".to_string();
    }
    let tokens: TokenStream = attr;
    let parsed: Meta = match syn::parse(tokens) {
        Ok(m) => m,
        Err(_) => return "unspecified".to_string(),
    };
    match parsed {
        Meta::NameValue(nv) if nv.path.is_ident("since") => {
            if let Expr::Lit(expr_lit) = nv.value {
                if let Lit::Str(s) = expr_lit.lit {
                    return s.value();
                }
            }
            "unspecified".to_string()
        }
        _ => "unspecified".to_string(),
    }
}
