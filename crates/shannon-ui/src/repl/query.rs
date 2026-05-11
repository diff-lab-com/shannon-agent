//! REPL AI query handling and streaming display


/// Rotating phrases shown during the thinking phase, cycled every 2 seconds.
const THINKING_PHRASES: &[&str] = &[
    "Thinking",
    "Analyzing",
    "Processing",
    "Reasoning",
    "Considering",
    "Evaluating",
];

use crate::{
    stream_buffer::StreamBuffer,
    widgets::ChatRole,
    Result,
};
use rust_i18n::t;
use shannon_types::recover_lock;
use ratatui::backend::CrosstermBackend;
use futures::StreamExt;
use ratatui::Terminal;
use std::collections::HashMap;
use std::io;
use std::time::Instant;
use uuid::Uuid;

use crate::repl_enhancement::TurnDiff;
use shannon_core::query_engine::{QueryContext, QueryEvent};

use super::Repl;

/// Shared streaming state between the async query task and the UI polling loop.
struct StreamingState {
    buffer: String,
    status: String,
    done: bool,
    cost: f64,
    progress: f64,
    multi_progress: Vec<(String, f64, ratatui::style::Color)>,
    tokens: (u64, u64), // (input, output)
    tools: usize,
    budget: Option<f64>,
    delta: String,
    /// Whether the model is still thinking (no text tokens yet)
    thinking_phase: bool,
    /// Rate limit info from API (used, total)
    rate_limit: Option<(u32, u32)>,
}

impl Default for StreamingState {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            status: "Processing...".to_string(),
            done: false,
            cost: 0.0,
            progress: 0.0,
            multi_progress: Vec::new(),
            tokens: (0, 0),
            tools: 0,
            budget: None,
            delta: String::new(),
            thinking_phase: true,
            rate_limit: None,
        }
    }
}

/// Handle a query (send to AI)
/// Type alias for the TUI terminal used by the REPL.
pub(crate) type Term = Terminal<CrosstermBackend<io::Stdout>>;

pub fn handle_query(repl: &mut Repl, input: &str, mut terminal: Option<&mut Term>) -> Result<()> {
    repl.state.status = t!("status.processing").to_string();
    repl.state.active_tool = None;
    repl.state.query_steps_done = 0;
    repl.state.query_steps_total = 0;
    repl.state.progress_bar_visible = false;
    repl.state.progress_bar.set_progress(0.0);

    let _turn_diff = TurnDiff::new(repl.current_turn);

    let assistant_msg_index = repl.chat.add_message(ChatRole::Assistant, String::new());

    // Take the query engine out — spawn requires 'static ownership
    let mut query_engine = match repl.query_engine.take() {
        Some(e) => e,
        None => {
            repl.chat.add_message(ChatRole::System, "Error: Query engine not available. Please restart the session.".to_string());
            repl.state.status = "Ready".to_string();
            return Ok(());
        }
    };

    // Sync the model (and provider, if changed) from REPL state into the engine's LLM client
    if let Some(ref provider) = repl.state.selected_provider {
        if let Some(ref model) = repl.state.model {
            query_engine.set_model_for_provider(model.clone(), provider.clone());
        }
    } else if let Some(ref model) = repl.state.model {
        query_engine.set_model(model.clone());
    }

    // Sync effort_level and focus_area from REPL state into the query engine
    query_engine.set_effort_level(repl.state.effort_level.clone());
    query_engine.set_focus_area(repl.state.focus_area.clone());

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
    let streaming: Arc<Mutex<StreamingState>> = Arc::new(Mutex::new(StreamingState::default()));
    let ss = streaming.clone();
    let permission_tx = repl.permission_req_tx.clone();

    // Save pre-stream values so we can show real-time totals during streaming
    let pre_stream_tokens = repl.state.tokens_used;
    let pre_stream_tools = repl.tools_invoked;
    let pre_stream_cost = repl.state.total_cost_usd;

    // Spawn the query processing in a separate thread
    let query_handle = repl.runtime.spawn(async move {
        shannon_core::prevent_sleep::start_prevent_sleep();
        let permission_channel = Some(permission_tx);
        let mut stream = query_engine.process_query(context, permission_channel).await;

        let mut response_text = String::new();
        let mut tokens_in_turn = 0u64;
        let mut tool_calls: Vec<String> = Vec::new();
        let mut tool_start_times: HashMap<String, Instant> = HashMap::new();
        let mut _tools_in_session: usize = 0;
        let mut progress_status = "Working".to_string();
        let mut steps_done = 0usize;
        let mut turn_diff = TurnDiff::new(0);

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(QueryEvent::Started { .. }) => {}
                Ok(QueryEvent::Thinking { content, .. }) => {
                    // Accumulate thinking content but don't display inline yet
                    // Will be shown as collapsible block after completion
                    let _ = content;
                }
                Ok(QueryEvent::Text { content, .. }) => {
                    response_text.push_str(&content);
                    if let Ok(mut s) = ss.lock() {
                        s.buffer = response_text.clone();
                        s.delta.push_str(&content);
                        s.thinking_phase = false;
                    }
                }
                Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                    steps_done += 1;
                    {
                        let colors = [
                            ratatui::style::Color::Cyan,
                            ratatui::style::Color::Green,
                            ratatui::style::Color::Yellow,
                            ratatui::style::Color::Magenta,
                            ratatui::style::Color::Blue,
                        ];
                        let color = colors[tool_calls.len() % colors.len()];
                        if let Ok(mut s) = ss.lock() {
                            s.tools += 1;
                            s.multi_progress.push((tool_name.clone(), 0.0, color));
                            s.status = format!("Tool: {tool_name}");
                            s.buffer = response_text.clone();
                        }
                    }

                    progress_status = format!("Tool: {tool_name}");
                    let tool_display = format!("\n> Using: {} with input: {}", tool_name,
                        serde_json::to_string_pretty(&tool_input).unwrap_or_else(|_| "invalid".to_string()));
                    tool_start_times.insert(tool_name.clone(), Instant::now());
                    response_text.push_str(&tool_display);
                    tool_calls.push(tool_name.clone());
                    _tools_in_session += 1;

                    if tool_name == "write" || tool_name == "edit" || tool_name == "WriteTool" {
                        if let Some(path) = tool_input.get("file_path").and_then(|v| v.as_str()) {
                            turn_diff.modify_file(path.to_string(), 1, 0);
                        }
                    }
                }
                Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                    let duration_str = tool_start_times.remove(&tool_name).map(|start| {
                        let elapsed = start.elapsed();
                        if elapsed.as_secs() >= 60 {
                            format!("{}m{:.0}s", elapsed.as_secs() / 60, elapsed.as_secs_f64() % 60.0)
                        } else {
                            format!("{:.1}s", elapsed.as_secs_f64())
                        }
                    }).unwrap_or_default();
                    let status_icon = if is_error { "x" } else { "ok" };
                    let formatted = crate::tool_format::format_tool_result(&tool_name, &result, is_error);
                    if duration_str.is_empty() {
                        response_text.push_str(&format!("\n{formatted}"));
                    } else {
                        response_text.push_str(&format!("\n{formatted}\n  [{status_icon}] {duration_str}"));
                    }
                    if let Ok(mut s) = ss.lock() {
                        s.buffer = response_text.clone();
                        if let Some(bar) = s.multi_progress.iter_mut().find(|(l, _, _)| l == &tool_name) {
                            bar.1 = 1.0;
                        }
                    }
                }
                Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                    tokens_in_turn += tokens_used;
                    response_text.push_str(&format!("\n\n[Turn {turn_number} completed, {tokens_used} tokens]"));
                }
                Ok(QueryEvent::Progress { message, .. }) => {
                    progress_status = format!("Working: {message}");
                    response_text.push_str(&format!("\n⏳ {message}"));
                    if let Ok(mut s) = ss.lock() {
                        s.status = progress_status.clone();
                        s.buffer = response_text.clone();
                    }
                }
                Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd: _, .. }) => {
                    if let Ok(mut s) = ss.lock() { s.tokens = (input_tokens, output_tokens); }
                }
                Ok(QueryEvent::Cost { total_cost_usd, input_tokens, output_tokens, .. }) => {
                    tokens_in_turn = input_tokens + output_tokens;
                    let budget_limit;
                    {
                        let mut s = recover_lock(ss.lock());
                        s.cost = total_cost_usd;
                        budget_limit = s.budget;
                    }
                    // Budget warning: alert when cost exceeds 80% of limit
                    if let Some(limit) = budget_limit {
                        if total_cost_usd > limit * 0.8 && total_cost_usd <= limit {
                            response_text.push_str(&format!(
                                "\n\n⚠️ Budget warning: ${total_cost_usd:.4} / ${limit:.2} ({:.0}% used)",
                                (total_cost_usd / limit) * 100.0
                            ));
                        } else if total_cost_usd > limit {
                            response_text.push_str(&format!(
                                "\n\n🚨 Budget exceeded: ${total_cost_usd:.4} > ${limit:.2}"
                            ));
                        }
                    }
                }
                Ok(QueryEvent::ToolProgress { progress, tool_name, .. }) => {
                    let pct = (progress * 100.0) as u32;
                    if let Ok(mut s) = ss.lock() {
                        s.progress = progress as f64;
                        s.status = format!("{tool_name}: {pct}%");
                        // Update the specific tool's progress bar
                        if let Some(bar) = s.multi_progress.iter_mut().find(|(l, _, _)| l == &tool_name) {
                            bar.1 = progress as f64;
                        }
                    }
                }
                Ok(QueryEvent::Completed { .. }) => {
                    let mut summary_parts = Vec::new();
                    if tokens_in_turn > 0 {
                        let turn_fmt = if tokens_in_turn >= 1000 {
                            format!("{:.1}k", tokens_in_turn as f64 / 1000.0)
                        } else {
                            tokens_in_turn.to_string()
                        };
                        summary_parts.push(format!("{turn_fmt} tokens this turn"));
                    }
                    if let Ok(s) = ss.lock() {
                        if s.cost > 0.0 {
                            summary_parts.push(format!("${:.4} total", s.cost));
                        }
                    }
                    if !summary_parts.is_empty() {
                        response_text.push_str(&format!("\n📊 {}", summary_parts.join(" · ")));
                    }
                }
                Ok(QueryEvent::Failed { error, .. }) => {
                    return Err(format!("Query failed: {error}"));
                }
                Ok(QueryEvent::Info { message, .. }) => {
                    tracing::info!("Query info: {message}");
                }
                Ok(QueryEvent::RateLimit { requests_used, requests_limit, .. }) => {
                    if let Ok(mut s) = ss.lock() {
                        s.rate_limit = Some((requests_used, requests_limit));
                    }
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
        let mut buffer = StreamBuffer::new();
        let stream_start = std::time::Instant::now();

        // Activate streaming state
        repl.state.streaming_active = true;
        repl.state.thinking_phase = true;
        repl.state.streaming_start = Some(stream_start);
        repl.state.streaming_token_rate = 0.0;
        repl.state.streaming_output_start = None;
        repl.state.desktop_notified = false;
        repl.chat.streaming_active = true;

        loop {
            let is_done = streaming.lock().map(|s| s.done).unwrap_or(false);
            let query_finished = is_done || query_handle.is_finished();

            let current_status;
            {
                let s = recover_lock(streaming.lock());
                current_status = s.status.clone();

                if !s.delta.is_empty() {
                    buffer.push_chunk(&s.delta);
                }
            }
            {
                let mut s = recover_lock(streaming.lock());
                s.delta.clear();
            }

            if buffer.needs_render() {
                let has_newline = buffer.has_newline_since_drain();
                let _ = buffer.drain_for_render();
                let rendered = repl.output_renderer.render_streaming(buffer.accumulated_text());
                repl.chat.update_streaming_message(assistant_msg_index, rendered, has_newline);
                buffer.take_newline_flag();
                if repl.state.auto_follow {
                    repl.chat.scroll_to_latest();
                }
            }

            // Thinking indicator: fixed-width label while model thinks
            let is_thinking = streaming.lock().map(|s| s.thinking_phase).unwrap_or(false);
            repl.state.thinking_phase = is_thinking;
            if is_thinking {
                let phase_idx = (stream_start.elapsed().as_secs() / 2) as usize % THINKING_PHRASES.len();
                repl.state.status = THINKING_PHRASES[phase_idx].to_string();
            } else if repl.state.streaming_token_rate > 0.0 {
                repl.state.status = format!("{} \u{00b7} {:.0} tok/s", current_status, repl.state.streaming_token_rate);
            } else {
                repl.state.status = current_status.clone();
            }

            // Toast for long operations (>5s)
            let elapsed = stream_start.elapsed();
            if elapsed.as_secs() >= 5 && repl.state.toast.is_none() {
                let tool_name = streaming.lock().ok()
                    .and_then(|s| s.multi_progress.last().map(|(n, _, _)| n.clone()))
                    .unwrap_or_else(|| "query".to_string());
                repl.state.toast = Some((format!("Running {tool_name}…"), stream_start));
            }

            {
                let s = recover_lock(streaming.lock());
                if s.cost > 0.0 { repl.state.total_cost_usd = s.cost; }

                // Update token display in real-time during streaming
                let (input, output) = s.tokens;
                if input > 0 || output > 0 {
                    repl.state.tokens_used = pre_stream_tokens + input + output;
                    repl.state.input_tokens = input;
                    repl.state.output_tokens = output;
                    // Track token output rate
                    if output > 0 {
                        let output_start = repl.state.streaming_output_start.get_or_insert_with(std::time::Instant::now);
                        let elapsed = output_start.elapsed().as_secs_f64();
                        if elapsed > 0.1 {
                            repl.state.streaming_token_rate = output as f64 / elapsed;
                        }
                    }
                }

                // Update tool count in real-time during streaming
                if s.tools > 0 {
                    let new_tools = pre_stream_tools + s.tools;
                    if new_tools > repl.tools_invoked {
                        let delta = new_tools - repl.tools_invoked;
                        repl.notify(format!("Tool completed (×{delta})"));
                    }
                    repl.tools_invoked = new_tools;
                }

                if s.progress > 0.0 {
                    repl.state.progress_bar_visible = true;
                    repl.state.progress_bar.set_progress(s.progress);
                    if let Some(ref tool) = repl.state.active_tool {
                        repl.state.progress_bar.set_title(tool.clone());
                    }
                } else {
                    repl.state.progress_bar_visible = false;
                }

                if !s.multi_progress.is_empty() {
                    repl.state.multi_progress_visible = true;
                    repl.state.multi_progress.clear();
                    for (label, progress, color) in s.multi_progress.iter() {
                        repl.state.multi_progress = repl.state.multi_progress.clone().add_bar(label.clone(), *progress, *color);
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
            let sidebar_info = repl.sidebar_info();

            if let Some(ref mut term) = terminal {
            term.draw(|f| {
                let spinner = &state.spinner;
                let pb = if state.progress_bar_visible { Some(&state.progress_bar) } else { None };
                crate::widgets::MainLayoutWidget::render_complete_with_spinner(
                    f, chat, prompt, &state.status,
                    state.model.as_deref(), Some(state.tokens_used),
                    &state.working_directory, Some(spinner), pb, sidebar_info.as_ref(), &state.theme, state.sidebar_tab,
                    Some(&state.approval_mode_label),
                    state.focus_mode, state.fullscreen_mode,
                    None, &[], None,
                    None, None, None,
                    None,
                    None,
                    None,
                    None,
                    state.auto_follow,
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
            } // end if let Some(ref mut term) = terminal

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
                        if let Ok(mut s) = streaming.lock() {
                            s.buffer.push_str("\n\n⚠️ Cancelled by user.");
                            s.status = t!("status.cancelled_status").to_string();
                        }
                        break;
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Clear streaming state
        repl.state.streaming_active = false;
        repl.state.thinking_phase = false;
        repl.state.streaming_start = None;
        repl.state.streaming_token_rate = 0.0;
        repl.state.streaming_output_start = None;
        // Transfer rate limit from streaming state
        if let Ok(s) = streaming.lock() {
            repl.state.rate_limit_5h = s.rate_limit;
        }
        repl.chat.streaming_active = false;
        repl.state.toast = None;
        repl.notify("Response complete");
        // Send terminal bell + desktop notification for long-running tasks (>30s)
        if stream_start.elapsed().as_secs() >= 30 {
            let _ = std::io::Write::write_all(&mut std::io::stderr(), b"\x07");
            if !repl.state.desktop_notified && repl.notifications_enabled {
                repl.state.desktop_notified = true;
                let _ = repl.notifier.notify(&shannon_core::notifier::Notification {
                    title: "Shannon".to_string(),
                    body: "Task completed".to_string(),
                    level: shannon_core::notifier::NotificationLevel::Info,
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        // Commit completed turns to terminal scrollback
        let width = repl.chat.last_render_area.lock()
            .ok()
            .and_then(|ra| ra.map(|r| r.width))
            .unwrap_or(80);
        let (lines, _height) = repl.chat.commit_to_lines(width);
        if !lines.is_empty() {
            repl.chat.pending_scrollback = lines;
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
            repl.state.tokens_used = pre_stream_tokens + tokens;
            repl.tools_invoked = pre_stream_tools + tools;

            // Record billing for this turn
            let turn_cost = repl.state.total_cost_usd - pre_stream_cost;
            if turn_cost > 0.0 {
                let model_name = repl.state.model.as_deref().unwrap_or("unknown");
                let record = shannon_core::billing::UsageRecord::new(
                    model_name,
                    tokens,
                    0, // output tokens not separately tracked per turn
                    turn_cost,
                );
                if let Err(e) = repl.state.billing_manager.record_usage(record) {
                    tracing::warn!("Billing recording failed: {e}");
                }
            }

            // Collect turn file info before move
            let turn_files: Vec<String> = turn.files_modified.iter()
                .map(|f| f.path.clone())
                .chain(turn.files_created.iter().cloned())
                .chain(turn.files_deleted.iter().cloned())
                .collect();
            let files_touched = turn.total_files_touched();

            if files_touched > 0 {
                repl.diff_data.record_turn_diff(turn);
            }
            repl.current_turn += 1;

            // Record per-turn checkpoint with file change tracking
            let prompt_preview = if input.chars().count() > 80 {
                let truncated: String = input.chars().take(80).collect();
                format!("{truncated}...")
            } else {
                input.to_string()
            };
            let cp_list = repl.checkpoint_manager.list_checkpoints();
            if let Some(latest_cp) = cp_list.last() {
                repl.checkpoint_manager.record_turn(
                    repl.current_turn - 1,
                    latest_cp.checkpoint.clone(),
                    turn_files,
                    Some(prompt_preview),
                );
            } else if files_touched > 0 {
                let synthetic_cp = shannon_core::Checkpoint {
                    hash: String::new(),
                    short_hash: String::new(),
                    description: format!("turn {}", repl.current_turn),
                    timestamp: chrono::Utc::now().timestamp(),
                };
                repl.checkpoint_manager.record_turn(
                    repl.current_turn - 1,
                    synthetic_cp,
                    turn_files,
                    Some(prompt_preview),
                );
            }

            // Auto-memory: if the assistant response contains memory-worthy
            // patterns, persist them to the memory store automatically.
            auto_save_memory(repl, &response);

            // Auto-save session state after each turn
            if let Some(ref engine) = repl.query_engine {
                let messages = engine.conversation_history();
                let metadata = shannon_core::state::SessionPersistMetadata {
                    model: repl.state.model.clone().unwrap_or_default(),
                    created_at: repl.session_started_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: chrono::Utc::now(),
                    total_input_tokens: repl.state.tokens_used,
                    total_output_tokens: 0,
                    turn_count: repl.current_turn,
                    title: None,
                    parent_session_id: None,
                    branch_point_message_index: None,
                };
                if let Err(e) = repl.state_manager.save_session(&engine.session_id(), &messages, &metadata) {
                    tracing::debug!("Auto-save session error: {e}");
                }

                // Crash-safe JSONL append: log the latest user message + assistant
                // response so that on crash only the in-flight turn is lost.
                {
                    let project_dir = std::env::current_dir().unwrap_or_default();
                    let session_id_str = engine.session_id().to_string();
                    let model = repl.state.model.clone().unwrap_or_default();
                    let sr = &repl.session_recovery;

                    // Ensure a recovery session exists for this engine session.
                    let log_path = sr.session_log_path(
                        &sr.project_session_dir(&project_dir),
                        &session_id_str,
                    );
                    if !log_path.exists() {
                        if let Err(e) = sr.create_session_with_id(&project_dir, &session_id_str, &model) {
                            tracing::debug!("Recovery session create error: {e}");
                        }
                    }

                    // Append the last two messages (user + assistant) if available.
                    let seq = (repl.current_turn.saturating_sub(1)) as u64 * 2;
                    if messages.len() >= 2 {
                        let user_msg = &messages[messages.len() - 2];
                        let asst_msg = &messages[messages.len() - 1];
                        if let Err(e) = sr.append_messages(
                            &project_dir,
                            &session_id_str,
                            seq,
                            &[user_msg.clone(), asst_msg.clone()],
                        ) {
                            tracing::debug!("Recovery append error: {e}");
                        }
                    } else if messages.len() == 1 {
                        if let Err(e) = sr.append_message(
                            &project_dir,
                            &session_id_str,
                            seq,
                            &messages[0],
                        ) {
                            tracing::debug!("Recovery append error: {e}");
                        }
                    }
                }
            }

            // Check context pressure and auto-compact if needed
            repl.check_context_pressure();

            repl.state.query_steps_done = steps;
            repl.state.query_steps_total = steps;
            repl.state.progress_bar_visible = false;
            repl.state.progress_bar.set_progress(0.0);
            repl.state.status = if steps > 0 {
                t!("query.ready_steps", steps = steps).to_string()
            } else {
                t!("status.ready").to_string()
            };

            // Desktop notification on query completion
            super::commands::notify_query_complete(
                &repl.notifier,
                repl.notifications_enabled,
                &repl.state.status,
            );

            // Check if loop/ralph iteration should continue
            let loop_continued = super::commands::check_loop_iteration(repl);
            if !loop_continued {
                super::commands::check_ralph_iteration(repl);
            }

            // Submit queued follow-up message if any
            if let Some(queued) = repl.state.queued_message.take() {
                if !queued.trim().is_empty() {
                    repl.state.toast = Some(("Sending queued message…".to_string(), std::time::Instant::now()));
                    super::commands::submit_input_with_text(repl, &queued);
                }
            }
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
                let current = streaming.lock().map(|s| s.buffer.clone()).unwrap_or_default();
                repl.chat.update_message(assistant_msg_index, current);
                repl.state.status = t!("status.ready").to_string();
            } else {
                repl.chat.update_message(assistant_msg_index, format!("❌ Error: {e}"));

                let err_lower = e.to_lowercase();
                if err_lower.contains("api key") || err_lower.contains("api_key") {
                    repl.show_input_dialog("API Key Required", "Enter your API key...", "set_api_key");
                } else if err_lower.contains("authentication") || err_lower.contains("unauthorized") || err_lower.contains("forbidden") {
                    repl.show_alert_dialog("Query Error", &e.to_string(), true);
                }
            }

            repl.state.status = t!("status.ready").to_string();
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

/// Auto-save memory: detect memory-worthy patterns in the assistant response
/// and persist them to the memory store.
///
/// This runs after every successful query turn. It scans for explicit memory
/// signals (e.g. the assistant saying "I'll remember that") and saves the
/// relevant context. This is a lightweight heuristic — the full LLM-based
/// `MemoryExtractor` handles deeper extraction when explicitly invoked.
fn auto_save_memory(repl: &mut Repl, response: &str) {
    let engine = match repl.query_engine.as_ref() {
        Some(e) => e,
        None => return,
    };

    let memory = match engine.memory() {
        Some(m) => m,
        None => return,
    };

    // Patterns that indicate the assistant is recording a memory
    let memory_signals = [
        "i'll remember that",
        "i'll keep that in mind",
        "saved to memory",
        "noted. i'll remember",
        "saved memory",
        "i've saved this",
        "memory saved",
        "i've noted",
        "stored in memory",
        "committing to memory",
        "i'll make a note of that",
        "remembering:",
        "saved:",
        "i'll remember",
    ];

    let lower = response.to_lowercase();
    let has_signal = memory_signals.iter().any(|sig| lower.contains(sig));
    if !has_signal {
        return;
    }

    // Extract the most relevant line(s) from the response
    let content = extract_memory_content(response);
    if content.is_empty() {
        return;
    }

    let mut store = match memory.write() {
        Ok(guard) => guard,
        Err(e) => {
            tracing::warn!("memory lock poisoned, recovering: {e}");
            e.into_inner()
        }
    };
    let project = repl.state.working_directory.clone();

    use shannon_core::memory::{MemoryEntry, MemoryCategory};
    let entry = MemoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        content: content.clone(),
        category: MemoryCategory::Preference,
        project: project.clone(),
        tags: vec!["auto-memory".to_string()],
        confidence: 0.8,
        created_at: chrono::Utc::now(),
        accessed_at: chrono::Utc::now(),
        access_count: 0,
    };

    let id = entry.id.clone();
    let _ = store.add(entry);
    if let Err(e) = store.save() {
        tracing::debug!("Auto-memory save failed: {e}");
        return;
    }
    drop(store);

    // Also save as file for Claude Code-compatible persistence
    let project_path = std::path::PathBuf::from(&project);
    if let Err(e) = shannon_core::project_memory::save_memory_file(
        &project_path,
        &id,
        &content,
    ) {
        tracing::debug!("Auto-memory file save skipped: {e}");
    }

    tracing::debug!("Auto-saved memory: {}...", &id[..8]);
}

/// Extract the most memory-worthy content from a response.
/// Takes the sentence(s) around the memory signal.
fn extract_memory_content(response: &str) -> String {
    // Find lines that contain substantial content (not just the signal phrase)
    let lines: Vec<&str> = response.lines().collect();
    let mut content_lines: Vec<String> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip very short lines (likely just the signal phrase)
        if trimmed.len() < 20 {
            continue;
        }
        // Skip lines that are just formatting
        if trimmed.starts_with('#') || trimmed.starts_with("---") || trimmed.starts_with("===") {
            continue;
        }
        content_lines.push(trimmed.to_string());
        // Cap at 5 lines to avoid saving the entire response
        if content_lines.len() >= 5 {
            break;
        }
    }

    if content_lines.is_empty() {
        return String::new();
    }

    let mut content = content_lines.join("\n");
    // Cap at 500 chars
    if content.len() > 500 {
        content.truncate(500);
        content.push_str("...");
    }
    content
}
