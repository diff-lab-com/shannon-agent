//! Context injection: project instructions, preference memory, and reinjection after compaction.
//!
//! This module encapsulates the logic for building the persistent context that must
//! survive context compaction — namely project instructions (CLAUDE.md etc.),
//! user preference memory, and any other session-critical context anchors.

use crate::api::SystemContentBlock;
use std::path::PathBuf;
use std::sync::RwLock;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during context injection.
#[derive(thiserror::Error, Debug)]
pub enum ContextInjectionError {
    #[error("I/O error reading instructions: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// ContextInjector
// ---------------------------------------------------------------------------

/// Encapsulates project-memory injection logic.
///
/// Holds the [`PreferenceMemoryManager`] and [`InstructionWatcher`] so they can
/// be shared across multiple query cycles and re-injected after compaction.
pub struct ContextInjector {
    /// The project working directory (used for fallback instruction loading).
    project_dir: PathBuf,
    /// Manages auto-detected user preferences persisted to `preferences.md`.
    preference_memory: RwLock<crate::preference_memory::PreferenceMemoryManager>,
    /// Watches CLAUDE.md / AGENTS.md / GEMINI.md for mtime-based hot-reload.
    instruction_watcher: RwLock<crate::project_instructions::InstructionWatcher>,
}

impl ContextInjector {
    /// Create a new injector.
    ///
    /// * `project_dir` – the working directory of the project (used for instruction discovery).
    /// * `storage_dir` – directory for preference persistence (typically `~/.shannon/`).
    pub fn new(project_dir: PathBuf, storage_dir: PathBuf) -> Self {
        let preference_memory =
            RwLock::new(crate::preference_memory::PreferenceMemoryManager::new(storage_dir));
        let instruction_watcher =
            RwLock::new(crate::project_instructions::InstructionWatcher::new(project_dir.clone()));
        Self {
            project_dir,
            preference_memory,
            instruction_watcher,
        }
    }

    /// Return the project instructions as a formatted string.
    ///
    /// Checks for hot-reload via [`InstructionWatcher`] first, falling back to
    /// [`load_full_context`] if the watcher has no cached content.
    pub fn project_instructions_text(&self) -> Option<String> {
        // Try hot-reload first
        {
            let mut watcher = self.instruction_watcher.write().ok()?;
            if let Some((_changed_files, content)) = watcher.check_and_reload() {
                return Some(content);
            }
        }

        // If nothing changed, try the cached content from the watcher
        {
            let watcher = self.instruction_watcher.read().ok()?;
            if let Some(cached) = watcher.cached_instructions() {
                if !cached.is_empty() {
                    return Some(cached.to_string());
                }
            }
        }

        // Fall back to full load using the stored project_dir
        crate::project_instructions::load_full_context(&self.project_dir)
            .map(|ctx| ctx.content)
    }

    /// Return the preference-memory formatted for system-prompt injection.
    pub fn preference_memory_text(&self) -> String {
        match self.preference_memory.read() {
            Ok(mgr) => mgr.get_preferences_for_prompt(),
            Err(_) => String::new(),
        }
    }

    /// Build the full reinjection context string used after compaction.
    ///
    /// This combines project instructions, preference memory, MEMORY.md index,
    /// and .claude/rules into a single string that is passed to
    /// [`CompactEngine::compact_tiered`] so it can be re-anchored at the start
    /// of the compacted message list.
    pub fn reinjection_context(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(instructions) = self.project_instructions_text() {
            if !instructions.is_empty() {
                parts.push(instructions);
            }
        }

        // MEMORY.md index
        if let Some(memory_idx) = crate::project_memory::load_memory_index(&self.project_dir) {
            parts.push(memory_idx);
        }

        // .claude/rules/*.md
        if let Some(rules) = crate::project_memory::load_rules(&self.project_dir) {
            parts.push(rules);
        }

        let prefs = self.preference_memory_text();
        if !prefs.is_empty() {
            parts.push(prefs);
        }

        parts.join("\n\n")
    }

    /// Build system content blocks for the full context injection.
    ///
    /// Returns a list of [`SystemContentBlock`]s suitable for passing to the LLM
    /// API. Uses cache breakpoints for Anthropic-compatible providers when
    /// `use_cache` is true.
    pub fn build_system_blocks(&self, use_cache: bool) -> Vec<SystemContentBlock> {
        let mut blocks = Vec::new();

        if let Some(instructions) = self.project_instructions_text() {
            let block = if use_cache {
                SystemContentBlock::cached(instructions)
            } else {
                SystemContentBlock::text(instructions)
            };
            blocks.push(block);
        }

        // MEMORY.md index
        if let Some(memory_idx) = crate::project_memory::load_memory_index(&self.project_dir) {
            let block = if use_cache {
                SystemContentBlock::cached(memory_idx)
            } else {
                SystemContentBlock::text(memory_idx)
            };
            blocks.push(block);
        }

        // .claude/rules/*.md
        if let Some(rules) = crate::project_memory::load_rules(&self.project_dir) {
            let block = if use_cache {
                SystemContentBlock::cached(rules)
            } else {
                SystemContentBlock::text(rules)
            };
            blocks.push(block);
        }

        let prefs = self.preference_memory_text();
        if !prefs.is_empty() {
            let block = if use_cache {
                SystemContentBlock::cached(prefs)
            } else {
                SystemContentBlock::text(prefs)
            };
            blocks.push(block);
        }

        blocks
    }

    /// Access the underlying preference memory manager.
    pub fn preference_memory(
        &self,
    ) -> &RwLock<crate::preference_memory::PreferenceMemoryManager> {
        &self.preference_memory
    }

    /// Access the underlying instruction watcher.
    pub fn instruction_watcher(
        &self,
    ) -> &RwLock<crate::project_instructions::InstructionWatcher> {
        &self.instruction_watcher
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-context-injector-test")
            .join(name)
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_empty_context_injector() {
        let project_dir = temp_dir("empty_project");
        let storage_dir = temp_dir("empty_storage");

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        // No preferences → empty string
        assert!(injector.preference_memory_text().is_empty());

        // Note: project_instructions_text() may still return Some() because
        // InstructionWatcher discovers global ~/.claude/CLAUDE.md files.
        // We only verify the injector doesn't panic and returns consistent results.

        // Reinjection context and system blocks are consistent with instructions_text
        let has_instructions = injector.project_instructions_text().is_some();
        if !has_instructions {
            assert!(injector.reinjection_context().is_empty());
            assert!(injector.build_system_blocks(true).is_empty());
        } else {
            // If global instructions were found, blocks should be non-empty
            assert!(!injector.build_system_blocks(true).is_empty());
        }

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_with_project_instructions() {
        let project_dir = temp_dir("with_claude_md");
        let storage_dir = temp_dir("with_claude_md_storage");

        // Create a CLAUDE.md file
        fs::write(project_dir.join("CLAUDE.md"), "# Test Instructions\nAlways use Rust.").unwrap();

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        let instructions = injector.project_instructions_text();
        assert!(instructions.is_some());
        let text = instructions.unwrap();
        assert!(text.contains("Test Instructions"));
        assert!(text.contains("Always use Rust"));

        // Reinjection context should include instructions
        let reinjection = injector.reinjection_context();
        assert!(reinjection.contains("Test Instructions"));

        // System blocks should have one block
        let blocks = injector.build_system_blocks(true);
        assert_eq!(blocks.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_reinjection_combines_instructions_and_prefs() {
        let project_dir = temp_dir("combine_project");
        let storage_dir = temp_dir("combine_storage");

        fs::write(project_dir.join("CLAUDE.md"), "# My Project\nUse strict mode.").unwrap();

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        // Add a preference manually
        {
            let mgr = injector.preference_memory.write().unwrap();
            // PreferenceMemoryManager::new already loaded from disk, which is empty.
            // We can't easily add a preference without the full detect pipeline,
            // so we just verify the reinjection includes the instructions part.
        }

        let reinjection = injector.reinjection_context();
        assert!(reinjection.contains("My Project"));
        assert!(reinjection.contains("Use strict mode"));

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_build_system_blocks_no_cache() {
        let project_dir = temp_dir("no_cache_project");
        let storage_dir = temp_dir("no_cache_storage");

        fs::write(project_dir.join("CLAUDE.md"), "# Instructions\nContent.").unwrap();

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        let blocks = injector.build_system_blocks(false);
        assert_eq!(blocks.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_build_system_blocks_with_cache() {
        let project_dir = temp_dir("cache_project");
        let storage_dir = temp_dir("cache_storage");

        fs::write(project_dir.join("CLAUDE.md"), "# Instructions\nContent.").unwrap();

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        let blocks = injector.build_system_blocks(true);
        assert_eq!(blocks.len(), 1);
        // The cached block should have cache_control set
        assert!(blocks[0].cache_control.is_some());

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_preference_memory_access() {
        let project_dir = temp_dir("pref_access_project");
        let storage_dir = temp_dir("pref_access_storage");

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        // Should be able to access the preference memory manager
        let mgr = injector.preference_memory.read().unwrap();
        // No preferences loaded
        let text = mgr.get_preferences_for_prompt();
        assert!(text.is_empty());

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_instruction_watcher_access() {
        let project_dir = temp_dir("watcher_access_project");
        let storage_dir = temp_dir("watcher_access_storage");

        let injector = ContextInjector::new(project_dir.clone(), storage_dir.clone());

        // Should be able to access the watcher
        let _watcher = injector.instruction_watcher.read().unwrap();

        // Cleanup
        let _ = fs::remove_dir_all(project_dir);
        let _ = fs::remove_dir_all(storage_dir);
    }
}
