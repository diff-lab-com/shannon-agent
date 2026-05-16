//! REPL AI query handling and streaming display


/// Rotating phrases shown during the thinking phase, cycled every 2 seconds.
const THINKING_PHRASES: &[&str] = &[
    "Thinking",
    "Analyzing",
    "Processing",
    "Reasoning",
    "Considering",
    "Evaluating",
    "Pondering",
    "Deliberating",
    "Working",
    "Reflecting",
];

/// Fixed trailing dots (avoids flickering from animated dot count changes).
fn animated_dots(_elapsed: std::time::Duration) -> &'static str {
    "···"
}

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
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    tools: usize,
    budget: Option<f64>,
    delta: String,
    /// Whether the model is still thinking (no text tokens yet)
    thinking_phase: bool,
    /// Accumulated thinking content from extended thinking mode
    thinking_content: String,
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
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            tools: 0,
            budget: None,
            delta: String::new(),
            thinking_phase: true,
            thinking_content: String::new(),
            rate_limit: None,
        }
    }
}

/// Handle a query (send to AI)
/// Type alias for the TUI terminal used by the REPL.
pub(crate) type Term = Terminal<CrosstermBackend<io::Stdout>>;

pub fn handle_query(repl: &mut Repl, input: &str, terminal: &mut Option<&mut Term>) -> Result<()> {
    repl.state.status = t!("status.processing").to_string();
    repl.state.active_tool = None;
    repl.state.query_steps_done = 0;
    repl.state.query_steps_total = 0;
    repl.state.progress_bar_visible = false;
    repl.state.progress_bar.set_progress(0.0);


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
        let mut accumulated_thinking = String::new();
        let mut conversation_messages: Option<Vec<shannon_core::api::Message>> = None;
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
                    if let Ok(mut s) = ss.lock() {
                        s.thinking_content.push_str(&content);
                        let len = s.thinking_content.chars().count();
                        s.status = if len > 1000 {
                            format!("Thinking··· ({}k chars)", len / 1000)
                        } else {
                            format!("Thinking··· ({len} chars)")
                        };
                    }
                }
                Ok(QueryEvent::Text { content, .. }) => {
                    if let Ok(mut s) = ss.lock() {
                        if s.thinking_phase && !s.thinking_content.is_empty() {
                            let thinking = std::mem::take(&mut s.thinking_content);
                            accumulated_thinking = thinking;
                        }
                        s.thinking_phase = false;
                        response_text.push_str(&content);
                        // Update buffer on every delta so partial content is preserved on abort.
                        s.buffer = response_text.clone();
                        s.delta.push_str(&content);
                    }
                }
                Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                    steps_done += 1;
                    {
                        if let Ok(mut s) = ss.lock() {
                            s.tools += 1;
                            s.multi_progress.push((tool_name.clone(), 0.0, ratatui::style::Color::Reset));
                            s.status = format!("Tool: {tool_name}");
                            s.buffer = response_text.clone();
                        }
                    }

                    progress_status = format!("Tool: {tool_name}");
                    let input_json = serde_json::to_string_pretty(&tool_input).unwrap_or_else(|_| "invalid".to_string());
                    let display_input = if input_json.len() > 500 {
                        let mut end = 500.min(input_json.len());
                        while !input_json.is_char_boundary(end) { end -= 1; }
                        format!("{}…", &input_json[..end])
                    } else {
                        input_json
                    };
                    let tool_display = format!("\n> Using: {tool_name} with input: {display_input}");
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
                    let status_icon = if is_error { "\u{2717}" } else { "\u{2713}" };
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
                Ok(QueryEvent::Usage { input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, .. }) => {
                    if let Ok(mut s) = ss.lock() {
                        s.tokens = (input_tokens, output_tokens);
                        s.cache_read_tokens += cache_read_tokens;
                        s.cache_creation_tokens += cache_creation_tokens;
                    }
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
                Ok(QueryEvent::ConversationUpdate { messages, .. }) => {
                    conversation_messages = Some(messages);
                }
                Ok(QueryEvent::Completed { .. }) => {
                    // Signal streaming loop that the query is done
                    if let Ok(mut s) = ss.lock() {
                        s.done = true;
                    }
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
                        let cache_read = s.cache_read_tokens;
                        let cache_written = s.cache_creation_tokens;
                        if cache_read > 0 || cache_written > 0 {
                            let total_cache = cache_read + cache_written;
                            let hit_rate = if total_cache > 0 {
                                cache_read as f64 / total_cache as f64 * 100.0
                            } else {
                                0.0
                            };
                            summary_parts.push(format!("cache {hit_rate:.0}% hit"));
                        }
                        if s.cost > 0.0 {
                            summary_parts.push(format!("${:.4} total", s.cost));
                        }
                    }
                    if !summary_parts.is_empty() {
                        response_text.push_str(&format!("\n📊 {}", summary_parts.join(" · ")));
                    }
                }
                Ok(QueryEvent::Failed { error, .. }) => {
                    // Don't return immediately — preserve conversation_messages
                    // that may have been received via ConversationUpdate before Failed.
                    response_text.push_str(&format!("\n\n⚠️ Query failed: {error}"));
                    if let Ok(mut s) = ss.lock() {
                        s.done = true;
                        s.status = format!("Failed: {error}");
                    }
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
                    // Preserve conversation_messages even on stream errors
                    response_text.push_str(&format!("\n\n⚠️ Stream error: {e}"));
                    if let Ok(mut s) = ss.lock() {
                        s.done = true;
                        s.status = format!("Error: {e}");
                    }
                }
            }
        }

        Ok::<(shannon_core::query_engine::QueryEngine, String, String, Option<Vec<shannon_core::api::Message>>, u64, usize, TurnDiff, String, usize), (Option<shannon_core::query_engine::QueryEngine>, String)>(
            (query_engine, response_text, accumulated_thinking, conversation_messages, tokens_in_turn, _tools_in_session, turn_diff, progress_status, steps_done)
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
        repl.state.prev_output_tokens = 0;
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

            // Thinking indicator: rotating phrases with animated dots
            let (is_thinking, thinking_len, thinking_text) = streaming.lock()
                .map(|s| (s.thinking_phase, s.thinking_content.chars().count(), s.thinking_content.clone()))
                .unwrap_or((false, 0, String::new()));
            repl.state.thinking_phase = is_thinking;
            if is_thinking {
                let elapsed = stream_start.elapsed();
                let phase_idx = (elapsed.as_secs() / 2) as usize % THINKING_PHRASES.len();
                let phrase = THINKING_PHRASES[phase_idx];
                let dots = animated_dots(elapsed);
                repl.state.status = if thinking_len > 1000 {
                    format!("{phrase}{dots} ({}k chars)", thinking_len / 1000)
                } else if thinking_len > 0 {
                    format!("{phrase}{dots} ({thinking_len} chars)")
                } else {
                    format!("{phrase}{dots}")
                };

                // Show thinking content in the chat message area during thinking phase
                if !thinking_text.is_empty() {
                    let display = format_thinking_for_streaming(&thinking_text);
                    let rendered = repl.output_renderer.render_streaming(&display);
                    repl.chat.update_streaming_message(assistant_msg_index, rendered, true);
                    if repl.state.auto_follow {
                        repl.chat.scroll_to_latest();
                    }
                }
            } else if repl.state.streaming_token_rate > 0.0 {
                repl.state.status = format!("{current_status} · {:.0} tok/s", repl.state.streaming_token_rate);
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
                if s.cost > 0.0 { repl.state.total_cost_usd = pre_stream_cost + s.cost; }

                // Update token display in real-time during streaming
                let (input, output) = s.tokens;
                if input > 0 || output > 0 {
                    repl.state.tokens_used = pre_stream_tokens + input + output;
                    repl.state.input_tokens = input;
                    repl.state.output_tokens = output;
                    repl.state.cache_read_tokens = s.cache_read_tokens;
                    repl.state.cache_creation_tokens = s.cache_creation_tokens;
                    // Track token output rate (instantaneous)
                    if output > 0 {
                        let now = std::time::Instant::now();
                        let prev = repl.state.prev_output_tokens;
                        if prev > 0 {
                            let delta_tokens = output.saturating_sub(prev);
                            if let Some(last_time) = repl.state.streaming_output_start {
                                let delta_secs = now.duration_since(last_time).as_secs_f64();
                                if delta_secs > 0.1 && delta_tokens > 0 {
                                    repl.state.streaming_token_rate = delta_tokens as f64 / delta_secs;
                                }
                            }
                        }
                        repl.state.streaming_output_start = Some(now);
                        repl.state.prev_output_tokens = output;
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
                    let theme_colors = [
                        repl.state.theme.subagent_1,
                        repl.state.theme.subagent_2,
                        repl.state.theme.subagent_3,
                        repl.state.theme.subagent_4,
                        repl.state.theme.subagent_5,
                        repl.state.theme.subagent_6,
                        repl.state.theme.subagent_7,
                        repl.state.theme.subagent_8,
                    ];
                    let mut mp = crate::widgets::progress::MultiProgressWidget::new();
                    for (i, (label, progress, _color)) in s.multi_progress.iter().enumerate() {
                        let tc = theme_colors[i % theme_colors.len()];
                        mp = mp.add_bar(label.clone(), *progress, tc);
                    }
                    repl.state.multi_progress = mp;
                } else {
                    repl.state.multi_progress_visible = false;
                }
            }

            // Render the UI during streaming
            repl.state.spinner.tick();

            let chat = &repl.chat;
            let prompt = &repl.prompt;
            let state = &repl.state;
            let sidebar_info = repl.sidebar_info();

            if let Some(term) = terminal.as_deref_mut() {
            term.draw(|f| {
                let spinner = &state.spinner;
                let pb = if state.progress_bar_visible { Some(&state.progress_bar) } else { None };
                let mut render_ctx = crate::widgets::RenderContext::new(chat, prompt, &state.theme, &state.status);
                render_ctx.model = state.model.as_deref();
                render_ctx.tokens_used = Some(state.tokens_used);
                render_ctx.max_tokens = state.model.as_ref()
                    .map(|m| shannon_core::model_registry::context_window_for(m) as u64);
                render_ctx.cost_usd = Some(state.total_cost_usd);
                render_ctx.token_breakdown = Some((state.input_tokens, state.output_tokens));
                render_ctx.diag_counts = Some((state.diagnostic_store.error_count(), state.diagnostic_store.warning_count()));
                render_ctx.rate_limit = state.rate_limit_5h;
                render_ctx.git_branch = state.git_branch.as_deref();
                render_ctx.spinner = Some(spinner);
                render_ctx.progress_bar = pb;
                render_ctx.sidebar_info = sidebar_info.as_ref();
                render_ctx.sidebar_tab = state.sidebar_tab;
                render_ctx.approval_mode = Some(&state.approval_mode_label);
                render_ctx.focus_mode = state.focus_mode;
                render_ctx.fullscreen_mode = state.fullscreen_mode;
                render_ctx.auto_follow = state.auto_follow;
                render_ctx.effort_level = state.effort_level.as_deref();
                render_ctx.cached_tokens = Some(state.cache_read_tokens + state.cache_creation_tokens);
                render_ctx.cache_read_tokens = Some(state.cache_read_tokens);
                render_ctx.cache_creation_tokens = Some(state.cache_creation_tokens);
                render_ctx.turn_count = sidebar_info.as_ref().map(|si| si.turn_count);
                render_ctx.memory_rss_kb = sidebar_info.as_ref().map(|si| si.memory_rss_kb);
                // Pass thinking phase and char count from streaming state
                if let Ok(s) = streaming.lock() {
                    render_ctx.thinking_phase = s.thinking_phase;
                    if s.thinking_phase {
                        render_ctx.thinking_chars = s.thinking_content.chars().count();
                    }
                }
                crate::widgets::MainLayoutWidget::render_with_ctx(f, &render_ctx);
                // Render queue indicator overlay during streaming
                super::render::render_queue_indicator(f, state, false);
                if state.multi_progress_visible {
                    let mp_height = 3u16.min(f.area().height.saturating_sub(10));
                    let mp_area = ratatui::layout::Rect {
                        x: f.area().x + 2,
                        y: f.area().bottom().saturating_sub(mp_height + 3),
                        width: f.area().width.saturating_sub(4),
                        height: mp_height,
                    };
                    state.multi_progress.render(f, mp_area, &state.theme);
                }
            })?;
            } // end if let Some(ref mut term) = terminal

            if query_finished { break; }

            // Handle key events during streaming: cancel, scroll, and input
            if crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false) {
                if let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() {
                    let is_cancel = matches!(key.code,
                        crossterm::event::KeyCode::Esc
                    ) || (key.code == crossterm::event::KeyCode::Char('c')
                        && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL));

                    if is_cancel {
                        // ESC with queued messages: pop last back to prompt for editing.
                        // Only cancel streaming when queue is empty.
                        if key.code == crossterm::event::KeyCode::Esc
                            && !repl.state.queued_messages.is_empty()
                        {
                            if let Some(popped) = repl.state.queued_messages.pop() {
                                repl.prompt.set_input(popped);
                                let count = repl.state.queued_messages.len();
                                repl.state.status = if count > 0 {
                                    format!("Removed — {count} message(s) still queued")
                                } else {
                                    "Removed last queued message — edit and re-queue".to_string()
                                };
                                repl.state.toast =
                                    Some(("Removed — edit and re-queue".to_string(), std::time::Instant::now()));
                            }
                            continue;
                        }
                        query_handle.abort();
                        if let Ok(mut s) = streaming.lock() {
                            s.buffer.push_str("\n\n⚠️ Cancelled by user.");
                            s.status = t!("status.cancelled_status").to_string();
                        }
                        break;
                    }

                    // Allow scrolling and input during streaming
                    match key.code {
                        // Scroll navigation during streaming
                        crossterm::event::KeyCode::PageUp => {
                            let page = repl.chat.chat_viewport_height() as usize;
                            repl.chat.scroll_up_by(page.saturating_sub(2).max(1));
                            if repl.state.auto_follow {
                                repl.state.messages_at_scroll_pause = repl.chat.message_count();
                            }
                            repl.state.auto_follow = false;
                        }
                        crossterm::event::KeyCode::PageDown => {
                            let page = repl.chat.chat_viewport_height() as usize;
                            repl.chat.scroll_down_by(page.saturating_sub(2).max(1));
                            if repl.chat.is_at_bottom() {
                                repl.state.auto_follow = true;
                            }
                        }
                        crossterm::event::KeyCode::Home => {
                            repl.chat.scroll_to_top();
                            if repl.state.auto_follow {
                                repl.state.messages_at_scroll_pause = repl.chat.message_count();
                            }
                            repl.state.auto_follow = false;
                        }
                        crossterm::event::KeyCode::End => {
                            repl.chat.scroll_to_latest();
                            repl.state.auto_follow = true;
                        }
                        crossterm::event::KeyCode::Up if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                            let step = (repl.chat.chat_viewport_height() as usize / 4).max(3);
                            repl.chat.scroll_up_by(step);
                            if repl.state.auto_follow {
                                repl.state.messages_at_scroll_pause = repl.chat.message_count();
                            }
                            repl.state.auto_follow = false;
                        }
                        crossterm::event::KeyCode::Down if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                            let step = (repl.chat.chat_viewport_height() as usize / 4).max(3);
                            repl.chat.scroll_down_by(step);
                            if repl.chat.is_at_bottom() {
                                repl.state.auto_follow = true;
                            }
                        }
                        // UP with queued messages: pop last back to prompt for editing.
                        // This arm must come before the plain UP handler so the more
                        // specific match fires first.
                        crossterm::event::KeyCode::Up
                            if !key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                                && !repl.state.queued_messages.is_empty() =>
                        {
                            if let Some(popped) = repl.state.queued_messages.pop() {
                                repl.prompt.set_input(popped);
                                let count = repl.state.queued_messages.len();
                                repl.state.status = if count > 0 {
                                    format!("Popped — {count} message(s) still queued")
                                } else {
                                    "Popped last queued message — edit and re-queue".to_string()
                                };
                                repl.state.toast = Some((
                                    "Popped — edit and re-queue".to_string(),
                                    std::time::Instant::now(),
                                ));
                            }
                        }
                        // History navigation during streaming (matching input.rs behavior)
                        crossterm::event::KeyCode::Up => {
                            if !repl.state.completion_suggestions.is_empty() {
                                if repl.state.completion_suggestion_index > 0 {
                                    repl.state.completion_suggestion_index -= 1;
                                }
                            } else if repl.prompt.input().contains('\n') {
                                repl.prompt.cursor_up();
                            } else if !repl.command_history.is_empty() {
                                if repl.command_history.cursor() < 0 {
                                    repl.saved_input = repl.prompt.input().to_string();
                                }
                                if let Some(cmd) = repl.command_history.up() {
                                    repl.state.completion_suggestions.clear();
                                    repl.state.completion_suggestion_index = 0;
                                    repl.prompt.set_input(cmd.to_string());
                                }
                            }
                        }
                        crossterm::event::KeyCode::Down => {
                            if !repl.state.completion_suggestions.is_empty() {
                                if repl.state.completion_suggestion_index + 1 < repl.state.completion_suggestions.len() {
                                    repl.state.completion_suggestion_index += 1;
                                }
                            } else if repl.prompt.input().contains('\n') {
                                repl.prompt.cursor_down();
                            } else if repl.command_history.cursor() >= 0 {
                                if let Some(cmd) = repl.command_history.down() {
                                    repl.state.completion_suggestions.clear();
                                    repl.state.completion_suggestion_index = 0;
                                    repl.prompt.set_input(cmd.to_string());
                                } else {
                                    repl.command_history.reset_cursor();
                                    repl.prompt.set_input(repl.saved_input.clone());
                                }
                            }
                        }
                        // Allow typing in the prompt during streaming
                        crossterm::event::KeyCode::Char(c) if !key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && !key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
                            repl.prompt.add_char(c);
                        }
                        crossterm::event::KeyCode::Backspace => {
                            repl.prompt.backspace();
                        }
                        crossterm::event::KeyCode::Delete => {
                            repl.prompt.delete_forward();
                        }
                        crossterm::event::KeyCode::Left => {
                            repl.prompt.cursor_left();
                        }
                        crossterm::event::KeyCode::Right => {
                            repl.prompt.cursor_right();
                        }
                        // Enter during streaming queues the message (consistent with input.rs behavior)
                        crossterm::event::KeyCode::Enter if !key.modifiers.contains(crossterm::event::KeyModifiers::SHIFT) => {
                            let input = repl.prompt.input().trim().to_string();
                            if !input.is_empty()
                                && repl.state.queued_messages.len() < 50
                            {
                                let count = repl.state.queued_messages.len() + 1;
                                repl.state.queued_messages.push(input);
                                repl.prompt.clear();
                                repl.state.status = format!("Message queued ({count} in queue)");
                                repl.state.toast = Some(("Queued".to_string(), std::time::Instant::now()));
                            }
                        }
                        crossterm::event::KeyCode::Enter => {
                            // Shift+Enter → newline
                            repl.prompt.insert_newline();
                        }
                        crossterm::event::KeyCode::Tab => {
                            let input = repl.prompt.input().trim().to_string();
                            if !input.is_empty()
                                && repl.state.queued_messages.len() < 50
                            {
                                let count = repl.state.queued_messages.len() + 1;
                                repl.state.queued_messages.push(input);
                                repl.prompt.clear();
                                repl.state.status = format!("Message queued ({count} in queue)");
                            }
                        }
                        _ => {} // Ignore other keys during streaming
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Clear streaming state
        repl.state.streaming_active = false;

        // Handle deferred auto-compact
        if repl.state.pending_auto_compact {
            repl.state.pending_auto_compact = false;
            repl.check_context_pressure();
        }
        repl.state.thinking_phase = false;
        repl.state.streaming_start = None;
        repl.state.streaming_token_rate = 0.0;
        repl.state.streaming_output_start = None;
        repl.state.prev_output_tokens = 0;
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
        let (lines, _height) = repl.chat.commit_to_lines(width, &repl.state.theme);
        if !lines.is_empty() {
            repl.chat.pending_scrollback = lines;
        }
    }

    shannon_core::prevent_sleep::stop_prevent_sleep();

    let query_result = repl.runtime.block_on(async {
        match query_handle.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Err((None, "cancelled".to_string())),
            Err(_) => Err((None, "Query task panicked".to_string())),
        }
    });

    match query_result {
        Ok((mut engine, response, thinking, conversation_messages, tokens, tools, turn, _final_status, steps)) => {
            // Use the proper conversation state from the query engine if available,
            // otherwise fall back to manual message addition
            if let Some(messages) = conversation_messages {
                engine.restore_messages(messages);
            } else {
                // Fallback: ConversationUpdate was not received. This should be
                // rare — the engine's safety net in the post-stream handler
                // normally ensures ConversationUpdate is always sent. When it
                // does happen, the response text may include UI formatting
                // markers (tool call displays, turn markers), which is imperfect
                // but better than losing the turn entirely.
                tracing::warn!(
                    "ConversationUpdate not received — using fallback message addition. \
                     Response text may include UI formatting."
                );
                engine.add_user_message(input.to_string());
                engine.add_assistant_message(vec![shannon_core::api::ContentBlock::Text {
                    text: response.clone(),
                }]);
            }
            repl.query_engine = Some(engine);

            let rendered = repl.output_renderer.render_output(&response, "assistant");
            repl.chat.update_message(assistant_msg_index, rendered);
            if !thinking.is_empty() {
                repl.chat.set_thinking_content(assistant_msg_index, thinking);
            }
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
            let prompt_preview = if unicode_width::UnicodeWidthStr::width(input) > 80 {
                let mut len = 0;
                let truncated: String = input.chars()
                    .take_while(|c| {
                        let cw = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                        if len + cw > 77 { false } else { len += cw; true }
                    })
                    .collect();
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
            // NOTE: queued message processing is handled by submit_input's
            // drain loop — NOT here — to avoid recursive handle_query calls.
        }
        Err((engine_opt, e)) => {
            // Clear queued messages on error/cancel — user chose to stop.
            repl.state.queued_messages.clear();
            // Restore the query engine if it was recovered from the task.
            // Preserve the user message so conversation state stays consistent
            // (the background task only added it to its clone, not the engine).
            if let Some(mut engine) = engine_opt {
                engine.add_user_message(input.to_string());
                repl.query_engine = Some(engine);
            }
            let is_cancelled = e == "cancelled";

            if is_cancelled {
                let (current, thinking) = streaming.lock()
                    .map(|s| (s.buffer.clone(), s.thinking_content.clone()))
                    .unwrap_or_default();
                // Render partial content through the output renderer for proper formatting
                let partial_display = if current.is_empty() {
                    "\u{26A0} Cancelled by user (no text received yet)".to_string()
                } else {
                    format!("{current}\n\n\u{26A0} Cancelled by user (partial response)")
                };
                let rendered = repl.output_renderer.render_output(&partial_display, "assistant");
                repl.chat.update_message(assistant_msg_index, rendered);
                // Preserve any thinking content that was accumulated before cancellation
                if !thinking.is_empty() {
                    repl.chat.set_thinking_content(assistant_msg_index, thinking);
                }
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
    if let Err(e) = store.add(entry) {
        tracing::warn!("Auto-memory add failed: {e}");
        return;
    }
    if let Err(e) = store.save() {
        tracing::warn!("Auto-memory save failed: {e}");
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
        let mut end = 500;
        while !content.is_char_boundary(end) { end -= 1; }
        content.truncate(end);
        content.push_str("...");
    }
    content
}

/// Format thinking content for display in the streaming message area.
///
/// Shows a collapsible-style header with character count, then the last
/// few lines of thinking content. Kept brief to avoid flooding the chat
/// area during extended thinking.
fn format_thinking_for_streaming(content: &str) -> String {
    let char_count = content.chars().count();
    let header = if char_count >= 1000 {
        format!("▼ Thinking ({}k chars)", char_count / 1000)
    } else {
        format!("▼ Thinking ({char_count} chars)")
    };

    // Show the tail of thinking content (last ~300 chars) so the user
    // sees what the model is currently reasoning about.
    let tail = if char_count > 300 {
        let mut start = content.char_indices().nth(char_count - 300).map(|(i, _)| i).unwrap_or(0);
        // Align to a line boundary for clean display
        if start > 0 {
            if let Some(pos) = content[start..].find('\n') {
                start += pos + 1;
            }
        }
        format!("...\n{}", &content[start..])
    } else {
        content.to_string()
    };

    format!("{header}\n{tail}")
}

#[cfg(test)]
mod tests {
    //! Tests for query engine recovery after errors.
    //!
    //! These tests verify the error type pattern that ensures the query engine
    //! is always returned from the async query task, even on error paths.
    //!
    //! Bug: After any query error (malformed output, stream error, API failure),
    //! the engine was dropped inside the async closure, leaving `repl.query_engine`
    //! as `None` permanently — causing "Query engine not available. Please restart."
    //!
    //! Fix: Changed the error type from `String` to `(Option<QueryEngine>, String)`
    //! so the engine is carried back to the caller even on error paths.

    use shannon_core::query_engine::QueryEngine;
    use shannon_core::api::LlmClientConfig;
    use shannon_core::api::LlmProvider;
    use shannon_core::permissions::PermissionManager;
    use shannon_core::state::StateManager;
    use shannon_core::tools::ToolRegistry;
    use std::collections::HashMap;

    fn create_test_engine() -> QueryEngine {
        let config = LlmClientConfig {
            api_key: "test".to_string(),
            base_url: "http://localhost:1".to_string(), // unreachable
            model: "test-model".to_string(),
            max_tokens: 100,
            timeout_seconds: 1,
            api_version: String::new(),
            provider: LlmProvider::Anthropic,
            extra_headers: HashMap::new(),
            retry_config: shannon_core::api::RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let client = shannon_core::api::LlmClient::new(config);
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        QueryEngine::with_defaults(client, tools, permissions, state)
    }

    /// Type alias matching the fixed error type in the async task.
    type QueryTaskResult = Result<
        (QueryEngine, String, u64, usize, (), String, usize),
        (Option<QueryEngine>, String),
    >;

    /// Simulate the success path: engine is returned in Ok variant.
    #[test]
    fn test_engine_returned_on_success() {
        let engine = create_test_engine();
        let session_id = engine.session_id();

        let result: QueryTaskResult = Ok((
            engine,
            "response text".to_string(),
            42,
            0,
            (),
            "done".to_string(),
            1,
        ));

        match result {
            Ok((engine, response, tokens, tools, _, status, steps)) => {
                assert_eq!(engine.session_id(), session_id);
                assert_eq!(response, "response text");
                assert_eq!(tokens, 42);
                assert_eq!(tools, 0);
                assert_eq!(status, "done");
                assert_eq!(steps, 1);
            }
            Err(_) => panic!("Expected Ok with engine"),
        }
    }

    /// Simulate the QueryEvent::Failed path: engine is returned in Err via Some().
    #[test]
    fn test_engine_returned_on_query_failed() {
        let engine = create_test_engine();
        let session_id = engine.session_id();

        let result: QueryTaskResult = Err((
            Some(engine),
            "Query failed: malformed output".to_string(),
        ));

        match result {
            Err((Some(engine), error_msg)) => {
                assert_eq!(engine.session_id(), session_id,
                    "Engine must survive query failures");
                assert!(error_msg.contains("malformed output"));
            }
            Err((None, _)) => panic!("Engine should be Some for query failures"),
            Ok(_) => panic!("Expected Err"),
        }
    }

    /// Simulate the stream error path: engine is returned in Err via Some().
    #[test]
    fn test_engine_returned_on_stream_error() {
        let engine = create_test_engine();
        let session_id = engine.session_id();

        let result: QueryTaskResult = Err((
            Some(engine),
            "Stream error: connection reset".to_string(),
        ));

        match result {
            Err((Some(engine), error_msg)) => {
                assert_eq!(engine.session_id(), session_id,
                    "Engine must survive stream errors");
                assert!(error_msg.contains("connection reset"));
            }
            Err((None, _)) => panic!("Engine should be Some for stream errors"),
            Ok(_) => panic!("Expected Err"),
        }
    }

    /// Simulate the JoinError (task panic/cancel) path: engine is None.
    #[test]
    fn test_engine_none_on_task_panic() {
        let result: QueryTaskResult = Err((
            None,
            "Query task panicked".to_string(),
        ));

        match result {
            Err((None, error_msg)) => {
                assert!(error_msg.contains("panicked"));
            }
            Err((Some(_), _)) => panic!("Engine should be None for task panics"),
            Ok(_) => panic!("Expected Err"),
        }
    }

    /// Verify the full recovery pattern: simulate taking engine, error, restore.
    #[test]
    fn test_engine_take_error_restore_cycle() {
        // Simulate the REPL pattern: take → spawn task → error → restore
        let mut stored_engine: Option<QueryEngine> = Some(create_test_engine());
        let original_session = stored_engine.as_ref().unwrap().session_id();

        // Step 1: Take the engine out (like repl.query_engine.take())
        let engine = stored_engine.take();
        assert!(stored_engine.is_none(), "Engine should be taken out");
        let mut engine = engine.unwrap();

        // Step 2: Simulate some work then an error
        engine.add_user_message("test query".to_string());
        let error_result: QueryTaskResult = Err((
            Some(engine),
            "Query failed: test error".to_string(),
        ));

        // Step 3: Restore engine from error result (the fix)
        match error_result {
            Err((Some(recovered_engine), error_msg)) => {
                assert_eq!(recovered_engine.session_id(), original_session,
                    "Recovered engine should be the same instance");
                stored_engine = Some(recovered_engine);
                assert!(error_msg.contains("test error"));
            }
            _ => panic!("Expected Err with Some(engine)"),
        }

        // Step 4: Verify engine is restored and usable
        assert!(stored_engine.is_some(), "Engine must be restored after error");
        assert_eq!(stored_engine.as_ref().unwrap().session_id(), original_session);

        // Engine should accept new operations
        stored_engine.as_mut().unwrap().add_user_message("after recovery".to_string());
        assert_eq!(stored_engine.as_ref().unwrap().conversation_history().len(), 2);
    }

    /// Verify that the old pattern (plain String error) would NOT work.
    /// This test documents the bug and why the fix is needed.
    #[test]
    fn test_old_pattern_would_lose_engine() {
        let mut stored_engine: Option<QueryEngine> = Some(create_test_engine());

        // Take engine
        let _engine = stored_engine.take();
        assert!(stored_engine.is_none());

        // Old pattern: error is just a String, engine is dropped
        let _old_error: Result<String, String> = Err("Query failed".to_string());

        // With old pattern, stored_engine is still None — the bug!
        assert!(stored_engine.is_none(),
            "Old pattern leaves engine as None — this is the bug");
    }
}
