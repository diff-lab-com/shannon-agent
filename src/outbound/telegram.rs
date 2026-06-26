//! Telegram outbound — `sendMessage` via the Bot API.
//!
//! The bot must be an admin of any target channel (`@channelusername`); for
//! private chats the user must have started the conversation first.

use serde::Deserialize;

use super::TelegramOutboundDto;

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    description: Option<String>,
}

fn api_url(token: &str) -> String {
    format!("https://api.telegram.org/bot{token}/sendMessage")
}

pub async fn send(
    http: &reqwest::Client,
    cfg: &TelegramOutboundDto,
    text: &str,
) -> Result<(), String> {
    if cfg.bot_token.trim().is_empty() {
        return Err("missing bot_token".into());
    }
    if cfg.chat_id.trim().is_empty() {
        return Err("missing chat_id".into());
    }

    let resp = http
        .post(api_url(cfg.bot_token.trim()))
        .json(&serde_json::json!({
            "chat_id": cfg.chat_id.trim(),
            "text": text,
        }))
        .send()
        .await
        .map_err(|e| format!("telegram http: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "telegram status {status}: {}",
            truncate(&body, 200)
        ));
    }

    let parsed: TelegramResponse = resp
        .json()
        .await
        .map_err(|e| format!("telegram decode: {e}"))?;
    if !parsed.ok {
        return Err(parsed
            .description
            .unwrap_or_else(|| "unknown telegram error".into()));
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
        let cfg = TelegramOutboundDto {
            bot_token: "".into(),
            chat_id: "@channel".into(),
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
    fn empty_chat_errors() {
        let cfg = TelegramOutboundDto {
            bot_token: "123:abc".into(),
            chat_id: "".into(),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(send(&reqwest::Client::new(), &cfg, "hi"))
            .unwrap_err();
        assert!(err.contains("chat_id"));
    }

    #[test]
    fn api_url_contains_token_path() {
        let url = api_url("123:ABC");
        assert!(url.contains("/bot123:ABC/sendMessage"));
    }
}
