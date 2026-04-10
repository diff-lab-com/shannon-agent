use std::sync::{Arc, RwLock};
use crate::api::{Message, MessageContent, ContentBlock};
use std::collections::HashSet;

use super::error::MemoryError;
use super::store::MemoryStore;
use super::types::{MemoryCategory, MemoryEntry};

// ============================================================================
// Extraction helpers
// ============================================================================

/// Keywords that signal a user preference.
const PREFERENCE_KEYWORDS: &[&str] = &[
    "i always",
    "i never",
    "i prefer",
    "prefer to",
    "always use",
    "never use",
    "please always",
    "please never",
    "make sure to always",
    "make sure to never",
    "don't use",
    "do not use",
    "remember to",
    "i like",
    "i dislike",
    "my preference",
    "style guide",
];

/// Keywords that signal an architectural decision.
const DECISION_KEYWORDS: &[&str] = &[
    "we decided",
    "let's use",
    "lets use",
    "we'll use",
    "going with",
    "agreed on",
    "chosen",
    "we chose",
    "decided to",
    "the decision",
    "we're going to",
    "we are going to",
    "as decided",
    "final decision",
    "let's go with",
    "lets go with",
];

/// Keywords that signal an error or solution.
const ERROR_KEYWORDS: &[&str] = &[
    "the issue was",
    "the error was",
    "the bug was",
    "the fix was",
    "the problem was",
    "caused by",
    "broken because",
    "failed because",
    "root cause",
    "the workaround",
    "to fix this",
    "solution was",
    "resolved by",
];

/// Keywords that signal a code pattern.
const PATTERN_KEYWORDS: &[&str] = &[
    "the pattern is",
    "typically we",
    "in this project we",
    "our convention",
    "we follow",
    "our pattern",
    "the standard approach",
    "our standard",
    "code style",
    "naming convention",
    "we structure",
    "our architecture",
    "the pattern here",
];

/// Detect a preference in the given sentence.
fn detect_preference(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in PREFERENCE_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Preference,
                &context,
                confidence,
                vec!["preference".to_string(), tag_from_keyword(kw).to_string()],
            )
            .ok()?;

            // Auto-extract some tags from the content
            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Detect a decision in the given sentence.
fn detect_decision(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in DECISION_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Decision,
                &context,
                confidence,
                vec!["decision".to_string(), tag_from_keyword(kw).to_string()],
            )
            .ok()?;

            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Detect an error/solution in the given sentence.
fn detect_error(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in ERROR_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Error,
                &context,
                confidence,
                vec!["error".to_string(), "solution".to_string()],
            )
            .ok()?;

            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Detect a code pattern in the given sentence.
fn detect_pattern(
    lower: &str,
    sentence: &str,
    sentences: &[&str],
    index: usize,
    project: &str,
) -> Option<MemoryEntry> {
    for kw in PATTERN_KEYWORDS {
        if lower.contains(kw) {
            let context = extract_context(sentences, index, 2);
            let confidence = confidence_for_sentence(sentence, kw);
            let mut entry = MemoryEntry::with_confidence(
                project,
                MemoryCategory::Pattern,
                &context,
                confidence,
                vec!["pattern".to_string()],
            )
            .ok()?;

            entry.tags = deduplicate_tags(entry.tags);
            return Some(entry);
        }
    }
    None
}

/// Extract surrounding context ( +/- `radius` sentences) around the sentence at `index`.
fn extract_context(sentences: &[&str], index: usize, radius: usize) -> String {
    let start = index.saturating_sub(radius);
    let end = (index + radius + 1).min(sentences.len());
    sentences[start..end].join(" ").trim().to_string()
}

/// Compute a confidence score based on how explicitly the keyword was used.
///
/// Higher confidence for:
/// - Sentence starts with the keyword (more direct)
/// - Keyword is longer (more specific)
/// - Sentence is not too short (more context)
fn confidence_for_sentence(sentence: &str, keyword: &str) -> f64 {
    let lower = sentence.to_lowercase();

    let mut confidence: f64 = 0.5;

    // Bonus for sentence starting with keyword
    if lower.starts_with(keyword) {
        confidence += 0.2;
    }

    // Bonus for keyword specificity (length)
    if keyword.len() > 10 {
        confidence += 0.1;
    }

    // Bonus for sentence length (more context = better)
    if sentence.len() > 50 {
        confidence += 0.1;
    }

    // Bonus for explicit markers
    if lower.contains("always") || lower.contains("never") {
        confidence += 0.1;
    }

    confidence.min(1.0)
}

/// Deduplicate memories by removing entries with very similar content.
///
/// Two entries are considered duplicates if their normalized content has a
/// Jaccard similarity above 0.8.
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

/// Simple word-level Jaccard similarity between two strings.
fn content_similarity(a: &str, b: &str) -> f64 {
    let words_a: HashSet<&str> = a.split_whitespace().collect();
    let words_b: HashSet<&str> = b.split_whitespace().collect();

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

/// Create a tag from a keyword phrase.
fn tag_from_keyword(keyword: &str) -> &str {
    // Take the last word of multi-word keywords as a tag
    keyword
        .split_whitespace()
        .last()
        .unwrap_or(keyword)
}

/// Remove duplicate tags from a vector.
fn deduplicate_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// Split text into sentences using common delimiters.
fn split_into_sentences(text: &str) -> Vec<&str> {
    let mut sentences: Vec<&str> = Vec::new();
    let mut start = 0;

    for (i, c) in text.char_indices() {
        if c == '.' || c == '!' || c == '?' || c == '\n' {
            let end = if c == '\n' { i } else { i + 1 };
            let sentence = text[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = i + 1;
        }
    }

    // Don't forget the last fragment
    let remaining = text[start..].trim();
    if !remaining.is_empty() {
        sentences.push(remaining);
    }

    sentences
}

// ============================================================================
// Auto-Dream Service
// ============================================================================

/// Pattern-based memory extraction service.
///
/// Scans conversation text for signals that indicate memorable information
/// (preferences, decisions, errors, patterns) and creates [`MemoryEntry`]
/// instances automatically.
pub struct AutoDreamService {
    store: Arc<RwLock<MemoryStore>>,
}

impl AutoDreamService {
    /// Create a new AutoDreamService backed by the given store.
    pub fn new(store: Arc<RwLock<MemoryStore>>) -> Self {
        Self { store }
    }

    /// Extract memories from a block of text.
    ///
    /// Uses pattern matching to detect:
    /// - **Preferences**: "always/never/prefer/use X not Y"
    /// - **Decisions**: "we decided/let's use/agreed on/chosen X"
    /// - **Errors**: "the issue was/error/bug/fix was/broken because"
    /// - **Patterns**: "the pattern is/typically we/in this project we"
    ///
    /// Surrounding context (approximately +/-2 sentences) is included.
    /// Confidence is assigned based on how explicitly the signal was stated.
    pub fn extract_memories(&self, conversation: &str, project: &str) -> Vec<MemoryEntry> {
        let mut memories = Vec::new();

        let sentences: Vec<&str> = split_into_sentences(conversation);

        for (i, sentence) in sentences.iter().enumerate() {
            let lower = sentence.to_lowercase();

            // --- Preference detection ---
            if let Some(memory) = detect_preference(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }

            // --- Decision detection ---
            if let Some(memory) = detect_decision(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }

            // --- Error detection ---
            if let Some(memory) = detect_error(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }

            // --- Pattern detection ---
            if let Some(memory) = detect_pattern(&lower, sentence, &sentences, i, project) {
                memories.push(memory);
                continue;
            }
        }

        memories
    }

    /// Process a list of conversation messages, extract memories, deduplicate,
    /// and store them.
    ///
    /// Returns the list of newly stored memories (excluding duplicates).
    pub fn process_conversation(
        &self,
        messages: &[Message],
        project: &str,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        // Concatenate all message text
        let full_text: String = messages
            .iter()
            .map(|m| match &m.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect::<Vec<_>>()
            .join("\n");

        let extracted = self.extract_memories(&full_text, project);
        let deduped = deduplicate_memories(extracted);

        // Store all new memories
        let mut store = self
            .store
            .write()
            .map_err(|e| MemoryError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        for entry in &deduped {
            store.add(entry.clone())?;
        }

        store.save()?;
        Ok(deduped)
    }

    /// Search the memory store.
    ///
    /// Convenience method that acquires a read lock on the store.
    pub fn search(&self, query: &str, project: Option<&str>) -> Vec<MemoryEntry> {
        match self.store.read() {
            Ok(store) => store.search(query, project),
            Err(_) => Vec::new(),
        }
    }

    /// Get all memories for a project.
    pub fn project_memories(&self, project: &str) -> Vec<MemoryEntry> {
        match self.store.read() {
            Ok(store) => store.project_memories(project),
            Err(_) => Vec::new(),
        }
    }
}
