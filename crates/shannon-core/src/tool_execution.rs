//! Tool-call helpers consulted by the query engine.
//!
//! [`is_file_modifying_tool`] decides whether an invocation should trigger
//! auto-checkpointing; [`inject_bash_default_cwd`] is the B2-2 dispatch
//! seam that pins Bash spawns to the engine's session working directory.

/// Tools that modify files and should trigger auto-checkpointing.
const FILE_MODIFYING_TOOLS: &[&str] = &[
    "Write",
    "write",
    "FileWrite",
    "file_write",
    "Edit",
    "edit",
    "FileEdit",
    "file_edit",
    "MultiEdit",
    "multi_edit",
    "Bash",
    "bash", // Bash may modify files via commands
];

/// Returns true if the tool is known to modify files.
pub fn is_file_modifying_tool(tool_name: &str) -> bool {
    // Case-insensitive: registered display names are capitalized ("Write").
    FILE_MODIFYING_TOOLS
        .iter()
        .any(|n| n.eq_ignore_ascii_case(tool_name))
}

/// B2-2 (P0-2): default a parsed Bash call's `cwd` to the engine's
/// configured session working directory.
///
/// Dispatch-seam injection: the engine owns `working_directory` per
/// instance, while the tool registry is shared across sessions (the desktop
/// holds one `Arc<ToolRegistry>`), so the default cannot live on the tool.
/// Runs on every parsed tool-call batch BEFORE the permission gate, so
/// prompts, hooks and execution all observe the effective input. A call
/// that already carries an explicit `cwd` is left untouched — the model's
/// choice wins; non-Bash calls and a `None` working directory (REPL/CLI/
/// server hosts) are no-ops, keeping their process-cwd inheritance exactly
/// as before. This is the other half of the env-block invariant: the
/// prompt advertises
/// [`QueryEngineConfig::effective_working_directory`](crate::query_engine::QueryEngineConfig::effective_working_directory)
/// and Bash spawns run in that same directory.
pub fn inject_bash_default_cwd(
    tool_inputs: &mut [(String, String, serde_json::Value)],
    working_directory: Option<&std::path::Path>,
) {
    let Some(wd) = working_directory else {
        return;
    };
    let cwd = serde_json::Value::String(wd.to_string_lossy().into_owned());
    for (_, tool_name, input) in tool_inputs.iter_mut() {
        if tool_name.eq_ignore_ascii_case("Bash") {
            if let Some(obj) = input.as_object_mut() {
                obj.entry("cwd".to_string()).or_insert(cwd.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Auto-checkpoint integration tests --

    #[test]
    fn test_is_file_modifying_tool() {
        assert!(is_file_modifying_tool("Write"));
        assert!(is_file_modifying_tool("write"));
        assert!(is_file_modifying_tool("Edit"));
        assert!(is_file_modifying_tool("Bash"));
        assert!(!is_file_modifying_tool("Read"));
        assert!(!is_file_modifying_tool("Grep"));
        assert!(!is_file_modifying_tool("Unknown"));
    }

    #[test]
    fn test_is_file_modifying_tool_variants() {
        // Uppercase
        assert!(is_file_modifying_tool("Write"));
        assert!(is_file_modifying_tool("Edit"));
        assert!(is_file_modifying_tool("Bash"));
        assert!(is_file_modifying_tool("MultiEdit"));
        assert!(is_file_modifying_tool("FileWrite"));
        assert!(is_file_modifying_tool("FileEdit"));
        // Lowercase
        assert!(is_file_modifying_tool("write"));
        assert!(is_file_modifying_tool("edit"));
        assert!(is_file_modifying_tool("bash"));
        assert!(is_file_modifying_tool("multi_edit"));
        assert!(is_file_modifying_tool("file_write"));
        assert!(is_file_modifying_tool("file_edit"));
        // Non-modifying
        assert!(!is_file_modifying_tool("Read"));
        assert!(!is_file_modifying_tool("Grep"));
        assert!(!is_file_modifying_tool("Glob"));
        assert!(!is_file_modifying_tool(""));
    }

    // -- B2-2: Bash default-cwd dispatch injection --

    fn bash_call(json: serde_json::Value) -> (String, String, serde_json::Value) {
        (
            format!("call-{}", uuid::Uuid::new_v4()),
            "Bash".to_string(),
            json,
        )
    }

    /// The core invariant: with a configured session working directory, a
    /// Bash call that omits `cwd` is spawned in it — the same directory the
    /// env block advertises (both read
    /// `QueryEngineConfig::effective_working_directory`). An explicit `cwd`
    /// and other tools pass through untouched.
    #[test]
    fn inject_bash_default_cwd_fills_missing_and_preserves_explicit() {
        let wd = std::path::Path::new("/tmp/session-dir");
        let mut calls = vec![
            bash_call(serde_json::json!({ "command": "pwd" })),
            bash_call(serde_json::json!({ "command": "ls", "cwd": "/explicit" })),
            (
                "call-read".to_string(),
                "Read".to_string(),
                serde_json::json!({ "file_path": "/x" }),
            ),
        ];
        inject_bash_default_cwd(&mut calls, Some(wd));
        assert_eq!(
            calls[0].2.get("cwd").and_then(|v| v.as_str()),
            Some("/tmp/session-dir"),
            "missing cwd must default to the session working directory"
        );
        assert_eq!(
            calls[1].2.get("cwd").and_then(|v| v.as_str()),
            Some("/explicit"),
            "an explicit cwd must win over the default"
        );
        assert!(
            calls[2].2.get("cwd").is_none(),
            "non-Bash calls must not gain a cwd"
        );
    }

    /// No configured working directory (REPL/CLI/server hosts): the input
    /// is untouched, so Bash keeps inheriting the process cwd — the exact
    /// pre-B2-2 fallback.
    #[test]
    fn inject_bash_default_cwd_without_config_is_noop() {
        let mut calls = vec![bash_call(serde_json::json!({ "command": "pwd" }))];
        let before = calls[0].2.clone();
        inject_bash_default_cwd(&mut calls, None);
        assert_eq!(
            calls[0].2, before,
            "None working_directory must not touch inputs"
        );
    }
}
