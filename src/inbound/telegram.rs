//! Telegram getUpdates long-polling worker.
//!
//! Loop: GET getUpdates with the last offset + a 30s timeout → process each
//! message → bump offset → repeat. Shutdown is cooperative via a watch
//! channel; the long-poll timeout is what bounds shutdown latency (≤30s).
//!
//! Only text messages trigger an emit. Channel posts and edits are ignored
//! for now — they're noise for the chat-trigger use case.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tauri::AppHandle;
use tokio::sync::watch;

use super::{InboundMessage, emit_message, matches_trigger};

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub trigger_word: String,
    pub allowed_chats: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TgResponse<T> {
    ok: bool,
    result: Vec<T>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Debug, Deserialize)]
struct TgMessage {
    #[serde(rename = "chat")]
    chat: TgChat,
    from: Option<TgUser>,
    date: i64,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgChat {
    id: i64,
    title: Option<String>,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgUser {
    id: i64,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
}

/// Build the API URL for a given method. Exposed so tests can verify shape.
pub(crate) fn api_url(bot_token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{bot_token}/{method}")
}

pub async fn run(app: AppHandle, cfg: TelegramConfig, shutdown: watch::Receiver<bool>) {
    if cfg.bot_token.trim().is_empty() {
        tracing::warn!("telegram listener: empty bot token, not starting");
        return;
    }

    // Short long-poll (5s) bounds shutdown latency — combined with the
    // watch check at the top of each iteration, the worker exits within
    // ~5s of the supervisor signalling stop.
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client builds with sensible defaults");
    let mut offset: i64 = 0;
    let allowed: std::collections::HashSet<i64> = cfg
        .allowed_chats
        .iter()
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect();

    tracing::info!(
        "telegram listener: started ({} allowed chats)",
        allowed.len()
    );

    loop {
        if *shutdown.borrow() {
            break;
        }

        let url = api_url(&cfg.bot_token, "getUpdates");
        let response = client
            .get(&url)
            .query(&[("offset", offset.to_string()), ("timeout", "5".to_string())])
            .send()
            .await;

        let resp = match response {
            Ok(r) => r,
            Err(e) => {
                if *shutdown.borrow() {
                    break;
                }
                tracing::warn!(error = %e, "telegram listener: getUpdates failed, sleeping 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let parsed: TgResponse<TgUpdate> = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "telegram listener: decode failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        if !parsed.ok {
            tracing::warn!(desc = ?parsed.description, "telegram listener: non-OK response");
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        for update in parsed.result {
            offset = update.update_id + 1;
            let Some(msg) = update.message else { continue };
            let Some(text) = msg.text else { continue };

            if !allowed.is_empty() && !allowed.contains(&msg.chat.id) {
                continue;
            }
            if !matches_trigger(&text, &cfg.trigger_word) {
                continue;
            }

            let sender_name = format_tg_user_name(&msg.from);
            let chat_name = format_tg_chat_name(&msg.chat);
            let stripped = strip_trigger(&text, &cfg.trigger_word);

            emit_message(
                &app,
                InboundMessage {
                    provider: "telegram".into(),
                    source_id: msg.chat.id.to_string(),
                    source_name: chat_name,
                    sender_id: msg.from.map(|u| u.id.to_string()).unwrap_or_default(),
                    sender_name,
                    text: stripped,
                    timestamp: msg.date,
                },
            );
        }
    }

    tracing::info!("telegram listener: stopped");
}

fn format_tg_user_name(user: &Option<TgUser>) -> String {
    let Some(u) = user else {
        return "unknown".into();
    };
    if let Some(un) = &u.username {
        format!("@{un}")
    } else if let (Some(first), Some(last)) = (&u.first_name, &u.last_name) {
        format!("{first} {last}")
    } else if let Some(first) = &u.first_name {
        first.clone()
    } else {
        u.id.to_string()
    }
}

fn format_tg_chat_name(chat: &TgChat) -> String {
    if let Some(t) = &chat.title {
        return t.clone();
    }
    if let Some(un) = &chat.username {
        return format!("@{un}");
    }
    if let Some(first) = &chat.first_name {
        if let Some(last) = &chat.last_name {
            return format!("{first} {last}");
        }
        return first.clone();
    }
    chat.id.to_string()
}

/// Strip the trigger prefix from the message body. If the trigger isn't
/// found (case-insensitive trim), returns the text unchanged.
///
/// UTF-8 safe: walks `char_indices` rather than slicing by byte length, so
/// multibyte triggers (CJK, emoji) do not panic.
fn strip_trigger(text: &str, trigger: &str) -> String {
    let trig = trigger.trim();
    if trig.is_empty() {
        return text.trim().to_string();
    }
    if starts_with_ignore_case(text, trig) {
        let prefix_chars = trig.chars().count();
        let split_at = text
            .char_indices()
            .nth(prefix_chars)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(text.len());
        text[split_at..].trim_start().to_string()
    } else {
        text.trim().to_string()
    }
}

/// Case-insensitive prefix check that is UTF-8 safe. Compares character by
/// character so multibyte triggers (CJK, emoji, etc.) work correctly.
fn starts_with_ignore_case(haystack: &str, needle: &str) -> bool {
    let mut h = haystack.chars();
    let mut n = needle.chars();
    loop {
        match (n.next(), h.next()) {
            (None, _) => return true,
            (Some(nc), Some(hc)) if nc.eq_ignore_ascii_case(&hc) => continue,
            (Some(_), _) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_match_is_case_insensitive_and_trimmed() {
        assert!(matches_trigger("Shannon do the thing", "shannon"));
        assert!(matches_trigger("  Shannon go", "  shannon  "));
        assert!(!matches_trigger("hello world", "shannon"));
    }

    #[test]
    fn empty_trigger_matches_anything() {
        assert!(matches_trigger("anything", ""));
        assert!(matches_trigger("", ""));
    }

    #[test]
    fn strip_trigger_removes_only_prefix() {
        assert_eq!(
            strip_trigger("Shannon do the thing", "shannon"),
            "do the thing"
        );
        assert_eq!(
            strip_trigger("shannon: run tests", "Shannon"),
            ": run tests"
        );
        assert_eq!(strip_trigger("not prefixed", "shannon"), "not prefixed");
        assert_eq!(strip_trigger("Shannon", "shannon"), "");
    }

    #[test]
    fn strip_trigger_multibyte_chinese_does_not_panic() {
        // Regression for audit #9: byte-slice `text[..trig_lower.len()]` would
        // panic on multibyte triggers. `帮我` is 6 bytes / 2 chars.
        let trigger = "帮我";
        let text = "帮我写个测试 please";
        assert_eq!(strip_trigger(text, trigger), "写个测试 please");
    }

    #[test]
    fn strip_trigger_multibyte_emoji() {
        let trigger = "🚀";
        let text = "🚀 launch it";
        assert_eq!(strip_trigger(text, trigger), "launch it");
    }

    #[test]
    fn strip_trigger_multibyte_no_match_returns_original() {
        assert_eq!(strip_trigger("hello world", "帮我"), "hello world");
    }

    #[test]
    fn empty_trigger_returns_trimmed_text() {
        assert_eq!(strip_trigger("  hello  ", ""), "hello");
    }

    #[test]
    fn api_url_shape_is_canonical() {
        assert_eq!(
            api_url("123:ABC", "getUpdates"),
            "https://api.telegram.org/bot123:ABC/getUpdates"
        );
    }
}
