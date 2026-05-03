use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_notify(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    match trimmed {
        "on" | "enable" | "true" | "yes" => {
            repl.notifications_enabled = true;
            repl.chat.add_message(
                ChatRole::System,
                "Desktop notifications enabled. You'll be notified when queries complete.".to_string(),
            );
        }
        "off" | "disable" | "false" | "no" => {
            repl.notifications_enabled = false;
            repl.chat.add_message(
                ChatRole::System,
                "Desktop notifications disabled.".to_string(),
            );
        }
        "test" => {
            repl.notifier.info("Shannon", "Test notification!").ok();
            repl.chat.add_message(
                ChatRole::System,
                "Test notification sent.".to_string(),
            );
        }
        _ => {
            let status = if repl.notifications_enabled { "enabled" } else { "disabled" };
            repl.chat.add_message(
                ChatRole::System,
                format!(
                    "Desktop notifications: {status}\n\n\
                     Usage:\n  /notify on  — enable notifications\n  \
                     /notify off — disable notifications\n  \
                     /notify test — send a test notification"
                ),
            );
        }
    }
    Ok(())
}

/// Handle `/webhook` command — manage webhook receiver for external event injection.
pub(crate) fn handle_webhook(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    match trimmed {
        "start" | "on" => {
            match repl.webhook_receiver {
                Some(ref rx) => {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Webhook receiver already running at {}", rx.address()),
                    );
                }
                None => {
                    let config = shannon_core::webhook::WebhookConfig::default();
                    let port = config.port;
                    let addr = format!("{}:{}", config.host, config.port);
                    let mut receiver = shannon_core::webhook::WebhookReceiver::new(config);

                    match receiver.try_start() {
                        Ok(()) => {
                            repl.chat.add_message(
                                ChatRole::System,
                                format!(
                                    "Webhook receiver started on {addr}\n\n\
                                     Endpoints:\n\
                                       POST http://{addr}/webhook/github  — GitHub webhooks\n\
                                       POST http://{addr}/webhook/generic — Generic webhooks\n\
                                       POST http://{addr}/webhook/health — Health check\n\n\
                                     Use /webhook status to check pending events.\n\
                                     Use /webhook stop to shut down."
                                ),
                            );
                            repl.webhook_receiver = Some(receiver);
                        }
                        Err(e) => {
                            super::set_error(repl, &format!("starting webhook receiver on port {port}: {e}"));
                        }
                    }
                }
            }
        }
        "stop" | "off" => {
            if let Some(mut rx) = repl.webhook_receiver.take() {
                rx.stop();
                repl.chat.add_message(
                    ChatRole::System,
                    "Webhook receiver stopped.".to_string(),
                );
            } else {
                repl.chat.add_message(
                    ChatRole::System,
                    "No webhook receiver is running.".to_string(),
                );
            }
        }
        "status" => {
            match repl.webhook_receiver {
                Some(ref rx) => {
                    repl.chat.add_message(
                        ChatRole::System,
                        format!("Webhook receiver active at {}", rx.address()),
                    );
                }
                None => {
                    repl.chat.add_message(
                        ChatRole::System,
                        "No webhook receiver running. Use /webhook start to begin.".to_string(),
                    );
                }
            }
        }
        "poll" => {
            if let Some(ref mut rx) = repl.webhook_receiver {
                let mut count = 0;
                while let Some(event) = rx.try_recv() {
                    count += 1;
                    let url_note = event.url
                        .map(|u| format!("\n  Link: {u}"))
                        .unwrap_or_default();
                    repl.chat.add_message(
                        ChatRole::System,
                        format!(
                            "[Webhook: {}] {}\n  {}{url_note}",
                            event.source, event.title, event.body
                        ),
                    );
                }
                if count == 0 {
                    repl.chat.add_message(
                        ChatRole::System,
                        "No pending webhook events.".to_string(),
                    );
                }
            } else {
                repl.chat.add_message(
                    ChatRole::System,
                    "No webhook receiver running. Use /webhook start first.".to_string(),
                );
            }
        }
        _ => {
            repl.chat.add_message(
                ChatRole::System,
                "Webhook receiver — receive external events into this session.\n\n\
                 Usage:\n\
                   /webhook start  — start the webhook HTTP server\n\
                   /webhook stop   — stop the receiver\n\
                   /webhook status — show receiver status\n\
                   /webhook poll   — inject pending events into session\n\n\
                 Default: 127.0.0.1:3789\n\
                 GitHub: POST /webhook/github (issue_comment, PR reviews)\n\
                 Generic: POST /webhook/generic {\"title\":\"...\",\"body\":\"...\"}".to_string(),
            );
        }
    }
    Ok(())
}

pub(crate) fn handle_web_search(repl: &mut Repl, args: &str) -> Result<()> {
    let query = args.trim();
    if query.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /web-search <query>\nSearches the web using Tavily API. Set SHANNON_SEARCH_API_KEY to configure.".to_string());
        return Ok(());
    }

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let input = serde_json::json!({
        "query": query,
        "max_results": 5,
        "search_depth": "basic"
    });

    match repl.runtime.block_on(engine.tools().execute("WebSearch", input)) {
        Ok(result) => {
            let mut output = format!("Web search results for: {query}\n\n");
            if let Some(results) = result.metadata.get("results").and_then(|r| r.as_array()) {
                for (i, item) in results.iter().enumerate() {
                    let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("Untitled");
                    let url = item.get("url").and_then(|u| u.as_str()).unwrap_or("");
                    let snippet = item.get("snippet").and_then(|s| s.as_str()).unwrap_or("");
                    output.push_str(&format!("{}. **{}**\n   {}\n   {}\n\n", i + 1, title, url, snippet));
                }
                if results.is_empty() {
                    output.push_str("No results found.");
                }
            } else {
                output.push_str(&result.content);
            }
            repl.chat.add_message(ChatRole::System, output);
        }
        Err(e) => {
            super::set_error(repl, &format!("web search: {e}\nSet SHANNON_SEARCH_API_KEY for web search capability."));
        }
    }
    Ok(())
}
