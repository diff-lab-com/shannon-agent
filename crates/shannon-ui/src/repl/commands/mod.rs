//! REPL command dispatch and handler implementations

mod config;
mod cost;
mod debug;
mod extensions;
mod file_ops;
mod git;
mod hooks;
mod loop_engine;
mod media;
mod memory;
mod session;
mod web;

// Re-export public API
pub use media::handle_image_paste_from_input;
pub(crate) use loop_engine::{notify_query_complete, check_loop_iteration, check_ralph_iteration};
#[allow(unused_imports)]
pub(crate) use cost::extract_plan_steps;
#[allow(unused_imports)]
pub(crate) use git::format_change_bar;

use crate::{
    widgets::ChatRole,
    Result,
};
use rust_i18n::t;
use shannon_types::recover_lock;

use super::Repl;

/// Display an error message in the chat as a system message.
/// All user-facing error messages from slash commands should use this helper
/// for a consistent "Error: <msg>" format.
pub(crate) fn set_error(repl: &mut Repl, msg: &str) {
    repl.chat.add_message(ChatRole::System, format!("Error: {msg}"));
}

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
    let repl_only_commands = ["browse", "files", "select-tools", "tools", "team", "agents", "agent", "route", "mcp", "compact", "cost", "permissions", "perms", "perm", "plan", "web-search", "websearch", "search-web", "review", "local-models", "local", "ci", "gh-actions", "hooks", "remember", "mem", "memo", "recall", "search-memory", "forget", "memory", "image", "img", "screenshot", "mode", "context", "undo", "rewind", "notify", "webhook", "routine", "create-pr", "patch", "sandbox", "find", "grep", "conv-search", "copy", "paste", "add", "add-dir", "adddir", "rename", "watch", "bind", "project", "terminal-setup", "theme", "diff", "commands", "recap", "effort", "focus"];
    let is_repl_command = repl_only_commands.contains(&cmd_name);

    if command_exists || is_repl_command {
        match cmd_name {
            "help" => handle_help(repl, args)?,
            "clear" => handle_clear(repl)?,
            "quit" | "exit" => handle_quit(repl)?,
            "model" | "models" => config::handle_model(repl, args)?,
            "init" => config::handle_init(repl)?,
            "config" => config::handle_config(repl, args)?,
            "sessions" => session::handle_sessions(repl, args)?,
            "resume" => session::handle_resume(repl, args)?,
            "history" => session::handle_history(repl, args)?,
            "worktree" => git::handle_worktree(repl, args)?,
            "credentials" | "creds" | "cred" => extensions::handle_credentials(repl, args)?,
            "status" | "st" | "git-status" => git::handle_status(repl, args)?,
            "export" | "save" => file_ops::handle_export(repl, args)?,
            "import" | "load" => file_ops::handle_import(repl, args)?,
            "diff" => git::handle_diff(repl, args)?,
            "search" | "?" | "hist" | "history-search" => file_ops::handle_search(repl, args)?,
            "find" | "grep" | "conv-search" => file_ops::handle_find(repl, args)?,
            "browse" | "files" => media::handle_browse(repl, args)?,
            "select-tools" | "tools" => debug::handle_select_tools(repl)?,
            "debug" | "dbg" | "dev" => debug::handle_debug(repl, args)?,
            "doctor" | "check" | "diagnostics" => debug::handle_doctor(repl, args)?,
            "terminal-setup" => config::handle_terminal_setup(repl)?,
            "compact" => session::handle_compact(repl, args)?,
            "cost" => cost::handle_cost(repl, args)?,
            "billing" | "usage" => cost::handle_billing(repl, args)?,
            "suggest" => cost::handle_suggest(repl, args)?,
            "permissions" | "perms" | "perm" => cost::handle_permissions(repl, args)?,
            "plan" => session::handle_plan(repl, args)?,
            "team" => extensions::handle_team(repl, args)?,
            "agents" => extensions::handle_agents(repl, args)?,
            "agent" => loop_engine::handle_agent(repl, args)?,
            "route" => extensions::handle_route(repl, args)?,
            "mcp" => extensions::handle_mcp(repl, args)?,
            "branch" | "fork" => session::handle_branch(repl, args)?,
            "web-search" | "websearch" | "search-web" => web::handle_web_search(repl, args)?,
            "review" => git::handle_review(repl, args)?,
            "stage" => git::handle_stage(repl, args)?,
            "stats" | "perf" => loop_engine::handle_stats(repl)?,
            "loop" => loop_engine::handle_loop(repl, args)?,
            "ralph" => loop_engine::handle_ralph(repl, args)?,
            "sandbox" => loop_engine::handle_sandbox(repl, args)?,
            "local-models" | "local" => config::handle_local_models(repl)?,
            "ci" | "gh-actions" => git::handle_ci(repl, args)?,
            "hooks" => hooks::handle_hooks(repl, args)?,
            "remember" | "mem" | "memo" => memory::handle_remember(repl, args)?,
            "recall" | "search-memory" => memory::handle_recall(repl, args)?,
            "forget" => memory::handle_forget(repl, args)?,
            "memory" => memory::handle_memory(repl, args)?,
            "image" | "img" | "screenshot" => media::handle_image(repl, args)?,
            "mode" => config::handle_mode(repl, args)?,
            "context" => config::handle_context(repl, args)?,
            "undo" => session::handle_undo(repl, args)?,
            "rewind" => session::handle_rewind(repl, args)?,
            "notify" => web::handle_notify(repl, args)?,
            "webhook" => web::handle_webhook(repl, args)?,
            "routine" => loop_engine::handle_routine(repl, args)?,
            "create-pr" => git::handle_create_pr(repl, args)?,
            "patch" => git::handle_patch(repl, args)?,
            "copy" | "clip" => media::handle_copy(repl, args)?,
            "paste" => media::handle_paste(repl)?,
            "add" => file_ops::handle_add(repl, args)?,
            "add-dir" | "adddir" => file_ops::handle_add_dir(repl, args)?,
            "watch" => file_ops::handle_watch(repl, args)?,
            "bind" => loop_engine::handle_bind(repl, args)?,
            "project" => loop_engine::handle_project(repl, args)?,
            "theme" => config::handle_theme(repl, args)?,
            "session" => session::handle_session(repl, args)?,
            "rename" => session::handle_rename(repl, args)?,
            "recap" => session::handle_recap(repl, args)?,
            "effort" => session::handle_effort(repl, args)?,
            "focus" => session::handle_focus(repl, args)?,
            "accessibility" | "a11y" => config::handle_accessibility(repl, args)?,
            "diag" => debug::handle_diag(repl, args)?,
            "commands" => hooks::handle_commands(repl, args)?,
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
                let mut perms = recover_lock(query_engine.permissions().write());
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
