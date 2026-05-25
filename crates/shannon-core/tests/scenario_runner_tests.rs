//! Integration tests for the YAML scenario system.
//!
//! Validates that all YAML scenario files parse correctly and that the
//! mock response conversion + workspace setup + validation pipeline works
//! end-to-end using mockito-backed HTTP servers.

use std::path::PathBuf;

use mockito::Server;
use serde_json::json;
use shannon_core::testing::mock_dsl::{
    anthropic_sse, provider_content_type, provider_endpoint, render_for_provider,
};
use shannon_core::testing::scenario::{
    create_scenario_workspace, parse_scenario, parse_scenarios_dir, validate_rules,
    yaml_to_mock_responses,
};

fn scenarios_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("scenarios")
}

/// Mount a sequence of mock responses on a mockito server.
/// Replicated here because `mount_sse_sequence` is `#[cfg(test)]` gated
/// in the library crate and unavailable to integration tests.
fn mount_mocks(
    server: &mut Server,
    provider: &str,
    responses: &[shannon_core::testing::mock_dsl::MockResponse],
) -> Vec<mockito::Mock> {
    let endpoint = provider_endpoint(provider);
    let content_type = provider_content_type(provider);
    responses
        .iter()
        .map(|resp| {
            let body = render_for_provider(provider, resp);
            let mut mock = server
                .mock("POST", endpoint)
                .with_status(200)
                .with_header("content-type", content_type)
                .with_body(&body)
                .expect(1);
            if provider == "anthropic" {
                mock = mock.with_header("anthropic-version", "2023-06-01");
            }
            mock.create()
        })
        .collect()
}

// ── Parsing Tests ─────────────────────────────────────────────────────

#[test]
fn all_scenarios_parse_successfully() {
    let dir = scenarios_dir();
    if !dir.exists() {
        eprintln!("Scenarios dir not found, skipping");
        return;
    }
    let scenarios = parse_scenarios_dir(&dir).expect("parse scenarios");
    assert!(!scenarios.is_empty(), "should have at least one scenario");

    for (path, scenario) in &scenarios {
        assert!(
            !scenario.name.is_empty(),
            "name empty in {}",
            path.display()
        );
        assert!(
            !scenario.mock_responses.is_empty(),
            "no mock_responses in {}",
            path.display()
        );
        assert!(
            !scenario.validate.is_empty(),
            "no validate rules in {}",
            path.display()
        );
    }
}

#[test]
fn scenario_names_are_unique() {
    let dir = scenarios_dir();
    if !dir.exists() {
        return;
    }
    let scenarios = parse_scenarios_dir(&dir).expect("parse scenarios");
    let names: Vec<&str> = scenarios.iter().map(|(_, s)| s.name.as_str()).collect();
    let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len(), "duplicate scenario names found");
}

// ── Per-scenario parse tests ──────────────────────────────────────────

macro_rules! scenario_parse_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let path = scenarios_dir().join($file);
            let scenario = parse_scenario(&path).unwrap_or_else(|e| {
                panic!("Failed to parse {}: {e}", $file);
            });
            // Verify mock responses convert correctly
            let mocks = yaml_to_mock_responses(&scenario.mock_responses);
            assert!(!mocks.is_empty(), "should have mock responses");

            // Verify each mock can render as Anthropic SSE
            for mock in &mocks {
                let sse = anthropic_sse(mock);
                assert!(
                    sse.contains("message_start"),
                    "SSE should have message_start"
                );
                assert!(sse.contains("message_stop"), "SSE should have message_stop");
            }
        }
    };
}

scenario_parse_test!(parse_text_only, "text_only.yaml");
scenario_parse_test!(parse_write_file, "write_file.yaml");
scenario_parse_test!(parse_read_file, "read_file.yaml");
scenario_parse_test!(parse_edit_file, "edit_file.yaml");
scenario_parse_test!(parse_bash_command, "bash_command.yaml");
scenario_parse_test!(parse_multi_turn_edit, "multi_turn_edit.yaml");
scenario_parse_test!(parse_code_search, "code_search.yaml");
scenario_parse_test!(parse_error_recovery, "error_recovery.yaml");
scenario_parse_test!(parse_multi_tool, "multi_tool.yaml");
scenario_parse_test!(parse_complex_refactor, "complex_refactor.yaml");

// ── Workspace + Validation Integration Tests ──────────────────────────

#[test]
fn write_file_workspace_and_validation() {
    let path = scenarios_dir().join("write_file.yaml");
    let scenario = parse_scenario(&path).expect("parse write_file");

    // Create workspace
    let workspace = create_scenario_workspace(&scenario.setup);

    // Simulate tool execution: Write creates the file
    std::fs::write(workspace.path().join("hello.txt"), "world").expect("write file");

    // Validate
    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(failures.is_empty(), "validation failures: {failures:?}");
}

#[test]
fn edit_file_workspace_and_validation() {
    let path = scenarios_dir().join("edit_file.yaml");
    let scenario = parse_scenario(&path).expect("parse edit_file");

    let workspace = create_scenario_workspace(&scenario.setup);

    // Simulate: Read happens, then Edit replaces Hello → Goodbye
    let original = std::fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
    let edited = original.replace("Hello", "Goodbye");
    std::fs::write(workspace.path().join("src/main.rs"), edited).expect("write");

    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(failures.is_empty(), "validation failures: {failures:?}");
}

#[test]
fn multi_turn_edit_workspace_and_validation() {
    let path = scenarios_dir().join("multi_turn_edit.yaml");
    let scenario = parse_scenario(&path).expect("parse multi_turn_edit");

    let workspace = create_scenario_workspace(&scenario.setup);

    // Simulate: Edit adds doc comment
    let original = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    let edited = original.replace("pub fn add", "/// Adds two integers.\npub fn add");
    std::fs::write(workspace.path().join("src/lib.rs"), edited).expect("write");

    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(failures.is_empty(), "validation failures: {failures:?}");
}

#[test]
fn complex_refactor_workspace_and_validation() {
    let path = scenarios_dir().join("complex_refactor.yaml");
    let scenario = parse_scenario(&path).expect("parse complex_refactor");

    let workspace = create_scenario_workspace(&scenario.setup);

    // Simulate: Create greeting.rs + edit main.rs
    std::fs::write(
        workspace.path().join("src/greeting.rs"),
        "pub fn greet(name: &str) {\n    println!(\"Hello, {}!\", name);\n}\n",
    )
    .expect("write greeting.rs");

    let main_edited = "mod greeting;\n\nfn main() {\n    greeting::greet(\"Alice\");\n    greeting::greet(\"Bob\");\n    greeting::greet(\"Charlie\");\n}\n";
    std::fs::write(workspace.path().join("src/main.rs"), main_edited).expect("write main.rs");

    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(failures.is_empty(), "validation failures: {failures:?}");
}

#[test]
fn error_recovery_validation() {
    let path = scenarios_dir().join("error_recovery.yaml");
    let scenario = parse_scenario(&path).expect("parse error_recovery");

    let workspace = create_scenario_workspace(&scenario.setup);

    // File doesn't exist (error path), but scenario still succeeds
    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(failures.is_empty(), "validation failures: {failures:?}");
}

// ── Mock Server Integration ───────────────────────────────────────────

#[tokio::test]
async fn write_file_with_mockito_server() {
    let path = scenarios_dir().join("write_file.yaml");
    let scenario = parse_scenario(&path).expect("parse write_file");
    let _workspace = create_scenario_workspace(&scenario.setup);

    let mut server = Server::new_async().await;
    let mocks = yaml_to_mock_responses(&scenario.mock_responses);
    let _guards = mount_mocks(&mut server, "anthropic", &mocks);

    // Verify server responds correctly
    let endpoint = provider_endpoint("anthropic");
    let resp = reqwest::Client::new()
        .post(format!("{}{endpoint}", server.url()))
        .header("content-type", "application/json")
        .json(&json!({"model": "test", "messages": []}))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(body.contains("message_start"));
    assert!(body.contains("tool_use"));
    assert!(body.contains("Write"));
}

#[tokio::test]
async fn multi_turn_with_mockito_sequence() {
    let path = scenarios_dir().join("multi_turn_edit.yaml");
    let scenario = parse_scenario(&path).expect("parse multi_turn_edit");
    let _workspace = create_scenario_workspace(&scenario.setup);

    let mut server = Server::new_async().await;
    let mocks = yaml_to_mock_responses(&scenario.mock_responses);
    assert_eq!(
        mocks.len(),
        3,
        "should have 3 mock responses (Read, Edit, text)"
    );

    let _guards = mount_mocks(&mut server, "anthropic", &mocks);

    // First response should be Read tool call
    let endpoint = provider_endpoint("anthropic");
    let resp1 = reqwest::Client::new()
        .post(format!("{}{endpoint}", server.url()))
        .json(&json!({"model": "test", "messages": []}))
        .send()
        .await
        .expect("request 1");
    let body1 = resp1.text().await.expect("body 1");
    assert!(
        body1.contains("Read"),
        "first response should be Read tool call"
    );

    // Second response should be Edit tool call
    let resp2 = reqwest::Client::new()
        .post(format!("{}{endpoint}", server.url()))
        .json(&json!({"model": "test", "messages": []}))
        .send()
        .await
        .expect("request 2");
    let body2 = resp2.text().await.expect("body 2");
    assert!(
        body2.contains("Edit"),
        "second response should be Edit tool call"
    );

    // Third response should be text
    let resp3 = reqwest::Client::new()
        .post(format!("{}{endpoint}", server.url()))
        .json(&json!({"model": "test", "messages": []}))
        .send()
        .await
        .expect("request 3");
    let body3 = resp3.text().await.expect("body 3");
    assert!(
        body3.contains("doc comment") || body3.contains("text_delta"),
        "third response should be text"
    );
}

// ── Validation failure tests ──────────────────────────────────────────

#[test]
fn write_file_validation_fails_when_file_missing() {
    let path = scenarios_dir().join("write_file.yaml");
    let scenario = parse_scenario(&path).expect("parse");
    let workspace = create_scenario_workspace(&scenario.setup);

    // Don't create hello.txt → validation should fail
    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(!failures.is_empty(), "should fail when file is missing");
}

#[test]
fn edit_file_validation_fails_when_content_wrong() {
    let path = scenarios_dir().join("edit_file.yaml");
    let scenario = parse_scenario(&path).expect("parse");
    let workspace = create_scenario_workspace(&scenario.setup);

    // Edit with wrong content
    let original = std::fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
    let edited = original.replace("Hello", "Wrong");
    std::fs::write(workspace.path().join("src/main.rs"), edited).expect("write");

    let failures = validate_rules(&scenario.validate, workspace.path(), "success", "");
    assert!(
        !failures.is_empty(),
        "should fail when content doesn't match"
    );
}
