//! Memory management command handlers

use super::super::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_types::recover_lock;

pub(crate) fn handle_remember(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::{MemoryCategory, MemoryEntry};

    let content = args.trim();
    if content.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.memory.usage_remember").to_string(),
        );
        return Ok(());
    }

    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => {
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.memory.store_not_available").to_string(),
            );
            return Ok(());
        }
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => {
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.memory.store_not_configured").to_string(),
            );
            return Ok(());
        }
    };

    let project = repl.state.working_directory.clone();
    let mut store = recover_lock(memory.write());
    let entry = MemoryEntry::new(&project, MemoryCategory::Context, content);
    let id = entry.id.clone();
    let _ = store.add(entry);
    if let Err(e) = store.save() {
        drop(store);
        super::set_error(repl, &format!("saving memory: {e}"));
        return Ok(());
    }
    drop(store);

    // Also save as file for Claude Code-compatible auto-memory
    let project_path = std::path::PathBuf::from(&project);
    if let Err(e) = shannon_core::project_memory::save_memory_file(&project_path, &id, content) {
        tracing::debug!("File-based memory save skipped: {e}");
    }

    repl.chat.add_message(
        ChatRole::System,
        format!("Remembered (id: {}...)", &id[..8]),
    );
    Ok(())
}

pub(crate) fn handle_recall(repl: &mut Repl, args: &str) -> Result<()> {
    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => {
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.memory.store_not_available").to_string(),
            );
            return Ok(());
        }
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => {
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.memory.store_not_configured").to_string(),
            );
            return Ok(());
        }
    };

    let store = recover_lock(memory.read());
    let project = repl.state.working_directory.clone();

    let results = if args.trim().is_empty() {
        store.project_memories(&project)
    } else {
        store.search(args.trim(), Some(&project))
    };

    if results.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.memory.no_memories").to_string(),
        );
        return Ok(());
    }

    let mut output = format!("Found {} memory(ies):\n\n", results.len());
    for entry in &results {
        let preview = if entry.content.len() > 100 {
            format!("{}...", &entry.content[..100])
        } else {
            entry.content.clone()
        };
        output.push_str(&format!(
            "  [{}] {} (category: {})\n",
            &entry.id[..8],
            preview,
            entry.category
        ));
    }
    output.push_str("\nUse /forget <id> to remove a memory.");
    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

pub(crate) fn handle_forget(repl: &mut Repl, args: &str) -> Result<()> {
    let id_prefix = args.trim();
    if id_prefix.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            "Usage: /forget <memory-id-prefix>".to_string(),
        );
        return Ok(());
    }

    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => {
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.memory.store_not_available").to_string(),
            );
            return Ok(());
        }
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => {
            repl.chat.add_message(
                ChatRole::System,
                t!("commands.memory.store_not_configured").to_string(),
            );
            return Ok(());
        }
    };

    let mut store = recover_lock(memory.write());
    // Find by prefix match
    let found = store
        .project_memories(&repl.state.working_directory)
        .into_iter()
        .find(|e| e.id.starts_with(id_prefix));

    match found {
        Some(entry) => {
            let display = &entry.id[..8.min(entry.id.len())];
            match store.delete(&entry.id) {
                Ok(true) => {
                    let _ = store.save();
                    repl.chat
                        .add_message(ChatRole::System, format!("Forgot memory {display}..."));
                }
                Ok(false) => {
                    repl.chat
                        .add_message(ChatRole::System, "Memory not found.".to_string());
                }
                Err(e) => {
                    drop(store);
                    super::set_error(repl, &format!("deleting memory: {e}"));
                }
            }
        }
        None => {
            repl.chat.add_message(
                ChatRole::System,
                format!("No memory found matching '{id_prefix}'"),
            );
        }
    }
    Ok(())
}

pub(crate) fn handle_memory(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.split_whitespace().next().unwrap_or("");

    match subcmd {
        "cleanup" | "clean" => {
            let engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        t!("commands.memory.store_not_available").to_string(),
                    );
                    return Ok(());
                }
            };
            let memory = match engine.memory() {
                Some(m) => m,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        t!("commands.memory.store_not_configured").to_string(),
                    );
                    return Ok(());
                }
            };
            let mut store = recover_lock(memory.write());
            let removed = store.cleanup(chrono::Duration::days(90), 500).unwrap_or(0);
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Cleanup complete: removed {removed} stale memories. {} remaining.",
                    store.len()
                ),
            );
        }
        _ => {
            let engine = match repl.query_engine.as_ref() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        t!("commands.memory.store_not_available").to_string(),
                    );
                    return Ok(());
                }
            };
            let memory = match engine.memory() {
                Some(m) => m,
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        t!("commands.memory.store_not_configured").to_string(),
                    );
                    return Ok(());
                }
            };
            let store = recover_lock(memory.read());
            let project = repl.state.working_directory.clone();
            let project_count = store.project_memories(&project).len();
            let total = store.len();
            repl.chat.add_message(ChatRole::System, format!(
                "Memory Store:\n  Total entries: {total}\n  Current project: {project_count}\n\nCommands: /remember <text>, /recall [query], /forget <id>, /memory cleanup"));
        }
    }
    Ok(())
}
