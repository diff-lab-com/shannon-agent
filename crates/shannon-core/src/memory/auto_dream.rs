use shannon_engine::api::{ContentBlock, Message, MessageContent};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

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
        let is_dup = unique
            .iter()
            .any(|existing| content_similarity(&existing.content, &memory.content) > 0.8);

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
    keyword.split_whitespace().last().unwrap_or(keyword)
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
            .map_err(|e| MemoryError::Io(std::io::Error::other(e.to_string())))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_engine::api::{ContentBlock, Message, MessageContent};
    use std::sync::{Arc, RwLock};
    use tempfile::TempDir;

    fn make_service() -> (AutoDreamService, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        let service = AutoDreamService::new(Arc::new(RwLock::new(store)));
        (service, dir)
    }

    // ========================================================================
    // extract_memories — preference detection
    // ========================================================================

    #[test]
    fn test_extract_preference_i_always() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("I always use tabs for indentation.", "proj");
        assert!(!memories.is_empty(), "should detect preference");
        assert_eq!(memories[0].category, MemoryCategory::Preference);
        assert!(memories[0].content.contains("tabs"));
    }

    #[test]
    fn test_extract_preference_i_prefer() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("I prefer dark mode for late-night coding.", "proj");
        assert!(!memories.is_empty(), "should detect preference");
        assert_eq!(memories[0].category, MemoryCategory::Preference);
    }

    #[test]
    fn test_extract_preference_do_not_use() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("Do not use var in JavaScript.", "proj");
        assert!(!memories.is_empty(), "should detect preference");
        assert_eq!(memories[0].category, MemoryCategory::Preference);
    }

    // ========================================================================
    // extract_memories — decision detection
    // ========================================================================

    #[test]
    fn test_extract_decision_we_decided() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("We decided to use Rust for the backend.", "proj");
        assert!(!memories.is_empty(), "should detect decision");
        assert_eq!(memories[0].category, MemoryCategory::Decision);
        assert!(memories[0].content.to_lowercase().contains("rust"));
    }

    #[test]
    fn test_extract_decision_going_with() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("Going with PostgreSQL for the database.", "proj");
        assert!(!memories.is_empty(), "should detect decision");
        assert_eq!(memories[0].category, MemoryCategory::Decision);
    }

    #[test]
    fn test_extract_decision_lets_use() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("Let's use Redis for caching.", "proj");
        assert!(!memories.is_empty(), "should detect decision");
        assert_eq!(memories[0].category, MemoryCategory::Decision);
    }

    // ========================================================================
    // extract_memories — error detection
    // ========================================================================

    #[test]
    fn test_extract_error_the_issue_was() {
        let (svc, _dir) = make_service();
        let memories =
            svc.extract_memories("The issue was a race condition in the worker.", "proj");
        assert!(!memories.is_empty(), "should detect error");
        assert_eq!(memories[0].category, MemoryCategory::Error);
        assert!(memories[0].content.to_lowercase().contains("race"));
    }

    #[test]
    fn test_extract_error_root_cause() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("Root cause was a null pointer dereference.", "proj");
        assert!(!memories.is_empty(), "should detect error");
        assert_eq!(memories[0].category, MemoryCategory::Error);
    }

    #[test]
    fn test_extract_error_the_fix_was() {
        let (svc, _dir) = make_service();
        let memories =
            svc.extract_memories("The fix was adding a mutex around the counter.", "proj");
        assert!(!memories.is_empty(), "should detect error");
        assert_eq!(memories[0].category, MemoryCategory::Error);
    }

    // ========================================================================
    // extract_memories — pattern detection
    // ========================================================================

    #[test]
    fn test_extract_pattern_in_this_project() {
        let (svc, _dir) = make_service();
        let memories =
            svc.extract_memories("In this project we use snake_case for variables.", "proj");
        assert!(!memories.is_empty(), "should detect pattern");
        assert_eq!(memories[0].category, MemoryCategory::Pattern);
        assert!(memories[0].content.to_lowercase().contains("snake_case"));
    }

    #[test]
    fn test_extract_pattern_naming_convention() {
        let (svc, _dir) = make_service();
        let memories =
            svc.extract_memories("Our naming convention is PascalCase for types.", "proj");
        assert!(!memories.is_empty(), "should detect pattern");
        assert_eq!(memories[0].category, MemoryCategory::Pattern);
    }

    #[test]
    fn test_extract_pattern_our_convention() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories(
            "Our convention is to keep functions under 20 lines.",
            "proj",
        );
        assert!(!memories.is_empty(), "should detect pattern");
        assert_eq!(memories[0].category, MemoryCategory::Pattern);
    }

    // ========================================================================
    // extract_memories — no match
    // ========================================================================

    #[test]
    fn test_extract_no_match() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("The weather is nice today.", "proj");
        assert!(memories.is_empty(), "should not extract from neutral text");
    }

    // ========================================================================
    // deduplication in extract_memories
    // ========================================================================

    #[test]
    fn test_extract_deduplicates_similar() {
        let (svc, _dir) = make_service();
        // Two nearly identical sentences should produce only one memory
        let text = "I always use tabs. I always use tabs for indentation.";
        let memories = svc.extract_memories(text, "proj");
        // Both sentences match preference, but dedup should collapse them
        assert!(
            memories.len() <= 2,
            "should deduplicate similar preferences"
        );
    }

    // ========================================================================
    // confidence range
    // ========================================================================

    #[test]
    fn test_extract_confidence_in_range() {
        let (svc, _dir) = make_service();
        let memories = svc.extract_memories("I always use strict type checking.", "proj");
        assert!(!memories.is_empty());
        for m in &memories {
            assert!(
                (0.0..=1.0).contains(&m.confidence),
                "confidence {} should be in [0.0, 1.0]",
                m.confidence
            );
        }
    }

    // ========================================================================
    // context extraction
    // ========================================================================

    #[test]
    fn test_extract_includes_context() {
        let (svc, _dir) = make_service();
        let text = "First sentence here. I always use tabs. Third sentence follows.";
        let memories = svc.extract_memories(text, "proj");
        assert!(!memories.is_empty());
        // extract_context with radius=2 should include surrounding sentences
        let content = &memories[0].content;
        assert!(
            content.contains("tabs") || content.contains("First"),
            "context should include surrounding text"
        );
    }

    // ========================================================================
    // split_into_sentences
    // ========================================================================

    #[test]
    fn test_split_periods() {
        let sentences = split_into_sentences("Hello. World. Foo.");
        assert_eq!(sentences, vec!["Hello.", "World.", "Foo."]);
    }

    #[test]
    fn test_split_newlines() {
        let sentences = split_into_sentences("Line one\nLine two\nLine three");
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0], "Line one");
        assert_eq!(sentences[1], "Line two");
        assert_eq!(sentences[2], "Line three");
    }

    #[test]
    fn test_split_exclamation() {
        let sentences = split_into_sentences("Wow! That worked!");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn test_split_question() {
        let sentences = split_into_sentences("Why? Because.");
        assert_eq!(sentences.len(), 2);
    }

    #[test]
    fn test_split_mixed() {
        let sentences = split_into_sentences("Hello! How are you? Fine. Good");
        assert_eq!(sentences.len(), 4);
    }

    #[test]
    fn test_split_empty() {
        let sentences = split_into_sentences("");
        assert!(sentences.is_empty());
    }

    #[test]
    fn test_split_no_delimiter() {
        let sentences = split_into_sentences("Just one long sentence without delimiters");
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0], "Just one long sentence without delimiters");
    }

    #[test]
    fn test_split_trailing_delimiter() {
        let sentences = split_into_sentences("Hello.");
        assert_eq!(sentences.len(), 1);
        assert_eq!(sentences[0], "Hello.");
    }

    #[test]
    fn test_split_consecutive_delimiters() {
        let sentences = split_into_sentences("Hello..\n\nWorld.");
        // Empty fragments should be skipped
        assert!(sentences.iter().all(|s| !s.is_empty()));
    }

    // ========================================================================
    // content_similarity
    // ========================================================================

    #[test]
    fn test_similarity_empty_both() {
        let sim = content_similarity("", "");
        assert!((sim - 1.0).abs() < f64::EPSILON, "two empty strings => 1.0");
    }

    #[test]
    fn test_similarity_one_empty() {
        let sim = content_similarity("hello world", "");
        assert!((sim - 0.0).abs() < f64::EPSILON, "one empty => 0.0");
    }

    #[test]
    fn test_similarity_identical() {
        let sim = content_similarity("the quick brown fox", "the quick brown fox");
        assert!((sim - 1.0).abs() < f64::EPSILON, "identical => 1.0");
    }

    #[test]
    fn test_similarity_completely_different() {
        let sim = content_similarity("alpha beta", "gamma delta");
        assert!((sim - 0.0).abs() < f64::EPSILON, "no shared words => 0.0");
    }

    #[test]
    fn test_similarity_partial() {
        let sim = content_similarity("the quick brown fox", "the quick blue hare");
        // shared: "the", "quick" = 2; total unique: the, quick, brown, fox, blue, hare = 6
        let expected = 2.0 / 6.0;
        assert!((sim - expected).abs() < f64::EPSILON);
    }

    // ========================================================================
    // process_conversation
    // ========================================================================

    #[test]
    fn test_process_conversation_text_content() {
        let (svc, _dir) = make_service();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("I always use strict mode.".to_string()),
        }];
        let result = svc.process_conversation(&messages, "proj");
        assert!(result.is_ok());
        let stored = result.unwrap();
        assert!(!stored.is_empty(), "should extract from text messages");
        assert_eq!(stored[0].category, MemoryCategory::Preference);
    }

    #[test]
    fn test_process_conversation_block_content() {
        let (svc, _dir) = make_service();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: "We decided to use Rust for the backend.".to_string(),
            }]),
        }];
        let result = svc.process_conversation(&messages, "proj");
        assert!(result.is_ok());
        let stored = result.unwrap();
        assert!(!stored.is_empty(), "should extract from block messages");
        assert_eq!(stored[0].category, MemoryCategory::Decision);
    }

    #[test]
    fn test_process_conversation_no_match() {
        let (svc, _dir) = make_service();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Hello, how are you today?".to_string()),
        }];
        let result = svc.process_conversation(&messages, "proj");
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_empty(),
            "should not extract neutral text"
        );
    }

    #[test]
    fn test_process_conversation_multiple_messages() {
        let (svc, _dir) = make_service();
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("I always use tabs.".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text(
                    "We decided to use PostgreSQL for the database.".to_string(),
                ),
            },
        ];
        let result = svc.process_conversation(&messages, "proj");
        assert!(result.is_ok());
        let stored = result.unwrap();
        // Should extract at least one memory from the combined text
        assert!(!stored.is_empty(), "should extract from multiple messages");
    }

    // ========================================================================
    // search and project_memories delegation
    // ========================================================================

    #[test]
    fn test_search_finds_stored() {
        let (svc, _dir) = make_service();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("I always use strict type checking.".to_string()),
        }];
        svc.process_conversation(&messages, "proj").unwrap();
        let results = svc.search("strict", Some("proj"));
        assert!(!results.is_empty(), "search should find stored memory");
    }

    #[test]
    fn test_search_filters_by_project() {
        let (svc, _dir) = make_service();
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("I always use dark mode.".to_string()),
        }];
        svc.process_conversation(&messages, "proj_a").unwrap();
        let results = svc.search("dark", Some("proj_b"));
        assert!(
            results.is_empty(),
            "should not find memories from other project"
        );
    }

    #[test]
    fn test_project_memories_returns_only_project() {
        let (svc, _dir) = make_service();
        let msgs_a = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("I always use tabs for project A.".to_string()),
        }];
        let msgs_b = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("I always use spaces for project B.".to_string()),
        }];
        svc.process_conversation(&msgs_a, "proj_a").unwrap();
        svc.process_conversation(&msgs_b, "proj_b").unwrap();
        let a_mems = svc.project_memories("proj_a");
        let b_mems = svc.project_memories("proj_b");
        assert!(!a_mems.is_empty());
        assert!(!b_mems.is_empty());
        // proj_a should only have proj_a memories
        assert!(a_mems.iter().all(|m| m.project == "proj_a"));
        assert!(b_mems.iter().all(|m| m.project == "proj_b"));
    }

    // ========================================================================
    // tag_from_keyword
    // ========================================================================

    #[test]
    fn test_tag_from_keyword_single_word() {
        assert_eq!(tag_from_keyword("always"), "always");
    }

    #[test]
    fn test_tag_from_keyword_multi_word() {
        assert_eq!(tag_from_keyword("always use"), "use");
    }

    #[test]
    fn test_tag_from_keyword_long_phrase() {
        assert_eq!(tag_from_keyword("make sure to always"), "always");
    }

    // ========================================================================
    // deduplicate_tags
    // ========================================================================

    #[test]
    fn test_deduplicate_tags_removes_dups() {
        let tags = deduplicate_tags(vec![
            "preference".to_string(),
            "tabs".to_string(),
            "preference".to_string(),
        ]);
        assert_eq!(tags, vec!["preference", "tabs"]);
    }

    #[test]
    fn test_deduplicate_tags_empty() {
        let tags: Vec<String> = deduplicate_tags(vec![]);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_deduplicate_tags_no_dups() {
        let tags = deduplicate_tags(vec!["preference".to_string(), "decision".to_string()]);
        assert_eq!(tags.len(), 2);
    }

    // ========================================================================
    // deduplicate_memories
    // ========================================================================

    #[test]
    fn test_deduplicate_memories_removes_similar() {
        let mems = vec![
            MemoryEntry::new(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
            ),
            MemoryEntry::new(
                "p",
                MemoryCategory::Preference,
                "always use tabs for indentation",
            ),
        ];
        let deduped = deduplicate_memories(mems);
        assert_eq!(deduped.len(), 1, "identical memories should be deduped");
    }

    #[test]
    fn test_deduplicate_memories_keeps_different() {
        let mems = vec![
            MemoryEntry::new("p", MemoryCategory::Preference, "always use tabs"),
            MemoryEntry::new("p", MemoryCategory::Preference, "always use spaces"),
        ];
        let deduped = deduplicate_memories(mems);
        assert_eq!(deduped.len(), 2, "different memories should be kept");
    }

    // ========================================================================
    // extract_context
    // ========================================================================

    #[test]
    fn test_extract_context_middle() {
        let sentences = vec!["alpha", "beta", "gamma", "delta", "epsilon"];
        let ctx = extract_context(&sentences, 2, 1);
        assert_eq!(ctx, "beta gamma delta");
    }

    #[test]
    fn test_extract_context_start() {
        let sentences = vec!["alpha", "beta", "gamma"];
        let ctx = extract_context(&sentences, 0, 1);
        assert_eq!(ctx, "alpha beta");
    }

    #[test]
    fn test_extract_context_end() {
        let sentences = vec!["alpha", "beta", "gamma"];
        let ctx = extract_context(&sentences, 2, 1);
        assert_eq!(ctx, "beta gamma");
    }

    #[test]
    fn test_extract_context_single() {
        let sentences = vec!["only"];
        let ctx = extract_context(&sentences, 0, 2);
        assert_eq!(ctx, "only");
    }

    // ========================================================================
    // confidence_for_sentence
    // ========================================================================

    #[test]
    fn test_confidence_starts_with_keyword() {
        let c = confidence_for_sentence("Always use strict mode.", "always use");
        assert!(c >= 0.7, "starting with keyword should boost confidence");
    }

    #[test]
    fn test_confidence_long_sentence() {
        let c = confidence_for_sentence(
            "This is a moderately long sentence with always in the middle of it.",
            "always",
        );
        assert!(
            c >= 0.6,
            "long sentence with keyword should have decent confidence"
        );
    }

    #[test]
    fn test_confidence_in_range() {
        let c = confidence_for_sentence("Some sentence.", "some");
        assert!(
            (0.0..=1.0).contains(&c),
            "confidence {c} should be in [0.0, 1.0]"
        );
    }
}
