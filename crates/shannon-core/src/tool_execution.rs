//! File-modifying tool classification.
//!
//! [`is_file_modifying_tool`] is consulted by the query engine to decide
//! whether a tool invocation should trigger auto-checkpointing.

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
}
