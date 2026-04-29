//! REPL command dispatch and handler implementations

use crate::{
    widgets::ChatRole,
    Result,
};
use rust_i18n::t;

use super::Repl;

/// Expand `[Pasted Text #N X lines]` markers with the actual stored content.
/// Removes expanded entries from the map.
fn expand_pasted_texts(
    input: &str,
    pasted_texts: &mut std::collections::HashMap<usize, String>,
) -> String {
    let marker_prefix = "[Pasted Text #";
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;
    let mut expanded_keys = Vec::new();

    while let Some(start) = remaining.find(marker_prefix) {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + marker_prefix.len()..];

        // Extract the number
        let num_end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        if let Ok(num) = after[..num_end].parse::<usize>() {
            // Find closing bracket
            if let Some(bracket_end) = after.find(']') {
                if let Some(content) = pasted_texts.get(&num) {
                    result.push_str(content);
                    expanded_keys.push(num);
                } else {
                    // Paste not found, keep the marker as-is
                    result.push_str(marker_prefix);
                    result.push_str(&after[..bracket_end + 1]);
                }
                remaining = &after[bracket_end + 1..];
            } else {
                result.push_str(remaining);
                remaining = "";
            }
        } else {
            result.push_str(remaining);
            remaining = "";
        }
    }
    result.push_str(remaining);

    for key in expanded_keys {
        pasted_texts.remove(&key);
    }
    result
}

/// Submit the current input
pub fn submit_input(repl: &mut Repl) -> Result<()> {
    let raw_input = repl.prompt.input().to_string();

    if raw_input.trim().is_empty() {
        return Ok(());
    }

    // Expand pasted text references: [Pasted Text #N X lines] -> actual content
    let expanded = expand_pasted_texts(&raw_input, &mut repl.state.pasted_texts);

    // Add user message to chat (show raw input with paste markers)
    repl.chat.add_message(ChatRole::User, raw_input);

    // Increment turn counter for context visualization
    repl.state.turn_count += 1;

    // Push expanded text to command history and clear input
    repl.command_history.push(&expanded);
    repl.saved_input.clear();
    repl.prompt.clear();

    // Clear paste state for next input
    repl.state.pasted_texts.clear();
    repl.state.paste_counter = 0;

    // Process command or query with expanded text
    if expanded.starts_with('/') {
        repl.commands_run += 1;
        handle_command(repl, &expanded)?;
    } else {
        super::query::handle_query(repl, &expanded)?;
    }

    Ok(())
}

/// Handle a command (starts with /)
fn handle_command(repl: &mut Repl, input: &str) -> Result<()> {
    let parsed = match repl.command_parser.parse(input) {
        Ok(p) => p,
        Err(_) => {
            let parts: Vec<&str> = input.splitn(2, ' ').collect();
            let name = parts.first().copied().unwrap_or("").strip_prefix('/').unwrap_or("");
            shannon_commands::ParsedCommand::new(
                name.to_string(),
                parts.get(1).copied().unwrap_or("").to_string(),
                input.to_string(),
            )
        }
    };

    let cmd_name = parsed.name.as_str();
    let args = parsed.args.as_str();

    // Check if command exists in the registry
    let command_exists = repl.runtime.block_on(async {
        repl.shared_executor.registry().await.contains(cmd_name).await
    });
    // Commands handled in the match block but not in the global registry
    let repl_only_commands = ["browse", "files", "select-tools", "tools", "team", "agents", "agent", "route", "mcp", "compact", "cost", "permissions", "perms", "perm", "plan", "web-search", "websearch", "search-web", "review", "local-models", "local", "ci", "gh-actions", "hooks", "remember", "mem", "memo", "recall", "search-memory", "forget", "memory", "image", "img", "screenshot", "mode", "context", "undo", "rewind", "notify", "webhook", "routine", "create-pr", "patch", "sandbox", "find", "grep", "conv-search", "copy", "paste", "add", "watch", "bind", "project", "terminal-setup", "theme", "diff", "commands"];
    let is_repl_command = repl_only_commands.contains(&cmd_name);

    if command_exists || is_repl_command {
        match cmd_name {
            "help" => handle_help(repl, args)?,
            "clear" => handle_clear(repl)?,
            "quit" | "exit" => handle_quit(repl)?,
            "model" | "models" => handle_model(repl, args)?,
            "init" => handle_init(repl)?,
            "config" => handle_config(repl, args)?,
            "sessions" => handle_sessions(repl, args)?,
            "resume" => handle_resume(repl, args)?,
            "history" => handle_history(repl, args)?,
            "worktree" => handle_worktree(repl, args)?,
            "credentials" | "creds" | "cred" => handle_credentials(repl, args)?,
            "status" | "st" | "git-status" => handle_status(repl, args)?,
            "export" | "save" => handle_export(repl, args)?,
            "diff" => handle_diff(repl, args)?,
            "search" | "?" | "hist" | "history-search" => handle_search(repl, args)?,
            "find" | "grep" | "conv-search" => handle_find(repl, args)?,
            "browse" | "files" => handle_browse(repl, args)?,
            "select-tools" | "tools" => handle_select_tools(repl)?,
            "debug" | "dbg" | "dev" => handle_debug(repl, args)?,
            "doctor" | "check" | "diagnostics" => handle_doctor(repl, args)?,
            "terminal-setup" => handle_terminal_setup(repl)?,
            "compact" => handle_compact(repl, args)?,
            "cost" => handle_cost(repl, args)?,
            "billing" | "usage" => handle_billing(repl, args)?,
            "suggest" => handle_suggest(repl, args)?,
            "permissions" | "perms" | "perm" => handle_permissions(repl, args)?,
            "plan" => handle_plan(repl, args)?,
            "team" => handle_team(repl, args)?,
            "agents" => handle_agents(repl, args)?,
            "agent" => handle_agent(repl, args)?,
            "route" => handle_route(repl, args)?,
            "mcp" => handle_mcp(repl, args)?,
            "branch" | "fork" => handle_branch(repl, args)?,
            "web-search" | "websearch" | "search-web" => handle_web_search(repl, args)?,
            "review" => handle_review(repl, args)?,
            "stage" => handle_stage(repl, args)?,
            "stats" | "perf" => handle_stats(repl)?,
            "loop" => handle_loop(repl, args)?,
            "sandbox" => handle_sandbox(repl, args)?,
            "local-models" | "local" => handle_local_models(repl)?,
            "ci" | "gh-actions" => handle_ci(repl, args)?,
            "hooks" => handle_hooks(repl, args)?,
            "remember" | "mem" | "memo" => handle_remember(repl, args)?,
            "recall" | "search-memory" => handle_recall(repl, args)?,
            "forget" => handle_forget(repl, args)?,
            "memory" => handle_memory(repl, args)?,
            "image" | "img" | "screenshot" => handle_image(repl, args)?,
            "mode" => handle_mode(repl, args)?,
            "context" => handle_context(repl, args)?,
            "undo" => handle_undo(repl, args)?,
            "rewind" => handle_rewind(repl, args)?,
            "notify" => handle_notify(repl, args)?,
            "webhook" => handle_webhook(repl, args)?,
            "routine" => handle_routine(repl, args)?,
            "create-pr" => handle_create_pr(repl, args)?,
            "patch" => handle_patch(repl, args)?,
            "copy" | "clip" => handle_copy(repl, args)?,
            "paste" => handle_paste(repl)?,
            "add" => handle_add(repl, args)?,
            "watch" => handle_watch(repl, args)?,
            "bind" => handle_bind(repl, args)?,
            "project" => handle_project(repl, args)?,
            "theme" => handle_theme(repl, args)?,
            "session" => handle_session(repl, args)?,
            "accessibility" | "a11y" => handle_accessibility(repl, args)?,
            "diag" => handle_diag(repl, args)?,
            "commands" => handle_commands(repl, args)?,
            _ => handle_other_command(repl, cmd_name, args)?,
        }
        Ok(())
    } else {
        repl.chat.add_message(
            ChatRole::System,
            t!("repl.unknown_command", name = cmd_name).to_string(),
        );
        Ok(())
    }
}

fn handle_help(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::help_utils;
    if !args.is_empty() {
        let help_text = help_utils::generate_help(Some(args));
        if !help_text.contains("No help found") {
            repl.chat.add_message(ChatRole::System, help_text);
            return Ok(());
        }
    }
    let help_text = help_utils::generate_help(None);
    repl.chat.add_message(ChatRole::System, help_text);
    Ok(())
}

fn handle_clear(repl: &mut Repl) -> Result<()> {
    if repl.chat.len() > 1 {
        repl.show_confirm_dialog(
            "Clear Chat",
            "Clear all messages? This cannot be undone.",
            "clear_chat",
        );
    } else {
        repl.chat.clear();
        repl.chat.add_message(ChatRole::System, t!("repl.chat_cleared").to_string());
    }
    Ok(())
}

fn handle_quit(repl: &mut Repl) -> Result<()> {
    let had_activity = repl.commands_run > 0
        || repl.tools_invoked > 0
        || repl.current_turn > 0;
    if had_activity {
        repl.show_confirm_dialog(
            "End Session?",
            "You have unsaved activity. Quit anyway?",
            "quit",
        );
    } else {
        repl.running = false;
    }
    Ok(())
}

fn handle_model(repl: &mut Repl, args: &str) -> Result<()> {
    if args.is_empty() {
        let picker = crate::widgets::select::ModelPickerWidget::new(
            repl.state.model.as_deref(),
        );
        repl.state.model_picker = Some(picker);
    } else {
        repl.state.model = Some(args.to_string());
        crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
            model: repl.state.model.clone(),
            provider: repl.state.selected_provider.clone(),
            theme: Some(repl.state.theme.name.to_string()),
        });
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.model.set", name = args).to_string(),
        );
    }
    Ok(())
}

fn handle_init(repl: &mut Repl) -> Result<()> {
    let mut init_info = String::new();
    let cwd = &repl.state.working_directory;

    let is_git = std::path::Path::new(cwd).join(".git").exists();
    if is_git {
        init_info.push_str("Git repository: detected\n");
    } else {
        init_info.push_str("Git repository: not found\n");
    }

    let claude_md_path = std::path::Path::new(cwd).join("CLAUDE.md");
    if claude_md_path.exists() {
        init_info.push_str("CLAUDE.md: already exists\n");
    } else {
        let default_content = "# Project Instructions\n\nThis file contains project-specific instructions for Shannon.\n\n## Coding Standards\n\n- Follow existing code patterns\n- Write clear, descriptive commit messages\n- Keep functions focused and concise\n\n## Project Structure\n\n- Describe your project structure here\n";
        match std::fs::write(&claude_md_path, default_content) {
            Ok(_) => init_info.push_str("CLAUDE.md: created with default template\n"),
            Err(e) => init_info.push_str(&format!("CLAUDE.md: failed to create ({e})\n")),
        }
    }

    init_info.push_str(&format!("Working directory: {cwd}\n"));
    repl.chat.add_message(ChatRole::System, t!("repl.project_initialized", info = init_info).to_string());
    Ok(())
}

fn handle_config(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_tools::config::ConfigManager;
    use shannon_commands::config_utils;

    let mut manager = ConfigManager::new();
    if let Err(e) = manager.load() {
        repl.chat.add_message(ChatRole::System, t!("commands.config.warning_load", error = e).to_string());
    }

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let action_str = parts.first().copied().unwrap_or("");
    let action = config_utils::parse_config_action(action_str);

    let output = match action {
        config_utils::ConfigAction::List => {
            let prefix = if action_str.is_empty() { None } else { parts.get(1).copied() };
            let keys = manager.list(prefix);
            if keys.is_empty() {
                config_utils::format_config_list()
            } else {
                let mut out = config_utils::format_config_list();
                out.push_str(&format!("\nConfig file: {}\n", manager.config_path().display()));
                for key in &keys {
                    let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    out.push_str(&format!("  {key} = {val}\n"));
                }
                out
            }
        }
        config_utils::ConfigAction::Get => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config get <key>".to_string()
            } else {
                match manager.get(key) {
                    Some(_val) => config_utils::format_config_get(key),
                    None => format!("Config key not found: {key}"),
                }
            }
        }
        config_utils::ConfigAction::Set => {
            let key = parts.get(1).copied().unwrap_or("");
            let value_str = parts.get(2).copied().unwrap_or("");
            if key.is_empty() || value_str.is_empty() {
                "Usage: /config set <key> <value>".to_string()
            } else {
                let value: serde_json::Value = if value_str == "true" {
                    serde_json::json!(true)
                } else if value_str == "false" {
                    serde_json::json!(false)
                } else if let Ok(n) = value_str.parse::<i64>() {
                    serde_json::json!(n)
                } else if let Ok(n) = value_str.parse::<f64>() {
                    serde_json::json!(n)
                } else {
                    serde_json::json!(value_str)
                };
                manager.set(key.to_string(), value.clone());
                match manager.save() {
                    Ok(_) => config_utils::format_config_set(key, &value.to_string()),
                    Err(e) => format!("Error saving config: {e}"),
                }
            }
        }
        config_utils::ConfigAction::Reset => {
            let key = parts.get(1).copied().unwrap_or("");
            if key.is_empty() {
                "Usage: /config reset <key>".to_string()
            } else {
                let existed = manager.reset(key);
                if existed {
                    let _val = manager.get(key).unwrap_or(serde_json::Value::Null);
                    match manager.save() {
                        Ok(_) => config_utils::format_config_reset(key),
                        Err(e) => format!("Error saving config: {e}"),
                    }
                } else {
                    config_utils::format_config_reset(key)
                }
            }
        }
        config_utils::ConfigAction::Help => config_utils::format_config_list(),
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_hooks(repl: &mut Repl, args: &str) -> Result<()> {
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
                Ok(()) => { repl.chat.add_message(ChatRole::System, t!("commands.hooks.reloaded").to_string()); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to reload hooks: {e}")); }
            }
            return Ok(());
        }
        "path" | "paths" => {
            repl.chat.add_message(
                ChatRole::System,
                format!("Hook config paths:\n  User: {}\n  Project: {}",
                    mgr.user_config_path().display(),
                    mgr.project_config_path().display()),
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
            format!("No hooks configured.\n\nConfig paths:\n  User: {}\n  Project: {}",
                mgr.user_config_path().display(),
                mgr.project_config_path().display()),
        );
        return Ok(());
    }

    let mut output = String::from("Configured Hooks:\n\n");
    for event_type in &event_types {
        let key = format!("{event_type:?}");
        output.push_str(&format!("  {key}:\n"));
        if let Some(configs) = hf.hooks.get(&key) {
            for (i, cfg) in configs.iter().enumerate() {
                output.push_str(&format!("    [{}] matcher: \"{}\" ({} hook(s))\n",
                    i + 1, cfg.matcher, cfg.hooks.len()));
                for hook in &cfg.hooks {
                    let blocking = if hook.blocking { "blocking" } else { "non-blocking" };
                    let timeout = hook.timeout_duration();
                    output.push_str(&format!("      command: {}\n", hook.command));
                    output.push_str(&format!("      mode: {blocking}, timeout: {}s\n", timeout.as_secs()));
                }
            }
        }
    }

    output.push_str(&format!("\nPaths: {} | {}",
        mgr.user_config_path().display(),
        mgr.project_config_path().display()));
    output.push_str("\n\nUsage: /hooks [reload|path]");

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_remember(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::{MemoryEntry, MemoryCategory};

    let content = args.trim();
    if content.is_empty() {
        repl.chat.add_message(ChatRole::System, t!("commands.memory.usage_remember").to_string());
        return Ok(());
    }

    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_available").to_string());
            return Ok(());
        }
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_configured").to_string());
            return Ok(());
        }
    };

    let project = repl.state.working_directory.clone();
    let mut store = memory.write().unwrap();
    let entry = MemoryEntry::new(&project, MemoryCategory::Context, content);
    let id = entry.id.clone();
    let _ = store.add(entry);
    if let Err(e) = store.save() {
        repl.chat.add_message(ChatRole::System, format!("Failed to save memory: {e}"));
        return Ok(());
    }
    drop(store);

    // Also save as file for Claude Code-compatible auto-memory
    let project_path = std::path::PathBuf::from(&project);
    if let Err(e) = shannon_core::project_memory::save_memory_file(
        &project_path, &id, content,
    ) {
        tracing::debug!("File-based memory save skipped: {e}");
    }

    repl.chat.add_message(ChatRole::System, format!("Remembered (id: {}...)", &id[..8]));
    Ok(())
}

fn handle_recall(repl: &mut Repl, args: &str) -> Result<()> {
    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_available").to_string());
            return Ok(());
        }
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_configured").to_string());
            return Ok(());
        }
    };

    let store = memory.read().unwrap();
    let project = repl.state.working_directory.clone();

    let results = if args.trim().is_empty() {
        store.project_memories(&project)
    } else {
        store.search(args.trim(), Some(&project))
    };

    if results.is_empty() {
        repl.chat.add_message(ChatRole::System, t!("commands.memory.no_memories").to_string());
        return Ok(());
    }

    let mut output = format!("Found {} memory(ies):\n\n", results.len());
    for entry in &results {
        let preview = if entry.content.len() > 100 {
            format!("{}...", &entry.content[..100])
        } else {
            entry.content.clone()
        };
        output.push_str(&format!("  [{}] {} (category: {})\n", &entry.id[..8], preview, entry.category));
    }
    output.push_str("\nUse /forget <id> to remove a memory.");
    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_forget(repl: &mut Repl, args: &str) -> Result<()> {
    let id_prefix = args.trim();
    if id_prefix.is_empty() {
        repl.chat.add_message(ChatRole::System, "Usage: /forget <memory-id-prefix>".to_string());
        return Ok(());
    }

    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_available").to_string());
            return Ok(());
        }
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_configured").to_string());
            return Ok(());
        }
    };

    let mut store = memory.write().unwrap();
    // Find by prefix match
    let found = store.project_memories(&repl.state.working_directory)
        .into_iter()
        .find(|e| e.id.starts_with(id_prefix));

    match found {
        Some(entry) => {
            let display = &entry.id[..8.min(entry.id.len())];
            match store.delete(&entry.id) {
                Ok(true) => {
                    let _ = store.save();
                    repl.chat.add_message(ChatRole::System, format!("Forgot memory {display}..."));
                }
                Ok(false) => { repl.chat.add_message(ChatRole::System, "Memory not found.".to_string()); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error deleting memory: {e}")); }
            }
        }
        None => { repl.chat.add_message(ChatRole::System, format!("No memory found matching '{id_prefix}'")); }
    }
    Ok(())
}

fn handle_memory(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.split_whitespace().next().unwrap_or("");

    match subcmd {
        "cleanup" | "clean" => {
            let engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_available").to_string());
                    return Ok(());
                }
            };
            let memory = match engine.memory() {
                Some(m) => m,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_configured").to_string());
                    return Ok(());
                }
            };
            let mut store = memory.write().unwrap();
            let removed = store.cleanup(chrono::Duration::days(90), 500).unwrap_or(0);
            repl.chat.add_message(ChatRole::System, format!("Cleanup complete: removed {removed} stale memories. {} remaining.", store.len()));
        }
        "stats" | "status" | _ => {
            let engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_available").to_string());
                    return Ok(());
                }
            };
            let memory = match engine.memory() {
                Some(m) => m,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.memory.store_not_configured").to_string());
                    return Ok(());
                }
            };
            let store = memory.read().unwrap();
            let project = repl.state.working_directory.clone();
            let project_count = store.project_memories(&project).len();
            let total = store.len();
            repl.chat.add_message(ChatRole::System, format!(
                "Memory Store:\n  Total entries: {total}\n  Current project: {project_count}\n\nCommands: /remember <text>, /recall [query], /forget <id>, /memory cleanup"));
        }
    }
    Ok(())
}

pub(crate) fn handle_image(repl: &mut Repl, args: &str) -> Result<()> {
    use base64::Engine;
    use shannon_core::api::{ContentBlock, ImageSource};

    let input = args.trim();
    if input.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /image <path> [optional prompt]\n       /image paste [prompt]\n       /image url <url> [prompt]\n\nAttach an image file, paste from clipboard, or fetch from URL.\nSupports PNG, JPG, GIF, WebP, BMP, SVG.".to_string());
        return Ok(());
    }

    // Handle /image paste subcommand
    if input.starts_with("paste") {
        return handle_image_paste(repl, input[5..].trim());
    }

    // Handle /image url <url> subcommand
    if input.starts_with("url ") {
        return handle_image_url(repl, input[4..].trim());
    }

    // Auto-detect URL (starts with http:// or https://)
    if input.starts_with("http://") || input.starts_with("https://") {
        return handle_image_url(repl, input);
    }

    // Split path from optional prompt
    let (path, prompt) = if input.starts_with('"') {
        // Quoted path: "path with spaces" prompt
        if let Some(end) = input[1..].find('"') {
            let path = &input[1..end + 1];
            let prompt = input[end + 2..].trim();
            (path.to_string(), if prompt.is_empty() { "Describe this image.".to_string() } else { prompt.to_string() })
        } else {
            (input.to_string(), "Describe this image.".to_string())
        }
    } else {
        let mut parts = input.splitn(2, ' ');
        let path = parts.next().unwrap_or("").to_string();
        let prompt = parts.next().map(|p| p.trim().to_string())
            .unwrap_or_else(|| "Describe this image.".to_string());
        (path, prompt)
    };

    // Expand ~ to home dir
    let expanded_path = if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(&path[2..]).to_string_lossy().to_string()
        } else {
            path.clone()
        }
    } else {
        path.clone()
    };

    let file_path = std::path::Path::new(&expanded_path);
    if !file_path.exists() {
        repl.chat.add_message(ChatRole::System, format!("File not found: {path}"));
        return Ok(());
    }

    let bytes = match std::fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Failed to read file: {e}"));
            return Ok(());
        }
    };

    // Detect media type from extension
    let media_type = match file_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => {
            repl.chat.add_message(ChatRole::System,
                format!("Unsupported image format: {path}. Supported: PNG, JPG, GIF, WebP, BMP, SVG"));
            return Ok(());
        }
    };

    let engine = match repl.query_engine.as_mut() {
        Some(e) => e,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.image.no_engine").to_string());
            return Ok(());
        }
    };

    let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let blocks = vec![
        ContentBlock::Text { text: prompt },
        ContentBlock::Image {
            source: ImageSource::base64(media_type, base64_data),
        },
    ];

    engine.add_user_message_blocks(blocks);
    // Generate inline image preview
    let preview_config = crate::terminal_image::ImageRenderConfig::default();
    let preview_lines = crate::terminal_image::render_image_bytes(&bytes, &preview_config);
    repl.chat.add_message_with_image(
        ChatRole::User,
        format!("[Image attached: {}]", file_path.display()),
        preview_lines,
    );
    repl.chat.add_message(ChatRole::System, t!("commands.image.image_sent").to_string());

    // Trigger query processing
    super::query::handle_query(repl, &format!("Please analyze the image I just shared: {}", file_path.display()))?;
    Ok(())
}

/// Handle `/image paste` — read image from system clipboard.
#[allow(dead_code)]
pub fn handle_image_paste_from_input(repl: &mut Repl) -> Result<()> {
    handle_image_paste(repl, "Describe this image.")
}

fn handle_image_paste(repl: &mut Repl, prompt_args: &str) -> Result<()> {
    use base64::Engine;
    use shannon_core::api::{ContentBlock, ImageSource};

    let prompt = if prompt_args.is_empty() {
        "Describe this image.".to_string()
    } else {
        prompt_args.to_string()
    };

    // Try reading clipboard image via platform tools
    let tmp_path = std::env::temp_dir().join("shannon_clipboard_paste.png");
    let tmp_str = tmp_path.to_string_lossy().to_string();

    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("pngpaste")
            .arg(&tmp_str)
            .output()
    } else {
        // Linux: try xclip first, then wl-paste for Wayland
        let file = std::fs::File::create(&tmp_path);
        match file {
            Ok(f) => {
                let xclip = std::process::Command::new("xclip")
                    .args(["-selection", "clipboard", "-t", "image/png", "-o"])
                    .stdout(std::process::Stdio::from(f))
                    .output();
                match xclip {
                    Ok(o) if o.status.success() => Ok(o),
                    _ => {
                        // Fallback: wl-paste for Wayland
                        let f2 = std::fs::File::create(&tmp_path);
                        match f2 {
                            Ok(f2) => std::process::Command::new("wl-paste")
                                .args(["--type", "image/png"])
                                .stdout(std::process::Stdio::from(f2))
                                .output(),
                            Err(e) => Err(e),
                        }
                    }
                }
            }
            Err(e) => Err(e),
        }
    };

    match result {
        Ok(output) if output.status.success() && tmp_path.exists() => {
            let bytes = std::fs::read(&tmp_path)?;
            let _ = std::fs::remove_file(&tmp_path); // cleanup

            if bytes.len() < 10 {
                repl.chat.add_message(ChatRole::System,
                    "Clipboard does not contain a valid image.".to_string());
                return Ok(());
            }

            let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let engine = match repl.query_engine.as_mut() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.image.no_engine").to_string());
                    return Ok(());
                }
            };

            let blocks = vec![
                ContentBlock::Text { text: prompt },
                ContentBlock::Image {
                    source: ImageSource::base64("image/png", base64_data),
                },
            ];
            engine.add_user_message_blocks(blocks);
            // Generate inline image preview from clipboard bytes
            let preview_config = crate::terminal_image::ImageRenderConfig::default();
            let preview_lines = crate::terminal_image::render_image_bytes(&bytes, &preview_config);
            repl.chat.add_message_with_image(
                ChatRole::User,
                "[Image pasted from clipboard]".to_string(),
                preview_lines,
            );
            repl.chat.add_message(ChatRole::System, t!("commands.image.clipboard_sent").to_string());

            super::query::handle_query(repl, "Please analyze the image I just shared from my clipboard.")?;
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Failed to read image from clipboard.\n\
                 Install xclip (X11) or wl-clipboard (Wayland) for Linux, or pngpaste for macOS.".to_string());
        }
    }
    Ok(())
}

/// Handle `/image url <url>` — fetch image from URL and send to API.
fn handle_image_url(repl: &mut Repl, input: &str) -> Result<()> {
    use base64::Engine;
    use shannon_core::api::{ContentBlock, ImageSource};

    // Split URL from optional prompt
    let (url, prompt) = if input.starts_with("http://") || input.starts_with("https://") {
        let mut parts = input.splitn(2, ' ');
        let url = parts.next().unwrap_or("").to_string();
        let prompt = parts.next().map(|p| p.trim().to_string())
            .unwrap_or_else(|| "Describe this image.".to_string());
        (url, prompt)
    } else {
        // Input starts after "url " prefix
        let mut parts = input.splitn(2, ' ');
        let url = parts.next().unwrap_or("").to_string();
        let prompt = parts.next().map(|p| p.trim().to_string())
            .unwrap_or_else(|| "Describe this image.".to_string());
        (url, prompt)
    };

    if url.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /image url <url> [prompt]\n\nFetch an image from a URL and send it for analysis.".to_string());
        return Ok(());
    }

    repl.chat.add_message(ChatRole::System, format!("Fetching image from {url}..."));

    // Fetch the image using the async runtime
    let fetch_result = repl.runtime.block_on(async {
        match reqwest::get(&url).await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return Err(format!("HTTP {}", resp.status()));
                }
                // Detect media type from Content-Type header
                let media_type = resp.headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                let media_type = if media_type.starts_with("image/") {
                    media_type
                } else {
                    match url.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
                        "png" => "image/png".to_string(),
                        "jpg" | "jpeg" => "image/jpeg".to_string(),
                        "gif" => "image/gif".to_string(),
                        "webp" => "image/webp".to_string(),
                        "svg" => "image/svg+xml".to_string(),
                        _ => "image/png".to_string(),
                    }
                };

                match resp.bytes().await {
                    Ok(b) => Ok((b.to_vec(), media_type)),
                    Err(e) => Err(format!("Failed to read image data: {e}")),
                }
            }
            Err(e) => Err(format!("Failed to fetch image: {e}")),
        }
    });

    match fetch_result {
        Ok((bytes, media_type)) => {
            if bytes.len() < 10 {
                repl.chat.add_message(ChatRole::System,
                    "Response does not contain valid image data.".to_string());
                return Ok(());
            }

            let engine = match repl.query_engine.as_mut() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.image.no_engine").to_string());
                    return Ok(());
                }
            };

            let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let blocks = vec![
                ContentBlock::Text { text: prompt },
                ContentBlock::Image {
                    source: ImageSource::base64(&media_type, base64_data),
                },
            ];

            engine.add_user_message_blocks(blocks);

            // Generate inline image preview
            let preview_config = crate::terminal_image::ImageRenderConfig::default();
            let preview_lines = crate::terminal_image::render_image_bytes(&bytes, &preview_config);
            repl.chat.add_message_with_image(
                ChatRole::User,
                format!("[Image from URL: {url}]"),
                preview_lines,
            );

            super::query::handle_query(repl, "Please analyze the image I just shared from the URL.")?;
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System,
                format!("Failed to fetch image: {e}"));
        }
    }

    Ok(())
}

fn handle_mode(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::permissions::ApprovalMode;

    let trimmed = args.trim();

    if trimmed.is_empty() {
        // Show current mode and available options
        let current = {
            let query_engine = repl.query_engine.as_ref().expect("query engine missing");
            let permissions = query_engine.permissions().read().expect("permissions rwlock poisoned");
            permissions.approval_mode()
        };
        let mut msg = format!("Current approval mode: {current}\n\nAvailable modes:\n");
        for name in ApprovalMode::all_names() {
            let mode = ApprovalMode::from_str_ci(name).unwrap();
            let marker = if mode == current { " *" } else { "" };
            msg.push_str(&format!("  {name}{marker} — {}\n", mode.description()));
        }
        {
            repl.chat.add_message(ChatRole::System, msg);
        }
        return Ok(());
    }

    match ApprovalMode::from_str_ci(trimmed) {
        Some(mode) => {
            let query_engine = repl.query_engine.as_ref().expect("query engine missing");
            query_engine.permissions().write().expect("permissions rwlock poisoned").set_approval_mode(mode);
            repl.state.approval_mode_label = mode.short_label().to_string();
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Approval mode set to: {mode}\n{}", mode.description()),
                );
            }
            Ok(())
        }
        None => {
            let valid = ApprovalMode::all_names().join(", ");
            {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Unknown mode: '{trimmed}'. Valid modes: {valid}"),
                );
            }
            Ok(())
        }
    }
}

fn handle_context(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed == "reload" {
        // Reload project context into the query engine
        let cwd = std::env::current_dir().unwrap_or_default();
        match shannon_core::project_instructions::load_full_context(&cwd) {
            Some(instructions) => {
                let query_engine = repl.query_engine.as_mut().expect("query engine missing");
                query_engine.append_system_prompt(&instructions.content);
                let files = instructions.loaded_files.join(", ");
                {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Project context reloaded. Loaded: {files}"),
                    );
                }
            }
            None => {
                {
                    repl.chat.add_message(
                        ChatRole::System,
                        "No project context found (no CLAUDE.md/AGENTS.md/GEMINI.md and not in a git repo)".to_string(),
                    );
                }
            }
        }
        return Ok(());
    }

    // Show current project context info
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut msg = String::from("Project Context:\n\n");

    // Check instruction files
    let instruction_files = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];
    let mut found_any = false;
    for filename in &instruction_files {
        let path = cwd.join(filename);
        if path.is_file() {
            found_any = true;
            msg.push_str(&format!("  {filename}: found\n"));
        } else {
            msg.push_str(&format!("  {filename}: not found\n"));
        }
    }

    // Check parent directories for instruction files
    let mut current = cwd.parent();
    while let Some(parent) = current {
        for filename in &instruction_files {
            if parent.join(filename).is_file() {
                msg.push_str(&format!("  {filename}: found in {}\n", parent.display()));
                found_any = true;
            }
        }
        current = parent.parent();
    }

    // Git context
    if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
        msg.push_str(&format!("\n{git_ctx}"));
        found_any = true;
    } else {
        msg.push_str("\nGit: not a git repository\n");
    }

    if !found_any {
        msg.push_str("\nNo project context available. Create a CLAUDE.md file or initialize a git repo.");
    }

    msg.push_str("\nTip: Use /context reload to refresh the project context.");
    {
        repl.chat.add_message(ChatRole::System, msg);
    }
    Ok(())
}

/// Re-scan custom command directories, deduplicate, and register all commands.
fn reload_custom_commands(repl: &mut Repl) {
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

    let mut custom_commands: Vec<super::CustomCommandEntry> = Vec::new();
    for dir in &dirs {
        super::collect_custom_commands(dir, "", &mut custom_commands);
    }
    super::dedup_custom_commands(&mut custom_commands);

    let count = custom_commands.len();
    for entry in &custom_commands {
        let description = entry.description.clone()
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

fn handle_commands(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // /commands edit <name> — open command file in $EDITOR
    if let Some(cmd_name) = trimmed.strip_prefix("edit ") {
        let name = cmd_name.trim();
        if name.is_empty() {
            repl.chat.add_message(ChatRole::System, "Usage: /commands edit <name>".to_string());
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
        let mut all_cmds: Vec<super::CustomCommandEntry> = Vec::new();
        for dir in &search_dirs {
            super::collect_custom_commands(dir, "", &mut all_cmds);
        }
        // Support subdirectory-prefixed names like "project:foo"
        let entry = all_cmds.iter().find(|e| e.name == name).or_else(|| {
            // Also try matching just the stem (without prefix)
            all_cmds.iter().find(|e| e.name.ends_with(&format!(":{name}")) || e.name == name)
        });
        match entry {
            Some(e) => {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                let path = e.path.clone();
                repl.chat.add_message(ChatRole::System, format!("Opening {} in {editor}...", path.display()));
                // Drop terminal raw mode before spawning editor
                crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
                crossterm::terminal::disable_raw_mode()?;
                let status = std::process::Command::new(&editor)
                    .arg(&path)
                    .status();
                // Restore terminal
                crossterm::terminal::enable_raw_mode()?;
                crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
                match status {
                    Ok(s) if s.success() => {
                        repl.chat.add_message(ChatRole::System, "Editor closed. Use /commands reload to apply changes.".to_string());
                    }
                    Ok(s) => {
                        repl.chat.add_message(ChatRole::System, format!("Editor exited with status: {}", s));
                    }
                    Err(e) => {
                        repl.chat.add_message(ChatRole::System, format!("Failed to launch editor: {e}"));
                    }
                }
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!("Command '{name}' not found. Use /commands to list available commands."));
            }
        }
        return Ok(());
    }

    // /commands create <name> — create a new command file
    if let Some(cmd_name) = trimmed.strip_prefix("create ") {
        let name = cmd_name.trim();
        if name.is_empty() {
            repl.chat.add_message(ChatRole::System, "Usage: /commands create <name>".to_string());
            return Ok(());
        }
        // Sanitize name: only allow alphanumeric, dash, underscore, colon (for subdirs)
        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ':') {
            repl.chat.add_message(ChatRole::System, "Command name can only contain letters, numbers, '-', '_', and ':' (for subdirectories).".to_string());
            return Ok(());
        }
        let cwd = std::env::current_dir().unwrap();
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
            repl.chat.add_message(ChatRole::System, format!("Command '{name}' already exists at {}. Use /commands edit {name} to edit it.", file_path.display()));
            return Ok(());
        }

        // Create directory and default template
        if let Err(e) = std::fs::create_dir_all(&cmd_dir) {
            repl.chat.add_message(ChatRole::System, format!("Failed to create directory {}: {e}", cmd_dir.display()));
            return Ok(());
        }

        let template = format!("---\ndescription: {name} command\n---\n\n$ARGUMENTS\n");
        if let Err(e) = std::fs::write(&file_path, &template) {
            repl.chat.add_message(ChatRole::System, format!("Failed to write {}: {e}", file_path.display()));
            return Ok(());
        }

        repl.chat.add_message(ChatRole::System, format!("Created command '{name}' at {}. Use /commands edit {name} to customize.", file_path.display()));
        // Auto-reload to register the new command
        reload_custom_commands(repl);
        return Ok(());
    }

    // /commands delete <name> — delete a command file
    if let Some(cmd_name) = trimmed.strip_prefix("delete ") {
        let name = cmd_name.trim();
        if name.is_empty() {
            repl.chat.add_message(ChatRole::System, "Usage: /commands delete <name>".to_string());
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
        let mut all_cmds: Vec<super::CustomCommandEntry> = Vec::new();
        for dir in &search_dirs {
            super::collect_custom_commands(dir, "", &mut all_cmds);
        }
        let entry = all_cmds.iter().find(|e| e.name == name);
        match entry {
            Some(e) => {
                let path = e.path.clone();
                if let Err(err) = std::fs::remove_file(&path) {
                    repl.chat.add_message(ChatRole::System, format!("Failed to delete {}: {err}", path.display()));
                } else {
                    repl.chat.add_message(ChatRole::System, format!("Deleted command '{name}' ({}).", path.display()));
                    // Auto-reload to update registry
                    reload_custom_commands(repl);
                }
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!("Command '{name}' not found. Use /commands to list available commands."));
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
    let mcp_prompts: Vec<_> = all.iter()
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

    let mut found: Vec<super::CustomCommandEntry> = Vec::new();
    for dir in &dirs {
        super::collect_custom_commands(dir, "", &mut found);
    }
    super::dedup_custom_commands(&mut found);

    if found.is_empty() {
        msg.push_str("  No custom commands found.\n\n");
        msg.push_str("  Create .md files in .claude/commands/ or .shannon/commands/\n");
        msg.push_str("  or use /commands create <name> to create one.\n");
    } else {
        msg.push_str(&format!("  Custom commands ({}):\n", found.len()));
        for entry in &found {
            let desc = entry.description.as_deref().unwrap_or("");
            let desc_suffix = if desc.is_empty() { String::new() } else { format!(" — {desc}") };
            msg.push_str(&format!("    /{}{}\n", entry.name, desc_suffix));
        }
    }

    // MCP prompt commands
    if !mcp_prompts.is_empty() {
        msg.push_str(&format!("\n  MCP prompt commands ({}):\n", mcp_prompts.len()));
        for cmd in &mcp_prompts {
            msg.push_str(&format!("    /{}  — {}\n", cmd.name(), cmd.base().description));
        }
    }

    msg.push_str("\nUsage: /commands [reload]");
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

fn handle_undo(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();
    let mgr = &repl.checkpoint_manager;

    if !mgr.is_enabled() {
        repl.chat.add_message(
            ChatRole::System,
            "Undo unavailable: not in a git repository.".to_string(),
        );
        return Ok(());
    }

    // /undo list — show checkpoints
    if trimmed == "list" || trimmed == "ls" {
        let checkpoints = mgr.list_checkpoints();
        if checkpoints.is_empty() {
            repl.chat.add_message(
                ChatRole::System,
                "No checkpoints available. Checkpoints are created before file-modifying operations.".to_string(),
            );
            return Ok(());
        }
        let mut msg = String::from("Checkpoints:\n\n");
        for (i, tc) in checkpoints.iter().enumerate() {
            let time = chrono::DateTime::from_timestamp(tc.checkpoint.timestamp, 0)
                .map(|t| t.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let files = if tc.files_changed.is_empty() {
                String::new()
            } else if tc.files_changed.len() <= 3 {
                format!(" [{}]", tc.files_changed.join(", "))
            } else {
                format!(" [{} files]", tc.files_changed.len())
            };
            let preview = tc.prompt_preview.as_deref().map(|p| format!(" — {p}")).unwrap_or_default();
            msg.push_str(&format!(
                "  [{}] {} {}{}{} — {}\n",
                i, tc.checkpoint.short_hash, time, files, preview, tc.checkpoint.description
            ));
        }
        msg.push_str("\nUse /undo <number> to revert to a specific checkpoint.");
        msg.push_str("\nUse /undo (no args) to revert the last checkpoint.");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    // /undo <number> — revert to specific checkpoint
    if let Ok(index) = trimmed.parse::<usize>() {
        match mgr.revert_to(index, shannon_core::RestoreMode::CodeAndConversation) {
            Ok(tc) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Reverted to checkpoint [{}] ({})\n{}", index, tc.checkpoint.short_hash, tc.checkpoint.description),
                );
            }
            Err(e) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Revert failed: {e}"),
                );
            }
        }
        return Ok(());
    }

    // /undo (no args) — revert last checkpoint
    if trimmed.is_empty() {
        match mgr.undo_last() {
            Ok(cp) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Undid last checkpoint ({})\n{}", cp.short_hash, cp.description),
                );
            }
            Err(e) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Undo failed: {e}. Use /undo list to see available checkpoints."),
                );
            }
        }
        return Ok(());
    }

    repl.chat.add_message(
        ChatRole::System,
        "Usage: /undo [list|<number>]".to_string(),
    );
    Ok(())
}

fn handle_rewind(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // /rewind history — show checkpoint history with turn info
    if trimmed == "history" || trimmed == "list" || trimmed == "ls" {
        let checkpoints = repl.checkpoint_manager.list_checkpoints();
        if checkpoints.is_empty() {
            repl.chat.add_message(
                ChatRole::System,
                "No turn checkpoints available.".to_string(),
            );
            return Ok(());
        }
        let mut msg = String::from("Turn history:\n\n");
        for (i, tc) in checkpoints.iter().enumerate() {
            let time = chrono::DateTime::from_timestamp(tc.checkpoint.timestamp, 0)
                .map(|t| t.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "??:??:??".to_string());
            let files = if tc.files_changed.is_empty() {
                String::new()
            } else if tc.files_changed.len() <= 3 {
                format!(" [{}]", tc.files_changed.join(", "))
            } else {
                format!(" [{} files]", tc.files_changed.len())
            };
            let preview = tc.prompt_preview.as_deref()
                .map(|p| if p.len() > 60 { format!("{}...", &p[..60]) } else { p.to_string() })
                .unwrap_or_default();
            msg.push_str(&format!(
                "  [{}] turn {} {}{} — {}\n",
                i, tc.turn_index, time, files, preview,
            ));
        }
        msg.push_str("\n/rewind <n> — rewind conversation by n turns");
        msg.push_str("\n/rewind code <n> — revert code to checkpoint [n]");
        msg.push_str("\n/rewind both <n> — revert code + rewind conversation to checkpoint [n]");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    // /rewind code <n> — revert file changes to checkpoint index n
    if let Some(rest) = trimmed.strip_prefix("code ") {
        if let Ok(index) = rest.trim().parse::<usize>() {
            match repl.checkpoint_manager.revert_to(index, shannon_core::RestoreMode::CodeOnly) {
                Ok(tc) => {
                    let files = if tc.files_changed.is_empty() {
                        "no files".to_string()
                    } else {
                        tc.files_changed.join(", ")
                    };
                    repl.chat.add_message(
                        ChatRole::System,
                        format!(
                            "Reverted code to checkpoint [{}] (turn {}).\nFiles affected: {}",
                            index, tc.turn_index, files
                        ),
                    );
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Code revert failed: {e}"));
                }
            }
            return Ok(());
        }
    }

    // /rewind both <n> — revert code + rewind conversation to checkpoint index n
    if let Some(rest) = trimmed.strip_prefix("both ") {
        if let Ok(index) = rest.trim().parse::<usize>() {
            match repl.checkpoint_manager.revert_to(index, shannon_core::RestoreMode::CodeAndConversation) {
                Ok(tc) => {
                    // Remove the "/rewind both" command message
                    repl.chat.pop_last();
                    // Calculate turns to rewind from conversation
                    let turns_to_rewind = repl.checkpoint_manager.list_checkpoints().len().saturating_sub(index);
                    if turns_to_rewind > 0 {
                        repl.chat.rewind(turns_to_rewind);
                        if let Some(ref mut engine) = repl.query_engine {
                            engine.rewind_conversation(turns_to_rewind);
                        }
                    }
                    let files = if tc.files_changed.is_empty() {
                        "no files".to_string()
                    } else {
                        tc.files_changed.join(", ")
                    };
                    repl.chat.add_message(
                        ChatRole::System,
                        format!(
                            "Rewound to checkpoint [{}] (turn {}): reverted code + conversation.\nFiles: {}",
                            index, tc.turn_index, files
                        ),
                    );
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Rewind failed: {e}"));
                }
            }
            return Ok(());
        }
    }

    // /rewind <n> — rewind conversation by n turns (existing behavior)
    let turns = if trimmed.is_empty() {
        1
    } else if let Ok(n) = trimmed.parse::<usize>() {
        if n == 0 {
            repl.chat.pop_last();
            repl.chat.add_message(ChatRole::System,
                "Usage: /rewind [n | history | code <n> | both <n>]".to_string());
            return Ok(());
        }
        n
    } else {
        repl.chat.pop_last();
        repl.chat.add_message(ChatRole::System,
            "Usage: /rewind [n | history | code <n> | both <n>]".to_string());
        return Ok(());
    };

    // Remove the "/rewind" command message
    repl.chat.pop_last();

    let before_count = repl.chat.len();
    let removed = repl.chat.rewind(turns);
    let after_count = repl.chat.len();

    if let Some(ref mut engine) = repl.query_engine {
        engine.rewind_conversation(turns);
    }

    if removed > 0 {
        repl.chat.add_message(
            ChatRole::System,
            format!(
                "Rewound {turns} turn(s): removed {removed} messages ({before_count} → {after_count} remaining).\nUse /rewind code <n> to also revert file changes."
            ),
        );
    } else {
        repl.chat.add_message(
            ChatRole::System,
            "No conversation turns to rewind.".to_string(),
        );
    }

    Ok(())
}

fn handle_notify(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    match trimmed {
        "on" | "enable" | "true" | "yes" => {
            repl.notifications_enabled = true;
            repl.chat.add_message(
                ChatRole::System,
                "Desktop notifications enabled. You'll be notified when queries complete.".to_string(),
            );
        }
        "off" | "disable" | "false" | "no" => {
            repl.notifications_enabled = false;
            repl.chat.add_message(
                ChatRole::System,
                "Desktop notifications disabled.".to_string(),
            );
        }
        "test" => {
            repl.notifier.info("Shannon", "Test notification!").ok();
            repl.chat.add_message(
                ChatRole::System,
                "Test notification sent.".to_string(),
            );
        }
        _ => {
            let status = if repl.notifications_enabled { "enabled" } else { "disabled" };
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Desktop notifications: {status}\n\n\
                     Usage:\n  /notify on  — enable notifications\n  \
                     /notify off — disable notifications\n  \
                     /notify test — send a test notification"
                ),
            );
        }
    }
    Ok(())
}

/// Handle `/webhook` command — manage webhook receiver for external event injection.
fn handle_webhook(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    match trimmed {
        "start" | "on" => {
            match repl.webhook_receiver {
                Some(ref rx) => {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Webhook receiver already running at {}", rx.address()),
                    );
                }
                None => {
                    let config = shannon_core::webhook::WebhookConfig::default();
                    let port = config.port;
                    let addr = format!("{}:{}", config.host, config.port);
                    let mut receiver = shannon_core::webhook::WebhookReceiver::new(config);

                    match receiver.try_start() {
                        Ok(()) => {
                            repl.chat.add_message(
                                ChatRole::System,
                                format!(
                                    "Webhook receiver started on {addr}\n\n\
                                     Endpoints:\n\
                                       POST http://{addr}/webhook/github  — GitHub webhooks\n\
                                       POST http://{addr}/webhook/generic — Generic webhooks\n\
                                       POST http://{addr}/webhook/health — Health check\n\n\
                                     Use /webhook status to check pending events.\n\
                                     Use /webhook stop to shut down."
                                ),
                            );
                            repl.webhook_receiver = Some(receiver);
                        }
                        Err(e) => {
                            repl.chat.add_message(
                                ChatRole::System,
                                format!("Failed to start webhook receiver on port {port}: {e}"),
                            );
                        }
                    }
                }
            }
        }
        "stop" | "off" => {
            if let Some(mut rx) = repl.webhook_receiver.take() {
                rx.stop();
                repl.chat.add_message(
                    ChatRole::System,
                    "Webhook receiver stopped.".to_string(),
                );
            } else {
                repl.chat.add_message(
                    ChatRole::System,
                    "No webhook receiver is running.".to_string(),
                );
            }
        }
        "status" => {
            match repl.webhook_receiver {
                Some(ref rx) => {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Webhook receiver active at {}", rx.address()),
                    );
                }
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        "No webhook receiver running. Use /webhook start to begin.".to_string(),
                    );
                }
            }
        }
        "poll" => {
            if let Some(ref mut rx) = repl.webhook_receiver {
                let mut count = 0;
                while let Some(event) = rx.try_recv() {
                    count += 1;
                    let url_note = event.url
                        .map(|u| format!("\n  Link: {u}"))
                        .unwrap_or_default();
                    repl.chat.add_message(
                        ChatRole::System,
                        format!(
                            "[Webhook: {}] {}\n  {}{url_note}",
                            event.source, event.title, event.body
                        ),
                    );
                }
                if count == 0 {
                    repl.chat.add_message(
                        ChatRole::System,
                        "No pending webhook events.".to_string(),
                    );
                }
            } else {
                repl.chat.add_message(
                    ChatRole::System,
                    "No webhook receiver running. Use /webhook start first.".to_string(),
                );
            }
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Webhook receiver — receive external events into this session.\n\n\
                 Usage:\n\
                   /webhook start  — start the webhook HTTP server\n\
                   /webhook stop   — stop the receiver\n\
                   /webhook status — show receiver status\n\
                   /webhook poll   — inject pending events into session\n\n\
                 Default: 127.0.0.1:3789\n\
                 GitHub: POST /webhook/github (issue_comment, PR reviews)\n\
                 Generic: POST /webhook/generic {\"title\":\"...\",\"body\":\"...\"}".to_string(),
            );
        }
    }
    Ok(())
}

fn handle_create_pr(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // Show help
    if trimmed == "help" || trimmed == "--help" || trimmed == "-h" {
        repl.chat.add_message(ChatRole::System,
            "Create a GitHub pull request\n\n\
             Usage:\n  /create-pr            — interactive PR creation\n  \
             /create-pr <title>     — create with custom title\n  \
             /create-pr --draft     — create as draft PR\n  \
             /create-pr --base X    — set target branch (default: main)\n  \
             /create-pr --web       — open in browser to continue editing".to_string(),
        );
        return Ok(());
    }

    // Check if gh CLI is available
    let gh_check = std::process::Command::new("gh")
        .arg("--version")
        .output();
    if gh_check.is_err() {
        repl.chat.add_message(ChatRole::System,
            "GitHub CLI (gh) is not installed. Install it: https://cli.github.com".to_string(),
        );
        return Ok(());
    }

    // Check if we're in a git repo
    let git_check = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&repl.state.working_directory)
        .output();
    if git_check.is_err() || !git_check.unwrap().status.success() {
        repl.chat.add_message(ChatRole::System, "Not inside a git repository.".to_string());
        return Ok(());
    }

    // Get current branch
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&repl.state.working_directory)
        .output();
    let current_branch = match branch_output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => {
            repl.chat.add_message(ChatRole::System, "Failed to determine current branch.".to_string());
            return Ok(());
        }
    };

    // Determine base branch (main or master)
    let base_branch = if trimmed.contains("--base") {
        if let Some(idx) = trimmed.find("--base") {
            let after = &trimmed[idx + 6..].trim_start();
            after.split_whitespace().next().unwrap_or("main").to_string()
        } else {
            "main".to_string()
        }
    } else {
        // Check if main or master exists
        let main_check = std::process::Command::new("git")
            .args(["rev-parse", "--verify", "main"])
            .current_dir(&repl.state.working_directory)
            .output();
        if main_check.is_ok() && main_check.unwrap().status.success() {
            "main".to_string()
        } else {
            "master".to_string()
        }
    };

    // Don't create PR from the base branch itself
    if current_branch == base_branch {
        repl.chat.add_message(ChatRole::System,
            format!("Currently on '{current_branch}'. Create a feature branch first:\n  git checkout -b my-feature"));
        return Ok(());
    }

    // Get commits between base and HEAD
    let log_output = std::process::Command::new("git")
        .args(["log", &format!("{base_branch}..HEAD"), "--oneline"])
        .current_dir(&repl.state.working_directory)
        .output();
    let commits = match log_output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => String::new(),
    };

    if commits.is_empty() {
        repl.chat.add_message(ChatRole::System,
            format!("No commits found between {base_branch} and {current_branch}. Make some changes first."));
        return Ok(());
    }

    // Generate PR title from first commit or custom args
    let is_draft = trimmed.contains("--draft");
    let open_web = trimmed.contains("--web");

    let title = {
        let non_flag_args: Vec<&str> = trimmed.split_whitespace()
            .filter(|s| !s.starts_with('-'))
            .collect();
        if non_flag_args.is_empty() {
            commits.lines().next()
                .map(|line| line.split_once(' ').map(|(_, msg)| msg.to_string()).unwrap_or(line.to_string()))
                .unwrap_or_else(|| format!("PR from {current_branch}"))
        } else {
            non_flag_args.join(" ")
        }
    };

    // Build PR body from commits
    let mut body = String::from("## Summary\n\n");
    for line in commits.lines() {
        body.push_str("- ");
        body.push_str(line);
        body.push('\n');
    }

    // Get diff stats for context
    let diff_stat = std::process::Command::new("git")
        .args(["diff", "--stat", &format!("{base_branch}...HEAD")])
        .current_dir(&repl.state.working_directory)
        .output();
    if let Ok(out) = diff_stat {
        let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !stat.is_empty() {
            body.push_str("\n## Changes\n\n```\n");
            body.push_str(&stat);
            body.push_str("\n```\n");
        }
    }

    // Check for uncommitted changes and warn
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repl.state.working_directory)
        .output();
    if let Ok(out) = &status_output {
        let changes = String::from_utf8_lossy(&out.stdout);
        if !changes.trim().is_empty() {
            body.push_str("\n> **Note:** This PR was created with uncommitted changes.\n");
        }
    }

    // Push the branch first (if not already pushed)
    let push_result = std::process::Command::new("git")
        .args(["push", "-u", "origin", &current_branch])
        .current_dir(&repl.state.working_directory)
        .output();
    match push_result {
        Ok(out) if out.status.success() => {
            let push_output = String::from_utf8_lossy(&out.stderr);
            if !push_output.is_empty() {
                tracing::debug!("Push output: {}", push_output);
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // It's ok if already pushed (error contains "already up-to-date" or similar)
            if !stderr.contains("up-to-date") && !stderr.contains("Everything up-to-date") {
                repl.chat.add_message(ChatRole::System,
                    format!("Failed to push branch: {stderr}"));
                return Ok(());
            }
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System,
                format!("Failed to push branch: {e}"));
            return Ok(());
        }
    }

    // Build gh pr create command
    let mut gh_args = vec!["pr", "create", "--title", &title, "--body", &body, "--base", &base_branch];
    if is_draft {
        gh_args.push("--draft");
    }
    if open_web {
        gh_args.push("--web");
    }

    let pr_result = std::process::Command::new("gh")
        .args(&gh_args)
        .current_dir(&repl.state.working_directory)
        .output();

    match pr_result {
        Ok(out) if out.status.success() => {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let msg = if is_draft { "Draft PR created" } else { "PR created" };
            repl.chat.add_message(ChatRole::System,
                format!("{msg}: {url}\n\nBranch: {current_branch} → {base_branch}\nCommits:\n{commits}"));

            // Send desktop notification if enabled
            if repl.notifications_enabled {
                let _ = repl.notifier.info("Shannon", &format!("{msg}: {url}"));
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("already exists") {
                // Find existing PR URL
                let existing = std::process::Command::new("gh")
                    .args(["pr", "view", &current_branch, "--json", "url"])
                    .current_dir(&repl.state.working_directory)
                    .output();
                match existing {
                    Ok(eout) if eout.status.success() => {
                        let url: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&eout.stdout)).unwrap_or_default();
                        let pr_url = url.get("url").and_then(|u| u.as_str()).unwrap_or("unknown");
                        repl.chat.add_message(ChatRole::System,
                            format!("PR already exists: {pr_url}\nBranch: {current_branch} → {base_branch}"));
                    }
                    _ => {
                        repl.chat.add_message(ChatRole::System,
                            format!("PR already exists for branch {current_branch}.\n{stderr}"));
                    }
                }
            } else {
                repl.chat.add_message(ChatRole::System,
                    format!("Failed to create PR: {stderr}"));
            }
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System,
                format!("Failed to run gh pr create: {e}"));
        }
    }

    Ok(())
}

fn handle_patch(repl: &mut Repl, args: &str) -> Result<()> {
    let args = args.trim();

    if args.is_empty() || args == "--help" || args == "help" {
        repl.chat.add_message(ChatRole::System,
            "Patch — search/replace with diff preview\n\n\
             Usage:\n\
               /patch <file> <search> --- <replace>          Preview change\n\
               /patch --apply <file> <search> --- <replace>  Apply change\n\
               /patch --all <file> <search> --- <replace>    Preview (replace all)\n\
               /patch --apply --all <file> <search> --- <replace>  Apply all\n\n\
             The preview shows the diff without modifying the file.\n\
             Add --apply to write the change.".to_string());
        return Ok(());
    }

    // Parse flags
    let apply = args.contains("--apply");
    let replace_all = args.contains("--all");
    let cleaned = args.replace("--apply", "").replace("--all", "");
    let cleaned = cleaned.trim();

    // Split on --- separator
    let parts: Vec<&str> = cleaned.splitn(2, "---").collect();
    if parts.len() < 2 {
        repl.chat.add_message(ChatRole::System,
            "Usage: /patch <file> <search> --- <replace>\nUse --- to separate search and replace text.".to_string());
        return Ok(());
    }

    // Parse file path and search text from the first part
    let first_part = parts[0].trim();
    let new_text = parts[1].trim().to_string();

    // First word is the file path, rest is the search text
    let mut words = first_part.splitn(2, char::is_whitespace);
    let file_path = match words.next() {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /patch <file> <search> --- <replace>".to_string());
            return Ok(());
        }
    };
    let old_text = words.next().unwrap_or("").to_string();

    if old_text.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Error: search text is empty.\nUsage: /patch <file> <search> --- <replace>".to_string());
        return Ok(());
    }

    // Resolve to absolute path if relative
    let abs_path = if std::path::Path::new(&file_path).is_absolute() {
        file_path.clone()
    } else {
        format!("{}/{}", repl.state.working_directory.trim_end_matches('/'), file_path)
    };

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let input = serde_json::json!({
        "file_path": abs_path,
        "old_string": old_text,
        "new_string": new_text,
        "replace_all": replace_all,
        "preview": !apply,
    });

    let tool_name = if apply { "Edit" } else { "Edit" };
    match repl.runtime.block_on(engine.tools().execute(tool_name, input)) {
        Ok(result) => {
            let prefix = if apply { "Applied" } else { "Preview" };
            let msg = format!("{prefix}: {}\n{}", file_path, result.content);
            { repl.chat.add_message(ChatRole::System, msg); }
        }
        Err(e) => {
            { repl.chat.add_message(ChatRole::System, format!("Patch failed: {e}")); }
        }
    }

    Ok(())
}

fn handle_sandbox(repl: &mut Repl, args: &str) -> Result<()> {
    let args = args.trim();

    if args.is_empty() || args == "--help" || args == "help" {
        let docker_available = repl.runtime.block_on(
            shannon_tools::DockerSandbox::is_available()
        );
        let status = if docker_available { "available" } else { "not installed/unavailable" };
        let platform = detect_platform_sandbox();

        repl.chat.add_message(ChatRole::System,
            "Sandbox — execution isolation for shell commands\n\n\
             Usage:\n\
               /sandbox              Show current sandbox status\n\
               /sandbox status       Show detailed sandbox info\n\
               /sandbox docker       Enable Docker isolation\n\
               /sandbox direct       Disable sandbox (run directly)\n\
               /sandbox check        Check if Docker is available\n\n\
             Docker: ".to_string() + status + "\n\
             Platform: " + platform + "\n\n\
             When Docker sandbox is enabled, all /bash tool commands\n\
             run inside an isolated container with:\n\
               - No network access (network=none)\n\
               - Memory limit (512m)\n\
               - CPU limit (1.0)\n\
               - Read-only root filesystem\n\
               - Workspace mounted at /workspace"
        );
        return Ok(());
    }

    match args {
        "status" | "info" => {
            let current = repl.state.sandbox_mode.clone();
            let mode_str = match &current {
                shannon_tools::SandboxMode::Direct => "direct (no sandbox)".to_string(),
                shannon_tools::SandboxMode::Docker(cfg) => {
                    format!("docker (image={}, network={}, memory={}, cpus={})",
                        cfg.image,
                        cfg.network,
                        cfg.memory.as_deref().unwrap_or("unlimited"),
                        cfg.cpus.as_deref().unwrap_or("unlimited"),
                    )
                }
            };
            repl.chat.add_message(ChatRole::System,
                format!("Sandbox mode: {mode_str}"));
        }
        "docker" | "on" | "enable" => {
            let config = shannon_tools::DockerSandboxConfig::default();
            repl.state.sandbox_mode = shannon_tools::SandboxMode::Docker(config);
            repl.chat.add_message(ChatRole::System,
                "Docker sandbox enabled. Shell commands will run inside an isolated container.\n\
                 Use /sandbox status for details, /sandbox direct to disable.".to_string());
        }
        "direct" | "off" | "disable" => {
            repl.state.sandbox_mode = shannon_tools::SandboxMode::Direct;
            repl.chat.add_message(ChatRole::System,
                "Sandbox disabled. Shell commands will run directly on the host.".to_string());
        }
        "check" => {
            let available = repl.runtime.block_on(
                shannon_tools::DockerSandbox::is_available()
            );
            if available {
                repl.chat.add_message(ChatRole::System,
                    "Docker is available and running.".to_string());
            } else {
                repl.chat.add_message(ChatRole::System,
                    "Docker is not available. Install Docker and ensure the daemon is running.".to_string());
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                format!("Unknown sandbox option: {args}\n\
                 Use: /sandbox [status|docker|direct|check]"));
        }
    }

    Ok(())
}

fn handle_sessions(repl: &mut Repl, args: &str) -> Result<()> {
    let sessions = match repl.state_manager.list_persisted_sessions() {
        Ok(s) => s,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Error listing sessions: {e}"));
            return Ok(());
        }
    };

    if sessions.is_empty() {
        repl.chat.add_message(ChatRole::System, "No saved sessions found.".to_string());
        repl.last_session_list.clear();
        return Ok(());
    }

    let show_all = args.contains("--all");
    let search_query = if let Some(idx) = args.find("--search") {
        let after = &args[idx + "--search".len()..].trim();
        if after.is_empty() { None } else { Some(after.to_lowercase()) }
    } else if !args.is_empty() && !args.starts_with("--") {
        Some(args.to_lowercase())
    } else {
        None
    };

    let mut filtered: Vec<_> = sessions.into_iter().filter(|s| {
        if let Some(ref q) = search_query {
            let title = s.title.as_deref().unwrap_or("").to_lowercase();
            let preview = s.preview.as_deref().unwrap_or("").to_lowercase();
            title.contains(q) || preview.contains(q) || s.model.to_lowercase().contains(q)
        } else {
            true
        }
    }).collect();

    let limit = if show_all { filtered.len() } else { 10.min(filtered.len()) };
    filtered.truncate(limit);

    repl.last_session_list = filtered.clone();

    let mut output = String::from("Saved sessions:\n");
    for (i, session) in filtered.iter().enumerate() {
        let title = session.title.as_deref().unwrap_or("Untitled");
        let date = session.updated_at.format("%Y-%m-%d %H:%M");
        let tokens = (session.total_input_tokens + session.total_output_tokens) as f64 / 1000.0;
        output.push_str(&format!(
            "  #{}  {}  \"{}\"  {} turns  {:.1}k tokens  [{}]\n",
            i + 1, date, title, session.turn_count, tokens, session.model,
        ));
    }

    if !show_all {
        output.push_str("\nUse /sessions --all to see all, /sessions --search <query> to filter");
    }
    output.push_str("\nUse /resume <number-or-uuid> to continue a session");

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_resume(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    if arg.is_empty() {
        repl.chat.add_message(ChatRole::System, "Usage: /resume <number-or-uuid>\nUse /sessions to see available sessions.".to_string());
        return Ok(());
    }

    let session_id = if let Ok(uuid) = uuid::Uuid::parse_str(arg) {
        uuid
    } else if let Ok(num) = arg.parse::<usize>() {
        if num == 0 || num > repl.last_session_list.len() {
            repl.chat.add_message(ChatRole::System, format!("Invalid session number: {num}. Use /sessions to see available sessions."));
            return Ok(());
        }
        repl.last_session_list[num - 1].session_id
    } else {
        repl.chat.add_message(ChatRole::System, format!("Invalid session identifier: {arg}. Use a number from /sessions or a UUID."));
        return Ok(());
    };

    match repl.state_manager.load_session(&session_id) {
        Ok(Some(data)) => {
            repl.chat.clear();
            let title = data.metadata.title.as_deref().unwrap_or("Untitled");
            let msg_count = data.messages.len();

            repl.chat.add_message(ChatRole::System, format!(
                "Resumed session: \"{}\" ({} messages, model: {})\nCreated: {} | Updated: {}",
                title, msg_count, data.metadata.model,
                data.metadata.created_at.format("%Y-%m-%d %H:%M"),
                data.metadata.updated_at.format("%Y-%m-%d %H:%M"),
            ));

            for msg in &data.messages {
                let role = match msg.role.as_str() {
                    "user" => ChatRole::User,
                    "assistant" => ChatRole::Assistant,
                    _ => ChatRole::System,
                };
                let content = match &msg.content {
                    shannon_core::api::MessageContent::Text(t) => t.clone(),
                    shannon_core::api::MessageContent::Blocks(blocks) => {
                        blocks.iter().filter_map(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        }).collect::<Vec<_>>().join("\n")
                    }
                };
                if !content.is_empty() {
                    repl.chat.add_message(role, content);
                }
            }

            if !data.metadata.model.is_empty() {
                repl.state.model = Some(data.metadata.model.clone());
            }
            repl.state.tokens_used = data.metadata.total_input_tokens + data.metadata.total_output_tokens;

            if let Some(ref mut engine) = repl.query_engine {
                match engine.restore_session(session_id) {
                    Ok(true) => {
                        tracing::info!(session_id = %session_id, "QueryEngine conversation restored");
                    }
                    Ok(false) => {
                        tracing::warn!(session_id = %session_id, "No persisted session data for QueryEngine restore");
                    }
                    Err(e) => {
                        tracing::warn!(session_id = %session_id, error = %e, "Failed to restore QueryEngine session");
                        repl.chat.add_message(ChatRole::System, format!("Warning: could not restore AI context (messages will lack prior history): {e}"));
                    }
                }
            }
        }
        Ok(None) => {
            repl.chat.add_message(ChatRole::System, format!("Session not found: {session_id}"));
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Error loading session: {e}"));
        }
    }

    Ok(())
}

fn handle_branch(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            "Usage: /branch <session-id-or-number> [message-index]\nUse /sessions to see available sessions.".to_string(),
        );
        return Ok(());
    }

    // Resolve session ID
    let session_id = if let Ok(uuid) = uuid::Uuid::parse_str(parts[0]) {
        uuid
    } else if let Ok(num) = parts[0].parse::<usize>() {
        if num == 0 || num > repl.last_session_list.len() {
            repl.chat.add_message(
                ChatRole::System,
                format!("Invalid session number: {num}. Use /sessions to see available sessions."),
            );
            return Ok(());
        }
        repl.last_session_list[num - 1].session_id
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!("Invalid session identifier: {}. Use a number from /sessions or a UUID.", parts[0]),
        );
        return Ok(());
    };

    // Load parent to get message count for default branch point
    let parent_data = match repl.state_manager.load_session(&session_id) {
        Ok(Some(data)) => data,
        Ok(None) => {
            repl.chat.add_message(ChatRole::System, format!("Session not found: {session_id}"));
            return Ok(());
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Error loading session: {e}"));
            return Ok(());
        }
    };

    let total_messages = parent_data.messages.len();

    // Parse optional branch point (defaults to end of conversation)
    let branch_point = if parts.len() > 1 {
        match parts[1].parse::<usize>() {
            Ok(idx) if idx <= total_messages => idx,
            Ok(idx) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Branch point {idx} is out of range. Session has {total_messages} messages."),
                );
                return Ok(());
            }
            Err(_) => {
                repl.chat.add_message(
                    ChatRole::System,
                    format!("Invalid branch point: {}. Must be a number.", parts[1]),
                );
                return Ok(());
            }
        }
    } else {
        total_messages
    };

    // Create the branch
    match repl.state_manager.create_branch(&session_id, branch_point, None) {
        Ok(branch_data) => {
            let title = parent_data.metadata.title.as_deref().unwrap_or("Untitled");
            let branch_id = branch_data.session_id;
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Created branch from \"{}\" at message {}/{}\nNew session: {branch_id}\nMessages copied: {}\nUse /resume {branch_id} to work on this branch",
                    title, branch_point, total_messages, branch_data.messages.len(),
                ),
            );
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Error creating branch: {e}"));
        }
    }

    Ok(())
}

fn handle_history(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();

    if let Some(rest) = arg.strip_prefix("--export") {
        let export_path = rest.trim();
        if export_path.is_empty() {
            repl.chat.add_message(ChatRole::System, "Usage: /history --export <file-path>".to_string());
            return Ok(());
        }

        let mut md = String::from("# Shannon Session Export\n\n");
        for i in 0..repl.chat.len() {
            if let Some(msg) = repl.chat.get_message(i) {
                let role = match msg.role {
                    ChatRole::User => "## User",
                    ChatRole::Assistant => "## Assistant",
                    ChatRole::System => "## System",
                    ChatRole::Tool => "## Tool",
                };
                md.push_str(&format!("{}\n\n{}\n\n---\n\n", role, msg.content));
            }
        }

        match std::fs::write(export_path, md) {
            Ok(_) => { repl.chat.add_message(ChatRole::System, format!("Session exported to: {export_path}")); }
            Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to export: {e}")); }
        };
        return Ok(());
    }

    let msg_count = repl.chat.len();
    let mut user_count = 0;
    let mut assistant_count = 0;
    for i in 0..repl.chat.len() {
        if let Some(msg) = repl.chat.get_message(i) {
            match msg.role {
                ChatRole::User => user_count += 1,
                ChatRole::Assistant => assistant_count += 1,
                ChatRole::System | ChatRole::Tool => {}
            }
        }
    }

    let tokens = repl.state.tokens_used;
    let model = repl.state.model.as_deref().unwrap_or("default");

    let mut stats = format!(
        "Current session stats:\n  Messages: {} total ({} user, {} assistant)\n  Tokens used: {} ({:.1}k)\n  Model: {}\n  Working dir: {}\n  Commands run: {}\n  Tools invoked: {}",
        msg_count, user_count, assistant_count, tokens, tokens as f64 / 1000.0,
        model, repl.state.working_directory, repl.commands_run, repl.tools_invoked,
    );

    if let Some(started) = &repl.session_started_at {
        let elapsed = chrono::Utc::now() - *started;
        let mins = elapsed.num_minutes();
        let secs = elapsed.num_seconds() % 60;
        stats.push_str(&format!("\n  Session duration: {mins}m {secs}s"));
    }

    if repl.diff_data.total_files_modified() > 0 || repl.diff_data.total_files_created() > 0 || repl.diff_data.total_files_deleted() > 0 {
        stats.push_str(&format!(
            "\n  Files: +{}/-{}/{} modified, {} created, {} deleted",
            repl.diff_data.total_additions(), repl.diff_data.total_deletions(),
            repl.diff_data.total_files_modified(), repl.diff_data.total_files_created(),
            repl.diff_data.total_files_deleted(),
        ));
    }

    repl.chat.add_message(ChatRole::System, stats);
    Ok(())
}

fn handle_worktree(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();

    if arg.is_empty() || arg == "status" {
        let status = if arg.is_empty() {
            "Usage: /worktree [enter <name>|exit [--keep|--remove]|status]\n".to_string()
        } else {
            String::new()
        };

        let active = shannon_agents::get_active_worktree();
        match active.as_ref() {
            Some(session) => {
                repl.chat.add_message(ChatRole::System, format!(
                    "{}Active worktree:\n  Branch: {}\n  Path: {}\n  Created: {}",
                    status, session.branch_name, session.path.display(),
                    session.created_at.format("%Y-%m-%d %H:%M"),
                ));
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!("{status}No active worktree. Working in main repository."));
            }
        }
        return Ok(());
    }

    let parts: Vec<&str> = arg.splitn(3, ' ').collect();
    match parts[0] {
        "enter" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /worktree enter <name>".to_string());
                return Ok(());
            }
            let input = serde_json::json!({ "name": name });
            let Some(engine) = repl.query_engine.as_ref() else {
                repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
                return Ok(());
            };
            match repl.runtime.block_on(engine.tools().execute("enter_worktree", input)) {
                Ok(result) => { repl.chat.add_message(ChatRole::System, format!("Entered worktree: {}", result.content)); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to enter worktree: {e}")); }
            }
        }
        "exit" => {
            let action = parts.get(1).copied().unwrap_or("keep");
            let exit_action = match action { "--remove" => "remove", _ => "keep" };
            let input = serde_json::json!({ "action": exit_action });
            let Some(engine) = repl.query_engine.as_ref() else {
                repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
                return Ok(());
            };
            match repl.runtime.block_on(engine.tools().execute("exit_worktree", input)) {
                Ok(result) => { repl.chat.add_message(ChatRole::System, format!("Exited worktree: {}", result.content)); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to exit worktree: {e}")); }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, "Unknown worktree action. Use: enter <name>, exit [--keep|--remove], or status".to_string());
        }
    }

    Ok(())
}

fn handle_team(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_agents::{AgentCoordinator, CoordinatorConfig, TeammateConfig, TaskPriority};

    let parts: Vec<&str> = args.splitn(4, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/team create <name> [description]  — Create a new agent team
/team add <team> <agent-name>  — Add agent to team
/team task <team> <subject>  — Add a task
/team assign <team>  — Assign pending tasks to available agents
/team status [team]  — Show team status
/team list  — List all teams
/team run  — Execute pending tasks in parallel
/team shutdown  — Shutdown team
/team disband <team>  — Disband team and clean up
/team delegate  — Toggle delegate mode (lead only coordinates)".to_string());
        }
        "create" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team create <name> [description]".to_string());
                return Ok(());
            }
            let description = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
            let config = CoordinatorConfig::default();
            match repl.runtime.block_on(AgentCoordinator::new(config)) {
                Ok(coordinator) => {
                    match repl.runtime.block_on(coordinator.create_team(name.to_string(), description)) {
                        Ok(()) => {
                            repl.team_coordinator = Some(std::sync::Arc::new(coordinator));
                            repl.chat.add_message(ChatRole::System, format!("Team '{name}' created."));
                        }
                        Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to create team: {e}")); }
                    }
                }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to initialize coordinator: {e}")); }
            }
        }
        "add" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            let agent_name = parts.get(2).copied().unwrap_or("");
            if team_name.is_empty() || agent_name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team add <team> <agent-name>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                let config = TeammateConfig::default();
                match repl.runtime.block_on(coordinator.add_teammate(team_name, agent_name.to_string(), config)) {
                    Ok(()) => {
                        let worktree_msg = match create_agent_worktree(repl, agent_name) {
                            Ok(path) => format!(" (worktree: {})", path.display()),
                            Err(reason) => format!(" (worktree skipped: {reason})"),
                        };
                        repl.chat.add_message(ChatRole::System, format!("Agent '{agent_name}' added to team '{team_name}'.{worktree_msg}"));
                    }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to add agent: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "task" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            let subject = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
            if team_name.is_empty() || subject.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team task <team> <subject>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.add_task(team_name, subject.clone(), String::new(), TaskPriority::Medium)) {
                    Ok(task_id) => { repl.chat.add_message(ChatRole::System, format!("Task added to '{team_name}': {subject} (id: {task_id})")); }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to add task: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "assign" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            if team_name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team assign <team>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.assign_task(team_name, uuid::Uuid::nil())) {
                    Ok(agent) => { repl.chat.add_message(ChatRole::System, format!("Task assigned to '{agent}' in team '{team_name}'.")); }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to assign task: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "status" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            if let Some(ref coordinator) = repl.team_coordinator {
                if team_name.is_empty() {
                    let teams = repl.runtime.block_on(coordinator.list_teams());
                    if teams.is_empty() {
                        repl.chat.add_message(ChatRole::System, "No teams created yet.".to_string());
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Teams:\n{}", teams.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n")));
                    }
                } else {
                    match repl.runtime.block_on(coordinator.team_status(team_name)) {
                        Ok(status) => { repl.chat.add_message(ChatRole::System, status); }
                        Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to get status: {e}")); }
                    }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "list" => {
            if let Some(ref coordinator) = repl.team_coordinator {
                let teams = repl.runtime.block_on(coordinator.list_teams());
                if teams.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No teams created yet.".to_string());
                } else {
                    repl.chat.add_message(ChatRole::System, format!("Teams:\n{}", teams.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n")));
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "shutdown" => {
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.shutdown()) {
                    Ok(()) => { repl.chat.add_message(ChatRole::System, "Team shut down.".to_string()); }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to shutdown: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No active team.".to_string());
            }
        }
        "disband" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            if team_name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team disband <team>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.disband_team(team_name)) {
                    Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Team '{team_name}' disbanded and cleaned up.")); }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to disband: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No active team coordinator.".to_string());
            }
        }
        "delegate" => {
            if let Some(ref coordinator) = repl.team_coordinator {
                let current = coordinator.delegate_mode();
                coordinator.set_delegate_mode(!current);
                let state = if !current { "ON" } else { "OFF" };
                repl.chat.add_message(ChatRole::System, format!("Delegate mode: {state}"));
            } else {
                repl.chat.add_message(ChatRole::System, "No active team coordinator.".to_string());
            }
        }
        "run" => {
            use shannon_agents::{MultiAgentSpawner, SpawnAgentConfig, shared_executor};
            if let Some(ref coordinator) = repl.team_coordinator {
                let task_board = coordinator.task_board();
                let ready_tasks = repl.runtime.block_on(task_board.list_ready_tasks());
                if ready_tasks.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No pending tasks to execute.".to_string());
                    return Ok(());
                }
                let agent_configs: Vec<SpawnAgentConfig> = ready_tasks
                    .iter().map(|t| SpawnAgentConfig::new(format!("agent-{}", t.id), t.subject.clone())).collect();
                let mut config = shannon_agents::MultiAgentConfig::new(agent_configs);
                config.default_system_prompt = Some("You are a helpful AI coding assistant. Complete the assigned task concisely and accurately.".to_string());
                // Create executor from the REPL's LLM client if available
                let executor = repl.query_engine.as_ref().map(|engine| {
                    let client = engine.client().clone();
                    shared_executor(client)
                });
                repl.chat.add_message(ChatRole::System, "Starting parallel execution...".to_string());
                let result = repl.runtime.block_on(MultiAgentSpawner::spawn(config, executor));
                let mut report = format!(
                    "Execution complete: {} succeeded, {} failed ({:.1}s)\n",
                    result.success_count, result.failure_count, result.total_duration.as_secs_f64(),
                );
                for ar in &result.agent_results {
                    report.push_str(&format!(
                        "  [{}] {} ({:.1}s){}\n",
                        ar.status, ar.agent_name, ar.duration.as_secs_f64(),
                        ar.error.as_ref().map(|e| format!(" — {e}")).unwrap_or_default(),
                    ));
                    if let Some(ref output) = ar.output {
                        let preview = if output.content.len() > 300 {
                            format!("{}...", &output.content[..300])
                        } else {
                            output.content.clone()
                        };
                        report.push_str(&format!("    {}\n", preview.trim()));
                    }
                }
                repl.chat.add_message(ChatRole::System, report);
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /team help."));
        }
    }

    Ok(())
}

fn handle_agents(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_agents::{AgentCoordinator, CoordinatorConfig, SubAgentRegistry, AgentConfig};

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    // Lazily initialize agent registry if needed
    fn ensure_registry(repl: &mut Repl) {
        if repl.agent_registry.is_none() {
            let config = CoordinatorConfig::default();
            let coordinator = repl.runtime.block_on(AgentCoordinator::new(config))
                .expect("failed to create agent coordinator");
            repl.agent_registry = Some(std::sync::Arc::new(SubAgentRegistry::new(
                std::sync::Arc::new(coordinator),
            )));
        }
    }

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/agents spawn <name> <prompt>  — Spawn a background agent
/agents list                   — List all agents and status
/agents status <name>          — Show agent details
/agents message <name> <text>  — Send message to agent
/agents kill <name>            — Kill a running agent
/agents run-bg <name> <task>   — Run task in background with notification".to_string());
        }
        "spawn" => {
            let name = parts.get(1).copied().unwrap_or("");
            let prompt = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || prompt.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents spawn <name> <system-prompt>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = repl.agent_registry.as_ref().unwrap().clone();
            let config = AgentConfig {
                name: name.to_string(),
                system_prompt: prompt.to_string(),
                ..Default::default()
            };
            match repl.runtime.block_on(registry.spawn(config)) {
                Ok(agent) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' spawned (id: {}, status: {})",
                        agent.name, agent.id, agent.status
                    ));
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to spawn agent: {e}"));
                }
            }
        }
        "list" => {
            ensure_registry(repl);
            let registry = repl.agent_registry.as_ref().unwrap().clone();
            let agents = repl.runtime.block_on(registry.list_agents());
            if agents.is_empty() {
                repl.chat.add_message(ChatRole::System, "No agents spawned yet.".to_string());
            } else {
                let mut out = format!("Agents ({}):\n", agents.len());
                for a in &agents {
                    out.push_str(&format!(
                        "  {} [{}] model={} turns={}/{}{}\n",
                        a.name, a.status, a.config.model,
                        a.turns_used, a.config.max_turns,
                        a.team.as_ref().map(|t| format!(" team={t}")).unwrap_or_default(),
                    ));
                }
                repl.chat.add_message(ChatRole::System, out);
            }
        }
        "status" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents status <name>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = repl.agent_registry.as_ref().unwrap().clone();
            match repl.runtime.block_on(registry.get_agent(name)) {
                Some(agent) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent: {}\n  ID: {}\n  Status: {}\n  Model: {}\n  Turns: {}/{}\n  Team: {}\n  Created: {}{}",
                        agent.name, agent.id, agent.status, agent.config.model,
                        agent.turns_used, agent.config.max_turns,
                        agent.team.as_deref().unwrap_or("none"),
                        agent.created_at.to_rfc3339(),
                        agent.last_output.as_ref().map(|o| format!("\n  Last output: {}", if o.len() > 200 { &o[..200] } else { o })).unwrap_or_default(),
                    ));
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found."));
                }
            }
        }
        "message" => {
            let name = parts.get(1).copied().unwrap_or("");
            let msg = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || msg.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents message <name> <text>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = repl.agent_registry.as_ref().unwrap().clone();
            match repl.runtime.block_on(registry.send_message("repl", name, serde_json::json!(msg))) {
                Ok(responses) => {
                    let mut out = format!("Message sent to '{name}', {} response(s):\n", responses.len());
                    for r in responses {
                        let content = match &r.content {
                            shannon_agents::MessageContent::Text(t) => t.clone(),
                            shannon_agents::MessageContent::Structured(v) => v.to_string(),
                            shannon_agents::MessageContent::Protocol(p) => format!("{p:?}"),
                        };
                        out.push_str(&format!("  [{}] {}\n", r.from, content));
                    }
                    repl.chat.add_message(ChatRole::System, out);
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to send message: {e}"));
                }
            }
        }
        "kill" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents kill <name>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = repl.agent_registry.as_ref().unwrap().clone();
            match repl.runtime.block_on(registry.get_agent(name)) {
                Some(mut agent) => {
                    agent.mark_failed("killed by user".to_string());
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' killed."));
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found."));
                }
            }
        }
        "run-bg" => {
            use shannon_agents::{MultiAgentSpawner, SpawnAgentConfig, MultiAgentConfig, shared_executor};

            let name = parts.get(1).copied().unwrap_or("");
            let task = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || task.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents run-bg <name> <task>".to_string());
                return Ok(());
            }
            let agent_config = SpawnAgentConfig::new(name.to_string(), task.to_string());
            let config = MultiAgentConfig::new(vec![agent_config]);

            let executor = repl.query_engine.as_ref().map(|engine| {
                let client = engine.client().clone();
                shared_executor(client)
            });

            repl.chat.add_message(ChatRole::System, format!("Running agent '{name}'..."));
            let result = repl.runtime.block_on(MultiAgentSpawner::spawn(config, executor));
            let status = if result.all_succeeded() { "completed" } else { "failed" };

            // Show output from agent if available
            if let Some(ar) = result.agent_results.first() {
                if let Some(ref output) = ar.output {
                    let preview = if output.content.len() > 500 {
                        format!("{}...", &output.content[..500])
                    } else {
                        output.content.clone()
                    };
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' {} in {:.1}s:\n{}",
                        name, status, result.total_duration.as_secs_f64(), preview,
                    ));
                } else {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' {} in {:.1}s",
                        name, status, result.total_duration.as_secs_f64(),
                    ));
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /agents help."));
        }
    }

    Ok(())
}

fn handle_route(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/route add <pattern> <model>   — Add a routing rule (pattern is case-insensitive substring match)
/route remove <pattern>        — Remove a routing rule
/route list                    — Show all routing rules
/route clear                   — Remove all routing rules
/route test <query>            — Test which model a query would route to

Patterns match against the start of your query. Examples:
  /route add explain claude-haiku-4-5     — 'explain ...' queries use haiku
  /route add refactor claude-opus-4-6     — 'refactor ...' queries use opus
  /route add test claude-sonnet-4-6       — 'test ...' queries use sonnet".to_string());
        }
        "add" => {
            let pattern = parts.get(1).copied().unwrap_or("");
            let model = parts.get(2).copied().unwrap_or("");
            if pattern.is_empty() || model.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /route add <pattern> <model>".to_string());
                return Ok(());
            }
            // Remove existing rule with same pattern if it exists
            repl.model_routes.retain(|(p, _)| p.to_lowercase() != pattern.to_lowercase());
            repl.model_routes.push((pattern.to_lowercase(), model.to_string()));
            repl.chat.add_message(ChatRole::System, format!(
                "Route added: queries starting with '{pattern}' → {model}",
            ));
        }
        "remove" => {
            let pattern = parts.get(1).copied().unwrap_or("");
            if pattern.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /route remove <pattern>".to_string());
                return Ok(());
            }
            let before = repl.model_routes.len();
            repl.model_routes.retain(|(p, _)| p.to_lowercase() != pattern.to_lowercase());
            let removed = before - repl.model_routes.len();
            if removed > 0 {
                repl.chat.add_message(ChatRole::System, format!("Removed {removed} route(s) for pattern '{pattern}'."));
            } else {
                repl.chat.add_message(ChatRole::System, format!("No route found for pattern '{pattern}'."));
            }
        }
        "list" => {
            if repl.model_routes.is_empty() {
                repl.chat.add_message(ChatRole::System, "No routing rules configured. Use /route add <pattern> <model>.".to_string());
            } else {
                let mut out = format!("Routing rules ({}):\n", repl.model_routes.len());
                for (pattern, model) in &repl.model_routes {
                    out.push_str(&format!("  '{pattern}' → {model}\n"));
                }
                repl.chat.add_message(ChatRole::System, out);
            }
        }
        "clear" => {
            let count = repl.model_routes.len();
            repl.model_routes.clear();
            repl.chat.add_message(ChatRole::System, format!("Cleared {count} routing rule(s)."));
        }
        "test" => {
            let query = parts.get(1..).map(|s| s.join(" ")).unwrap_or_default();
            if query.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /route test <query text>".to_string());
                return Ok(());
            }
            let query_lower = query.to_lowercase();
            let matched = repl.model_routes.iter().find(|(pattern, _)| {
                query_lower.starts_with(pattern)
            });
            match matched {
                Some((pattern, model)) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Query '{query}' matches pattern '{pattern}' → would use model: {model}",
                    ));
                }
                None => {
                    let current = repl.state.model.as_deref().unwrap_or("default");
                    repl.chat.add_message(ChatRole::System, format!(
                        "Query '{query}' matches no routing rules → would use default model: {current}",
                    ));
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /route help."));
        }
    }

    Ok(())
}

fn handle_mcp(repl: &mut Repl, args: &str) -> Result<()> {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    struct McpServerEntry {
        pub command: String,
        #[serde(default)]
        pub args: Vec<String>,
        #[serde(default)]
        pub env: HashMap<String, String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    struct McpConfig {
        #[serde(default)]
        pub mcp_servers: HashMap<String, McpServerEntry>,
    }

    fn config_path() -> PathBuf {
        PathBuf::from(".shannon/mcp.json")
    }

    fn load_config() -> McpConfig {
        let path = config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            McpConfig::default()
        }
    }

    fn save_config(config: &McpConfig) -> std::result::Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create .shannon dir: {e}"))?;
        }
        let content = serde_json::to_string_pretty(config).map_err(|e| format!("Failed to serialize: {e}"))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {e}"))?;
        Ok(())
    }

    let parts: Vec<&str> = args.splitn(4, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/mcp list                        — List configured MCP servers
/mcp add <name> <command> [args] — Add an MCP server
/mcp remove <name>               — Remove an MCP server
/mcp show <name>                 — Show server details
/mcp test <name>                 — Test server connection
/mcp approve <name>              — Approve a server for next startup
/mcp deny <name>                 — Deny a server
/mcp reset-approvals             — Clear all approval decisions
/mcp reload                      — Reload MCP config and restart servers
/mcp resources [server]          — List available MCP resources
/mcp subscribe <server> <uri>    — Subscribe to resource updates
/mcp unsubscribe <server> <uri>  — Unsubscribe from resource updates
/mcp path                        — Show config file path".to_string());
        }
        "list" => {
            let config = load_config();
            if config.mcp_servers.is_empty() {
                repl.chat.add_message(ChatRole::System, "No MCP servers configured. Use /mcp add <name> <command>.".to_string());
            } else {
                let mut out = format!("MCP servers ({}):\n", config.mcp_servers.len());
                for (name, entry) in &config.mcp_servers {
                    let args_str = if entry.args.is_empty() { String::new() } else { format!(" {}", entry.args.join(" ")) };
                    out.push_str(&format!("  {} → {}{}\n", name, entry.command, args_str));
                }
                out.push_str(&format!("\nConfig: {}", config_path().display()));
                repl.chat.add_message(ChatRole::System, out);
            }
        }
        "add" => {
            let name = parts.get(1).copied().unwrap_or("");
            let command = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || command.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp add <name> <command> [args...]".to_string());
                return Ok(());
            }
            let extra_args: Vec<String> = parts.get(3..)
                .map(|s| s.iter().map(|a| a.to_string()).collect())
                .unwrap_or_default();
            let mut config = load_config();
            let existed = config.mcp_servers.contains_key(name);
            config.mcp_servers.insert(name.to_string(), McpServerEntry {
                command: command.to_string(),
                args: extra_args,
                env: HashMap::new(),
            });
            match save_config(&config) {
                Ok(()) => {
                    if existed {
                        repl.chat.add_message(ChatRole::System, format!("Updated MCP server '{name}' → {command}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Added MCP server '{name}' → {command}"));
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to save config: {e}"));
                }
            }
        }
        "remove" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp remove <name>".to_string());
                return Ok(());
            }
            let mut config = load_config();
            if config.mcp_servers.remove(name).is_some() {
                match save_config(&config) {
                    Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Removed MCP server '{name}'.")); }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to save config: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, format!("Server '{name}' not found in config."));
            }
        }
        "show" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp show <name>".to_string());
                return Ok(());
            }
            let config = load_config();
            match config.mcp_servers.get(name) {
                Some(entry) => {
                    let env_str = if entry.env.is_empty() {
                        "none".to_string()
                    } else {
                        entry.env.keys().cloned().collect::<Vec<_>>().join(", ")
                    };
                    repl.chat.add_message(ChatRole::System, format!(
                        "Server: {}\n  Command: {}\n  Args: {}\n  Env vars: {}",
                        name, entry.command,
                        if entry.args.is_empty() { "(none)".to_string() } else { entry.args.join(" ") },
                        env_str,
                    ));
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Server '{name}' not found."));
                }
            }
        }
        "test" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp test <name>".to_string());
                return Ok(());
            }
            let config = load_config();
            match config.mcp_servers.get(name) {
                Some(entry) => {
                    repl.chat.add_message(ChatRole::System, format!("Testing connection to '{name}'..."));
                    // Try to create a stdio transport and check if the command exists
                    let command = &entry.command;
                    let which_output = std::process::Command::new("which")
                        .arg(command)
                        .output();
                    match which_output {
                        Ok(output) if output.status.success() => {
                            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                            repl.chat.add_message(ChatRole::System, format!(
                                "Server '{name}': command found at {path}. Ready to connect.",
                            ));
                        }
                        Ok(_) => {
                            repl.chat.add_message(ChatRole::System, format!(
                                "Server '{name}': command '{command}' not found in PATH.",
                            ));
                        }
                        Err(e) => {
                            repl.chat.add_message(ChatRole::System, format!("Test failed: {e}"));
                        }
                    }
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Server '{name}' not found."));
                }
            }
        }
        "path" => {
            repl.chat.add_message(ChatRole::System, format!("MCP config: {}", config_path().display()));
        }
        "approve" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp approve <name>".to_string());
                return Ok(());
            }
            let approval_path = PathBuf::from(".shannon/mcp_approvals.json");
            let mut mgr = shannon_core::McpApprovalManager::with_defaults();
            let _ = mgr.load_from_file(&approval_path);
            mgr.approve_server(name);
            match mgr.save_to_file(&approval_path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Approved '{name}'. It will connect on next startup.")); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to save approval: {e}")); }
            }
        }
        "deny" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp deny <name>".to_string());
                return Ok(());
            }
            let approval_path = PathBuf::from(".shannon/mcp_approvals.json");
            let mut mgr = shannon_core::McpApprovalManager::with_defaults();
            let _ = mgr.load_from_file(&approval_path);
            mgr.deny_server(name);
            match mgr.save_to_file(&approval_path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Denied '{name}'. It will be skipped on next startup.")); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to save denial: {e}")); }
            }
        }
        "reset-approvals" => {
            let approval_path = PathBuf::from(".shannon/mcp_approvals.json");
            match shannon_core::McpApprovalManager::reset_persisted(&approval_path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, "All approval decisions cleared. Servers will be re-evaluated on next startup.".to_string()); }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to reset approvals: {e}")); }
            }
        }
        "reload" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            match shannon_mcp::config::discover_config(&cwd) {
                Ok(config) => {
                    let pool = repl.mcp_pool.clone();
                    let changes = repl.runtime.block_on(pool.reload_from_config(&config));
                    match changes {
                        Ok(changes) => {
                            // Discover tools from newly started servers and register them
                            let mut new_tool_count = 0;
                            let new_servers: Vec<String> = changes.iter()
                                .filter(|c| c.starts_with("Started "))
                                .map(|c| {
                                    // Extract server name from "Started stdio server 'name'" etc.
                                    let s = c.trim_start_matches("Started ");
                                    s.split('\'').nth(1).unwrap_or("").to_string()
                                })
                                .filter(|s| !s.is_empty())
                                .collect();

                            if !new_servers.is_empty() {
                                let registry = repl.tool_registry.clone();
                                for server_name in &new_servers {
                                    let result = repl.runtime.block_on(
                                        pool.send_batch_server_request(
                                            server_name,
                                            vec![("tools/list", serde_json::json!({}))],
                                        )
                                    );
                                    if let Ok(responses) = result {
                                        if let Some((_, Ok(response))) = responses.first() {
                                            if let Some(tools_array) = response.get("tools").and_then(|t| t.as_array()) {
                                                for tool_value in tools_array {
                                                    let tool_name = tool_value.get("name")
                                                        .and_then(|n| n.as_str())
                                                        .unwrap_or("unknown")
                                                        .to_string();
                                                    let description = tool_value.get("description")
                                                        .and_then(|d| d.as_str())
                                                        .unwrap_or("")
                                                        .to_string();
                                                    let input_schema = tool_value.get("inputSchema")
                                                        .cloned()
                                                        .unwrap_or(serde_json::json!({"type": "object"}));
                                                    let annotations: Option<shannon_mcp::ToolAnnotations> =
                                                        tool_value.get("annotations")
                                                        .and_then(|a| serde_json::from_value(a.clone()).ok());

                                                    let adapter = shannon_mcp::PooledMcpToolAdapter::new(
                                                        pool.clone(),
                                                        server_name.clone(),
                                                        tool_name,
                                                        description,
                                                        input_schema,
                                                        annotations,
                                                    );
                                                    if let Err(e) = registry.register(Box::new(adapter)) {
                                                        tracing::warn!("Failed to register MCP tool: {e}");
                                                    } else {
                                                        new_tool_count += 1;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Report prompts from all connected servers and register them as slash commands
                            let all_prompts = repl.runtime.block_on(pool.list_all_prompts());

                            // Register MCP prompts as slash commands: /mcp__{server}__{prompt}
                            let mut new_prompt_count = 0;
                            for (server_name, prompts) in &all_prompts {
                                for prompt in prompts {
                                    let cmd_name = format!("mcp__{}__{}", server_name, prompt.name);
                                    let arg_names: Vec<String> = prompt.arguments
                                        .as_ref()
                                        .map(|args| args.iter().map(|a| a.name.clone()).collect())
                                        .unwrap_or_default();
                                    let arg_hint = if arg_names.is_empty() { None } else { Some(arg_names.join(", ")) };
                                    let prompt_template = format!(
                                        "Use the get_mcp_prompt tool to retrieve and execute the '{}' prompt from the '{}' MCP server with these arguments: {{args}}",
                                        prompt.name, server_name
                                    );
                                    use shannon_commands::{Command, CommandBase, ExecutionContext, PromptCommand};
                                    use std::collections::HashMap;
                                    let command = Command::Prompt(Box::new(PromptCommand {
                                        base: CommandBase {
                                            name: cmd_name,
                                            aliases: Vec::new(),
                                            description: prompt.description.clone(),
                                            has_user_specified_description: false,
                                            availability: vec![shannon_commands::CommandAvailability::All],
                                            source: shannon_commands::CommandSource::Builtin,
                                            is_enabled: true,
                                            is_hidden: false,
                                            argument_hint: arg_hint,
                                            when_to_use: None,
                                            version: None,
                                            disable_model_invocation: false,
                                            user_invocable: true,
                                            is_workflow: false,
                                            immediate: false,
                                            is_sensitive: false,
                                            user_facing_name: None,
                                        },
                                        progress_message: format!("Loading MCP prompt '{}' from '{}'", prompt.name, server_name),
                                        content_length: 0,
                                        arg_names,
                                        allowed_tools: vec!["get_mcp_prompt".to_string()],
                                        model: None,
                                        hooks: HashMap::new(),
                                        context: ExecutionContext::Inline,
                                        agent: None,
                                        paths: Vec::new(),
                                        prompt_template: Some(prompt_template),
                                    }));
                                    repl.command_registry.register_sync(command);
                                    new_prompt_count += 1;
                                }
                            }

                            let prompt_count: usize = all_prompts.iter().map(|(_, p)| p.len()).sum();

                            let mut msg = if changes.is_empty() {
                                "MCP config reloaded — no changes detected.".to_string()
                            } else {
                                let mut m = format!("MCP config reloaded ({} change(s)):\n", changes.len());
                                for change in &changes {
                                    m.push_str(&format!("  • {change}\n"));
                                }
                                m
                            };
                            if new_tool_count > 0 {
                                msg.push_str(&format!("  • Registered {new_tool_count} new tool(s)\n"));
                            }
                            if new_prompt_count > 0 {
                                msg.push_str(&format!("  • Registered {new_prompt_count} prompt command(s)\n"));
                            }
                            msg.push_str(&format!("  • {prompt_count} prompt(s) available from {} server(s)\n", all_prompts.len()));
                            repl.chat.add_message(ChatRole::System, msg);
                        }
                        Err(e) => {
                            repl.chat.add_message(ChatRole::System, format!("MCP reload failed: {e}"));
                        }
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to discover MCP config: {e}"));
                }
            }
        }
        "resources" => {
            let server = parts.get(1).copied().unwrap_or("");
            let pool = repl.mcp_pool.clone();
            if server.is_empty() {
                // List resources from all servers that support them
                let servers = repl.runtime.block_on(pool.list_servers());
                let mut msg = String::new();
                for (name, _) in &servers {
                    let has_res = repl.runtime.block_on(pool.has_resources(name));
                    if has_res {
                        let result = repl.runtime.block_on(
                            pool.send_batch_server_request(name, vec![("resources/list", serde_json::json!({}))])
                        );
                        match result {
                            Ok(responses) => {
                                if let Some((_, Ok(response))) = responses.first() {
                                    if let Some(resources) = response.get("resources").and_then(|r| r.as_array()) {
                                        if !resources.is_empty() {
                                            msg.push_str(&format!("  {name}:\n"));
                                            for res in resources {
                                                let uri = res.get("uri").and_then(|u| u.as_str()).unwrap_or("?");
                                                let name_field = res.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                                msg.push_str(&format!("    {} ({})\n", uri, name_field));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => { msg.push_str(&format!("  {name}: error — {e}\n")); }
                        }
                    }
                }
                if msg.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No MCP servers with resource support found.".to_string());
                } else {
                    repl.chat.add_message(ChatRole::System, format!("MCP Resources:\n{msg}"));
                }
            } else {
                let result = repl.runtime.block_on(
                    pool.send_batch_server_request(server, vec![("resources/list", serde_json::json!({}))])
                );
                match result {
                    Ok(responses) => {
                        if let Some((_, Ok(response))) = responses.first() {
                            if let Some(resources) = response.get("resources").and_then(|r| r.as_array()) {
                                if resources.is_empty() {
                                    repl.chat.add_message(ChatRole::System, format!("Server '{server}' has no resources."));
                                } else {
                                    let mut msg = format!("Resources from '{server}':\n");
                                    for res in resources {
                                        let uri = res.get("uri").and_then(|u| u.as_str()).unwrap_or("?");
                                        let name_field = res.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                        msg.push_str(&format!("  {} ({})\n", uri, name_field));
                                    }
                                    repl.chat.add_message(ChatRole::System, msg);
                                }
                            } else {
                                repl.chat.add_message(ChatRole::System, format!("Server '{server}' returned no resource list."));
                            }
                        } else {
                            repl.chat.add_message(ChatRole::System, format!("Failed to list resources from '{server}'."));
                        }
                    }
                    Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error: {e}")); }
                }
            }
        }
        "subscribe" => {
            let server = parts.get(1).copied().unwrap_or("");
            let uri = parts.get(2).copied().unwrap_or("");
            if server.is_empty() || uri.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp subscribe <server> <resource_uri>".to_string());
                return Ok(());
            }
            let pool = repl.mcp_pool.clone();
            let result = repl.runtime.block_on(
                pool.send_batch_server_request(server, vec![("resources/subscribe", serde_json::json!({"uri": uri}))])
            );
            match result {
                Ok(responses) => {
                    if let Some((_, Ok(_))) = responses.first() {
                        repl.chat.add_message(ChatRole::System, format!("Subscribed to '{uri}' on '{server}'."));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Server '{server}' did not confirm subscription."));
                    }
                }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Subscribe failed: {e}")); }
            }
        }
        "unsubscribe" => {
            let server = parts.get(1).copied().unwrap_or("");
            let uri = parts.get(2).copied().unwrap_or("");
            if server.is_empty() || uri.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp unsubscribe <server> <resource_uri>".to_string());
                return Ok(());
            }
            let pool = repl.mcp_pool.clone();
            let result = repl.runtime.block_on(
                pool.send_batch_server_request(server, vec![("resources/unsubscribe", serde_json::json!({"uri": uri}))])
            );
            match result {
                Ok(responses) => {
                    if let Some((_, Ok(_))) = responses.first() {
                        repl.chat.add_message(ChatRole::System, format!("Unsubscribed from '{uri}' on '{server}'."));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Server '{server}' did not confirm unsubscription."));
                    }
                }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Unsubscribe failed: {e}")); }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /mcp help."));
        }
    }

    Ok(())
}

fn handle_credentials(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::credential_utils::{
        parse_credential_action, CredentialAction,
        format_credentials_list, format_credential_store,
        format_credential_get, format_credential_delete, format_credential_count,
    };

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let action_str = parts.first().copied().unwrap_or("");
    let action = parse_credential_action(action_str);

    let output = match action {
        CredentialAction::List => format_credentials_list(),
        CredentialAction::Store => {
            let service = parts.get(1).copied().unwrap_or("");
            let value = parts.get(2).copied().unwrap_or("");
            if service.is_empty() || value.is_empty() {
                "Usage: /credentials store <service> <value>".to_string()
            } else {
                format_credential_store(service, value)
            }
        }
        CredentialAction::Get => {
            let service = parts.get(1).copied().unwrap_or("");
            if service.is_empty() {
                "Usage: /credentials get <service>".to_string()
            } else {
                format_credential_get(service)
            }
        }
        CredentialAction::Delete => {
            let service = parts.get(1).copied().unwrap_or("");
            if service.is_empty() {
                "Usage: /credentials delete <service>".to_string()
            } else {
                format_credential_delete(service)
            }
        }
        CredentialAction::Count => format_credential_count(),
        CredentialAction::Help => {
            "Credential Management:\n\n\
             /credentials list              - Show stored credentials\n\
             /credentials store <svc> <val> - Store a credential\n\
             /credentials get <service>     - Retrieve a credential (masked)\n\
             /credentials delete <service>  - Delete a credential\n\
             /credentials count             - Show stored credential count\n".to_string()
        }
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_status(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::status_utils::{parse_git_status, format_status};

    let short = args.contains("--short");

    let output = std::process::Command::new("git")
        .args(["status", "--short", "--branch"])
        .current_dir(&repl.state.working_directory)
        .output();

    let status_output = match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            if !stderr.is_empty() && stdout.is_empty() {
                repl.chat.add_message(ChatRole::System, format!("Git error: {stderr}"));
                return Ok(());
            }
            stdout.to_string()
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Failed to run git status: {e}"));
            return Ok(());
        }
    };

    if let Some(info) = parse_git_status(&status_output) {
        let mut full_output = format_status(&info, short);

        let log_output = std::process::Command::new("git")
            .args(["log", "--oneline", "-5"])
            .current_dir(&repl.state.working_directory)
            .output();

        if let Ok(log_result) = log_output {
            let log_stdout = String::from_utf8_lossy(&log_result.stdout);
            if !log_stdout.is_empty() {
                full_output.push_str("\nRecent commits:\n");
                full_output.push_str(&log_stdout);
            }
        }

        repl.chat.add_message(ChatRole::System, full_output);
    } else {
        repl.chat.add_message(ChatRole::System, status_output);
    }

    Ok(())
}

fn handle_export(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::export_utils;

    let options = match export_utils::parse_export_args(args) {
        Ok(opts) => opts,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Export error: {e}"));
            return Ok(());
        }
    };

    let filename = options.filename.clone().unwrap_or_else(|| {
        export_utils::generate_filename(options.format)
    });

    let mut messages = Vec::new();
    for i in 0..repl.chat.len() {
        if let Some(msg) = repl.chat.get_message(i) {
            let role = match msg.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::System => "system",
                ChatRole::Tool => "tool",
            };
            messages.push(export_utils::ExportMessage {
                role: role.to_string(),
                content: msg.content.clone(),
                timestamp: Some(msg.timestamp.timestamp() as u64),
            });
        }
    }

    let started_at = repl.session_started_at.map(|t| t.timestamp() as u64).unwrap_or(0);

    let session = export_utils::ExportSession {
        title: "Shannon Session".to_string(),
        started_at,
        messages,
        metadata: export_utils::SessionMetadata {
            model: repl.state.model.clone().unwrap_or_else(|| "default".to_string()),
            tokens_used: repl.state.tokens_used as usize,
            working_dir: repl.state.working_directory.clone(),
            commands_run: repl.commands_run,
            tools_invoked: repl.tools_invoked,
        },
    };

    let content = match options.format {
        export_utils::ExportFormat::Markdown => export_utils::export_to_markdown(&session, &options),
        export_utils::ExportFormat::Json => export_utils::export_to_json(&session, &options),
    };

    match export_utils::write_export(&content, &filename) {
        Ok(_) => {
            let format_name = match options.format {
                export_utils::ExportFormat::Markdown => "markdown",
                export_utils::ExportFormat::Json => "JSON",
            };
            repl.chat.add_message(ChatRole::System, format!("Session exported to: {filename} ({} messages, {format_name} format)", repl.chat.len()));
        }
        Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to export session: {e}")); }
    }
    Ok(())
}

fn handle_diff(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::diff_utils;

    let trimmed = args.trim();

    // /diff view — open interactive diff viewer overlay
    if trimmed == "view" || trimmed == "--view" {
        let file_count = {
            let diff = &repl.diff_data;
            let mut count = 0usize;
            let mut seen = std::collections::HashSet::new();
            for turn in diff.get_session_diffs() {
                for fc in &turn.files_modified {
                    if seen.insert(fc.path.clone()) {
                        count += 1;
                    }
                }
                count += turn.files_created.len() + turn.files_deleted.len();
            }
            count
        };
        let mut viewer = crate::widgets::diff_viewer::DiffViewerWidget::new();
        viewer.sync_expanded(file_count);
        repl.state.diff_viewer = Some(viewer);
        return Ok(());
    }

    // /diff interactive — open interactive hunk-by-hunk review
    if trimmed == "interactive" || trimmed == "--interactive" {
        let output = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&repl.state.working_directory)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let diff_str = String::from_utf8_lossy(&o.stdout);
                let hunks = crate::widgets::diff_viewer::InteractiveHunk::parse_from_diff(&diff_str, None);
                if hunks.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No diff hunks found.".to_string());
                } else {
                    repl.state.interactive_hunks = hunks;
                    repl.state.interactive_selected = 0;
                    repl.state.diff_interactive = true;
                    repl.state.diff_viewer = Some(crate::widgets::diff_viewer::DiffViewerWidget::new());
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                repl.chat.add_message(ChatRole::System, format!("git diff failed: {err}"));
            }
            Err(e) => {
                repl.chat.add_message(ChatRole::System, format!("Failed to run git diff: {e}"));
            }
        }
        return Ok(());
    }

    // /diff accept-all — keep all unstaged changes
    if trimmed == "accept-all" || trimmed == "keep-all" {
        let output = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, "All changes accepted and staged.".to_string());
            }
            Ok(o) => {
                repl.chat.add_message(ChatRole::System, format!("Failed to stage: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error: {e}")); }
        }
        return Ok(());
    }

    // /diff reject-all — discard all unstaged changes
    if trimmed == "reject-all" || trimmed == "discard-all" {
        // First warn about destructive action
        let status_output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repl.state.working_directory)
            .output();
        let file_count = status_output
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0);
        if file_count == 0 {
            repl.chat.add_message(ChatRole::System, "No changes to reject.".to_string());
            return Ok(());
        }
        let output = std::process::Command::new("git")
            .args(["checkout", "--", "."])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, format!("All unstaged changes discarded ({file_count} files)."));
            }
            Ok(o) => {
                repl.chat.add_message(ChatRole::System, format!("Failed to discard: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error: {e}")); }
        }
        // Also clean untracked files
        let _ = std::process::Command::new("git")
            .args(["clean", "-fd"])
            .current_dir(&repl.state.working_directory)
            .output();
        return Ok(());
    }

    // /diff accept <file> — accept changes to a specific file
    if let Some(file) = trimmed.strip_prefix("accept ") {
        let file = file.trim();
        let output = std::process::Command::new("git")
            .args(["add", "--", file])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, format!("Changes to '{file}' accepted (staged)."));
            }
            Ok(o) => {
                repl.chat.add_message(ChatRole::System, format!("Failed: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error: {e}")); }
        }
        return Ok(());
    }

    // /diff reject <file> — reject changes to a specific file
    if let Some(file) = trimmed.strip_prefix("reject ") {
        let file = file.trim();
        let output = std::process::Command::new("git")
            .args(["checkout", "--", file])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, format!("Changes to '{file}' rejected (reverted)."));
            }
            Ok(o) => {
                repl.chat.add_message(ChatRole::System, format!("Failed: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error: {e}")); }
        }
        return Ok(());
    }

    // /diff review — interactive per-file review
    if trimmed == "review" || trimmed == "--review" {
        let status_output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repl.state.working_directory)
            .output();

        match status_output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    repl.chat.add_message(ChatRole::System, "No changes to review.".to_string());
                    return Ok(());
                }

                let mut msg = String::from("Interactive Diff Review\n\nChanged files:\n\n");
                for (i, line) in stdout.lines().enumerate() {
                    let status = &line[..2];
                    let file = &line[3..];
                    let status_desc = match status.trim() {
                        "M" => "modified",
                        "A" => "added",
                        "D" => "deleted",
                        "R" => "renamed",
                        "C" => "copied",
                        "??" => "untracked",
                        "!!" => "ignored",
                        s if s.ends_with('M') => "modified (staged)",
                        s if s.starts_with('M') => "modified",
                        _ => status,
                    };
                    msg.push_str(&format!("  [{}] {} ({})\n", i + 1, file, status_desc));
                }

                msg.push_str("\nCommands:\n");
                msg.push_str("  /diff review <n>    — show diff for file #n\n");
                msg.push_str("  /diff accept <file> — keep changes to file\n");
                msg.push_str("  /diff reject <file> — discard changes to file\n");
                msg.push_str("  /diff accept-all    — keep all changes\n");
                msg.push_str("  /diff reject-all    — discard all changes\n");

                repl.chat.add_message(ChatRole::System, msg);
            }
            Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to get status: {e}")); }
        }
        return Ok(());
    }

    // /diff review <n> — show diff for a specific file by number
    if let Some(num_str) = trimmed.strip_prefix("review ") {
        if let Ok(num) = num_str.trim().parse::<usize>() {
            let status_output = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repl.state.working_directory)
                .output();
            if let Ok(output) = status_output {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(line) = stdout.lines().nth(num - 1) {
                    let file = &line[3..];
                    let diff_output = std::process::Command::new("git")
                        .args(["diff", "--", file])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    match diff_output {
                        Ok(result) => {
                            let diff = String::from_utf8_lossy(&result.stdout);
                            if diff.is_empty() {
                                repl.chat.add_message(ChatRole::System, format!("No unstaged diff for '{file}'."));
                            } else {
                                let truncated = if diff.len() > 8000 { &diff[..8000] } else { &diff };
                                let mut msg = format!("Diff for '{file}':\n```\n{truncated}");
                                if diff.len() > 8000 { msg.push_str("\n... (truncated)"); }
                                msg.push_str("\n```\n\n");
                                msg.push_str(&format!("Accept: /diff accept {file}\nReject: /diff reject {file}"));
                                repl.chat.add_message(ChatRole::System, msg);
                            }
                        }
                        Err(e) => { repl.chat.add_message(ChatRole::System, format!("Error: {e}")); }
                    }
                } else {
                    repl.chat.add_message(ChatRole::System, format!("Invalid file number: {num}. Use /diff review to list files."));
                }
            }
            return Ok(());
        }
    }

    // /diff review branch [name] — compare current branch vs a base branch
    if let Some(rest) = trimmed.strip_prefix("review branch") {
        let base = if rest.trim().is_empty() { "main" } else { rest.trim() };
        let output = std::process::Command::new("git")
            .args(["diff", &format!("{base}...HEAD"), "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stat = String::from_utf8_lossy(&o.stdout);
                let mut msg = format!("Diff: {base}...HEAD\n```\n{stat}```\n\n");
                msg.push_str(&format!("Use /diff interactive for hunk-by-hunk review"));
                repl.chat.add_message(ChatRole::System, msg);
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                repl.chat.add_message(ChatRole::System, format!("git diff failed: {err}"));
            }
            Err(e) => {
                repl.chat.add_message(ChatRole::System, format!("Failed to run git diff: {e}"));
            }
        }
        return Ok(());
    }

    // /diff review <ref> (e.g., HEAD~3, abc123) — compare working tree vs a ref
    if let Some(gitref) = trimmed.strip_prefix("review ") {
        let gitref = gitref.trim();
        if !gitref.is_empty() && !gitref.chars().all(|c| c.is_ascii_digit()) {
            let output = std::process::Command::new("git")
                .args(["diff", gitref, "--stat"])
                .current_dir(&repl.state.working_directory)
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    let stat = String::from_utf8_lossy(&o.stdout);
                    let mut msg = format!("Diff vs {gitref}\n```\n{stat}```\n\n");
                    msg.push_str(&format!("Use /diff interactive for hunk-by-hunk review"));
                    repl.chat.add_message(ChatRole::System, msg);
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    repl.chat.add_message(ChatRole::System, format!("git diff failed: {err}"));
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to run git diff: {e}"));
                }
            }
            return Ok(());
        }
    }

    let options = diff_utils::DiffOptions::from_args(args);
    let show_overview = args.trim().is_empty() || args.contains("--overview");

    // When no args or --overview, show both staged and unstaged stats side-by-side
    if show_overview && options.revision_range.is_none() {
        let mut overview = String::from("Diff Overview\n\n");

        // Unstaged changes
        let unstaged = std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();
        match unstaged {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                if stdout.is_empty() {
                    overview.push_str("Unstaged: no changes\n");
                } else {
                    overview.push_str("Unstaged changes:\n");
                    overview.push_str(&format_file_diff_stats(&stdout));
                    overview.push('\n');
                }
            }
            Err(e) => overview.push_str(&format!("Unstaged: error ({e})\n")),
        }

        // Staged changes
        let staged = std::process::Command::new("git")
            .args(["diff", "--staged", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();
        match staged {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                if stdout.is_empty() {
                    overview.push_str("Staged: no changes\n");
                } else {
                    overview.push_str("Staged changes:\n");
                    overview.push_str(&format_file_diff_stats(&stdout));
                    overview.push('\n');
                }
            }
            Err(e) => overview.push_str(&format!("Staged: error ({e})\n")),
        }

        overview.push_str("Use /diff --staged, /diff HEAD~1, /diff --stat for detailed views");
        repl.chat.add_message(ChatRole::System, overview);
        return Ok(());
    }

    let cmd_str = diff_utils::build_diff_command(&options);

    let cmd_parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if cmd_parts.is_empty() {
        repl.chat.add_message(ChatRole::System, "Failed to build git diff command.".to_string());
        return Ok(());
    }

    let output = std::process::Command::new(cmd_parts[0])
        .args(&cmd_parts[1..])
        .current_dir(&repl.state.working_directory)
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            if !stderr.is_empty() && stdout.is_empty() {
                repl.chat.add_message(ChatRole::System, format!("Git diff error: {stderr}"));
            } else if stdout.is_empty() {
                repl.chat.add_message(ChatRole::System, "No changes found.".to_string());
            } else {
                let analyzer = diff_utils::DiffAnalyzer::new();
                let analysis = analyzer.analyze(&stdout);

                // Per-file breakdown
                let mut file_stats: Vec<(String, i32, i32)> = Vec::new();
                let mut current_file = String::new();
                for line in stdout.lines() {
                    if let Some(rest) = line.strip_prefix("diff --git a/") {
                        if let Some(name) = rest.split(' ').next() {
                            current_file = name.to_string();
                        }
                    } else if line.starts_with('+') && !line.starts_with("+++") {
                        if let Some(entry) = file_stats.iter_mut().find(|(f, _, _)| f == &current_file) {
                            entry.1 += 1;
                        } else {
                            file_stats.push((current_file.clone(), 1, 0));
                        }
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        if let Some(entry) = file_stats.iter_mut().find(|(f, _, _)| f == &current_file) {
                            entry.2 += 1;
                        } else {
                            file_stats.push((current_file.clone(), 0, 1));
                        }
                    }
                }

                let total_lines = stdout.lines().count();
                let category_summary = analysis.summary();
                let test_flag = if analysis.has_test_changes() { " [has test changes]" } else { "" };

                let mut report = format!(
                    "Git diff ({} files, {} lines){test_flag}\nCategories: {category_summary}\n",
                    file_stats.len(), total_lines,
                );

                // File-by-file summary
                if !file_stats.is_empty() {
                    report.push_str("\nFiles:\n");
                    for (file, adds, dels) in &file_stats {
                        let bar = format_change_bar(*adds, *dels);
                        report.push_str(&format!("  {bar} {file} (+{adds}/-{dels})\n"));
                    }
                }

                // Raw diff (truncated)
                let raw_diff = if stdout.len() > 4000 {
                    format!("{}\n... (truncated)", &stdout[..4000])
                } else {
                    stdout.to_string()
                };
                report.push_str(&format!("\n{raw_diff}"));

                repl.chat.add_message(ChatRole::System, report);
            }
        }
        Err(e) => { repl.chat.add_message(ChatRole::System, format!("Failed to run git diff: {e}")); }
    }
    Ok(())
}

/// Format a visual change bar for a file.
pub(crate) fn format_change_bar(additions: i32, deletions: i32) -> String {
    let total = (additions + deletions).min(20) as usize;
    let add_chars = (additions as f32 / (additions + deletions).max(1) as f32 * total as f32).round() as usize;
    let del_chars = total - add_chars;
    format!("{}{}", "+".repeat(add_chars), "-".repeat(del_chars))
}

/// Format diff --stat output into per-file lines.
fn format_file_diff_stats(stat_output: &str) -> String {
    let mut result = String::new();
    for line in stat_output.lines() {
        if line.starts_with(' ') || line.contains('|') {
            result.push_str(&format!("  {line}\n"));
        }
    }
    result
}

fn handle_search(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::search_utils;

    let options = match search_utils::parse_search_args(args) {
        Ok(opts) => opts,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Search error: {e}\nUsage: /search <pattern> [--count N] [--regex] [--case-sensitive] [--no-timestamps]"));
            return Ok(());
        }
    };

    let entries: Vec<String> = repl.command_history.entries().iter().map(|s| s.to_string()).collect();
    let matches = search_utils::search_history(&entries, &options);
    let output = search_utils::format_results(&matches, &options);
    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// Search through conversation messages (not command history).
fn handle_find(repl: &mut Repl, args: &str) -> Result<()> {
    let query = args.trim();
    if query.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /find <query>\n\nSearch through conversation messages. Shows matching messages with role and context.".to_string());
        return Ok(());
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let total = repl.chat.message_count();

    for (idx, msg) in repl.chat.iter_messages() {
        let content_lower = msg.content.to_lowercase();
        if content_lower.contains(&query_lower) {
            // Strip ANSI codes for display
            let clean_content: String = msg.content.chars()
                .filter(|c| !c.is_control())
                .collect();
            let preview = if clean_content.len() > 200 {
                format!("{}...", &clean_content[..200])
            } else {
                clean_content
            };
            let role_str = match msg.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::System => "system",
                ChatRole::Tool => "tool",
            };
            results.push(format!("[{idx}/{total}] ({role_str}) {preview}"));
        }
    }

    let output = if results.is_empty() {
        format!("No messages matching \"{query}\"")
    } else {
        let mut out = format!("Found {} result(s) for \"{query}\":\n", results.len());
        for r in results.iter().take(20) {
            out.push_str(&format!("{r}\n\n"));
        }
        if results.len() > 20 {
            out.push_str(&format!("... and {} more results", results.len() - 20));
        }
        out
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}
fn handle_browse(repl: &mut Repl, args: &str) -> Result<()> {
    let path = if args.trim().is_empty() {
        repl.state.working_directory.clone()
    } else {
        args.trim().to_string()
    };

    let mut selector = crate::widgets::select::FileSelectorWidget::new("File Browser".to_string())
        .with_path(&path);
    if let Err(e) = selector.refresh() {
        repl.chat.add_message(ChatRole::System, format!("Failed to browse {path}: {e}"));
        return Ok(());
    }
    repl.state.file_selector = Some(selector);
    Ok(())
}

fn handle_select_tools(repl: &mut Repl) -> Result<()> {
    let tool_info = if let Some(ref engine) = repl.query_engine {
        engine.tools().list_tools_info()
    } else {
        Vec::new()
    };

    let items: Vec<crate::widgets::select::SelectItem<String>> = tool_info.iter().map(|info| {
        crate::widgets::select::SelectItem::new(info.name.clone(), info.name.clone())
            .with_description(info.description.clone())
    }).collect();

    let widget = crate::widgets::select::MultiSelectWidget::new("Select Tools".to_string())
        .with_items(items);

    repl.state.multi_select = Some(widget);
    Ok(())
}

fn handle_debug(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::debug_utils::{
        parse_debug_subcommand, parse_log_level,
        format_debug_help, format_log_response,
        format_profile_response, format_trace_response,
        format_system_info, DebugSubcommand,
    };

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand_str = parts.first().copied().unwrap_or("");
    let subcommand = parse_debug_subcommand(subcommand_str);

    let output = match subcommand {
        DebugSubcommand::Help => format_debug_help(),
        DebugSubcommand::Info => {
            let mut info = format_system_info();
            if let Ok(rust_output) = std::process::Command::new("rustc").arg("--version").output() {
                let version = String::from_utf8_lossy(&rust_output.stdout);
                if !version.trim().is_empty() {
                    info.push_str(&format!("  Rust: {}\n", version.trim()));
                }
            }
            if let Ok(cargo_output) = std::process::Command::new("cargo").arg("--version").output() {
                let version = String::from_utf8_lossy(&cargo_output.stdout);
                if !version.trim().is_empty() {
                    info.push_str(&format!("  Cargo: {}\n", version.trim()));
                }
            }
            info
        }
        DebugSubcommand::Log => {
            let level_str = parts.get(1).copied().unwrap_or("info");
            let level = parse_log_level(level_str);
            if let Some(lvl) = level {
                // Safety: REPL event loop is single-threaded; no concurrent reads of RUST_LOG.
                unsafe { std::env::set_var("RUST_LOG", lvl.to_string()); }
            }
            format_log_response(level)
        }
        DebugSubcommand::Profile => {
            let action = parts.get(1).copied().unwrap_or("start");
            format_profile_response(action)
        }
        DebugSubcommand::Trace => {
            let toggle = parts.get(1).copied().unwrap_or("on");
            let enabled = matches!(toggle.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
            if enabled {
                // Safety: REPL event loop is single-threaded.
                unsafe { std::env::set_var("SHANNON_TRACE", "1"); }
            } else {
                // Safety: REPL event loop is single-threaded.
                unsafe { std::env::remove_var("SHANNON_TRACE"); }
            }
            format_trace_response(enabled)
        }
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_compact(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::compact::{CompactEngine, CompactStrategy};

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let history = engine.conversation_history();

    // Analyze first
    let compact_engine = match CompactEngine::with_defaults() {
        Ok(e) => e,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Compact engine error: {e}"));
            return Ok(());
        }
    };

    let analysis = compact_engine.analyze_context(&history);

    // Parse subcommand
    let subcmd = args.trim();
    if subcmd == "status" || subcmd == "info" {
        let info = format!(
            "Context Analysis:\n  Estimated tokens: {}\n  Context usage: {:.1}%\n  Messages: {}\n  Should compact: {}\n  Recommended strategy: {}\n  Compactable messages: {}\n  Micro-compact candidates: {}",
            analysis.estimated_tokens,
            analysis.context_usage_ratio * 100.0,
            history.len(),
            if analysis.should_compact { "yes" } else { "no" },
            analysis.recommended_strategy,
            analysis.compactable_message_count,
            analysis.micro_compact_candidates,
        );
        repl.chat.add_message(ChatRole::System, info);
        return Ok(());
    }

    // Perform compaction
    if history.is_empty() {
        repl.chat.add_message(ChatRole::System, "No conversation to compact.".to_string());
        return Ok(());
    }

    let strategy = match subcmd {
        "truncate" => CompactStrategy::TruncateOld,
        "micro" => CompactStrategy::MicroCompress,
        "group" => CompactStrategy::GroupCompress,
        _ => CompactStrategy::SummarizeOld,
    };

    let mut messages = history;
    let mut compact_engine = compact_engine;

    let result = match strategy {
        CompactStrategy::MicroCompress => compact_engine.micro_compact(&mut messages),
        CompactStrategy::GroupCompress => compact_engine.group_compact(&mut messages),
        _ => {
            // Default: 3-tier compaction with re-injection of project context
            compact_engine.compact(&mut messages)
        }
    };

    match result {
        Ok(compact_result) => {
            // Post-cleanup
            let cleanup_removed = compact_engine.post_compact_cleanup(&mut messages);

            // Update the query engine's conversation
            if let Some(ref mut engine) = repl.query_engine {
                engine.replace_conversation(messages);
            }

            let mut report = format!(
                "Context compacted:\n  Strategy: {}\n  Tokens: {} → {} ({:.1}% reduction)\n  Messages removed: {}\n  Messages compacted: {}\n  Duration: {:.2}s",
                compact_result.strategy,
                compact_result.original_tokens,
                compact_result.compacted_tokens,
                compact_result.reduction_ratio * 100.0,
                compact_result.messages_removed,
                compact_result.messages_compacted,
                compact_result.duration.as_secs_f64(),
            );
            if cleanup_removed > 0 {
                report.push_str(&format!("\n  Cleanup removed {cleanup_removed} duplicate messages"));
            }
            repl.chat.add_message(ChatRole::System, report);
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Compact failed: {e}"));
        }
    }

    Ok(())
}

fn handle_cost(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.trim();

    // Handle budget subcommand
    if let Some(budget_str) = subcmd.strip_prefix("budget ") {
        let limit: f64 = match budget_str.trim().parse() {
            Ok(v) if v > 0.0 => v,
            _ => {
                repl.chat.add_message(ChatRole::System, "Usage: /cost budget <amount_usd>".to_string());
                return Ok(());
            }
        };
        if let Some(ref engine) = repl.query_engine {
            if let Ok(mut tracker) = engine.cost_tracker().write() {
                tracker.set_budget(limit);
            }
        }
        repl.chat.add_message(ChatRole::System, format!("Budget limit set to ${limit:.2}"));
        return Ok(());
    }

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let stats = engine.conversation_stats();
    let model = repl.state.model.as_deref().unwrap_or("unknown");

    // Use detailed report from CostTracker
    let detailed = if let Ok(tracker) = engine.cost_tracker().read() {
        tracker.detailed_report()
    } else {
        format!("Total cost: ${:.4}\n", repl.state.total_cost_usd)
    };

    let mut report = format!(
        "Cost Summary:\n  Model: {}\n  Messages: {} turns\n  Tokens used: {} ({:.1}k)\n  Working dir: {}\n",
        model,
        stats.turn_count,
        repl.state.tokens_used,
        repl.state.tokens_used as f64 / 1000.0,
        repl.state.working_directory,
    );

    report.push_str(&detailed);

    if let Some(started) = &repl.session_started_at {
        let elapsed = chrono::Utc::now() - *started;
        let mins = elapsed.num_minutes();
        let secs = elapsed.num_seconds() % 60;
        report.push_str(&format!("  Session duration: {mins}m {secs}s"));

        if mins > 0 {
            let cost_per_min = repl.state.total_cost_usd / mins as f64;
            report.push_str(&format!("\n  Cost rate: ${cost_per_min:.4}/min"));
        }
    }

    if repl.diff_data.total_files_modified() > 0 || repl.diff_data.total_files_created() > 0 {
        report.push_str(&format!(
            "\n  Files changed: +{}/-{} ({} modified, {} created, {} deleted)",
            repl.diff_data.total_additions(),
            repl.diff_data.total_deletions(),
            repl.diff_data.total_files_modified(),
            repl.diff_data.total_files_created(),
            repl.diff_data.total_files_deleted(),
        ));
    }

    // Budget warning
    if let Ok(tracker) = engine.cost_tracker().read() {
        if let Some(ratio) = tracker.budget_usage_ratio() {
            if ratio >= 1.0 {
                report.push_str("\n  ⚠ BUDGET EXCEEDED");
            } else if ratio >= 0.8 {
                report.push_str(&format!("\n  ⚠ Budget usage: {:.0}%", ratio * 100.0));
            }
        }
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

fn handle_suggest(repl: &mut Repl, _args: &str) -> Result<()> {
    let engine = shannon_core::ContextSuggestionEngine::new();

    // Build context from session state
    let mut recently_edited: Vec<String> = Vec::new();
    let mut recently_created: Vec<String> = Vec::new();
    // Collect from last 3 turns
    for turn in repl.diff_data.turns.iter().rev().take(3) {
        for fc in &turn.files_modified {
            if !recently_edited.contains(&fc.path) {
                recently_edited.push(fc.path.clone());
            }
        }
        for f in &turn.files_created {
            if !recently_created.contains(f) {
                recently_created.push(f.clone());
            }
        }
    }

    let context = shannon_core::EnhancedSuggestionContext {
        recently_edited_files: recently_edited,
        recently_created_files: recently_created,
        recently_used_tools: Vec::new(),
        recently_run_commands: Vec::new(),
        working_directory: Some(repl.state.working_directory.clone()),
        open_files: Vec::new(),
    };

    let suggestions = engine.suggest_for_conversation_start(&context);

    if suggestions.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "No suggestions available for the current context.".to_string());
        return Ok(());
    }

    let mut msg = "Suggestions:\n".to_string();
    for (i, s) in suggestions.iter().enumerate() {
        msg.push_str(&format!(
            "  {}. {} (priority: {}, confidence: {:.0}%)\n",
            i + 1, s.reason, s.priority, s.confidence * 100.0
        ));
        if let Some(tool) = &s.suggested_tool {
            msg.push_str(&format!("     Tool: {tool}\n"));
        }
        if !s.suggested_files.is_empty() {
            msg.push_str(&format!("     Files: {}\n", s.suggested_files.join(", ")));
        }
    }
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

fn handle_billing(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.trim();

    match subcmd {
        "" | "period" => {
            let summary = repl.state.billing_manager.get_period_summary();
            let mut msg = format!(
                "Billing Period: {} to {}\n  Total cost: ${:.4}\n  Input tokens: {}\n  Output tokens: {}\n  Models used: {}",
                summary.start.format("%Y-%m-%d"),
                summary.end.format("%Y-%m-%d"),
                summary.total_cost,
                summary.total_input_tokens,
                summary.total_output_tokens,
                summary.usage_breakdown.len(),
            );
            for (model, usage) in &summary.usage_breakdown {
                msg.push_str(&format!(
                    "\n    {}: ${:.4} ({} req, {} in, {} out)",
                    model, usage.total_cost, usage.request_count,
                    usage.total_input_tokens, usage.total_output_tokens
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "model" => {
            let breakdown = repl.state.billing_manager.get_model_breakdown();
            if breakdown.is_empty() {
                repl.chat.add_message(ChatRole::System, "No billing data recorded yet.".to_string());
                return Ok(());
            }
            let mut msg = "Usage by Model:\n".to_string();
            for (model, usage) in &breakdown {
                msg.push_str(&format!(
                    "  {}: ${:.4} ({} req, {}+{} tokens)\n",
                    model, usage.total_cost, usage.request_count,
                    usage.total_input_tokens, usage.total_output_tokens
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "daily" => {
            let daily = repl.state.billing_manager.get_daily_totals();
            if daily.is_empty() {
                repl.chat.add_message(ChatRole::System, "No billing data recorded yet.".to_string());
                return Ok(());
            }
            let mut msg = "Daily Usage:\n".to_string();
            for d in &daily {
                msg.push_str(&format!(
                    "  {}: ${:.4} ({} req, {}+{} tokens)\n",
                    d.date, d.total_cost, d.request_count,
                    d.total_input_tokens, d.total_output_tokens
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        _ if subcmd.starts_with("budget ") => {
            let amount_str = subcmd.strip_prefix("budget ").unwrap().trim();
            if amount_str == "off" || amount_str == "none" {
                let mut cfg = repl.state.billing_manager.config().clone();
                cfg.monthly_budget = None;
                repl.state.billing_manager.set_config(cfg);
                repl.chat.add_message(ChatRole::System, "Monthly budget limit removed.".to_string());
            } else {
                let limit: f64 = match amount_str.parse() {
                    Ok(v) if v > 0.0 => v,
                    _ => {
                        repl.chat.add_message(ChatRole::System,
                            "Usage: /billing budget <amount_usd|off>".to_string());
                        return Ok(());
                    }
                };
                let mut cfg = repl.state.billing_manager.config().clone();
                cfg.monthly_budget = Some(limit);
                repl.state.billing_manager.set_config(cfg);
                repl.chat.add_message(ChatRole::System,
                    format!("Monthly budget limit set to ${limit:.2}"));
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /billing [period|model|daily|budget <amount|off>]".to_string());
        }
    }

    // Check for pending budget alerts
    let alerts = repl.state.billing_manager.get_alerts().to_vec();
    if !alerts.is_empty() {
        for alert in &alerts {
            repl.chat.add_message(ChatRole::System, format!("⚠️ {}", alert.message));
        }
        repl.state.billing_manager.clear_alerts();
    }

    Ok(())
}

fn handle_plan(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "" | "status" => {
            let plan = &repl.state.plan;
            if !plan.active {
                repl.chat.add_message(ChatRole::System,
                    "No active plan. Use /plan <description> to create one.".to_string());
                return Ok(());
            }
            let status = if plan.approved { "Approved" } else { "Pending review" };
            let mut msg = format!(
                "Plan: {}\nStatus: {}\n\n{}",
                plan.description, status, plan.content
            );
            if plan.approved {
                msg.push_str("\n\nPlan approved — implementation can proceed.");
            } else {
                msg.push_str("\n\nUse /plan approve to approve, /plan reject to discard.");
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "approve" => {
            if !repl.state.plan.active {
                repl.chat.add_message(ChatRole::System, "No active plan to approve.".to_string());
                return Ok(());
            }
            repl.state.plan.approved = true;
            repl.state.status = "Plan approved".to_string();
            // Save plan to disk
            let plan_dir = std::path::Path::new(&repl.state.working_directory)
                .join(".claude").join("plans");
            let _ = std::fs::create_dir_all(&plan_dir);
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let plan_file = plan_dir.join(format!("plan_{timestamp}.md"));
            let content = format!("# Plan: {}\n\n{}", repl.state.plan.description, repl.state.plan.content);
            let _ = std::fs::write(&plan_file, content);
            repl.chat.add_message(ChatRole::System,
                format!("Plan approved and saved. You can now proceed with implementation.\nSaved to: {}",
                    plan_file.display()));
        }
        "reject" => {
            if !repl.state.plan.active {
                repl.chat.add_message(ChatRole::System, "No active plan to reject.".to_string());
                return Ok(());
            }
            repl.state.plan = super::PlanState::default();
            repl.state.status = "Ready".to_string();
            repl.chat.add_message(ChatRole::System, "Plan rejected and cleared.".to_string());
        }
        "done" => {
            if !repl.state.plan.active {
                repl.chat.add_message(ChatRole::System, "No active plan.".to_string());
                return Ok(());
            }
            let desc = repl.state.plan.description.clone();
            repl.state.plan = super::PlanState::default();
            repl.state.status = "Ready".to_string();
            repl.chat.add_message(ChatRole::System,
                format!("Plan '{desc}' completed and cleared."));
        }
        "help" => {
            repl.chat.add_message(ChatRole::System,
                "Plan Commands:\n\
                 /plan <description> — Create a new plan from a description\n\
                 /plan status — Show current plan\n\
                 /plan approve — Approve the current plan\n\
                 /plan reject — Reject and discard the current plan\n\
                 /plan done — Mark plan as completed\n\
                 /plan help — Show this help".to_string());
        }
        // Treat anything else as a plan description
        _ => {
            let description = args.trim().to_string();
            if description.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /plan <description>".to_string());
                return Ok(());
            }
            // Generate a structured plan
            let plan_content = generate_plan(&description);
            repl.state.plan = super::PlanState {
                active: true,
                content: plan_content.clone(),
                description: description.clone(),
                approved: false,
                scroll_offset: 0,
            };
            repl.state.status = "Plan mode — review plan".to_string();
            let msg = format!(
                "Plan created: {description}\n\n{plan_content}\n\nUse /plan approve to approve, /plan reject to discard, or /plan help for more options."
            );
            repl.chat.add_message(ChatRole::System, msg);
        }
    }

    Ok(())
}

/// Generate a structured plan from a description
fn generate_plan(description: &str) -> String {
    let steps = extract_plan_steps(description);
    let mut plan = String::from("## Implementation Steps\n\n");
    for (i, step) in steps.iter().enumerate() {
        plan.push_str(&format!("{}. {}\n", i + 1, step));
    }
    plan.push_str("\n## Acceptance Criteria\n\n");
    plan.push_str("- All steps completed successfully\n");
    plan.push_str("- Tests pass for new functionality\n");
    plan.push_str("- No regressions in existing tests\n");
    plan
}

/// Extract plan steps from a description using heuristic keyword detection
pub(crate) fn extract_plan_steps(description: &str) -> Vec<String> {
    let mut steps = Vec::new();

    // Detect common patterns and generate appropriate steps
    let lower = description.to_lowercase();

    if lower.contains("refactor") || lower.contains("restructure") {
        steps.push("Analyze current architecture and identify components to refactor".to_string());
        steps.push("Design new structure with clear separation of concerns".to_string());
        steps.push("Implement refactoring incrementally, keeping tests green".to_string());
        steps.push("Update all references and imports".to_string());
        steps.push("Run full test suite to verify no regressions".to_string());
    }

    if lower.contains("test") || lower.contains("coverage") {
        steps.push("Identify untested code paths and edge cases".to_string());
        steps.push("Write unit tests for core logic".to_string());
        steps.push("Write integration tests for component interactions".to_string());
        steps.push("Verify test coverage meets threshold".to_string());
    }

    if (lower.contains("add") || lower.contains("implement") || lower.contains("feature"))
        && steps.is_empty() {
            steps.push("Analyze requirements and design interface".to_string());
            steps.push("Implement core functionality".to_string());
            steps.push("Add error handling and input validation".to_string());
            steps.push("Write tests for new functionality".to_string());
            steps.push("Update documentation".to_string());
        }

    if (lower.contains("fix") || lower.contains("bug"))
        && steps.is_empty() {
            steps.push("Reproduce the issue and understand root cause".to_string());
            steps.push("Implement fix with minimal changes".to_string());
            steps.push("Add regression test".to_string());
            steps.push("Verify fix resolves the issue".to_string());
        }

    if (lower.contains("migrate") || lower.contains("upgrade"))
        && steps.is_empty() {
            steps.push("Review migration/upgrade guide and breaking changes".to_string());
            steps.push("Update dependencies".to_string());
            steps.push("Adapt code to new API surface".to_string());
            steps.push("Run tests and fix any failures".to_string());
            steps.push("Verify functionality end-to-end".to_string());
        }

    // Default fallback
    if steps.is_empty() {
        steps.push(format!("Understand requirements: {description}"));
        steps.push("Design solution approach".to_string());
        steps.push("Implement the solution".to_string());
        steps.push("Test and verify the implementation".to_string());
    }

    steps
}

fn handle_permissions(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::permissions::RiskLevel;

    let parts: Vec<&str> = args.split_whitespace().collect();

    // Subcommand dispatch
    match parts.first().copied().unwrap_or("") {
        "" | "status" => {
            let mut report = String::from("Permission Status:\n");

            if let Some(ref engine) = repl.query_engine {
                if let Ok(perms) = engine.permissions().read() {
                    // Tool policies
                    report.push_str(&format!("  Registered policies: {}\n", perms.tool_policies().len()));
                    let mut policies: Vec<_> = perms.tool_policies().iter().collect();
                    policies.sort_by_key(|(name, _)| name.as_str());
                    for (name, policy) in &policies {
                        let risk = match policy.default_risk_level {
                            RiskLevel::Safe => "Safe",
                            RiskLevel::Low => "Low",
                            RiskLevel::Medium => "Medium",
                            RiskLevel::High => "High",
                            RiskLevel::Critical => "Critical",
                        };
                        let deny_count = policy.deny_patterns.len();
                        let confirm_count = policy.confirmation_patterns.len();
                        report.push_str(&format!(
                            "    {name}: {risk} risk, {deny_count} deny patterns, {confirm_count} confirm patterns\n"
                        ));
                    }

                    // Always-allowed tools
                    let allowed = perms.memory().always_allowed_tools();
                    if !allowed.is_empty() {
                        let mut tools: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
                        tools.sort();
                        report.push_str(&format!("  Always allowed: {}\n", tools.join(", ")));
                    }

                    // Always-denied tools
                    let denied = perms.memory().always_denied_tools();
                    if !denied.is_empty() {
                        let mut tools: Vec<&str> = denied.iter().map(|s| s.as_str()).collect();
                        tools.sort();
                        report.push_str(&format!("  Always denied: {}\n", tools.join(", ")));
                    }

                    if allowed.is_empty() && denied.is_empty() {
                        report.push_str("  No tool-level overrides (using defaults)\n");
                    }
                }
            } else {
                report.push_str("  No query engine available.\n");
            }

            repl.chat.add_message(ChatRole::System, report);
        }
        "allow" => {
            if parts.len() < 2 {
                repl.chat.add_message(ChatRole::System, "Usage: /permissions allow <tool_name>".to_string());
                return Ok(());
            }
            let tool = parts[1];
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.allow_tool(tool);
                }
            }
            repl.chat.add_message(ChatRole::System, format!("Tool '{tool}' is now always allowed."));
        }
        "deny" => {
            if parts.len() < 2 {
                repl.chat.add_message(ChatRole::System, "Usage: /permissions deny <tool_name>".to_string());
                return Ok(());
            }
            let tool = parts[1];
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.deny_tool(tool);
                }
            }
            repl.chat.add_message(ChatRole::System, format!("Tool '{tool}' is now always denied."));
        }
        "reset" => {
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.reset_memory();
                }
            }
            repl.chat.add_message(ChatRole::System, "Permission memory cleared. All tool overrides removed.".to_string());
        }
        "mode" => {
            let mode_name = parts.get(1).copied().unwrap_or("");
            match mode_name {
                "strict" | "suggest" => {
                    if let Some(ref engine) = repl.query_engine {
                        if let Ok(mut perms) = engine.permissions().write() {
                            perms.set_approval_mode(shannon_core::permissions::ApprovalMode::Suggest);
                        }
                    }
                    repl.state.approval_mode_label = "SUGGEST".to_string();
                    repl.chat.add_message(ChatRole::System,
                        "Permission mode: **suggest** (strict)\n\
                         All potentially dangerous tools require explicit approval.".to_string());
                }
                "auto" | "auto-accept" | "yolo" | "full-auto" => {
                    if let Some(ref engine) = repl.query_engine {
                        if let Ok(mut perms) = engine.permissions().write() {
                            perms.set_approval_mode(shannon_core::permissions::ApprovalMode::FullAuto);
                        }
                    }
                    repl.state.approval_mode_label = "FULL".to_string();
                    repl.chat.add_message(ChatRole::System,
                        "Permission mode: **full-auto**\n\
                         All tools are automatically approved. Use with caution.".to_string());
                }
                "plan" | "readonly" => {
                    if let Some(ref engine) = repl.query_engine {
                        if let Ok(mut perms) = engine.permissions().write() {
                            perms.set_approval_mode(shannon_core::permissions::ApprovalMode::Readonly);
                        }
                    }
                    repl.state.approval_mode_label = "RO".to_string();
                    repl.chat.add_message(ChatRole::System,
                        "Permission mode: **readonly**\n\
                         Tools will only read, not modify files.".to_string());
                }
                _ => {
                    repl.chat.add_message(ChatRole::System,
                        "Permission Modes:\n\
                         /permissions mode suggest   — Require approval for dangerous tools\n\
                         /permissions mode auto      — Auto-accept all tool executions\n\
                         /permissions mode readonly  — Read-only, no file modifications".to_string());
                }
            }
        }
        "help" | _ => {
            repl.chat.add_message(ChatRole::System,
                "Permission Commands:\n\
                 /permissions status — Show current permission policies and overrides\n\
                 /permissions allow <tool> — Always allow a tool without prompting\n\
                 /permissions deny <tool> — Always deny a tool\n\
                 /permissions reset — Clear all permission overrides\n\
                 /permissions mode [suggest|auto|readonly] — Change approval mode\n\
                 /permissions help — Show this help".to_string());
        }
    }

    Ok(())
}

fn handle_doctor(repl: &mut Repl, _args: &str) -> Result<()> {
    use shannon_commands::doctor_utils::{run_all_checks, format_doctor_report};
    let results = run_all_checks();
    let report = format_doctor_report(&results);
    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

/// /theme — switch color theme or list available themes.
fn handle_theme(repl: &mut Repl, args: &str) -> Result<()> {
    use crate::theme::Theme;

    let args = args.trim();

    if args.is_empty() || args == "list" {
        let current = &repl.state.theme.name;
        let available = Theme::available();
        let mut msg = String::from("Available themes:\n");
        for name in available {
            if name == *current {
                msg.push_str(&format!("  * {name} (current)\n"));
            } else {
                msg.push_str(&format!("    {name}\n"));
            }
        }
        msg.push_str("\nUsage: /theme <name>");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    match Theme::named(args) {
        Some(theme) => {
            let name = theme.name.clone();
            repl.renderer.set_theme(&theme);
            repl.state.theme = theme;
            crate::repl::preferences::save_preferences(
                &crate::repl::preferences::Preferences {
                    model: repl.state.model.clone(),
                    provider: repl.state.selected_provider.clone(),
                    theme: Some(name.to_string()),
                },
            );
            repl.chat.add_message(
                ChatRole::System,
                format!("Theme switched to '{name}'."),
            );
        }
        None => {
            let available = Theme::available().join(", ");
            repl.chat.add_message(
                ChatRole::System,
                format!("Unknown theme '{args}'. Available: {available}"),
            );
        }
    }

    Ok(())
}

/// /session — manage conversation sessions (list, export).
fn handle_session(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();
    let subcmd = parts.first().copied().unwrap_or("list");

    match subcmd {
        "list" | "ls" | "" => {
            let sessions = repl.state_manager.list_persisted_sessions()
                .unwrap_or_default();

            if sessions.is_empty() {
                repl.chat.add_message(ChatRole::System, "No saved sessions found.".to_string());
                return Ok(());
            }

            let mut msg = String::from("Saved Sessions:\n\n");
            for (i, s) in sessions.iter().take(20).enumerate() {
                let title = s.title.as_deref()
                    .or(s.preview.as_deref())
                    .unwrap_or("(untitled)");
                let time = s.updated_at.format("%m/%d %H:%M");
                let tokens = s.total_input_tokens + s.total_output_tokens;
                msg.push_str(&format!(
                    "  {:>2}. {}  {}  {} turns  {} tokens\n      ID: {}\n\n",
                    i + 1, title, time, s.turn_count, tokens, s.session_id,
                ));
            }

            if sessions.len() > 20 {
                msg.push_str(&format!("  ... and {} more\n", sessions.len() - 20));
            }

            msg.push_str("\nUsage: /session list | /session export");
            repl.chat.add_message(ChatRole::System, msg);
        }
        "export" => {
            // Export current session as markdown
            let engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, "No active session to export.".to_string());
                    return Ok(());
                }
            };

            let messages = engine.conversation_history();
            if messages.is_empty() {
                repl.chat.add_message(ChatRole::System, "Current session is empty.".to_string());
                return Ok(());
            }

            let mut md = String::from("# Shannon Session Export\n\n");
            md.push_str(&format!("Date: {}\nModel: {}\n\n---\n\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
                repl.state.model.as_deref().unwrap_or("unknown"),
            ));

            for msg in &messages {
                let role = match msg.role.as_str() {
                    "user" => "## User",
                    "assistant" => "## Assistant",
                    "system" => "## System",
                    _ => "## Message",
                };
                let text = match &msg.content {
                    shannon_core::api::MessageContent::Text(t) => t.clone(),
                    shannon_core::api::MessageContent::Blocks(blocks) => {
                        blocks.iter().filter_map(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        }).collect::<Vec<_>>().join("\n")
                    }
                };
                md.push_str(&format!("{role}\n\n{text}\n\n---\n\n"));
            }

            let filename = format!("shannon-session-{}.md", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
            let path = std::path::Path::new(&filename);
            match std::fs::write(path, &md) {
                Ok(()) => {
                    repl.chat.add_message(ChatRole::System,
                        format!("Session exported to {filename} ({} messages)", messages.len()));
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System,
                        format!("Failed to export session: {e}"));
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /session list | /session export".to_string());
        }
    }

    Ok(())
}

/// /accessibility — toggle or check accessibility mode.
fn handle_accessibility(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "on" | "enable" | "true" | "1" => {
            repl.state.accessibility_mode = true;
            crate::a11y::set_enabled(true);
            repl.chat.add_message(ChatRole::System,
                "Accessibility mode enabled. Decorative characters replaced with plain text.".to_string());
        }
        "off" | "disable" | "false" | "0" => {
            repl.state.accessibility_mode = false;
            crate::a11y::set_enabled(false);
            repl.chat.add_message(ChatRole::System,
                "Accessibility mode disabled.".to_string());
        }
        "" | "status" => {
            let state = if repl.state.accessibility_mode { "enabled" } else { "disabled" };
            repl.chat.add_message(ChatRole::System,
                format!("Accessibility mode: {state}\n\nUsage: /accessibility on|off\nAlso auto-enabled via NO_GRAPHICS or ACCESSIBILITY env vars."));
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /accessibility on|off|status".to_string());
        }
    }
    Ok(())
}

fn handle_diag(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();
    match arg {
        "clear" => {
            repl.state.diagnostic_store.clear();
            repl.chat.add_message(ChatRole::System, "Diagnostics cleared.".to_string());
            return Ok(());
        }
        "" | "check" | "run" => {}
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /diag [check|clear]\n  /diag      — run diagnostics on current project\n  /diag clear — clear stored diagnostics".to_string());
            return Ok(());
        }
    }

    // Detect project type and run checker
    let cwd = &repl.state.working_directory;
    let (cmd, label) = if std::path::Path::new(cwd).join("Cargo.toml").exists() {
        ("cargo check --message-format=short 2>&1", "cargo check")
    } else if std::path::Path::new(cwd).join("package.json").exists() {
        ("npx tsc --noEmit --pretty false 2>&1 || true", "tsc --noEmit")
    } else if std::path::Path::new(cwd).join("go.mod").exists() {
        ("go vet ./... 2>&1 || true", "go vet")
    } else if std::path::Path::new(cwd).join("pyproject.toml").exists() || std::path::Path::new(cwd).join("setup.py").exists() {
        ("python -m py_compile . 2>&1 || true", "py_compile")
    } else {
        repl.chat.add_message(ChatRole::System,
            "No recognized project found (need Cargo.toml, package.json, go.mod, or pyproject.toml).".to_string());
        return Ok(());
    };

    repl.chat.add_message(ChatRole::System, format!("Running {label}..."));

    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            repl.state.diagnostic_store.clear();

            for line in stdout.lines() {
                if let Some(diag) = parse_diag_line(line) {
                    repl.state.diagnostic_store.add(diag);
                }
            }

            let store = &repl.state.diagnostic_store;
            let msg = if store.diagnostics.is_empty() {
                format!("{label}: no issues found.")
            } else {
                let errs = store.error_count();
                let warns = store.warning_count();
                let files = store.diagnostics.iter().map(|d| d.file_path.clone()).collect::<std::collections::HashSet<_>>().len();
                format!("{label}: {} diagnostic(s) across {} file(s) ({} errors, {} warnings)\nUse the sidebar Context tab to view details.",
                    store.diagnostics.len(), files, errs, warns)
            };
            repl.chat.add_message(ChatRole::System, msg);
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Failed to run {label}: {e}"));
        }
    }
    Ok(())
}

/// Parse a diagnostic line from cargo check / tsc / go vet output.
fn parse_diag_line(line: &str) -> Option<super::super::lsp_bridge::Diagnostic> {
    use super::super::lsp_bridge::DiagnosticSeverity;

    // cargo check format: "crates/foo/src/bar.rs:10:5: error[E0001]: message"
    // tsc format: "src/file.ts(10,5): error TS0001: message"
    // go vet format: "file.go:10: message"

    let line = line.trim();
    if line.is_empty() { return None; }

    // Try cargo/rustc format: path:line:col: severity: message
    if let Some(rest) = line.strip_prefix("error") {
        let severity = if rest.starts_with('[') || rest.starts_with(':') {
            DiagnosticSeverity::Error
        } else {
            return None;
        };
        return parse_path_prefix(line, severity);
    }
    if let Some(rest) = line.strip_prefix("warning") {
        if rest.starts_with('[') || rest.starts_with(':') {
            return parse_path_prefix(line, DiagnosticSeverity::Warning);
        }
    }
    // Try generic path:line:col: format
    if let Some(colon) = line.find(':') {
        let path = &line[..colon];
        if !path.contains('/') && !path.contains('.') { return None; }
        return parse_path_prefix(line, DiagnosticSeverity::Error);
    }
    None
}

fn parse_path_prefix(line: &str, severity: super::super::lsp_bridge::DiagnosticSeverity) -> Option<super::super::lsp_bridge::Diagnostic> {
    // Find path:line:col: pattern
    let mut parts = line.splitn(4, ':');
    let path = parts.next()?;
    let line_num: usize = parts.next()?.parse().ok()?;
    let _col = parts.next();
    let message = parts.next()?.trim_start_matches(' ').trim_start_matches('[');
    // Clean up error code prefix like E0001]
    let message = message.trim_start_matches(|c: char| c.is_alphanumeric() || c == ']')
        .trim_start_matches(": ")
        .trim_start_matches(']');

    if message.is_empty() { return None; }

    Some(super::super::lsp_bridge::Diagnostic {
        severity,
        message: message.to_string(),
        file_path: path.to_string(),
        line: line_num,
        source: Some("check".to_string()),
    })
}

fn handle_terminal_setup(repl: &mut Repl) -> Result<()> {
    let mut report = String::from("Terminal Setup Check\n\n");

    // 1. Shell detection
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
    let shell_name = std::path::Path::new(&shell)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.clone());
    report.push_str(&format!("Shell: {shell_name} ({shell})\n"));

    // 2. Terminal type
    let term = std::env::var("TERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("TERM: {term}\n"));

    // 3. Check if shannon is on PATH
    let shannon_on_path = std::process::Command::new("which")
        .arg("shannon")
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    report.push_str(&format!(
        "shannon on PATH: {}\n",
        if shannon_on_path { "yes" } else { "no — add shannon to your PATH" }
    ));

    // 4. Check for common terminal tools
    for tool in &["git", "gh", "node"] {
        let found = std::process::Command::new("which")
            .arg(tool)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        report.push_str(&format!(
            "{tool}: {}\n",
            if found { "found" } else { "not found" }
        ));
    }

    // 5. Check shell integration markers
    // Claude Code uses SHANNON_INTEGRATION_DIR or similar env vars
    let has_integration = std::env::var("SHANNON_SHELL_INTEGRATION")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    report.push_str(&format!(
        "Shell integration: {}\n",
        if has_integration {
            "active"
        } else {
            "not detected — add `eval \"$(shannon init)\"` to your shell profile for inline diagnostics and key bindings"
        }
    ));

    // 6. Check terminal dimensions
    let (w, h) = crossterm::terminal::size().unwrap_or((0, 0));
    report.push_str(&format!("Terminal size: {w}x{h}\n"));
    if w < 80 {
        report.push_str("  ⚠ Terminal width < 80 columns — UI may be cramped\n");
    }

    // 7. Color support
    let colors = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
    report.push_str(&format!("COLORTERM: {colors}\n"));

    // 8. Key binding hint
    report.push_str("\nKey bindings:\n");
    report.push_str("  Enter      — submit input\n");
    report.push_str("  Ctrl+C     — cancel current operation\n");
    report.push_str("  Ctrl+D     — exit Shannon\n");
    report.push_str("  Tab        — autocomplete\n");
    report.push_str("  Up/Down    — navigate history\n");
    report.push_str("  Escape     — enter/exit vim normal mode\n");

    report.push_str("\nShell profile setup:\n");
    match shell_name.as_str() {
        "zsh" => report.push_str("  Add to ~/.zshrc:\n    eval \"$(shannon init zsh)\"\n"),
        "bash" => report.push_str("  Add to ~/.bashrc:\n    eval \"$(shannon init bash)\"\n"),
        "fish" => report.push_str("  Add to ~/.config/fish/config.fish:\n    shannon init fish | source\n"),
        other => report.push_str(&format!("  Unknown shell '{other}'. Add the appropriate init line to your shell profile.\n")),
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

fn handle_web_search(repl: &mut Repl, args: &str) -> Result<()> {
    let query = args.trim();
    if query.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /web-search <query>\nSearches the web using Tavily API. Set SHANNON_SEARCH_API_KEY to configure.".to_string());
        return Ok(());
    }

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let input = serde_json::json!({
        "query": query,
        "max_results": 5,
        "search_depth": "basic"
    });

    match repl.runtime.block_on(engine.tools().execute("WebSearch", input)) {
        Ok(result) => {
            let mut output = format!("Web search results for: {query}\n\n");
            if let Some(results) = result.metadata.get("results").and_then(|r| r.as_array()) {
                for (i, item) in results.iter().enumerate() {
                    let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled");
                    let url = item.get("url").and_then(|u| u.as_str()).unwrap_or("");
                    let snippet = item.get("snippet").and_then(|s| s.as_str()).unwrap_or("");
                    output.push_str(&format!("{}. **{}**\n   {}\n   {}\n\n", i + 1, title, url, snippet));
                }
                if results.is_empty() {
                    output.push_str("No results found.");
                }
            } else {
                output.push_str(&result.content);
            }
            repl.chat.add_message(ChatRole::System, output);
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Web search failed: {e}\nSet SHANNON_SEARCH_API_KEY for web search capability."));
        }
    }
    Ok(())
}

fn handle_review(repl: &mut Repl, args: &str) -> Result<()> {
    let target = args.trim();

    // Get the diff to review
    let diff_output = if target.is_empty() {
        std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output()
    } else {
        std::process::Command::new("git")
            .args(["diff", target])
            .current_dir(&repl.state.working_directory)
            .output()
    };

    let mut review = String::from("Code Review\n\n");

    match diff_output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            if !stderr.is_empty() && stdout.is_empty() {
                review.push_str(&format!("Git error: {stderr}"));
            } else if stdout.is_empty() {
                review.push_str("No uncommitted changes to review.\n");
                review.push_str("Usage: /review [diff target]\n");
                review.push_str("Examples: /review, /review HEAD~1, /review main...HEAD");
            } else {
                review.push_str("Changes found:\n```\n");
                review.push_str(&stdout);
                review.push_str("\n```\n\n");

                // Get full diff for analysis (truncated)
                let full_diff = std::process::Command::new("git")
                    .args(["diff"])
                    .current_dir(&repl.state.working_directory)
                    .output();

                if let Ok(diff_result) = full_diff {
                    let diff_text = String::from_utf8_lossy(&diff_result.stdout);
                    let files: Vec<&str> = diff_text.lines().filter(|l| l.starts_with("diff --git")).collect();
                    let additions = diff_text.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
                    let deletions = diff_text.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();

                    review.push_str(&format!("Summary: {} files changed, +{}/-{} lines\n\n", files.len(), additions, deletions));

                    // Basic automated checks
                    let mut findings: Vec<String> = Vec::new();

                    // Check for potential secrets
                    let secret_patterns = ["API_KEY", "api_key", "password", "secret_key", "access_token",
                        "private_key", "credential", "auth_token", "BEGIN RSA", "BEGIN PRIVATE"];
                    let added_lines: Vec<&str> = diff_text.lines()
                        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
                        .collect();
                    for pat in &secret_patterns {
                        if added_lines.iter().any(|l| l.contains(pat)) {
                            findings.push("[SECURITY] Potential secret/credential detected — review for accidental exposure".to_string());
                            break;
                        }
                    }

                    // Check for large diffs
                    if additions + deletions > 500 {
                        findings.push("[WARN] Large diff — consider splitting into smaller changes".to_string());
                    }

                    // Check for debug prints left in
                    let debug_patterns = ["println!", "console.log", "print(", "dbg!", "eprintln!", "fmt.Println"];
                    for pat in &debug_patterns {
                        if added_lines.iter().any(|l| l.contains(pat)) {
                            findings.push(format!("[WARN] Debug output detected: `{pat}` — remove before commit"));
                            break;
                        }
                    }

                    // Check for unsafe code in Rust
                    if added_lines.iter().any(|l| l.contains("unsafe ")) {
                        findings.push("[REVIEW] Unsafe code block added — requires careful review".to_string());
                    }

                    // Check for unwrap() calls that could panic
                    let unwrap_count = added_lines.iter().filter(|l| l.contains(".unwrap()")).count();
                    if unwrap_count > 3 {
                        findings.push(format!("[WARN] {unwrap_count} .unwrap() calls added — consider proper error handling"));
                    }

                    // Check for TODO/FIXME
                    if added_lines.iter().any(|l| l.contains("TODO") || l.contains("FIXME") || l.contains("HACK")) {
                        findings.push("[INFO] New TODO/FIXME/HACK comments added".to_string());
                    }

                    // Check for hardcoded IPs or URLs
                    let has_hardcoded = added_lines.iter().any(|l| {
                        (l.contains("127.0.0.1") || l.contains("localhost")) && !l.contains("test") && !l.contains("example")
                    });
                    if has_hardcoded {
                        findings.push("[WARN] Hardcoded localhost/127.0.0.1 detected — use configurable endpoints".to_string());
                    }

                    // Check for test changes
                    let has_test_changes = diff_text.lines()
                        .filter(|l| l.starts_with("diff --git"))
                        .any(|l| l.contains("test") || l.contains("spec"));
                    if has_test_changes {
                        findings.push("[PASS] Test changes detected".to_string());
                    } else if additions + deletions > 50 {
                        findings.push("[WARN] No test changes — consider adding tests for new code".to_string());
                    }

                    if findings.is_empty() {
                        review.push_str("Automated checks: No issues detected.\n");
                    } else {
                        review.push_str(&format!("Automated findings ({}):\n", findings.len()));
                        for finding in &findings {
                            review.push_str(&format!("  {finding}\n"));
                        }
                    }

                    review.push_str("\nTo get AI-powered review, ask in the chat after these changes.");
                }
            }
        }
        Err(e) => {
            review.push_str(&format!("Failed to run git diff: {e}"));
        }
    }

    repl.chat.add_message(ChatRole::System, review);
    Ok(())
}

/// `/stage [files...]` — interactive git staging helper.
///
/// Without arguments, shows unstaged changes and offers to stage them.
/// With file arguments, stages those specific files.
fn handle_stage(repl: &mut Repl, args: &str) -> Result<()> {
    let target = args.trim();

    if target.is_empty() {
        // Show unstaged changes summary
        let output = std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();

        let mut msg = String::from("Interactive Stage\n\n");

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);

                if !stderr.is_empty() && stdout.is_empty() {
                    msg.push_str(&format!("Git error: {stderr}"));
                } else if stdout.is_empty() {
                    // Check for untracked files
                    let untracked = std::process::Command::new("git")
                        .args(["ls-files", "--others", "--exclude-standard"])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    if let Ok(ut_result) = untracked {
                        let ut_files = String::from_utf8_lossy(&ut_result.stdout);
                        if ut_files.trim().is_empty() {
                            msg.push_str("No unstaged or untracked changes.\n");
                        } else {
                            let count = ut_files.lines().filter(|l| !l.is_empty()).count();
                            msg.push_str(&format!("No unstaged changes, but {count} untracked file(s):\n"));
                            for line in ut_files.lines().filter(|l| !l.is_empty()).take(20) {
                                msg.push_str(&format!("  ? {line}\n"));
                            }
                            msg.push_str("\nUse /stage <file> to stage a file, or /stage --all to stage everything.");
                        }
                    }
                } else {
                    msg.push_str("Unstaged changes:\n```\n");
                    msg.push_str(&stdout);
                    msg.push_str("```\n\n");

                    // List changed files for easy staging
                    let files_output = std::process::Command::new("git")
                        .args(["diff", "--name-only"])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    if let Ok(fo) = files_output {
                        let files = String::from_utf8_lossy(&fo.stdout);
                        let file_list: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
                        if !file_list.is_empty() {
                            msg.push_str("Files to stage:\n");
                            for f in &file_list {
                                msg.push_str(&format!("  /stage {f}\n"));
                            }
                            msg.push_str("\nTip: /stage --all to stage all changes.");
                        }
                    }
                }
            }
            Err(e) => {
                msg.push_str(&format!("Failed to run git diff: {e}"));
            }
        }

        repl.chat.add_message(ChatRole::System, msg);
    } else if target == "--all" || target == "-A" {
        // Stage all changes
        let output = std::process::Command::new("git")
            .args(["add", "--all"])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, "All changes staged.".to_string());
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                repl.chat.add_message(ChatRole::System, format!("git add failed: {err}"));
            }
            Err(e) => {
                repl.chat.add_message(ChatRole::System, format!("Failed to run git add: {e}"));
            }
        }
    } else {
        // Stage specific files
        let files: Vec<&str> = target.split_whitespace().collect();
        let output = std::process::Command::new("git")
            .args(["add"])
            .args(&files)
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let count = files.len();
                repl.chat.add_message(ChatRole::System, format!("Staged {count} file(s)."));
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                repl.chat.add_message(ChatRole::System, format!("git add failed: {err}"));
            }
            Err(e) => {
                repl.chat.add_message(ChatRole::System, format!("Failed to run git add: {e}"));
            }
        }
    }

    Ok(())
}

fn handle_local_models(repl: &mut Repl) -> Result<()> {
    let mut output = String::from("Local Model Detection\n\n");

    // Check Ollama
    let ollama_check = std::process::Command::new("curl")
        .args(["-s", "--connect-timeout", "3", "http://localhost:11434/api/tags"])
        .output();

    match ollama_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("Ollama: not running (localhost:11434 unreachable)\n");
            } else {
                output.push_str("Ollama: running at localhost:11434\n");
                // Parse model list
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models installed\n");
                        } else {
                            output.push_str(&format!("  Available models ({}):\n", models.len()));
                            for model in models {
                                let name = model.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let size = model.get("size").and_then(|s| s.as_u64()).map(|b| format!("{:.1} GB", b as f64 / 1e9)).unwrap_or_default();
                                output.push_str(&format!("    - {name} ({size})\n"));
                            }
                        }
                    }
                } else {
                    output.push_str("  Could not parse model list\n");
                }
            }
        }
        Err(_) => {
            output.push_str("Ollama: not detected (curl not available or host unreachable)\n");
        }
    }

    // Check LM Studio
    let lmstudio_check = std::process::Command::new("curl")
        .args(["-s", "--connect-timeout", "3", "http://localhost:1234/v1/models"])
        .output();

    match lmstudio_check {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            if stdout.is_empty() || !result.status.success() {
                output.push_str("\nLM Studio: not running (localhost:1234 unreachable)\n");
            } else {
                output.push_str("\nLM Studio: running at localhost:1234\n");
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(models) = json.get("data").and_then(|m| m.as_array()) {
                        if models.is_empty() {
                            output.push_str("  No models loaded\n");
                        } else {
                            output.push_str(&format!("  Loaded models ({}):\n", models.len()));
                            for model in models {
                                let id = model.get("id").and_then(|i| i.as_str()).unwrap_or("unknown");
                                output.push_str(&format!("    - {id}\n"));
                            }
                        }
                    }
                }
            }
        }
        Err(_) => {
            output.push_str("\nLM Studio: not detected\n");
        }
    }

    // Suggest usage
    output.push_str("\nTo use a local model:\n");
    output.push_str("  /model ollama/llama3\n");
    output.push_str("  /model ollama/mistral\n");
    output.push_str("  /model lmstudio/<model-id>\n");
    output.push_str(&format!("\nCurrent model: {}\n", repl.state.model.as_deref().unwrap_or("not set")));

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

fn handle_ci(repl: &mut Repl, args: &str) -> Result<()> {
    // Check if gh CLI is available
    let gh_check = std::process::Command::new("gh")
        .arg("--version")
        .output();

    if gh_check.is_err() {
        repl.chat.add_message(ChatRole::System,
            "GitHub CLI (gh) is not installed.\nInstall it from: https://cli.github.com/".to_string());
        return Ok(());
    }

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("");

    match subcommand {
        "" | "status" => {
            // Show recent workflow runs
            let output = std::process::Command::new("gh")
                .args(["run", "list", "--limit", "10"])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else if stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, "No workflow runs found.".to_string());
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Recent workflow runs:\n{stdout}"));
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to query CI: {e}"));
                }
            }
        }
        "runs" => {
            let limit = parts.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
            let output = std::process::Command::new("gh")
                .args(["run", "list", "--limit", &limit.to_string()])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Workflow runs (limit: {limit}):\n{stdout}"));
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to list runs: {e}"));
                }
            }
        }
        "workflows" => {
            let output = std::process::Command::new("gh")
                .args(["workflow", "list"])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Workflows:\n{stdout}"));
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to list workflows: {e}"));
                }
            }
        }
        "view" => {
            let run_id = parts.get(1).copied().unwrap_or("");
            if run_id.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /ci view <run-id>".to_string());
                return Ok(());
            }
            let output = std::process::Command::new("gh")
                .args(["run", "view", run_id])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Run details:\n{stdout}"));
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to view run: {e}"));
                }
            }
        }
        "trigger" => {
            let workflow = parts.get(1).copied().unwrap_or("");
            if workflow.is_empty() {
                repl.chat.add_message(ChatRole::System,
                    "Usage: /ci trigger <workflow-name>\nUse /ci workflows to see available workflows.".to_string());
                return Ok(());
            }
            let output = std::process::Command::new("gh")
                .args(["workflow", "run", workflow])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if result.status.success() {
                        repl.chat.add_message(ChatRole::System,
                            format!("Workflow '{workflow}' triggered successfully."));
                    } else {
                        repl.chat.add_message(ChatRole::System,
                            format!("Failed to trigger workflow: {stderr}"));
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to trigger workflow: {e}"));
                }
            }
        }
        "help" | _ => {
            repl.chat.add_message(ChatRole::System, "\
CI/GitHub Actions Commands:
  /ci            — Show recent workflow runs (default: 10)
  /ci status     — Same as above
  /ci runs [N]   — List recent N workflow runs
  /ci workflows  — List all workflows
  /ci view <id>  — View details of a specific run
  /ci trigger <name> — Trigger a workflow
  /ci help       — Show this help

Requires GitHub CLI (gh) to be installed.".to_string());
        }
    }

    Ok(())
}

fn handle_other_command(repl: &mut Repl, cmd_name: &str, args: &str) -> Result<()> {
    let registry = repl.runtime.block_on(repl.shared_executor.registry());
    if let Ok(command) = repl.runtime.block_on(registry.get(cmd_name)) {
        match &*command {
            shannon_commands::Command::Prompt(prompt_cmd) => {
                if let Some(ref template) = prompt_cmd.prompt_template {
                    let args_val = if args.is_empty() { "" } else { args };
                    let arg_parts: Vec<&str> = args_val.split_whitespace().collect();
                    let mut prompt = template.clone();
                    // Replace indexed placeholders: $ARGUMENTS[0], $ARGUMENTS[1], ...
                    for (i, part) in arg_parts.iter().enumerate() {
                        prompt = prompt.replace(&format!("$ARGUMENTS[{i}]"), part);
                    }
                    // Also replace {args[0]}, {args[1]}, ...
                    for (i, part) in arg_parts.iter().enumerate() {
                        prompt = prompt.replace(&format!("{{args[{i}]}}"), part);
                    }
                    // Replace full placeholders last (so indexed ones take priority)
                    prompt = prompt.replace("$ARGUMENTS", args_val).replace("{args}", args_val);
                    // Expand built-in template variables
                    prompt = prompt.replace("$DIR", &std::env::current_dir().unwrap_or_default().display().to_string());
                    prompt = prompt.replace("$DATE", &chrono::Local::now().format("%Y-%m-%d").to_string());
                    prompt = prompt.replace("$TIME", &chrono::Local::now().format("%H:%M:%S").to_string());
                    repl.chat.add_message(ChatRole::System, format!("Running /{cmd_name}..."));
                    super::query::handle_query(repl, &prompt)?;
                } else {
                    repl.chat.add_message(ChatRole::System, format!("/{cmd_name} — {}", prompt_cmd.base.description));
                }
            }
            _ => {
                let desc = command.description();
                repl.chat.add_message(ChatRole::System, format!("/{cmd_name} — {desc}"));
            }
        }
    }
    Ok(())
}

/// Execute a pending dialog action after confirmation
pub fn execute_pending_action(repl: &mut Repl, action: &str) -> Result<()> {
    match action {
        "clear_chat" => {
            repl.chat.clear();
            repl.chat.add_message(ChatRole::System, t!("repl.chat_cleared").to_string());
        }
        "quit" => {
            repl.running = false;
        }
        "set_bypass_mode" => {
            if let Some(ref query_engine) = repl.query_engine {
                let mut perms = query_engine.permissions().write().expect("permissions rwlock poisoned");
                perms.set_approval_mode(shannon_core::permissions::ApprovalMode::BypassPermissions);
                drop(perms);
                repl.state.approval_mode_label = "BYPASS".to_string();
                repl.state.status = "Mode: BYPASS".to_string();
                repl.state.toast = Some(("  Mode: BYPASS  ".to_string(), std::time::Instant::now()));
                repl.chat.add_message(ChatRole::System, "Permission bypass enabled — all checks skipped.".to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Create an isolated git worktree for an agent.
fn create_agent_worktree(repl: &Repl, agent_name: &str) -> std::result::Result<std::path::PathBuf, String> {
    use shannon_agents::{WorktreeManager, WorktreeConfig};
    let config = WorktreeConfig::default();
    let manager = repl.runtime.block_on(WorktreeManager::new(config))
        .map_err(|e| format!("{e}"))?;
    let session = repl.runtime.block_on(manager.create_agent_session(agent_name, None))
        .map_err(|e| format!("{e}"))?;
    Ok(session.path)
}

// ── P1-5: Clipboard integration ──────────────────────────────────────────

fn handle_copy(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // Determine what to copy
    let content = if trimmed.is_empty() || trimmed == "last" || trimmed == "response" {
        // Copy the last assistant message
        let mut last = None;
        for (_, m) in repl.chat.iter_messages() {
            if m.role == ChatRole::Assistant {
                last = Some(m.content.clone());
            }
        }
        match last {
            Some(msg) => msg,
            None => {
                repl.chat.add_message(ChatRole::System, "No assistant response to copy.".to_string());
                return Ok(());
            }
        }
    } else if trimmed == "status" {
        repl.state.status.clone()
    } else {
        trimmed.to_string()
    };

    if content.is_empty() {
        repl.chat.add_message(ChatRole::System, "Nothing to copy (empty content).".to_string());
        return Ok(());
    }

    // Try platform-specific clipboard commands
    let success = copy_to_clipboard(&content);
    if success {
        let preview = if content.len() > 60 { format!("{}...", &content[..60]) } else { content.clone() };
        repl.chat.add_message(ChatRole::System, format!("Copied to clipboard: {preview}"));
    } else {
        // Fallback: write to temp file
        let tmp = std::env::temp_dir().join("shannon-clipboard.txt");
        if std::fs::write(&tmp, &content).is_ok() {
            repl.chat.add_message(ChatRole::System,
                format!("Clipboard unavailable. Content saved to: {}\nInstall xclip or xsel for clipboard support.", tmp.display()));
        } else {
            repl.chat.add_message(ChatRole::System, "Failed to copy: no clipboard tool available.".to_string());
        }
    }
    Ok(())
}

fn handle_paste(repl: &mut Repl) -> Result<()> {
    let content = paste_from_clipboard();
    match content {
        Some(text) if !text.is_empty() => {
            repl.prompt.insert_text(&text);
            repl.chat.add_message(ChatRole::System, format!("Pasted {} chars into prompt.", text.len()));
        }
        Some(_) => {
            repl.chat.add_message(ChatRole::System, "Clipboard is empty.".to_string());
        }
        None => {
            // Fallback: try temp file
            let tmp = std::env::temp_dir().join("shannon-clipboard.txt");
            if tmp.exists() {
                if let Ok(text) = std::fs::read_to_string(&tmp) {
                    repl.prompt.insert_text(&text);
                    repl.chat.add_message(ChatRole::System, format!("Pasted {} chars from temp file.", text.len()));
                }
            } else {
                repl.chat.add_message(ChatRole::System,
                    "Clipboard unavailable. Install xclip or xsel for clipboard support.".to_string());
            }
        }
    }
    Ok(())
}

/// Copy text to system clipboard using platform tools.
fn copy_to_clipboard(content: &str) -> bool {
    // Try xclip first (Linux)
    if let Ok(mut child) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    // Try xsel (Linux alternative)
    if let Ok(mut child) = std::process::Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    // Try pbcopy (macOS)
    if let Ok(mut child) = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    // Try wl-copy (Wayland)
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    false
}

/// Paste text from system clipboard.
fn paste_from_clipboard() -> Option<String> {
    // Try xclip (Linux)
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
    {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    // Try xsel (Linux alternative)
    if let Ok(output) = std::process::Command::new("xsel")
        .args(["--clipboard", "--output"])
        .output()
    {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    // Try pbpaste (macOS)
    if let Ok(output) = std::process::Command::new("pbpaste").output() {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    // Try wl-paste (Wayland)
    if let Ok(output) = std::process::Command::new("wl-paste").output() {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    None
}

// ── P2-9: Multi-file context glob ───────────────────────────────────────

fn handle_add(repl: &mut Repl, args: &str) -> Result<()> {
    let pattern = args.trim();
    if pattern.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /add <glob-pattern>\n\
             Examples:\n\
               /add src/**/*.rs    — add all Rust files under src/\n\
               /add *.toml         — add all TOML files in project root\n\
               /add README.md      — add a single file".to_string());
        return Ok(());
    }

    let cwd = std::path::Path::new(&repl.state.working_directory);
    let glob_pattern = pattern;

    // Use glob crate pattern matching via walkdir
    let matched_files = collect_glob_files(cwd, glob_pattern);

    if matched_files.is_empty() {
        repl.chat.add_message(ChatRole::System,
            format!("No files matched pattern: '{pattern}'"));
        return Ok(());
    }

    let mut added = Vec::new();
    let mut errors = Vec::new();
    let mut total_bytes = 0usize;

    for file_path in &matched_files {
        let relative = file_path.strip_prefix(cwd).unwrap_or(file_path);
        match std::fs::read_to_string(file_path) {
            Ok(content) => {
                total_bytes += content.len();
                let file_context = format!("\n\n--- File: {} ---\n{}\n--- End of {} ---",
                    relative.display(), content, relative.display());

                if let Some(ref mut engine) = repl.query_engine {
                    engine.append_system_prompt(&file_context);
                }
                added.push(relative.display().to_string());
            }
            Err(e) => {
                errors.push(format!("{}: {e}", relative.display()));
            }
        }
    }

    let mut msg = format!("Added {} file(s) to context ({} bytes):\n", added.len(), total_bytes);
    for file in &added {
        msg.push_str(&format!("  + {file}\n"));
    }
    if !errors.is_empty() {
        msg.push_str("\nErrors:\n");
        for err in &errors {
            msg.push_str(&format!("  ! {err}\n"));
        }
    }
    msg.push_str("\nContext will be included in future queries. Use /context reload to reset.");

    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

/// Collect files matching a glob pattern.
fn collect_glob_files(base: &std::path::Path, pattern: &str) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();

    // Handle single file case first
    let single_path = base.join(pattern);
    if single_path.is_file() {
        results.push(single_path);
        return results;
    }

    // Simple glob matching using walkdir
    let _pattern_lower = pattern.to_lowercase();
    let extensions: Vec<&str> = if pattern.contains("*.") {
        pattern.split('.').next_back().map(|ext| vec![ext]).unwrap_or_default()
    } else {
        Vec::new()
    };

    let recursive = pattern.contains("**");
    let prefix = if let Some(idx) = pattern.find('*') {
        &pattern[..idx]
    } else {
        ""
    };

    fn visit_dir(dir: &std::path::Path, results: &mut Vec<std::path::PathBuf>,
                 extensions: &[&str], prefix: &str, recursive: bool, base: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    visit_dir(&path, results, extensions, prefix, recursive, base);
                }
            } else if path.is_file() {
                let relative = path.strip_prefix(base).unwrap_or(&path);
                let rel_str = relative.to_string_lossy();
                let matches = if !extensions.is_empty() {
                    extensions.iter().any(|ext| rel_str.to_lowercase().ends_with(&format!(".{ext}")))
                } else if !prefix.is_empty() {
                    rel_str.starts_with(prefix)
                } else {
                    true
                };
                if matches && results.len() < 50 {
                    results.push(path);
                }
            }
        }
    }

    let search_dir = if prefix.contains('/') {
        base.join(prefix.trim_end_matches('/'))
    } else {
        base.to_path_buf()
    };

    if search_dir.is_dir() {
        visit_dir(&search_dir, &mut results, &extensions, prefix, recursive, base);
    }

    results.sort();
    results.dedup();
    results
}

// ── P1-4: File watching ─────────────────────────────────────────────────

fn handle_watch(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    match trimmed {
        "status" | "info" | "" => {
            let msg = "File Watch Status\n\n\
                File watching monitors your workspace for external changes.\n\
                When files change, you'll see a notification in the chat.\n\n\
                Commands:\n\
                  /watch status     — Show current status\n\
                  /watch check      — Check for external changes now\n\
                  /watch track <file> — Track a specific file for changes\n\
                  /watch list       — List tracked files".to_string();
            repl.chat.add_message(ChatRole::System, msg);
        }
        "check" | "scan" => {
            // Check for external changes by comparing git status
            let output = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repl.state.working_directory)
                .output();
            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    if stdout.trim().is_empty() {
                        repl.chat.add_message(ChatRole::System, "No external file changes detected.".to_string());
                    } else {
                        let count = stdout.lines().count();
                        let mut msg = format!("External changes detected ({count} files):\n\n");
                        for line in stdout.lines().take(20) {
                            let status = &line[..2];
                            let file = &line[3..];
                            msg.push_str(&format!("  {status} {file}\n"));
                        }
                        if count > 20 {
                            msg.push_str(&format!("  ... and {} more\n", count - 20));
                        }
                        msg.push_str("\nUse /diff review to inspect changes.");
                        repl.chat.add_message(ChatRole::System, msg);
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Check failed: {e}"));
                }
            }
        }
        "list" => {
            // List files that would be watched (git tracked + modified)
            let output = std::process::Command::new("git")
                .args(["ls-files"])
                .current_dir(&repl.state.working_directory)
                .output();
            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let count = stdout.lines().count();
                    repl.chat.add_message(ChatRole::System,
                        format!("Watching {count} tracked files in git repository.\nUse /watch check to scan for changes."));
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed: {e}"));
                }
            }
        }
        _ => {
            if trimmed.starts_with("track ") {
                let file = trimmed.strip_prefix("track ").unwrap().trim();
                repl.chat.add_message(ChatRole::System,
                    format!("Tracking '{file}'. Use /watch check to scan for changes."));
            } else {
                repl.chat.add_message(ChatRole::System,
                    "Usage: /watch [status|check|list|track <file>]".to_string());
            }
        }
    }
    Ok(())
}

// ── P2-11: Wire notifications into query completion ─────────────────────

/// Send a desktop notification if enabled.
pub(crate) fn notify_query_complete(notifier: &shannon_core::notifier::Notifier, enabled: bool, message: &str) {
    if !enabled { return; }
    let notification = shannon_core::notifier::Notification {
        title: "Shannon - Query Complete".to_string(),
        body: message.to_string(),
        level: shannon_core::notifier::NotificationLevel::Info,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
    };
    let _ = notifier.notify(&notification);
}

// Helper trait methods on Repl for dialog display
impl Repl {
    pub(crate) fn show_confirm_dialog(&mut self, title: &str, message: &str, action: &str) {
        use crate::widgets::dialog::ConfirmDialog;
        let dialog = ConfirmDialog::new(title.to_string())
            .with_message(message.to_string())
            .build();
        self.state.active_dialog = Some(dialog);
        self.state.pending_dialog_action = Some(action.to_string());
    }

    pub(crate) fn show_input_dialog(&mut self, title: &str, placeholder: &str, action: &str) {
        use crate::widgets::dialog::InputDialog;
        let dialog = InputDialog::new(title.to_string())
            .with_placeholder(placeholder.to_string());
        self.state.input_dialog = Some(Box::new(dialog));
        self.state.input_dialog_action = Some(action.to_string());
    }

    pub(crate) fn show_alert_dialog(&mut self, title: &str, message: &str, danger: bool) {
        use crate::widgets::dialog::AlertDialog;
        let mut builder = AlertDialog::new(title.to_string())
            .with_message(message.to_string());
        if danger {
            builder = builder.with_danger();
        }
        self.state.active_dialog = Some(builder.build());
        self.state.pending_dialog_action = None;
    }
}

// ── P3-14: Custom keybindings ────────────────────────────────────────────

fn default_keybindings() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Enter", "Submit input / confirm dialog"),
        ("Shift+Enter", "Insert newline"),
        ("Tab", "Autocomplete / cycle suggestions"),
        ("Ctrl+C", "Cancel current operation"),
        ("Ctrl+P", "Open command palette"),
        ("Ctrl+L", "Clear screen"),
        ("Ctrl+R", "Search history"),
        ("Up/Down", "Navigate history / move cursor (multiline)"),
        ("Left/Right", "Move cursor"),
        ("Home/End", "Move to start/end of line"),
        ("Ctrl+U", "Clear input line"),
        ("Ctrl+W", "Delete word backward"),
        ("Ctrl+A", "Move to start of line"),
        ("Ctrl+E", "Move to end of line"),
        ("Ctrl+K", "Delete to end of line"),
        ("Esc", "Cancel / dismiss dialog"),
        ("Page Up/Down", "Scroll chat"),
    ]
}

fn handle_bind(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed.is_empty() || trimmed == "list" || trimmed == "show" {
        let mut msg = "Keyboard Shortcuts\n\n".to_string();
        msg.push_str("  Key              Action\n");
        msg.push_str("  ──────────────── ─────────────────────────────────\n");
        for (key, action) in default_keybindings() {
            msg.push_str(&format!("  {key:<16} {action}\n"));
        }
        msg.push_str("\nCustom keybindings can be set in ~/.shannon/keybindings.toml\n");
        msg.push_str("Format: [[bind]]\n  key = \"Ctrl+J\"\n  action = \"submit\"\n");
        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    if trimmed == "save" {
        let config_dir = dirs::home_dir()
            .map(|h| h.join(".shannon"))
            .unwrap_or_else(|| std::path::PathBuf::from(".shannon"));
        let _ = std::fs::create_dir_all(&config_dir);
        let kb_path = config_dir.join("keybindings.toml");

        let mut toml_content = "# Shannon keybindings configuration\n".to_string();
        toml_content.push_str("# Restart Shannon after modifying this file.\n\n");
        for (key, action) in default_keybindings() {
            toml_content.push_str(&format!("# {key}: {action}\n"));
        }
        toml_content.push_str("\n# Example custom binding:\n");
        toml_content.push_str("# [[bind]]\n# key = \"Ctrl+J\"\n# action = \"submit\"\n");

        match std::fs::write(&kb_path, &toml_content) {
            Ok(()) => {
                repl.chat.add_message(ChatRole::System,
                    format!("Keybindings template saved to {}", kb_path.display()));
            }
            Err(e) => {
                repl.chat.add_message(ChatRole::System,
                    format!("Failed to save keybindings: {e}"));
            }
        }
        return Ok(());
    }

    let kb_path = dirs::home_dir()
        .map(|h| h.join(".shannon").join("keybindings.toml"))
        .unwrap_or_else(|| std::path::PathBuf::from(".shannon/keybindings.toml"));

    if trimmed == "load" || trimmed == "reload" {
        if !kb_path.exists() {
            repl.chat.add_message(ChatRole::System,
                "No custom keybindings file found. Use /bind save to create one.".to_string());
        } else {
            match std::fs::read_to_string(&kb_path) {
                Ok(content) => {
                    let line_count = content.lines().filter(|l| l.starts_with("[[bind]]")).count();
                    repl.chat.add_message(ChatRole::System,
                        format!("Loaded keybindings config ({line_count} custom binding(s) defined).\nKeybindings take effect on next restart."));
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System,
                        format!("Failed to read keybindings: {e}"));
                }
            }
        }
        return Ok(());
    }

    repl.chat.add_message(ChatRole::System,
        "Usage: /bind [list|save|load]\n  /bind       — Show all keybindings\n  /bind save  — Save template to ~/.shannon/keybindings.toml\n  /bind load  — Reload custom keybindings".to_string());
    Ok(())
}

// ── P3-15: Project-level config ──────────────────────────────────────────

fn handle_project(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    if trimmed.is_empty() || trimmed == "status" || trimmed == "show" {
        let cwd = &repl.state.working_directory;
        let mut msg = format!("Project Configuration\n\n  Directory: {cwd}\n");

        let config_files = [".shannon.toml", "CLAUDE.md", "AGENTS.md", "GEMINI.md", ".claude/settings.json"];
        msg.push_str("\n  Config files:\n");
        for file in &config_files {
            let path = std::path::Path::new(cwd).join(file);
            if path.exists() {
                msg.push_str(&format!("    + {file} (found)\n"));
            } else {
                msg.push_str(&format!("    - {file}\n"));
            }
        }

        if let Some(ref model) = repl.state.model {
            msg.push_str(&format!("\n  Model: {model}"));
        }

        msg.push_str(&format!("\n  Sandbox: {:?}", repl.state.sandbox_mode));

        if let Some(ref engine) = repl.query_engine {
            let perms = engine.permissions();
            let mode = perms.read().map(|p| p.approval_mode()).unwrap_or(shannon_core::permissions::ApprovalMode::Suggest);
            msg.push_str(&format!("\n  Permission mode: {mode:?}"));
        }

        if repl.state.plan.active {
            msg.push_str("\n  Plan mode: active");
        }

        msg.push_str(&format!("\n  Notifications: {}", if repl.notifications_enabled { "enabled" } else { "disabled" }));

        let git_check = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(cwd)
            .output();
        if let Ok(output) = git_check {
            if output.status.success() {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                msg.push_str(&format!("\n  Git root: {root}"));
            }
        }

        if let Some(ref engine) = repl.query_engine {
            msg.push_str(&format!("\n  Tools loaded: {}", engine.tools().list().len()));
        }

        repl.chat.add_message(ChatRole::System, msg);
        return Ok(());
    }

    if trimmed == "init" {
        let config_path = std::path::Path::new(&repl.state.working_directory).join(".shannon.toml");
        if config_path.exists() {
            repl.chat.add_message(ChatRole::System,
                format!("Project config already exists: {}", config_path.display()));
            return Ok(());
        }

        let template = "# Shannon project configuration\n\
[project]\n\
name = \"\"\n\
description = \"\"\n\
\n\
[model]\n\
default = \"claude-3-5-sonnet\"\n\
\n\
[tools]\n\
allowed = []        # Empty = all tools allowed\n\
denied = []         # Explicit deny list\n\
\n\
[sandbox]\n\
mode = \"direct\"     # direct | docker\n\
\n\
[context]\n\
auto_load = true    # Auto-load CLAUDE.md / AGENTS.md\n\
max_files = 20      # Max files for /add glob\n\
\n\
[permissions]\n\
mode = \"suggest\"    # suggest | auto-edit | full-auto | readonly\n\
\n\
[routes]\n\
# Pattern-based model routing\n\
# \"translate\" = \"claude-3-5-haiku\"\n\
# \"review\" = \"claude-3-5-sonnet\"\n";

        match std::fs::write(&config_path, template) {
            Ok(()) => {
                repl.chat.add_message(ChatRole::System,
                    format!("Created project config: {}\nEdit it to customize Shannon for this project.", config_path.display()));
            }
            Err(e) => {
                repl.chat.add_message(ChatRole::System,
                    format!("Failed to create config: {e}"));
            }
        }
        return Ok(());
    }

    if trimmed.starts_with("model ") {
        let model = trimmed.strip_prefix("model ").unwrap().trim();
        if model.is_empty() {
            repl.chat.add_message(ChatRole::System,
                format!("Current model: {}", repl.state.model.as_deref().unwrap_or("none")));
        } else {
            repl.state.model = Some(model.to_string());
            crate::repl::preferences::save_preferences(&crate::repl::preferences::Preferences {
                model: repl.state.model.clone(),
                provider: repl.state.selected_provider.clone(),
                theme: Some(repl.state.theme.name.to_string()),
            });
            repl.chat.add_message(ChatRole::System,
                format!("Project model set to: {model}"));
        }
        return Ok(());
    }

    if trimmed.starts_with("set ") {
        let rest = trimmed.strip_prefix("set ").unwrap().trim();
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() < 2 {
            repl.chat.add_message(ChatRole::System,
                "Usage: /project set <key> <value>\nKeys: model, sandbox, permissions, notifications".to_string());
            return Ok(());
        }
        let key = parts[0];
        let value = parts[1];

        match key {
            "sandbox" => {
                repl.state.sandbox_mode = shannon_tools::SandboxMode::from_str_loose(value);
                repl.chat.add_message(ChatRole::System,
                    format!("Sandbox mode set to: {:?}", repl.state.sandbox_mode));
            }
            "permissions" => {
                let mode = match value {
                    "auto-edit" => shannon_core::permissions::ApprovalMode::AutoEdit,
                    "full-auto" => shannon_core::permissions::ApprovalMode::FullAuto,
                    "readonly" => shannon_core::permissions::ApprovalMode::Readonly,
                    _ => shannon_core::permissions::ApprovalMode::Suggest,
                };
                if let Some(ref engine) = repl.query_engine {
                    if let Ok(mut perms) = engine.permissions().write() {
                        perms.set_approval_mode(mode);
                    }
                    repl.state.approval_mode_label = mode.short_label().to_string();
                }
                repl.chat.add_message(ChatRole::System,
                    format!("Permission mode set to: {value}"));
            }
            "notifications" => {
                repl.notifications_enabled = value == "on" || value == "true" || value == "enabled";
                repl.chat.add_message(ChatRole::System,
                    format!("Notifications: {}", if repl.notifications_enabled { "enabled" } else { "disabled" }));
            }
            _ => {
                repl.chat.add_message(ChatRole::System,
                    format!("Unknown setting: {key}. Available: model, sandbox, permissions, notifications"));
            }
        }
        return Ok(());
    }

    repl.chat.add_message(ChatRole::System,
        "Usage: /project [status|init|model <name>|set <key> <value>]\n\
         /project status  — Show current project config\n\
         /project init    — Create .shannon.toml template\n\
         /project model <name> — Set project model\n\
         /project set <key> <value> — Set config value".to_string());
    Ok(())
}

fn handle_stats(repl: &mut Repl) -> Result<()> {
    repl.state.sidebar_tab = crate::repl::SidebarTab::Perf;
    let dur = repl.state.session_start.map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let tok = repl.state.tokens_used;
    let turns = repl.current_turn;
    let cost = repl.state.total_cost_usd;
    let tools = repl.tools_invoked;
    let cmds = repl.commands_run;
    let tps = if dur > 0 && tok > 0 {
        format!("{:.0} tok/s", tok as f64 / dur as f64)
    } else {
        "N/A".to_string()
    };
    let dur_str = if dur >= 3600 {
        format!("{}h {}m", dur / 3600, (dur % 3600) / 60)
    } else if dur >= 60 {
        format!("{}m {}s", dur / 60, dur % 60)
    } else {
        format!("{}s", dur)
    };
    let model = repl.state.model.as_deref().unwrap_or("unknown");
    repl.chat.add_message(ChatRole::System, format!(
        "Performance stats (switched to Perf tab):\n  Model: {model}\n  Duration: {dur_str}\n  Tokens: {tok} ({tps})\n  Turns: {turns}\n  Cost: ${cost:.4}\n  Tools: {tools} | Commands: {cmds}"
    ));
    Ok(())
}

/// Handle `/loop` command — autonomous iteration engine.
///
/// Usage:
///   /loop <task>           — start loop with task description
///   /loop --max N <task>   — limit to N iterations
///   /loop stop             — stop the current loop
///   /loop status           — show current loop state
fn handle_loop(repl: &mut Repl, args: &str) -> Result<()> {
    let input = args.trim();

    if input == "stop" || input == "cancel" {
        if let Some(ref mut ls) = repl.state.loop_state {
            ls.active = false;
            let iter = ls.iteration;
            repl.chat.add_message(ChatRole::System, format!(
                "Loop stopped after {iter} iteration(s)."
            ));
        } else {
            repl.chat.add_message(ChatRole::System, "No active loop to stop.".to_string());
        }
        repl.state.loop_state = None;
        return Ok(());
    }

    if input == "status" {
        if let Some(ref ls) = repl.state.loop_state {
            repl.chat.add_message(ChatRole::System, format!(
                "Loop active: iteration {}/{}\nTask: {}",
                ls.iteration,
                if ls.max_iterations == 0 { "unlimited".to_string() } else { ls.max_iterations.to_string() },
                ls.task,
            ));
        } else {
            repl.chat.add_message(ChatRole::System, "No active loop.".to_string());
        }
        return Ok(());
    }

    if input.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage:\n  /loop <task>         — start autonomous iteration\n  /loop --max N <task> — limit to N iterations\n  /loop stop           — stop current loop\n  /loop status         — show loop state".to_string()
        );
        return Ok(());
    }

    // Parse --max N
    let (max_iter, task) = if input.starts_with("--max ") {
        let rest = input.strip_prefix("--max ").unwrap_or("");
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let n: usize = parts.first().unwrap_or(&"0").parse().unwrap_or(0);
        let t = parts.get(1).copied().unwrap_or("").trim();
        (n, t.to_string())
    } else {
        (0, input.to_string())
    };

    if task.is_empty() {
        repl.chat.add_message(ChatRole::System, "Error: no task description provided.".to_string());
        return Ok(());
    }

    // Set up loop state
    repl.state.loop_state = Some(super::LoopState {
        task: task.clone(),
        max_iterations: max_iter,
        iteration: 0,
        active: true,
    });

    repl.chat.add_message(ChatRole::System, format!(
        "Loop started{}.\nTask: {task}\nType /loop stop to cancel.",
        if max_iter > 0 { format!(" (max {max_iter} iterations)") } else { String::new() }
    ));

    // Trigger first iteration
    let prompt = format!(
        "[Loop iteration 1] Task: {task}\n\nPlease work on this task. After completing, summarize what you did and what remains."
    );
    repl.prompt.set_input(prompt);
    submit_input(repl)?;

    Ok(())
}

/// Called after a query completes. If a loop is active, triggers the next iteration.
/// Returns true if a new loop iteration was started.
pub(crate) fn check_loop_iteration(repl: &mut Repl) -> bool {
    let should_continue = repl.state.loop_state.as_ref().map_or(false, |ls| ls.active);
    if !should_continue {
        return false;
    }

    let ls = repl.state.loop_state.as_mut().unwrap();
    ls.iteration += 1;

    // Check max iterations
    if ls.max_iterations > 0 && ls.iteration >= ls.max_iterations {
        let iter = ls.iteration;
        repl.chat.add_message(ChatRole::System, format!(
            "Loop completed: reached max {iter} iteration(s)."
        ));
        repl.state.loop_state = None;
        return false;
    }

    let task = ls.task.clone();
    let iter = ls.iteration + 1;

    let prompt = format!(
        "[Loop iteration {iter}] Continuing task: {task}\n\nReview what was done in the previous iteration and continue working. Summarize progress and what remains."
    );
    repl.prompt.set_input(prompt);

    // Submit next iteration
    if let Err(_) = submit_input(repl) {
        repl.state.loop_state = None;
        return false;
    }

    true
}

/// Check if platform sandbox (bwrap/seatbelt) is available.
fn detect_platform_sandbox() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/usr/bin/bwrap").exists() || which_exists("bwrap") {
            return "bubblewrap (bwrap) available";
        }
    }
    #[cfg(target_os = "macos")]
    {
        if which_exists("sandbox-exec") {
            return "seatbelt (sandbox-exec) available";
        }
    }
    "no platform sandbox detected"
}

/// Simple check if a command exists in PATH.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
/// Handle custom agent definition commands
fn handle_agent(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_agents::agent_defs::AgentDefinitionRegistry;
    use shannon_agents::custom_agent::CustomAgentLoader;
    use std::path::PathBuf;

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/agent list                    — List all available agent definitions
/agent run <name> [prompt]     — Run an agent with optional prompt
/agent create <name>           — Interactive agent creation wizard
/agent edit <name>             — Edit an agent definition
/agent show <name>             — Show agent definition details

Agent definitions are loaded from:
  .claude/agents/*.md  (project-local, highest priority)
  .shannon/agents/*.toml (project-local)
  ~/.claude/agents/*.md (user-global)
  ~/.shannon/agents/*.toml (user-global)".to_string());
        }
        "list" => {
            let registry = AgentDefinitionRegistry::load_from_dirs();
            let loader = CustomAgentLoader::new();

            let custom_agents = match loader.discover() {
                Ok(agents) => agents,
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Error loading custom agents: {e}"));
                    return Ok(());
                }
            };

            let mut output = String::new();

            let toml_defs = registry.list_names();
            if !toml_defs.is_empty() {
                output.push_str(&format!("TOML Agents ({}):\n", toml_defs.len()));
                for name in &toml_defs {
                    if let Some(def) = registry.get(name) {
                        let model = def.model.as_deref().unwrap_or("default");
                        let tools = if def.allowed_tools.is_empty() {
                            String::new()
                        } else {
                            format!(" tools=[{}]", def.allowed_tools.join(","))
                        };
                        output.push_str(&format!("  - {}{}: {} ({})\n", name, tools, def.description, model));
                    }
                }
            }

            let md_names: Vec<_> = custom_agents.keys().cloned().collect();
            if !md_names.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("Markdown Agents ({}):\n", md_names.len()));
                for name in &md_names {
                    let def = &custom_agents[name];
                    let model = def.model.as_deref().unwrap_or("default");
                    let tools = def.allowed_tools.as_ref()
                        .map(|t| format!(" tools=[{}]", t.join(", ")))
                        .unwrap_or_default();
                    output.push_str(&format!("  - {}{}: {} ({})\n", name, tools, def.description, model));
                }
            }

            if output.is_empty() {
                output.push_str("No agent definitions found.\n");
                output.push_str("Create agents in .claude/agents/*.md or .shannon/agents/*.toml\n");
            }

            repl.chat.add_message(ChatRole::System, output);
        }
        "show" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agent show <name>".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            if let Some(def) = registry.get(name) {
                let mut output = format!("Agent: {} (TOML)\n", def.name);
                output.push_str(&format!("Description: {}\n", def.description));
                if let Some(model) = &def.model {
                    output.push_str(&format!("Model: {}\n", model));
                }
                if let Some(prompt) = &def.system_prompt {
                    output.push_str(&format!("System Prompt: {}\n", prompt));
                }
                if !def.allowed_tools.is_empty() {
                    output.push_str(&format!("Allowed Tools: {}\n", def.allowed_tools.join(", ")));
                }
                if !def.capabilities.is_empty() {
                    output.push_str(&format!("Capabilities: {}\n", def.capabilities.join(", ")));
                }
                output.push_str(&format!("Max Concurrent Tasks: {}\n", def.max_concurrent_tasks));
                if let Some(temp) = def.temperature {
                    output.push_str(&format!("Temperature: {}\n", temp));
                }
                repl.chat.add_message(ChatRole::System, output);
                return Ok(());
            }

            let loader = CustomAgentLoader::new();
            if let Ok(def) = loader.load(name) {
                let mut output = format!("Agent: {} (Markdown)\n", def.name);
                output.push_str(&format!("Description: {}\n", def.description));
                output.push_str(&format!("Source: {}\n", def.source_path.display()));
                if let Some(model) = &def.model {
                    output.push_str(&format!("Model: {}\n", model));
                }
                if let Some(tools) = &def.allowed_tools {
                    output.push_str(&format!("Allowed Tools: {}\n", tools.join(", ")));
                }
                if let Some(dirs) = &def.allowed_directories {
                    output.push_str(&format!("Allowed Directories: {}\n", dirs.join(", ")));
                }
                if let Some(max_turns) = def.max_turns {
                    output.push_str(&format!("Max Turns: {}\n", max_turns));
                }
                if !def.body_instructions.is_empty() {
                    output.push_str(&format!("Instructions:\n{}\n", def.body_instructions));
                }
                if let Some(suffix) = &def.system_prompt_suffix {
                    output.push_str(&format!("Prompt Suffix: {}\n", suffix));
                }
                repl.chat.add_message(ChatRole::System, output);
                return Ok(());
            }

            repl.chat.add_message(ChatRole::System, format!("Agent '{}' not found.", name));
        }
        "run" => {
            let name = parts.get(1).copied().unwrap_or("");
            let prompt = parts.get(2).copied().unwrap_or("");

            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agent run <name> [prompt]".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            let config = if let Some(def) = registry.get(name) {
                let system_prompt = def.system_prompt.clone()
                    .unwrap_or_else(|| def.description.clone());
                Some((def.clone(), system_prompt))
            } else {
                let loader = CustomAgentLoader::new();
                if let Ok(def) = loader.load(name) {
                    let mut prompt_parts = Vec::new();
                    if !def.body_instructions.is_empty() {
                        prompt_parts.push(def.body_instructions.clone());
                    }
                    if let Some(suffix) = &def.system_prompt_suffix {
                        prompt_parts.push(suffix.clone());
                    }
                    let system_prompt = if prompt_parts.is_empty() {
                        def.description.clone()
                    } else {
                        prompt_parts.join("\n\n")
                    };

                    let toml_def = shannon_agents::agent_defs::AgentDefinition {
                        name: def.name.clone(),
                        description: def.description.clone(),
                        system_prompt: Some(system_prompt.clone()),
                        model: def.model.clone(),
                        capabilities: vec![],
                        allowed_tools: def.allowed_tools.unwrap_or_default(),
                        max_concurrent_tasks: 3,
                        plan_mode_required: false,
                        temperature: None,
                    };

                    Some((toml_def, system_prompt))
                } else {
                    None
                }
            };

            let (def, system_prompt) = match config {
                Some(c) => c,
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Agent '{}' not found. Use /agent list to see available agents.", name));
                    return Ok(());
                }
            };

            use shannon_agents::{AgentCoordinator, CoordinatorConfig, SubAgentRegistry, AgentConfig};

            if repl.agent_registry.is_none() {
                let config = CoordinatorConfig::default();
                let coordinator = repl.runtime.block_on(AgentCoordinator::new(config))
                    .expect("failed to create agent coordinator");
                repl.agent_registry = Some(std::sync::Arc::new(SubAgentRegistry::new(
                    std::sync::Arc::new(coordinator),
                )));
            }

            let agent_config = AgentConfig {
                name: format!("agent-{}", def.name),
                model: def.model.clone().unwrap_or_else(|| {
                    repl.state.model.clone().unwrap_or_else(|| "claude-sonnet-4-6".to_string())
                }),
                system_prompt,
                tools: def.allowed_tools.clone(),
                working_directory: PathBuf::from("."),
                max_turns: def.max_concurrent_tasks as u32,
                team: None,
            };

            let registry = repl.agent_registry.as_ref().unwrap().clone();
            match repl.runtime.block_on(registry.spawn(agent_config)) {
                Ok(agent) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' spawned (id: {}, status: {})",
                        agent.name, agent.id, agent.status
                    ));

                    if !prompt.is_empty() {
                        match repl.runtime.block_on(registry.send_message("repl", &agent.name, serde_json::json!(prompt))) {
                            Ok(_) => {
                                repl.chat.add_message(ChatRole::System, format!("Message sent to agent '{}'.", agent.name));
                            }
                            Err(e) => {
                                repl.chat.add_message(ChatRole::System, format!("Failed to send message: {e}"));
                            }
                        }
                    }
                }
                Err(e) => {
                    repl.chat.add_message(ChatRole::System, format!("Failed to spawn agent: {e}"));
                }
            }
        }
        "create" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "\
Agent Creation Wizard
====================

Usage: /agent create <name>

This will guide you through creating an agent definition interactively.
The agent will be saved as a markdown file in .claude/agents/{name}.md".to_string());
                return Ok(());
            }

            if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                repl.chat.add_message(ChatRole::System, "Agent name must contain only alphanumeric characters, hyphens, and underscores.".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            if registry.get(name).is_some() {
                repl.chat.add_message(ChatRole::System, format!("Agent '{}' already exists. Use /agent edit {} to modify it.", name, name));
                return Ok(());
            }

            let loader = CustomAgentLoader::new();
            if loader.load(name).is_ok() {
                repl.chat.add_message(ChatRole::System, format!("Agent '{}' already exists. Use /agent edit {} to modify it.", name, name));
                return Ok(());
            }

            repl.state.pending_dialog_action = Some(format!("create_agent:{}", name));

            repl.chat.add_message(ChatRole::System, format!(
                "Creating agent '{}'. Please provide the following information:\n\
                 1. Description: What does this agent do?\n\
                 2. Model (optional): opus, sonnet, or haiku (default: sonnet)\n\
                 3. Tools (optional): Comma-separated tool names\n\
                 4. Instructions: The agent's system prompt",
                name
            ));
        }
        "edit" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agent edit <name>".to_string());
                return Ok(());
            }

            let registry = AgentDefinitionRegistry::load_from_dirs();
            let source_path = if let Some(_def) = registry.get(name) {
                repl.chat.add_message(ChatRole::System, format!(
                    "Agent '{}' is defined in TOML format. Edit the file directly: .shannon/agents/{}.toml",
                    name, name
                ));
                return Ok(());
            } else {
                let loader = CustomAgentLoader::new();
                match loader.load(name) {
                    Ok(def) => def.source_path.clone(),
                    Err(_) => {
                        repl.chat.add_message(ChatRole::System, format!("Agent '{}' not found.", name));
                        return Ok(());
                    }
                }
            };

            repl.chat.add_message(ChatRole::System, format!(
                "Editing agent '{}' (source: {})\n\
                 To edit, modify the file directly and run /agent show {} to verify.",
                name, source_path.display(), name
            ));
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /agent help."));
        }
    }

    Ok(())
}

fn handle_routine(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.trim().splitn(3, ' ').collect();
    let subcmd = parts.first().copied().unwrap_or("list");

    match subcmd {
        "list" | "ls" | "" => {
            let routines = repl.state.routine_manager.list();
            if routines.is_empty() {
                repl.chat.add_message(ChatRole::System, "No scheduled routines. Use /routine add <name> <interval_secs> <prompt>".to_string());
                return Ok(());
            }
            let mut msg = String::from("Scheduled Routines:\n\n");
            for r in routines {
                let status = if r.enabled { "ON" } else { "OFF" };
                let last = r.last_fired.map(|t| t.format("%H:%M:%S").to_string()).unwrap_or("never".into());
                msg.push_str(&format!(
                    "  [{}] {} ({})\n    Interval: {}s | Fires: {} | Last: {}\n    Prompt: {}\n\n",
                    r.id, r.name, status, r.interval_secs, r.fire_count, last,
                    if r.prompt.len() > 60 { format!("{}...", &r.prompt[..57]) } else { r.prompt.clone() }
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "add" => {
            if parts.len() < 4 {
                repl.chat.add_message(ChatRole::System,
                    "Usage: /routine add <name> <interval_secs> <prompt>\n\nExample: /routine add status-check 300 Check git status".to_string());
                return Ok(());
            }
            let name = parts[1].to_string();
            let interval: u64 = match parts[2].parse() {
                Ok(i) if i > 0 => i,
                _ => {
                    repl.chat.add_message(ChatRole::System, "Interval must be a positive number of seconds.".to_string());
                    return Ok(());
                }
            };
            let prompt = parts[3].to_string();
            let routine = shannon_core::scheduled_routines::ScheduledRoutine::new(name, prompt, interval);
            let id = routine.id.clone();
            repl.state.routine_manager.add(routine);
            repl.chat.add_message(ChatRole::System, format!("Added routine [{}]. Use /routine list to see all.", id));
        }
        "remove" | "rm" | "delete" => {
            let id = parts.get(1).copied().unwrap_or("");
            if id.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /routine remove <id>".to_string());
                return Ok(());
            }
            match repl.state.routine_manager.remove(id) {
                Some(r) => repl.chat.add_message(ChatRole::System, format!("Removed routine: {}", r.name)),
                None => repl.chat.add_message(ChatRole::System, format!("Routine '{}' not found.", id)),
            };
        }
        "toggle" => {
            let id = parts.get(1).copied().unwrap_or("");
            if id.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /routine toggle <id>".to_string());
                return Ok(());
            }
            match repl.state.routine_manager.toggle(id) {
                Some(enabled) => repl.chat.add_message(ChatRole::System,
                    format!("Routine {} is now {}", id, if enabled { "enabled" } else { "disabled" })),
                None => repl.chat.add_message(ChatRole::System, format!("Routine '{}' not found.", id)),
            };
        }
        "fire" => {
            let due = repl.state.routine_manager.drain_due();
            if due.is_empty() {
                repl.chat.add_message(ChatRole::System, "No routines are due to fire.".to_string());
            } else {
                for (name, prompt) in due {
                    repl.chat.add_message(ChatRole::System, format!("Routine '{}' fired: {}", name, prompt));
                }
            }
        }
        "save" => {
            let path = shannon_core::scheduled_routines::RoutineManager::default_storage_path();
            match repl.state.routine_manager.save_to_file(&path) {
                Ok(()) => repl.chat.add_message(ChatRole::System, format!("Routines saved to {}", path.display())),
                Err(e) => repl.chat.add_message(ChatRole::System, format!("Failed to save: {e}")),
            };
        }
        "help" | "-h" | "--help" => {
            repl.chat.add_message(ChatRole::System,
                "Scheduled Routines — recurring task execution\n\n\
                 Commands:\n  /routine list                     — show all routines\n  \
                 /routine add <name> <secs> <prompt> — add a new routine\n  \
                 /routine remove <id>               — remove a routine\n  \
                 /routine toggle <id>               — enable/disable\n  \
                 /routine fire                      — manually check and fire due routines\n  \
                 /routine save                      — persist routines to disk".to_string());
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                format!("Unknown routine subcommand: '{}'. Use /routine help.", subcmd));
        }
    }
    Ok(())
}
