//! /doctor command - Run system diagnostics

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Doctor prompt template
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
        // Verify the command has a non-empty prompt template
        if let crate::command::Command::Prompt(pc) = cmd {
            assert!(pc.prompt_template.is_some());
            let template = pc.prompt_template.unwrap();
            assert!(template.contains("diagnostics"));
            assert!(template.contains("API Key"));
        } else {
            panic!("Expected PromptCommand");
        }
    }
}
