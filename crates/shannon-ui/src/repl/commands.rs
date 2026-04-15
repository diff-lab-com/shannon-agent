//! REPL command dispatch and handler implementations

use crate::{
    widgets::ChatRole,
    Result,
};

use super::Repl;

/// Submit the current input
pub fn submit_input(repl: &mut Repl) -> Result<()> {
    let input = repl.prompt.input().to_string();

    if input.trim().is_empty() {
        return Ok(());
    }

    // Add user message to chat
    repl.chat.add_message(ChatRole::User, input.clone());

    // Push to command history and clear input
    repl.command_history.push(&input);
    repl.saved_input.clear();
    repl.prompt.clear();

    // Process command or query
    if input.starts_with('/') {
        repl.commands_run += 1;
        handle_command(repl, &input)?;
    } else {
        super::query::handle_query(repl, &input)?;
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

    // Check if command exists in the registry or as a plugin command
    let command_exists = repl.runtime.block_on(async {
        repl.shared_executor.registry().await.contains(cmd_name).await
    });
    let is_plugin_command = repl.plugin_manager.get_plugin_commands()
        .iter().any(|c| c.name == cmd_name);
    // Commands handled in the match block but not in the global registry
    let repl_only_commands = ["browse", "files", "select-tools", "tools", "team", "compact", "cost", "permissions", "perms", "perm", "plan", "web-search", "websearch", "search-web", "review", "local-models", "local", "ci", "gh-actions"];
    let is_repl_command = repl_only_commands.contains(&cmd_name);

    if command_exists || is_plugin_command || is_repl_command {
        match cmd_name {
            "help" => handle_help(repl, args)?,
            "clear" => handle_clear(repl)?,
            "quit" | "exit" => handle_quit(repl)?,
            "model" => handle_model(repl, args)?,
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
            "browse" | "files" => handle_browse(repl, args)?,
            "select-tools" | "tools" => handle_select_tools(repl)?,
            "debug" | "dbg" | "dev" => handle_debug(repl, args)?,
            "doctor" | "check" | "diagnostics" => handle_doctor(repl, args)?,
            "compact" => handle_compact(repl, args)?,
            "cost" => handle_cost(repl, args)?,
            "permissions" | "perms" | "perm" => handle_permissions(repl, args)?,
            "plan" => handle_plan(repl, args)?,
            "team" => handle_team(repl, args)?,
            "branch" | "fork" => handle_branch(repl, args)?,
            "web-search" | "websearch" | "search-web" => handle_web_search(repl, args)?,
            "review" => handle_review(repl, args)?,
            "local-models" | "local" => handle_local_models(repl)?,
            "ci" | "gh-actions" => handle_ci(repl, args)?,
            _ => handle_other_command(repl, cmd_name, args)?,
        }
        Ok(())
    } else {
        repl.chat.add_message(
            ChatRole::System,
            format!("Unknown command: /{cmd_name}. Type /help for available commands."),
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
        repl.chat.add_message(ChatRole::System, "Chat cleared.".to_string());
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
        repl.show_input_dialog(
            "Set Model",
            "Enter model name (e.g. claude-3.5-sonnet, gpt-4o)...",
            "set_model",
        );
    } else {
        repl.state.model = Some(args.to_string());
        repl.chat.add_message(
            ChatRole::System,
            format!("Model set to: {args}"),
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
    repl.chat.add_message(ChatRole::System, format!("Project initialized.\n{init_info}"));
    Ok(())
}

fn handle_config(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_tools::config::ConfigManager;
    use shannon_commands::config_utils;

    let mut manager = ConfigManager::new();
    if let Err(e) = manager.load() {
        repl.chat.add_message(ChatRole::System, format!("Warning: could not load config: {e}"));
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
/team shutdown  — Shutdown team".to_string());
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
                            repl.team_coordinator = Some(coordinator);
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
        "run" => {
            use shannon_agents::{MultiAgentSpawner, SpawnAgentConfig};
            if let Some(ref coordinator) = repl.team_coordinator {
                let task_board = coordinator.task_board();
                let ready_tasks = repl.runtime.block_on(task_board.list_ready_tasks());
                if ready_tasks.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No pending tasks to execute.".to_string());
                    return Ok(());
                }
                let agent_configs: Vec<SpawnAgentConfig> = ready_tasks
                    .iter().map(|t| SpawnAgentConfig::new(format!("agent-{}", t.id), t.subject.clone())).collect();
                let config = shannon_agents::MultiAgentConfig::new(agent_configs);
                repl.chat.add_message(ChatRole::System, "Starting parallel execution...".to_string());
                let result = repl.runtime.block_on(MultiAgentSpawner::spawn(config));
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
                        if let Some(name) = rest.splitn(2, ' ').next() {
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
        _ => compact_engine.compact(&mut messages),
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

fn handle_plan(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();

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
                format!("Plan '{}' completed and cleared.", desc));
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
            };
            repl.state.status = "Plan mode — review plan".to_string();
            let msg = format!(
                "Plan created: {}\n\n{}\n\nUse /plan approve to approve, /plan reject to discard, or /plan help for more options.",
                description, plan_content
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

    if lower.contains("add") || lower.contains("implement") || lower.contains("feature") {
        if steps.is_empty() {
            steps.push("Analyze requirements and design interface".to_string());
            steps.push("Implement core functionality".to_string());
            steps.push("Add error handling and input validation".to_string());
            steps.push("Write tests for new functionality".to_string());
            steps.push("Update documentation".to_string());
        }
    }

    if lower.contains("fix") || lower.contains("bug") {
        if steps.is_empty() {
            steps.push("Reproduce the issue and understand root cause".to_string());
            steps.push("Implement fix with minimal changes".to_string());
            steps.push("Add regression test".to_string());
            steps.push("Verify fix resolves the issue".to_string());
        }
    }

    if lower.contains("migrate") || lower.contains("upgrade") {
        if steps.is_empty() {
            steps.push("Review migration/upgrade guide and breaking changes".to_string());
            steps.push("Update dependencies".to_string());
            steps.push("Adapt code to new API surface".to_string());
            steps.push("Run tests and fix any failures".to_string());
            steps.push("Verify functionality end-to-end".to_string());
        }
    }

    // Default fallback
    if steps.is_empty() {
        steps.push(format!("Understand requirements: {}", description));
        steps.push("Design solution approach".to_string());
        steps.push("Implement the solution".to_string());
        steps.push("Test and verify the implementation".to_string());
    }

    steps
}

fn handle_permissions(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::permissions::RiskLevel;

    let parts: Vec<&str> = args.trim().split_whitespace().collect();

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
                            "    {}: {} risk, {} deny patterns, {} confirm patterns\n",
                            name, risk, deny_count, confirm_count
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
            repl.chat.add_message(ChatRole::System, format!("Tool '{}' is now always allowed.", tool));
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
            repl.chat.add_message(ChatRole::System, format!("Tool '{}' is now always denied.", tool));
        }
        "reset" => {
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.reset_memory();
                }
            }
            repl.chat.add_message(ChatRole::System, "Permission memory cleared. All tool overrides removed.".to_string());
        }
        "help" | _ => {
            repl.chat.add_message(ChatRole::System,
                "Permission Commands:\n\
                 /permissions status — Show current permission policies and overrides\n\
                 /permissions allow <tool> — Always allow a tool without prompting\n\
                 /permissions deny <tool> — Always deny a tool\n\
                 /permissions reset — Clear all permission overrides\n\
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
                    let mut findings = Vec::new();

                    // Check for potential secrets
                    if diff_text.contains("API_KEY") || diff_text.contains("api_key") || diff_text.contains("password") {
                        findings.push("[WARN] Potential secrets detected — review for accidental credential exposure");
                    }

                    // Check for large diffs
                    if additions + deletions > 500 {
                        findings.push("[WARN] Large diff — consider splitting into smaller changes");
                    }

                    // Check for TODO/FIXME
                    if diff_text.lines().filter(|l| l.starts_with('+')).any(|l| l.contains("TODO") || l.contains("FIXME")) {
                        findings.push("[INFO] New TODO/FIXME comments added");
                    }

                    // Check for test changes
                    let has_test_changes = diff_text.lines()
                        .filter(|l| l.starts_with("diff --git"))
                        .any(|l| l.contains("test") || l.contains("spec"));
                    if has_test_changes {
                        findings.push("[PASS] Test changes detected");
                    } else if additions + deletions > 50 {
                        findings.push("[WARN] No test changes — consider adding tests for new code");
                    }

                    if findings.is_empty() {
                        review.push_str("Automated checks: No issues detected.\n");
                    } else {
                        review.push_str("Automated findings:\n");
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
    // Check plugin commands first
    let plugin_cmd = repl.plugin_manager.get_plugin_commands()
        .iter().find(|c| c.name == cmd_name).cloned();

    if let Some(plugin) = plugin_cmd {
        let prompt = plugin.prompt_template.replace("{args}", if args.is_empty() { "" } else { args });
        repl.chat.add_message(ChatRole::System, format!("Running /{cmd_name} (plugin)..."));
        super::query::handle_query(repl, &prompt)?;
        return Ok(());
    }

    let registry = repl.runtime.block_on(repl.shared_executor.registry());
    if let Ok(command) = repl.runtime.block_on(registry.get(cmd_name)) {
        match &*command {
            shannon_commands::Command::Prompt(prompt_cmd) => {
                if let Some(ref template) = prompt_cmd.prompt_template {
                    let prompt = template.replace("{args}", if args.is_empty() { "" } else { args });
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
            repl.chat.add_message(ChatRole::System, "Chat cleared.".to_string());
        }
        "quit" => {
            repl.running = false;
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
