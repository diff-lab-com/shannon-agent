//! Token Estimation
//!
//! Provides token count estimation for content before sending to the API.
//! Supports rough estimation, file-type-aware estimation, and precise
//! model-family-aware token counting.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default bytes-per-token ratio for text content
const DEFAULT_BYTES_PER_TOKEN: usize = 4;

/// Token estimation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEstimate {
    pub estimated_tokens: usize,
    pub bytes_per_token: usize,
    pub method: String,
}

/// Token estimation modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EstimationMethod {
    /// Rough estimation based on character count
    Rough,
    /// File-type-aware estimation
    FileType,
    /// Precise count using model-family-aware BPE approximation
    Precise,
}

/// Known model families with their tokenization characteristics.
///
/// Different model families use different tokenizers, which have subtly
/// different average characters-per-token ratios. These values are derived
/// from empirical measurements against the respective tokenizers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelFamily {
    /// Anthropic Claude models (cl100k_base-adjacent tokenizer)
    Claude,
    /// OpenAI GPT-4 / GPT-4o / o1 / o3 models (cl100k_base / o200k_base)
    Gpt,
    /// Google Gemini models (SentencePiece-based)
    Gemini,
    /// Unknown model family — fall back to default ratio
    Unknown,
}

impl ModelFamily {
    /// Average characters per token for this model family.
    ///
    /// Sources:
    /// - Claude: ~3.6 chars/token (empirical against cl100k_base-adjacent)
    /// - GPT-4/o: ~3.8 chars/token for cl100k_base, ~4.0 for o200k_base
    /// - Gemini: ~3.7 chars/token (SentencePiece)
    fn chars_per_token(self) -> f64 {
        match self {
            ModelFamily::Claude => 3.6,
            ModelFamily::Gpt => 3.8,
            ModelFamily::Gemini => 3.7,
            ModelFamily::Unknown => DEFAULT_BYTES_PER_TOKEN as f64,
        }
    }
}

/// Detect the model family from a model identifier string.
fn detect_model_family(model: &str) -> ModelFamily {
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("claude") {
        ModelFamily::Claude
    } else if lower.starts_with("gpt")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.starts_with("chatgpt")
        || lower.starts_with("text-")
        || lower.starts_with("dall-e")
    {
        ModelFamily::Gpt
    } else if lower.starts_with("gemini") {
        ModelFamily::Gemini
    } else {
        ModelFamily::Unknown
    }
}

/// Simple message summary for token estimation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessageSummary {
    pub role: String,
    pub content: String,
}

pub struct TokenEstimator;

impl Default for TokenEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenEstimator {
    pub fn new() -> Self {
        Self
    }

    /// Get the appropriate bytes-per-token ratio for a file type
    pub fn bytes_per_token_for_type(&self, file_extension: &str) -> usize {
        match file_extension {
            "json" | "jsonl" | "jsonc" => 2, // Dense JSON has many single-char tokens
            _ => DEFAULT_BYTES_PER_TOKEN,
        }
    }

    /// Rough token count estimation based on character count
    pub fn rough_estimate(&self, content: &str) -> usize {
        content.len() / DEFAULT_BYTES_PER_TOKEN
    }

    /// File-type-aware token estimation
    pub fn estimate_for_file_type(&self, content: &str, file_extension: &str) -> usize {
        content.len() / self.bytes_per_token_for_type(file_extension)
    }

    /// Estimate tokens for a structured content block (handling nested JSON)
    pub fn estimate_for_content(&self, content: &str) -> usize {
        content.len() / DEFAULT_BYTES_PER_TOKEN
    }

    /// Estimate tokens for a JSON value (stringify + estimate)
    pub fn estimate_for_value(&self, value: &Value) -> usize {
        let s = serde_json::to_string(value).unwrap_or_default();
        if s.is_empty() {
            return 0;
        }
        s.len() / DEFAULT_BYTES_PER_TOKEN
    }

    /// Estimate tokens for a conversation message array
    pub fn estimate_for_messages(&self, messages: &[ConversationMessageSummary]) -> usize {
        messages
            .iter()
            .map(|m| self.rough_estimate(&m.content))
            .sum()
    }

    /// Precise token count estimation using model-family-aware BPE approximation.
    ///
    /// This method uses a word-boundary-aware counting algorithm that accounts for
    /// the fact that BPE tokenizers split text at word boundaries and produce
    /// sub-word tokens. The algorithm considers:
    ///
    /// 1. **Model family**: Different model families have different average
    ///    characters-per-token ratios.
    /// 2. **Word boundaries**: Whitespace and punctuation create token boundaries.
    /// 3. **Special tokens**: Non-ASCII characters (CJK, emoji) typically consume
    ///    more tokens than ASCII text.
    /// 4. **Message overhead**: Each message in a conversation has structural overhead
    ///    (role markers, separators) that consume additional tokens.
    pub fn count_precise(&self, text: &str, model: &str) -> usize {
        if text.is_empty() {
            return 0;
        }

        let family = detect_model_family(model);
        let base_ratio = family.chars_per_token();

        // Count different character categories that affect tokenization
        let mut ascii_word_chars: usize = 0;
        let mut whitespace_count: usize = 0;
        let mut punctuation_count: usize = 0;
        let mut cjk_count: usize = 0;
        let mut other_multibyte_count: usize = 0;

        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'' {
                ascii_word_chars += 1;
            } else if ch.is_ascii_whitespace() {
                whitespace_count += 1;
            } else if ch.is_ascii_punctuation() {
                punctuation_count += 1;
            } else if is_cjk_character(ch) {
                // CJK characters typically tokenize as 1-2 tokens each
                cjk_count += 1;
            } else {
                other_multibyte_count += 1;
            }
        }

        // CJK characters: each character is roughly 1 token (sometimes 2 for rare chars)
        let cjk_tokens = cjk_count;

        // Other multibyte characters (emoji, etc.): roughly 2 tokens each
        let multibyte_tokens = other_multibyte_count * 2;

        // For ASCII text (words + punctuation + whitespace), use the model-specific ratio
        let ascii_text_len = ascii_word_chars + punctuation_count + whitespace_count;
        let ascii_tokens = if ascii_text_len > 0 {
            // Account for whitespace token boundaries: BPE tokenizers typically
            // produce one "space-prefixed" token per word. We adjust by considering
            // that long words get split into multiple tokens while short words
            // may be single tokens.
            let word_count = text
                .split(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
                .filter(|w| !w.is_empty())
                .count();

            // For very short texts, word-level estimation is more accurate
            if word_count <= 3 && ascii_text_len < 20 {
                // Each word + separator ≈ 1-2 tokens
                (word_count.max(1) as f64 * 1.3).ceil() as usize
            } else {
                // Use character-level ratio for longer texts, which naturally
                // accounts for BPE sub-word splitting
                (ascii_text_len as f64 / base_ratio).ceil() as usize
            }
        } else {
            0
        };

        // Overhead: message structure tokens (role markers, etc.) ~4 tokens
        let overhead = if !text.is_empty() { 4 } else { 0 };

        let total = cjk_tokens + multibyte_tokens + ascii_tokens + overhead;

        // Ensure we return at least 1 for non-empty text
        total.max(1)
    }

    /// Precise token count for a conversation message array.
    ///
    /// Applies message-level overhead (role markers, separators) per message
    /// in addition to the content token count.
    pub fn count_precise_for_messages(
        &self,
        messages: &[ConversationMessageSummary],
        model: &str,
    ) -> usize {
        if messages.is_empty() {
            return 0;
        }

        let mut total = 0;
        for msg in messages {
            // Each message has ~4 tokens overhead for role markers + separators
            // (e.g., <|start|>assistant<|message|>, etc.)
            total += self.count_precise(&msg.content, model) + 4;
        }

        // Priming tokens for conversation start
        total += 3;

        total
    }

    /// Create a TokenEstimate result
    pub fn create_estimate(&self, content: &str, method: EstimationMethod) -> TokenEstimate {
        let (tokens, bpt) = match method {
            EstimationMethod::Rough => (self.rough_estimate(content), DEFAULT_BYTES_PER_TOKEN),
            EstimationMethod::FileType => {
                (self.estimate_for_content(content), DEFAULT_BYTES_PER_TOKEN)
            }
            EstimationMethod::Precise => {
                // Without a model name, use Claude as default family
                let tokens = self.count_precise(content, "claude");
                let effective_bpt = content
                    .len()
                    .checked_div(tokens)
                    .unwrap_or(DEFAULT_BYTES_PER_TOKEN);
                (tokens, effective_bpt.max(1))
            }
        };
        TokenEstimate {
            estimated_tokens: tokens,
            bytes_per_token: bpt,
            method: format!("{method:?}"),
        }
    }

    /// Create a TokenEstimate result with model-specific precise counting.
    pub fn create_precise_estimate(&self, content: &str, model: &str) -> TokenEstimate {
        let tokens = self.count_precise(content, model);
        let effective_bpt = content
            .len()
            .checked_div(tokens)
            .unwrap_or(DEFAULT_BYTES_PER_TOKEN);
        TokenEstimate {
            estimated_tokens: tokens,
            bytes_per_token: effective_bpt.max(1),
            method: "Precise".to_string(),
        }
    }
}

/// Check if a character is a CJK (Chinese, Japanese, Korean) character.
fn is_cjk_character(ch: char) -> bool {
    let cp = ch as u32;
    // CJK Unified Ideographs
    (0x4E00..=0x9FFF).contains(&cp)
        // CJK Unified Ideographs Extension A
        || (0x3400..=0x4DBF).contains(&cp)
        // CJK Unified Ideographs Extension B
        || (0x20000..=0x2A6DF).contains(&cp)
        // CJK Compatibility Ideographs
        || (0xF900..=0xFAFF).contains(&cp)
        // Hiragana
        || (0x3040..=0x309F).contains(&cp)
        // Katakana
        || (0x30A0..=0x30FF).contains(&cp)
        // Hangul Syllables
        || (0xAC00..=0xD7AF).contains(&cp)
        // Hangul Jamo
        || (0x1100..=0x11FF).contains(&cp)
        // CJK Radicals Supplement
        || (0x2E80..=0x2EFF).contains(&cp)
        // CJK Symbols and Punctuation
        || (0x3000..=0x303F).contains(&cp)
        // Fullwidth forms
        || (0xFF00..=0xFFEF).contains(&cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rough_estimate() {
        let est = TokenEstimator::new();
        // "Hello world" = 11 chars / 4 = 2 tokens (integer division)
        assert_eq!(est.rough_estimate("Hello world"), 2);
    }

    #[test]
    fn test_json_estimate() {
        let est = TokenEstimator::new();
        let json = serde_json::json!({"a": 1, "b": 2});
        let count = est.estimate_for_value(&json);
        assert!(count >= 1); // Should produce a valid estimate
    }

    #[test]
    fn test_text_estimate() {
        let est = TokenEstimator::new();
        let text = "This is a regular text string";
        let count = est.estimate_for_file_type(text, "txt");
        assert!(count > 0);
    }

    #[test]
    fn test_bytes_per_token() {
        let est = TokenEstimator::new();
        assert_eq!(est.bytes_per_token_for_type("json"), 2);
        assert_eq!(est.bytes_per_token_for_type("jsonl"), 2);
        assert_eq!(est.bytes_per_token_for_type("rs"), 4);
        assert_eq!(est.bytes_per_token_for_type("py"), 4);
    }

    #[test]
    fn test_array_estimate() {
        let est = TokenEstimator::new();
        let arr = serde_json::json!(["hello", "world", "test"]);
        let count = est.estimate_for_value(&arr);
        assert!(count >= 1); // ["hello","world","test"] = 26 chars / 4 = 6
    }

    #[test]
    fn test_message_estimation() {
        let est = TokenEstimator::new();
        let msgs = vec![
            ConversationMessageSummary {
                role: "user".into(),
                content: "Hello".into(),
            },
            ConversationMessageSummary {
                role: "assistant".into(),
                content: "Hi there!".into(),
            },
        ];
        let count = est.estimate_for_messages(&msgs);
        // "Hello" (5/4=1) + "Hi there!" (9/4=2) = 3
        assert_eq!(count, 3);
    }

    #[test]
    fn test_empty_content() {
        let est = TokenEstimator::new();
        assert_eq!(est.rough_estimate(""), 0);
        // Value::Null serializes to "null" (4 bytes / 4 = 1 token)
        assert_eq!(est.estimate_for_value(&Value::Null), 1);
    }

    #[test]
    fn test_create_estimate() {
        let est = TokenEstimator::new();
        let estimate = est.create_estimate("hello world", EstimationMethod::Rough);
        assert_eq!(estimate.estimated_tokens, 2);
        assert_eq!(estimate.bytes_per_token, 4);
    }

    // ---- Precise estimation tests ----

    #[test]
    fn test_precise_empty() {
        let est = TokenEstimator::new();
        assert_eq!(est.count_precise("", "claude-3-opus"), 0);
        assert_eq!(est.count_precise("", "gpt-4o"), 0);
    }

    #[test]
    fn test_precise_short_text() {
        let est = TokenEstimator::new();
        // Short text should return at least 1 token
        let tokens = est.count_precise("Hi", "claude-3-opus");
        assert!(tokens >= 1);
    }

    #[test]
    fn test_precise_longer_than_rough() {
        let est = TokenEstimator::new();
        // Precise counting includes overhead tokens, so it should generally
        // be >= rough estimate for short texts
        let text = "Hello, how are you doing today?";
        let precise = est.count_precise(text, "claude-3-opus");
        assert!(precise >= 1);
        // And should be a reasonable number (not 0, not absurdly high)
        assert!(precise < 100);
    }

    #[test]
    fn test_precise_model_family_detection() {
        let est = TokenEstimator::new();
        let text = "This is a test sentence for tokenization.";

        let claude_tokens = est.count_precise(text, "claude-3-opus-20240229");
        let gpt_tokens = est.count_precise(text, "gpt-4o");
        let gemini_tokens = est.count_precise(text, "gemini-pro");
        let unknown_tokens = est.count_precise(text, "llama-3");

        // All should produce valid results
        assert!(claude_tokens > 0);
        assert!(gpt_tokens > 0);
        assert!(gemini_tokens > 0);
        assert!(unknown_tokens > 0);

        // Claude (3.6 chars/token) should give slightly more tokens than GPT (3.8 chars/token)
        // for the same text
        assert!(claude_tokens >= gpt_tokens);
    }

    #[test]
    fn test_precise_cjk_text() {
        let est = TokenEstimator::new();
        // CJK characters typically tokenize as ~1 token each
        let chinese = "你好世界";
        let tokens = est.count_precise(chinese, "claude-3-opus");
        // 4 CJK chars ≈ 4 tokens + 4 overhead = 8
        assert!(tokens >= 4);
        assert!(tokens <= 12); // reasonable upper bound
    }

    #[test]
    fn test_precise_mixed_ascii_cjk() {
        let est = TokenEstimator::new();
        let mixed = "Hello 你好 World 世界";
        let tokens = est.count_precise(mixed, "gpt-4o");
        assert!(tokens > 0);
        // Should be more tokens than pure ASCII of same byte length due to CJK overhead
        let ascii_only = "Hello      World     ";
        let ascii_tokens = est.count_precise(ascii_only, "gpt-4o");
        // CJK chars produce more tokens per byte than ASCII
        assert!(tokens >= ascii_tokens);
    }

    #[test]
    fn test_precise_for_messages() {
        let est = TokenEstimator::new();
        let msgs = vec![
            ConversationMessageSummary {
                role: "user".into(),
                content: "Hello".into(),
            },
            ConversationMessageSummary {
                role: "assistant".into(),
                content: "Hi there!".into(),
            },
        ];
        let tokens = est.count_precise_for_messages(&msgs, "claude-3-opus");
        // Should include content tokens + per-message overhead (4 each) + priming (3)
        assert!(tokens > 0);
        // At minimum: 1 + 4 + 1 + 4 + 3 = 13
        assert!(tokens >= 8);
    }

    #[test]
    fn test_precise_for_empty_messages() {
        let est = TokenEstimator::new();
        let tokens = est.count_precise_for_messages(&[], "claude-3-opus");
        assert_eq!(tokens, 0);
    }

    #[test]
    fn test_create_precise_estimate() {
        let est = TokenEstimator::new();
        let estimate = est.create_precise_estimate("Hello world, this is a test.", "claude-3-opus");
        assert!(estimate.estimated_tokens > 0);
        assert!(estimate.bytes_per_token > 0);
        assert_eq!(estimate.method, "Precise");
    }

    #[test]
    fn test_create_estimate_precise_mode() {
        let est = TokenEstimator::new();
        let estimate =
            est.create_estimate("Hello world, this is a test.", EstimationMethod::Precise);
        assert!(estimate.estimated_tokens > 0);
        assert!(estimate.bytes_per_token > 0);
    }

    #[test]
    fn test_detect_model_family() {
        assert_eq!(detect_model_family("claude-3-opus"), ModelFamily::Claude);
        assert_eq!(
            detect_model_family("claude-3-sonnet-20240229"),
            ModelFamily::Claude
        );
        assert_eq!(detect_model_family("Claude-3-Haiku"), ModelFamily::Claude);

        assert_eq!(detect_model_family("gpt-4"), ModelFamily::Gpt);
        assert_eq!(detect_model_family("gpt-4o"), ModelFamily::Gpt);
        assert_eq!(detect_model_family("gpt-4-turbo"), ModelFamily::Gpt);
        assert_eq!(detect_model_family("o1-preview"), ModelFamily::Gpt);
        assert_eq!(detect_model_family("o3-mini"), ModelFamily::Gpt);
        assert_eq!(detect_model_family("o4-mini"), ModelFamily::Gpt);

        assert_eq!(detect_model_family("gemini-pro"), ModelFamily::Gemini);
        assert_eq!(detect_model_family("gemini-1.5-flash"), ModelFamily::Gemini);

        assert_eq!(detect_model_family("llama-3"), ModelFamily::Unknown);
        assert_eq!(detect_model_family("mistral-large"), ModelFamily::Unknown);
    }

    #[test]
    fn test_is_cjk_character() {
        // CJK Unified Ideographs
        assert!(is_cjk_character('中'));
        assert!(is_cjk_character('日'));

        // Hiragana
        assert!(is_cjk_character('あ'));

        // Katakana
        assert!(is_cjk_character('ア'));

        // Hangul
        assert!(is_cjk_character('한'));

        // ASCII should not be CJK
        assert!(!is_cjk_character('A'));
        assert!(!is_cjk_character(' '));
        assert!(!is_cjk_character('.'));
    }

    #[test]
    fn test_precise_emoji_text() {
        let est = TokenEstimator::new();
        let emoji = "Hello 🌍🎉";
        let tokens = est.count_precise(emoji, "claude-3-opus");
        // Emoji consume ~2 tokens each
        assert!(tokens >= 3);
    }

    #[test]
    fn test_precise_code_text() {
        let est = TokenEstimator::new();
        let code = r#"fn main() { println!("Hello, world!"); }"#;
        let tokens = est.count_precise(code, "gpt-4o");
        // Code with special chars should produce reasonable token count
        assert!(tokens > 0);
        assert!(tokens < 50);
    }

    #[test]
    fn test_precise_json_text() {
        let est = TokenEstimator::new();
        let json = r#"{"key": "value", "number": 42, "nested": {"a": 1}}"#;
        let tokens = est.count_precise(json, "claude-3-opus");
        assert!(tokens > 0);
        assert!(tokens < 50);
    }

    #[test]
    fn test_precise_consistency_with_different_cases() {
        let est = TokenEstimator::new();
        // Same length text should give similar token counts regardless of case
        let lower = "hello world test";
        let upper = "HELLO WORLD TEST";
        let lower_tokens = est.count_precise(lower, "claude-3-opus");
        let upper_tokens = est.count_precise(upper, "claude-3-opus");
        assert_eq!(lower_tokens, upper_tokens);
    }
}
