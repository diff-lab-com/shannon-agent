//! Tool schema snapshot tests using insta.
//!
//! Captures the input_schema() JSON of every registered tool. Any change to
//! tool schemas will surface as a diff in PR review, preventing accidental
//! breaking changes to the tool API.

use shannon_core::tools::ToolRegistry;
use shannon_tools::register_default_tools;

fn full_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_default_tools(&mut registry).expect("register_default_tools should succeed");
    registry
}

/// Snapshot all tool schemas in a single deterministic snapshot.
/// Sorted by tool name for stable diff output.
#[test]
fn snapshot_all_tool_schemas() {
    let registry = full_registry();
    let mut entries: Vec<(String, serde_json::Value)> = registry
        .list()
        .into_iter()
        .filter_map(|name| {
            let tool = registry.get(&name)?;
            Some((name, tool.input_schema()))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut output = String::new();
    for (name, schema) in &entries {
        output.push_str(&format!("## {name}\n"));
        output.push_str(&serde_json::to_string_pretty(schema).unwrap());
        output.push_str("\n\n");
    }

    insta::assert_snapshot!("all_tool_schemas", output);
}

/// Snapshot all tool names and descriptions for stable documentation.
#[test]
fn snapshot_all_tool_descriptions() {
    let registry = full_registry();
    let mut entries: Vec<(String, String)> = registry
        .list()
        .into_iter()
        .filter_map(|name| {
            let tool = registry.get(&name)?;
            Some((name, tool.description().to_string()))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut output = String::new();
    for (name, desc) in &entries {
        output.push_str(&format!("{name}: {desc}\n"));
    }

    insta::assert_snapshot!("all_tool_descriptions", output);
}
