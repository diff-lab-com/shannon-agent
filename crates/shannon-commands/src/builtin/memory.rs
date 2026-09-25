//! /memory command — guidance for the curated memory system.
//!
//! Shannon's curated memory is a single engine-managed store: JSONL files
//! under `~/.shannon/memories/`, injected into every prompt (ADR-0010).
//! Writes go through the `MemorySave` / `MemoryForget` tools (or the REPL's
//! `/remember` / `/forget`). This command exists to *inform the model* of
//! that contract — an earlier revision taught a different on-disk schema
//! (`~/.shannon/memory/*.json`) that the engine never read, so following it
//! produced orphan files.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

const MEMORY_PROMPT: &str = r##"
Manage cross-session memory for persistent context.

Arguments: {args}

## How memory works (read carefully)

Memory is engine-managed. Do NOT create, read, or edit memory files yourself —
no Bash cat/echo, no Read/Write of `~/.shannon/...`. Durable facts are saved
with the **MemorySave** tool and removed with the **MemoryForget** tool.
Everything you save is injected into future sessions automatically.

- Facts the user states about themselves or the project ("always deploy with
  cargo dist", "we use trunk-based branches") → MemorySave.
- The user's explicit request to remember something → MemorySave verbatim.
- Transient task state, code snippets, or anything already written in project
  instruction files (CLAUDE.md / SHANNON.md) → do NOT save.
- Never save credentials, API keys, or tokens (secrets are redacted, but do
  not rely on that).

## Model-facing operations

- **MemorySave** {{"content": "...", "category": "Preference|Pattern|Decision|Error|Context", "tags": [...], "global": false}}
  — save one concise, self-contained fact. Set `"global": true` only for
  preferences that apply across every project (editor choice, language
  habits).
- **MemoryForget** {{"id": "<id-prefix>"}}
  — delete a stale or wrong entry (prefix as shown by the save result).

## User-facing commands (mention, don't execute)

`/remember [--global] <text>`, `/recall [--all] [query]`, `/forget <id>`,
`/memory cleanup`, `/memory doctor`.
"##;

/// Create the /memory command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "memory".to_string(),
            // "remember" is the REPL's native command (repl/commands/memory.rs)
            // and previously collided with this alias, so it is not repeated
            // here.
            aliases: vec!["mem".to_string()],
            description: "Manage cross-session persistent memory".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[save|list|search|delete|clear|export|auto] [text]".to_string()),
            when_to_use: Some(
                "Save important context that should persist across sessions".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading memory...".to_string(),
        content_length: 1200,
        arg_names: vec!["action".to_string(), "text".to_string()],
        // Memory writes go through the MemorySave/MemoryForget tools, which
        // are already permission-gated; no shell/file access is needed and
        // granting it here previously taught the model to hand-edit a store
        // the engine never reads.
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(MEMORY_PROMPT.to_string()),
    }))
}
