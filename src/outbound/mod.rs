//! Outbound messaging — P1.3.
//!
//! Sends a Shannon message (e.g. task completion, manual test) to one or more
//! configured providers. The inbound module covers receiving messages from
//! Slack/Telegram; outbound closes the loop so the assistant can post a reply
//! or push a notification back to the same channels.
//!
//! The dispatcher is intentionally stateless — every call reads the current
//! config from disk so changes take effect immediately without a restart.
//! Errors from one provider are returned but never short-circuit delivery to
//! the others (best-effort fan-out).

mod slack;
mod telegram;

use serde::{Deserialize, Serialize};

/// Provider-side config + recipient for Slack.
///
/// `channel` may be a channel id (`C…`), a user id (`U…`), or a channel name
/// (`#general`); the Slack `chat.postMessage` endpoint accepts all three.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlackOutboundDto {
    /// `xoxb-…` bot token. Required.
    pub bot_token: String,
    /// Channel id, user id, or `#name`.
    pub channel: String,
}

/// Provider-side config + recipient for Telegram.
///
/// `chat_id` may be numeric (`-1001234567890` for a supergroup) or a `@username`
/// for public channels where the bot is admin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelegramOutboundDto {
    /// `1234567890:ABC…` bot token from BotFather. Required.
    pub bot_token: String,
    /// Numeric chat id or `@channelusername`.
    pub chat_id: String,
}

/// All configured outbound destinations. Mirrors `InboundConfigDto`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutboundConfigDto {
    pub slack: Option<SlackOutboundDto>,
    pub telegram: Option<TelegramOutboundDto>,
}

/// Result of a single send attempt. Aggregated into a `SendOutcome` so the UI
/// can surface per-channel failures.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelResult {
    pub provider: String, // "slack" | "telegram"
    pub ok: bool,
    pub error: Option<String>,
}

/// Aggregated result of a fan-out send.
#[derive(Debug, Clone, Serialize)]
pub struct SendOutcome {
    pub results: Vec<ChannelResult>,
}

impl SendOutcome {
    pub fn any_ok(&self) -> bool {
        self.results.iter().any(|r| r.ok)
    }
}

/// Fan a message out to every configured provider in `dto`. Providers that
/// fail (network error, 4xx, etc.) are recorded but do not abort the loop.
pub async fn send_all(
    http: &reqwest::Client,
    dto: &OutboundConfigDto,
    text: &str,
) -> SendOutcome {
    let mut results = Vec::new();
    if let Some(s) = dto.slack.as_ref() {
        let res = slack::send(http, s, text).await;
        results.push(ChannelResult {
            provider: "slack".into(),
            ok: res.is_ok(),
            error: res.err(),
        });
    }
    if let Some(t) = dto.telegram.as_ref() {
        let res = telegram::send(http, t, text).await;
        results.push(ChannelResult {
            provider: "telegram".into(),
            ok: res.is_ok(),
            error: res.err(),
        });
    }
    SendOutcome { results }
}

/// Build the shared HTTP client with sane defaults.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("reqwest client with default TLS stack builds successfully")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_dto_roundtrip_preserves_fields() {
        let dto = OutboundConfigDto {
            slack: Some(SlackOutboundDto {
                bot_token: "xoxb-test".into(),
                channel: "#general".into(),
            }),
            telegram: Some(TelegramOutboundDto {
                bot_token: "123:abc".into(),
                chat_id: "@mychannel".into(),
            }),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let back: OutboundConfigDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slack.as_ref().unwrap().bot_token, "xoxb-test");
        assert_eq!(back.slack.as_ref().unwrap().channel, "#general");
        assert_eq!(back.telegram.as_ref().unwrap().chat_id, "@mychannel");
    }

    #[test]
    fn empty_outbound_dto_defaults_to_none() {
        let dto = OutboundConfigDto::default();
        assert!(dto.slack.is_none());
        assert!(dto.telegram.is_none());
    }

    #[test]
    fn send_outcome_any_ok_reflects_per_channel_state() {
        let mixed = SendOutcome {
            results: vec![
                ChannelResult {
                    provider: "slack".into(),
                    ok: true,
                    error: None,
                },
                ChannelResult {
                    provider: "telegram".into(),
                    ok: false,
                    error: Some("network".into()),
                },
            ],
        };
        assert!(mixed.any_ok());

        let all_bad = SendOutcome {
            results: vec![ChannelResult {
                provider: "slack".into(),
                ok: false,
                error: Some("rate limited".into()),
            }],
        };
        assert!(!all_bad.any_ok());
    }
}
