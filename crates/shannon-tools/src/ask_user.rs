//! Ask User Question Tool
//!
//! Provides an interactive tool that lets the AI ask the user questions during execution.
//! Supports confirmation prompts, option selection (single/multi), and information gathering.
//!
//! Uses a callback pattern (QuestionHandler trait) so the tool can be decoupled from
//! the specific UI/terminal implementation, similar to plan_mode.rs shared state.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during user question interactions.
#[derive(Debug, thiserror::Error)]
pub enum AskUserError {
    /// An I/O error occurred while reading from or writing to the terminal.
    #[error("{0}")]
    Io(String),
    /// No input was received from the user.
    #[error("No input received")]
    NoInput,
    /// No valid selection was made (e.g. empty multi-select).
    #[error("No valid selection made")]
    NoSelection,
    /// The user's selection index was out of range.
    #[error("Invalid selection {index}. Choose 1-{max} or {other} for Other.")]
    InvalidSelection {
        /// The invalid index the user provided.
        index: usize,
        /// The maximum valid option index.
        max: usize,
        /// The "Other" option index (max + 1).
        other: usize,
    },
    /// The mock handler has no answers configured.
    #[error("MockQuestionHandler has no answers configured")]
    MockNoAnswers,
    /// A custom error from a handler implementation.
    #[error("{0}")]
    Handler(String),
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Input for the AskUserQuestion tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AskUserInput {
    /// The questions to present to the user.
    pub questions: Vec<Question>,
}

/// A single question to ask the user.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Question {
    /// The question text displayed to the user.
    pub question: String,

    /// Short label / chip displayed alongside the question (max 12 chars).
    #[serde(default)]
    pub header: String,

    /// Selectable options. If empty, the question accepts free-form text.
    #[serde(default)]
    pub options: Vec<QuestionOption>,

    /// Allow multiple selections (only meaningful when `options` is non-empty).
    #[serde(default)]
    pub multi_select: bool,
}

/// A selectable option within a question.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuestionOption {
    /// Short display label used as the selection key.
    pub label: String,

    /// Longer human-readable description.
    #[serde(default)]
    pub description: String,
}

/// Answer returned for a single question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionAnswer {
    /// The original question text.
    pub question: String,

    /// Selected option label(s) or free-form text. For multi-select this is
    /// comma-separated labels; for single-select it is the chosen label or free text.
    pub answer: String,

    /// All selected labels (empty for free-text responses).
    #[serde(default)]
    pub answers: Vec<String>,
}

// ---------------------------------------------------------------------------
// QuestionHandler trait
// ---------------------------------------------------------------------------

/// Trait for handling user question interactions.
///
/// Implementations bridge the tool layer to a concrete UI (terminal, TUI, HTTP, etc.).
#[async_trait]
pub trait QuestionHandler: Send + Sync {
    /// Present a question to the user and return the selected answer(s).
    async fn ask_question(&self, question: &Question) -> Result<Vec<String>, AskUserError>;
}

/// Shared question handler that can be injected into the tool.
pub type SharedQuestionHandler = Arc<dyn QuestionHandler>;

// ---------------------------------------------------------------------------
// Terminal implementation
// ---------------------------------------------------------------------------

/// Default handler that reads from stdin / writes to stdout.
pub struct TerminalQuestionHandler;

impl TerminalQuestionHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TerminalQuestionHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl QuestionHandler for TerminalQuestionHandler {
    async fn ask_question(&self, question: &Question) -> Result<Vec<String>, AskUserError> {
        // Print header chip if present
        if !question.header.is_empty() {
            let display_header = if question.header.len() > 12 {
                &question.header[..12]
            } else {
                &question.header
            };
            println!("[{display_header}]");
        }

        // Print question
        println!("{}", question.question);

        // Free-form text (no options)
        if question.options.is_empty() {
            print!("> ");
            io::stdout()
                .flush()
                .map_err(|e| AskUserError::Io(e.to_string()))?;
            let stdin = io::stdin();
            let line = stdin
                .lock()
                .lines()
                .next()
                .ok_or(AskUserError::NoInput)?
                .map_err(|e| AskUserError::Io(e.to_string()))?;
            return Ok(vec![line.trim().to_string()]);
        }

        // Print numbered options
        for (i, opt) in question.options.iter().enumerate() {
            if opt.description.is_empty() {
                println!("  {}. {}", i + 1, opt.label);
            } else {
                println!("  {}. {} - {}", i + 1, opt.label, opt.description);
            }
        }
        // Print "Other" escape hatch
        println!("  {}. Other (free text)", question.options.len() + 1);

        let prompt_suffix = if question.multi_select {
            " (comma-separated)"
        } else {
            ""
        };
        print!("Your choice{prompt_suffix}: ");
        io::stdout()
            .flush()
            .map_err(|e| AskUserError::Io(e.to_string()))?;

        let stdin = io::stdin();
        let raw = stdin
            .lock()
            .lines()
            .next()
            .ok_or(AskUserError::NoInput)?
            .map_err(|e| AskUserError::Io(e.to_string()))?;
        let input = raw.trim();

        // Parse selection(s)
        if question.multi_select {
            let mut selected: Vec<String> = Vec::new();
            for part in input.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let label = resolve_single_choice(part, &question.options)?;
                selected.push(label);
            }
            if selected.is_empty() {
                return Err(AskUserError::NoSelection);
            }
            Ok(selected)
        } else {
            resolve_single_choice(input, &question.options).map(|label| vec![label])
        }
    }
}

/// Resolve a single user choice string against the option list.
///
/// Accepts: number index (1-based), or exact label match.
/// If the choice equals "Other" or falls outside the range, the user is
/// prompted for free-form text.
fn resolve_single_choice(input: &str, options: &[QuestionOption]) -> Result<String, AskUserError> {
    // Try numeric index first
    if let Ok(idx) = input.parse::<usize>() {
        if idx >= 1 && idx <= options.len() {
            return Ok(options[idx - 1].label.clone());
        }
        // "Other" slot
        if idx == options.len() + 1 {
            return prompt_free_text();
        }
        return Err(AskUserError::InvalidSelection {
            index: idx,
            max: options.len(),
            other: options.len() + 1,
        });
    }

    // Try exact label match (case-insensitive)
    let lower = input.to_lowercase();
    for opt in options {
        if opt.label.to_lowercase() == lower {
            return Ok(opt.label.clone());
        }
    }

    // Check if user typed "other" (case-insensitive)
    if lower == "other" {
        return prompt_free_text();
    }

    // Treat as free text (implicit "Other")
    Ok(input.to_string())
}

/// Prompt the user for free-form text on a new line.
fn prompt_free_text() -> Result<String, AskUserError> {
    print!("Enter your answer: ");
    io::stdout()
        .flush()
        .map_err(|e| AskUserError::Io(e.to_string()))?;
    let stdin = io::stdin();
    let line = stdin
        .lock()
        .lines()
        .next()
        .ok_or(AskUserError::NoInput)?
        .map_err(|e| AskUserError::Io(e.to_string()))?;
    Ok(line.trim().to_string())
}

// ---------------------------------------------------------------------------
// AskUserQuestionTool
// ---------------------------------------------------------------------------

/// Tool that asks the user one or more questions during AI execution.
pub struct AskUserQuestionTool {
    handler: SharedQuestionHandler,
}

impl AskUserQuestionTool {
    /// Create a new tool with the given question handler.
    pub fn new(handler: SharedQuestionHandler) -> Self {
        Self { handler }
    }

    /// Convenience constructor using the default terminal handler.
    pub fn with_terminal_handler() -> Self {
        Self::new(Arc::new(TerminalQuestionHandler::new()))
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "ask_user_question"
    }

    fn description(&self) -> &str {
        "Ask the user a question during execution. Supports single-select, multi-select, and free-text input. \
         Use this when you need confirmation, option selection, or information from the user."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "The questions to present to the user.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {
                                "type": "string",
                                "description": "The question text displayed to the user."
                            },
                            "header": {
                                "type": "string",
                                "description": "Short label/chip (max 12 chars)."
                            },
                            "options": {
                                "type": "array",
                                "description": "Selectable options. If empty, accepts free-form text.",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {
                                            "type": "string",
                                            "description": "Short display label."
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "Longer description of the option."
                                        }
                                    },
                                    "required": ["label"]
                                }
                            },
                            "multi_select": {
                                "type": "boolean",
                                "description": "Allow multiple selections.",
                                "default": false
                            }
                        },
                        "required": ["question"]
                    }
                }
            },
            "required": ["questions"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Parse input
        let ask_input: AskUserInput = serde_json::from_value(input.clone()).map_err(|e| {
            ToolError::InvalidInput(format!("Failed to parse ask_user_question input: {e}"))
        })?;

        // Validate: at least one question is required
        if ask_input.questions.is_empty() {
            return Ok(ToolOutput {
                content: "Error: at least one question is required.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        // Iterate questions and collect answers
        let mut answers: Vec<QuestionAnswer> = Vec::new();
        for q in &ask_input.questions {
            match self.handler.ask_question(q).await {
                Ok(selected) => {
                    let answer_str = selected.join(", ");
                    answers.push(QuestionAnswer {
                        question: q.question.clone(),
                        answer: answer_str.clone(),
                        answers: selected,
                    });
                }
                Err(err) => {
                    return Ok(ToolOutput {
                        content: format!("Error asking question '{}': {}", q.question, err),
                        is_error: true,
                        metadata: HashMap::new(),
                    });
                }
            }
        }

        // Build human-readable content
        let mut content_parts: Vec<String> = Vec::new();
        for a in &answers {
            content_parts.push(format!("Q: {}\nA: {}", a.question, a.answer));
        }
        let content = content_parts.join("\n\n");

        // Build metadata with structured answers
        let mut metadata = HashMap::new();
        metadata.insert("answers".to_string(), json!(answers));
        metadata.insert("question_count".to_string(), json!(answers.len()));

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata,
        })
    }

    fn category(&self) -> &str {
        "interaction"
    }
    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Mock handler for tests
// ---------------------------------------------------------------------------

/// A mock handler that returns predetermined answers. Useful for unit tests.
pub struct MockQuestionHandler {
    /// Queue of answers to return (one per question). When the queue is
    /// exhausted the last element is reused.
    answers: Vec<Vec<String>>,
}

impl MockQuestionHandler {
    /// Create a new mock with the given answer sequence.
    ///
    /// Each element is the set of selections for one question call.
    pub fn new(answers: Vec<Vec<String>>) -> Self {
        Self { answers }
    }
}

#[async_trait]
impl QuestionHandler for MockQuestionHandler {
    async fn ask_question(&self, _question: &Question) -> Result<Vec<String>, AskUserError> {
        if self.answers.is_empty() {
            return Err(AskUserError::MockNoAnswers);
        }
        // Always return the first answer (simplest mock behaviour)
        Ok(self.answers[0].clone())
    }
}

/// A mock handler that errors on every call.
pub struct ErrorQuestionHandler {
    pub error_message: String,
}

#[async_trait]
impl QuestionHandler for ErrorQuestionHandler {
    async fn ask_question(&self, _question: &Question) -> Result<Vec<String>, AskUserError> {
        Err(AskUserError::Handler(self.error_message.clone()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Data type tests ---------------------------------------------------

    #[test]
    fn test_question_deserialization() {
        let json_str = r#"{
            "question": "Pick a color",
            "header": "Color",
            "options": [
                {"label": "Red", "description": "The color red"},
                {"label": "Blue", "description": "The color blue"}
            ],
            "multi_select": false
        }"#;
        let q: Question = serde_json::from_str(json_str).unwrap();
        assert_eq!(q.question, "Pick a color");
        assert_eq!(q.header, "Color");
        assert_eq!(q.options.len(), 2);
        assert_eq!(q.options[0].label, "Red");
        assert!(!q.multi_select);
    }

    #[test]
    fn test_question_deserialization_minimal() {
        let json_str = r#"{"question": "What is your name?"}"#;
        let q: Question = serde_json::from_str(json_str).unwrap();
        assert_eq!(q.question, "What is your name?");
        assert!(q.header.is_empty());
        assert!(q.options.is_empty());
        assert!(!q.multi_select);
    }

    #[test]
    fn test_ask_user_input_deserialization() {
        let json_str = r#"{
            "questions": [
                {"question": "Continue?", "header": "Confirm", "options": [{"label": "Yes"}, {"label": "No"}]},
                {"question": "Free text input"}
            ]
        }"#;
        let input: AskUserInput = serde_json::from_str(json_str).unwrap();
        assert_eq!(input.questions.len(), 2);
        assert_eq!(input.questions[0].options.len(), 2);
        assert!(input.questions[1].options.is_empty());
    }

    #[test]
    fn test_question_answer_serialization() {
        let answer = QuestionAnswer {
            question: "Pick a color".to_string(),
            answer: "Red".to_string(),
            answers: vec!["Red".to_string()],
        };
        let json = serde_json::to_string(&answer).unwrap();
        let parsed: QuestionAnswer = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.question, "Pick a color");
        assert_eq!(parsed.answer, "Red");
        assert_eq!(parsed.answers, vec!["Red".to_string()]);
    }

    // -- Tool trait tests ---------------------------------------------------

    #[test]
    fn test_tool_name() {
        let tool = AskUserQuestionTool::with_terminal_handler();
        assert_eq!(tool.name(), "ask_user_question");
    }

    #[test]
    fn test_tool_description() {
        let tool = AskUserQuestionTool::with_terminal_handler();
        let desc = tool.description();
        assert!(desc.contains("question"));
        assert!(desc.contains("user"));
    }

    #[test]
    fn test_tool_category() {
        let tool = AskUserQuestionTool::with_terminal_handler();
        assert_eq!(tool.category(), "interaction");
    }

    #[test]
    fn test_tool_input_schema() {
        let tool = AskUserQuestionTool::with_terminal_handler();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("questions"))
        );
        let questions_schema = &schema["properties"]["questions"];
        assert_eq!(questions_schema["type"], "array");
    }

    // -- Execute tests (using mock handler) --------------------------------

    #[tokio::test]
    async fn test_single_select_valid_option() {
        let handler = Arc::new(MockQuestionHandler::new(vec![vec!["Option A".to_string()]]));
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": [
                {
                    "question": "Pick one",
                    "options": [
                        {"label": "Option A"},
                        {"label": "Option B"}
                    ],
                    "multi_select": false
                }
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Pick one"));
        assert!(result.content.contains("Option A"));

        let answers: Vec<QuestionAnswer> =
            serde_json::from_value(result.metadata["answers"].clone()).unwrap();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].answer, "Option A");
        assert_eq!(answers[0].answers, vec!["Option A".to_string()]);
    }

    #[tokio::test]
    async fn test_single_select_free_text() {
        let handler = Arc::new(MockQuestionHandler::new(vec![vec![
            "My custom answer".to_string(),
        ]]));
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": [
                {
                    "question": "Enter your name",
                    "options": [
                        {"label": "Skip"}
                    ]
                }
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("My custom answer"));
    }

    #[tokio::test]
    async fn test_multi_select() {
        let handler = Arc::new(MockQuestionHandler::new(vec![vec![
            "Alpha".to_string(),
            "Beta".to_string(),
            "Gamma".to_string(),
        ]]));
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": [
                {
                    "question": "Select all that apply",
                    "options": [
                        {"label": "Alpha"},
                        {"label": "Beta"},
                        {"label": "Gamma"},
                        {"label": "Delta"}
                    ],
                    "multi_select": true
                }
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Alpha, Beta, Gamma"));

        let answers: Vec<QuestionAnswer> =
            serde_json::from_value(result.metadata["answers"].clone()).unwrap();
        assert_eq!(answers[0].answers.len(), 3);
    }

    #[tokio::test]
    async fn test_empty_questions_list() {
        let handler = Arc::new(MockQuestionHandler::new(vec![]));
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": []
        });

        let result = tool.execute(input).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("at least one question"));
    }

    #[tokio::test]
    async fn test_handler_error_propagated() {
        let handler = Arc::new(ErrorQuestionHandler {
            error_message: "User cancelled".to_string(),
        });
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": [
                {"question": "Continue?"}
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("User cancelled"));
    }

    #[tokio::test]
    async fn test_multiple_questions() {
        let handler = Arc::new(MockQuestionHandler::new(vec![vec!["Yes".to_string()]]));
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": [
                {"question": "First?"},
                {"question": "Second?"},
                {"question": "Third?"}
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);

        let answers: Vec<QuestionAnswer> =
            serde_json::from_value(result.metadata["answers"].clone()).unwrap();
        assert_eq!(answers.len(), 3);
        assert_eq!(answers[0].question, "First?");
        assert_eq!(answers[1].question, "Second?");
        assert_eq!(answers[2].question, "Third?");
    }

    #[tokio::test]
    async fn test_question_count_metadata() {
        let handler = Arc::new(MockQuestionHandler::new(vec![vec!["Ok".to_string()]]));
        let tool = AskUserQuestionTool::new(handler);

        let input = json!({
            "questions": [
                {"question": "Q1"},
                {"question": "Q2"}
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert_eq!(result.metadata["question_count"].as_u64().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_free_form_question() {
        let handler = Arc::new(MockQuestionHandler::new(vec![vec![
            "hello world".to_string(),
        ]]));
        let tool = AskUserQuestionTool::new(handler);

        // Question with no options at all -- pure free-form
        let input = json!({
            "questions": [
                {
                    "question": "Tell me something"
                }
            ]
        });

        let result = tool.execute(input).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("hello world"));
    }

    #[tokio::test]
    async fn test_invalid_input_json() {
        let handler = Arc::new(MockQuestionHandler::new(vec![]));
        let tool = AskUserQuestionTool::new(handler);

        // Pass something that isn't valid AskUserInput
        let input = json!({"unexpected_key": 42});

        let result = tool.execute(input).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_question_option_serialization_roundtrip() {
        let opt = QuestionOption {
            label: "Test".to_string(),
            description: "A test option".to_string(),
        };
        let json = serde_json::to_string(&opt).unwrap();
        let parsed: QuestionOption = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.label, "Test");
        assert_eq!(parsed.description, "A test option");
    }

    #[test]
    fn test_question_option_default_description() {
        let json_str = r#"{"label": "Only Label"}"#;
        let opt: QuestionOption = serde_json::from_str(json_str).unwrap();
        assert_eq!(opt.label, "Only Label");
        assert!(opt.description.is_empty());
    }
}
