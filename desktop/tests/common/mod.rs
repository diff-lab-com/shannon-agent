//! Shared helpers for the desktop ACL contract tests (review §P2-21).
//!
//! Included via `#[allow(dead_code)] mod common;` from each test binary so
//! unused helpers in one binary don't trip `-D warnings`.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Absolute path of the `desktop/` package directory, valid at test runtime
/// regardless of the cargo invocation's working directory.
pub fn desktop_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Extracts the command inventory from `generate_handler![...]` in
/// `src/main.rs` (simple text scan, per the review §P2-21 design).
///
/// Every non-comment line inside the macro brackets must be a
/// `module::command` path whose last segment is a snake_case command name.
/// Any deviation is a hard assertion failure, so a formatting change that
/// would blind the parser fails loudly instead of silently shrinking the
/// inventory. The `total >= 200` sanity bound makes a parsing regression
/// visible even before cross-checking against the ACL files.
pub fn handler_inventory() -> BTreeSet<String> {
    let src = std::fs::read_to_string(desktop_dir().join("src").join("main.rs"))
        .expect("read desktop/src/main.rs");
    let macro_start = src
        .find("generate_handler![")
        .expect("generate_handler![ not found in src/main.rs");
    let rest = &src[macro_start..];
    let open = rest.find('[').expect("opening [ of generate_handler!");
    let close = rest
        .find("\n        ])")
        .expect("closing ]) of generate_handler! (main.rs formatting drifted?)");
    let body = &rest[open + 1..close];

    let mut commands = BTreeSet::new();
    let mut total = 0usize;
    for line in body.lines() {
        let line = line.trim();
        // strip full-line or trailing `//` comments
        let code = match line.split_once("//") {
            Some((code, _)) => code.trim(),
            None => line,
        };
        if code.is_empty() {
            continue;
        }
        total += 1;
        let entry = code.strip_suffix(',').unwrap_or(code);
        assert!(
            entry.contains("::"),
            "unexpected generate_handler entry `{entry}`: parser expects `module::command` paths"
        );
        let cmd = entry.rsplit("::").next().expect("non-empty entry");
        assert!(
            !cmd.is_empty()
                && cmd
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "command `{cmd}` is not lowercase snake_case ASCII; the ACL naming contract \
             (allow-<kebab-of-command>) would be ambiguous"
        );
        assert!(
            commands.insert(cmd.to_string()),
            "duplicate command `{cmd}` in generate_handler!"
        );
    }
    assert!(
        total >= 200,
        "generate_handler inventory implausibly small ({total} entries): the text parser \
         or src/main.rs regressed — do not weaken the ACL before fixing this"
    );
    commands
}
