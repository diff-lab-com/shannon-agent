//! /monitor command - Run background commands with streaming output
//!
//! Monitor allows running long-lived commands (test suites, build watchers,
//! log tailing) in the background with output streaming back to the conversation.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

const MONITOR_PROMPT: &str = r##"
Run a command in the background and monitor its output.

Arguments: {args}

## Usage

- **/monitor <command>** — Start a background command and report its output
- **/monitor status** — Check status of running background commands
- **/monitor logs [id]** — Show recent output from a background command
- **/monitor stop <id>** — Stop a running background command

## How It Works

When you run `/monitor <command>`:

1. Use Bash to start the command with output redirection:
   ```bash
   # For one-shot commands that may take a while:
   cargo test 2>&1 | tee /tmp/shannon_monitor_$(date +%s).log

   # For long-running services:
   cargo watch -x test 2>&1 | tee /tmp/shannon_monitor_watch.log &
   ```

2. Report the process ID and log file location
3. Periodically check the log file for new output
4. Summarize the output when the command completes or on request

## Common Use Cases

- `/monitor cargo test` — Run test suite and report results
- `/monitor cargo build` — Build and report errors
- `/monitor cargo clippy` — Run linter and report warnings
- `/monitor "cargo test -- --test-threads=1"` — Run tests sequentially
- `/monitor "tail -f /var/log/app.log"` — Monitor a log file

## Tips

- The command runs in the background — you can continue working
- Check status anytime with `/monitor status`
- Output is captured to log files in /tmp/
- Commands are automatically cleaned up when the session ends

Keep the monitoring output concise — summarize rather than dumping raw output.
"##;

/// Create the /monitor command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "monitor".to_string(),
            aliases: vec!["watch".to_string(), "bg".to_string()],
            description: "Run commands in background with streaming output".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("<command> | status | logs | stop".to_string()),
            when_to_use: Some(
                "Run long commands in background while continuing to work".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Starting background monitor...".to_string(),
        content_length: 2000,
        arg_names: vec!["command".to_string()],
        allowed_tools: vec!["Bash".to_string()],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(MONITOR_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "monitor");
        assert!(cmd.aliases().contains(&"watch".to_string()));
    }
}
