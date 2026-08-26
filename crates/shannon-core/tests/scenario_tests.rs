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
    ScenarioResult, ToolCallTrace, TrajectorySummary, ValidationContext, create_scenario_workspace,
    evaluate_rules, parse_scenario, parse_scenarios_dir, validate_rules, yaml_to_mock_responses,
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
// W2-M1a behavioral assertion vocabulary: positive/negative pairs.
scenario_parse_test!(parse_traj_contains_hit, "traj_contains_hit.yaml");
scenario_parse_test!(parse_traj_contains_miss, "traj_contains_miss.yaml");
scenario_parse_test!(
    parse_forbidden_tool_respected,
    "forbidden_tool_respected.yaml"
);
scenario_parse_test!(
    parse_forbidden_tool_violated,
    "forbidden_tool_violated.yaml"
);
scenario_parse_test!(parse_diff_matches_applied, "diff_matches_applied.yaml");
scenario_parse_test!(
    parse_diff_matches_mismatched,
    "diff_matches_mismatched.yaml"
);
scenario_parse_test!(
    parse_cost_below_within_budget,
    "cost_below_within_budget.yaml"
);
scenario_parse_test!(parse_cost_below_over_budget, "cost_below_over_budget.yaml");

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

// ── W2-M1a behavioral assertion workflows ─────────────────────────────

/// Setup-time file baselines for `diff_matches`.
fn setup_baselines(
    scenario: &shannon_core::testing::scenario::ScenarioYaml,
) -> Vec<(String, String)> {
    scenario
        .setup
        .files
        .iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect()
}

/// Locate a specific rule's outcome in an evaluation result.
fn rule_outcome<'a>(
    outcomes: &'a [shannon_core::testing::scenario::RuleOutcome],
    tag: &str,
) -> &'a shannon_core::testing::scenario::RuleOutcome {
    outcomes
        .iter()
        .find(|o| o.rule == tag)
        .unwrap_or_else(|| panic!("outcome for rule '{tag}' missing"))
}

#[test]
fn trajectory_contains_yaml_positive_and_negative() {
    // Positive: observed Read->Edit order contains the declared subsequence.
    let hit = parse_scenario(&scenarios_dir().join("traj_contains_hit.yaml")).expect("parse hit");
    let dir = create_scenario_workspace(&hit.setup);
    let trace_hit = ToolCallTrace::from_mock_turns(&hit.mock_responses);
    assert_eq!(trace_hit.len(), 2, "mocks declare Read then Edit");

    let ctx = ValidationContext::new(dir.path(), "success", "").with_trajectory(&trace_hit);
    let outcomes = evaluate_rules(&hit.validate, &ctx);
    assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

    // Negative: demanding Edit *before* Read cannot match and must fail.
    let miss =
        parse_scenario(&scenarios_dir().join("traj_contains_miss.yaml")).expect("parse miss");
    let trace_miss = ToolCallTrace::from_mock_turns(&miss.mock_responses);
    let ctx = ValidationContext::new(dir.path(), "success", "").with_trajectory(&trace_miss);
    let outcomes = evaluate_rules(&miss.validate, &ctx);
    let outcome = rule_outcome(&outcomes, "trajectory_contains");
    assert!(!outcome.passed);
    assert!(
        outcome.details[0].starts_with("trajectory_contains: step"),
        "unexpected detail: {:?}",
        outcome.details
    );
}

#[test]
fn forbidden_tool_yaml_respected_and_violated() {
    let respected = parse_scenario(&scenarios_dir().join("forbidden_tool_respected.yaml"))
        .expect("parse respected");
    let violated = parse_scenario(&scenarios_dir().join("forbidden_tool_violated.yaml"))
        .expect("parse violated");
    let dir = create_scenario_workspace(&respected.setup);
    let trace = ToolCallTrace::from_mock_turns(&respected.mock_responses);
    let ctx = ValidationContext::new(dir.path(), "success", "").with_trajectory(&trace);

    // Bash never occurs → the ban holds.
    let outcomes = evaluate_rules(&respected.validate, &ctx);
    assert!(outcomes[0].passed, "{:?}", outcomes[0].details);

    // Edit is banned here but the trajectory uses it → flagged with detail.
    let outcomes = evaluate_rules(&violated.validate, &ctx);
    let outcome = rule_outcome(&outcomes, "forbidden_tool");
    assert!(!outcome.passed);
    assert_eq!(outcome.details[0], "forbidden_tool: 'Edit' was invoked");
}

#[test]
fn diff_matches_yaml_applied_and_mismatched() {
    let applied =
        parse_scenario(&scenarios_dir().join("diff_matches_applied.yaml")).expect("parse applied");
    let mismatched = parse_scenario(&scenarios_dir().join("diff_matches_mismatched.yaml"))
        .expect("parse mismatched");
    let dir = create_scenario_workspace(&applied.setup);

    // Apply exactly the edit the mocks declare.
    let main_path = dir.path().join("src/main.rs");
    let edited = std::fs::read_to_string(&main_path)
        .unwrap()
        .replace("Hello", "Goodbye");
    std::fs::write(&main_path, edited).expect("apply edit");

    let baselines = setup_baselines(&applied);
    let ctx = ValidationContext::new(dir.path(), "success", "").with_initial_files(&baselines);

    // Positive declaration: both diff regexes match the applied change.
    let outcomes = evaluate_rules(&applied.validate, &ctx);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|o| o.passed), "{outcomes:?}");

    // Negative declaration expects a Farewell line that never appears.
    let outcomes = evaluate_rules(&mismatched.validate, &ctx);
    let outcome = rule_outcome(&outcomes, "diff_matches");
    assert!(!outcome.passed);
    assert!(
        outcome.details[0].contains("diff does not match regex"),
        "unexpected detail: {:?}",
        outcome.details
    );
}

#[test]
fn cost_below_yaml_within_and_over_budget() {
    let within = parse_scenario(&scenarios_dir().join("cost_below_within_budget.yaml"))
        .expect("parse within");
    let over =
        parse_scenario(&scenarios_dir().join("cost_below_over_budget.yaml")).expect("parse over");
    let dir = create_scenario_workspace(&within.setup);

    // Observed per-turn spend from the run's usage accounting.
    let costs = vec![0.01_f64, 0.02];
    let ctx = ValidationContext::new(dir.path(), "success", "").with_turn_costs_usd(&costs);

    // Positive: $0.03 total under both budgets passes at task AND turn grain.
    let outcomes = evaluate_rules(&within.validate, &ctx);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|o| o.passed), "{outcomes:?}");

    let result = ScenarioResult::evaluated(
        "cost_below_within_budget",
        1,
        outcomes,
        TrajectorySummary::from_observations(
            &ToolCallTrace::from_mock_turns(&within.mock_responses),
            &costs,
        ),
    );
    assert!(result.passed);
    assert_eq!(result.trajectory_summary.tool_calls, vec!["Read", "Edit"]);
    assert!((result.trajectory_summary.total_cost_usd - 0.03).abs() < 1e-9);

    // Negative: $0.015 ceilings trip on the total (task) and on turn 2 (turn).
    let outcomes = evaluate_rules(&over.validate, &ctx);
    let failed: Vec<_> = outcomes.iter().filter(|o| !o.passed).collect();
    assert_eq!(
        failed.len(),
        2,
        "both budget bases must be reported as violated"
    );
    assert!(
        failed.iter().any(|o| o.details[0].contains("task total")),
        "task-basis violation missing: {failed:?}"
    );
    assert!(
        failed.iter().any(|o| o.details[0].contains("turn 2")),
        "turn-basis violation missing: {failed:?}"
    );
}
