//! File operation command handlers: search, find, add, export, import, watch, glob.

use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_search(repl: &mut Repl, args: &str) -> Result<()> {
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
pub(crate) fn handle_find(repl: &mut Repl, args: &str) -> Result<()> {
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

pub(crate) fn handle_add(repl: &mut Repl, args: &str) -> Result<()> {
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
pub(crate) fn collect_glob_files(base: &std::path::Path, pattern: &str) -> Vec<std::path::PathBuf> {
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

pub(crate) fn handle_export(repl: &mut Repl, args: &str) -> Result<()> {
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

pub(crate) fn handle_import(repl: &mut Repl, args: &str) -> Result<()> {
    let filename = args.trim();
    if filename.is_empty() {
        repl.chat.add_message(ChatRole::System, "Usage: /import <filename>".to_string());
        return Ok(());
    }

    let content = match std::fs::read_to_string(filename) {
        Ok(c) => c,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Failed to read file '{filename}': {e}"));
            return Ok(());
        }
    };

    let imported_count = if content.trim_start().starts_with('{') || content.trim_start().starts_with('[') {
        // Try JSON import
        import_from_json(repl, &content)?
    } else {
        // Try Markdown import
        import_from_markdown(repl, &content)?
    };

    if imported_count > 0 {
        repl.chat.add_message(ChatRole::System, format!("Imported {imported_count} messages from: {filename}"));
    } else {
        repl.chat.add_message(ChatRole::System, format!("No messages found in: {filename}"));
    }

    Ok(())
}

pub(crate) fn import_from_json(repl: &mut Repl, content: &str) -> Result<usize> {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            repl.chat.add_message(ChatRole::System, format!("Invalid JSON: {e}"));
            return Ok(0);
        }
    };

    let messages = match parsed.get("messages").and_then(|m| m.as_array()) {
        Some(msgs) => msgs,
        None => {
            // Try as a bare array of messages
            if let Some(arr) = parsed.as_array() {
                arr
            } else {
                repl.chat.add_message(ChatRole::System, "JSON does not contain 'messages' array".to_string());
                return Ok(0);
            }
        }
    };

    let mut count = 0;
    for msg in messages {
        let role_str = msg.get("role").and_then(|r| r.as_str()).unwrap_or("system");
        let role = match role_str {
            "user" => ChatRole::User,
            "assistant" => ChatRole::Assistant,
            "tool" => ChatRole::Tool,
            _ => ChatRole::System,
        };
        let text = msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
        if !text.is_empty() {
            repl.chat.add_message(role, text);
            count += 1;
        }
    }

    Ok(count)
}

pub(crate) fn import_from_markdown(repl: &mut Repl, content: &str) -> Result<usize> {
    let mut count = 0;
    for line in content.lines() {
        // Markdown export uses "## role" or "**role**" headers
        if line.starts_with("## user:") || line.starts_with("## User:") {
            let text = line.trim_start_matches('#').trim()
                .trim_start_matches("user:").trim_start_matches("User:")
                .trim().to_string();
            if !text.is_empty() {
                repl.chat.add_message(ChatRole::User, text);
                count += 1;
            }
        } else if line.starts_with("## assistant:") || line.starts_with("## Assistant:") {
            let text = line.trim_start_matches('#').trim()
                .trim_start_matches("assistant:").trim_start_matches("Assistant:")
                .trim().to_string();
            if !text.is_empty() {
                repl.chat.add_message(ChatRole::Assistant, text);
                count += 1;
            }
        } else if line.starts_with("**User**:") || line.starts_with("**user**:") {
            let text = line.trim_start_matches('*').trim()
                .trim_start_matches("User:").trim_start_matches("user:")
                .trim().to_string();
            if !text.is_empty() {
                repl.chat.add_message(ChatRole::User, text);
                count += 1;
            }
        } else if line.starts_with("**Assistant**:") || line.starts_with("**assistant**:") {
            let text = line.trim_start_matches('*').trim()
                .trim_start_matches("Assistant:").trim_start_matches("assistant:")
                .trim().to_string();
            if !text.is_empty() {
                repl.chat.add_message(ChatRole::Assistant, text);
                count += 1;
            }
        }
    }
    Ok(count)
}

pub(crate) fn handle_add_dir(repl: &mut Repl, args: &str) -> Result<()> {
    let path = args.trim();
    if path.is_empty() {
        repl.chat.add_message(ChatRole::System, "Usage: /add-dir <path>\nAdds a directory for file access during this session.".to_string());
        return Ok(());
    }

    // Expand ~/ to home directory
    let expanded = if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(path.strip_prefix("~/").unwrap_or(path)).to_string_lossy().to_string()
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    let abs_path = if std::path::Path::new(&expanded).is_absolute() {
        expanded
    } else {
        let base = std::path::PathBuf::from(&repl.state.working_directory);
        base.join(&expanded).to_string_lossy().to_string()
    };

    let p = std::path::Path::new(&abs_path);
    if !p.exists() {
        repl.chat.add_message(ChatRole::System, format!("Directory not found: {abs_path}"));
        return Ok(());
    }
    if !p.is_dir() {
        repl.chat.add_message(ChatRole::System, format!("Not a directory: {abs_path}"));
        return Ok(());
    }

    if repl.state.extra_dirs.contains(&abs_path) {
        repl.chat.add_message(ChatRole::System, format!("Directory already added: {abs_path}"));
        return Ok(());
    }

    repl.state.extra_dirs.push(abs_path.clone());
    let count = repl.state.extra_dirs.len();
    repl.chat.add_message(ChatRole::System, format!("Added directory: {abs_path}\nExtra directories ({count}): {}", repl.state.extra_dirs.join(", ")));
    Ok(())
}

pub(crate) fn handle_watch(repl: &mut Repl, args: &str) -> Result<()> {
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
                let file = trimmed.strip_prefix("track ").expect("checked starts_with").trim();
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
