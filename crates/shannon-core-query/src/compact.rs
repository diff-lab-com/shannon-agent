//! Message compaction and optimization

use serde::{Deserialize, Serialize};
use shannon_types::Message;
use std::collections::HashMap;
use chrono::Timelike;

/// Strategy for message compaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactStrategy {
    /// Compact by semantic similarity
    Semantic,
    /// Compact by time windows
    TimeWindow,
    /// Compact by token count
    TokenCount,
    /// Compact by message type
    MessageType,
    /// No compaction
    None,
}

/// Group of related messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageGroup {
    pub id: String,
    pub messages: Vec<Message>,
    pub metadata: GroupMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMetadata {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub estimated_tokens: usize,
    pub tags: Vec<String>,
}

/// Message compaction engine
pub struct CompactEngine {
    strategy: CompactStrategy,
    max_tokens: usize,
}

impl CompactEngine {
    pub fn new(strategy: CompactStrategy, max_tokens: usize) -> Self {
        Self {
            strategy,
            max_tokens,
        }
    }

    pub fn compact(&self, messages: Vec<Message>) -> Result<Vec<Message>, CompactError> {
        // If token count is under threshold, no compaction needed
        let estimated_tokens = self.estimate_tokens(&messages);
        if estimated_tokens <= self.max_tokens {
            return Ok(messages);
        }

        match self.strategy {
            CompactStrategy::Semantic => self.compact_semantic(messages),
            CompactStrategy::TimeWindow => self.compact_time_window(messages),
            CompactStrategy::TokenCount => self.compact_token_count(messages),
            CompactStrategy::MessageType => self.compact_message_type(messages),
            CompactStrategy::None => Ok(messages),
        }
    }

    fn estimate_tokens(&self, messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|m| {
                // Rough estimation: ~4 chars per token
                (m.content.len() / 4) + m.role.as_str().len() / 4
            })
            .sum()
    }

    fn compact_semantic(&self, messages: Vec<Message>) -> Result<Vec<Message>, CompactError> {
        // Group messages by semantic similarity
        let groups = self.group_by_similarity(messages)?;

        // Create summary messages for each group
        let mut result = Vec::new();
        for group in groups {
            if group.messages.len() > 1 {
                let summary = self.summarize_group(&group)?;
                result.push(summary);
            } else {
                result.extend(group.messages);
            }
        }

        Ok(result)
    }

    fn compact_time_window(&self, messages: Vec<Message>) -> Result<Vec<Message>, CompactError> {
        // Group messages by time windows (e.g., 10-minute intervals)
        let mut groups: HashMap<String, Vec<Message>> = HashMap::new();

        for msg in messages {
            let window = self.get_time_window(&msg.timestamp);
            groups.entry(window).or_default().push(msg);
        }

        // Summarize each time window
        let mut result = Vec::new();
        for (_, group_messages) in groups {
            if group_messages.len() > 3 {
                let summary_text = format!(
                    "[{} messages from time window: {}]",
                    group_messages.len(),
                    group_messages.first().map(|m| m.timestamp.to_string()).unwrap_or_default()
                );
                result.push(Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: "system".to_string(),
                    content: summary_text,
                    timestamp: chrono::Utc::now(),
                    metadata: serde_json::Value::Null,
                });
            } else {
                result.extend(group_messages);
            }
        }

        Ok(result)
    }

    fn compact_token_count(&self, messages: Vec<Message>) -> Result<Vec<Message>, CompactError> {
        let mut result = Vec::new();
        let mut current_tokens = 0;
        let mut batch = Vec::new();

        for msg in messages {
            let msg_tokens = msg.content.len() / 4;

            if current_tokens + msg_tokens > self.max_tokens && !batch.is_empty() {
                // Summarize the batch and start a new one
                let summary = self.summarize_batch(&batch)?;
                result.push(summary);
                batch = vec![msg];
                current_tokens = msg_tokens;
            } else {
                batch.push(msg);
                current_tokens += msg_tokens;
            }
        }

        // Add remaining messages
        result.extend(batch);

        Ok(result)
    }

    fn compact_message_type(&self, messages: Vec<Message>) -> Result<Vec<Message>, CompactError> {
        // Group by role/message type
        let mut groups: HashMap<String, Vec<Message>> = HashMap::new();

        for msg in messages {
            groups.entry(msg.role.clone()).or_default().push(msg);
        }

        // Summarize groups with many messages
        let mut result = Vec::new();
        for (role, group_messages) in groups {
            if group_messages.len() > 5 {
                let summary_text = format!(
                    "[{} {} messages summarized]",
                    group_messages.len(),
                    role
                );
                result.push(Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: "system".to_string(),
                    content: summary_text,
                    timestamp: chrono::Utc::now(),
                    metadata: serde_json::Value::Null,
                });
            } else {
                result.extend(group_messages);
            }
        }

        Ok(result)
    }

    fn group_by_similarity(&self, messages: Vec<Message>) -> Result<Vec<MessageGroup>, CompactError> {
        // Simple grouping by content similarity keywords
        let mut groups: Vec<MessageGroup> = Vec::new();
        let mut current_group: Vec<Message> = Vec::new();

        for msg in messages {
            if current_group.is_empty() {
                current_group.push(msg);
            } else {
                let last = &current_group.last().unwrap().content;
                let similarity = self.calculate_similarity(last, &msg.content);

                if similarity > 0.5 {
                    current_group.push(msg);
                } else {
                    // Save current group and start new one
                    groups.push(MessageGroup {
                        id: uuid::Uuid::new_v4().to_string(),
                        messages: current_group.clone(),
                        metadata: GroupMetadata {
                            created_at: chrono::Utc::now(),
                            message_count: current_group.len(),
                            estimated_tokens: self.estimate_tokens(&current_group),
                            tags: vec![],
                        },
                    });
                    current_group = vec![msg];
                }
            }
        }

        // Add last group
        if !current_group.is_empty() {
            groups.push(MessageGroup {
                id: uuid::Uuid::new_v4().to_string(),
                messages: current_group,
                metadata: GroupMetadata {
                    created_at: chrono::Utc::now(),
                    message_count: 0,
                    estimated_tokens: 0,
                    tags: vec![],
                },
            });
        }

        Ok(groups)
    }

    fn calculate_similarity(&self, text1: &str, text2: &str) -> f64 {
        // Simple word overlap similarity
        let words1: std::collections::HashSet<&str> = text1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = text2.split_whitespace().collect();

        if words1.is_empty() || words2.is_empty() {
            return 0.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        intersection as f64 / union as f64
    }

    fn summarize_group(&self, group: &MessageGroup) -> Result<Message, CompactError> {
        let summary_text = format!(
            "[Group of {} messages summarized: {:?}]",
            group.messages.len(),
            group.metadata.tags
        );

        Ok(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: "system".to_string(),
            content: summary_text,
            timestamp: chrono::Utc::now(),
            metadata: serde_json::Value::Null,
        })
    }

    fn summarize_batch(&self, batch: &[Message]) -> Result<Message, CompactError> {
        Ok(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: "system".to_string(),
            content: format!("[{} messages compacted]", batch.len()),
            timestamp: chrono::Utc::now(),
            metadata: serde_json::Value::Null,
        })
    }

    fn get_time_window(&self, timestamp: &chrono::DateTime<chrono::Utc>) -> String {
        // 10-minute windows
        let minutes = timestamp.minute() / 10 * 10;
        format!("{}:{}-{}:{}", timestamp.hour(), minutes, timestamp.hour(), minutes + 10)
    }
}

/// Compaction errors
#[derive(Debug, thiserror::Error)]
pub enum CompactError {
    #[error("Failed to compact messages: {0}")]
    CompactFailed(String),

    #[error("Invalid message format: {0}")]
    InvalidMessage(String),
}
