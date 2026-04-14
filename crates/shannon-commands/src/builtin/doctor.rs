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
    Command::Prompt(PromptCommand {
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
    })
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

/// Run all local diagnostic checks and return results
pub fn run_all_checks() -> Vec<CheckResult> {
    vec![
        check_api_keys(),
        check_network(),
        check_required_tools(),
        check_git_repo(),
        check_disk_space(),
        check_config_files(),
        check_rust_toolchain(),
    ]
}

/// Check for API key environment variables
pub fn check_api_keys() -> CheckResult {
    let keys = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "SHANNON_API_KEY"];
    let mut found = Vec::new();
    let mut missing = Vec::new();

    for key in &keys {
        match std::env::var(key) {
            Ok(val) => {
                if !val.is_empty() {
                    found.push(*key);
                } else {
                    missing.push(*key);
                }
            }
            Err(_) => missing.push(*key),
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

/// Check network connectivity to AI provider endpoints
pub fn check_network() -> CheckResult {
    let endpoints = [
        ("Anthropic API", "https://api.anthropic.com"),
        ("OpenAI API", "https://api.openai.com"),
    ];
    let mut reachable = Vec::new();
    let mut unreachable = Vec::new();

    for (name, url) in &endpoints {
        // Use curl for a lightweight connectivity probe (HEAD request, 5s timeout)
        let result = std::process::Command::new("curl")
            .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--head", "--max-time", "5", url])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        match result {
            Ok(out) if out.status.success() => {
                let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // Any HTTP response (even 401/403) means network works
                if !code.is_empty() && code != "000" {
                    reachable.push(*name);
                } else {
                    unreachable.push((*name, "no response".to_string()));
                }
            }
            Ok(out) => {
                let msg = if out.status.code() == Some(28) {
                    "timeout".to_string()
                } else {
                    "connection failed".to_string()
                };
                unreachable.push((*name, msg));
            }
            Err(_) => {
                unreachable.push((*name, "curl not available".to_string()));
            }
        }
    }

    if !reachable.is_empty() && unreachable.is_empty() {
        CheckResult {
            name: "Network".to_string(),
            status: CheckStatus::Pass,
            message: format!("Reachable: {}", reachable.join(", ")),
            fix_hint: None,
        }
    } else if !reachable.is_empty() {
        let failed: Vec<String> = unreachable.iter().map(|(n, r)| format!("{n} ({r})")).collect();
        CheckResult {
            name: "Network".to_string(),
            status: CheckStatus::Warn,
            message: format!("Partial: {} reachable, {} unreachable", reachable.join(", "), failed.join(", ")),
            fix_hint: Some("Check internet connection or proxy settings".to_string()),
        }
    } else if unreachable.iter().any(|(_, r)| r == "curl not available") {
        CheckResult {
            name: "Network".to_string(),
            status: CheckStatus::Skip,
            message: "curl not available for connectivity test".to_string(),
            fix_hint: None,
        }
    } else {
        let failed: Vec<String> = unreachable.iter().map(|(n, r)| format!("{n} ({r})")).collect();
        CheckResult {
            name: "Network".to_string(),
            status: CheckStatus::Fail,
            message: format!("Unreachable: {}", failed.join(", ")),
            fix_hint: Some("Check internet connection, DNS, or firewall settings".to_string()),
        }
    }
}

/// Check for required external tools
pub fn check_required_tools() -> CheckResult {
    let tools = ["git", "cargo"];
    let optional_tools = ["gh", "pdftotext", "rg"];
    let mut missing_required = Vec::new();
    let mut missing_optional = Vec::new();
    let mut found = Vec::new();

    for tool in &tools {
        if which_exists(tool) {
            found.push(*tool);
        } else {
            missing_required.push(*tool);
        }
    }

    for tool in &optional_tools {
        if which_exists(tool) {
            found.push(*tool);
        } else {
            missing_optional.push(*tool);
        }
    }

    if missing_required.is_empty() {
        let mut msg = format!("Required tools: OK ({})", found.join(", "));
        if !missing_optional.is_empty() {
            msg.push_str(&format!("\n  Optional missing: {}", missing_optional.join(", ")));
        }
        CheckResult {
            name: "Tools".to_string(),
            status: if missing_optional.is_empty() { CheckStatus::Pass } else { CheckStatus::Warn },
            message: msg,
            fix_hint: if missing_optional.is_empty() {
                None
            } else {
                Some(format!("Install optional tools for full functionality: {}", missing_optional.join(", ")))
            },
        }
    } else {
        CheckResult {
            name: "Tools".to_string(),
            status: CheckStatus::Fail,
            message: format!("Missing required: {}", missing_required.join(", ")),
            fix_hint: Some("Install missing tools before using Shannon".to_string()),
        }
    }
}

/// Check git repository status
pub fn check_git_repo() -> CheckResult {
    if !std::path::Path::new(".git").exists() {
        return CheckResult {
            name: "Git".to_string(),
            status: CheckStatus::Warn,
            message: "Not in a git repository".to_string(),
            fix_hint: Some("Run 'git init' or clone a repository".to_string()),
        };
    }

    // Check if git is accessible
    if !which_exists("git") {
        return CheckResult {
            name: "Git".to_string(),
            status: CheckStatus::Fail,
            message: "git not found in PATH".to_string(),
            fix_hint: Some("Install git".to_string()),
        };
    }

    CheckResult {
        name: "Git".to_string(),
        status: CheckStatus::Pass,
        message: "Git repository detected".to_string(),
        fix_hint: None,
    }
}

/// Check available disk space
pub fn check_disk_space() -> CheckResult {
    // Use `df` command which is universally available on Unix
    let output = std::process::Command::new("df")
        .arg("-h")
        .arg(".")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            // Parse df output: "Filesystem Size Used Avail Use% Mounted on"
            // Second line has actual values
            if let Some(line) = text.lines().nth(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let avail = parts[3]; // e.g. "123G", "500M", "999K"
                    let status = parse_disk_size(avail);
                    return CheckResult {
                        name: "Disk Space".to_string(),
                        status,
                        message: format!("{avail} available"),
                        fix_hint: if status == CheckStatus::Fail {
                            Some("Free up disk space for optimal performance".to_string())
                        } else if status == CheckStatus::Warn {
                            Some("Consider freeing up disk space soon".to_string())
                        } else {
                            None
                        },
                    };
                }
            }
        }
    }

    CheckResult {
        name: "Disk Space".to_string(),
        status: CheckStatus::Skip,
        message: "Could not check disk space".to_string(),
        fix_hint: None,
    }
}

/// Parse a human-readable disk size string (e.g. "123G", "500M") into a status
fn parse_disk_size(s: &str) -> CheckStatus {
    let s = s.trim();
    if let Some(num_str) = s.strip_suffix('G') {
        if let Ok(gb) = num_str.parse::<f64>() {
            return if gb < 1.0 { CheckStatus::Fail } else if gb < 5.0 { CheckStatus::Warn } else { CheckStatus::Pass };
        }
    }
    if s.ends_with('M') || s.ends_with('K') {
        return CheckStatus::Fail; // Less than 1GB
    }
    if s.ends_with('T') || s.ends_with('P') {
        return CheckStatus::Pass; // Multiple TB or PB
    }
    CheckStatus::Pass // Default to pass if we can't parse
}

/// Check for Shannon configuration files
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
            name: "Configuration".to_string(),
            status: CheckStatus::Pass,
            message: format!("Found: {}", found_configs.join(", ")),
            fix_hint: None,
        }
    } else {
        CheckResult {
            name: "Configuration".to_string(),
            status: CheckStatus::Warn,
            message: "No Shannon config files found (using defaults)".to_string(),
            fix_hint: Some("Create .shannon.toml or ~/.shannon/config.toml for custom settings".to_string()),
        }
    }
}

/// Check Rust toolchain availability
pub fn check_rust_toolchain() -> CheckResult {
    if !which_exists("rustc") {
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

/// Check if a command exists in PATH
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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
    fn test_check_api_keys_logic() {
        // This test just ensures the check runs without panicking
        let result = check_api_keys();
        assert!(!result.name.is_empty());
        // Either pass or fail depending on env
        assert!(matches!(result.status, CheckStatus::Pass | CheckStatus::Fail));
    }

    #[test]
    fn test_check_required_tools_runs() {
        let result = check_required_tools();
        assert_eq!(result.name, "Tools");
    }

    #[test]
    fn test_check_network_runs() {
        let result = check_network();
        assert_eq!(result.name, "Network");
        // Either pass, warn, fail, or skip depending on network/curl
        assert!(matches!(result.status, CheckStatus::Pass | CheckStatus::Warn | CheckStatus::Fail | CheckStatus::Skip));
    }

    #[test]
    fn test_check_git_repo_runs() {
        let result = check_git_repo();
        assert_eq!(result.name, "Git");
    }

    #[test]
    fn test_check_disk_space_runs() {
        let result = check_disk_space();
        assert_eq!(result.name, "Disk Space");
    }

    #[test]
    fn test_check_config_files_runs() {
        let result = check_config_files();
        assert_eq!(result.name, "Configuration");
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
        assert!(results.len() >= 7); // At least 7 checks
    }

    #[test]
    fn test_which_exists_git() {
        // git should be available in any reasonable development environment
        assert!(which_exists("git"));
    }

    #[test]
    fn test_which_not_exists() {
        assert!(!which_exists("nonexistent_command_12345"));
    }

    #[test]
    fn test_parse_disk_size() {
        assert_eq!(parse_disk_size("500G"), CheckStatus::Pass);
        assert_eq!(parse_disk_size("3G"), CheckStatus::Warn);
        assert_eq!(parse_disk_size("0.5G"), CheckStatus::Fail);
        assert_eq!(parse_disk_size("500M"), CheckStatus::Fail);
        assert_eq!(parse_disk_size("999K"), CheckStatus::Fail);
        assert_eq!(parse_disk_size("2T"), CheckStatus::Pass);
    }
}
