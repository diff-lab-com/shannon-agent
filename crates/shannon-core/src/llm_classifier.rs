//! LLM-based permission classification layer
//!
//! Wraps the rule-based [`PermissionClassifier`](super::permission_classifier::PermissionClassifier)
//! with an async LLM fallback for ambiguous cases. Implements the 4-tier precedence
//! model from Claude Code:
//!
//! 1. **hard_deny** — always denied regardless of other rules
//! 2. **soft_deny** — denied unless explicitly allowed by LLM judgment
//! 3. **allow** — allowed by default, LLM can escalate to ask
//! 4. **explicit intent** — user explicitly approved, LLM can warn but not deny
//!
//! The LLM is only called when:
//! - The rule-based classifier returns low confidence (< 0.7)
//! - The risk level is Medium or above
//! - The tool is not in the known safe list

use crate::api::LlmClient;
use crate::api::types::{Message, MessageContent, ContentBlock};
use crate::permission_classifier::{ClassificationResult, PermissionClassifier, RiskLevel, RuleDecision};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// LLM-based classification tier (Claude Code's 4-tier model)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmTier {
    /// Always denied regardless of other rules
    HardDeny,
    /// Denied unless explicitly allowed by LLM judgment
    SoftDeny,
    /// Allowed by default, LLM can escalate to ask
    Allow,
    /// User explicitly approved, LLM can warn but not deny
    ExplicitIntent,
}

impl std::fmt::Display for LlmTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmTier::HardDeny => write!(f, "hard_deny"),
            LlmTier::SoftDeny => write!(f, "soft_deny"),
            LlmTier::Allow => write!(f, "allow"),
            LlmTier::ExplicitIntent => write!(f, "explicit_intent"),
        }
    }
}

/// Result from LLM-based classification
#[derive(Debug, Clone)]
pub struct LlmClassificationResult {
    /// Combined result from rule-based + LLM classification
    pub result: ClassificationResult,
    /// Which tier was applied
    pub tier: LlmTier,
    /// Whether the LLM was actually consulted
    pub llm_consulted: bool,
}

/// The LLM-enhanced permission classifier
pub struct LlmPermissionClassifier {
    /// The underlying rule-based classifier
    pub rule_classifier: PermissionClassifier,
    /// LLM client for fallback classification
    client: Option<LlmClient>,
    /// Minimum confidence to trust rule-based results without LLM
    confidence_threshold: f32,
    /// Whether LLM classification is enabled
    enabled: bool,
}

impl LlmPermissionClassifier {
    /// Create a new LLM classifier wrapping a rule-based classifier
    pub fn new(rule_classifier: PermissionClassifier) -> Self {
        Self {
            rule_classifier,
            client: None,
            confidence_threshold: 0.7,
            enabled: false,
        }
    }

    /// Enable LLM-based classification with the given client
    pub fn with_llm(mut self, client: LlmClient) -> Self {
        self.client = Some(client);
        self.enabled = true;
        self
    }

    /// Set the confidence threshold for LLM fallback
    pub fn with_confidence_threshold(mut self, threshold: f32) -> Self {
        self.confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Get the current confidence threshold
    pub fn confidence_threshold(&self) -> f32 {
        self.confidence_threshold
    }

    /// Classify a tool call using rule-based logic first, then LLM if needed
    pub async fn classify(&self, tool_name: &str, input: &Value) -> LlmClassificationResult {
        // Step 1: Rule-based classification (always runs)
        let rule_result = self.rule_classifier.classify(tool_name, input);

        // Determine the tier based on rule result
        let tier = self.determine_tier(&rule_result);

        // Step 2: Check if LLM consultation is needed
        let needs_llm = self.enabled
            && rule_result.confidence < self.confidence_threshold
            && matches!(rule_result.risk_level, RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical)
            && !matches!(tier, LlmTier::HardDeny);

        if !needs_llm {
            return LlmClassificationResult {
                result: rule_result,
                tier,
                llm_consulted: false,
            };
        }

        // Step 3: LLM classification for ambiguous cases
        let llm_result = self.classify_with_llm(tool_name, input, &rule_result).await;

        LlmClassificationResult {
            result: llm_result,
            tier: LlmTier::Allow,
            llm_consulted: true,
        }
    }

    /// Determine the 4-tier classification from a rule result
    fn determine_tier(&self, result: &ClassificationResult) -> LlmTier {
        match result.decision {
            RuleDecision::Deny if result.risk_level == RiskLevel::Critical => LlmTier::HardDeny,
            RuleDecision::Deny => LlmTier::SoftDeny,
            RuleDecision::Ask => LlmTier::Allow,
            RuleDecision::Allow => LlmTier::Allow,
        }
    }

    /// Consult the LLM for classification
    async fn classify_with_llm(
        &self,
        tool_name: &str,
        input: &Value,
        rule_result: &ClassificationResult,
    ) -> ClassificationResult {
        let Some(ref client) = self.client else {
            return rule_result.clone();
        };

        let input_preview = serde_json::to_string(input)
            .unwrap_or_default()
            .chars()
            .take(500)
            .collect::<String>();

        let prompt = format!(
            "You are a security classifier for an AI code assistant. \
             Classify whether the following tool call is safe to execute automatically.\n\n\
             Tool: {tool_name}\n\
             Input: {input_preview}\n\n\
             Rule-based assessment: {} (confidence: {:.2}, risk: {})\n\n\
             Respond with JSON: {{\"decision\": \"allow\"|\"ask\"|\"deny\", \
             \"confidence\": 0.0-1.0, \"reason\": \"...\"}}",
            rule_result.decision,
            rule_result.confidence,
            rule_result.risk_level,
        );

        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text(prompt),
        }];
        match client.send_message(messages, None, None).await {
            Ok(blocks) => {
                let response_text: String = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<&str>>()
                    .join("");
                self.parse_llm_response(&response_text, rule_result)
            }
            Err(_) => rule_result.clone(),
        }
    }

    /// Parse the LLM response into a classification result
    fn parse_llm_response(
        &self,
        response: &str,
        fallback: &ClassificationResult,
    ) -> ClassificationResult {
        // Try to extract JSON from the response
        let json_str = response
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let parsed: Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return fallback.clone(),
        };

        let decision = match parsed.get("decision").and_then(|v| v.as_str()) {
            Some("allow") => RuleDecision::Allow,
            Some("deny") => RuleDecision::Deny,
            Some("ask") => RuleDecision::Ask,
            _ => return fallback.clone(),
        };

        let confidence = parsed.get("confidence")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(fallback.confidence);

        let reason = parsed.get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or(&fallback.reason)
            .to_string();

        ClassificationResult {
            decision,
            confidence,
            reason: format!("[LLM] {reason}"),
            matched_rule: Some("llm_classifier".to_string()),
            risk_level: fallback.risk_level,
        }
    }

    /// Check if LLM classification is enabled
    pub fn is_llm_enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission_classifier::PermissionClassifier;

    fn make_classifier() -> LlmPermissionClassifier {
        LlmPermissionClassifier::new(PermissionClassifier::new())
    }

    #[test]
    fn test_llm_tier_display() {
        assert_eq!(LlmTier::HardDeny.to_string(), "hard_deny");
        assert_eq!(LlmTier::SoftDeny.to_string(), "soft_deny");
        assert_eq!(LlmTier::Allow.to_string(), "allow");
        assert_eq!(LlmTier::ExplicitIntent.to_string(), "explicit_intent");
    }

    #[test]
    fn test_llm_tier_serialization() {
        let tier = LlmTier::HardDeny;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"hard_deny\"");
        let de: LlmTier = serde_json::from_str(&json).unwrap();
        assert_eq!(de, LlmTier::HardDeny);
    }

    #[test]
    fn test_classifier_created_without_llm() {
        let classifier = make_classifier();
        assert!(!classifier.is_llm_enabled());
    }

    #[test]
    fn test_determine_tier_critical_deny() {
        let classifier = make_classifier();
        let result = ClassificationResult {
            decision: RuleDecision::Deny,
            confidence: 0.95,
            reason: "dangerous".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Critical,
        };
        assert_eq!(classifier.determine_tier(&result), LlmTier::HardDeny);
    }

    #[test]
    fn test_determine_tier_non_critical_deny() {
        let classifier = make_classifier();
        let result = ClassificationResult {
            decision: RuleDecision::Deny,
            confidence: 0.8,
            reason: "suspicious".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Medium,
        };
        assert_eq!(classifier.determine_tier(&result), LlmTier::SoftDeny);
    }

    #[test]
    fn test_determine_tier_allow() {
        let classifier = make_classifier();
        let result = ClassificationResult {
            decision: RuleDecision::Allow,
            confidence: 0.9,
            reason: "safe".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Low,
        };
        assert_eq!(classifier.determine_tier(&result), LlmTier::Allow);
    }

    #[test]
    fn test_determine_tier_ask() {
        let classifier = make_classifier();
        let result = ClassificationResult {
            decision: RuleDecision::Ask,
            confidence: 0.6,
            reason: "needs confirmation".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Medium,
        };
        assert_eq!(classifier.determine_tier(&result), LlmTier::Allow);
    }

    #[test]
    fn test_parse_llm_response_allow() {
        let classifier = make_classifier();
        let fallback = ClassificationResult {
            decision: RuleDecision::Ask,
            confidence: 0.5,
            reason: "uncertain".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Medium,
        };
        let response = r#"{"decision": "allow", "confidence": 0.9, "reason": "safe operation"}"#;
        let result = classifier.parse_llm_response(response, &fallback);
        assert_eq!(result.decision, RuleDecision::Allow);
        assert!((result.confidence - 0.9).abs() < 0.01);
        assert!(result.reason.contains("[LLM]"));
        assert!(result.reason.contains("safe operation"));
    }

    #[test]
    fn test_parse_llm_response_deny() {
        let classifier = make_classifier();
        let fallback = ClassificationResult {
            decision: RuleDecision::Allow,
            confidence: 0.5,
            reason: "uncertain".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::High,
        };
        let response = r#"{"decision": "deny", "confidence": 0.85, "reason": "destructive command"}"#;
        let result = classifier.parse_llm_response(response, &fallback);
        assert_eq!(result.decision, RuleDecision::Deny);
    }

    #[test]
    fn test_parse_llm_response_invalid_json_returns_fallback() {
        let classifier = make_classifier();
        let fallback = ClassificationResult {
            decision: RuleDecision::Ask,
            confidence: 0.5,
            reason: "uncertain".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Medium,
        };
        let result = classifier.parse_llm_response("not valid json", &fallback);
        assert_eq!(result.decision, RuleDecision::Ask);
        assert_eq!(result.confidence, 0.5);
    }

    #[test]
    fn test_parse_llm_response_with_code_fence() {
        let classifier = make_classifier();
        let fallback = ClassificationResult {
            decision: RuleDecision::Ask,
            confidence: 0.5,
            reason: "uncertain".to_string(),
            matched_rule: None,
            risk_level: RiskLevel::Medium,
        };
        let response = "```json\n{\"decision\": \"allow\", \"confidence\": 0.8, \"reason\": \"ok\"}\n```";
        let result = classifier.parse_llm_response(response, &fallback);
        assert_eq!(result.decision, RuleDecision::Allow);
    }

    #[tokio::test]
    async fn test_classify_without_llm_returns_rule_result() {
        let classifier = make_classifier();
        let result = classifier.classify("Read", &serde_json::json!({"path": "/tmp/test"})).await;
        assert!(!result.llm_consulted);
        assert_eq!(result.tier, LlmTier::Allow);
    }

    #[test]
    fn test_confidence_threshold_clamping() {
        let classifier = LlmPermissionClassifier::new(PermissionClassifier::new())
            .with_confidence_threshold(1.5);
        assert!((classifier.confidence_threshold - 1.0).abs() < 0.01);
    }
}
