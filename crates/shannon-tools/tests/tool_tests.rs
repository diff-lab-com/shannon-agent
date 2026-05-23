//! Tool tests for shannon-tools
//!
//! Tests tool implementations including:
//! - CronTool: cron expression parsing and validation
//! - BashTool: security analysis function with various commands
//! - ReplTool: command execution through the Tool interface
//! - SleepTool: basic sleep functionality

use shannon_tools::{
    BashTool, ReplTool, SleepTool, Tool,
    cron::validate_cron,
    system::{SecurityLevel, analyze_command_security},
};
use std::collections::HashMap;

// ============================================================================
// CronTool Tests
// ============================================================================

#[test]
fn test_cron_validate_valid_every_minute() {
    assert!(validate_cron("* * * * *").is_ok());
    assert!(validate_cron("0 * * * *").is_ok());
    assert!(validate_cron("*/5 * * * *").is_ok());
}

#[test]
fn test_cron_validate_specific_time() {
    assert!(validate_cron("30 14 * * *").is_ok());
    assert!(validate_cron("0 0 * * *").is_ok()); // midnight
    assert!(validate_cron("59 23 31 12 *").is_ok()); // 11:59 PM on Dec 31
}

#[test]
fn test_cron_validate_hourly() {
    assert!(validate_cron("0 * * * *").is_ok());
    assert!(validate_cron("15 * * * *").is_ok());
    assert!(validate_cron("0 */2 * * *").is_ok()); // every 2 hours
}

#[test]
fn test_cron_validate_weekdays() {
    assert!(validate_cron("0 0 * * 1").is_ok()); // Monday
    assert!(validate_cron("0 0 * * 5").is_ok()); // Friday
    assert!(validate_cron("0 0 * * 1-5").is_ok()); // Mon-Fri
}

#[test]
fn test_cron_validate_day_names() {
    assert!(validate_cron("0 0 * * MON").is_ok());
    assert!(validate_cron("0 0 * * Fri").is_ok());
    assert!(validate_cron("0 0 * * mon,tue,wed").is_ok());
}

#[test]
fn test_cron_validate_month_names() {
    assert!(validate_cron("0 0 1 JAN *").is_ok());
    assert!(validate_cron("0 0 1 Dec *").is_ok());
    assert!(validate_cron("0 0 1 Jan-Mar *").is_ok());
}

#[test]
fn test_cron_validate_range_expressions() {
    assert!(validate_cron("0 9-17 * * *").is_ok()); // 9 AM to 5 PM
    assert!(validate_cron("0 0 1-15 * *").is_ok()); // first half of month
    assert!(validate_cron("0 */6 1-10 * *").is_ok()); // every 6 hours, days 1-10
}

#[test]
fn test_cron_validate_list_values() {
    assert!(validate_cron("0 0,12 * * *").is_ok()); // midnight and noon
    assert!(validate_cron("0 9,12,15 * * *").is_ok()); // 9 AM, noon, 3 PM
}

#[test]
fn test_cron_invalid_too_few_fields() {
    assert!(validate_cron("* * * *").is_err());
    assert!(validate_cron("* * *").is_err());
    assert!(validate_cron("").is_err());
}

#[test]
fn test_cron_invalid_too_many_fields() {
    assert!(validate_cron("* * * * * *").is_err());
    assert!(validate_cron("* * * * * extra").is_err());
}

#[test]
fn test_cron_invalid_minute_out_of_range() {
    assert!(validate_cron("60 * * * *").is_err());
    assert!(validate_cron("-1 * * * *").is_err());
    assert!(validate_cron("100 * * * *").is_err());
}

#[test]
fn test_cron_invalid_hour_out_of_range() {
    assert!(validate_cron("* 24 * * *").is_err());
    assert!(validate_cron("* -1 * * *").is_err());
}

#[test]
fn test_cron_invalid_day_of_month_out_of_range() {
    assert!(validate_cron("* * 0 * *").is_err());
    assert!(validate_cron("* * 32 * *").is_err());
}

#[test]
fn test_cron_invalid_month_out_of_range() {
    assert!(validate_cron("* * * 0 *").is_err());
    assert!(validate_cron("* * * 13 *").is_err());
}

#[test]
fn test_cron_invalid_day_of_week_out_of_range() {
    assert!(validate_cron("* * * * 8").is_err());
    assert!(validate_cron("* * * * -1").is_err());
}

#[test]
fn test_cron_invalid_step_zero() {
    assert!(validate_cron("*/0 * * * *").is_err());
    assert!(validate_cron("* */0 * * *").is_err());
}

#[test]
fn test_cron_invalid_range_inverted() {
    assert!(validate_cron("* 23-9 * * *").is_err()); // 23-9 is invalid
}

#[test]
fn test_cron_invalid_non_numeric() {
    assert!(validate_cron("abc * * * *").is_err());
    assert!(validate_cron("* abc * * *").is_err());
}

#[test]
fn test_cron_invalid_day_name() {
    assert!(validate_cron("* * * * ABC").is_err());
    assert!(validate_cron("* * * * Mond").is_err()); // typo
}

#[test]
fn test_cron_invalid_month_name() {
    assert!(validate_cron("* * * Abc *").is_err());
    assert!(validate_cron("* * * Janu *").is_err()); // typo
}

#[test]
fn test_cron_invalid_negative() {
    assert!(validate_cron("-1 * * * *").is_err());
    assert!(validate_cron("* -1 * * *").is_err());
}

// ============================================================================
// BashTool Security Analysis Tests
// ============================================================================

#[test]
fn test_security_safe_read_only_commands() {
    let analysis = analyze_command_security("ls -la");
    assert_eq!(analysis.risk_level, SecurityLevel::Low);
    assert!(analysis.is_read_only);
    assert!(!analysis.is_destructive);

    let analysis = analyze_command_security("cat file.txt");
    assert_eq!(analysis.risk_level, SecurityLevel::Low);
    assert!(analysis.is_read_only);

    let analysis = analyze_command_security("grep pattern file.txt");
    assert_eq!(analysis.risk_level, SecurityLevel::Low);
    assert!(analysis.is_read_only);
}

#[test]
fn test_security_destructive_patterns() {
    let analysis = analyze_command_security("rm -rf /");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
    assert!(!analysis.warnings.is_empty());

    let analysis = analyze_command_security("dd if=/dev/zero of=/dev/sda");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
}

#[test]
fn test_security_confirmation_required_patterns() {
    let analysis = analyze_command_security("rm -rf mydir");
    assert_eq!(analysis.risk_level, SecurityLevel::High);
    assert!(analysis.is_destructive);
    assert!(analysis.requires_confirmation);
}

#[test]
fn test_security_path_traversal_detection() {
    let analysis = analyze_command_security("cat ../../../etc/passwd");
    assert!(analysis.contains_path_traversal);
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(!analysis.warnings.is_empty());

    let analysis = analyze_command_security("ls ../../tmp");
    assert!(analysis.contains_path_traversal);
}

#[test]
fn test_security_sudo_elevated_privileges() {
    let analysis = analyze_command_security("sudo apt update");
    assert!(analysis.risk_level >= SecurityLevel::Medium);
    assert!(!analysis.warnings.is_empty());
    assert!(analysis.warnings.iter().any(|w| w.contains("sudo")));
}

#[test]
fn test_security_pipe_chain_risk() {
    // Read-only commands with pipes stay Low risk
    let analysis = analyze_command_security("cat file.txt | grep pattern");
    assert_eq!(analysis.risk_level, SecurityLevel::Low);

    // Non-read-only commands with pipes become Medium risk
    // Use commands that aren't in READ_ONLY_PATTERNS and don't contain them
    let analysis = analyze_command_security("make build | tee output");
    assert_eq!(analysis.risk_level, SecurityLevel::Medium);
}

#[test]
fn test_security_redirect_overwrite_risk() {
    // echo is in READ_ONLY_PATTERNS, so the redirect doesn't upgrade risk
    let analysis = analyze_command_security("echo data > file.txt");
    assert_eq!(analysis.risk_level, SecurityLevel::Low);

    // For a command with redirect that's not read-only, use non-read-only command
    let analysis = analyze_command_security("python script.py > output.txt");
    assert_eq!(analysis.risk_level, SecurityLevel::Medium);
}

#[test]
fn test_security_safe_git_read_operations() {
    let analysis = analyze_command_security("git status");
    assert!(analysis.is_read_only);
    assert_eq!(analysis.risk_level, SecurityLevel::Low);

    let analysis = analyze_command_security("git log");
    assert!(analysis.is_read_only);
}

#[test]
fn test_security_analysis_warning_content() {
    let analysis = analyze_command_security("rm -rf important_dir");
    assert!(analysis.warnings.iter().any(|w| w.contains("rm -rf")));
}

// ============================================================================
// SleepTool Tests
// ============================================================================

#[tokio::test]
async fn test_sleep_short_duration() {
    let tool = SleepTool::new();
    let output = tool.execute_sleep(10).await.unwrap();
    assert!(output.success);
    assert!(output.stdout.contains("10ms"));
    assert_eq!(output.exit_code, 0);
}

#[tokio::test]
async fn test_sleep_tool_interface() {
    let tool = SleepTool::new();
    let input = serde_json::json!({
        "duration_ms": 50
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("50ms"));
    assert_eq!(
        result.metadata.get("duration_ms"),
        Some(&serde_json::json!(50))
    );
}

#[tokio::test]
async fn test_sleep_tool_name_and_description() {
    let tool = SleepTool::new();
    assert_eq!(tool.name(), "Sleep");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_sleep_tool_input_schema() {
    let tool = SleepTool::new();
    let schema = tool.input_schema();
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["duration_ms"]["type"] == "integer");
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("duration_ms"))
    );
}

// ============================================================================
// ReplTool Tests
// ============================================================================

#[tokio::test]
async fn test_repl_tool_basic_command() {
    let tool = ReplTool::new();
    let input = serde_json::json!({
        "command": "echo hello world"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("hello world"));
    assert_eq!(
        result.metadata.get("exit_code"),
        Some(&serde_json::json!(0))
    );
}

#[tokio::test]
async fn test_repl_tool_with_working_directory() {
    let tool = ReplTool::new();
    let input = serde_json::json!({
        "command": "pwd",
        "cwd": "/tmp"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("tmp"));
}

#[tokio::test]
async fn test_repl_tool_with_environment_variables() {
    let tool = ReplTool::new();
    let mut env = HashMap::new();
    env.insert("TEST_VAR".to_string(), "test_value".to_string());

    let input = serde_json::json!({
        "command": "env",
        "env": {"TEST_VAR": "test_value"}
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    // The env command should list our test variable
    assert!(result.content.contains("TEST_VAR"));
}

#[tokio::test]
async fn test_repl_tool_failed_command() {
    let tool = ReplTool::new();
    // Use ls with a nonexistent path
    let input = serde_json::json!({
        "command": "ls /nonexistent_path_xyz_123"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(result.is_error); // Command should fail
    assert!(result.content.contains("failed") || result.content.contains("No such file"));
}

#[tokio::test]
async fn test_repl_tool_empty_command_rejected() {
    let tool = ReplTool::new();
    let input = serde_json::json!({
        "command": ""
    });

    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Empty command"));
}

#[tokio::test]
async fn test_repl_tool_name_and_description() {
    let tool = ReplTool::new();
    assert_eq!(tool.name(), "REPL");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_repl_tool_input_schema() {
    let tool = ReplTool::new();
    let schema = tool.input_schema();
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["command"]["type"] == "string");
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("command"))
    );
}

#[tokio::test]
async fn test_repl_tool_command_injection_blocked() {
    let tool = ReplTool::new();
    let input = serde_json::json!({
        "command": "echo hello; rm -rf /"
    });

    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("command chaining"));
}

// ============================================================================
// BashTool Tests
// ============================================================================

#[tokio::test]
async fn test_bash_tool_simple_echo() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "echo test"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("test"));
}

#[tokio::test]
async fn test_bash_tool_name_and_description() {
    let tool = BashTool::new();
    assert_eq!(tool.name(), "Bash");
    assert!(!tool.description().is_empty());
}

#[tokio::test]
async fn test_bash_tool_input_schema() {
    let tool = BashTool::new();
    let schema = tool.input_schema();
    assert_eq!(schema["type"], "object");
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("command"))
    );
}

#[tokio::test]
async fn test_bash_tool_critical_command_rejected() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "rm -rf /"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("critical security risk"));
    assert!(result.metadata.get("security_rejected").is_some());
}

#[tokio::test]
async fn test_bash_tool_with_working_directory() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "pwd",
        "cwd": "/tmp"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_bash_tool_with_timeout() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "echo quick",
        "timeout": 5000
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("quick"));
}

#[tokio::test]
async fn test_bash_tool_with_environment_variables() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "echo $MY_VAR",
        "env": {"MY_VAR": "test_value"}
    });

    let result = tool.execute(input).await.unwrap();
    assert!(!result.is_error);
    assert!(result.content.contains("test_value"));
}

// ============================================================================
// Describe Risk Level Tests
// ============================================================================

#[test]
fn test_describe_risk_level_safe() {
    let desc = shannon_tools::system::describe_risk_level(SecurityLevel::Safe);
    assert!(desc.contains("Safe"));
}

#[test]
fn test_describe_risk_level_low() {
    let desc = shannon_tools::system::describe_risk_level(SecurityLevel::Low);
    assert!(desc.contains("Low Risk"));
}

#[test]
fn test_describe_risk_level_medium() {
    let desc = shannon_tools::system::describe_risk_level(SecurityLevel::Medium);
    assert!(desc.contains("Medium Risk"));
}

#[test]
fn test_describe_risk_level_high() {
    let desc = shannon_tools::system::describe_risk_level(SecurityLevel::High);
    assert!(desc.contains("High Risk"));
}

#[test]
fn test_describe_risk_level_critical() {
    let desc = shannon_tools::system::describe_risk_level(SecurityLevel::Critical);
    assert!(desc.contains("Critical"));
}

// ============================================================================
// Tool Metadata Tests
// ============================================================================

#[tokio::test]
async fn test_tool_metadata_includes_risk_level() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "ls -la"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(result.metadata.get("risk_level").is_some());
}

#[tokio::test]
async fn test_tool_metadata_includes_destructive_flag() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "rm -rf test_dir"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(result.metadata.get("is_destructive").is_some());
    assert_eq!(
        result.metadata.get("is_destructive"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn test_tool_metadata_includes_read_only_flag() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "cat file.txt"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(result.metadata.get("is_read_only").is_some());
    assert_eq!(
        result.metadata.get("is_read_only"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn test_tool_metadata_includes_warnings() {
    let tool = BashTool::new();
    // Use a destructive pattern (Critical) so the command is rejected early
    // with warnings in metadata — avoids actually executing anything.
    let input = serde_json::json!({
        "command": "rm -rf / --no-preserve-root"
    });

    let result = tool.execute(input).await.unwrap();
    assert!(result.metadata.get("warnings").is_some());
}
