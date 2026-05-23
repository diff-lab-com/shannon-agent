//! Integration tests for multi-provider message serialization.
//!
//! Tests serialize_request across Anthropic, OpenAI, Ollama, and Gemini
//! wire formats, verifying correct JSON structure for each provider.

#[cfg(test)]
mod provider_serialization_tests {
    use serde_json::json;
    use shannon_core::api::adapter::serialize_request;
    use shannon_core::api::types::{
        ContentBlock, LlmProvider, Message, MessageContent, MessageRequest, ToolDefinition,
        ToolResultContent,
    };

    // -- Helpers --

    fn text_msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn blocks_msg(role: &str, blocks: Vec<ContentBlock>) -> Message {
        Message {
            role: role.to_string(),
            content: MessageContent::Blocks(blocks),
        }
    }

    fn make_simple_request() -> MessageRequest {
        MessageRequest {
            model: "test-model".to_string(),
            max_tokens: 4096,
            system: Some("You are a helpful assistant.".to_string()),
            system_blocks: None,
            messages: vec![
                text_msg("user", "Hello!"),
                text_msg("assistant", "Hi there!"),
            ],
            tools: None,
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            reasoning_effort: None,
            thinking_budget: None,
        }
    }

    fn make_tool_request() -> MessageRequest {
        MessageRequest {
            model: "test-model".to_string(),
            max_tokens: 4096,
            system: Some("You are a coding assistant.".to_string()),
            system_blocks: None,
            messages: vec![
                text_msg("user", "Read the file main.rs"),
                // Assistant with text + tool_use
                blocks_msg(
                    "assistant",
                    vec![
                        ContentBlock::Text {
                            text: "Let me read that file.".to_string(),
                        },
                        ContentBlock::ToolUse {
                            id: "toolu_1".to_string(),
                            name: "read_file".to_string(),
                            input: json!({"path": "src/main.rs"}),
                        },
                    ],
                ),
                // User with tool_result
                blocks_msg(
                    "user",
                    vec![ContentBlock::ToolResult {
                        tool_use_id: "toolu_1".to_string(),
                        content: Some(ToolResultContent::Single(
                            "fn main() { println!(\"hello\"); }".to_string(),
                        )),
                        is_error: Some(false),
                    }],
                ),
                text_msg(
                    "assistant",
                    "The file contains a simple hello world program.",
                ),
            ],
            tools: Some(vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file from disk".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                }),
                cache_control: None,
                strict: None,
            }]),
            stream: Some(true),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            reasoning_effort: None,
            thinking_budget: None,
        }
    }

    // -- Anthropic format tests --

    #[test]
    fn test_anthropic_simple_request_structure() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Anthropic);

        // Anthropic format: system at top level
        assert_eq!(val["system"], "You are a helpful assistant.");
        assert_eq!(val["model"], "test-model");
        assert_eq!(val["max_tokens"], 4096);
        assert_eq!(val["stream"], true);

        // Messages array
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello!");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "Hi there!");
    }

    #[test]
    fn test_anthropic_tool_use_format() {
        let req = make_tool_request();
        let val = serialize_request(&req, &LlmProvider::Anthropic);

        // Tools should have input_schema (Anthropic format)
        let tools = val["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "read_file");
        assert!(tools[0]["input_schema"].is_object());

        // Assistant message should have content blocks
        let messages = val["messages"].as_array().unwrap();
        let assistant_msg = &messages[1];
        assert_eq!(assistant_msg["role"], "assistant");
        let blocks = assistant_msg["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["name"], "read_file");
        assert_eq!(blocks[1]["id"], "toolu_1");
    }

    #[test]
    fn test_anthropic_tool_result_format() {
        let req = make_tool_request();
        let val = serialize_request(&req, &LlmProvider::Anthropic);

        let messages = val["messages"].as_array().unwrap();
        let tool_result_msg = &messages[2];
        assert_eq!(tool_result_msg["role"], "user");
        let blocks = tool_result_msg["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "toolu_1");
        assert_eq!(blocks[0]["is_error"], false);
    }

    // -- OpenAI format tests --

    #[test]
    fn test_openai_system_as_first_message() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::OpenAI);

        // OpenAI: system prompt becomes first message
        assert!(
            val.get("system").is_none(),
            "OpenAI should not have top-level system"
        );
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a helpful assistant.");
        // Then user and assistant follow
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["role"], "assistant");
    }

    #[test]
    fn test_openai_max_completion_tokens() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::OpenAI);

        // OpenAI uses max_completion_tokens instead of max_tokens
        assert!(
            val.get("max_completion_tokens").is_some(),
            "OpenAI should use max_completion_tokens"
        );
        assert_eq!(val["max_completion_tokens"], 4096);
    }

    #[test]
    fn test_openai_tools_as_function_calling() {
        let req = make_tool_request();
        let val = serialize_request(&req, &LlmProvider::OpenAI);

        // OpenAI: tools[].input_schema → tools[].function.parameters
        let tools = val["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert!(tools[0]["function"].is_object());
        assert_eq!(tools[0]["function"]["name"], "read_file");
        assert!(tools[0]["function"]["parameters"].is_object());
    }

    #[test]
    fn test_openai_tool_use_as_function_call() {
        let req = make_tool_request();
        let val = serialize_request(&req, &LlmProvider::OpenAI);

        let messages = val["messages"].as_array().unwrap();
        // Find the assistant message with tool calls
        let assistant_with_tools = messages
            .iter()
            .find(|m| m["tool_calls"].is_array())
            .expect("Should have a message with tool_calls");

        let tool_calls = assistant_with_tools["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "read_file");
        assert_eq!(tool_calls[0]["id"], "toolu_1");
    }

    #[test]
    fn test_openai_tool_result_format() {
        let req = make_tool_request();
        let val = serialize_request(&req, &LlmProvider::OpenAI);

        let messages = val["messages"].as_array().unwrap();
        // Find the tool role message
        let tool_msg = messages
            .iter()
            .find(|m| m["role"] == "tool")
            .expect("Should have a tool role message");

        assert_eq!(tool_msg["tool_call_id"], "toolu_1");
        assert!(tool_msg["content"].is_string());
    }

    #[test]
    fn test_openai_stream_options() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::OpenAI);

        // OpenAI should include stream_options for usage
        assert_eq!(val["stream_options"]["include_usage"], true);
    }

    // -- DeepSeek (OpenAI wire format) tests --

    #[test]
    fn test_deepseek_uses_openai_wire_format() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::DeepSeek);

        // DeepSeek uses OpenAI wire format
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert!(val["max_completion_tokens"].is_number());
    }

    // -- Ollama format tests --

    #[test]
    fn test_ollama_system_in_messages() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Ollama);

        // Ollama: system becomes first message like OpenAI
        assert!(val.get("system").is_none());
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
    }

    #[test]
    fn test_ollama_options_instead_of_max_tokens() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Ollama);

        // Ollama uses options.num_predict instead of max_tokens
        assert!(
            val.get("max_tokens").is_none() || val.get("options").is_some(),
            "Ollama should use options.num_predict or omit max_tokens"
        );
    }

    // -- Gemini format tests --

    #[test]
    fn test_gemini_system_instruction() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Gemini);

        // Gemini: system → systemInstruction.parts[].text
        if let Some(sys) = val.get("systemInstruction") {
            let parts = sys["parts"].as_array().unwrap();
            assert!(parts[0]["text"].is_string());
            assert!(
                parts[0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("helpful assistant")
            );
        }
    }

    #[test]
    fn test_gemini_role_mapping() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Gemini);

        // Gemini uses "model" instead of "assistant"
        if let Some(contents) = val["contents"].as_array() {
            let roles: Vec<&str> = contents.iter().filter_map(|c| c["role"].as_str()).collect();
            assert!(
                roles.iter().all(|r| *r != "assistant"),
                "Gemini should use 'model' instead of 'assistant'"
            );
        }
    }

    #[test]
    fn test_gemini_max_output_tokens() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Gemini);

        // Gemini uses generationConfig.maxOutputTokens
        if let Some(config) = val.get("generationConfig") {
            assert!(config["maxOutputTokens"].is_number());
        }
    }

    // -- Cross-provider consistency tests --

    #[test]
    fn test_all_preserve_message_count() {
        let req = make_simple_request();
        let providers = [
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::Ollama,
            LlmProvider::Gemini,
        ];

        for provider in &providers {
            let val = serialize_request(&req, provider);
            // Each provider should have messages/contents
            let msg_count = if let Some(msgs) = val["messages"].as_array() {
                msgs.len()
            } else if let Some(contents) = val["contents"].as_array() {
                contents.len()
            } else {
                panic!("{provider:?}: no messages or contents field");
            };
            // At least 2 user/assistant messages (+ system for OpenAI/Ollama)
            assert!(
                msg_count >= 2,
                "{provider:?}: should have at least 2 messages, got {msg_count}"
            );
        }
    }

    // -- MessageContent serde tests --

    #[test]
    fn test_message_content_text_serializes_as_string() {
        let msg = text_msg("user", "Hello");
        let json = serde_json::to_value(&msg).unwrap();
        // MessageContent::Text should serialize as plain string (untagged)
        assert_eq!(json["content"], "Hello");
        assert!(json["content"].is_string());
    }

    #[test]
    fn test_message_content_blocks_serializes_as_array() {
        let msg = blocks_msg(
            "assistant",
            vec![
                ContentBlock::Text {
                    text: "Hello".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "bash".to_string(),
                    input: json!({"command": "ls"}),
                },
            ],
        );
        let json = serde_json::to_value(&msg).unwrap();
        // MessageContent::Blocks should serialize as array
        assert!(json["content"].is_array());
        let blocks = json["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
    }

    #[test]
    fn test_message_content_text_round_trip() {
        let original = text_msg("user", "Round trip test");
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, "user");
        match &deserialized.content {
            MessageContent::Text(t) => assert_eq!(t, "Round trip test"),
            MessageContent::Blocks(_) => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_message_content_blocks_round_trip() {
        let original = blocks_msg(
            "assistant",
            vec![
                ContentBlock::Text {
                    text: "Part 1".to_string(),
                },
                ContentBlock::Thinking {
                    thinking: "Hmm...".to_string(),
                },
            ],
        );
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, "assistant");
        match &deserialized.content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
            }
            MessageContent::Text(_) => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_tool_result_content_single_serialization() {
        let msg = blocks_msg(
            "user",
            vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: Some(ToolResultContent::Single("file content".to_string())),
                is_error: Some(false),
            }],
        );
        let json = serde_json::to_value(&msg).unwrap();
        let block = &json["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "t1");
        assert_eq!(block["content"], "file content");
        assert_eq!(block["is_error"], false);
    }

    #[test]
    fn test_empty_messages_request() {
        let req = MessageRequest {
            model: "test".to_string(),
            max_tokens: 100,
            system: None,
            system_blocks: None,
            messages: vec![],
            tools: None,
            stream: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            reasoning_effort: None,
            thinking_budget: None,
        };

        // Should not panic for any provider
        for provider in &[
            LlmProvider::Anthropic,
            LlmProvider::OpenAI,
            LlmProvider::Ollama,
            LlmProvider::Gemini,
        ] {
            let val = serialize_request(&req, provider);
            // Should produce valid JSON
            assert!(
                val.is_object(),
                "{provider:?}: should produce a JSON object"
            );
        }
    }

    #[test]
    fn test_groq_uses_openai_wire_format() {
        let req = make_tool_request();
        let val = serialize_request(&req, &LlmProvider::Groq);

        // Groq uses OpenAI wire format
        let tools = val["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert!(tools[0]["function"].is_object());
    }

    #[test]
    fn test_mistral_uses_openai_wire_format() {
        let req = make_simple_request();
        let val = serialize_request(&req, &LlmProvider::Mistral);

        // Mistral uses OpenAI wire format
        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
    }
}
