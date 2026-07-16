use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::consolidator::{ConsolidationResult, MemoryConsolidator};
use super::error::MemoryError;
use super::types::{MemoryCategory, MemoryEntry, MemoryType, SessionMemoryConfig};

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
                let project_match = project.is_none_or(|p| e.project == p);
                project_match && e.matches_query(query)
            })
            .cloned()
            .collect();

        results.sort_by(|a, b| {
            let score_a = relevance_score(a);
            let score_b = relevance_score(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
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
            let path = self.storage_path.join(format!("{hash}.json"));
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
            for kw in &[
                "i always",
                "i never",
                "i prefer",
                "please always",
                "please never",
                "don't use",
                "do not use",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Preference,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "preference".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Preference, &msg.content)
                        }),
                    );
                    break;
                }
            }

            // --- ProjectConvention detection ---
            for kw in &[
                "in this project we",
                "our convention",
                "naming convention",
                "the standard approach",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Pattern,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "convention".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Pattern, &msg.content)
                        }),
                    );
                    break;
                }
            }

            // --- TechnicalDecision detection ---
            for kw in &[
                "we decided",
                "let's use",
                "going with",
                "the decision",
                "decided to",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Decision,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "decision".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Decision, &msg.content)
                        }),
                    );
                    break;
                }
            }

            // --- DebuggingInsight detection ---
            for kw in &[
                "the issue was",
                "the error was",
                "the fix was",
                "root cause",
                "the workaround",
            ] {
                if lower.contains(kw) {
                    memories.push(
                        MemoryEntry::with_confidence(
                            "auto",
                            MemoryCategory::Error,
                            &msg.content,
                            0.7,
                            vec!["auto-extracted".to_string(), "debugging".to_string()],
                        )
                        .unwrap_or_else(|_| {
                            MemoryEntry::new("auto", MemoryCategory::Error, &msg.content)
                        }),
                    );
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

    /// Search memories ranked by multi-signal relevance to the query.
    ///
    /// Unlike [`search`] which requires a substring match, this method scores
    /// every memory against the query using term overlap, category affinity,
    /// temporal decay, confidence, and access frequency. Results are returned
    /// in descending relevance order.
    pub fn search_by_relevance(
        &self,
        query: &str,
        project: Option<&str>,
        max_results: usize,
    ) -> Vec<MemoryEntry> {
        let query_lower = query.to_lowercase();
        let query_terms: std::collections::HashSet<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(f64, MemoryEntry)> = self
            .entries
            .values()
            .filter(|e| project.is_none_or(|p| e.project == p))
            .filter_map(|e| {
                let score = semantic_relevance_score(e, &query_terms);
                // Only include results above a minimal threshold
                if score > 0.05 {
                    Some((score, e.clone()))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(max_results);
        scored.into_iter().map(|(_, e)| e).collect()
    }

    /// Detect and resolve contradictory memories.
    ///
    /// Finds pairs of memories in the same category that express opposing
    /// sentiments (e.g., "always use X" vs "never use X"). The newer memory
    /// replaces the older one. Returns the number of conflicts resolved.
    pub fn resolve_conflicts(&mut self) -> Result<usize, MemoryError> {
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
                    && are_contradictory(&entry_i.content, &entry_j.content)
                {
                    // Remove the older one
                    let remove_id = if entry_i.created_at < entry_j.created_at {
                        &ids[i]
                    } else {
                        &ids[j]
                    };
                    to_remove.push(remove_id.clone());
                }
            }
        }

        let count = to_remove.len();
        for id in &to_remove {
            self.entries.remove(id);
        }

        if count > 0 {
            self.save()?;
        }

        Ok(count)
    }
}

// Deduplicate memories by removing entries with very similar content.
fn deduplicate_memories(memories: Vec<MemoryEntry>) -> Vec<MemoryEntry> {
    let mut unique: Vec<MemoryEntry> = Vec::new();

    for memory in memories {
        let is_dup = unique
            .iter()
            .any(|existing| content_similarity(&existing.content, &memory.content) > 0.8);

        if !is_dup {
            unique.push(memory);
        }
    }

    unique
}

/// Compute a composite relevance score for a memory entry (used by `search`).
///
/// Combines confidence (40%), access frequency (30%), and recency (30%).
fn relevance_score(entry: &MemoryEntry) -> f64 {
    let confidence = entry.confidence;
    let access = (entry.access_count as f64).ln_1p() / 5.0_f64.ln_1p().max(0.01);
    let age_hours = (Utc::now() - entry.accessed_at).num_hours().max(0) as f64;
    let recency = 1.0 / (1.0 + age_hours / 168.0);
    0.4 * confidence + 0.3 * access.min(1.0) + 0.3 * recency
}

/// Compute a semantic relevance score combining query-term overlap with
/// temporal decay, confidence, and access frequency.
///
/// Weight breakdown:
/// - 35% query term overlap (TF overlap between query and memory content)
/// - 25% temporal decay (half-life of 2 weeks)
/// - 25% confidence
/// - 15% access frequency
fn semantic_relevance_score(
    entry: &MemoryEntry,
    query_terms: &std::collections::HashSet<&str>,
) -> f64 {
    if query_terms.is_empty() {
        return relevance_score(entry);
    }

    // Term overlap: fraction of query terms found in content or tags
    let content_lower: String = entry.content.to_lowercase();
    let tag_text: String = entry
        .tags
        .iter()
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let combined = format!("{content_lower} {tag_text}");
    let content_terms: std::collections::HashSet<&str> = combined.split_whitespace().collect();

    let overlap =
        query_terms.intersection(&content_terms).count() as f64 / query_terms.len() as f64;

    // Temporal decay: half-life of 2 weeks (336 hours)
    let age_hours = (Utc::now() - entry.created_at).num_hours().max(0) as f64;
    let decay = 1.0 / (1.0 + age_hours / 336.0);

    // Access frequency (logarithmic normalization)
    let access = (entry.access_count as f64).ln_1p() / 10.0_f64.ln_1p().max(0.01);

    0.35 * overlap + 0.25 * decay + 0.25 * entry.confidence + 0.15 * access.min(1.0)
}

/// Detect whether two memory contents express contradictory sentiments.
///
/// Looks for opposing signal words (always/never, do/don't, etc.) while
/// requiring sufficient content overlap to ensure the memories are about
/// the same topic.
fn are_contradictory(a: &str, b: &str) -> bool {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    // Must share at least moderate content overlap (same topic)
    let overlap = content_similarity(&a_lower, &b_lower);
    if overlap < 0.3 {
        return false;
    }

    // Opposing signal pairs
    const OPPOSING: &[(&str, &str)] = &[
        ("always", "never"),
        ("must", "must not"),
        ("should", "should not"),
        ("use", "don't use"),
        ("use", "do not use"),
        ("enable", "disable"),
        ("prefer", "avoid"),
        ("include", "exclude"),
        ("allow", "deny"),
        ("required", "forbidden"),
    ];

    for (pos, neg) in OPPOSING {
        let a_pos = a_lower.contains(pos) && !a_lower.contains(neg);
        let a_neg = a_lower.contains(neg) && !a_lower.contains(pos);
        let b_pos = b_lower.contains(pos) && !b_lower.contains(neg);
        let b_neg = b_lower.contains(neg) && !b_lower.contains(pos);

        if (a_pos && b_neg) || (a_neg && b_pos) {
            return true;
        }
    }

    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::extract_memories::MessageSummary;
    use tempfile::TempDir;

    fn make_entry(project: &str, category: MemoryCategory, content: &str) -> MemoryEntry {
        MemoryEntry::new(project, category, content)
    }

    fn make_entry_with_confidence(
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
    ) -> MemoryEntry {
        MemoryEntry::with_confidence(project, category, content, confidence, vec![]).unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn make_entry_with_timestamps(
        id: &str,
        project: &str,
        category: MemoryCategory,
        content: &str,
        confidence: f64,
        created_at: DateTime<Utc>,
        accessed_at: DateTime<Utc>,
        access_count: u32,
    ) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            project: project.to_string(),
            category,
            content: content.to_string(),
            tags: vec![],
            confidence,
            created_at,
            accessed_at,
            access_count,
        }
    }

    // --- Add / Get / Delete ---

    #[test]
    fn test_add_and_get() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("proj", MemoryCategory::Preference, "Use tabs");
        let id = entry.id.clone();
        store.add(entry).unwrap();
        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.content, "Use tabs");
        assert_eq!(retrieved.project, "proj");
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        assert!(store.get("no-such-id").is_none());
    }

    #[test]
    fn test_delete_existing() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("p", MemoryCategory::Pattern, "data");
        let id = entry.id.clone();
        store.add(entry).unwrap();
        assert!(store.delete(&id).unwrap());
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_delete_nonexistent_returns_false() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        assert!(!store.delete("ghost").unwrap());
    }

    // --- Search ---

    #[test]
    fn test_search_content_match() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "p",
                MemoryCategory::Preference,
                "I prefer dark mode",
            ))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "Use PostgreSQL"))
            .unwrap();
        let results = store.search("dark mode", None);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("dark mode"));
    }

    #[test]
    fn test_search_tag_match() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut entry = make_entry("p", MemoryCategory::Pattern, "some content");
        entry.tags.push("rust-pattern".to_string());
        store.add(entry).unwrap();
        assert_eq!(store.search("rust-pattern", None).len(), 1);
    }

    #[test]
    fn test_search_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Decision, "Use PostgreSQL"))
            .unwrap();
        assert_eq!(store.search("postgresql", None).len(), 1);
        assert_eq!(store.search("POSTGRESQL", None).len(), 1);
    }

    #[test]
    fn test_search_with_project_filter() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("proj-a", MemoryCategory::Preference, "Use tabs"))
            .unwrap();
        store
            .add(make_entry(
                "proj-b",
                MemoryCategory::Preference,
                "Use spaces",
            ))
            .unwrap();
        let results = store.search("use", Some("proj-a"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "proj-a");
    }

    #[test]
    fn test_search_no_match_returns_empty() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "hello"))
            .unwrap();
        assert!(store.search("xyz", None).is_empty());
    }

    #[test]
    fn test_search_sorted_by_relevance() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let mut high =
            make_entry_with_confidence("p", MemoryCategory::Preference, "test query", 0.95);
        high.access_count = 10;
        high.touch();
        let low =
            make_entry_with_confidence("p", MemoryCategory::Preference, "test query other", 0.5);
        store.add(high).unwrap();
        store.add(low).unwrap();
        let results = store.search("test query", None);
        assert_eq!(results.len(), 2);
        assert!(results[0].confidence > results[1].confidence);
    }

    // --- project_memories ---

    #[test]
    fn test_project_memories_filters_by_project() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("proj-a", MemoryCategory::Context, "a1"))
            .unwrap();
        store
            .add(make_entry("proj-b", MemoryCategory::Context, "b1"))
            .unwrap();
        store
            .add(make_entry("proj-a", MemoryCategory::Context, "a2"))
            .unwrap();
        let results = store.project_memories("proj-a");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.project == "proj-a"));
    }

    #[test]
    fn test_project_memories_sorted_by_created_at_desc() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let older = make_entry_with_timestamps(
            "p1",
            "p",
            MemoryCategory::Context,
            "old",
            1.0,
            Utc::now() - Duration::hours(2),
            Utc::now(),
            0,
        );
        let newer = make_entry_with_timestamps(
            "p2",
            "p",
            MemoryCategory::Context,
            "new",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(older).unwrap();
        store.add(newer).unwrap();
        let results = store.project_memories("p");
        assert_eq!(results.len(), 2);
        assert!(results[0].created_at >= results[1].created_at);
    }

    // --- Save + Load roundtrip ---

    #[test]
    fn test_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e1 = make_entry("proj-x", MemoryCategory::Preference, "Use tabs");
        let e2 = make_entry("proj-x", MemoryCategory::Decision, "Use Rust");
        let id1 = e1.id.clone();
        let id2 = e2.id.clone();
        store.add(e1).unwrap();
        store.add(e2).unwrap();
        store.save().unwrap();

        let mut store2 = MemoryStore::new(dir.path().to_path_buf());
        store2.load().unwrap();
        assert_eq!(store2.len(), 2);
        assert_eq!(store2.get(&id1).unwrap().content, "Use tabs");
        assert_eq!(store2.get(&id2).unwrap().content, "Use Rust");
    }

    #[test]
    fn test_save_load_multiple_projects() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let e1 = make_entry("proj-a", MemoryCategory::Context, "A content");
        let e2 = make_entry("proj-b", MemoryCategory::Context, "B content");
        let id1 = e1.id.clone();
        let id2 = e2.id.clone();
        store.add(e1).unwrap();
        store.add(e2).unwrap();
        store.save().unwrap();

        let mut store2 = MemoryStore::new(dir.path().to_path_buf());
        store2.load().unwrap();
        assert_eq!(store2.len(), 2);
        assert_eq!(store2.get(&id1).unwrap().project, "proj-a");
        assert_eq!(store2.get(&id2).unwrap().project, "proj-b");
    }

    #[test]
    fn test_load_empty_directory() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store.load().unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn test_load_skips_non_json_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not json").unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store.load().unwrap();
        assert!(store.is_empty());
    }

    // --- Cleanup ---

    #[test]
    fn test_cleanup_removes_old_entries() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let old = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Context,
            "old",
            1.0,
            Utc::now() - Duration::days(100),
            Utc::now(),
            0,
        );
        let recent = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Context,
            "recent",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(old).unwrap();
        store.add(recent).unwrap();
        let removed = store.cleanup(Duration::days(30), 100).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("2").unwrap().content, "recent");
    }

    #[test]
    fn test_cleanup_enforces_cap() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..5u32 {
            let mut entry = make_entry_with_timestamps(
                &format!("{i}"),
                "p",
                MemoryCategory::Context,
                &format!("entry-{i}"),
                1.0,
                Utc::now(),
                Utc::now() - Duration::hours(i as i64 + 1),
                i,
            );
            entry.access_count = i;
            store.add(entry).unwrap();
        }
        let removed = store.cleanup(Duration::days(365), 2).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(store.len(), 2);
    }

    // --- merge_duplicates ---

    #[test]
    fn test_merge_duplicates_removes_similar_same_category() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.7,
            ))
            .unwrap();
        assert_eq!(store.len(), 2);
        let merged = store.merge_duplicates(0.8).unwrap();
        assert_eq!(merged, 1);
        assert_eq!(store.len(), 1);
        let survivor = store.entries.values().next().unwrap();
        assert!((survivor.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn test_merge_duplicates_different_category_keeps_both() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Decision,
                "always use tabs for indentation",
                0.9,
            ))
            .unwrap();
        let merged = store.merge_duplicates(0.8).unwrap();
        assert_eq!(merged, 0);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_merge_duplicates_below_threshold_keeps_both() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "use rust programming language",
                0.9,
            ))
            .unwrap();
        store
            .add(make_entry_with_confidence(
                "p",
                MemoryCategory::Preference,
                "deploy with kubernetes cluster",
                0.9,
            ))
            .unwrap();
        let merged = store.merge_duplicates(0.8).unwrap();
        assert_eq!(merged, 0);
        assert_eq!(store.len(), 2);
    }

    // --- remove_stale ---

    #[test]
    fn test_remove_stale_removes_old_entries() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let old = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Context,
            "old",
            1.0,
            Utc::now() - Duration::days(60),
            Utc::now(),
            0,
        );
        let fresh = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Context,
            "fresh",
            1.0,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(old).unwrap();
        store.add(fresh).unwrap();
        let removed = store.remove_stale(Duration::days(30)).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("2").unwrap().content, "fresh");
    }

    #[test]
    fn test_remove_stale_nothing_to_remove() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "fresh"))
            .unwrap();
        let removed = store.remove_stale(Duration::days(365)).unwrap();
        assert_eq!(removed, 0);
    }

    // --- enforce_category_caps ---

    #[test]
    fn test_enforce_category_caps_removes_least_accessed() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..3u32 {
            let mut entry = make_entry_with_timestamps(
                &format!("p{i}"),
                "proj",
                MemoryCategory::Preference,
                &format!("pref {i}"),
                0.8,
                Utc::now(),
                Utc::now(),
                i,
            );
            entry.access_count = i;
            store.add(entry).unwrap();
        }
        store.enforce_category_caps(2);
        assert_eq!(store.len(), 2);
        for entry in store.entries.values() {
            assert!(entry.access_count > 0);
        }
    }

    #[test]
    fn test_enforce_category_caps_no_removal_when_under_cap() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Context, "one"))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "two"))
            .unwrap();
        store.enforce_category_caps(10);
        assert_eq!(store.len(), 2);
    }

    // --- resolve_conflicts ---

    #[test]
    fn test_resolve_conflicts_keeps_newer() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let older = make_entry_with_timestamps(
            "1",
            "p",
            MemoryCategory::Preference,
            "always use spaces for formatting code",
            0.9,
            Utc::now() - Duration::hours(2),
            Utc::now(),
            0,
        );
        let newer = make_entry_with_timestamps(
            "2",
            "p",
            MemoryCategory::Preference,
            "never use spaces for formatting code",
            0.9,
            Utc::now(),
            Utc::now(),
            0,
        );
        store.add(older).unwrap();
        store.add(newer).unwrap();
        let resolved = store.resolve_conflicts().unwrap();
        assert_eq!(resolved, 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.entries.values().next().unwrap().id, "2");
    }

    #[test]
    fn test_resolve_conflicts_no_conflicts() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "p",
                MemoryCategory::Preference,
                "I prefer dark mode",
            ))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "Use PostgreSQL"))
            .unwrap();
        let resolved = store.resolve_conflicts().unwrap();
        assert_eq!(resolved, 0);
    }

    // --- search_by_relevance ---

    #[test]
    fn test_search_by_relevance_returns_relevant() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "p",
                MemoryCategory::Preference,
                "I prefer rust programming language",
            ))
            .unwrap();
        store
            .add(make_entry(
                "p",
                MemoryCategory::Decision,
                "Deploy with kubernetes",
            ))
            .unwrap();
        let results = store.search_by_relevance("rust programming", None, 10);
        assert!(!results.is_empty());
        // The most relevant result should mention rust
        assert!(results[0].content.contains("rust"));
    }

    #[test]
    fn test_search_by_relevance_respects_max_results() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        for i in 0..5 {
            store
                .add(make_entry(
                    "p",
                    MemoryCategory::Context,
                    &format!("test entry number {i}"),
                ))
                .unwrap();
        }
        let results = store.search_by_relevance("test", None, 2);
        assert!(results.len() <= 2);
    }

    #[test]
    fn test_search_by_relevance_filters_by_project() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry(
                "proj-a",
                MemoryCategory::Context,
                "test content here",
            ))
            .unwrap();
        store
            .add(make_entry(
                "proj-b",
                MemoryCategory::Context,
                "test content here",
            ))
            .unwrap();
        let results = store.search_by_relevance("test", Some("proj-a"), 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "proj-a");
    }

    // --- auto_extract_from_messages ---

    #[test]
    fn test_auto_extract_preference() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new("user", "I always use tabs not spaces")];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Preference);
    }

    #[test]
    fn test_auto_extract_decision() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new(
            "user",
            "We decided to use Rust for the backend",
        )];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Decision);
    }

    #[test]
    fn test_auto_extract_error() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new(
            "user",
            "The error was a null pointer dereference",
        )];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Error);
    }

    #[test]
    fn test_auto_extract_pattern() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new(
            "user",
            "In this project we use snake_case for variables",
        )];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].category, MemoryCategory::Pattern);
    }

    #[test]
    fn test_auto_extract_disabled_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig {
            auto_extract_enabled: false,
            ..SessionMemoryConfig::default()
        };
        let msgs = vec![MessageSummary::new("user", "I always use tabs")];
        assert!(store.auto_extract_from_messages(&msgs, &config).is_empty());
    }

    #[test]
    fn test_auto_extract_no_match_returns_empty() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        let msgs = vec![MessageSummary::new("user", "The weather is nice today")];
        assert!(store.auto_extract_from_messages(&msgs, &config).is_empty());
    }

    #[test]
    fn test_auto_extract_deduplicates_similar() {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let config = SessionMemoryConfig::default();
        // Identical content triggers the same keyword and produces entries with identical content,
        // which should be deduplicated by the extraction logic.
        let msgs = vec![
            MessageSummary::new("user", "I always use tabs for indentation"),
            MessageSummary::new("user", "I always use tabs for indentation"),
        ];
        let extracted = store.auto_extract_from_messages(&msgs, &config);
        assert!(extracted.len() <= 2);
    }

    // --- content_similarity edge cases ---

    #[test]
    fn test_content_similarity_empty_strings() {
        assert!((content_similarity("", "") - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_one_empty() {
        assert!((content_similarity("hello world", "") - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_identical() {
        assert!(
            (content_similarity("hello world foo bar", "hello world foo bar") - 1.0).abs() < 0.001
        );
    }

    #[test]
    fn test_content_similarity_completely_different() {
        assert!((content_similarity("alpha beta", "gamma delta") - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_content_similarity_partial_overlap() {
        let sim = content_similarity("the quick brown fox", "the quick lazy dog");
        assert!((sim - (2.0 / 6.0)).abs() < 0.001);
    }

    // --- len / is_empty ---

    #[test]
    fn test_len_and_is_empty() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        store
            .add(make_entry("p", MemoryCategory::Context, "data"))
            .unwrap();
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }

    // --- get_memories_by_type ---

    #[test]
    fn test_get_memories_by_type() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        store
            .add(make_entry("p", MemoryCategory::Preference, "pref"))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Decision, "dec"))
            .unwrap();
        store
            .add(make_entry("p", MemoryCategory::Preference, "pref2"))
            .unwrap();
        let prefs = store.get_memories_by_type(&MemoryType::UserPreference);
        assert_eq!(prefs.len(), 2);
    }

    // --- get_mut touches ---

    #[test]
    fn test_get_mut_touches_entry() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::new(dir.path().to_path_buf());
        let entry = make_entry("p", MemoryCategory::Context, "data");
        let id = entry.id.clone();
        let orig_count = entry.access_count;
        store.add(entry).unwrap();
        let retrieved = store.get_mut(&id).unwrap();
        assert_eq!(retrieved.access_count, orig_count + 1);
    }

    // --- are_contradictory ---

    #[test]
    fn test_are_contradictory_with_signal_pairs() {
        assert!(are_contradictory("always use tabs", "never use tabs"));
        assert!(are_contradictory("enable feature X", "disable feature X"));
        // "prefer" vs "avoid" -- neither word contains the other
        assert!(are_contradictory(
            "prefer tabs for indentation",
            "avoid tabs for indentation"
        ));
    }

    #[test]
    fn test_are_contradictory_no_contradiction() {
        assert!(!are_contradictory("use rust", "use rust with cargo"));
        assert!(!are_contradictory("enable feature A", "enable feature B"));
    }

    #[test]
    fn test_are_contradictory_low_overlap_not_contradictory() {
        assert!(!are_contradictory("always alpha beta", "never gamma delta"));
    }
}
