//! Memory management command handlers
//!
//! Storage model (single source of truth): the shared
//! [`shannon_core::MemoryStore`] JSONL files under `~/.shannon/memories/`.
//! `/remember` writes through `add_or_update` (dedup + secret redaction);
//! no parallel markdown copies are maintained.

use super::super::Repl;
use crate::{Result, widgets::ChatRole};
use rust_i18n::t;
use shannon_core::memory::GLOBAL_SCOPE;
use shannon_types::recover_lock;

pub(crate) fn handle_remember(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::{MemoryCategory, MemoryEntry};

    let raw = args.trim();
    // `/remember --global <text>` stores a cross-project (user-level) fact.
    let (global, content) = match raw.strip_prefix("--global") {
        Some(rest) => (true, rest.trim()),
        None => (false, raw),
    };
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

    let project = if global {
        GLOBAL_SCOPE.to_string()
    } else {
        repl.state.working_directory.clone()
    };
    let mut store = recover_lock(memory.write());
    let entry = MemoryEntry::new(&project, MemoryCategory::Context, content);
    let id = entry.id.clone();
    let outcome = store.add_or_update(entry);
    if let Err(e) = outcome {
        drop(store);
        super::set_error(repl, &format!("saving memory: {e}"));
        return Ok(());
    }
    if let Err(e) = store.save() {
        drop(store);
        super::set_error(repl, &format!("saving memory: {e}"));
        return Ok(());
    }
    drop(store);

    let scope_note = if global { " (global)" } else { "" };
    repl.chat.add_message(
        ChatRole::System,
        format!("Remembered{scope_note} (id: {}...)", &id[..8]),
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
    let raw = args.trim();
    // `/recall --all [query]` also surfaces bi-temporally expired entries
    // (marked) so invalidated facts remain auditable.
    let (include_expired, query) = match raw.strip_prefix("--all") {
        Some(rest) => (true, rest.trim()),
        None => (false, raw),
    };

    let results = if query.is_empty() {
        if include_expired {
            store.project_memories_all(&project)
        } else {
            store.project_memories(&project)
        }
    } else if include_expired {
        store.search_including_expired(query, Some(&project))
    } else {
        store.search(query, Some(&project))
    };

    if results.is_empty() {
        repl.chat.add_message(
            ChatRole::System,
            t!("commands.memory.no_memories").to_string(),
        );
        return Ok(());
    }

    let now = chrono::Utc::now();
    let mut output = format!("Found {} memory(ies):\n\n", results.len());
    for entry in &results {
        let expired_mark = if entry.is_expired(now) {
            " [expired]"
        } else {
            ""
        };
        let preview = if entry.content.len() > 100 {
            format!("{}...", &entry.content[..100])
        } else {
            entry.content.clone()
        };
        output.push_str(&format!(
            "  [{}]{} {} (category: {})\n",
            &entry.id[..8],
            expired_mark,
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
    // Find by prefix match — project first, then global scope (including
    // expired entries so stale facts stay forgettable).
    let project = repl.state.working_directory.clone();
    let mut found = store
        .project_memories_all(&project)
        .into_iter()
        .find(|e| e.id.starts_with(id_prefix));
    if found.is_none() {
        found = store
            .project_memories_all(GLOBAL_SCOPE)
            .into_iter()
            .find(|e| e.id.starts_with(id_prefix));
    }

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

/// Build the `/memory doctor` report: live/expired counts, per-category
/// distribution, and mergeable near-duplicate pairs. Pure function so it is
/// unit-testable without a TUI.
pub(crate) fn memory_doctor_report(stats: &shannon_core::memory::MemoryDoctorStats) -> String {
    let mut out = String::from("Memory Doctor:\n");
    out.push_str(&format!(
        "  Entries: {} total, {} live, {} expired (excluded from injection)\n",
        stats.total,
        (stats.total - stats.expired),
        stats.expired
    ));
    if !stats.by_category.is_empty() {
        out.push_str("  Live by category:");
        let mut cats: Vec<_> = stats.by_category.iter().collect();
        cats.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (cat, count) in cats {
            out.push_str(&format!(" {cat}={count}"));
        }
        out.push('\n');
    }
    if stats.near_duplicate_pairs > 0 {
        out.push_str(&format!(
            "  ⚠ {} near-duplicate pair(s) — run /memory cleanup to merge them\n",
            stats.near_duplicate_pairs
        ));
    }
    if stats.expired > 0 {
        out.push_str(&format!(
            "  ⚠ {} expired entr{} — /memory cleanup reclaims their lines\n",
            stats.expired,
            if stats.expired == 1 { "y" } else { "ies" }
        ));
    }
    if stats.near_duplicate_pairs == 0 && stats.expired == 0 {
        out.push_str("  No issues found.\n");
    }
    out
}

pub(crate) fn handle_memory(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.split_whitespace().next().unwrap_or("");

    fn get_store(
        repl: &Repl,
    ) -> Option<std::sync::Arc<std::sync::RwLock<shannon_core::MemoryStore>>> {
        let engine = repl.query_engine.as_ref()?;
        engine.memory().cloned()
    }

    match subcmd {
        "cleanup" | "clean" => {
            let Some(memory) = get_store(repl) else {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.memory.store_not_available").to_string(),
                );
                return Ok(());
            };
            let project = repl.state.working_directory.clone();
            let mut store = recover_lock(memory.write());
            // Scoped to the current project: cleanup used to sweep every
            // project in the store and delete other projects' history.
            let removed = store
                .cleanup(&project, chrono::Duration::days(90), 500)
                .unwrap_or(0);
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Cleanup complete: {removed} stale memories invalidated/removed. {} live remaining.",
                    store.len()
                ),
            );
        }
        "doctor" => {
            let Some(memory) = get_store(repl) else {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.memory.store_not_available").to_string(),
                );
                return Ok(());
            };
            let project = repl.state.working_directory.clone();
            let store = recover_lock(memory.read());
            let stats = store.doctor_stats(Some(&project));
            let report = memory_doctor_report(&stats);
            repl.chat.add_message(ChatRole::System, report);
        }
        _ => {
            let Some(memory) = get_store(repl) else {
                repl.chat.add_message(
                    ChatRole::System,
                    t!("commands.memory.store_not_available").to_string(),
                );
                return Ok(());
            };
            let store = recover_lock(memory.read());
            let project = repl.state.working_directory.clone();
            let project_count = store.project_memories(&project).len();
            let global_count = store.project_memories(GLOBAL_SCOPE).len();
            let total = store.len();
            repl.chat.add_message(ChatRole::System, format!(
                "Memory Store:\n  Total live entries: {total}\n  Current project: {project_count}\n  Global (cross-project): {global_count}\n\nCommands: /remember [--global] <text>, /recall [--all] [query], /forget <id>, /memory cleanup, /memory doctor"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_core::{MemoryCategory, MemoryEntry};
    use std::collections::HashMap;

    #[test]
    fn doctor_report_flags_duplicates_and_expired() {
        let mut by_category = HashMap::new();
        by_category.insert(MemoryCategory::Context, 3);
        by_category.insert(MemoryCategory::Preference, 1);
        let stats = shannon_core::memory::MemoryDoctorStats {
            total: 5,
            expired: 1,
            by_category,
            near_duplicate_pairs: 2,
        };
        let report = memory_doctor_report(&stats);
        assert!(report.contains("5 total"), "{report}");
        assert!(report.contains("1 expired"), "{report}");
        assert!(report.contains("2 near-duplicate pair(s)"), "{report}");
        // MemoryCategory Display is lowercase.
        assert!(report.contains("context=3"), "{report}");
        assert!(report.contains("preference=1"), "{report}");
    }

    #[test]
    fn doctor_report_clean_store_says_no_issues() {
        let stats = shannon_core::memory::MemoryDoctorStats::default();
        let report = memory_doctor_report(&stats);
        assert!(report.contains("No issues found"), "{report}");
    }

    #[test]
    fn memory_entry_expired_roundtrip() {
        let mut entry = MemoryEntry::new("p", MemoryCategory::Context, "fact");
        assert!(!entry.is_expired(chrono::Utc::now()));
        entry.expire(chrono::Utc::now());
        assert!(entry.is_expired(chrono::Utc::now()));
        // Invalidation cannot be rolled back: expiring at a later instant
        // keeps the original (earlier) invalidation point.
        let t = entry.valid_until;
        entry.expire(chrono::Utc::now() + chrono::Duration::days(1));
        assert_eq!(entry.valid_until, t, "cannot un-expire");
    }
}
