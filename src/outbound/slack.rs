//! Slack outbound — `chat.postMessage` via the Web API.
//!
//! Uses `Bearer <bot_token>` auth; the bot must be invited to the target
//! channel. Messages are plain text (no block kit) — fine for the push-style
//! notifications Shannon sends. Long messages are truncated by Slack.

use serde::Deserialize;

use super::SlackOutboundDto;

#[derive(Debug, Deserialize)]
struct SlackResponse {
    ok: bool,
    error: Option<String>,
}

const POST_MESSAGE_URL: &str = "https://slack.com/api/chat.postMessage";

pub async fn send(
    http: &reqwest::Client,
    cfg: &SlackOutboundDto,
    text: &str,
) -> Result<(), String> {
    if cfg.bot_token.trim().is_empty() {
        return Err("missing bot_token".into());
    }
    if cfg.channel.trim().is_empty() {
        return Err("missing channel".into());
    }

    let resp = http
        .post(POST_MESSAGE_URL)
        .bearer_auth(cfg.bot_token.trim())
        .json(&serde_json::json!({
            "channel": cfg.channel.trim(),
            "text": text,
        }))
        .send()
        .await
        .map_err(|e| format!("slack http: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("slack status {status}: {}", truncate(&body, 200)));
    }

    let parsed: SlackResponse = resp
        .json()
        .await
        .map_err(|e| format!("slack decode: {e}"))?;
    if !parsed.ok {
        return Err(parsed.error.unwrap_or_else(|| "unknown slack error".into()));
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_token_errors() {
        let cfg = SlackOutboundDto {
            bot_token: "  ".into(),
            channel: "#general".into(),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(send(&reqwest::Client::new(), &cfg, "hi"))
            .unwrap_err();
        assert!(err.contains("bot_token"));
    }

    #[test]
    fn empty_channel_errors() {
        let cfg = SlackOutboundDto {
            bot_token: "xoxb-x".into(),
            channel: "".into(),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(send(&reqwest::Client::new(), &cfg, "hi"))
            .unwrap_err();
        assert!(err.contains("channel"));
    }

    #[test]
    fn truncate_handles_short_input() {
        assert_eq!(truncate("abc", 10), "abc");
        let long = "x".repeat(100);
        let t = truncate(&long, 5);
        assert_eq!(t.chars().count(), 6); // 5 chars + ellipsis
        assert!(t.ends_with('…'));
    }
}
