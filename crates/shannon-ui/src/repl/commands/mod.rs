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

// Re-export the single switch-path helper so the REPL init/resume paths
// (repl/mod.rs) can refresh the first-screen StatusCard through the same
// derivation used by every /connect, /model, /provider switch
// (ADR-0008 Decision 1+2).
pub(crate) use config::{apply_model_selection, sync_active_to_chat};

// Re-export public API
#[allow(unused_imports)]
pub(crate) use cost::extract_plan_steps;
#[allow(unused_imports)]
pub(crate) use git::format_change_bar;
pub(crate) use loop_engine::{check_loop_iteration, check_ralph_iteration, notify_query_complete};
pub use media::handle_image_paste_from_input;
pub(crate) use media::{copy_nth_response, copy_to_clipboard};
pub(crate) use session::apply_file_rewind;

use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_types::recover_lock;

use super::Repl;

/// Display an error message in the chat as a system message.
/// All user-facing error messages from slash commands should use this helper
/// for a consistent "Error: <msg>" format.
pub(crate) fn set_error(repl: &mut Repl, msg: &str) {
    repl.chat
        .add_message(ChatRole::System, format!("Error: {msg}"));
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

/// Redact inline secrets from a recorded command line so they are never
/// persisted into the chat widget, command history, or session JSON.
///
/// Currently redacts the API key from `/connect <provider> <key>`, replacing
/// the key with `***`. The real key still reaches the command handler — this
/// redaction only affects what is *recorded* (chat message + up-arrow history).
/// Returns the input unchanged for any other command, free-text input, or a
/// `/connect` invocation without an inline key.
///
/// Tokenization uses `split_whitespace`, matching how `parse_connect_args`
/// splits the real command, so runs of whitespace (`/connect  minimax  k`) are
/// handled the same way as the single-space form.
fn redact_secret_command(input: &str) -> String {
    // Preserve any leading whitespace the user typed before the '/'.
    let trimmed = input.trim_start();
    let lead = &input[..input.len() - trimmed.len()];
    let rest = match trimmed.strip_prefix('/') {
        Some(r) => r,
        None => return input.to_string(),
    };
    let mut tokens = rest.split_whitespace();
    let cmd = tokens.next().unwrap_or("");
    if !cmd.eq_ignore_ascii_case("connect") {
        return input.to_string();
    }
    let provider = match tokens.next() {
        Some(p) if !p.is_empty() => p,
        _ => return input.to_string(),
    };
    match tokens.next() {
        Some(k) if !k.is_empty() => format!("{lead}/connect {provider} ***"),
        _ => input.to_string(),
    }
}

/// Submit the current input
pub fn submit_input(repl: &mut Repl, mut terminal: Option<&mut super::query::Term>) -> Result<()> {
    let raw_input = repl.prompt.input().to_string();

    if raw_input.trim().is_empty() {
        return Ok(());
    }

    // Detect URLs in input for potential @-reference expansion
    if let Some(_url) = crate::repl::at_reference::detect_url_in_input(&raw_input) {
        tracing::debug!(url = %_url, "URL detected in input");
    }

    // Expand pasted text references: [Pasted Text #N X lines] -> actual content
    let expanded = expand_pasted_texts(&raw_input, &mut repl.state.pasted_texts);

    // Add user message to chat. Redact inline secrets (e.g. an API key passed
    // to /connect) so the plaintext never lands in the chat widget or the
    // session JSON written by save_session. The unredacted `expanded` below is
    // what the command handler actually receives.
    let chat_text = redact_secret_command(&raw_input);
    repl.chat.add_message(ChatRole::User, chat_text);

    // Increment turn counter for context visualization
    repl.state.turn_count += 1;

    // Push to command history (up-arrow recall). Redact the same way so a
    // recalled command can't leak the key either.
    let history_entry = redact_secret_command(&expanded);
    repl.command_history.push(&history_entry);
    repl.saved_input.clear();
    repl.prompt.clear();

    // Clear paste state for next input
    repl.state.pasted_texts.clear();
    repl.state.paste_counter = 0;

    // Process command or query with expanded text
    if expanded.starts_with('!') {
        // Inline shell execution: "!command" or "! command"
        let shell_cmd = expanded.trim_start_matches('!').trim();
        if !shell_cmd.is_empty() {
            let start = chrono::Utc::now();
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(shell_cmd)
                .current_dir(&repl.state.working_directory)
                .output();
            let msg = match &output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let mut parts = String::new();
                    if !stdout.is_empty() {
                        parts.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !parts.is_empty() {
                            parts.push('\n');
                        }
                        parts.push_str("[stderr]\n");
                        parts.push_str(&stderr);
                    }
                    if parts.is_empty() {
                        format!("$ {shell_cmd}\n(exit {})", out.status.code().unwrap_or(-1))
                    } else {
                        format!("$ {shell_cmd}\n{parts}")
                    }
                }
                Err(e) => format!("$ {shell_cmd}\nFailed to execute: {e}"),
            };
            let is_error = output.as_ref().map(|o| !o.status.success()).unwrap_or(true);
            repl.chat
                .add_tool_message(shell_cmd.to_string(), msg, is_error, Some(start));
        }
    } else if expanded.starts_with('/') {
        repl.commands_run += 1;
        handle_command(repl, &expanded)?;
    } else {
        super::query::handle_query(repl, &expanded, &mut terminal)?;
    }

    // Drain queued follow-up messages in a flat loop.
    // This avoids recursive handle_query calls that could leave the
    // query engine unavailable.
    while !repl.state.queued_messages.is_empty() {
        let queued = repl.state.queued_messages.remove(0);
        if queued.trim().is_empty() {
            continue;
        }
        repl.state.toast = Some((
            "Sending queued message…".to_string(),
            std::time::Instant::now(),
        ));
        submit_input_with_text(repl, &queued, &mut terminal);
    }

    Ok(())
}

/// Submit pre-formed text as if the user typed and entered it.
/// Used for queued follow-up messages.
pub fn submit_input_with_text(
    repl: &mut Repl,
    text: &str,
    terminal: &mut Option<&mut super::query::Term>,
) {
    let expanded = expand_pasted_texts(text, &mut repl.state.pasted_texts);
    // Redact inline secrets (e.g. /connect <provider> <key>) before recording
    // — same contract as submit_input. The unredacted `expanded` is executed.
    let chat_text = redact_secret_command(text);
    repl.chat.add_message(ChatRole::User, chat_text);
    repl.state.turn_count += 1;
    let history_entry = redact_secret_command(&expanded);
    repl.command_history.push(&history_entry);
    repl.prompt.clear();
    repl.state.pasted_texts.clear();
    repl.state.paste_counter = 0;

    if expanded.starts_with('!') {
        let shell_cmd = expanded.trim_start_matches('!').trim();
        if !shell_cmd.is_empty() {
            let start = chrono::Utc::now();
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(shell_cmd)
                .current_dir(&repl.state.working_directory)
                .output();
            let msg = match &output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let mut parts = String::new();
                    if !stdout.is_empty() {
                        parts.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !parts.is_empty() {
                            parts.push('\n');
                        }
                        parts.push_str("[stderr]\n");
                        parts.push_str(&stderr);
                    }
                    if parts.is_empty() {
                        format!("$ {shell_cmd}\n(exit {})", out.status.code().unwrap_or(-1))
                    } else {
                        format!("$ {shell_cmd}\n{parts}")
                    }
                }
                Err(e) => format!("$ {shell_cmd}\nFailed to execute: {e}"),
            };
            let is_error = output.as_ref().map(|o| !o.status.success()).unwrap_or(true);
            repl.chat
                .add_tool_message(shell_cmd.to_string(), msg, is_error, Some(start));
        }
    } else if expanded.starts_with('/') {
        repl.commands_run += 1;
        if let Err(e) = handle_command(repl, &expanded) {
            repl.chat
                .add_message(ChatRole::System, format!("Error: {e}"));
        }
    } else if let Err(e) = super::query::handle_query(repl, &expanded, terminal) {
        repl.chat
            .add_message(ChatRole::System, format!("Error: {e}"));
    }
}

/// Handle a command (starts with /)
pub fn handle_command(repl: &mut Repl, input: &str) -> Result<()> {
    let parsed = match repl.command_parser.parse(input) {
        Ok(p) => p,
        Err(_) => {
            let parts: Vec<&str> = input.splitn(2, ' ').collect();
            let name = parts
                .first()
                .copied()
                .unwrap_or("")
                .strip_prefix('/')
                .unwrap_or("");
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
        repl.shared_executor
            .registry()
            .await
            .contains(cmd_name)
            .await
    });
    // Commands handled in the match block but not in the global registry
    let repl_only_commands = [
        "help",
        "clear",
        "quit",
        "exit",
        "model",
        "models",
        "provider",
        "prov",
        "init",
        "config",
        "connect",
        "disconnect",
        "sessions",
        "resume",
        "history",
        "worktree",
        "credentials",
        "creds",
        "cred",
        "status",
        "st",
        "git-status",
        "export",
        "save",
        "import",
        "load",
        "diff",
        "search",
        "?",
        "hist",
        "history-search",
        "find",
        "grep",
        "conv-search",
        "browse",
        "files",
        "select-tools",
        "tools",
        "notools",
        "debug",
        "dbg",
        "dev",
        "doctor",
        "check",
        "diagnostics",
        "terminal-setup",
        "compact",
        "cost",
        "billing",
        "usage",
        "suggest",
        "permissions",
        "perms",
        "perm",
        "plan",
        "team",
        "agents",
        "agent",
        "route",
        "mcp",
        "branch",
        "fork",
        "web-search",
        "websearch",
        "search-web",
        "review",
        "stage",
        "stats",
        "perf",
        "loop",
        "ralph",
        "sandbox",
        "local-models",
        "local",
        "ci",
        "gh-actions",
        "hooks",
        "remember",
        "mem",
        "memo",
        "recall",
        "search-memory",
        "forget",
        "memory",
        "image",
        "img",
        "screenshot",
        "mode",
        "context",
        "undo",
        "rewind",
        "checkpoint",
        "notify",
        "webhook",
        "routine",
        "schedule",
        "cron",
        "create-pr",
        "patch",
        "copy",
        "clip",
        "paste",
        "add",
        "add-dir",
        "adddir",
        "watch",
        "bind",
        "project",
        "theme",
        "session",
        "rename",
        "recap",
        "effort",
        "focus",
        "accessibility",
        "a11y",
        "color",
        "diag",
        "commands",
        "statusline",
        "lang",
        "language",
    ];
    let is_repl_command = repl_only_commands.contains(&cmd_name);

    if command_exists || is_repl_command {
        match cmd_name {
            "help" => handle_help(repl, args)?,
            "clear" => handle_clear(repl)?,
            "quit" | "exit" => handle_quit(repl)?,
            "model" | "models" => config::handle_model(repl, args)?,
            "provider" | "prov" => config::handle_provider(repl, args)?,
            "init" => config::handle_init(repl)?,
            "config" => config::handle_config(repl, args)?,
            "connect" => config::handle_connect(repl, args)?,
            "disconnect" => config::handle_disconnect(repl, args)?,
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
            "notools" => {
                repl.state.tools_enabled = false;
                repl.chat.add_message(
                    ChatRole::System,
                    "Tools disabled — model will respond as plain text. Use /tools to re-enable."
                        .to_string(),
                );
            }
            "select-tools" | "tools" => {
                if !repl.state.tools_enabled {
                    repl.state.tools_enabled = true;
                    repl.chat
                        .add_message(ChatRole::System, "Tools re-enabled.".to_string());
                } else {
                    debug::handle_select_tools(repl)?;
                }
            }
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
            "rewind" | "checkpoint" => session::handle_rewind(repl, args)?,
            "notify" => web::handle_notify(repl, args)?,
            "webhook" => web::handle_webhook(repl, args)?,
            "routine" => loop_engine::handle_routine(repl, args)?,
            "schedule" | "cron" => loop_engine::handle_schedule(repl, args)?,
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
            "color" => config::handle_color(repl, args)?,
            "diag" => debug::handle_diag(repl, args)?,
            "commands" => hooks::handle_commands(repl, args)?,
            "statusline" => config::handle_statusline(repl, args)?,
            "lang" | "language" => config::handle_lang(repl, args)?,
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
    use crate::repl::state::HelpOverlayState;
    let filter = if args.is_empty() {
        None
    } else {
        Some(args.trim().to_string())
    };
    repl.state.help_overlay = Some(HelpOverlayState {
        filter,
        ..Default::default()
    });
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
        repl.chat
            .add_message(ChatRole::System, t!("repl.chat_cleared").to_string());
        if let Some(ref mut engine) = repl.query_engine {
            engine.new_session();
        }
        repl.current_turn = 0;
        repl.state.tokens_used = 0;
    }
    Ok(())
}

fn handle_quit(repl: &mut Repl) -> Result<()> {
    repl.running = false;
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
                    prompt = prompt
                        .replace("$ARGUMENTS", args_val)
                        .replace("{args}", args_val);
                    // Expand built-in template variables
                    prompt = prompt.replace(
                        "$DIR",
                        &std::env::current_dir()
                            .unwrap_or_default()
                            .display()
                            .to_string(),
                    );
                    prompt = prompt.replace(
                        "$DATE",
                        &chrono::Local::now().format("%Y-%m-%d").to_string(),
                    );
                    prompt = prompt.replace(
                        "$TIME",
                        &chrono::Local::now().format("%H:%M:%S").to_string(),
                    );

                    // Run native pre-analysis for supported commands
                    let native_context = match cmd_name {
                        "diff" | "git-diff" => {
                            Some(shannon_commands::diff_utils::run_diff_analysis(args_val))
                        }
                        "review-pr" | "pr-review" => {
                            Some(shannon_commands::review_utils::run_pr_analysis(args_val))
                        }
                        _ => None,
                    };

                    if let Some(ref analysis) = native_context {
                        prompt = format!(
                            "{analysis}\n\n---\n\nBased on the above native analysis, provide additional insights:\n\n{prompt}"
                        );
                    }

                    repl.chat
                        .add_message(ChatRole::System, format!("Running /{cmd_name}..."));
                    super::query::handle_query(repl, &prompt, &mut None)?;
                } else {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("/{cmd_name} — {}", prompt_cmd.base.description),
                    );
                }
            }
            _ => {
                let desc = command.description();
                repl.chat
                    .add_message(ChatRole::System, format!("/{cmd_name} — {desc}"));
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
            repl.chat
                .add_message(ChatRole::System, t!("repl.chat_cleared").to_string());
            if let Some(ref mut engine) = repl.query_engine {
                engine.new_session();
            }
            repl.current_turn = 0;
            repl.state.tokens_used = 0;
        }
        "quit" => {
            repl.running = false;
        }
        "set_bypass_mode" => {
            if let Some(ref query_engine) = repl.query_engine {
                let mut perms = recover_lock(query_engine.permissions().write());
                perms.set_approval_mode(
                    shannon_engine::permissions::ApprovalMode::BypassPermissions,
                );
                drop(perms);
                repl.state.approval_mode_label = "FULL".to_string();
                repl.state.status = "Mode: FULL".to_string();
                repl.state.toast = Some(("  Mode: FULL  ".to_string(), std::time::Instant::now()));
                repl.chat.add_message(
                    ChatRole::System,
                    "Permission bypass enabled — all checks skipped.".to_string(),
                );
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
        let dialog = InputDialog::new(title.to_string()).with_placeholder(placeholder.to_string());
        self.state.input_dialog = Some(Box::new(dialog));
        self.state.input_dialog_action = Some(action.to_string());
    }

    pub(crate) fn show_alert_dialog(&mut self, title: &str, message: &str, danger: bool) {
        use crate::widgets::dialog::AlertDialog;
        let mut builder = AlertDialog::new(title.to_string()).with_message(message.to_string());
        if danger {
            builder = builder.with_danger();
        }
        self.state.active_dialog = Some(builder.build());
        self.state.pending_dialog_action = None;
    }
}

#[cfg(test)]
mod tests {
    use super::redact_secret_command;

    #[test]
    fn redact_connect_key_replaces_inline_key_with_marker() {
        // The plaintext key must never appear in the recorded form.
        let out = redact_secret_command("/connect minimax sk-secret-12345");
        assert_eq!(out, "/connect minimax ***");
        assert!(!out.contains("sk-secret-12345"));
    }

    #[test]
    fn redact_connect_key_preserves_provider_casing_and_leading_whitespace() {
        // Provider echoes back as-typed; leading whitespace is preserved.
        let out = redact_secret_command("   /connect MiniMax abc-KEY-xyz");
        assert_eq!(out, "   /connect MiniMax ***");
    }

    #[test]
    fn redact_connect_key_is_case_insensitive_on_command_name() {
        // Command name matching is case-insensitive; the recorded form is
        // normalized to the canonical lowercase `/connect`.
        let out = redact_secret_command("/CONNECT anthropic sk-ant-9");
        assert_eq!(out, "/connect anthropic ***");
        assert!(!out.contains("sk-ant-9"));
    }

    #[test]
    fn redact_connect_key_handles_whitespace_runs_like_parser() {
        // The real /connect parser (parse_connect_args) treats runs of
        // whitespace as a single separator, so the redactor must too —
        // otherwise a double-spaced key would leak into history verbatim.
        let out = redact_secret_command("/connect    minimax    sk-secret");
        assert_eq!(out, "/connect minimax ***");
        assert!(!out.contains("sk-secret"));
        // Tab-separated form is also covered by split_whitespace.
        assert_eq!(
            redact_secret_command("/connect\tminimax\tsk-secret"),
            "/connect minimax ***"
        );
    }

    #[test]
    fn redact_connect_without_key_is_unchanged() {
        // No inline key → nothing to redact. Must not fabricate a `***`.
        assert_eq!(
            redact_secret_command("/connect minimax"),
            "/connect minimax"
        );
        assert_eq!(redact_secret_command("/connect"), "/connect");
        // A blank key argument is treated as "no key".
        assert_eq!(
            redact_secret_command("/connect minimax "),
            "/connect minimax "
        );
    }

    #[test]
    fn redact_leaves_other_commands_and_free_text_untouched() {
        assert_eq!(redact_secret_command("/model gpt-4o"), "/model gpt-4o");
        assert_eq!(
            redact_secret_command("how do I parse JSON?"),
            "how do I parse JSON?"
        );
        // Inline shell, env-var dumps, etc. are not /connect.
        assert_eq!(redact_secret_command("!echo $HOME"), "!echo $HOME");
    }

    /// Regression guard for the `/notools` dispatch drift (2026-08-28
    /// review PM-1): the `match cmd_name` arm existed, but neither
    /// `repl_only_commands` nor the builtin registry provided the name, so
    /// the gate rejected `/notools` as an unknown command. Every alias
    /// group in the dispatch match must be reachable through at least one
    /// alias: the repl-only list or a registered builtin (name or alias).
    #[test]
    fn every_dispatch_match_arm_is_reachable_from_the_gate() {
        let src = include_str!("mod.rs");

        // Pull the repl-only list body out of this very file so the check
        // can never drift from the compiled list.
        let list_start = src
            .find("let repl_only_commands = [")
            .expect("repl_only_commands array present");
        let list_slice = &src[list_start..];
        let list_end = list_slice
            .find("];")
            .expect("repl_only_commands array terminated");
        let list: std::collections::HashSet<String> = list_slice[..list_end]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect();
        assert!(
            list.contains("notools"),
            "sanity: the fixed drift entry must stay in the list"
        );

        // Bound the `match cmd_name { ... }` block with a brace-counting
        // scan that skips string literals (arm bodies carry format strings).
        let match_start = src
            .find("match cmd_name {")
            .expect("dispatch match present");
        let bytes = src.as_bytes();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut cursor = match_start;
        let mut match_end = None;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' if !in_string => in_string = true,
                b'"' => in_string = false,
                b'\\' if in_string => {
                    cursor += 1; // skip escaped character
                }
                b'{' if !in_string => depth += 1,
                b'}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        match_end = Some(cursor);
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        let match_body = &src[match_start..match_end.expect("dispatch match terminated")];

        // Collect alias groups from arm heads at the outermost arm level
        // (`"a" | "b" => ...`, i.e. brace depth 1 of the dispatch match).
        // Nested matches sit deeper and are ignored.
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut line_depth = 0i32;
        for line in match_body.lines() {
            let mut opens = 0i32;
            let mut closes = 0i32;
            let mut chars = line.chars().peekable();
            let mut line_in_string = false;
            while let Some(ch) = chars.next() {
                match ch {
                    '"' => line_in_string = !line_in_string,
                    '\\' if line_in_string => {
                        chars.next();
                    }
                    '{' if !line_in_string => opens += 1,
                    '}' if !line_in_string => closes += 1,
                    _ => {}
                }
            }
            let trimmed = line.trim_start();
            if line_depth == 1
                && !line_in_string
                && trimmed.starts_with('"')
                && trimmed.contains("=>")
            {
                let head = &trimmed[..trimmed.find("=>").expect("arrow checked")];
                let aliases: Vec<String> = head
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect();
                if !aliases.is_empty() {
                    groups.push(aliases);
                }
            }
            line_depth += opens - closes;
        }

        // Non-vacuous coverage: known-first and known-drifted arms.
        let has = |name: &str| groups.iter().any(|g| g.iter().any(|a| a == name));
        assert!(has("help"), "dispatch arm extraction failed (no `help`)");
        assert!(
            has("notools"),
            "dispatch arm extraction failed (no `notools`)"
        );

        let registry_names: std::collections::HashSet<String> =
            shannon_commands::builtin_commands::all_commands()
                .iter()
                .flat_map(|command| {
                    let mut names = vec![command.name().to_string()];
                    names.extend(command.aliases().iter().cloned());
                    names
                })
                .collect();

        let unreachable: Vec<String> = groups
            .into_iter()
            .filter(|group| {
                !group
                    .iter()
                    .any(|alias| list.contains(alias) || registry_names.contains(alias))
            })
            .flatten()
            .collect();
        assert!(
            unreachable.is_empty(),
            "dispatch arms unreachable from the gate (add to repl_only_commands \
             or the builtin registry): {unreachable:?}"
        );
    }
}
