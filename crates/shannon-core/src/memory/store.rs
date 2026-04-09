use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::error::MemoryError;
use super::types::{MemoryCategory, MemoryEntry, MemoryType, SessionMemoryConfig};
use super::consolidator::{ConsolidationResult, MemoryConsolidator};

// Hash a project path to a safe filename.
fn project_hash(project: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    project.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// Simple word-level Jaccard similarity between two strings.
fn content_similarity(a: &str, b: &str) -> f64 {
    let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 1.0;
    }

    intersection as f64 / union as f64
}

// ============================================================================
// Memory Store
// ============================================================================

/// Persistent storage for memory entries, backed by JSON files on disk.
///
/// Each project's memories are stored in a separate file:
/// `{storage_path}/{project_hash}.json`
pub struct MemoryStore {
    entries: HashMap<String, MemoryEntry>,
    storage_path: PathBuf,
}

impl MemoryStore {
    /// Create a new empty memory store.
    ///
    /// The storage directory will be created on [`load`](Self::load) if it
    /// does not already exist.
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            entries: HashMap::new(),
            storage_path,
        }
    }

    /// Add a memory entry to the store.
    ///
    /// If an entry with the same ID already exists it will be overwritten.
    pub fn add(&mut self, entry: MemoryEntry) -> Result<(), MemoryError> {
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }

    /// Retrieve a memory entry by ID.
    ///
    /// Returns `None` if no entry with the given ID exists.
    pub fn get(&self, id: &str) -> Option<&MemoryEntry> {
        self.entries.get(id)
    }

    /// Retrieve a mutable memory entry by ID and record an access.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut MemoryEntry> {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.touch();
        }
        self.entries.get_mut(id)
    }

    /// Search memories by substring match on content and tags.
    ///
    /// If `project` is provided, results are filtered to that project.
    /// Results are sorted by confidence descending.
    pub fn search(&self, query: &str, project: Option<&str>) -> Vec<MemoryEntry> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| {
                let project_match = project.map_or(true, |p| e.project == p);
                project_match && e.matches_query(query)
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        results
    }

    /// Get all memories belonging to a specific project.
    ///
    /// Results are sorted by creation date, most recent first.
    pub fn project_memories(&self, project: &str) -> Vec<MemoryEntry> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .values()
            .filter(|e| e.project == project)
            .cloned()
            .collect();

        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        results
    }

    /// Delete a memory entry by ID.
    ///
    /// Returns `Ok(true)` if the entry was found and removed, `Ok(false)` otherwise.
    pub fn delete(&mut self, id: &str) -> Result<bool, MemoryError> {
        Ok(self.entries.remove(id).is_some())
    }

    /// Persist all memories to disk as JSON.
    ///
    /// One JSON file per project is written to `{storage_path}/{project_hash}.json`.
    /// Each file contains a `Vec<MemoryEntry>`.
    pub fn save(&self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.storage_path)?;

        // Group entries by project
        let mut by_project: HashMap<String, Vec<&MemoryEntry>> = HashMap::new();
        for entry in self.entries.values() {
            by_project
                .entry(entry.project.clone())
                .or_default()
                .push(entry);
        }

        for (project, entries) in &by_project {
            let hash = project_hash(project);
            let path = self.storage_path.join(format!("{}.json", hash));
            let json = serde_json::to_string_pretty(entries)?;
            fs::write(&path, json)?;
        }

        Ok(())
    }

    /// Load memories from disk.
    ///
    /// Reads all `{project_hash}.json` files from the storage directory and
    /// merges them into the in-memory store. Creates the storage directory
    /// if it does not exist.
    pub fn load(&mut self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.storage_path)?;

        if !self.storage_path.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(&self.storage_path)?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let contents = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let file_entries: Vec<MemoryEntry> = match serde_json::from_str(&contents) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for mem in file_entries {
                self.entries.insert(mem.id.clone(), mem);
            }
        }

        Ok(())
    }

    /// Remove old entries and cap the total count.
    ///
    /// First removes entries older than `max_age`. Then, if the store still
    /// exceeds `max_entries`, removes the least-recently-accessed entries until
    /// the cap is met.
    ///
    /// Returns the total number of entries removed.
    pub fn cleanup(&mut self, max_age: Duration, max_entries: usize) -> Result<usize, MemoryError> {
        let cutoff = Utc::now() - max_age;
        let initial_count = self.entries.len();

        // Remove entries older than max_age
        self.entries.retain(|_, entry| entry.created_at > cutoff);

        // If still over capacity, remove least-recently-accessed entries
        if self.entries.len() > max_entries {
            let mut access_times: Vec<(String, DateTime<Utc>)> = self
                .entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.accessed_at))
                .collect();

            access_times.sort_by_key(|(_, t)| *t);

            let to_remove = self.entries.len() - max_entries;
            for (id, _) in access_times.into_iter().take(to_remove) {
                self.entries.remove(&id);
            }
        }

        // Persist changes after cleanup
        self.save()?;

        Ok(initial_count - self.entries.len())
    }

    /// Return the number of entries currently in the store.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return true if the store contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get memories filtered by [`MemoryType`].
    ///
    /// Maps the `MemoryType` to the corresponding [`MemoryCategory`] and
    /// returns all entries matching that category.
    pub fn get_memories_by_type(&self, memory_type: &MemoryType) -> Vec<MemoryEntry> {
        let category: MemoryCategory = memory_type.clone().into();
        self.entries
            .values()
            .filter(|e| e.category == category)
            .cloned()
            .collect()
    }

    /// Consolidate memories: merge duplicates, remove stale entries, enforce caps.
    ///
    /// This is a convenience method that creates a default [`MemoryConsolidator`]
    /// and runs consolidation with the given config.
    pub fn consolidate_memories(
        &mut self,
        config: &SessionMemoryConfig,
    ) -> Result<ConsolidationResult, MemoryError> {
        let consolidator = MemoryConsolidator::default();
        consolidator.consolidate(self, config)
    }

    /// Auto-extract memories from a list of message summaries.
    ///
    /// Uses pattern matching to detect preferences, decisions, errors, and
    /// conventions in the provided messages, returning newly extracted
    /// [`MemoryEntry`] instances.
    pub fn auto_extract_from_messages(
        &self,
        messages: &[crate::extract_memories::MessageSummary],
        config: &SessionMemoryConfig,
    ) -> Vec<MemoryEntry> {
        if !config.auto_extract_enabled {
            return Vec::new();
        }

        let mut memories = Vec::new();

        for msg in messages {
            let lower = msg.content.to_lowercase();

            // --- UserPreference detection ---
            for kw in &["i always", "i never", "i prefer", "please always", "please never", "don't use", "do not use"] {
                if lower.contains(kw) {
                    memories.push(MemoryEntry::with_confidence(
                        "auto",
                        MemoryCategory::Preference,
                        &msg.content,
                        0.7,
                        vec!["auto-extracted".to_string(), "preference".to_string()],
                    ).unwrap_or_else(|_| MemoryEntry::new("auto", MemoryCategory::Preference, &msg.content)));
                    break;
                }
            }

            // --- ProjectConvention detection ---
            for kw in &["in this project we", "our convention", "naming convention", "the standard approach"] {
                if lower.contains(kw) {
                    memories.push(MemoryEntry::with_confidence(
                        "auto",
                        MemoryCategory::Pattern,
                        &msg.content,
                        0.7,
                        vec!["auto-extracted".to_string(), "convention".to_string()],
                    ).unwrap_or_else(|_| MemoryEntry::new("auto", MemoryCategory::Pattern, &msg.content)));
                    break;
                }
            }

            // --- TechnicalDecision detection ---
            for kw in &["we decided", "let's use", "going with", "the decision", "decided to"] {
                if lower.contains(kw) {
                    memories.push(MemoryEntry::with_confidence(
                        "auto",
                        MemoryCategory::Decision,
                        &msg.content,
                        0.7,
                        vec!["auto-extracted".to_string(), "decision".to_string()],
                    ).unwrap_or_else(|_| MemoryEntry::new("auto", MemoryCategory::Decision, &msg.content)));
                    break;
                }
            }

            // --- DebuggingInsight detection ---
            for kw in &["the issue was", "the error was", "the fix was", "root cause", "the workaround"] {
                if lower.contains(kw) {
                    memories.push(MemoryEntry::with_confidence(
                        "auto",
                        MemoryCategory::Error,
                        &msg.content,
                        0.7,
                        vec!["auto-extracted".to_string(), "debugging".to_string()],
                    ).unwrap_or_else(|_| MemoryEntry::new("auto", MemoryCategory::Error, &msg.content)));
                    break;
                }
            }
        }

        // Deduplicate by content similarity
        deduplicate_memories(memories)
    }

    /// Merge duplicate memories based on Jaccard similarity.
    ///
    /// When two entries have similarity above the threshold, the one with
    /// the higher confidence is kept and the other is removed.
    /// Returns the number of duplicates removed.
    pub fn merge_duplicates(&mut self, similarity_threshold: f64) -> Result<usize, MemoryError> {
        let mut to_remove: Vec<String> = Vec::new();
        let ids: Vec<String> = self.entries.keys().cloned().collect();

        for i in 0..ids.len() {
            if to_remove.contains(&ids[i]) {
                continue;
            }
            for j in (i + 1)..ids.len() {
                if to_remove.contains(&ids[j]) {
                    continue;
                }
                let entry_i = &self.entries[&ids[i]];
                let entry_j = &self.entries[&ids[j]];

                if entry_i.category == entry_j.category
                    && content_similarity(&entry_i.content, &entry_j.content) > similarity_threshold
                {
                    // Remove the one with lower confidence
                    let remove_id = if entry_i.confidence >= entry_j.confidence {
                        &ids[j]
                    } else {
                        &ids[i]
                    };
                    to_remove.push(remove_id.clone());
                }
            }
        }

        for id in &to_remove {
            self.entries.remove(id);
        }

        Ok(to_remove.len())
    }

    /// Remove entries that are older than the given TTL.
    ///
    /// Returns the number of entries removed.
    pub fn remove_stale(&mut self, ttl: Duration) -> Result<usize, MemoryError> {
        let cutoff = Utc::now() - ttl;
        let initial_count = self.entries.len();

        self.entries.retain(|_, entry| entry.created_at > cutoff);

        Ok(initial_count - self.entries.len())
    }

    /// Enforce per-category caps by removing the least-accessed entries.
    pub fn enforce_category_caps(&mut self, max_per_category: usize) {
        let mut by_category: HashMap<MemoryCategory, Vec<String>> = HashMap::new();

        for (id, entry) in &self.entries {
            by_category
                .entry(entry.category.clone())
                .or_default()
                .push(id.clone());
        }

        for (_category, mut ids) in by_category {
            if ids.len() <= max_per_category {
                continue;
            }

            // Sort by access count ascending, then by accessed_at ascending
            ids.sort_by(|a, b| {
                let entry_a = &self.entries[a];
                let entry_b = &self.entries[b];
                entry_a
                    .access_count
                    .cmp(&entry_b.access_count)
                    .then_with(|| entry_a.accessed_at.cmp(&entry_b.accessed_at))
            });

            let to_remove = ids.len() - max_per_category;
            for id in ids.into_iter().take(to_remove) {
                self.entries.remove(&id);
            }
        }
    }
}

// Deduplicate memories by removing entries with very similar content.
fn deduplicate_memories(memories: Vec<MemoryEntry>) -> Vec<MemoryEntry> {
    let mut unique: Vec<MemoryEntry> = Vec::new();

    for memory in memories {
        let is_dup = unique.iter().any(|existing| {
            content_similarity(&existing.content, &memory.content) > 0.8
        });

        if !is_dup {
            unique.push(memory);
        }
    }

    unique
}
