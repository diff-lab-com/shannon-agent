//! /security-review command - OWASP-focused security audit

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

const SECURITY_REVIEW_PROMPT: &str = r##"
You are a security-focused code reviewer. Perform an OWASP-aligned security audit on the code changes.

## Steps

1. Run `git diff --stat` to see what files changed.
2. Run `git diff` and `git diff --cached` to get all changes.
3. If a target path is specified, also read the full file for context.
4. Analyze every change for security vulnerabilities.

## OWASP Top 10 Checklist

Systematically check each category:

1. **Broken Access Control**: Missing auth checks, privilege escalation, insecure direct object references
2. **Cryptographic Failures**: Hardcoded secrets, weak hashing, missing encryption, insecure key storage
3. **Injection**: SQL injection, command injection, XSS, LDAP injection, path traversal
4. **Insecure Design**: Missing rate limiting, insecure defaults, trust boundary violations
5. **Security Misconfiguration**: Debug mode enabled, default credentials, unnecessary services, CORS misconfiguration
6. **Vulnerable Components**: Known-vulnerable dependencies, outdated libraries
7. **Auth Failures**: Weak passwords, missing MFA, session fixation, credential stuffing risks
8. **Data Integrity Failures**: Missing input validation, unsafe deserialization, insecure CI/CD
9. **Logging Failures**: Missing audit trails, sensitive data in logs, no alerting
10. **SSRF**: Unvalidated URLs, internal service access, DNS rebinding

## Additional Checks

- **Secrets Exposure**: API keys, tokens, passwords in code or config files
- **Unsafe Rust**: Raw pointer misuse, unchecked arithmetic, unsafe blocks
- **Path Traversal**: User-controlled file paths without sanitization
- **Resource Exhaustion**: Unbounded allocation, missing timeouts, DoS vectors
- **Error Handling**: Information leakage in error messages, panic in production code

## Severity

- **CRITICAL**: Actively exploitable, data breach or RCE risk
- **HIGH**: Exploitable with moderate effort
- **MEDIUM**: Requires specific conditions to exploit
- **LOW**: Defense-in-depth improvement
- **INFO**: Security observation

## Output Format

### Security Summary
Brief overview of changes and security posture.

### Vulnerabilities Found
For each finding:
- Severity and OWASP category
- File and line location
- Vulnerability description
- Proof of concept or attack scenario
- Remediation with code example

### Security Positives
Good security practices observed.

### Risk Assessment
**Risk Level**: Critical / High / Medium / Low
**Exploitability**: Easy / Moderate / Hard
**Business Impact**: Data breach / Service disruption / Information disclosure / None

Prioritize critical and high findings. Be specific about attack vectors.
"##;

/// Create the /security-review command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "security-review".to_string(),
            aliases: vec!["sec-review".to_string(), "security".to_string()],
            description: "OWASP-aligned security audit of code changes".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[file or path]".to_string()),
            when_to_use: Some(
                "Security audit before deploying or when handling sensitive data".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Running security review...".to_string(),
        content_length: 2500,
        arg_names: vec!["target".to_string()],
        allowed_tools: vec![
            "Bash(git diff:*)".to_string(),
            "Bash(git diff --stat:*)".to_string(),
            "Bash(git diff --cached:*)".to_string(),
            "Bash(git status:*)".to_string(),
            "Bash(cargo audit:*)".to_string(),
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(SECURITY_REVIEW_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_review_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "security-review");
        assert!(cmd.aliases().contains(&"sec-review".to_string()));
    }

    #[test]
    fn test_security_review_structure() {
        let cmd = command();
        assert!(!cmd.description().is_empty());
    }
}
