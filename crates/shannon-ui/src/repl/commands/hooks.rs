//! Hook and custom command management handlers

use super::super::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;

/// Re-scan custom command directories, deduplicate, and register all commands.
pub(crate) fn reload_custom_commands(repl: &mut Repl) {
    use shannon_commands::{Command, CommandBase, ExecutionContext, PromptCommand};
    use std::collections::HashMap;

    let cwd = std::env::current_dir().unwrap_or_default();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    dirs.push(cwd.join(".claude").join("commands"));
    dirs.push(cwd.join(".shannon").join("commands"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".claude").join("commands"));
        dirs.push(home.join(".shannon").join("commands"));
    }

    let mut custom_commands: Vec<super::super::CustomCommandEntry> = Vec::new();
    for dir in &dirs {
        super::super::collect_custom_commands(dir, "", &mut custom_commands);
    }
    super::super::dedup_custom_commands(&mut custom_commands);

    let count = custom_commands.len();
    for entry in &custom_commands {
        let description = entry
            .description
            .clone()
            .unwrap_or_else(|| format!("Custom command (from {})", entry.path.display()));
        let arg_names = if entry.arguments.is_empty() {
            vec!["$ARGUMENTS".to_string()]
        } else {
            entry.arguments.clone()
        };
        let argument_hint = if entry.arguments.is_empty() {
            Some("$ARGUMENTS".to_string())
        } else {
            Some(entry.arguments.join(" "))
        };
        let command = Command::Prompt(Box::new(PromptCommand {
            base: CommandBase {
                name: entry.name.clone(),
                aliases: Vec::new(),
                description,
                has_user_specified_description: entry.description.is_some(),
                availability: vec![shannon_commands::CommandAvailability::All],
                source: shannon_commands::CommandSource::Builtin,
                is_enabled: true,
                is_hidden: false,
                argument_hint,
                when_to_use: None,
                version: None,
                disable_model_invocation: false,
                user_invocable: true,
                is_workflow: false,
                immediate: false,
                is_sensitive: false,
                user_facing_name: None,
            },
            progress_message: format!("Running /{}...", entry.name),
            content_length: entry.template.len(),
            arg_names,
            allowed_tools: entry.allowed_tools.clone(),
            model: entry.model.clone(),
            hooks: HashMap::new(),
            context: ExecutionContext::Inline,
            agent: entry.agent.clone(),
            paths: Vec::new(),
            prompt_template: Some(entry.template.clone()),
        }));
        repl.command_registry.register_sync(command);
    }
    repl.chat.add_message(
        ChatRole::System,
        format!("Reloaded {count} custom command(s)."),
    );
}

pub(crate) fn handle_hooks(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::hooks::HookManager;

    let mut mgr = HookManager::new();
    if let Err(e) = mgr.load() {
        repl.chat.add_message(
            ChatRole::System,
            format!("No hooks configured.\n\nConfig paths checked:\n  User: {}\n  Project: {}\n\nError: {e}\n\nCreate ~/.shannon/hooks.json or .shannon/hooks.json to configure hooks.",
                mgr.user_config_path().display(),
                mgr.project_config_path().display()),
        );
        return Ok(());
    }

    let subcmd = args.split_whitespace().next().unwrap_or("");

    match subcmd {
        "reload" | "refresh" => {
            let mut mgr2 = HookManager::new();
            match mgr2.load() {
                Ok(()) => {
                    repl.chat
                        .add_message(ChatRole::System, t!("commands.hooks.reloaded").to_string());
                }
                Err(e) => {
                    super::set_error(repl, &format!("reloading hooks: {e}"));
                }
            }
            return Ok(());
        }
        "path" | "paths" => {
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Hook config paths:\n  User: {}\n  Project: {}",
                    mgr.user_config_path().display(),
                    mgr.project_config_path().display()
                ),
            );
            return Ok(());
        }
        _ => {}
    }

    let hf = mgr.hooks_file();
    let event_types = mgr.configured_event_types();

    if event_types.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "No hooks configured.\n\nConfig paths:\n  User: {}\n  Project: {}",
                mgr.user_config_path().display(),
                mgr.project_config_path().display()
            ),
        );
        return Ok(());
    }

    let mut output = String::from("Configured Hooks:\n\n");
    for event_type in &event_types {
        let key = format!("{event_type:?}");
        output.push_str(&format!("  {key}:\n"));
        if let Some(configs) = hf.hooks.get(&key) {
            for (i, cfg) in configs.iter().enumerate() {
                output.push_str(&format!(
                    "    [{}] matcher: \"{}\" ({} hook(s))\n",
                    i + 1,
                    cfg.matcher,
                    cfg.hooks.len()
                ));
                for hook in &cfg.hooks {
                    let blocking = if hook.blocking {
                        "blocking"
                    } else {
                        "non-blocking"
                    };
                    let timeout = hook.timeout_duration();
                    output.push_str(&format!("      command: {}\n", hook.command));
                    output.push_str(&format!(
                        "      mode: {blocking}, timeout: {}s\n",
                        timeout.as_secs()
                    ));
                }
            }
        }
    }

    output.push_str(&format!(
        "\nPaths: {} | {}",
        mgr.user_config_path().display(),
        mgr.project_config_path().display()
    ));
    output.push_str("\n\nUsage: /hooks [reload|path]");

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

pub(crate) fn handle_commands(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // /commands edit <name> — open command file in $EDITOR
    if let Some(cmd_name) = trimmed.strip_prefix("edit ") {
        let name = cmd_name.trim();
        if name.is_empty() {
            repl.chat
                .add_message(ChatRole::System, "Usage: /commands edit <name>".to_string());
            return Ok(());
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut search_dirs: Vec<std::path::PathBuf> = Vec::new();
        search_dirs.push(cwd.join(".claude").join("commands"));
        search_dirs.push(cwd.join(".shannon").join("commands"));
        if let Some(home) = dirs::home_dir() {
            search_dirs.push(home.join(".claude").join("commands"));
            search_dirs.push(home.join(".shannon").join("commands"));
        }
        // Search for matching command file
        let mut all_cmds: Vec<super::super::CustomCommandEntry> = Vec::new();
        for dir in &search_dirs {
            super::super::collect_custom_commands(dir, "", &mut all_cmds);
        }
        // Support subdirectory-prefixed names like "project:foo"
        let entry = all_cmds.iter().find(|e| e.name == name).or_else(|| {
            // Also try matching just the stem (without prefix)
            all_cmds
                .iter()
                .find(|e| e.name.ends_with(&format!(":{name}")) || e.name == name)
        });
        match entry {
            Some(e) => {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                let path = e.path.clone();
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Opening {} in {editor}...", path.display()),
                );
                // Drop terminal raw mode before spawning editor
                crossterm::terminal::disable_raw_mode()?;
                let status = std::process::Command::new(&editor).arg(&path).status();
                // Restore terminal
                crossterm::terminal::enable_raw_mode()?;
                match status {
                    Ok(s) if s.success() => {
                        repl.chat.add_message(
                            ChatRole::System,
                            "Editor closed. Use /commands reload to apply changes.".to_string(),
                        );
                    }
                    Ok(s) => {
                        repl.chat.add_message(
                            ChatRole::System,
                            format!("Editor exited with status: {s}"),
                        );
                    }
                    Err(e) => {
                        super::set_error(repl, &format!("launching editor: {e}"));
                    }
                }
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!(
                        "Command '{name}' not found. Use /commands to list available commands."
                    ),
                );
            }
        }
        return Ok(());
    }

    // /commands create <name> — create a new command file
    if let Some(cmd_name) = trimmed.strip_prefix("create ") {
        let name = cmd_name.trim();
        if name.is_empty() {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /commands create <name>".to_string(),
            );
            return Ok(());
        }
        // Sanitize name: only allow alphanumeric, dash, underscore, colon (for subdirs)
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ':')
        {
            repl.chat.add_message(ChatRole::System, "Command name can only contain letters, numbers, '-', '_', and ':' (for subdirectories).".to_string());
            return Ok(());
        }
        let cwd = match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                super::set_error(repl, &format!("cannot determine current directory: {e}"));
                return Ok(());
            }
        };
        let dir = cwd.join(".claude").join("commands");
        // Handle subdirectory prefix: "project:foo" → .claude/commands/project/foo.md
        let (sub_dir, file_name) = if let Some((prefix, stem)) = name.split_once(':') {
            (Some(prefix), stem)
        } else {
            (None, name)
        };
        let cmd_dir = if let Some(sd) = sub_dir {
            dir.join(sd)
        } else {
            dir.clone()
        };
        let file_path = cmd_dir.join(format!("{file_name}.md"));

        if file_path.exists() {
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Command '{name}' already exists at {}. Use /commands edit {name} to edit it.",
                    file_path.display()
                ),
            );
            return Ok(());
        }

        // Create directory and default template
        if let Err(e) = std::fs::create_dir_all(&cmd_dir) {
            super::set_error(
                repl,
                &format!("creating directory {}: {e}", cmd_dir.display()),
            );
            return Ok(());
        }

        let template = format!("---\ndescription: {name} command\n---\n\n$ARGUMENTS\n");
        if let Err(e) = std::fs::write(&file_path, &template) {
            super::set_error(repl, &format!("writing {}: {e}", file_path.display()));
            return Ok(());
        }

        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Created command '{name}' at {}. Use /commands edit {name} to customize.",
                file_path.display()
            ),
        );
        // Auto-reload to register the new command
        reload_custom_commands(repl);
        return Ok(());
    }

    // /commands delete <name> — delete a command file
    if let Some(cmd_name) = trimmed.strip_prefix("delete ") {
        let name = cmd_name.trim();
        if name.is_empty() {
            repl.chat.add_message(
                ChatRole::System,
                "Usage: /commands delete <name>".to_string(),
            );
            return Ok(());
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut search_dirs: Vec<std::path::PathBuf> = Vec::new();
        search_dirs.push(cwd.join(".claude").join("commands"));
        search_dirs.push(cwd.join(".shannon").join("commands"));
        if let Some(home) = dirs::home_dir() {
            search_dirs.push(home.join(".claude").join("commands"));
            search_dirs.push(home.join(".shannon").join("commands"));
        }
        let mut all_cmds: Vec<super::super::CustomCommandEntry> = Vec::new();
        for dir in &search_dirs {
            super::super::collect_custom_commands(dir, "", &mut all_cmds);
        }
        let entry = all_cmds.iter().find(|e| e.name == name);
        match entry {
            Some(e) => {
                let path = e.path.clone();
                if let Err(err) = std::fs::remove_file(&path) {
                    super::set_error(repl, &format!("deleting {}: {err}", path.display()));
                } else {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Deleted command '{name}' ({}).", path.display()),
                    );
                    // Auto-reload to update registry
                    reload_custom_commands(repl);
                }
            }
            None => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!(
                        "Command '{name}' not found. Use /commands to list available commands."
                    ),
                );
            }
        }
        return Ok(());
    }

    if trimmed == "reload" {
        reload_custom_commands(repl);
        return Ok(());
    }

    // Default: list all custom commands
    let registry = &repl.command_registry;
    let all = repl.runtime.block_on(registry.list_enabled());

    // MCP prompt commands
    let mcp_prompts: Vec<_> = all
        .iter()
        .filter(|c| c.name().starts_with("mcp__") && c.name().contains("__"))
        .collect();

    let mut msg = String::from("Commands:\n\n");

    // Custom file-based commands
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    dirs.push(cwd.join(".claude").join("commands"));
    dirs.push(cwd.join(".shannon").join("commands"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".claude").join("commands"));
        dirs.push(home.join(".shannon").join("commands"));
    }

    let mut found: Vec<super::super::CustomCommandEntry> = Vec::new();
    for dir in &dirs {
        super::super::collect_custom_commands(dir, "", &mut found);
    }
    super::super::dedup_custom_commands(&mut found);

    if found.is_empty() {
        msg.push_str("  No custom commands found.\n\n");
        msg.push_str("  Create .md files in .claude/commands/ or .shannon/commands/\n");
        msg.push_str("  or use /commands create <name> to create one.\n");
    } else {
        msg.push_str(&format!("  Custom commands ({}):\n", found.len()));
        for entry in &found {
            let desc = entry.description.as_deref().unwrap_or("");
            let desc_suffix = if desc.is_empty() {
                String::new()
            } else {
                format!(" — {desc}")
            };
            msg.push_str(&format!("    /{}{}\n", entry.name, desc_suffix));
        }
    }

    // MCP prompt commands
    if !mcp_prompts.is_empty() {
        msg.push_str(&format!(
            "\n  MCP prompt commands ({}):\n",
            mcp_prompts.len()
        ));
        for cmd in &mcp_prompts {
            msg.push_str(&format!(
                "    /{}  — {}\n",
                cmd.name(),
                cmd.base().description
            ));
        }
    }

    msg.push_str("\nUsage: /commands [reload]");
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}
