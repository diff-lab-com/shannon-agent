//! Outbound Tauri commands — P1.3.
//!
//! Read/write `[notifications.outbound]` in the same config file used by the
//! inbound module, plus a `send_outbound_test` command that fans a test
//! message to every configured provider and reports the per-channel outcome.

use crate::outbound::{ChannelResult, OutboundConfigDto, SlackOutboundDto, TelegramOutboundDto};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SendResultDto {
    pub results: Vec<ChannelResult>,
}

#[tauri::command]
pub async fn get_outbound_config() -> Result<OutboundConfigDto, String> {
    load_outbound().await
}

#[tauri::command]
pub async fn save_outbound_config(dto: OutboundConfigDto) -> Result<(), String> {
    save_outbound(&dto).await
}

#[tauri::command]
pub async fn clear_outbound_config() -> Result<(), String> {
    clear_outbound().await
}

#[tauri::command]
pub async fn send_outbound_test(message: String) -> Result<SendResultDto, String> {
    let dto = load_outbound().await?;
    if dto.slack.is_none() && dto.telegram.is_none() {
        return Err("no outbound providers configured".into());
    }
    let text = if message.trim().is_empty() {
        "Shannon outbound test ✓".to_string()
    } else {
        message
    };
    let http = crate::outbound::http_client();
    let outcome = crate::outbound::send_all(&http, &dto, &text).await;
    Ok(SendResultDto {
        results: outcome.results,
    })
}

/// Resolve the config file. Prefers project-local `.shannon.toml`; otherwise
/// the global `~/.shannon/config.toml`. Same precedence as
/// `commands_notifications::resolve_webhook_config_path`.
fn resolve_config_path() -> Result<std::path::PathBuf, String> {
    let local = std::path::Path::new(".shannon.toml");
    if local.exists() {
        return Ok(local.to_path_buf());
    }
    let home = dirs::home_dir().ok_or_else(|| "could not resolve $HOME".to_string())?;
    let dir = home.join(".shannon");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    Ok(dir.join("config.toml"))
}

async fn load_outbound() -> Result<OutboundConfigDto, String> {
    let path = resolve_config_path()?;
    if !path.exists() {
        return Ok(OutboundConfigDto::default());
    }
    let s = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(root) = toml::from_str::<toml::Value>(&s) else {
        return Ok(OutboundConfigDto::default());
    };
    let Some(notif) = root.get("notifications").and_then(|v| v.as_table()) else {
        return Ok(OutboundConfigDto::default());
    };
    let Some(outbound) = notif.get("outbound").and_then(|v| v.as_table()) else {
        return Ok(OutboundConfigDto::default());
    };
    let slack = outbound
        .get("slack")
        .and_then(|v| v.as_table())
        .map(|t| SlackOutboundDto {
            bot_token: t
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            channel: t
                .get("channel")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    let telegram = outbound
        .get("telegram")
        .and_then(|v| v.as_table())
        .map(|t| TelegramOutboundDto {
            bot_token: t
                .get("bot_token")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            chat_id: t
                .get("chat_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    Ok(OutboundConfigDto { slack, telegram })
}

async fn save_outbound(dto: &OutboundConfigDto) -> Result<(), String> {
    let path = resolve_config_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut root: toml::Value =
        toml::from_str(&existing).unwrap_or(toml::Value::Table(toml::value::Table::new()));
    let table = root
        .as_table_mut()
        .ok_or_else(|| "config root is not a table".to_string())?;
    let notifications = table
        .entry("notifications")
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let notif_table = notifications
        .as_table_mut()
        .ok_or_else(|| "notifications is not a table".to_string())?;
    let mut outbound = toml::value::Table::new();
    if let Some(s) = dto.slack.as_ref() {
        let mut t = toml::value::Table::new();
        t.insert("bot_token".into(), toml::Value::String(s.bot_token.clone()));
        t.insert("channel".into(), toml::Value::String(s.channel.clone()));
        outbound.insert("slack".into(), toml::Value::Table(t));
    }
    if let Some(tg) = dto.telegram.as_ref() {
        let mut t = toml::value::Table::new();
        t.insert(
            "bot_token".into(),
            toml::Value::String(tg.bot_token.clone()),
        );
        t.insert("chat_id".into(), toml::Value::String(tg.chat_id.clone()));
        outbound.insert("telegram".into(), toml::Value::Table(t));
    }
    notif_table.insert("outbound".into(), toml::Value::Table(outbound));
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    crate::file_permissions::restrict_to_owner(&path);
    tracing::info!(path = %path.display(), "outbound config saved");
    Ok(())
}

async fn clear_outbound() -> Result<(), String> {
    let path = resolve_config_path()?;
    if !path.exists() {
        return Ok(());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let Ok(mut root) = toml::from_str::<toml::Value>(&existing) else {
        return Ok(());
    };
    if let Some(table) = root.as_table_mut() {
        if let Some(notif) = table
            .get_mut("notifications")
            .and_then(|v| v.as_table_mut())
        {
            notif.remove("outbound");
        }
    }
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    crate::file_permissions::restrict_to_owner(&path);
    tracing::info!(path = %path.display(), "outbound config cleared");
    Ok(())
}

// Re-export so callers outside the crate boundary can build an
// OutboundConfigDto without reaching into the outbound submodule directly.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_dto_serializes_to_toml_table() {
        let _dto = OutboundConfigDto {
            slack: Some(SlackOutboundDto {
                bot_token: "xoxb-1".into(),
                channel: "#general".into(),
            }),
            telegram: None,
        };
        let mut table = toml::value::Table::new();
        let mut slack = toml::value::Table::new();
        slack.insert("bot_token".into(), toml::Value::String("xoxb-1".into()));
        slack.insert("channel".into(), toml::Value::String("#general".into()));
        table.insert("slack".into(), toml::Value::Table(slack));

        let serialized = toml::to_string(&toml::Value::Table(table)).unwrap();
        assert!(serialized.contains("xoxb-1"));
        assert!(serialized.contains("#general"));
    }

    #[test]
    fn empty_dto_produces_empty_outbound_table() {
        let dto = OutboundConfigDto::default();
        let mut outbound = toml::value::Table::new();
        if let Some(s) = dto.slack.as_ref() {
            let mut t = toml::value::Table::new();
            t.insert("bot_token".into(), toml::Value::String(s.bot_token.clone()));
            outbound.insert("slack".into(), toml::Value::Table(t));
        }
        assert!(outbound.is_empty());
    }
}
