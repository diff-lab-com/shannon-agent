//! /doctor command - Run system diagnostics
//!
//! Provides both a prompt template (for AI-driven deep diagnostics) and
//! local structured checks that run without consuming AI tokens.

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Doctor prompt template (used when deep AI analysis is requested)
const DOCTOR_PROMPT: &str = r##"
Run system diagnostics and health checks for Shannon Code.

Arguments: {args}
- If args contains a specific check name, run only that check
- If args is empty, run all checks

Checks to perform:
1. **API Key**: Verify the required API keys are set (ANTHROPIC_API_KEY or equivalent)
2. **Network**: Test connectivity to the AI provider endpoint
3. **Tools**: Check for required external tools (git, gh, etc.)
4. **Permissions**: Verify file system permissions in working directory
5. **Configuration**: Validate Shannon configuration files
6. **Disk Space**: Check available disk space
7. **Git**: Verify git is installed and repository state

For each check, report:
- Status: PASS, WARN, FAIL, or SKIP
- Details about what was found
- Suggested fixes for any issues

Use shell commands to gather information (uname, which, df, git, etc.).
"##;

/// Create the /doctor command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "doctor".to_string(),
            aliases: vec!["check".to_string(), "diagnostics".to_string()],
            description: "Run system diagnostics and health checks".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[check name]".to_string()),
            when_to_use: Some(
                "Use to diagnose issues with your Shannon Code installation and environment".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: true,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Running diagnostics...".to_string(),
        content_length: 2000,
        arg_names: vec!["check".to_string()],
        allowed_tools: vec![
            "Bash(which:*)".to_string(),
            "Bash(uname:*)".to_string(),
            "Bash(df:*)".to_string(),
            "Bash(git:*)".to_string(),
            "Bash(env:*)".to_string(),
            "Bash(gh:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(DOCTOR_PROMPT.to_string()),
    }))
}

// ── Local diagnostic checks (no AI tokens consumed) ─────────────

/// Status of a single diagnostic check
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "PASS"),
            CheckStatus::Warn => write!(f, "WARN"),
            CheckStatus::Fail => write!(f, "FAIL"),
            CheckStatus::Skip => write!(f, "SKIP"),
        }
    }
}

/// Result of a single diagnostic check
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
    pub fix_hint: Option<String>,
}

/// Run all local diagnostic checks and return results.
///
/// Delegates to `shannon_core::doctor::Doctor` for API key, network, tools,
/// permissions, configuration, disk space, and git checks. Then appends
/// Shannon-specific checks (Rust toolchain, Shannon config files) that the
/// core doctor doesn't cover.
pub fn run_all_checks() -> Vec<CheckResult> {
    let mut results = Vec::new();

    // Use the core Doctor for standard checks
    let doctor = shannon_core::doctor::Doctor::new();
    match doctor.run_full_diagnostic() {
        Ok(report) => {
            for check in &report.checks {
                results.push(CheckResult {
                    name: check.name.clone(),
                    status: convert_status(&check.status),
                    message: check.message.clone(),
                    fix_hint: check.fix_suggestion.clone(),
                });
            }
        }
        Err(_) => {
            // Fallback: run a minimal set of local checks if core Doctor fails
            results.push(check_api_keys());
            results.push(check_required_tools());
            results.push(check_git_repo());
        }
    }

    // Shannon-specific checks not in core Doctor
    results.push(check_config_files());
    results.push(check_rust_toolchain());

    results
}

/// Convert core CheckStatus to commands CheckStatus
fn convert_status(status: &shannon_core::doctor::CheckStatus) -> CheckStatus {
    match status {
        shannon_core::doctor::CheckStatus::Pass => CheckStatus::Pass,
        shannon_core::doctor::CheckStatus::Warn => CheckStatus::Warn,
        shannon_core::doctor::CheckStatus::Fail => CheckStatus::Fail,
        shannon_core::doctor::CheckStatus::Skip => CheckStatus::Skip,
    }
}

// ── Fallback local checks (used when core Doctor is unavailable) ──

/// Fallback: Check for API key environment variables
fn check_api_keys() -> CheckResult {
    let keys = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "SHANNON_API_KEY"];
    let mut found = Vec::new();

    for key in &keys {
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                found.push(*key);
            }
        }
    }

    if !found.is_empty() {
        CheckResult {
            name: "API Keys".to_string(),
            status: CheckStatus::Pass,
            message: format!("Found: {}", found.join(", ")),
            fix_hint: None,
        }
    } else {
        CheckResult {
            name: "API Keys".to_string(),
            status: CheckStatus::Fail,
            message: "No API keys found in environment".to_string(),
            fix_hint: Some("Set ANTHROPIC_API_KEY or OPENAI_API_KEY in your shell profile".to_string()),
        }
    }
}

/// Fallback: Check for required external tools
fn check_required_tools() -> CheckResult {
    let tools = ["git", "cargo"];
    let mut missing = Vec::new();

    for tool in &tools {
        if std::process::Command::new("which")
            .arg(tool)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            // tool found
        } else {
            missing.push(*tool);
        }
    }

    if missing.is_empty() {
        CheckResult {
            name: "Tools".to_string(),
            status: CheckStatus::Pass,
            message: "Required tools available".to_string(),
            fix_hint: None,
        }
    } else {
        CheckResult {
            name: "Tools".to_string(),
            status: CheckStatus::Fail,
            message: format!("Missing: {}", missing.join(", ")),
            fix_hint: Some("Install missing tools".to_string()),
        }
    }
}

/// Fallback: Check git repository status
fn check_git_repo() -> CheckResult {
    if !std::path::Path::new(".git").exists() {
        return CheckResult {
            name: "Git".to_string(),
            status: CheckStatus::Warn,
            message: "Not in a git repository".to_string(),
            fix_hint: Some("Run 'git init' or clone a repository".to_string()),
        };
    }

    CheckResult {
        name: "Git".to_string(),
        status: CheckStatus::Pass,
        message: "Git repository detected".to_string(),
        fix_hint: None,
    }
}

// ── Shannon-specific checks (not in core Doctor) ──────────────────

/// Check for Shannon-specific configuration files (not covered by core Doctor)
pub fn check_config_files() -> CheckResult {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_paths = [
        format!("{home}/.shannon/config.toml"),
        ".shannon.toml".to_string(),
        "shannon.toml".to_string(),
    ];

    let mut found_configs = Vec::new();
    for path in &config_paths {
        if std::path::Path::new(path).exists() {
            found_configs.push(path.clone());
        }
    }

    if !found_configs.is_empty() {
        CheckResult {
            name: "Shannon Config".to_string(),
            status: CheckStatus::Pass,            message: format!("Found: {}", found_configs.join(", ")),
            fix_hint: None,
        }
    } else {
        CheckResult {
            name: "Shannon Config".to_string(),
            status: CheckStatus::Warn,
            message: "No Shannon config files found (using defaults)".to_string(),
            fix_hint: Some("Create .shannon.toml or ~/.shannon/config.toml for custom settings".to_string()),
        }
    }
}

/// Check Rust toolchain availability
pub fn check_rust_toolchain() -> CheckResult {
    let has_rustc = std::process::Command::new("which")
        .arg("rustc")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !has_rustc {
        return CheckResult {
            name: "Rust Toolchain".to_string(),
            status: CheckStatus::Fail,
            message: "rustc not found in PATH".to_string(),
            fix_hint: Some("Install Rust via https://rustup.rs".to_string()),
        };
    }

    // Try to get rustc version
    if let Ok(output) = std::process::Command::new("rustc").arg("--version").output() {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return CheckResult {
                name: "Rust Toolchain".to_string(),
                status: CheckStatus::Pass,
                message: version,
                fix_hint: None,
            };
        }
    }

    CheckResult {
        name: "Rust Toolchain".to_string(),
        status: CheckStatus::Warn,
        message: "rustc found but version check failed".to_string(),
        fix_hint: None,
    }
}

/// Format all check results into a human-readable report
pub fn format_doctor_report(results: &[CheckResult]) -> String {
    let mut report = String::from("Shannon Code Diagnostics\n");
    report.push_str(&"─".repeat(40));
    report.push('\n');

    let pass_count = results.iter().filter(|r| r.status == CheckStatus::Pass).count();
    let warn_count = results.iter().filter(|r| r.status == CheckStatus::Warn).count();
    let fail_count = results.iter().filter(|r| r.status == CheckStatus::Fail).count();

    for result in results {
        let indicator = match result.status {
            CheckStatus::Pass => "[PASS]",
            CheckStatus::Warn => "[WARN]",
            CheckStatus::Fail => "[FAIL]",
            CheckStatus::Skip => "[SKIP]",
        };
        report.push_str(&format!("{indicator} {}: {}\n", result.name, result.message));
        if let Some(ref hint) = result.fix_hint {
            report.push_str(&format!("      Fix: {hint}\n"));
        }
    }

    report.push_str(&"─".repeat(40));
    report.push('\n');
    report.push_str(&format!(
        "Results: {pass_count} passed, {warn_count} warnings, {fail_count} failed\n"
    ));

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doctor_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "doctor");
        assert!(cmd.aliases().contains(&"check".to_string()));
        assert!(cmd.aliases().contains(&"diagnostics".to_string()));
    }

    #[test]
    fn test_doctor_command_has_prompt() {
        let cmd = command();
        if let crate::command::Command::Prompt(pc) = cmd {
            assert!(pc.prompt_template.is_some());
            let template = pc.prompt_template.unwrap();
            assert!(template.contains("diagnostics"));
            assert!(template.contains("API Key"));
        } else {
            panic!("Expected PromptCommand");
        }
    }

    #[test]
    fn test_check_config_files_runs() {
        let result = check_config_files();
        assert_eq!(result.name, "Shannon Config");
    }

    #[test]
    fn test_check_rust_toolchain_runs() {
        let result = check_rust_toolchain();
        assert_eq!(result.name, "Rust Toolchain");
        // We're building with Rust, so this should pass
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn test_format_doctor_report() {
        let results = vec![
            CheckResult {
                name: "Test 1".to_string(),
                status: CheckStatus::Pass,
                message: "All good".to_string(),
                fix_hint: None,
            },
            CheckResult {
                name: "Test 2".to_string(),
                status: CheckStatus::Fail,
                message: "Something wrong".to_string(),
                fix_hint: Some("Fix it".to_string()),
            },
        ];
        let report = format_doctor_report(&results);
        assert!(report.contains("[PASS]"));
        assert!(report.contains("[FAIL]"));
        assert!(report.contains("Fix: Fix it"));
        assert!(report.contains("1 passed"));
        assert!(report.contains("1 failed"));
    }

    #[test]
    fn test_check_status_display() {
        assert_eq!(CheckStatus::Pass.to_string(), "PASS");
        assert_eq!(CheckStatus::Warn.to_string(), "WARN");
        assert_eq!(CheckStatus::Fail.to_string(), "FAIL");
        assert_eq!(CheckStatus::Skip.to_string(), "SKIP");
    }

    #[test]
    fn test_run_all_checks() {
        let results = run_all_checks();
        assert!(!results.is_empty());
        // Core Doctor provides 7 checks + 2 Shannon-specific = 9 total
        assert!(results.len() >= 7);
    }

    // ── Integration tests for core Doctor delegation ──

    #[test]
    fn test_convert_status_all_variants() {
        assert_eq!(convert_status(&shannon_core::doctor::CheckStatus::Pass), CheckStatus::Pass);
        assert_eq!(convert_status(&shannon_core::doctor::CheckStatus::Warn), CheckStatus::Warn);
        assert_eq!(convert_status(&shannon_core::doctor::CheckStatus::Fail), CheckStatus::Fail);
        assert_eq!(convert_status(&shannon_core::doctor::CheckStatus::Skip), CheckStatus::Skip);
    }

    #[test]
    fn test_core_doctor_report_structure() {
        // Verify core Doctor produces a valid report
        let doctor = shannon_core::doctor::Doctor::new();
        let report = doctor.run_full_diagnostic();
        assert!(report.is_ok(), "Core Doctor should produce a report without errors");

        let report = report.unwrap();
        assert!(!report.checks.is_empty(), "Core Doctor should return at least one check");

        for check in &report.checks {
            assert!(!check.name.is_empty(), "Each check should have a name");
        }
    }

    #[test]
    fn test_run_all_checks_includes_shannon_specific() {
        let results = run_all_checks();
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();

        // Shannon-specific checks should always be present
        assert!(names.iter().any(|n| *n == "Rust Toolchain"), "Should include Rust Toolchain check");
        assert!(names.iter().any(|n| *n == "Shannon Config"), "Should include Shannon Config check");
    }

    #[test]
    fn test_run_all_checks_no_duplicates() {
        let results = run_all_checks();
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();

        // Shannon-specific checks should not duplicate core Doctor checks
        let rust_count = names.iter().filter(|n| **n == "Rust Toolchain").count();
        let config_count = names.iter().filter(|n| **n == "Shannon Config").count();
        assert_eq!(rust_count, 1, "Rust Toolchain should appear exactly once");
        assert_eq!(config_count, 1, "Configuration should appear exactly once");
    }

    #[test]
    fn test_fallback_api_keys_check() {
        // The fallback check should produce a valid result
        let result = check_api_keys();
        assert_eq!(result.name, "API Keys");
        assert!(matches!(result.status, CheckStatus::Pass | CheckStatus::Fail));
    }

    #[test]
    fn test_fallback_required_tools_check() {
        let result = check_required_tools();
        assert_eq!(result.name, "Tools");
    }

    #[test]
    fn test_fallback_git_repo_check() {
        let result = check_git_repo();
        assert_eq!(result.name, "Git");
    }
}
