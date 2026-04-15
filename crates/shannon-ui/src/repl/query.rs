//! REPL AI query handling and streaming display

use crate::{
    widgets::ChatRole,
    Result,
};
use ratatui::backend::CrosstermBackend;
use futures::StreamExt;
use ratatui::Terminal;
use std::io;
use uuid::Uuid;

use crate::repl_enhancement::TurnDiff;
use shannon_core::query_engine::{QueryContext, QueryEvent};

use super::Repl;

/// Handle a query (send to AI)
pub fn handle_query(repl: &mut Repl, input: &str) -> Result<()> {
    repl.state.status = "Processing...".to_string();
    repl.state.active_tool = None;
    repl.state.query_steps_done = 0;
    repl.state.query_steps_total = 0;
    repl.state.progress_bar_visible = false;
    repl.state.progress_bar.set_progress(0.0);

    let _turn_diff = TurnDiff::new(repl.current_turn);

    let assistant_msg_index = repl.chat.add_message(ChatRole::Assistant, String::new());

    // Take the query engine out — spawn requires 'static ownership
    let query_engine = repl.query_engine.take().expect("QueryEngine not initialized");

    let query_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();

    let context = QueryContext {
        query_id,
        session_id,
        user_message: input.to_string(),
        metadata: shannon_core::query_engine::QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: Some(4096),
            model: {
                // Check model routing rules first
                let input_lower = input.to_lowercase();
                let routed = repl.model_routes.iter().find(|(pattern, _)| {
                    input_lower.starts_with(pattern)
                });
                routed.map(|(_, m)| m.clone())
                    .or_else(|| repl.state.model.clone())
                    .unwrap_or_else(|| "claude-3-5-sonnet".to_string())
            },
            temperature: None,
            top_p: None,
        },
    };

    // Estimate cost before sending
    {
        let model = context.metadata.model.as_str();
        let max_tokens: u64 = context.metadata.max_tokens.unwrap_or(4096) as u64;
        let history_chars: usize = repl.tools_invoked * 200 + repl.current_turn * 500;
        let new_msg_chars = input.len();
        let tracker = shannon_core::query_engine::CostTracker::new(model.to_string());
        let estimate = tracker.estimate_query_cost(model, history_chars, new_msg_chars, max_tokens);
        if estimate.estimated_cost_usd > 0.0 {
            repl.state.status = format!("Cost estimate: {estimate}");
        }
    }

    // Shared state between the async query task and the main UI loop
    use std::sync::{Arc, Mutex};
    let streaming_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let streaming_status: Arc<Mutex<String>> = Arc::new(Mutex::new("Processing...".to_string()));
    let streaming_done: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let streaming_cost: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.0));
    let streaming_progress: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.0));
    let streaming_multi_progress: Arc<Mutex<Vec<(String, f64, ratatui::style::Color)>>> = Arc::new(Mutex::new(Vec::new()));

    let buffer_clone = streaming_buffer.clone();
    let status_clone = streaming_status.clone();
    let done_clone = streaming_done.clone();
    let cost_clone = streaming_cost.clone();
    let progress_clone = streaming_progress.clone();
    let multi_progress_clone = streaming_multi_progress.clone();
    let permission_tx = repl.permission_req_tx.clone();

    // Spawn the query processing in a separate thread
    let query_handle = repl.runtime.spawn(async move {
        shannon_core::prevent_sleep::start_prevent_sleep();
        let permission_channel = Some(permission_tx);
        let mut stream = query_engine.process_query(context, permission_channel).await;

        let mut response_text = String::new();
        let mut tokens_in_turn = 0u64;
        let mut tool_calls: Vec<String> = Vec::new();
        let mut _tools_in_session: usize = 0;
        let mut progress_status = "Processing...".to_string();
        let mut steps_done = 0usize;
        let mut turn_diff = TurnDiff::new(0);

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(QueryEvent::Started { .. }) => {}
                Ok(QueryEvent::Text { content, .. }) => {
                    response_text.push_str(&content);
                    if let Ok(mut buf) = buffer_clone.lock() {
                        *buf = response_text.clone();
                    }
                }
                Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                    steps_done += 1;
                    progress_status = format!("Running: {tool_name} (step {steps_done})");
                    let tool_display = format!("\n🔧 Using: {} with input: {}", tool_name,
                        serde_json::to_string_pretty(&tool_input).unwrap_or_else(|_| "invalid".to_string()));
                    response_text.push_str(&tool_display);
                    tool_calls.push(tool_name.clone());
                    _tools_in_session += 1;

                    {
                        let colors = [
                            ratatui::style::Color::Cyan,
                            ratatui::style::Color::Green,
                            ratatui::style::Color::Yellow,
                            ratatui::style::Color::Magenta,
                            ratatui::style::Color::Blue,
                        ];
                        let color = colors[tool_calls.len() % colors.len()];
                        if let Ok(mut mp) = multi_progress_clone.lock() {
                            mp.push((tool_name.clone(), 0.0, color));
                        }
                    }

                    if let Ok(mut s) = status_clone.lock() { *s = progress_status.clone(); }
                    if let Ok(mut buf) = buffer_clone.lock() { *buf = response_text.clone(); }

                    if tool_name == "write" || tool_name == "edit" || tool_name == "WriteTool" {
                        if let Some(path) = tool_input.get("file_path").and_then(|v| v.as_str()) {
                            turn_diff.modify_file(path.to_string(), 1, 0);
                        }
                    }
                }
                Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                    let formatted = crate::tool_format::format_tool_result(&tool_name, &result, is_error);
                    response_text.push_str(&format!("\n{formatted}"));
                    if let Ok(mut buf) = buffer_clone.lock() { *buf = response_text.clone(); }
                    if let Ok(mut mp) = multi_progress_clone.lock() {
                        if let Some(bar) = mp.iter_mut().find(|(l, _, _)| l == &tool_name) {
                            bar.1 = 1.0;
                        }
                    }
                }
                Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                    tokens_in_turn += tokens_used;
                    response_text.push_str(&format!("\n\n[Turn {turn_number} completed, {tokens_used} tokens]"));
                }
                Ok(QueryEvent::Progress { message, .. }) => {
                    progress_status = format!("Processing: {message}");
                    response_text.push_str(&format!("\n⏳ {message}"));
                    if let Ok(mut s) = status_clone.lock() { *s = progress_status.clone(); }
                    if let Ok(mut buf) = buffer_clone.lock() { *buf = response_text.clone(); }
                }
                Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                    response_text.push_str(&format!("\n📊 Tokens: {input_tokens} in + {output_tokens} out = ${cost_usd:.4}"));
                }
                Ok(QueryEvent::Cost { total_cost_usd, input_tokens, output_tokens, .. }) => {
                    tokens_in_turn = input_tokens + output_tokens;
                    if let Ok(mut c) = cost_clone.lock() { *c = total_cost_usd; }
                }
                Ok(QueryEvent::ToolProgress { progress, tool_name, .. }) => {
                    let pct = (progress * 100.0) as u32;
                    response_text.push_str(&format!("\n⏳ Tool progress: {pct}%"));
                    if let Ok(mut p) = progress_clone.lock() { *p = progress as f64; }
                    if let Ok(mut buf) = buffer_clone.lock() { *buf = response_text.clone(); }
                    progress_status = format!("{tool_name}: {pct}%");
                    if let Ok(mut s) = status_clone.lock() { *s = progress_status.clone(); }
                }
                Ok(QueryEvent::Completed { .. }) => {
                    if let Ok(cost) = cost_clone.lock() {
                        if *cost > 0.0 {
                            response_text.push_str(&format!("\n💰 Session total: ${:.4}", *cost));
                        }
                    }
                }
                Ok(QueryEvent::Failed { error, .. }) => {
                    return Err(format!("Query failed: {error}"));
                }
                Err(e) => {
                    return Err(format!("Stream error: {e}"));
                }
            }
        }

        Ok::<(shannon_core::query_engine::QueryEngine, String, u64, usize, TurnDiff, String, usize), String>(
            (query_engine, response_text, tokens_in_turn, _tools_in_session, turn_diff, progress_status, steps_done)
        )
    });

    // Poll the streaming buffer while the query runs
    {
        let terminal_backend = CrosstermBackend::new(io::stdout());
        let mut polling_terminal = Terminal::new(terminal_backend)?;
        let mut last_rendered_len = 0usize;

        loop {
            let is_done = done_clone.lock().map(|g| *g).unwrap_or(false);
            let query_finished = is_done || query_handle.is_finished();

            let current_text = streaming_buffer.lock().map(|g| g.clone()).unwrap_or_default();
            let current_status = streaming_status.lock().map(|g| g.clone()).unwrap_or_default();

            if current_text.len() != last_rendered_len {
                let rendered = repl.output_renderer.render_streaming(&current_text);
                repl.chat.update_message(assistant_msg_index, rendered);
                last_rendered_len = current_text.len();
            }

            repl.state.status = current_status;

            if let Ok(cost) = streaming_cost.lock().map(|g| *g) {
                if cost > 0.0 { repl.state.total_cost_usd = cost; }
            }

            if let Ok(progress_val) = streaming_progress.lock().map(|g| *g) {
                if progress_val > 0.0 {
                    repl.state.progress_bar_visible = true;
                    repl.state.progress_bar.set_progress(progress_val);
                    if let Some(ref tool) = repl.state.active_tool {
                        repl.state.progress_bar.set_title(tool.clone());
                    }
                } else {
                    repl.state.progress_bar_visible = false;
                }
            }

            if let Ok(mp_data) = streaming_multi_progress.lock().map(|g| g.clone()) {
                if !mp_data.is_empty() {
                    repl.state.multi_progress_visible = true;
                    repl.state.multi_progress.clear();
                    for (label, progress, color) in mp_data {
                        repl.state.multi_progress = repl.state.multi_progress.clone().add_bar(label, progress, color);
                    }
                } else {
                    repl.state.multi_progress_visible = false;
                }
            }

            // Render the UI during streaming
            repl.state.spinner.tick();
            let chat = &repl.chat;
            let prompt = &repl.prompt;
            let state = repl.state.clone();
            let spinner = &repl.state.spinner;
            let pb = if repl.state.progress_bar_visible { Some(&repl.state.progress_bar) } else { None };

            polling_terminal.draw(|f| {
                crate::widgets::MainLayoutWidget::render_complete_with_spinner(
                    f, chat, prompt, &state.status,
                    state.model.as_deref(), Some(state.tokens_used),
                    &state.working_directory, Some(spinner), pb,
                );
                if state.multi_progress_visible {
                    let mp_height = 3u16.min(f.area().height.saturating_sub(10));
                    let mp_area = ratatui::layout::Rect {
                        x: f.area().x + 2,
                        y: f.area().bottom().saturating_sub(mp_height + 3),
                        width: f.area().width.saturating_sub(4),
                        height: mp_height,
                    };
                    state.multi_progress.render(f, mp_area);
                }
            })?;

            if query_finished { break; }

            // Check for cancel key (Escape or Ctrl+C) during streaming
            if crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false) {
                if let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() {
                    let is_cancel = matches!(key.code,
                        crossterm::event::KeyCode::Esc
                    ) || (key.code == crossterm::event::KeyCode::Char('c')
                        && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL));

                    if is_cancel {
                        query_handle.abort();
                        if let Ok(mut buf) = streaming_buffer.lock() {
                            buf.push_str("\n\n⚠️ Cancelled by user.");
                        }
                        if let Ok(mut s) = streaming_status.lock() {
                            *s = "Cancelled".to_string();
                        }
                        break;
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    shannon_core::prevent_sleep::stop_prevent_sleep();

    let query_result = repl.runtime.block_on(async {
        match query_handle.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Err("cancelled".to_string()),
            Err(_) => Err("Query task panicked".to_string()),
        }
    });

    match query_result {
        Ok((mut engine, response, tokens, tools, turn, _final_status, steps)) => {
            engine.add_user_message(input.to_string());
            engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
                text: response.clone(),
            }]);
            repl.query_engine = Some(engine);

            let rendered = repl.output_renderer.render_output(&response, "assistant");
            repl.chat.update_message(assistant_msg_index, rendered);
            repl.state.tokens_used += tokens;
            repl.tools_invoked += tools;

            if turn.total_files_touched() > 0 {
                repl.diff_data.record_turn_diff(turn);
            }
            repl.current_turn += 1;

            repl.state.query_steps_done = steps;
            repl.state.query_steps_total = steps;
            repl.state.progress_bar_visible = false;
            repl.state.progress_bar.set_progress(0.0);
            repl.state.status = if steps > 0 {
                format!("Ready ({steps} steps completed)")
            } else {
                "Ready".to_string()
            };
        }
        Err(e) => {
            let is_cancelled = e == "cancelled";

            let mut new_engine = shannon_core::query_engine::QueryEngine::with_defaults(
                shannon_core::api::LlmClient::new(shannon_core::api::LlmClientConfig::default()),
                shannon_core::tools::ToolRegistry::new(),
                shannon_core::permissions::PermissionManager::new(),
                shannon_core::state::StateManager::new(),
            );
            new_engine.add_user_message(input.to_string());
            repl.query_engine = Some(new_engine);

            if is_cancelled {
                let current = streaming_buffer.lock().map(|g| g.clone()).unwrap_or_default();
                repl.chat.update_message(assistant_msg_index, current);
                repl.state.status = "Ready".to_string();
            } else {
                repl.chat.update_message(assistant_msg_index, format!("❌ Error: {e}"));

                let err_lower = e.to_lowercase();
                if err_lower.contains("api key") || err_lower.contains("api_key") {
                    repl.show_input_dialog("API Key Required", "Enter your API key...", "set_api_key");
                } else if err_lower.contains("authentication") || err_lower.contains("unauthorized") || err_lower.contains("forbidden") {
                    repl.show_alert_dialog("Query Error", &e.to_string(), true);
                }
            }

            repl.state.status = "Ready".to_string();
            repl.state.progress_bar_visible = false;
            repl.state.progress_bar.set_progress(0.0);
        }
    }

    repl.state.active_tool = None;
    repl.state.progress_bar_visible = false;
    repl.state.multi_progress_visible = false;
    repl.state.multi_progress.clear();

    Ok(())
}
