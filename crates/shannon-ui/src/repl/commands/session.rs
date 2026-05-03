//! Session management command handlers

use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_sessions(repl: &mut Repl, args: &str) -> Result<()> {
    let sessions = match repl.state_manager.list_persisted_sessions() {
        Ok(s) => s,
        Err(e) => {
            super::set_error(repl, &format!("listing sessions: {e}"));
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

pub(crate) fn handle_resume(repl: &mut Repl, args: &str) -> Result<()> {
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
            super::set_error(repl, &format!("session not found: {session_id}"));
        }
        Err(e) => {
            super::set_error(repl, &format!("loading session: {e}"));
        }
    }

    Ok(())
}

pub(crate) fn handle_branch(repl: &mut Repl, args: &str) -> Result<()> {
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
            super::set_error(repl, &format!("loading session for branch: {e}"));
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
            super::set_error(repl, &format!("creating branch: {e}"));
        }
    }

    Ok(())
}

pub(crate) fn handle_history(repl: &mut Repl, args: &str) -> Result<()> {
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
            Err(e) => { super::set_error(repl, &format!("exporting session: {e}")); }
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

pub(crate) fn handle_undo(repl: &mut Repl, args: &str) -> Result<()> {
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

pub(crate) fn handle_rewind(repl: &mut Repl, args: &str) -> Result<()> {
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

pub(crate) fn handle_plan(repl: &mut Repl, args: &str) -> Result<()> {
    let args = args.trim();

    // Handle plan mode deactivation
    if args == "off" || args == "exit" || args == "end" {
        if let Ok(mut flag) = repl.plan_mode_flag.write() {
            *flag = false;
        }
        repl.state.plan.active = false;
        repl.state.plan.approved = false;
        repl.chat.add_message(ChatRole::System, "Plan mode deactivated. Write operations are now enabled.".to_string());
        return Ok(());
    }

    // Delegate to cost::handle_plan for all other cases (creates plan, status, approve, reject, etc.)
    // and also activate the plan-mode flag so write tools are blocked.
    super::cost::handle_plan(repl, args)?;

    // If a plan was created (active and has content), also set the engine flag
    if repl.state.plan.active {
        if let Ok(mut flag) = repl.plan_mode_flag.write() {
            *flag = true;
        }
    }

    Ok(())
}

pub(crate) fn handle_compact(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::compact::{CompactEngine, CompactStrategy};

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let history = engine.conversation_history();

    // Analyze first — use LLM summarizer for quality, fallback to rule-based
    let client = engine.client().clone();
    let compact_engine = match CompactEngine::with_llm_summarizer(client) {
        Ok(e) => e,
        Err(_) => match CompactEngine::with_defaults() {
            Ok(e) => e,
            Err(e) => {
                repl.chat.add_message(ChatRole::System, format!("Compact engine error: {e}"));
                return Ok(());
            }
        },
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

    // /compact preview — show what will be compacted without doing it
    if subcmd == "preview" {
        let total = history.len();
        let recent_keep = 6; // matches default keep_recent_count
        let old_count = total.saturating_sub(recent_keep);
        let mut preview = format!(
            "Compact Preview:\n  Total messages: {total}\n  Keep recent: {recent_keep}\n  Compactible: {old_count}\n  Strategy: {}\n  Estimated tokens: {} ({:.1}% of context)",
            analysis.recommended_strategy,
            analysis.estimated_tokens,
            analysis.context_usage_ratio * 100.0,
        );
        if old_count > 0 {
            preview.push_str("\n\nMessages to compact:");
            let preview_count = old_count.min(10);
            for (i, msg) in history.iter().take(preview_count).enumerate() {
                let role = &msg.role;
                let preview_text: String = match &msg.content {
                    shannon_core::api::MessageContent::Text(t) => t.chars().take(60).collect(),
                    shannon_core::api::MessageContent::Blocks(blocks) => {
                        blocks.iter().take(1).filter_map(|b| match b {
                            shannon_core::api::ContentBlock::Text { text } => Some(text.chars().take(60).collect::<String>()),
                            _ => None,
                        }).next().unwrap_or_default()
                    }
                };
                preview.push_str(&format!("\n  {}. [{role}] {preview_text}{}", i + 1, if preview_text.len() >= 60 { "..." } else { "" }));
            }
            if old_count > preview_count {
                preview.push_str(&format!("\n  ... and {} more", old_count - preview_count));
            }
        }
        preview.push_str("\n\nUse /compact to proceed, or /compact <strategy> to choose a strategy.");
        repl.chat.add_message(ChatRole::System, preview);
        return Ok(());
    }

    // /compact focus <topic> — compact but preserve messages about topic
    let (strategy, focus_keywords) = if let Some(focus) = subcmd.strip_prefix("focus ") {
        let keywords: Vec<&str> = focus.split_whitespace().collect();
        let mut filtered: Vec<shannon_core::api::Message> = Vec::new();
        let mut preserved = 0;
        for msg in &history {
            let text = match &msg.content {
                shannon_core::api::MessageContent::Text(t) => t.to_lowercase(),
                shannon_core::api::MessageContent::Blocks(blocks) => blocks.iter()
                    .filter_map(|b| match b { shannon_core::api::ContentBlock::Text { text } => Some(text.clone()), _ => None })
                    .collect::<Vec<_>>().join(" ").to_lowercase(),
            };
            let matches_focus = keywords.iter().any(|kw| text.contains(kw));
            if matches_focus || msg.role == "system" {
                preserved += 1;
                filtered.push(msg.clone());
            } else {
                filtered.push(msg.clone());
            }
        }
        repl.chat.add_message(ChatRole::System, format!(
            "Focus compact: preserving {preserved} messages matching '{}'\nCompacting remaining messages...",
            keywords.join("', '")
        ));
        (CompactStrategy::SummarizeOld, Some(keywords.into_iter().map(String::from).collect::<Vec<_>>()))
    } else {
        let strategy = match subcmd {
            "truncate" => CompactStrategy::TruncateOld,
            "micro" => CompactStrategy::MicroCompress,
            "group" => CompactStrategy::GroupCompress,
            "auto" | "" => CompactStrategy::SummarizeOld,
            // Treat unrecognized non-empty args as focus keywords (freeform instructions)
            other if !other.is_empty() => {
                let keywords: Vec<&str> = other.split_whitespace().collect();
                repl.chat.add_message(ChatRole::System, format!(
                    "Focus compact: preserving messages matching '{}'\nCompacting remaining messages...",
                    keywords.join("', '")
                ));
                return handle_compact_with_focus(repl, keywords, history, compact_engine);
            }
            _ => CompactStrategy::SummarizeOld,
        };
        (strategy, None)
    };

    let (messages, compact_result) = if let Some(ref keywords) = focus_keywords {
        // For focus mode, compact only non-matching messages
        let mut to_compact: Vec<shannon_core::api::Message> = Vec::new();
        let mut to_keep: Vec<shannon_core::api::Message> = Vec::new();
        for msg in history {
            let text = match &msg.content {
                shannon_core::api::MessageContent::Text(t) => t.to_lowercase(),
                shannon_core::api::MessageContent::Blocks(blocks) => blocks.iter()
                    .filter_map(|b| match b { shannon_core::api::ContentBlock::Text { text } => Some(text.clone()), _ => None })
                    .collect::<Vec<_>>().join(" ").to_lowercase(),
            };
            let matches_focus = keywords.iter().any(|kw| text.contains(&kw.to_lowercase()));
            if matches_focus || msg.role == "system" {
                to_keep.push(msg);
            } else {
                to_compact.push(msg);
            }
        }
        let _original_count = to_compact.len();
        if !to_compact.is_empty() {
            let mut compact_engine = compact_engine;
            let cr = compact_engine.compact(&mut to_compact);
            to_keep.append(&mut to_compact);
            (to_keep, cr.ok())
        } else {
            (to_keep, None)
        }
    } else {
        let mut messages = history;
        let mut compact_engine = compact_engine;
        let result = match strategy {
            CompactStrategy::MicroCompress => compact_engine.micro_compact(&mut messages),
            CompactStrategy::GroupCompress => compact_engine.group_compact(&mut messages),
            _ => compact_engine.compact(&mut messages),
        };
        (messages, result.ok())
    };

    // Update the query engine's conversation
    if let Some(ref mut engine) = repl.query_engine {
        engine.replace_conversation(messages);
    }

    if let Some(compact_result) = compact_result {
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
        if let Some(ref kws) = focus_keywords {
            report.push_str(&format!("\n  Focus: {}", kws.join(", ")));
        }
        repl.chat.add_message(ChatRole::System, report);
    } else if focus_keywords.is_some() {
        repl.chat.add_message(ChatRole::System, "Focus compact complete (no compaction needed for focused messages).".to_string());
    }

    Ok(())
}

/// Helper for freeform-focus compaction (called when /compact receives unrecognized non-empty args).
fn handle_compact_with_focus(
    repl: &mut Repl,
    keywords: Vec<&str>,
    history: Vec<shannon_core::api::Message>,
    compact_engine: shannon_core::compact::CompactEngine,
) -> Result<()> {
    let keyword_strings: Vec<String> = keywords.iter().map(|s| s.to_string()).collect();
    let mut to_compact: Vec<shannon_core::api::Message> = Vec::new();
    let mut to_keep: Vec<shannon_core::api::Message> = Vec::new();
    for msg in history {
        let text = match &msg.content {
            shannon_core::api::MessageContent::Text(t) => t.to_lowercase(),
            shannon_core::api::MessageContent::Blocks(blocks) => blocks.iter()
                .filter_map(|b| match b { shannon_core::api::ContentBlock::Text { text } => Some(text.clone()), _ => None })
                .collect::<Vec<_>>().join(" ").to_lowercase(),
        };
        let matches_focus = keywords.iter().any(|kw| text.contains(&kw.to_lowercase()));
        if matches_focus || msg.role == "system" {
            to_keep.push(msg);
        } else {
            to_compact.push(msg);
        }
    }
    let compact_result = if !to_compact.is_empty() {
        let mut compact_engine = compact_engine;
        let cr = compact_engine.compact(&mut to_compact);
        to_keep.append(&mut to_compact);
        cr.ok()
    } else {
        None
    };

    if let Some(ref mut engine) = repl.query_engine {
        engine.replace_conversation(to_keep);
    }

    if let Some(cr) = compact_result {
        repl.chat.add_message(ChatRole::System, format!(
            "Context compacted (focus: {}):\n  Tokens: {} → {} ({:.1}% reduction)\n  Messages removed: {}",
            keyword_strings.join(", "),
            cr.original_tokens, cr.compacted_tokens, cr.reduction_ratio * 100.0,
            cr.messages_removed,
        ));
    } else {
        repl.chat.add_message(ChatRole::System, "Focus compact complete (no compaction needed for focused messages).".to_string());
    }

    Ok(())
}

/// /session — manage conversation sessions (list, export).
pub(crate) fn handle_session(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();
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
                    super::set_error(repl, &format!("exporting session: {e}"));
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

pub(crate) fn handle_rename(repl: &mut Repl, args: &str) -> Result<()> {
    let name = args.trim();
    if name.is_empty() {
        // Show current title
        match &repl.state.session_title {
            Some(title) => {
                repl.chat.add_message(ChatRole::System, format!("Current session: {title}\nUsage: /rename <new-name>"));
            }
            None => {
                repl.chat.add_message(ChatRole::System, "No custom session name set.\nUsage: /rename <new-name>".to_string());
            }
        }
        return Ok(());
    }

    if name == "reset" || name == "clear" {
        repl.state.session_title = None;
        repl.chat.add_message(ChatRole::System, "Session name reset to default.".to_string());
        return Ok(());
    }

    repl.state.session_title = Some(name.to_string());
    repl.chat.add_message(ChatRole::System, format!("Session renamed to: {name}"));
    Ok(())
}

/// /recap — Generate a summary of the conversation so far.
///
/// Shows message counts by role, the last N user messages, total turns,
/// and the session title if set. REPL-only, no API call.
pub(crate) fn handle_recap(repl: &mut Repl, _args: &str) -> Result<()> {
    let total = repl.chat.len();
    if total == 0 {
        repl.chat.add_message(ChatRole::System, "No messages in this session yet.".to_string());
        return Ok(());
    }

    let mut user_count = 0usize;
    let mut assistant_count = 0usize;
    let mut system_count = 0usize;
    let mut tool_count = 0usize;
    let mut user_messages: Vec<String> = Vec::new();

    for i in 0..repl.chat.len() {
        if let Some(msg) = repl.chat.get_message(i) {
            match msg.role {
                ChatRole::User => {
                    user_count += 1;
                    let preview: String = msg.content.chars().take(80).collect();
                    let ellipsis = if msg.content.len() > 80 { "..." } else { "" };
                    user_messages.push(format!("{preview}{ellipsis}"));
                }
                ChatRole::Assistant => assistant_count += 1,
                ChatRole::System => system_count += 1,
                ChatRole::Tool => tool_count += 1,
            }
        }
    }

    let mut output = String::from("Conversation Recap:\n\n");
    output.push_str(&format!(
        "  Messages: {total} total ({user_count} user, {assistant_count} assistant, {system_count} system, {tool_count} tool)\n"
    ));
    output.push_str(&format!("  Turns: {}\n", repl.state.turn_count));

    if let Some(ref title) = repl.state.session_title {
        output.push_str(&format!("  Session: \"{title}\"\n"));
    }

    if let Some(started) = &repl.session_started_at {
        let elapsed = chrono::Utc::now() - *started;
        let mins = elapsed.num_minutes();
        let secs = elapsed.num_seconds() % 60;
        output.push_str(&format!("  Duration: {mins}m {secs}s\n"));
    }

    let model = repl.state.model.as_deref().unwrap_or("default");
    output.push_str(&format!("  Model: {model}\n"));

    if !user_messages.is_empty() {
        let last_n: Vec<&String> = user_messages.iter().rev().take(5).collect();
        output.push_str("\n  Recent user messages:\n");
        for (i, msg) in last_n.iter().rev().enumerate() {
            output.push_str(&format!("    {}. {msg}\n", i + 1));
        }
    }

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// /effort — Set or view the thinking effort level for the model.
///
/// With no args: show current effort level.
/// With args "low", "medium", "high": set the effort level.
pub(crate) fn handle_effort(repl: &mut Repl, args: &str) -> Result<()> {
    let level = args.trim().to_lowercase();

    if level.is_empty() {
        match &repl.state.effort_level {
            Some(effort) => {
                repl.chat.add_message(ChatRole::System, format!("Current effort level: {effort}\nUsage: /effort <low|medium|high>"));
            }
            None => {
                repl.chat.add_message(ChatRole::System, "No effort level set (using model default).\nUsage: /effort <low|medium|high>".to_string());
            }
        }
        return Ok(());
    }

    match level.as_str() {
        "low" | "medium" | "high" => {
            repl.state.effort_level = Some(level.clone());
            repl.chat.add_message(ChatRole::System, format!("Effort level set to: {level}"));
        }
        _ => {
            repl.chat.add_message(ChatRole::System, "Invalid effort level. Use: low, medium, or high.".to_string());
        }
    }

    Ok(())
}

/// /focus — Set context focus to limit what the model pays attention to.
///
/// With no args: show current focus.
/// With args: set focus area (e.g., "frontend", "backend", "security").
/// With "off" or "clear": remove focus.
pub(crate) fn handle_focus(repl: &mut Repl, args: &str) -> Result<()> {
    let area = args.trim();

    if area.is_empty() {
        match &repl.state.focus_area {
            Some(focus) => {
                repl.chat.add_message(ChatRole::System, format!("Current focus: {focus}\nUsage: /focus <area> | /focus off"));
            }
            None => {
                repl.chat.add_message(ChatRole::System, "No focus area set.\nUsage: /focus <area> (e.g., frontend, backend, security)".to_string());
            }
        }
        return Ok(());
    }

    let area_lower = area.to_lowercase();
    if area_lower == "off" || area_lower == "clear" {
        repl.state.focus_area = None;
        repl.chat.add_message(ChatRole::System, "Focus area cleared.".to_string());
    } else {
        repl.state.focus_area = Some(area.to_string());
        repl.chat.add_message(ChatRole::System, format!("Focus area set to: {area}"));
    }

    Ok(())
}
