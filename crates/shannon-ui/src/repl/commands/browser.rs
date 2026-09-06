//! `/browser` — one-command setup for MCP-based browser automation.
//!
//! Shannon has no native browser engine; browser control follows the same
//! MCP pattern as Claude Code (Playwright MCP, chrome-devtools-mcp). Once
//! the server is configured, the pipeline is wired end-to-end: the
//! `mcp__playwright__browser_*` tools register at startup and the engine
//! injects the browser-control workflow prompt (see
//! `shannon_core::query_engine::browser_control_prompt`).
//!
//! Subcommands:
//! - `/browser setup` — merge the Playwright MCP server into the project
//!   `.mcp.json` (idempotent; preserves unrelated servers)
//! - `/browser status` — report whether browser automation is configured

use std::path::Path;

use super::Repl;
use crate::{Result, widgets::ChatRole};

/// Server key written into `.mcp.json`'s `mcpServers` object.
const PLAYWRIGHT_SERVER: &str = "playwright";
/// Launch command for the Playwright MCP server.
const PLAYWRIGHT_COMMAND: &str = "npx";
/// Playwright MCP package (official `@playwright/mcp`; tool surface matches
/// the injected browser-control prompt: browser_navigate/snapshot/click/...).
const PLAYWRIGHT_ARGS: &[&str] = &["@playwright/mcp@latest"];

pub(crate) fn handle_browser(repl: &mut Repl, args: &str) -> Result<()> {
    match args.trim() {
        "setup" => handle_setup(repl),
        "uninstall" | "remove" | "rm" => handle_uninstall(repl),
        "" | "status" => handle_status(repl),
        other => {
            repl.chat.add_message(
                ChatRole::System,
                format!("Unknown /browser subcommand: {other}\n\nUsage: /browser [setup|status|uninstall]"),
            );
            Ok(())
        }
    }
}

fn handle_status(repl: &mut Repl) -> Result<()> {
    let config_path = Path::new(&repl.state.working_directory).join(".mcp.json");
    let configured = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| {
            v.get("mcpServers")
                .and_then(|s| s.get(PLAYWRIGHT_SERVER))
                .cloned()
        });

    let msg = match configured {
        Some(entry) => format!(
            "✓ Browser automation is configured ({PLAYWRIGHT_SERVER} server in {})\n\nServer entry: {entry}\n\nBrowser tools (mcp__playwright__browser_*) register at startup; the browser-control workflow prompt is injected automatically.",
            config_path.display()
        ),
        None => format!(
            "Browser automation is not configured in {}.\n\nRun `/browser setup` to add the Playwright MCP server (requires npx). After restarting Shannon you get browser_navigate / browser_snapshot / browser_click / browser_take_screenshot tools.",
            config_path.display()
        ),
    };
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

/// Remove the Playwright MCP server entry from the project `.mcp.json`.
/// Preserves every other server. Idempotent (no-op if already absent).
fn handle_uninstall(repl: &mut Repl) -> Result<()> {
    let config_path = Path::new(&repl.state.working_directory).join(".mcp.json");
    if config_path.is_symlink() {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Refusing to write through symlink {} for security. Remove it and re-run `/browser uninstall`.",
                config_path.display()
            ),
        );
        return Ok(());
    }
    let existing = std::fs::read_to_string(&config_path)
        .ok()
        .map(|text| serde_json::from_str::<serde_json::Value>(&text))
        .transpose()
        .ok()
        .flatten();
    let (doc, outcome) = uninstall_playwright(existing);
    match (doc, outcome) {
        (Some(ref doc), UninstallOutcome::Removed) => {
            let result: std::result::Result<(), String> = (|| {
                let text =
                    serde_json::to_string_pretty(doc).map_err(|e| format!("serialize: {e}"))?;
                std::fs::write(&config_path, text + "\n").map_err(|e| format!("write: {e}"))?;
                Ok(())
            })();
            let msg = match result {
                Ok(()) => format!(
                    "✓ Removed the {PLAYWRIGHT_SERVER} server from {}.\n\nRestart Shannon for the browser tools to disappear.",
                    config_path.display()
                ),
                Err(e) => format!("Failed to write {}: {e}", config_path.display()),
            };
            repl.chat.add_message(ChatRole::System, msg);
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "✓ The {PLAYWRIGHT_SERVER} server is already absent from {}.",
                    config_path.display()
                ),
            );
        }
    }
    Ok(())
}

fn handle_setup(repl: &mut Repl) -> Result<()> {
    // npx is the launch vehicle for the Playwright MCP server; without it
    // the server would be configured but never start.
    let npx_ok = std::process::Command::new("npx")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !npx_ok {
        repl.chat.add_message(
            ChatRole::System,
            "`npx` was not found on PATH. Install Node.js (https://nodejs.org) and re-run `/browser setup`.".to_string(),
        );
        return Ok(());
    }

    let config_path = Path::new(&repl.state.working_directory).join(".mcp.json");
    // Refuse to write through a symlink — a planted symlink could redirect
    // the write to arbitrary files (same policy as `shannon mcp` install).
    if config_path.is_symlink() {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Refusing to write through symlink {} for security. Remove it and re-run `/browser setup`.",
                config_path.display()
            ),
        );
        return Ok(());
    }

    let existing = std::fs::read_to_string(&config_path)
        .ok()
        .map(|text| serde_json::from_str::<serde_json::Value>(&text))
        .transpose()
        .ok()
        .flatten();

    let (merged, outcome) = merge_playwright_server(existing);
    if let Err(e) =
        serde_json::to_string_pretty(&merged).map(|text| std::fs::write(&config_path, text + "\n"))
    {
        repl.chat.add_message(
            ChatRole::System,
            format!("Failed to write {}: {e}", config_path.display()),
        );
        return Ok(());
    }

    let msg = match outcome {
        MergeOutcome::Added => format!(
            "✓ Added the Playwright MCP server to {}.\n\nRestart Shannon (or start a new session) to load it. You'll get browser tools (mcp__playwright__browser_navigate, browser_snapshot, browser_click, browser_take_screenshot, ...) plus automatic browser-workflow guidance.",
            config_path.display()
        ),
        MergeOutcome::AlreadyConfigured => format!(
            "✓ The Playwright MCP server is already configured in {}.",
            config_path.display()
        ),
        MergeOutcome::Replaced => format!(
            "✓ Replaced the stale '{PLAYWRIGHT_SERVER}' entry with the current package ({}).\n\nRestart Shannon to load it.",
            PLAYWRIGHT_ARGS.join(" ")
        ),
    };
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

/// Outcome of removing the Playwright server from `.mcp.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UninstallOutcome {
    /// Entry was present and is now removed; doc reflects the post-write state.
    Removed,
    /// Entry was already absent; doc is the unchanged input (or None for missing file).
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeOutcome {
    Added,
    AlreadyConfigured,
    Replaced,
}

/// Merge the Playwright MCP server entry into an `.mcp.json` document
/// (or create one). Idempotent: an identical entry is left untouched and
/// unrelated servers are preserved.
fn merge_playwright_server(
    existing: Option<serde_json::Value>,
) -> (serde_json::Value, MergeOutcome) {
    let desired = serde_json::json!({
        "command": PLAYWRIGHT_COMMAND,
        "args": PLAYWRIGHT_ARGS,
    });

    let mut doc = existing.unwrap_or_else(|| serde_json::json!({}));
    if !doc.is_object() {
        doc = serde_json::json!({});
    }
    if doc.get("mcpServers").is_none_or(|s| !s.is_object()) {
        if let Some(obj) = doc.as_object_mut() {
            obj.insert("mcpServers".to_string(), serde_json::json!({}));
        }
    }

    let servers = doc
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(|s| s.as_object_mut())
        .expect("mcpServers normalized to an object above");

    let outcome = match servers.get(PLAYWRIGHT_SERVER) {
        Some(entry) if *entry == desired => MergeOutcome::AlreadyConfigured,
        Some(_) => MergeOutcome::Replaced,
        None => MergeOutcome::Added,
    };
    servers.insert(PLAYWRIGHT_SERVER.to_string(), desired);
    (doc, outcome)
}

/// Remove the Playwright server from `.mcp.json`. Returns the updated doc
/// plus whether the entry was actually present (so the caller can report
/// "removed" vs "already absent").
fn uninstall_playwright(
    existing: Option<serde_json::Value>,
) -> (Option<serde_json::Value>, UninstallOutcome) {
    let Some(mut doc) = existing else {
        return (None, UninstallOutcome::Absent);
    };
    let removed = doc
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(|s| s.as_object_mut())
        .and_then(|servers| servers.remove(PLAYWRIGHT_SERVER));
    match removed {
        Some(_) => (Some(doc), UninstallOutcome::Removed),
        None => (Some(doc), UninstallOutcome::Absent),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_merge_into_empty_document() {
        let (doc, outcome) = merge_playwright_server(None);
        assert_eq!(outcome, MergeOutcome::Added);
        assert_eq!(doc["mcpServers"]["playwright"]["command"], json!("npx"));
        assert_eq!(
            doc["mcpServers"]["playwright"]["args"],
            json!(["@playwright/mcp@latest"])
        );
    }

    #[test]
    fn test_merge_preserves_unrelated_servers() {
        let existing = json!({
            "mcpServers": {
                "github": { "command": "npx", "args": ["@modelcontextprotocol/server-github"] }
            },
            "otherTopLevel": true
        });
        let (doc, outcome) = merge_playwright_server(Some(existing));
        assert_eq!(outcome, MergeOutcome::Added);
        assert!(doc["mcpServers"]["github"].is_object());
        assert_eq!(doc["otherTopLevel"], json!(true));
        assert!(doc["mcpServers"]["playwright"].is_object());
    }

    #[test]
    fn test_merge_is_idempotent() {
        let (first, outcome1) = merge_playwright_server(None);
        let (second, outcome2) = merge_playwright_server(Some(first));
        assert_eq!(outcome1, MergeOutcome::Added);
        assert_eq!(outcome2, MergeOutcome::AlreadyConfigured);
        assert_eq!(
            second["mcpServers"]["playwright"]["args"],
            json!(["@playwright/mcp@latest"])
        );
    }

    #[test]
    fn test_merge_replaces_stale_playwright_entry() {
        let existing = json!({
            "mcpServers": {
                "playwright": { "command": "npx", "args": ["@anthropic-ai/mcp-server-playwright"] }
            }
        });
        let (doc, outcome) = merge_playwright_server(Some(existing));
        assert_eq!(outcome, MergeOutcome::Replaced);
        assert_eq!(
            doc["mcpServers"]["playwright"]["args"],
            json!(["@playwright/mcp@latest"])
        );
    }

    #[test]
    fn test_merge_repairs_invalid_document_shapes() {
        // Non-object root and non-object mcpServers are both normalized.
        for bad in [json!("nope"), json!({ "mcpServers": [] })] {
            let (doc, outcome) = merge_playwright_server(Some(bad));
            assert_eq!(outcome, MergeOutcome::Added);
            assert!(doc["mcpServers"]["playwright"].is_object());
        }
    }

    // ── uninstall_playwright ──

    #[test]
    fn test_uninstall_removes_only_playwright_server() {
        let existing = json!({
            "mcpServers": {
                "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] },
                "github": { "command": "npx", "args": ["@modelcontextprotocol/server-github"] }
            },
            "otherTopLevel": true
        });
        let (doc, outcome) = uninstall_playwright(Some(existing));
        assert_eq!(outcome, UninstallOutcome::Removed);
        let doc = doc.unwrap();
        assert!(doc["mcpServers"]["github"].is_object());
        assert!(doc["mcpServers"]["playwright"].is_null());
        assert_eq!(doc["otherTopLevel"], json!(true));
    }

    #[test]
    fn test_uninstall_when_already_absent() {
        let existing = json!({ "mcpServers": { "github": {} } });
        let (doc, outcome) = uninstall_playwright(Some(existing.clone()));
        assert_eq!(outcome, UninstallOutcome::Absent);
        // The doc is returned unchanged for Absent (caller may rewrite or skip).
        assert_eq!(doc.unwrap(), existing);
    }

    #[test]
    fn test_uninstall_on_missing_file() {
        let (doc, outcome) = uninstall_playwright(None);
        assert_eq!(outcome, UninstallOutcome::Absent);
        assert!(doc.is_none());
    }

    #[test]
    fn test_uninstall_round_trips_with_setup() {
        // setup then uninstall returns to a doc where playwright is gone but
        // other servers / top-level keys survive.
        let initial = json!({ "mcpServers": { "github": {} }, "k": 1 });
        let (after_setup, _) = merge_playwright_server(Some(initial.clone()));
        let (after_uninstall, _) = uninstall_playwright(Some(after_setup));
        let restored = after_uninstall.unwrap();
        assert!(restored["mcpServers"]["playwright"].is_null());
        assert_eq!(restored["mcpServers"]["github"], json!({}));
        assert_eq!(restored["k"], json!(1));
    }
}
