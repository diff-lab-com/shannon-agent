//! The `process_query` agent loop and its private machinery, moved verbatim
//! from the former single-file `engine.rs`. The loop is a single deeply
//! nested function and cannot be subdivided move-only, so it lives here
//! wholesale; sibling `impl QueryEngine` blocks stay in `super` (legal for
//! the same type within one crate).

use super::events::{AbortOnDropStream, EventTx, QUERY_EVENT_CHANNEL_CAPACITY};
use super::*;

/// Publish the `Stop` hook trigger (§4.8): fired when the agent finishes
/// responding to a query, immediately before `QueryEvent::Completed` is
/// sent. `should_continue` is always `false` — the engine never uses hook
/// feedback to force an extra turn (exit code 2 semantics are advisory).
fn publish_stop_trigger(bus: &crate::bus::EventBus, tool_calls_count: usize) {
    crate::query_engine::guard_nodes::publish_hook_trigger(
        bus,
        "Stop",
        serde_json::json!({
            "tool_calls_count": tool_calls_count,
            "should_continue": false,
        }),
    );
}

/// Progress sender that forwards tool output lines as `ToolProgress` events.
struct ChannelProgressSender {
    tx: EventTx,
    query_id: Uuid,
    tool_use_id: String,
    tool_name: String,
}

#[async_trait::async_trait]
impl crate::tools::ProgressSender for ChannelProgressSender {
    async fn send(&self, line: &str) {
        send_event!(
            self.tx,
            QueryEvent::ToolProgress {
                query_id: self.query_id,
                tool_use_id: self.tool_use_id.clone(),
                tool_name: self.tool_name.clone(),
                progress: -1.0,
                message: line.to_string(),
            }
        );
    }
}

// ── Tool result entry ──────────────────────────────────────────────

/// A pending tool result waiting to be assembled into an API message.
///
/// Carries the tool's output metadata so the engine can construct rich
/// content blocks (e.g. `ContentBlock::Image`) when the tool returned
/// binary image data.
pub(super) struct ToolResultEntry {
    pub(super) tool_use_id: String,
    pub(super) content: String,
    pub(super) is_error: bool,
    /// Metadata from the tool's `ToolOutput`. Currently only used to
    /// detect image results (`metadata["type"] == "image"`).
    pub(super) metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl ToolResultEntry {
    /// Build the appropriate `ToolResultContent` for this entry.
    ///
    /// For single-image results (detected via `metadata["type"] == "image"`),
    /// returns `ToolResultContent::Multiple` containing a text description
    /// block followed by a `ContentBlock::Image` block so the LLM can
    /// "see" the image.
    ///
    /// For multi-image batch results (`metadata["type"] == "images"`,
    /// produced by the AnalyzeImages tool — C-ImgBatch), expands the
    /// `images[]` payload into interleaved `## <path>` text headings and
    /// `ContentBlock::Image` blocks so the whole batch rides in ONE
    /// tool_result → ONE LLM vision request.
    ///
    /// For everything else, returns `ToolResultContent::Single`.
    pub(super) fn to_tool_result_content(&self) -> Option<ToolResultContent> {
        if self.is_error {
            return Some(ToolResultContent::Single(self.content.clone()));
        }

        let output_type = self
            .metadata
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Multi-image batch results (C-ImgBatch).
        if output_type == "images" {
            return Some(self.multi_image_content());
        }

        // Check if this is an image result from the Read/AnalyzeImage tool.
        let is_image = output_type == "image";

        if is_image {
            let media_type = self
                .metadata
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream");

            // Two metadata conventions carry the base64 payload: the
            // computer tool returns it in `metadata["data"]` with plain text
            // in `content`, while Read/AnalyzeImage return a JSON object in
            // `content` with a `data` field. Prefer the metadata form.
            let base64_data = self
                .metadata
                .get("data")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    serde_json::from_str::<serde_json::Value>(&self.content)
                        .ok()
                        .and_then(|v| v.get("data").and_then(|d| d.as_str()).map(String::from))
                })
                .unwrap_or_default();

            if base64_data.is_empty() {
                // Fallback: couldn't parse, return as text
                return Some(ToolResultContent::Single(self.content.clone()));
            }

            let mut text = match self.metadata.get("file_path").and_then(|v| v.as_str()) {
                Some(path) => format!("Image file: {path} ({media_type})"),
                None => format!("Image ({media_type})"),
            };
            if let (Some(w), Some(h)) = (
                self.metadata.get("width").and_then(|v| v.as_u64()),
                self.metadata.get("height").and_then(|v| v.as_u64()),
            ) {
                text.push_str(&format!(" {w}x{h}"));
            }
            text.push_str("\nThe image content is provided as an image block below.");

            Some(ToolResultContent::Multiple(vec![
                ContentBlock::Text { text },
                ContentBlock::Image {
                    source: ImageSource::base64(media_type, base64_data),
                },
            ]))
        } else {
            Some(ToolResultContent::Single(self.content.clone()))
        }
    }

    /// Expand an AnalyzeImages batch payload (C-ImgBatch) into a single
    /// `ToolResultContent::Multiple` holding, per image, a `## <path>` text
    /// heading followed by an image block — one tool_result, one vision
    /// request with N image parts.
    ///
    /// Wire support: Anthropic passes `tool_result` content blocks through
    /// verbatim (multi-image works). The OpenAI/Ollama/Gemini adapters
    /// currently flatten `tool_result` content to text, so there the model
    /// only sees the text headings without pixels — degradation that already
    /// applied to the single-image path. Follow-up: teach those adapters to
    /// emit multi-image tool results; add a per-batch pixel budget.
    fn multi_image_content(&self) -> ToolResultContent {
        let parsed = serde_json::from_str::<serde_json::Value>(&self.content).ok();
        let images = parsed
            .as_ref()
            .and_then(|v| v.get("images"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if images.is_empty() {
            // Fallback: couldn't parse the batch payload, return as text.
            return ToolResultContent::Single(self.content.clone());
        }

        let prompt = parsed
            .as_ref()
            .and_then(|v| v.get("prompt"))
            .and_then(|p| p.as_str())
            .unwrap_or("");

        let count = images.len();
        let mut intro = format!(
            "Batch of {count} images follows; each image is preceded by a `## <path>` heading. Analyze each in order."
        );
        if !prompt.is_empty() {
            intro.push_str(&format!("\nPrompt for each image: {prompt}"));
        }

        let mut blocks = Vec::with_capacity(1 + images.len() * 2);
        blocks.push(ContentBlock::Text { text: intro });

        let mut emitted = 0usize;
        for (i, img) in images.iter().enumerate() {
            let source = img
                .get("source")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            let media_type = img
                .get("media_type")
                .and_then(|m| m.as_str())
                .unwrap_or("application/octet-stream");
            let data = img.get("data").and_then(|d| d.as_str()).unwrap_or_default();
            if data.is_empty() {
                tracing::warn!(index = i, source, "batch image entry has no data; skipping");
                continue;
            }
            blocks.push(ContentBlock::Text {
                text: format!("## {source}"),
            });
            blocks.push(ContentBlock::Image {
                source: ImageSource::base64(media_type, data),
            });
            emitted += 1;
        }

        if emitted == 0 {
            // No decodable image data — degrade to the raw text payload.
            return ToolResultContent::Single(self.content.clone());
        }

        ToolResultContent::Multiple(blocks)
    }
}

// ── Streaming state machine ────────────────────────────────────────

/// Phase of the streaming response lifecycle within a single turn.
///
/// Replaces the previous flag-based control (`stream_finalized: bool`)
/// with an explicit state that makes transitions self-documenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamingPhase {
    /// Actively receiving content blocks from the SSE stream.
    Receiving,
    /// `MessageDelta` processed with tool calls — response saved to
    /// conversation, will break from stream loop and continue the
    /// outer turn loop to dispatch tool results.
    Finalized,
}

// ── Turn-level stream-death continuation (A8) ─────────────────────────────
//
// DeepSWE smoke RCA (smoke-2 turn 8 / smoke-4 turn 14, three attempts all
// dead; docs/research/pier-adapter-notes-2026-09.md §五): the GLM
// coding-plan gateway hard-cuts a single LLM call at ~6 minutes. With
// thinking=max a long-thinking turn routinely crosses that line, the stream
// dies mid-turn, the engine surfaced a Timeout-class `QueryEvent::Failed`,
// and the whole headless run was lost — every prior turn and tool result
// with it (rc=3, empty patch). The client-level reconnect and the run-level
// A7 restart both replay from scratch and re-enter the same >6min call.
//
// A8 instead continues THE TURN in place: on a timeout-class stream death
// (establishment or mid-stream), re-send with all history and prior tool
// state intact, plus a one-shot continuation nudge from the second attempt
// on. Bounded by `SHANNON_TURN_RETRIES` (default 2, 0 disables — naming
// aligned with the run-level `SHANNON_RUN_RETRIES`); on exhaustion the
// existing failure path runs unchanged so A7 remains the last line.

// P3 cleanup: the A8/A14 constants live in `query_engine::recovery` (single
// source) — `recovery::TURN_CONTINUATION_NUDGE_PROMPT`, `DEFAULT_TURN_RETRIES`,
// `STREAM_IDLE_ESCALATION_CAP_SECS`, `STREAM_IDLE_ESCALATION_FACTOR_BASE`.
// engine.rs references them via `recovery::…`.

/// A14: cap for the stream-idle watchdog budget when the engine escalates
/// it across timeout-class turn continuations. Mirrored from
/// [`crate::query_engine::recovery::STREAM_IDLE_ESCALATION_CAP_SECS`]; kept as a local
/// alias because the idle-budget backstop arithmetic below composes with
/// it directly.
use crate::query_engine::recovery::STREAM_IDLE_ESCALATION_CAP_SECS;

// ── Wrap-up protocol before the final turn (A10) ──────────────────────────
//
// DeepSWE w4: arcane / dynamodb died at the turn limit with the work done
// but nothing committed — exit 2, empty patch (F10,
// docs/deepswe-eval-findings-2026-09.md). A one-shot nudge entering the
// LAST turn tells the model to land its work and summarize. The hard stop
// itself is unchanged: exhausting the budget still ends the run exactly as
// before (the nudge lands work, it does not lie about success).

/// Re-prompt injected once when the upcoming iteration is the final turn.
/// Verbatim from the A10 plan; pinned by test.
pub(super) const WRAP_UP_NUDGE_PROMPT: &str = "Your turn budget is nearly exhausted — this is \
     your final turn. Finish your current work now: if you are working in a git \
     repository, commit your changes; then give a brief summary of what was \
     completed and what remains.";

// R2-6: A8/N-3/A14 ladder helpers live in `query_engine::recovery` so the
// agent loop reads top-down as a pipeline rather than 200 lines of inline
// retry-ladder logic. Call sites use:
//   `recovery::push_turn_continuation_nudge(&mut conversation.messages)`
//   `recovery::provider_error_retryable(&message)`
//   `recovery::turn_retries_max()`
//   `recovery::escalate_stream_idle_override(...)` / `recovery::clear_stream_idle_override(...)`

/// Generate a unified diff preview for a file edit operation.
fn generate_diff_preview(path: &str, old: &str, new: &str) -> String {
    let mut diff = format!("--- {path} (current)\n+++ {path} (proposed)\n");
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Simple line-by-line diff: show removed (-) and added (+) lines
    let max_lines = old_lines.len().max(new_lines.len());
    let mut changes = 0u32;
    let max_changes = 30; // Limit diff output size

    for i in 0..max_lines {
        if changes >= max_changes {
            diff.push_str(&format!("... ({} more lines)\n", max_lines - i));
            break;
        }
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            (Some(o), Some(n)) if o == n => {}
            (Some(_), None) => {
                diff.push_str(&format!("-{}\n", old_lines[i]));
                changes += 1;
            }
            (None, Some(_)) => {
                diff.push_str(&format!("+{}\n", new_lines[i]));
                changes += 1;
            }
            (Some(_), Some(_)) => {
                diff.push_str(&format!("-{}\n", old_lines[i]));
                diff.push_str(&format!("+{}\n", new_lines[i]));
                changes += 2;
            }
            (None, None) => { /* both iterators exhausted — skip */ }
        }
    }

    if changes == 0 && old_lines.len() != new_lines.len() {
        // Length changed but no line-level diff caught
        diff.push_str(&format!(
            "@@ file size changed: {} -> {} lines @@\n",
            old_lines.len(),
            new_lines.len()
        ));
    }

    diff
}

impl QueryEngine {
    /// Process a query with streaming events
    ///
    /// `permission_request_tx` carries interactive approval prompts to the
    /// host. It is intentionally a **bounded** channel
    /// (`PERMISSION_REQUEST_CHANNEL_CAPACITY`, review §P3-6): prompts are
    /// strictly sequential (the engine waits for each response before
    /// continuing), so the small bound only guards against a host that
    /// stopped draining. The per-request response lane is a oneshot (see
    /// [`crate::query_engine::types::PermissionRequest`]).
    pub async fn process_query(
        &self,
        context: QueryContext,
        permission_request_tx: Option<mpsc::Sender<crate::query_engine::types::PermissionRequest>>,
    ) -> QueryStream {
        let query_id = context.query_id;
        // Secret-guard Phase 2 enablement: env first (`SHANNON_SECRET_GUARD
        // =audit|redact`; anything set but unparseable means explicit off),
        // then the `[secret_guard]` config section. Installs once per
        // process; no-op unless explicitly enabled (blueprint §9.6).
        crate::secret_guard::init_from_env_or_config();
        let config = self.config.clone();
        let session_id_for_permissions = context.session_id;

        // Create receiver for events. The channel is **bounded** at
        // [`QUERY_EVENT_CHANNEL_CAPACITY`] (review §P3-6): the producer task
        // below awaits every send, so a stalled consumer applies backpressure
        // to the LLM/tool loop instead of letting the queue grow without
        // bound. The legacy mpsc channel stays as the compatibility facade
        // for TUI/SSE/desktop consumers (§4.8); every broadcast QueryEvent
        // additionally flows through this session's [`EventBus`], whose
        // built-in L0 subscriber mirrors durable rows to
        // `~/.shannon/sessions/<session_id>/events.jsonl` — one dispatch
        // path for distribution and persistence.
        // `SHANNON_SESSION_LOG=off` still disables recording (the tee's own
        // switch). Subscriptions and the L0 writer are mounted at the top of
        // the producer task below so their guards live exactly as long as
        // the query.
        let (tx_raw, rx) = mpsc::channel(QUERY_EVENT_CHANNEL_CAPACITY);
        let session_bus = std::sync::Arc::new(crate::bus::EventBus::new());
        let tx = EventTx::new(tx_raw, session_bus.shared());

        // Get necessary state for the spawned task
        let tools = self.tools.clone();
        let permissions = self.permissions.clone();
        let client_api_key = self.client.api_key().to_string();
        let client_model = self.client.model().to_string();

        // Resolve model aliases in fast_model and plan_model
        let fast_model = self
            .config
            .fast_model
            .as_ref()
            .map(|m| crate::model_registry::resolve_model(m, Some(self.client.provider())));
        let plan_model = self
            .config
            .plan_model
            .as_ref()
            .map(|m| crate::model_registry::resolve_model(m, Some(self.client.provider())));

        // Multi-tier model routing
        let client_model = {
            let query = &context.user_message;
            let complexity = classify_query_complexity(query);

            match complexity {
                QueryComplexity::Simple => {
                    fast_model.as_deref().unwrap_or(&client_model).to_string()
                }
                QueryComplexity::Planning => {
                    plan_model.as_deref().unwrap_or(&client_model).to_string()
                }
                QueryComplexity::Standard => client_model.clone(),
            }
        };
        let client_base_url = self.client.base_url().to_string();
        let client_max_tokens = self.client.max_tokens();
        let client_provider = self.client.provider().clone();
        let self_session_id = self.session_id.to_string();
        // §4.6: the log lives in the sessions container owned by this
        // engine's `StateManager`, so restore/list always find it —
        // `SHANNON_HOME` still wins when set (legacy whole-root override).
        let l0_container = crate::session_log::effective_log_container(self.state.sessions_dir());
        let user_message = context.user_message.clone();
        let user_attachments = context.attachments.clone();
        let attachment_count = user_attachments.len();
        let cost_tracker = self.cost_tracker.clone();
        let hook_manager = self.hook_manager.clone();
        let session_start_emitted = self.session_start_emitted.clone();
        let triggered_routines = self.triggered_routines.clone();
        let context_injector = self.context_injector.clone();
        let plan_mode_active = self.plan_mode_active.clone();
        let effective_max_context_tokens = self.effective_max_context_tokens;

        // Scoped injection (ADR-0010 D2): load ALL of the active project's
        // memories into the prompt rather than search-then-inject-matches.
        // Recall is 100% for the bounded volume a curated layer holds; the
        // model decides relevance. `search()` is retained for the REPL
        // `/memory` command (a deliberate user keyword search). The current
        // user message ranks candidates when the cap forces truncation, and
        // the project key is the session's pinned working directory (the
        // process cwd races between desktop sessions).
        let memory_injection: Option<String> = if let Some(ref mem_store) = self.memory {
            match mem_store.read() {
                Ok(store) => {
                    let project = self.memory_project_key();
                    store.format_for_injection(&project, Some(&user_message))
                }
                Err(_) => None,
            }
        } else {
            None
        };

        // Build structured system prompt with cache breakpoints.
        //
        // Cache policy + assembly logic live in `system_prompt` (A PR-1
        // extraction). This block is now 6 lines: pass the inputs in,
        // read the assembled blocks + plain-string fallback back out.
        let mut assembled = crate::query_engine::system_prompt::build(
            &crate::query_engine::system_prompt::SystemPromptInputs {
                config: &config,
                tools: &tools,
                memory_injection: memory_injection.clone(),
                repo_map_injector: &self.repo_map_injector,
                context_injector: self.context_injector.as_deref(),
                provider: client_provider.clone(),
                user_message: &user_message,
                plan_mode_active: self.is_plan_mode_active(),
            },
        );
        // Secret-guard wiring point 1b (blueprint §9.6, injected-context
        // face): CLAUDE.md / AGENTS.md / repo map / memory / the base prompt
        // ride these blocks, so they must redact like conversation content.
        // Deterministic (I1) keeps the cached stable prefix byte-stable.
        if let Some(ref mut blocks) = assembled.blocks {
            crate::secret_guard::transform_system_blocks(blocks);
        }
        if let Some(ref mut plain) = assembled.plain {
            crate::secret_guard::transform_system_prompt_text(plain);
        }
        let mut system_blocks_opt = assembled.blocks;
        let mut system_prompt = if context.metadata.tools_allowed {
            // Prefer the plain fallback assembled by the system_prompt
            // module (it appends the env block already); fall back to
            // the local minimal prompt for tool-less runs.
            assembled.plain.or_else(|| config.system_prompt.clone())
        } else if client_provider == shannon_engine::api::LlmProvider::Ollama {
            // Ollama models use their own chat templates; a system prompt
            // confuses small/unstable models causing malformed output.
            None
        } else {
            // P1-1 review fix: the tool-less LOCAL fallback must still
            // carry the environment block (cwd/date/platform/git/sandbox)
            // — pre-extraction behavior. build_env_block is what the
            // structured path uses; reuse it so every prompt shape stays
            // in sync.
            let mut local = LOCAL_MODEL_SYSTEM_PROMPT.to_string();
            if let Ok(cwd) = std::env::current_dir() {
                local.push_str(&crate::query_engine::system_prompt::build_env_block(&cwd));
            }
            Some(local)
        };

        // Clone existing conversation to preserve multi-turn context
        let mut conversation = self.conversation.clone();
        tracing::debug!(
            existing_msgs = conversation.messages.len(),
            last_role = conversation
                .messages
                .last()
                .map(|m| m.role.as_str())
                .unwrap_or("none"),
            "Starting new query: cloning conversation for background task"
        );
        // Attachments (e.g. images from the REST API, desktop app, or TUI)
        // switch the user message to content blocks so multimodal adapters
        // can carry them to the provider; text-only queries keep the plain
        // string form.
        let user_content = if user_attachments.is_empty() {
            MessageContent::Text(user_message.clone())
        } else {
            let mut blocks = Vec::with_capacity(user_attachments.len() + 1);
            blocks.push(shannon_engine::api::ContentBlock::Text {
                text: user_message.clone(),
            });
            blocks.extend(user_attachments);
            MessageContent::Blocks(blocks)
        };
        conversation.messages.push(Message {
            role: "user".to_string(),
            content: user_content,
        });

        // Clone memory store for post-query extraction (fire-and-forget)
        let memory_for_extraction = self.memory.clone();
        // The session's pinned project key (owned — the spawned producer
        // must not capture `self`).
        let memory_project_key = self.memory_project_key();
        // Extraction cursor (P0-10 incremental extraction): index into the
        // conversation up to which facts have already been extracted.
        let memory_extract_cursor_cell = self.memory_extract_cursor.clone();
        let memory_extract_cursor_cursor =
            memory_extract_cursor_cell.load(std::sync::atomic::Ordering::Relaxed);

        // Engine-config snapshot embedded in every `request/header` (§4.2),
        // so each logged request is a pure function of the log.
        let header_config_snapshot = serde_json::json!({
            "max_turns": config.max_turns,
            "max_budget_usd": config.max_budget_usd,
            "timeout_seconds": config.timeout_seconds,
            "enable_thinking": config.enable_thinking,
            "effective_max_context_tokens": self.effective_max_context_tokens,
            "effort": config.effort,
            "repo_map_enabled": config.repo_map_enabled,
            "tools_allowed": context.metadata.tools_allowed,
            "temperature": context.metadata.temperature,
            "top_p": context.metadata.top_p,
        });

        // Spawn background task to handle query processing. Its `JoinHandle`
        // is captured by `AbortOnDropStream` below so the task is aborted when
        // the consumer drops the `QueryStream` (the cancellation path).
        // R1-3: capture a clone of the reinjection-provider handle so the
        // producer task can read host-registered providers across the
        // `'static` move boundary.
        let reinjection_providers = self.reinjection_providers.clone();
        // Review §P2-4: plugin-gate decisions (§4.9 route (b)) republish onto
        // THIS query's session bus through a task-scoped sink. The former
        // process-wide sink was overwritten by every query, so with
        // concurrent queries one session's decisions landed in another
        // session's channel. `scope_decision_sink` pins the closure to this
        // producer task only; ending/aborting the query ends the scope, so
        // no cleanup is needed.
        let decision_sink: crate::bus::DecisionSink = {
            let sink_bus = session_bus.shared();
            std::sync::Arc::new(move |frame: &crate::bus::PluginDecisionFrame| {
                use shannon_types::session_event::PermissionDecisionPayload;
                let decision = if frame.allowed { "allow" } else { "deny" };
                let reason = format!(
                    "plugin gate '{}' requires '{}' declared [{}]",
                    frame.point,
                    frame.required,
                    frame.declared.join(", ")
                );
                sink_bus.dispatch(
                    crate::bus::permission_decision_event(PermissionDecisionPayload {
                        tool_name: None,
                        request: Some(format!("plugin '{}'", frame.plugin)),
                        decision: decision.to_string(),
                        reason: Some(reason),
                        mode: Some("PLUGIN".to_string()),
                    })
                    .into(),
                    crate::bus::DispatchMode::Emit,
                );
            })
        };
        let producer = tokio::spawn(crate::bus::scope_decision_sink(decision_sink, async move {
            // Prevent OS sleep during long-running queries (drops on exit)
            let _sleep_guard = crate::prevent_sleep::PreventSleepGuard::new();

            // ---- §4.8: mount the built-in subscriptions on this session's
            // bus. The L0 writer is now a subscriber ("log-as-subscribe"),
            // so in-process distribution and persistence share one path;
            // dropping its guard (task end / abort) closes the log with the
            // same interrupted-turn semantics as before.
            let l0_tee = crate::session_log::TeeHandle::open_in_container(
                &l0_container,
                &self_session_id,
                client_model.as_str(),
                Some(client_provider.to_string().as_str()),
            );
            let _l0_guard = session_bus.subscribe(
                crate::bus::TopicFilter::all(),
                std::sync::Arc::new(crate::session_log::L0TeeSubscriber::new(l0_tee.clone())),
            );
            // §4.8: mount the HookManagerAdapter — it decodes hook triggers
            // published on this session's bus (UserPromptSubmit, PostToolUse,
            // Stop, SessionStart, …) and runs them through the HookManager.
            // Fire-and-forget per trigger (spawned), advisory (decisions are
            // not enforced here), and a no-op when no hooks are configured.
            let _hook_adapter_guard = session_bus.subscribe(
                crate::bus::TopicFilter::kind(
                    shannon_types::session_event::SessionEventKind::Custom,
                ),
                std::sync::Arc::new(crate::query_engine::guard_nodes::HookManagerAdapter::new(
                    hook_manager.clone(),
                    session_bus.shared(),
                )),
            );

            // §4.8: SessionStart — once per engine session, at the start of
            // the first query. Hosts that fire it themselves at startup (the
            // REPL) pre-mark the flag so this stays silent for them.
            if !session_start_emitted.swap(true, std::sync::atomic::Ordering::Relaxed) {
                crate::query_engine::guard_nodes::publish_hook_trigger(
                    &session_bus,
                    "SessionStart",
                    serde_json::json!({ "session_id": self_session_id }),
                );
            }
            let tee = l0_tee;

            // §4.2: open the L0 record for this query — the user message and
            // turn boundary precede anything the model sees.
            tee.record_user_message_with_count(&user_message, attachment_count);
            tee.record_turn_start(Some(query_id.to_string()));

            // Fire UserPromptSubmit hook — §4.8: as a bus trigger picked up
            // by the HookManagerAdapter subscription (results unused here,
            // same as the pre-bus direct call).
            crate::query_engine::guard_nodes::publish_hook_trigger(
                &session_bus,
                "UserPromptSubmit",
                serde_json::json!({ "prompt": user_message.clone() }),
            );

            // Create a new client for this task, preserving provider from original config
            let client_config = {
                // For Ollama models with tiny context (< 4096), cap num_predict
                // to half the context so the model has room for input tokens.
                let capped_max_tokens = if client_provider
                    == shannon_engine::api::LlmProvider::Ollama
                    && effective_max_context_tokens < 4096
                    && client_max_tokens as usize > effective_max_context_tokens / 2
                {
                    tracing::info!(
                        max_tokens = effective_max_context_tokens / 2,
                        "Capping num_predict for tiny Ollama model"
                    );
                    (effective_max_context_tokens / 2) as u32
                } else {
                    client_max_tokens
                };
                let mut cfg = shannon_engine::api::LlmClientConfig {
                    api_key: client_api_key,
                    base_url: client_base_url,
                    model: client_model.clone(),
                    max_tokens: capped_max_tokens,
                    provider: client_provider.clone(),
                    ..Default::default()
                };
                // Enable extended thinking with a default budget if configured
                // (legacy `enable_thinking` knob; an explicit effort level
                // below overrides it).
                if config.enable_thinking {
                    cfg.budget_tokens = Some(10000);
                }
                // Effort dial: High/Max enable extended thinking through the
                // existing budget plumbing — Anthropic-style providers get an
                // explicit `budget_tokens` (8k/16k), OpenAI-style providers get
                // `reasoning_effort: high`. Standard (the default) sends
                // nothing, byte-identical to the pre-dial behavior.
                if let Some(budget) = config.effort.thinking_budget() {
                    cfg.budget_tokens = Some(budget);
                    // Extended thinking spends out of `max_tokens`: raise it so
                    // it stays above the thinking budget with visible-answer
                    // headroom instead of starving the reply.
                    let needed = budget.saturating_add(EFFORT_THINKING_HEADROOM_TOKENS);
                    if cfg.max_tokens < needed {
                        cfg.max_tokens = needed;
                    }
                    if !matches!(
                        cfg.provider,
                        shannon_engine::api::LlmProvider::Anthropic
                            | shannon_engine::api::LlmProvider::Bedrock
                            | shannon_engine::api::LlmProvider::Custom
                    ) {
                        cfg.reasoning_effort =
                            Some(shannon_engine::api::types::ReasoningEffort::High);
                    }
                }
                cfg
            };
            // Attach the request observer: every adapter-serialized request
            // body is teed into the L0 log verbatim as a `request/header`
            // (§4.2). Taking the adapter's own product (rather than
            // re-serializing) guarantees byte-identity with the wire request.
            let client = LlmClient::new(client_config).with_request_capture({
                let tee = tee.clone();
                let header_model = client_model.clone();
                let header_provider = client_provider.to_string();
                let snapshot = header_config_snapshot.clone();
                std::sync::Arc::new(move |wire: &serde_json::Value| {
                    // Phase-0 secret-guard audit: read-only, no-op unless a
                    // transform is installed via `secret_guard::set_context_transform`.
                    crate::secret_guard::audit_wire_and_log(wire);
                    tee.record_request_header(
                        wire,
                        &header_model,
                        Some(&header_provider),
                        snapshot.clone(),
                    );
                })
            });

            let mut turn = 0;
            let mut tool_results: Vec<ToolResultEntry> = Vec::new();
            // Runtime notices (permission soft-limit warnings, auto-test
            // results) that travel as plain user text — never as tool_result
            // blocks, whose synthetic ids would reference no assistant
            // ToolUse and draw a provider 400 ("unexpected tool_use_id").
            let mut user_notices: Vec<String> = Vec::new();
            let mut total_input_tokens: u64 = 0;
            let mut total_output_tokens: u64 = 0;
            let mut file_edits_made = false;
            // P-B: session-scoped file-edits flag. file_edits_made resets every
            // turn; session_file_edits_made sticks for the lifetime of this
            // query so the turn-N checkpoint can decide whether to inject.
            let mut session_file_edits_made = false;
            // P-B: fires once per query when the checkpoint turn is reached.
            let mut turn_checkpoint_fired = false;
            // A10: fires once per query when the upcoming iteration is the
            // final turn. The A8 continuation re-enters the same turn
            // without advancing the counter, so the flag — not the turn
            // arithmetic — is what keeps the nudge one-shot.
            let mut wrap_up_nudge_fired = false;
            // P-M: fires once per threshold per query (60% and 80% independently).
            let mut token_warning_60_fired = false;
            let mut token_warning_80_fired = false;
            let mut compaction_failures: u32 = 0;
            // Micro-compaction (tool-result clearing) fires once per query.
            let mut micro_pruned_fired = false;
            const MAX_COMPACTION_FAILURES: u32 = 2;

            // Denial circuit breaker: track consecutive permission denials.
            // After MAX_CONSECUTIVE_DENIALS the model is told to stop retrying;
            // if it still retries HARD_LIMIT more times, the loop aborts.
            let mut consecutive_denials: u32 = 0;
            const DENIAL_SOFT_LIMIT: u32 = 3; // inject warning to LLM
            const DENIAL_HARD_LIMIT: u32 = 5; // abort the agent loop

            // Truncation continuations: a response cut off by the output
            // token limit (stop_reason `length`/`max_tokens`) before any
            // tool call is re-prompted instead of ending the query. Bounded
            // independently of max_turns so a model that always overruns
            // its output budget cannot monopolize the loop.
            let mut truncation_continuations: u32 = 0;
            const MAX_TRUNCATION_CONTINUATIONS: u32 = 5;

            // Think-only continuation nudge (A1, see the constants near
            // `THINK_ONLY_NUDGE_PROMPT`): a response with no tool calls and
            // no substantive answer is re-prompted instead of ending the
            // query as a silent no-op. Bounded per query — the counter
            // increments on every nudge and resets whenever a real tool turn
            // completes, so only *consecutive* think-only responses exhaust
            // the budget.
            let mut think_only_nudges: u32 = 0;
            let max_think_only_nudges = think_only_nudge_max();
            let think_only_min_chars = think_only_min_answer_chars();

            // Auto-test loop state (P1-5). Initialized lazily inside the loop body
            // because `AutoLoopState` is only needed when `config.auto_test` is `Some`.
            // We keep the struct default-constructible so this declaration is cheap.
            let mut auto_test_state: crate::auto_test::AntiLoopState =
                crate::auto_test::AntiLoopState::new();

            // P3-10: query-scoped dedup of `tool_use_id` echoes. Each per-
            // stream HashSet below only sees one HTTP response; but the
            // agent loop runs many API calls per query (default max_turns=20)
            // and a misbehaving provider/model can replay the same
            // `tool_use_id` across turns. Anthropic rejects duplicate
            // tool_use_ids with HTTP 400 (`duplicate tool_call id`), so
            // re-emitting them breaks the next request. Track every id we
            // have already emitted this query and drop later duplicates.
            let mut seen_tool_use_ids_query: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            // A8: per-turn budget of timeout-class continuation retries.
            // Incremented when a dead LLM call is re-sent in place; reset
            // whenever a stream completes normally so every fresh turn gets
            // its own budget. A retry does NOT consume the max_turns budget
            // (the turn is re-entered, not advanced).
            let mut turn_retries_used: u32 = 0;
            let max_turn_retries = recovery::turn_retries_max();

            // A14: base stream-idle budget, read once per query from the
            // engine-side SHANNON_STREAM_IDLE_SECS env (default 420s, the
            // same constant the streaming layer reads directly — we duplicate
            // the lookup here so we can compute escalations without poking
            // engine internals from the test suite).
            let stream_idle_base_secs = std::env::var("SHANNON_STREAM_IDLE_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v > 0);

            'agent_loop: loop {
                if turn >= config.max_turns {
                    let total_cost = CostTracker::calculate_cost(
                        &client_model,
                        total_input_tokens,
                        total_output_tokens,
                    );
                    send_event!(
                        tx,
                        QueryEvent::Cost {
                            query_id,
                            total_cost_usd: total_cost,
                            input_tokens: total_input_tokens,
                            output_tokens: total_output_tokens,
                        }
                    );
                    send_event!(
                        tx,
                        QueryEvent::ConversationUpdate {
                            query_id,
                            messages: conversation.messages.clone(),
                        }
                    );
                    publish_stop_trigger(&session_bus, tool_results.len());
                    send_event!(tx, QueryEvent::Completed { query_id });

                    break;
                }

                // Build messages for API call
                let mut messages = conversation.messages.clone();

                // Add pending tool results from previous turn.
                // Persist to conversation.messages as well so multi-turn context
                // maintains the required assistant(tool_use) → user(tool_result) sequence.
                for entry in tool_results.drain(..) {
                    let content = entry.to_tool_result_content();
                    let tool_msg = Message {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id: entry.tool_use_id,
                            content,
                            is_error: Some(entry.is_error),
                        }]),
                    };
                    messages.push(tool_msg.clone());
                    conversation.messages.push(tool_msg);
                }

                // Drain runtime notices as a plain user text message. They must
                // land AFTER the tool_result drain (same pairing constraint as
                // the turn-N checkpoint below): a synthetic user message before
                // `user(tool_result)` violates the assistant(tool_use) →
                // user(tool_result) API contract.
                if !user_notices.is_empty() {
                    let notice_msg = Message {
                        role: "user".to_string(),
                        content: MessageContent::Text(user_notices.join("\n\n")),
                    };
                    messages.push(notice_msg.clone());
                    conversation.messages.push(notice_msg);
                    user_notices.clear();
                }

                // ── P-B: turn-N checkpoint ─────────────────────────────────
                // Fires ONCE per query when `turn` (the count of completed
                // turns, incremented at the bottom of each iteration) first
                // reaches the configured checkpoint AND Edit/Write hasn't
                // been called this session. Forces the agent out of an
                // explore-only loop (SWE-bench context_thrash mode).
                // Production default = off (opt-in via SHANNON_TURN_CHECKPOINT).
                //
                // Placed AFTER tool_results drain so the synthetic user message
                // lands after `user(tool_result)` in the conversation —
                // inserting it before would produce
                // `assistant(tool_use) → user(synthetic) → user(tool_result)`
                // which violates the Anthropic API contract and was the root
                // cause of 16/50 batch-10 "invalid params 400 (2013)" errors
                // (every failing task died at exactly the checkpoint turn
                // with no Edit yet recorded).
                if let Some(checkpoint) = config.turn_checkpoint_turn {
                    let turn_u32 = turn as u32;
                    if !turn_checkpoint_fired && turn_u32 >= checkpoint && !session_file_edits_made
                    {
                        tracing::info!(
                            turn = turn_u32,
                            checkpoint,
                            "turn-N checkpoint fired — injecting commit-now reminder"
                        );
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: format!(
                                    "Turn checkpoint {turn_u32} reached without file edits — \
                                     injecting commit reminder"
                                ),
                            }
                        );
                        let reminder = format!(
                            "[Turn {turn_u32} reminder] You have used {turn_u32} of your turn \
                             budget and have NOT yet called Edit or Write. STOP exploring and \
                             commit a fix now — a wrong or partial fix is better than an empty \
                             patch. The official harness will judge correctness; you do not \
                             need to verify locally."
                        );
                        let synth_msg = Message {
                            role: "user".to_string(),
                            content: MessageContent::Text(reminder),
                        };
                        messages.push(synth_msg.clone());
                        conversation.messages.push(synth_msg);
                        turn_checkpoint_fired = true;
                    }
                }

                // ── A10: wrap-up protocol entering the final turn ──────────
                // Fires ONCE per query when the upcoming iteration is the
                // last one (`turn + 1 == max_turns`): the model is told to
                // land its work (commit) and summarize. w4 evidence: arcane
                // / dynamodb hit the turn limit with the work done but
                // nothing committed — exit 2, empty patch (F10,
                // docs/deepswe-eval-findings-2026-09.md; backlog A10). The
                // hard stop below is UNCHANGED — exhausting the budget still
                // ends the run exactly as before; the nudge lands work, it
                // does not lie about success. Placed after the tool_results
                // drain for the same wire-order reason as the P-B checkpoint
                // above (synthetic user message must follow user(tool_result)).
                // One-shot by flag: the A8 continuation re-enters this same
                // iteration without advancing `turn`, and must not stack a
                // second wrap-up nudge.
                if !wrap_up_nudge_fired && turn + 1 == config.max_turns {
                    wrap_up_nudge_fired = true;
                    tracing::info!(
                        turn,
                        max_turns = config.max_turns,
                        "final-turn wrap-up nudge injected"
                    );
                    send_event!(
                        tx,
                        QueryEvent::Progress {
                            query_id,
                            message: format!(
                                "Agent turn budget nearly exhausted — final turn \
                                 ({}/{}): wrap up, commit your work, and summarize.",
                                turn + 1,
                                config.max_turns
                            ),
                        }
                    );
                    let wrap_up_msg = Message {
                        role: "user".to_string(),
                        content: MessageContent::Text(WRAP_UP_NUDGE_PROMPT.to_string()),
                    };
                    messages.push(wrap_up_msg.clone());
                    conversation.messages.push(wrap_up_msg);
                }

                // Resolve effective max context FIRST: Ollama num_ctx > model registry > fallback.
                // Run every turn — check_ollama_capabilities() caches results after the first
                // HTTP call, so subsequent turns just read the cache with zero overhead.
                let mut effective_max_context = effective_max_context_tokens;
                if client_provider == shannon_engine::api::LlmProvider::Ollama
                    && config.max_context_tokens.is_none()
                {
                    if let Some(info) = client.check_ollama_capabilities().await {
                        if info.num_ctx > 0 {
                            effective_max_context = info.num_ctx;
                        }
                        tracing::debug!(
                            num_ctx = effective_max_context,
                            turn,
                            "Ollama context resolved"
                        );
                    }
                }

                // Get tools schema — respect tools_allowed from QueryContext.
                // For Ollama models with small context (< 8192), tool definitions
                // consume too much of the context window and crowd out conversation
                // history, causing multi-turn context loss. Auto-disable tools.
                // Also replace the full system prompt with a minimal one to free up
                // context for actual conversation.
                let mut tools_schema = if context.metadata.tools_allowed {
                    let mut tool_defs = tools.to_tool_definitions();
                    // Secret-guard wiring point 1c: descriptions redact;
                    // names/input schemas stay verbatim (the model must
                    // reproduce them exactly for calls to parse).
                    crate::secret_guard::transform_tool_definitions(&mut tool_defs);
                    if client_provider == shannon_engine::api::LlmProvider::Ollama
                        && effective_max_context < 8192
                    {
                        tracing::info!(
                            num_ctx = effective_max_context,
                            "Ollama model has small context, auto-disabling tools to preserve conversation history"
                        );
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: "Tools disabled (model context under 8K)".to_string(),
                            }
                        );
                        // Replace full system prompt with minimal one to free context
                        system_blocks_opt = None;
                        system_prompt = Some("You are a helpful assistant.".to_string());
                        None
                    } else {
                        Some(tool_defs)
                    }
                } else {
                    None
                };

                // Auto-compress conversation if it exceeds the threshold
                {
                    let estimated_tokens =
                        shannon_engine::compact::helpers::estimate_tokens(&messages)
                            + config
                                .system_prompt
                                .as_ref()
                                .map(|sp| {
                                    shannon_engine::compact::helpers::estimate_text_tokens(sp)
                                })
                                .unwrap_or(0);
                    let max_context = effective_max_context.max(1); // Guard against division by zero
                    let mut estimated_tokens = estimated_tokens;
                    let mut usage_ratio = estimated_tokens as f32 / max_context as f32;

                    // Pre-compaction warning at 60% — gives users visibility before compression fires.
                    // P-M: also inject a SYNTHETIC USER MESSAGE at 60% so the model itself
                    // sees the warning (not just the TUI/headless observer). Production
                    // default = on; opt-out via SHANNON_TOKEN_BUDGET_WARNING=false.
                    if usage_ratio > 0.6 && usage_ratio <= config.compression_threshold {
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: format!(
                                    "Context: {:.0}% full ({}/{}) — compaction will trigger at {:.0}%",
                                    usage_ratio * 100.0,
                                    estimated_tokens,
                                    max_context,
                                    config.compression_threshold * 100.0,
                                ),
                            }
                        );
                        if config.token_budget_warning && !token_warning_60_fired {
                            let pct = (usage_ratio * 100.0).round() as u32;
                            let reminder = format!(
                                "[Token budget at {pct}%] ~{pct}% of the context window used \
                                 ({estimated_tokens}/{max_context} tokens). Focus on completing \
                                 the task — wrap up exploration, commit a fix, and stop re-reading \
                                 the same code."
                            );
                            let synth_msg = Message {
                                role: "user".to_string(),
                                content: MessageContent::Text(reminder),
                            };
                            // Push to BOTH `messages` (the API payload) and
                            // `conversation.messages` so the sync at line ~2040
                            // doesn't drop it. Pre-fix this only pushed to
                            // conversation.messages, where it was overwritten
                            // by the sync — meaning the warning never reached
                            // the model.
                            messages.push(synth_msg.clone());
                            conversation.messages.push(synth_msg);
                            token_warning_60_fired = true;
                            tracing::info!(pct, "P-M 60% token-budget synthetic message fired");
                        }
                    }

                    // P-M: 80% warning (pre-compaction imminent). Fires once.
                    // Distinct from the existing USD-budget warning at line ~2457
                    // (CostTracker budget_warned) — this is *token-usage* against the
                    // context window, which is what SWE-bench agents actually need.
                    if usage_ratio > 0.8 && config.token_budget_warning && !token_warning_80_fired {
                        let pct = (usage_ratio * 100.0).round() as u32;
                        let reminder = format!(
                            "[Token budget at {pct}%] ~{pct}% of the context window used \
                             ({estimated_tokens}/{max_context} tokens). Compaction is imminent. \
                             Finalize your fix NOW — write the patch, do not start new exploration."
                        );
                        let synth_msg = Message {
                            role: "user".to_string(),
                            content: MessageContent::Text(reminder),
                        };
                        // Same dual-push fix as the 60% warning — see comment
                        // there. Without pushing to `messages`, the sync at
                        // line ~2040 silently drops the synthetic.
                        messages.push(synth_msg.clone());
                        conversation.messages.push(synth_msg);
                        token_warning_80_fired = true;
                        tracing::info!(pct, "P-M 80% token-budget synthetic message fired");
                    }

                    // B.6: SHANNON_TOKEN_BUDGET watchdog. Different from the
                    // 60%/80% ratio warnings above — that pair is keyed to
                    // the model's context window (which is provider/model
                    // specific). The B.6 budget is keyed to a caller-supplied
                    // cap (env `SHANNON_TOKEN_BUDGET`, default 0 = off; eval
                    // recommends 120_000). When the cumulative input-token
                    // total crosses the cap the engine nudges the model
                    // toward targeted reads (`Grep` / `head -c` / Read with
                    // offset+limit) instead of full-file reads.
                    //
                    // Fires on EVERY turn where the cap is exceeded (unlike
                    // the once-per-query ratio warnings) because the model
                    // may still default to `cat` until it sees the reminder
                    // in the current turn's user-context.
                    let budget = token_budget_limit();
                    if budget > 0 && total_input_tokens > budget {
                        if let Some(text) = token_budget_nudge_for(total_input_tokens, budget) {
                            let synth_msg = Message {
                                role: "user".to_string(),
                                content: MessageContent::Text(text.clone()),
                            };
                            messages.push(synth_msg.clone());
                            conversation.messages.push(synth_msg);
                            tracing::info!(
                                used = total_input_tokens,
                                budget,
                                turn,
                                "B.6 SHANNON_TOKEN_BUDGET synthetic message fired",
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: format!(
                                        "Token budget exceeded ({total_input_tokens} > {budget});                                          injecting targeted-read nudge"
                                    ),
                                }
                            );
                        }
                    }

                    // Micro-compaction (tool-result clearing): once per query,
                    // above MICRO_PRUNE_THRESHOLD but BEFORE lossy full
                    // compaction, shrink stale tool results in older turns to
                    // 200-char previews (error results kept in full). This is
                    // the cheapest context win and often avoids summarization
                    // entirely. Pruning edits content in place — no messages
                    // are removed, so tool_use/tool_result pairing is safe;
                    // the unconditional sync below persists it.
                    // A PR-2 wiring: gate via context_policy (single source).
                    if context_policy::should_micro_prune(usage_ratio, micro_pruned_fired) {
                        micro_pruned_fired = true;
                        let keep = config.keep_recent_messages.min(messages.len());
                        let head = messages.len() - keep;
                        let before = estimated_tokens;
                        shannon_engine::compact::CompactEngine::prune_stale_tool_results(
                            &mut messages[..head],
                        );
                        // A PR-2 wiring: re-estimation via context_policy
                        // (single source for the tokens+system-prompt sum).
                        estimated_tokens =
                            context_policy::reestimate(&messages, config.system_prompt.as_deref());
                        usage_ratio = estimated_tokens as f32 / max_context as f32;
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: format!(
                                    "Cleared stale tool results from older turns: ~{before} → ~{estimated_tokens} tokens (context now {:.0}% full)",
                                    usage_ratio * 100.0,
                                ),
                            }
                        );
                    }

                    // A PR-2 wiring: the compact-vs-truncate decision comes
                    // from `context_policy::evaluate` (single source for the
                    // threshold ladder). Semantics identical to the original
                    // inline ladder: below the threshold nothing happens even
                    // when the breaker is saturated; above it the breaker
                    // picks truncate-vs-compact.
                    let action = context_policy::evaluate(
                        usage_ratio,
                        compaction_failures,
                        MAX_COMPACTION_FAILURES,
                        config.compression_threshold,
                    );
                    if matches!(
                        action,
                        ContextAction::Compact | ContextAction::TruncateFallback
                    ) {
                        // Circuit breaker: if compaction has failed repeatedly, skip it and just truncate
                        if action == ContextAction::TruncateFallback {
                            let keep = config.keep_recent_messages;
                            if messages.len() > keep {
                                // Pair-aware: never split a tool_use/tool_result pair
                                // when cutting history (an orphaned half is rejected by
                                // providers with a 400).
                                let split = shannon_engine::compact::safe_split_point(
                                    &messages,
                                    messages.len() - keep,
                                );
                                messages = messages.split_off(split);
                            }
                            send_event!(tx, QueryEvent::Progress {
                                query_id,
                                message: "Compaction skipped (too many failures), truncating old messages".to_string(),
                            });
                        } else {
                            // P2-1 multi-strategy selector: choose between the
                            // LLM-backed path (preserves high-fidelity summary)
                            // and the cheaper token-based path (greedy drop-oldest)
                            // based on the conversation profile. TokenDense or
                            // small history -> token-based; otherwise the LLM
                            // summarizer is preferred.
                            let selector = p2_compact::default_selector();
                            let decision = selector.recommend(&messages, effective_max_context);
                            let wants_token_only =
                                matches!(decision.strategy, p2_compact::Strategy::TokenBased);

                            if wants_token_only {
                                // Greedy drop-oldest — never blocks on an LLM
                                // call. P2-1 contract: summary_path_or_local
                                // guarantees a fallback even when the LLM is
                                // unavailable.
                                let p2_policy = p2_compact::Policy {
                                    keep_recent: config.keep_recent_messages,
                                    ..p2_compact::Policy::default()
                                };
                                let outcome = p2_compact::maybe_compact_with_policy(
                                    &messages,
                                    effective_max_context,
                                    p2_policy,
                                );
                                let reduction = outcome.reduction_ratio();
                                let compacted_vec = outcome.compacted.clone();
                                let removed = messages.len() - compacted_vec.len();
                                let original = outcome.original_tokens;
                                let compacted_tok = outcome.compacted_tokens;
                                if outcome.did_compact {
                                    messages = compacted_vec;
                                    send_event!(
                                        tx,
                                        QueryEvent::Progress {
                                            query_id,
                                            message: format!(
                                                "Context compacted (token-based): {} → {} tokens ({:.0}% reduction, {} messages removed)",
                                                original,
                                                compacted_tok,
                                                reduction * 100.0,
                                                removed,
                                            ),
                                        }
                                    );
                                    // Re-inject critical context after compaction
                                    // so the model retains project instructions.
                                    let mut reinjection = context_injector
                                        .as_ref()
                                        .map(|ci| ci.reinjection_context())
                                        .unwrap_or_default();
                                    // Curated project memories must survive the
                                    // compaction boundary too — system blocks are
                                    // rebuilt next query, but the compacted
                                    // history loses them until then.
                                    if let Some(mem) = memory_injection.as_ref() {
                                        if !mem.is_empty() {
                                            if !reinjection.is_empty() {
                                                reinjection.push_str("\n\n");
                                            }
                                            reinjection.push_str(mem);
                                        }
                                    }
                                    // R1-3: host-registered providers (todo
                                    // checklist, skill activations, etc.).
                                    for provider in reinjection_providers
                                        .lock()
                                        .expect("reinjection_providers lock")
                                        .iter()
                                        .cloned()
                                    {
                                        if let Some(block) = provider() {
                                            if block.is_empty() {
                                                continue;
                                            }
                                            if !reinjection.is_empty() {
                                                reinjection.push_str("\n\n");
                                            }
                                            reinjection.push_str(&block);
                                        }
                                    }
                                    if !reinjection.is_empty() && !messages.is_empty() {
                                        let ctx_msg = shannon_engine::api::Message {
                                            role: "system".to_string(),
                                            content: shannon_engine::api::MessageContent::Text(
                                                format!(
                                                    "[Re-injected context after compaction]\n\n{reinjection}"
                                                ),
                                            ),
                                        };
                                        messages.insert(0, ctx_msg);
                                    }
                                    compaction_failures = 0;
                                } else {
                                    // Selector said token-based but no progress
                                    // possible (e.g. all-system messages) — count
                                    // as failure so the circuit breaker engages.
                                    compaction_failures += 1;
                                }
                                // No `continue` here: fall through to the
                                // conversation sync below so the compaction result
                                // is not discarded by the top-of-loop re-clone
                                // (which previously caused a compaction livelock:
                                // the threshold check re-fired every turn with the
                                // compaction silently dropped).
                            }

                            match shannon_engine::compact::CompactEngine::with_llm_summarizer(
                                client.clone(),
                            ) {
                                Ok(mut compact_engine) => {
                                    // Sync compact engine's context limit with our effective limit
                                    compact_engine.config.max_context_tokens =
                                        effective_max_context;
                                    // Build re-injection context from ContextInjector if available,
                                    // otherwise fall back to the system prompt (truncated).
                                    // Build re-injection context from ContextInjector if available
                                    let mut reinjection = context_injector
                                        .as_ref()
                                        .map(|ci| ci.reinjection_context())
                                        .unwrap_or_default();
                                    // Curated project memories survive the
                                    // compaction boundary too.
                                    if let Some(mem) = memory_injection.as_ref() {
                                        if !mem.is_empty() {
                                            if !reinjection.is_empty() {
                                                reinjection.push_str("\n\n");
                                            }
                                            reinjection.push_str(mem);
                                        }
                                    }

                                    // Secret-guard (blueprint: redaction must
                                    // precede the compaction request): the
                                    // summarizer sends `messages` verbatim to
                                    // the LLM, so transform first — the wire
                                    // must carry surrogates only (pinned by
                                    // `compaction_summarizer_wire_carries_surrogates_not_secrets`).
                                    // The compacted result is surrogate-
                                    // consistent, matching the overflow-retry
                                    // history sync (§5.4).
                                    let taken = std::mem::take(&mut messages);
                                    messages =
                                        crate::secret_guard::transform_outgoing_messages(taken);
                                    match compact_engine.compact(&mut messages) {
                                        Ok(result) => {
                                            compaction_failures = 0; // reset on success

                                            // Re-inject critical context after compaction so
                                            // the model retains project instructions, MEMORY.md,
                                            // and preference memory across the compaction boundary.
                                            if !reinjection.is_empty() && !messages.is_empty() {
                                                let ctx_msg = shannon_engine::api::Message {
                                                    role: "system".to_string(),
                                                    content:
                                                        shannon_engine::api::MessageContent::Text(
                                                            format!(
                                                                "[Re-injected context after compaction]\n\n{reinjection}"
                                                            ),
                                                        ),
                                                };
                                                messages.insert(0, ctx_msg);
                                            }

                                            send_event!(
                                                tx,
                                                QueryEvent::Progress {
                                                    query_id,
                                                    message: format!(
                                                        "Context compressed (3-tier): {} → {} tokens ({:.0}% reduction, {} messages compacted)",
                                                        result.original_tokens,
                                                        result.compacted_tokens,
                                                        result.reduction_ratio * 100.0,
                                                        result.messages_compacted,
                                                    ),
                                                }
                                            );
                                            send_event!(
                                                tx,
                                                QueryEvent::Info {
                                                    query_id,
                                                    message: format!(
                                                        "compaction: {} → {} tokens ({:.0}% reduction, {} removed, {} compacted, {:?})",
                                                        result.original_tokens,
                                                        result.compacted_tokens,
                                                        result.reduction_ratio * 100.0,
                                                        result.messages_removed,
                                                        result.messages_compacted,
                                                        result.strategy,
                                                    ),
                                                }
                                            );
                                        }
                                        Err(e) => {
                                            compaction_failures += 1;
                                            tracing::warn!(
                                                "Compression failed: {}, truncating instead",
                                                e
                                            );
                                            let keep = 20;
                                            if messages.len() > keep {
                                                let split =
                                                    shannon_engine::compact::safe_split_point(
                                                        &messages,
                                                        messages.len() - keep,
                                                    );
                                                messages = messages.split_off(split);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    compaction_failures += 1;
                                    tracing::warn!(
                                        "CompactEngine init failed ({}), truncating old messages",
                                        e
                                    );
                                    let keep = 20;
                                    if messages.len() > keep {
                                        let split = shannon_engine::compact::safe_split_point(
                                            &messages,
                                            messages.len() - keep,
                                        );
                                        messages = messages.split_off(split);
                                    }
                                }
                            }
                        }
                    }
                }

                // Sync: always keep conversation.messages in sync with the messages
                // actually sent to the API. Compact may produce same-count but
                // different-content messages, so sync unconditionally.
                if conversation.messages.len() != messages.len() {
                    tracing::debug!(
                        before = conversation.messages.len(),
                        after = messages.len(),
                        "Syncing conversation.messages with compressed messages"
                    );
                }
                conversation.messages = messages.clone();

                // Diagnostic: log conversation state before API call
                tracing::info!(
                    msg_count = messages.len(),
                    estimated_tokens = shannon_engine::compact::helpers::estimate_tokens(&messages),
                    turn = turn + 1,
                    max_turns = config.max_turns,
                    "Sending API request"
                );

                // Pre-send context overflow detection: estimate total tokens including
                // tools, warn if near limit, and auto-strip tools for Ollama to preserve
                // conversation history.
                {
                    let tools_tokens = tools_schema
                        .as_ref()
                        .map(|t| {
                            // Rough estimate: ~4 chars per token for JSON tool definitions
                            let json_len = serde_json::to_string(t).map(|s| s.len()).unwrap_or(0);
                            json_len / 4
                        })
                        .unwrap_or(0);
                    let mut pre_send_estimate =
                        shannon_engine::compact::helpers::estimate_tokens(&messages)
                            + config
                                .system_prompt
                                .as_ref()
                                .map(|sp| {
                                    shannon_engine::compact::helpers::estimate_text_tokens(sp)
                                })
                                .unwrap_or(0)
                            + tools_tokens;
                    let mut pre_send_ratio =
                        pre_send_estimate as f32 / effective_max_context as f32;
                    if pre_send_ratio > 0.9 {
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: format!(
                                    "Context at {:.0}% ({}/{} tokens) — approaching limit",
                                    pre_send_ratio * 100.0,
                                    pre_send_estimate,
                                    effective_max_context,
                                ),
                            }
                        );
                        tracing::warn!(
                            estimated_tokens = pre_send_estimate,
                            tools_tokens,
                            max_context = effective_max_context,
                            "Sending request near context limit"
                        );
                        // Auto-strip tools for Ollama when context overflow detected —
                        // preserving conversation history is more important than tool support.
                        if client_provider == shannon_engine::api::LlmProvider::Ollama
                            && tools_schema.is_some()
                            && tools_tokens > 0
                        {
                            tracing::info!(
                                "Auto-stripping tools to preserve conversation context for Ollama"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: "Auto-disabling tools — context near limit"
                                        .to_string(),
                                }
                            );
                            tools_schema = None;
                            pre_send_estimate -= tools_tokens;
                            pre_send_ratio =
                                pre_send_estimate as f32 / effective_max_context as f32;
                        }
                        // For Ollama still over limit: strip system prompt/blocks
                        // to free context for conversation history.
                        if pre_send_ratio > 0.95
                            && client_provider == shannon_engine::api::LlmProvider::Ollama
                        {
                            if system_blocks_opt.is_some() {
                                tracing::info!(
                                    "Stripping system blocks for Ollama — context near limit"
                                );
                                send_event!(
                                    tx,
                                    QueryEvent::Progress {
                                        query_id,
                                        message: "Stripping system context — context near limit"
                                            .to_string(),
                                    }
                                );
                                system_blocks_opt = None;
                            } else if system_prompt.is_some() {
                                tracing::info!(
                                    "Stripping system prompt for Ollama — context near limit"
                                );
                                system_prompt = None;
                            }
                        }
                        // If still over limit after stripping tools, truncate older messages
                        // to fit within context. Keep the most recent turns.
                        if pre_send_ratio > 1.0 && messages.len() > 2 {
                            let target_tokens = (effective_max_context as f32 * 0.8) as usize;
                            while shannon_engine::compact::helpers::estimate_tokens(&messages)
                                > target_tokens
                                && messages.len() > 2
                            {
                                // Remove the oldest non-adjacent pair to maintain
                                // assistant/user message alternation
                                if messages.len() > 3 {
                                    messages.remove(0);
                                    messages.remove(0);
                                } else {
                                    break;
                                }
                            }
                            // Front-removal can orphan the first kept message: if it
                            // holds a ToolResult whose assistant ToolUse partner was
                            // dropped with the prefix, providers reject the request.
                            // Drop leading tool_result-only messages as well.
                            while messages.len() > 2
                                && shannon_engine::compact::has_tool_result(&messages[0])
                                && !shannon_engine::compact::has_tool_use(&messages[0])
                            {
                                messages.remove(0);
                            }
                            let new_estimate =
                                shannon_engine::compact::helpers::estimate_tokens(&messages);
                            tracing::info!(
                                truncated_to = messages.len(),
                                new_estimate,
                                "Truncated older messages to fit context"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: format!(
                                        "Truncated history to {} messages ({} tokens)",
                                        messages.len(),
                                        new_estimate
                                    ),
                                }
                            );
                        }
                    }
                }

                // Secret-guard wiring point 1 (blueprint §9.6): run the
                // outgoing messages through the installed transform before
                // any send in this region (main turn + in-region fallbacks
                // reuse this binding). Deterministic (I1) + idempotent (I3)
                // keeps the request prefix byte-stable across turns, so
                // provider prompt caching is unaffected. No-op unless a
                // plugin is installed.
                let mut messages = crate::secret_guard::transform_outgoing_messages(messages);
                // Call the API — use structured system blocks when available for prompt caching
                // Surface API retry activity as query progress (§ retry
                // observability): without this the retry loop is invisible
                // to consumers and a rate-limited run looks like a silent
                // multi-second stall.
                client.set_retry_observer(Some(std::sync::Arc::new({
                    let tx = tx.clone();
                    // Returns a future the retry loop awaits (§P3-6): the
                    // notice is forwarded through the bounded event channel,
                    // so backpressure reaches the retry sleep itself while
                    // FIFO order with the surrounding events is preserved.
                    move |notice: shannon_engine::api::retry::RetryNotice| {
                        // `Fn` closure: clone the handle per invocation so the
                        // returned future owns its own sender.
                        let tx = tx.clone();
                        let message = format!(
                            "API retry {}/{} (next try in {:.0}s): {}",
                            notice.attempt,
                            notice.total_attempts,
                            notice.wait.as_secs_f32(),
                            notice.reason
                        );
                        Box::pin(async move {
                            send_event!(tx, QueryEvent::Progress { query_id, message });
                        }) as futures::future::BoxFuture<'static, ()>
                    }
                })));
                let stream_result = if let Some(ref blocks) = system_blocks_opt {
                    client
                        .send_message_stream_structured_with_retry(
                            messages.clone(),
                            tools_schema.clone(),
                            blocks.clone(),
                        )
                        .await
                } else {
                    client
                        .send_message_stream_with_retry(
                            messages.clone(),
                            tools_schema.clone(),
                            system_prompt.clone(),
                        )
                        .await
                };
                client.set_retry_observer(None);
                match stream_result {
                    Ok(mut stream) => {
                        // Per-index tool call state. OpenAI streaming interleaves
                        // InputJsonDelta chunks across parallel tool calls (multiple
                        // tools in one assistant message); a single shared buffer would
                        // concatenate fragments from different tools into invalid JSON
                        // and the parser would discard every tool call in the batch.
                        // Map key is the streaming `index`; value is (id, name,
                        // accumulated JSON input).
                        let mut tool_call_state: std::collections::HashMap<
                            usize,
                            (String, String, String),
                        > = std::collections::HashMap::new();
                        let mut tool_inputs: Vec<(String, String, serde_json::Value)> = Vec::new();
                        // P3-10: dedup `tool_use_id` echoes is query-scoped
                        // (declared above the agent loop). It catches both
                        // within-stream echoes (MiniMax streaming reconnect
                        // replays the same ContentBlockStop+ToolUseRequest
                        // pair) and cross-turn replays (a misbehaving model
                        // returning the same id in successive API calls).
                        // Without this guard the tool runs twice, the
                        // conversation accumulates two `tool_result`
                        // entries for one assistant message, and downstream
                        // TUI/clients double-charge the tool's side effect
                        // (Write tool emits two files, Bash runs twice).
                        let mut has_content = false;
                        // Accumulate the full assistant response for conversation tracking
                        let mut assistant_text = String::new();
                        // WP-15 P0-2: re-split inline `<think>` reasoning out of
                        // the content-delta stream (MiniMax M-series / GLM on the
                        // OpenAI wire). Reasoning becomes Thinking events; only
                        // the visible answer lands in `assistant_text` — which
                        // also keeps reasoning markup out of saved history.
                        let mut think_splitter = ThinkStreamSplitter::default();
                        // Display-face secret restore across streamed deltas:
                        // a surrogate token spans several deltas, so the
                        // restorer carries the partial tail between feeds and
                        // flushes it with the splitter's tail below.
                        let mut display_restorer = crate::secret_guard::DisplayRestorer::new();
                        // Same display-face restore for the thinking stream:
                        // reasoning echoes surrogates and a token spans
                        // deltas just the same.
                        let mut thinking_restorer = crate::secret_guard::DisplayRestorer::new();
                        let mut assistant_tool_uses: Vec<ContentBlock> = Vec::new();
                        // Terminal stop reason for this response, latched from
                        // whichever MessageDelta carried it (providers split
                        // finish_reason and real usage across separate frames).
                        let mut assistant_stop_reason: Option<String> = None;
                        let mut phase = StreamingPhase::Receiving;
                        // Cache tokens arrive in MessageStart (Anthropic), merge with MessageDelta
                        let mut start_cache_read: u64 = 0;
                        let mut start_cache_creation: u64 = 0;
                        // Per-REQUEST usage (reset when a new stream is opened
                        // for the next turn). The sentinel-usage guard below
                        // must defer on "no usage seen for THIS request", not
                        // on the query-wide totals — once tool turns drain
                        // their trailing usage frames, the totals are non-zero
                        // at the final request too.
                        let mut request_input_tokens: u64 = 0;
                        let mut request_output_tokens: u64 = 0;

                        // Process streaming events.
                        // C-2: a stalled stream (no events within the stall
                        // budget) is fed into the loop as a synthetic
                        // `Err(ApiError::Timeout)` so the existing recovery
                        // ladder applies — partial content is preserved, and a
                        // dead stream can no longer hang the query forever.
                        //
                        // Composition with A8 (turn-level stream-death
                        // continuation): A8 escalates the client idle budget
                        // up to `STREAM_IDLE_ESCALATION_CAP_SECS` and retries
                        // timeout-class deaths in place. This loop-level
                        // timeout is therefore a LAST-RESORT backstop that
                        // must never fire before A8's escalation ceiling —
                        // floor it above the cap.
                        let stall_budget = config
                            .timeout_seconds
                            .max(30)
                            .max(STREAM_IDLE_ESCALATION_CAP_SECS + 60);
                        while let Some(event_result) = match tokio::time::timeout(
                            std::time::Duration::from_secs(stall_budget),
                            stream.next(),
                        )
                        .await
                        {
                            Ok(item) => item,
                            Err(_elapsed) => {
                                tracing::warn!(
                                    timeout_secs = stall_budget,
                                    "LLM stream stalled — no events within stall budget"
                                );
                                Some(Err(shannon_engine::api::ApiError::Timeout))
                            }
                        } {
                            match event_result {
                                Ok(stream_event) => {
                                    match stream_event {
                                        StreamEvent::MessageStart { message } => {
                                            start_cache_read =
                                                message.usage.cache_read_input_tokens as u64;
                                            start_cache_creation =
                                                message.usage.cache_creation_input_tokens as u64;
                                        }
                                        StreamEvent::ContentBlockStart {
                                            index,
                                            content_block,
                                        } => {
                                            match &content_block {
                                                ContentBlock::ToolUse { id, name, input: _ } => {
                                                    // P3-8: do NOT emit ToolUseRequest
                                                    // here — the adapter sends
                                                    // `input: Value::Null` at
                                                    // ContentBlockStart because the
                                                    // tool arguments haven't streamed
                                                    // yet. We accumulate the input
                                                    // JSON deltas into `raw` (see
                                                    // ContentDelta::InputJsonDelta
                                                    // below) and emit ToolUseRequest
                                                    // once on ContentBlockStop with
                                                    // the fully parsed input.
                                                    let entry = tool_call_state
                                                        .entry(index)
                                                        .or_insert_with(|| {
                                                            (
                                                                id.clone(),
                                                                name.clone(),
                                                                String::new(),
                                                            )
                                                        });
                                                    entry.0 = id.clone();
                                                    entry.1 = name.clone();
                                                }
                                                ContentBlock::Thinking { .. } => {
                                                    // Thinking block started — deltas will arrive via ThinkingDelta
                                                }
                                                _ => {}
                                            }
                                        }
                                        StreamEvent::ContentBlockDelta { index, delta } => {
                                            match delta {
                                                ContentDelta::TextDelta { text } => {
                                                    has_content = true;
                                                    // WP-15 P0-2: route inline
                                                    // `<think>` reasoning to the
                                                    // Thinking channel instead of
                                                    // leaking it as visible text.
                                                    let (thinking, visible) =
                                                        think_splitter.feed(&text);
                                                    if !thinking.is_empty() {
                                                        let restored =
                                                            thinking_restorer.feed(&thinking);
                                                        if !restored.is_empty() {
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::Thinking {
                                                                    query_id,
                                                                    content: restored,
                                                                }
                                                            );
                                                        }
                                                    }
                                                    if !visible.is_empty() {
                                                        assistant_text.push_str(&visible);
                                                        // Display-face restore: the
                                                        // emitted copy carries real
                                                        // values; `assistant_text`
                                                        // (history) keeps surrogates.
                                                        // `display_restorer` holds
                                                        // back any partial surrogate
                                                        // token so a token split
                                                        // across deltas still restores.
                                                        let display =
                                                            display_restorer.feed(&visible);
                                                        if !display.is_empty() {
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::Text {
                                                                    query_id,
                                                                    content: display,
                                                                }
                                                            );
                                                        }
                                                    }
                                                }
                                                ContentDelta::InputJsonDelta { partial_json } => {
                                                    if let Some(entry) =
                                                        tool_call_state.get_mut(&index)
                                                    {
                                                        entry.2.push_str(&partial_json);
                                                    }
                                                }
                                                ContentDelta::ThinkingDelta { thinking } => {
                                                    let restored =
                                                        thinking_restorer.feed(&thinking);
                                                    if !restored.is_empty() {
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Thinking {
                                                                query_id,
                                                                content: restored,
                                                            }
                                                        );
                                                    }
                                                }
                                                // Signature/unknown deltas: parsed for
                                                // stream compatibility, nothing to emit.
                                                ContentDelta::SignatureDelta { .. }
                                                | ContentDelta::Unknown => {}
                                            }
                                        }
                                        StreamEvent::ContentBlockStop { index } => {
                                            if let Some((id, name, raw)) =
                                                tool_call_state.remove(&index)
                                            {
                                                match serde_json::from_str::<serde_json::Value>(
                                                    &raw,
                                                ) {
                                                    Ok(json_val) => {
                                                        // P3-10: dedup at query
                                                        // scope (HashSet declared
                                                        // above the agent loop).
                                                        // Catches both within-
                                                        // stream ContentBlockStop
                                                        // echoes (provider
                                                        // reconnect replays the
                                                        // same stop event) and
                                                        // cross-turn replays (a
                                                        // misbehaving model
                                                        // returning the same id
                                                        // in successive API
                                                        // calls). If we already
                                                        // emitted this id this
                                                        // query, drop the
                                                        // duplicate — re-emitting
                                                        // would either double-run
                                                        // the tool or, worse, send
                                                        // a second `tool_use_id`
                                                        // to Anthropic in the
                                                        // next request which
                                                        // rejects with `duplicate
                                                        // tool_call id`.
                                                        if !seen_tool_use_ids_query
                                                            .insert(id.clone())
                                                        {
                                                            tracing::warn!(
                                                                "tool_use_id dedup: \
                                                                 dropping duplicate \
                                                                 emission for '{name}' \
                                                                 (id={id})"
                                                            );
                                                            continue; // do NOT
                                                            // push to
                                                            // tool_inputs
                                                            // either —
                                                            // the tool
                                                            // loop only
                                                            // runs each
                                                            // id once
                                                        }
                                                        // P3-8: emit ToolUseRequest
                                                        // with the FULLY PARSED
                                                        // input now that the input
                                                        // JSON deltas have been
                                                        // accumulated. Emitting on
                                                        // ContentBlockStart gave
                                                        // downstream consumers
                                                        // (NDJSON `--output-format
                                                        // json-stream`, recorder)
                                                        // a `tool_input: null`
                                                        // because the adapter only
                                                        // knows the args AFTER
                                                        // InputJsonDelta lands.
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ToolUseRequest {
                                                                query_id,
                                                                tool_use_id: id.clone(),
                                                                tool_name: name.clone(),
                                                                tool_input: json_val.clone(),
                                                            }
                                                        );
                                                        tool_inputs.push((
                                                            id.clone(),
                                                            name.clone(),
                                                            json_val.clone(),
                                                        ));
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input: json_val,
                                                            },
                                                        );
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            "Tool input JSON parse failed for '{name}': {e}"
                                                        );
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: id.clone(),
                                                                tool_name: name.clone(),
                                                                result: format!(
                                                                    "Failed to parse tool arguments: {e}"
                                                                ),
                                                                is_error: true,
                                                                meta: Box::new(
                                                                    serde_json::Value::Null
                                                                ),
                                                            }
                                                        );
                                                        // Preserve the real tool_use_id in both
                                                        // the synthetic tool_result AND the
                                                        // synthetic ToolUse block. The
                                                        // Anthropic API requires the two to
                                                        // match on the next request; pairing
                                                        // the real id with an empty-object
                                                        // ToolUse lets the model see "Malformed
                                                        // tool input" as a tool_result and retry
                                                        // with corrected JSON.
                                                        // A13-c: the input MUST be a JSON
                                                        // object, never Null — minimax parses
                                                        // tool_calls arguments and rejects the
                                                        // "null" literal with 400 (2013) while
                                                        // "{}" succeeds (live-API decisive
                                                        // test, request_ids 06fc6407792530aa1d
                                                        // 9df28fe350fd1a / 06fc64093ab64d878
                                                        // b704669ba551957). The synthetic error
                                                        // result below still tells the model the
                                                        // call failed.
                                                        tool_results.push(ToolResultEntry {
                                                            tool_use_id: id.clone(),
                                                            content: format!(
                                                                "Malformed tool input: {e}"
                                                            ),
                                                            is_error: true,
                                                            metadata: Default::default(),
                                                        });
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input: serde_json::json!({}),
                                                            },
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        StreamEvent::MessageDelta { delta, usage } => {
                                            // Flush the <think> splitter's held-back
                                            // tail before this arm reads the final
                                            // `assistant_text` (fallback below, save
                                            // paths). A dangling partial tag at stream
                                            // end is literal text; an unclosed block
                                            // routes to Thinking.
                                            let (tail_thinking, tail_visible) =
                                                think_splitter.finish();
                                            let mut tail_thinking_restored =
                                                thinking_restorer.feed(&tail_thinking);
                                            tail_thinking_restored
                                                .push_str(&thinking_restorer.finish());
                                            if !tail_thinking_restored.is_empty() {
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Thinking {
                                                        query_id,
                                                        content: tail_thinking_restored,
                                                    }
                                                );
                                            }
                                            if !tail_visible.is_empty() {
                                                assistant_text.push_str(&tail_visible);
                                                // Same display-face restore as the
                                                // delta arm; the restorer is also
                                                // finished here so any held-back
                                                // partial token flushes before the
                                                // final assistant text is used.
                                                let mut display =
                                                    display_restorer.feed(&tail_visible);
                                                display.push_str(&display_restorer.finish());
                                                if !display.is_empty() {
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Text {
                                                            query_id,
                                                            content: display,
                                                        }
                                                    );
                                                }
                                            } else {
                                                // Even without a visible tail, a held
                                                // partial token must flush here or it
                                                // would never reach the display.
                                                let flushed = display_restorer.finish();
                                                if !flushed.is_empty() {
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Text {
                                                            query_id,
                                                            content: flushed,
                                                        }
                                                    );
                                                }
                                            }
                                            if delta.stop_reason.is_some() {
                                                assistant_stop_reason = delta.stop_reason.clone();
                                            }
                                            let input_tokens = usage.input_tokens as u64;
                                            let output_tokens = usage.output_tokens as u64;
                                            // Cache tokens are priced at the model's
                                            // cache rate when known (fallback: input
                                            // rate); Anthropic reports them separately
                                            // from input_tokens, so no double counting.
                                            let cost_usd = CostTracker::calculate_cost_with_cache(
                                                &client_model,
                                                input_tokens,
                                                output_tokens,
                                                usage.cache_read_input_tokens as u64,
                                                usage.cache_creation_input_tokens as u64,
                                            );

                                            total_input_tokens += input_tokens;
                                            total_output_tokens += output_tokens;
                                            request_input_tokens += input_tokens;
                                            request_output_tokens += output_tokens;

                                            // Update shared cost tracker, then
                                            // send any budget event AFTER the
                                            // guard is dropped (§P3-6 lock-
                                            // across-send audit): the channel
                                            // is bounded, so a stalled
                                            // consumer suspends the producer
                                            // at the send — it must not do so
                                            // while holding the engine-wide
                                            // cost lock that `conversation_stats`,
                                            // `set_model`, and other queries
                                            // share.
                                            //
                                            // `budget_exceeded` keeps the
                                            // original precedence (limit beats
                                            // 80% warning; the warning flag is
                                            // only marked when not exceeded).
                                            let budget_outcome = {
                                                let mut tracker = cost_tracker
                                                    .write()
                                                    .unwrap_or_else(|e| e.into_inner());
                                                tracker.record_usage(
                                                    &client_model,
                                                    input_tokens,
                                                    output_tokens,
                                                );

                                                // Budget enforcement: check if limit exceeded
                                                if tracker.is_budget_exceeded() {
                                                    let limit =
                                                        tracker.budget_limit_usd.unwrap_or(0.0);
                                                    let total = tracker.total_cost();
                                                    Some((
                                                        true,
                                                        format!(
                                                            "Budget limit reached (${limit:.2}). Stopping. (spent: ${total:.4})"
                                                        ),
                                                    ))
                                                }
                                                // Budget warning at 80% usage (fires once)
                                                else if tracker.check_and_mark_budget_warning() {
                                                    let limit =
                                                        tracker.budget_limit_usd.unwrap_or(0.0);
                                                    let total = tracker.total_cost();
                                                    let pct = if limit > 0.0 {
                                                        (total / limit * 100.0) as u32
                                                    } else {
                                                        0
                                                    };
                                                    Some((
                                                        false,
                                                        format!(
                                                            "Budget warning: ${total:.4} / ${limit:.2} ({pct}%)"
                                                        ),
                                                    ))
                                                } else {
                                                    None
                                                }
                                            };
                                            if let Some((exceeded, message)) = budget_outcome {
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Progress { query_id, message }
                                                );
                                                if exceeded {
                                                    // Break out of the loop by setting turn to max
                                                    turn = config.max_turns;
                                                    break;
                                                }
                                            }

                                            let cache_creation_tokens = start_cache_creation
                                                .max(usage.cache_creation_input_tokens as u64);
                                            let cache_read_tokens = start_cache_read
                                                .max(usage.cache_read_input_tokens as u64);

                                            send_event!(
                                                tx,
                                                QueryEvent::Usage {
                                                    query_id,
                                                    input_tokens,
                                                    output_tokens,
                                                    cost_usd,
                                                    cache_creation_tokens,
                                                    cache_read_tokens,
                                                }
                                            );

                                            // Flush any pending tool inputs that weren't finalized by
                                            // ContentBlockStop (e.g. finish_reason arrived without
                                            // a prior synthesized stop event). Drain every
                                            // remaining per-index entry — multiple parallel tool
                                            // calls may have been mid-flight.
                                            //
                                            // This is a BROADCAST path, not just a
                                            // bookkeeping path: entries here reached the
                                            // engine without a ContentBlockStop, so the
                                            // canonical ToolUseRequest emission (P3-8)
                                            // never fired for them. Emitting here keeps
                                            // OpenAI-style providers on parity with the
                                            // Anthropic path (one event, fully parsed
                                            // input) even when an adapter gap strands
                                            // state. The P3-10 query-scope dedup applies
                                            // exactly as in the ContentBlockStop handler:
                                            // a dropped duplicate is also withheld from
                                            // tool_inputs so the id runs once.
                                            for (id, name, raw) in
                                                tool_call_state.drain().map(|(_, v)| v)
                                            {
                                                match serde_json::from_str::<serde_json::Value>(
                                                    &raw,
                                                ) {
                                                    Ok(json_val) => {
                                                        if !seen_tool_use_ids_query
                                                            .insert(id.clone())
                                                        {
                                                            tracing::warn!(
                                                                "tool_use_id dedup (post-stream \
                                                                 flush): dropping duplicate \
                                                                 emission for '{name}' (id={id})"
                                                            );
                                                            continue;
                                                        }
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ToolUseRequest {
                                                                query_id,
                                                                tool_use_id: id.clone(),
                                                                tool_name: name.clone(),
                                                                tool_input: json_val.clone(),
                                                            }
                                                        );
                                                        tool_inputs.push((
                                                            id.clone(),
                                                            name.clone(),
                                                            json_val.clone(),
                                                        ));
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input: json_val,
                                                            },
                                                        );
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            "Malformed tool input (post-stream flush): {e}"
                                                        );
                                                        // A13-c: pair the synthetic result
                                                        // with an assistant ToolUse block
                                                        // (empty-object input — Null is
                                                        // wire-illegal for minimax, see the
                                                        // ContentBlockStop site) so the result
                                                        // is never orphaned on the wire.
                                                        tool_results.push(ToolResultEntry {
                                                            tool_use_id: id.clone(),
                                                            content: format!(
                                                                "Malformed tool input: {e}"
                                                            ),
                                                            is_error: true,
                                                            metadata: Default::default(),
                                                        });
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input: serde_json::json!({}),
                                                            },
                                                        );
                                                    }
                                                }
                                            }

                                            // WP-15 P0-1: text-form tool-call fallback.
                                            // A model on the OpenAI wire may answer a
                                            // tool-worthy prompt without a native tool
                                            // call — either as GLM/MiniMax-style
                                            // `<tool_call><invoke>` text (MiniMax M3's
                                            // vendor-token-wrapped form observed in the
                                            // field) or as a bare ```bash block; the
                                            // turn then loops forever ("no tool calls"
                                            // → retry → same text). Convert these into
                                            // real tool calls so the normal permission
                                            // gate + approval chain applies. Only fires
                                            // when the request actually had tools and
                                            // nothing native arrived; disable via
                                            // `markdown_tool_fallback: false`.
                                            if tool_inputs.is_empty()
                                                && config.markdown_tool_fallback
                                                && tools_schema.is_some()
                                            {
                                                // Priority 1: `<tool_call><invoke>` text —
                                                // an unambiguous tool-call attempt.
                                                let text_calls =
                                                    parse_text_tool_calls(&assistant_text);
                                                if let Some(calls) = text_calls {
                                                    tracing::warn!(
                                                        "no native tool call in response; \
                                                         recovered {} textual tool call(s) \
                                                         (markdown_tool_fallback)",
                                                        calls.len()
                                                    );
                                                    for (name, input) in calls {
                                                        let id =
                                                            format!("txt-call-{}", Uuid::new_v4());
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ToolUseRequest {
                                                                query_id,
                                                                tool_use_id: id.clone(),
                                                                tool_name: name.clone(),
                                                                tool_input: input.clone(),
                                                            }
                                                        );
                                                        tool_inputs.push((
                                                            id.clone(),
                                                            name.clone(),
                                                            input.clone(),
                                                        ));
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input,
                                                            },
                                                        );
                                                    }
                                                } else if let Some(command) =
                                                    markdown_bash_command(&assistant_text)
                                                {
                                                    // Priority 2: a single bare shell code
                                                    // block (less certain — see
                                                    // `markdown_bash_command`).
                                                    tracing::warn!(
                                                        "no native tool call in response; \
                                                         executing bash from markdown code \
                                                         block (markdown_tool_fallback)"
                                                    );
                                                    let id = format!("md-bash-{}", Uuid::new_v4());
                                                    let input = serde_json::json!({
                                                        "command": command
                                                    });
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ToolUseRequest {
                                                            query_id,
                                                            tool_use_id: id.clone(),
                                                            tool_name: "Bash".to_string(),
                                                            tool_input: input.clone(),
                                                        }
                                                    );
                                                    tool_inputs.push((
                                                        id.clone(),
                                                        "Bash".to_string(),
                                                        input.clone(),
                                                    ));
                                                    assistant_tool_uses.push(
                                                        ContentBlock::ToolUse {
                                                            id,
                                                            name: "Bash".to_string(),
                                                            input,
                                                        },
                                                    );
                                                }
                                            }

                                            if !tool_inputs.is_empty() {
                                                // Phase 1: Check permissions and hooks (sequential — may need user input)
                                                let mut approved_tools: Vec<(
                                                    String,
                                                    String,
                                                    serde_json::Value,
                                                )> = Vec::new();

                                                for (tool_id, tool_name, tool_input) in
                                                    tool_inputs.drain(..)
                                                {
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Progress {
                                                            query_id,
                                                            message: format!(
                                                                "Executing tool: {tool_name}"
                                                            ),
                                                        }
                                                    );

                                                    // Plan mode gate: block write tools when plan mode is active.
                                                    // Derives mutability from the tool's own trait
                                                    // metadata instead of a name list, so newly
                                                    // registered mutators (NotebookEdit, git
                                                    // mutators, Worktree, Cron, …) are covered.
                                                    // Pure bookkeeping tools stay allowed.
                                                    let is_plan_active = plan_mode_active
                                                        .read()
                                                        .map(|g| *g)
                                                        .unwrap_or(false);
                                                    if is_plan_active {
                                                        const PLAN_ALLOWED_STATELESS: &[&str] = &[
                                                            "TodoWrite",
                                                            "TaskCreate",
                                                            "TaskList",
                                                            "TaskUpdate",
                                                            "TaskGet",
                                                            "TaskTool",
                                                            "AskUserQuestion",
                                                            "EnterPlanMode",
                                                            "ExitPlanMode",
                                                            "GetPlanStatus",
                                                            "Brief",
                                                            "StructuredOutput",
                                                            "Sleep",
                                                        ];
                                                        let tool_is_mutating = match tools.get(&tool_name) {
                                                            Some(t) => !t.is_read_only(),
                                                            None => crate::tool_execution::is_file_modifying_tool(&tool_name),
                                                        };
                                                        if tool_is_mutating
                                                            && !PLAN_ALLOWED_STATELESS.iter().any(
                                                                |n| {
                                                                    n.eq_ignore_ascii_case(
                                                                        &tool_name,
                                                                    )
                                                                },
                                                            )
                                                        {
                                                            let error_msg = format!(
                                                                "Plan mode: write operations blocked. \
                                                                 Use exit_plan_mode to resume editing. \
                                                                 Blocked tool: {tool_name}"
                                                            );
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::ToolUseResult {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name,
                                                                    result: error_msg.clone(),
                                                                    is_error: true,
                                                                    meta: Box::new(
                                                                        serde_json::Value::Null
                                                                    ),
                                                                }
                                                            );
                                                            tool_results.push(ToolResultEntry {
                                                                tool_use_id: tool_id,
                                                                content: error_msg,
                                                                is_error: true,
                                                                metadata: Default::default(),
                                                            });
                                                            continue;
                                                        }
                                                    }

                                                    // §4.8: the permission gate is the
                                                    // chain-head node of the tool
                                                    // pre-execute waterfall; every
                                                    // verdict is published as a
                                                    // durable permission/decision row
                                                    // by the node itself.
                                                    use crate::query_engine::guard_nodes::PermissionVerdict;
                                                    let mut guard_ctx =
                                                        crate::query_engine::guard_nodes::ToolGuardContext::new(
                                                            tool_name.clone(),
                                                            tool_input.clone(),
                                                        );
                                                    {
                                                        use crate::query_engine::guard_nodes::PermissionGateNode;
                                                        let gate = PermissionGateNode::new(
                                                            permissions.clone(),
                                                            session_id_for_permissions,
                                                            session_bus.shared(),
                                                        );
                                                        gate.evaluate(&mut guard_ctx).await;
                                                    }

                                                    match &guard_ctx.verdict {
                                                        PermissionVerdict::Denied {
                                                            reason,
                                                            ..
                                                        } => {
                                                            consecutive_denials += 1;
                                                            let error_msg = reason.clone();
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::ToolUseResult {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name,
                                                                    result: error_msg.clone(),
                                                                    is_error: true,
                                                                    meta: Box::new(
                                                                        serde_json::Value::Null
                                                                    ),
                                                                }
                                                            );
                                                            tool_results.push(ToolResultEntry {
                                                                tool_use_id: tool_id,
                                                                content: error_msg,
                                                                is_error: true,
                                                                metadata: Default::default(),
                                                            });
                                                            continue;
                                                        }
                                                        PermissionVerdict::Allowed
                                                        | PermissionVerdict::Pending => {
                                                            // Auto-allowed (low risk or always-allowed)
                                                            // Fall through to check hooks
                                                        }
                                                        PermissionVerdict::Prompt {
                                                            prompt,
                                                            ..
                                                        } => {
                                                            // The gate already refused Critical risk;
                                                            // this arm is the interactive path. (Critical
                                                            // prompts can no longer occur here.)
                                                            #[allow(unused_mut)]
                                                            let mut prompt = (**prompt).clone();
                                                            let _ = &mut prompt;

                                                            // Send permission request if a channel is provided
                                                            if let Some(ref req_tx) =
                                                                permission_request_tx
                                                            {
                                                                // Generate diff preview for file edit/write tools
                                                                if matches!(
                                                                    tool_name.as_str(),
                                                                    "edit"
                                                                        | "write"
                                                                        | "EditTool"
                                                                        | "WriteTool"
                                                                ) {
                                                                    if let Some(path) = tool_input
                                                                        .get("file_path")
                                                                        .and_then(|v| v.as_str())
                                                                    {
                                                                        let path_buf = std::path::PathBuf::from(path);
                                                                        if path_buf.exists() {
                                                                            if let Ok(old_content) = std::fs::read_to_string(&path_buf) {
                                                                                let new_content = tool_input.get("content")
                                                                                    .or_else(|| tool_input.get("new_string"))
                                                                                    .and_then(|v| v.as_str())
                                                                                    .unwrap_or("");
                                                                                let diff = generate_diff_preview(path, &old_content, new_content);
                                                                                prompt.diff_preview = Some(diff);
                                                                            }
                                                                        } else if tool_name
                                                                            == "write"
                                                                            || tool_name
                                                                                == "WriteTool"
                                                                        {
                                                                            // New file — show that it's being created
                                                                            if let Some(content) =
                                                                                tool_input
                                                                                    .get("content")
                                                                                    .and_then(|v| {
                                                                                        v.as_str()
                                                                                    })
                                                                            {
                                                                                let preview =
                                                                                    if content.len()
                                                                                        > 500
                                                                                    {
                                                                                        let mut end = 500.min(content.len());
                                                                                        while !content.is_char_boundary(end) { end -= 1; }
                                                                                        format!("+ Creating new file ({} bytes)\n{}\n... (truncated)", content.len(), &content[..end])
                                                                                    } else {
                                                                                        format!(
                                                                                            "+ Creating new file\n{content}"
                                                                                        )
                                                                                    };
                                                                                prompt
                                                                                    .diff_preview =
                                                                                    Some(preview);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                // Single-use response lane:
                                                                // exactly one PermissionChoice
                                                                // comes back per prompt, so a
                                                                // oneshot carries no unbounded
                                                                // buffer and surfaces a dropped
                                                                // host as a recv error (§P3-6).
                                                                let (response_tx, response_rx) =
                                                                    tokio::sync::oneshot::channel();
                                                                // Clone prompt for the request; keep a reference for deny message
                                                                let prompt_desc =
                                                                    prompt.description.clone();
                                                                let prompt_for_choice =
                                                                    prompt.clone();
                                                                // Bounded request channel:
                                                                // await the send. If the host
                                                                // dropped its receiver this
                                                                // fails and `response_tx` is
                                                                // dropped with the unsent
                                                                // request, so the recv below
                                                                // still resolves (→ deny).
                                                                let _ = req_tx
                                                                    .send(
                                                                        crate::query_engine::types::PermissionRequest {
                                                                            prompt,
                                                                            response_tx,
                                                                        },
                                                                    )
                                                                    .await;

                                                                // Wait for user response (a
                                                                // dropped sender resolves to
                                                                // Err → deny, same contract as
                                                                // the old channel's `None`).
                                                                match response_rx.await {
                                                                    Ok(
                                                                        shannon_engine::permissions::PermissionChoice::Deny,
                                                                    ) => {
                                                                        consecutive_denials += 1;
                                                                        crate::query_engine::guard_nodes::emit_decision(
                                                                            &session_bus,
                                                                            &tool_name,
                                                                            "deny",
                                                                            Some("user denied the operation"),
                                                                            "USER",
                                                                            0,
                                                                        );
                                                                        let denied_msg = format!(
                                                                            "Permission denied: {prompt_desc}"
                                                                        );
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name,
                                                                            result: denied_msg
                                                                                .clone(),
                                                                            is_error: true,
                                                                            meta: Box::new(serde_json::Value::Null),
                                                                            });
                                                                        tool_results
                                                                            .push(ToolResultEntry {
                                                                                tool_use_id: tool_id,
                                                                                content: denied_msg,
                                                                                is_error: true,
                                                                                metadata: Default::default(),
                                                                            });
                                                                        continue;
                                                                    }
                                                                    Ok(
                                                                        shannon_engine::permissions::PermissionChoice::AllowOnce,
                                                                    ) => {
                                                                        crate::query_engine::guard_nodes::emit_decision(
                                                                            &session_bus,
                                                                            &tool_name,
                                                                            "allow",
                                                                            Some("user allowed once"),
                                                                            "USER",
                                                                            0,
                                                                        );
                                                                    }
                                                                    Ok(
                                                                        shannon_engine::permissions::PermissionChoice::AlwaysAllow,
                                                                    ) => {
                                                                        let _ = recover_lock(permissions.write())
                                                                            .process_permission_choice(
                                                                                session_id_for_permissions,
                                                                                &prompt_for_choice,
                                                                                shannon_engine::permissions::PermissionChoice::AlwaysAllow,
                                                                            );
                                                                        crate::query_engine::guard_nodes::emit_decision(
                                                                            &session_bus,
                                                                            &tool_name,
                                                                            "allow",
                                                                            Some("user chose always allow"),
                                                                            "USER",
                                                                            0,
                                                                        );
                                                                    }
                                                                    Ok(
                                                                        shannon_engine::permissions::PermissionChoice::EditAndRun,
                                                                    ) => {
                                                                        // User edited the command; treat as allow-once
                                                                        let _ = recover_lock(permissions.write())
                                                                            .process_permission_choice(
                                                                                session_id_for_permissions,
                                                                                &prompt_for_choice,
                                                                                shannon_engine::permissions::PermissionChoice::EditAndRun,
                                                                            );
                                                                        crate::query_engine::guard_nodes::emit_decision(
                                                                            &session_bus,
                                                                            &tool_name,
                                                                            "allow",
                                                                            Some("user edited then allowed"),
                                                                            "USER",
                                                                            0,
                                                                        );
                                                                    }
                                                                    Err(_) => {
                                                                        crate::query_engine::guard_nodes::emit_decision(
                                                                            &session_bus,
                                                                            &tool_name,
                                                                            "deny",
                                                                            Some("permission channel closed"),
                                                                            "USER",
                                                                            0,
                                                                        );
                                                                        let error_msg =
                                                                            "Permission channel closed"
                                                                                .to_string();
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name,
                                                                            result: error_msg
                                                                                .clone(),
                                                                            is_error: true,
                                                                            meta: Box::new(serde_json::Value::Null),
                                                                            });
                                                                        tool_results
                                                                            .push(ToolResultEntry {
                                                                                tool_use_id: tool_id,
                                                                                content: error_msg,
                                                                                is_error: true,
                                                                                metadata: Default::default(),
                                                                            });
                                                                        continue;
                                                                    }
                                                                }
                                                            } else {
                                                                // N-1: fail closed. With no approval
                                                                // channel attached (REST /api/query,
                                                                // automation runners), an interactive
                                                                // prompt can never be answered — deny
                                                                // instead of silently auto-allowing.
                                                                consecutive_denials += 1;
                                                                crate::query_engine::guard_nodes::emit_decision(
                                                                    &session_bus,
                                                                    &tool_name,
                                                                    "deny",
                                                                    Some("no approval channel attached"),
                                                                    "SYSTEM",
                                                                    0,
                                                                );
                                                                let error_msg = format!(
                                                                    "Permission required for '{tool_name}' but this session has no approval channel; the operation was denied. Re-run interactively or grant a broader permission mode."
                                                                );
                                                                send_event!(
                                                                    tx,
                                                                    QueryEvent::ToolUseResult {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name,
                                                                        result: error_msg.clone(),
                                                                        is_error: true,
                                                                        meta: Box::new(
                                                                            serde_json::Value::Null
                                                                        ),
                                                                    }
                                                                );
                                                                tool_results.push(
                                                                    ToolResultEntry {
                                                                        tool_use_id: tool_id,
                                                                        content: error_msg,
                                                                        is_error: true,
                                                                        metadata: Default::default(
                                                                        ),
                                                                    },
                                                                );
                                                                continue;
                                                            }
                                                        }
                                                    }

                                                    // §4.8 stage 2: PreToolUse hooks run as the
                                                    // second waterfall node; executed hooks are
                                                    // audited as durable hook/fired rows by the node.
                                                    {
                                                        use crate::query_engine::guard_nodes::PreToolUseHookNode;
                                                        let hooks_stage = PreToolUseHookNode::new(
                                                            hook_manager.clone(),
                                                            session_bus.shared(),
                                                        );
                                                        hooks_stage.evaluate(&mut guard_ctx).await;
                                                    }
                                                    if let Some(reason) = guard_ctx.hook_deny.take()
                                                    {
                                                        let error_msg =
                                                            format!("Hook denied: {reason}");
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: tool_id.clone(),
                                                                tool_name,
                                                                result: error_msg.clone(),
                                                                is_error: true,
                                                                meta: Box::new(
                                                                    serde_json::Value::Null
                                                                ),
                                                            }
                                                        );
                                                        tool_results.push(ToolResultEntry {
                                                            tool_use_id: tool_id,
                                                            content: error_msg,
                                                            is_error: true,
                                                            metadata: Default::default(),
                                                        });
                                                        continue;
                                                    }

                                                    approved_tools.push((
                                                        tool_id,
                                                        tool_name,
                                                        guard_ctx.input,
                                                    ));
                                                }

                                                // Circuit breaker: check consecutive denials before executing tools.
                                                if consecutive_denials >= DENIAL_HARD_LIMIT {
                                                    send_event!(tx, QueryEvent::ToolUseResult {
                                                        query_id,
                                                        tool_use_id: "circuit-breaker".to_string(),
                                                        tool_name: "system".to_string(),
                                                        result: "Too many consecutive permission denials. Stopping.".to_string(),
                                                        is_error: true,
                                                        meta: Box::new(serde_json::Value::Null),
                                                        });
                                                    break; // exit the agent loop
                                                }

                                                // Phase 2: Execute approved tools using read/write-aware batch scheduler.
                                                //
                                                // Read-only tools are grouped into parallel batches.
                                                // Write tools execute one at a time to avoid race conditions.
                                                {
                                                    let batches = tools.partition_tool_calls(
                                                        approved_tools,
                                                        config.max_parallel_tools,
                                                    );

                                                    for batch in batches {
                                                        match batch {
                                                            crate::tools::ToolBatch::Parallel(
                                                                tool_calls,
                                                            ) => {
                                                                // Execute read-only tools concurrently
                                                                let mut exec_handles = Vec::new();
                                                                for (
                                                                    tool_id,
                                                                    tool_name,
                                                                    effective_input,
                                                                ) in tool_calls
                                                                {
                                                                    let id_for_error =
                                                                        tool_id.clone();
                                                                    // Emit progress: tool started
                                                                    send_event!(
                                                                        tx,
                                                                        QueryEvent::ToolProgress {
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name: tool_name
                                                                                .clone(),
                                                                            progress: 0.0,
                                                                            message: format!(
                                                                                "{tool_name} started"
                                                                            ),
                                                                        }
                                                                    );
                                                                    let tools_exec = tools.clone();
                                                                    let exec_name =
                                                                        tool_name.clone();
                                                                    let exec_input =
                                                                        effective_input.clone();
                                                                    let progress_sender =
                                                                        std::sync::Arc::new(
                                                                            ChannelProgressSender {
                                                                                tx: tx.clone(),
                                                                                query_id,
                                                                                tool_use_id:
                                                                                    tool_id.clone(),
                                                                                tool_name:
                                                                                    tool_name
                                                                                        .clone(),
                                                                            },
                                                                        );
                                                                    // Review §P2-4: batched
                                                                    // tools run in spawned
                                                                    // tasks that would not
                                                                    // inherit the producer's
                                                                    // decision-sink scope;
                                                                    // re-scope explicitly so
                                                                    // plugin gates inside
                                                                    // them route to this
                                                                    // query's bus.
                                                                    let handle = tokio::spawn(
                                                                        crate::bus::inherit_decision_sink(async move {
                                                                            (tool_id, tool_name, effective_input, tools_exec.execute_streaming(&exec_name, exec_input, progress_sender).await)
                                                                        }),
                                                                    );
                                                                    exec_handles.push((
                                                                        id_for_error,
                                                                        handle,
                                                                    ));
                                                                }

                                                                for (saved_tool_id, handle) in
                                                                    exec_handles
                                                                {
                                                                    match handle.await {
                                                                        Ok((
                                                                            tool_id,
                                                                            tool_name,
                                                                            effective_input,
                                                                            result,
                                                                        )) => {
                                                                            // §4.8: PostToolUse fires as a bus
                                                                            // trigger handled by the adapter
                                                                            // subscription.
                                                                            {
                                                                                let output_val = match &result {
                                                                                    Ok(o) => serde_json::Value::String(o.content.clone()),
                                                                                    Err(e) => serde_json::Value::String(format!("Error: {e}")),
                                                                                };
                                                                                let is_error = match &result {
                                                                                    Ok(o) => o.is_error,
                                                                                    Err(_) => true,
                                                                                };
                                                                                crate::query_engine::guard_nodes::publish_hook_trigger(
                                                                                    &session_bus,
                                                                                    "PostToolUse",
                                                                                    serde_json::json!({
                                                                                        "tool_name": tool_name.clone(),
                                                                                        "input": effective_input.clone(),
                                                                                        "output": output_val,
                                                                                        "is_error": is_error,
                                                                                    }),
                                                                                );
                                                                            }

                                                                            // Execute matching triggered routines (non-blocking)
                                                                            {
                                                                                let routines =
                                                                                    triggered_routines
                                                                                        .clone();
                                                                                let tool =
                                                                                    tool_name
                                                                                        .clone();
                                                                                tokio::spawn(
                                                                                    async move {
                                                                                        let reg =
                                                                                            routines
                                                                                                .read()
                                                                                                .await;
                                                                                        let results = reg
                                                                                            .execute_matching(
                                                                                                &crate::HookEventType::PostToolUse,
                                                                                                &tool,
                                                                                                None,
                                                                                            )
                                                                                            .await;
                                                                                        for r in
                                                                                            &results
                                                                                        {
                                                                                            if r.success() {
                                                                                                tracing::info!(name = %r.name, "Triggered routine completed");
                                                                                            } else {
                                                                                                tracing::warn!(name = %r.name, stderr = %r.stderr, "Triggered routine failed");
                                                                                            }
                                                                                        }
                                                                                    },
                                                                                );
                                                                            }

                                                                            // Emit progress: tool completed
                                                                            send_event!(tx, QueryEvent::ToolProgress {
                                                                                query_id,
                                                                                tool_use_id: tool_id.clone(),
                                                                                tool_name: tool_name.clone(),
                                                                                progress: 1.0,
                                                                                message: format!("{tool_name} completed"),
                                                                            });
                                                                            match result {
                                                                                Ok(output) => {
                                                                                    let is_err = output.is_error;
                                                                                    send_event!(tx, QueryEvent::ToolUseResult {
                                                                                        query_id,
                                                                                        tool_use_id: tool_id.clone(),
                                                                                        tool_name: tool_name.clone(),
                                                                                        result: output.content.clone(),
                                                                                        is_error: is_err,
                                                                                        meta: Box::new(crate::tools::sandbox_meta_from(&output.metadata)),
                                                                                        });
                                                                                    let (capped, truncated) = cap_tool_result(output.content.clone());
                                                                                    let mut meta = output.metadata.clone();
                                                                                    if truncated {
                                                                                        meta.insert("truncated".to_string(), serde_json::json!(true));
                                                                                    }
                                                                                    tool_results
                                                                                        .push(ToolResultEntry {
                                                                                        tool_use_id: tool_id,
                                                                                        content: capped,
                                                                                        is_error: is_err,
                                                                                        metadata: meta,
                                                                                    });
                                                                                }
                                                                                Err(e) => {
                                                                                    // Tool execution errors are not permission denials
                                                                                    let error_msg = format!(
                                                                                        "Tool error: {e}"
                                                                                    );
                                                                                    send_event!(tx, QueryEvent::ToolUseResult {
                                                                                        query_id,
                                                                                        tool_use_id: tool_id.clone(),
                                                                                        tool_name,
                                                                                        result: error_msg.clone(),
                                                                                        is_error: true,
                                                                                        meta: Box::new(serde_json::Value::Null),
                                                                                        });
                                                                                    tool_results
                                                                                        .push(ToolResultEntry {
                                                                                        tool_use_id: tool_id,
                                                                                        content: error_msg,
                                                                                        is_error: true,
                                                                                        metadata: Default::default(),
                                                                                    });
                                                                                }
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            // Task join errors are not permission denials
                                                                            let error_msg = format!(
                                                                                "Task join error: {e}"
                                                                            );
                                                                            send_event!(tx, QueryEvent::ToolUseResult {
                                                                                query_id,
                                                                                tool_use_id: saved_tool_id.clone(),
                                                                                tool_name: String::new(),
                                                                                result: error_msg.clone(),
                                                                                is_error: true,
                                                                                meta: Box::new(serde_json::Value::Null),
                                                                                });
                                                                            tool_results.push(ToolResultEntry {
                                                                                tool_use_id: saved_tool_id,
                                                                                content: error_msg,
                                                                                is_error: true,
                                                                                metadata: Default::default(),
                                                                            });
                                                                        }
                                                                    }
                                                                }
                                                                // Every call in a parallel batch
                                                                // already passed the permission gate,
                                                                // so the denial counter resets here
                                                                // unconditionally (runtime errors are
                                                                // not denials).
                                                                consecutive_denials = 0;
                                                            }
                                                            crate::tools::ToolBatch::Serial((
                                                                tool_id,
                                                                tool_name,
                                                                effective_input,
                                                            )) => {
                                                                // Execute write tools sequentially (one at a time)
                                                                // Emit progress: tool started
                                                                send_event!(
                                                                    tx,
                                                                    QueryEvent::ToolProgress {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name: tool_name
                                                                            .clone(),
                                                                        progress: 0.0,
                                                                        message: format!(
                                                                            "{tool_name} started"
                                                                        ),
                                                                    }
                                                                );
                                                                let progress_sender =
                                                                    std::sync::Arc::new(
                                                                        ChannelProgressSender {
                                                                            tx: tx.clone(),
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name: tool_name
                                                                                .clone(),
                                                                        },
                                                                    );
                                                                let result = tools
                                                                    .execute_streaming(
                                                                        &tool_name,
                                                                        effective_input.clone(),
                                                                        progress_sender,
                                                                    )
                                                                    .await;

                                                                // §4.8: PostToolUse fires as a bus trigger
                                                                // handled by the adapter subscription.
                                                                {
                                                                    let output_val = match &result {
                                                                        Ok(o) => serde_json::Value::String(o.content.clone()),
                                                                        Err(e) => serde_json::Value::String(format!("Error: {e}")),
                                                                    };
                                                                    let is_error = match &result {
                                                                        Ok(o) => o.is_error,
                                                                        Err(_) => true,
                                                                    };
                                                                    crate::query_engine::guard_nodes::publish_hook_trigger(
                                                                        &session_bus,
                                                                        "PostToolUse",
                                                                        serde_json::json!({
                                                                            "tool_name": tool_name.clone(),
                                                                            "input": effective_input.clone(),
                                                                            "output": output_val,
                                                                            "is_error": is_error,
                                                                        }),
                                                                    );
                                                                }

                                                                // Execute matching triggered routines (non-blocking)
                                                                {
                                                                    let routines =
                                                                        triggered_routines.clone();
                                                                    let tool = tool_name.clone();
                                                                    tokio::spawn(async move {
                                                                        let reg =
                                                                            routines.read().await;
                                                                        let results = reg
                                                                                .execute_matching(
                                                                                    &crate::HookEventType::PostToolUse,
                                                                                    &tool,
                                                                                    None,
                                                                                )
                                                                                .await;
                                                                        for r in &results {
                                                                            if r.success() {
                                                                                tracing::info!(name = %r.name, "Triggered routine completed");
                                                                            } else {
                                                                                tracing::warn!(name = %r.name, stderr = %r.stderr, "Triggered routine failed");
                                                                            }
                                                                        }
                                                                    });
                                                                }

                                                                // Emit progress: tool completed
                                                                send_event!(
                                                                    tx,
                                                                    QueryEvent::ToolProgress {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name: tool_name
                                                                            .clone(),
                                                                        progress: 1.0,
                                                                        message: format!(
                                                                            "{tool_name} completed"
                                                                        ),
                                                                    }
                                                                );
                                                                match result {
                                                                    Ok(output) => {
                                                                        let is_err =
                                                                            output.is_error;
                                                                        consecutive_denials = 0; // reset on success
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id.clone(),
                                                                            tool_name: tool_name.clone(),
                                                                            result: output.content.clone(),
                                                                            is_error: is_err,
                                                                            meta: Box::new(crate::tools::sandbox_meta_from(&output.metadata)),
                                                                            });
                                                                        let (capped, truncated) =
                                                                            cap_tool_result(
                                                                                output
                                                                                    .content
                                                                                    .clone(),
                                                                            );
                                                                        let mut meta =
                                                                            output.metadata.clone();
                                                                        if truncated {
                                                                            meta.insert(
                                                                                "truncated"
                                                                                    .to_string(),
                                                                                serde_json::json!(
                                                                                    true
                                                                                ),
                                                                            );
                                                                        }
                                                                        tool_results.push(
                                                                            ToolResultEntry {
                                                                                tool_use_id:
                                                                                    tool_id,
                                                                                content: capped,
                                                                                is_error: is_err,
                                                                                metadata: meta,
                                                                            },
                                                                        );
                                                                        if matches!(
                                                                            tool_name.as_str(),
                                                                            "Edit" | "Write"
                                                                        ) {
                                                                            file_edits_made = true;
                                                                            // P-B: session-level flag for the
                                                                            // turn-N checkpoint decision.
                                                                            session_file_edits_made = true;
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        let error_msg = format!(
                                                                            "Tool error: {e}"
                                                                        );
                                                                        // Gate-approved call: a runtime
                                                                        // error is not a denial.
                                                                        consecutive_denials = 0;
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id.clone(),
                                                                            tool_name,
                                                                            result: error_msg.clone(),
                                                                            is_error: true,
                                                                            meta: Box::new(serde_json::Value::Null),
                                                                            });
                                                                        tool_results.push(ToolResultEntry {
                                                                            tool_use_id: tool_id,
                                                                            content: error_msg,
                                                                            is_error: true,
                                                                            metadata: Default::default(),
                                                                        });
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                // Auto-test loop (P1-5): when enabled, after a
                                                // successful file-modifying tool, run the
                                                // configured test command. On failure, inject
                                                // the failure into the next LLM context so the
                                                // model can fix the code. Anti-loop guards
                                                // (max_iterations / total_timeout / no_progress)
                                                // cap iteration. Only triggered by Edit/Write —
                                                // we don't auto-test after Read/Grep/Bash.
                                                if file_edits_made {
                                                    if let Some(auto_cfg) = config.auto_test.clone()
                                                    {
                                                        maybe_run_auto_test(
                                                            &auto_cfg,
                                                            &mut auto_test_state,
                                                            &mut user_notices,
                                                            &tx,
                                                            query_id,
                                                        )
                                                        .await;
                                                    }
                                                }

                                                // Soft-limit warning: inject a message telling the model to stop retrying
                                                if (DENIAL_SOFT_LIMIT..DENIAL_HARD_LIMIT)
                                                    .contains(&consecutive_denials)
                                                {
                                                    // Delivered as a plain user notice, NOT a
                                                    // tool_result: the synthetic
                                                    // "denial-warning" id references no assistant
                                                    // ToolUse and providers reject the request.
                                                    user_notices.push(format!(
                                                        "The user has denied {consecutive_denials} consecutive tool calls. \
                                                         Stop retrying the same or similar operations. \
                                                         Ask the user for clarification or try a completely different approach."
                                                    ));
                                                }

                                                turn += 1;

                                                // Save assistant response to conversation for multi-turn context.
                                                // The API requires: assistant(tool_use) → user(tool_result).
                                                // Without the assistant message, the next API call has no
                                                // context for which tools were requested.
                                                {
                                                    let mut assistant_blocks: Vec<ContentBlock> =
                                                        Vec::new();
                                                    if !assistant_text.is_empty() {
                                                        assistant_blocks.push(ContentBlock::Text {
                                                            text: assistant_text.clone(),
                                                        });
                                                    }
                                                    assistant_blocks
                                                        .append(&mut assistant_tool_uses);
                                                    if !assistant_blocks.is_empty() {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Blocks(
                                                                assistant_blocks,
                                                            ),
                                                        });
                                                    }
                                                }

                                                // Auto-commit: if enabled and file-write tools were used,
                                                // stage changes and commit automatically.
                                                if config.auto_commit && file_edits_made {
                                                    file_edits_made = false; // reset for next turn
                                                    let _ = async {
                                                        let add_output = tokio::process::Command::new("git")
                                                            .args(["add", "-A"])
                                                            .output()
                                                            .await;
                                                        if let Ok(out) = add_output {
                                                            if out.status.success() {
                                                                // Generate commit message from diff stat
                                                                let stat_output = tokio::process::Command::new("git")
                                                                    .args(["diff", "--stat", "--cached"])
                                                                    .output()
                                                                    .await;
                                                                let msg = match stat_output {
                                                                    Ok(s) if s.status.success() => {
                                                                        let stat = String::from_utf8_lossy(&s.stdout);
                                                                        let file_count = stat.lines().filter(|l| !l.trim().is_empty()).count().saturating_sub(1);
                                                                        if file_count == 0 {
                                                                            "chore: update files".to_string()
                                                                        } else {
                                                                            format!("chore: auto-commit ({file_count} files)")
                                                                        }
                                                                    }
                                                                    _ => "chore: auto-commit".to_string(),
                                                                };
                                                                let commit_output = tokio::process::Command::new("git")
                                                                    .args(["commit", "-m", &msg])
                                                                    .output()
                                                                    .await;
                                                                if let Ok(co) = commit_output {
                                                                    if co.status.success() {
                                                                        let hash = String::from_utf8_lossy(&co.stdout)
                                                                            .lines()
                                                                            .find(|l| l.starts_with('['))
                                                                            .unwrap_or("committed")
                                                                            .to_string();
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: String::new(),
                                                                            tool_name: "auto_commit".to_string(),
                                                                            result: format!("Auto-committed: {hash}"),
                                                                            is_error: false,
                                                                            meta: Box::new(serde_json::Value::Null),
                                                                            });
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }.await;
                                                }

                                                // OpenAI-compatible providers (MiniMax
                                                // M-series, DeepSeek) deliver the real
                                                // usage in a separate SSE frame AFTER
                                                // the finish_reason chunk. Tool turns
                                                // break out of the stream below, so
                                                // without draining the tail that frame
                                                // is dropped and the whole request
                                                // counts as zero tokens (dogfood
                                                // 2026-08-23 l1: 62 of 63 requests
                                                // lost their usage this way). The
                                                // no-tool path already defers via the
                                                // sentinel guard; this mirrors it for
                                                // tool turns. Bounded so a server that
                                                // holds the connection open cannot
                                                // stall the agent loop; providers that
                                                // inline usage into the finish chunk
                                                // skip the drain entirely.
                                                let mut turn_tokens_used = (usage.input_tokens
                                                    as u64)
                                                    + (usage.output_tokens as u64);
                                                if turn_tokens_used == 0 {
                                                    let deadline = tokio::time::Instant::now()
                                                        + std::time::Duration::from_millis(2000);
                                                    while let Ok(Some(Ok(trailing_event))) =
                                                        tokio::time::timeout_at(
                                                            deadline,
                                                            stream.next(),
                                                        )
                                                        .await
                                                    {
                                                        let StreamEvent::MessageDelta {
                                                            usage: trailing,
                                                            ..
                                                        } = trailing_event
                                                        else {
                                                            continue;
                                                        };
                                                        if trailing.input_tokens == 0
                                                            && trailing.output_tokens == 0
                                                        {
                                                            continue;
                                                        }
                                                        let trailing_in =
                                                            trailing.input_tokens as u64;
                                                        let trailing_out =
                                                            trailing.output_tokens as u64;
                                                        total_input_tokens += trailing_in;
                                                        total_output_tokens += trailing_out;
                                                        turn_tokens_used =
                                                            trailing_in + trailing_out;
                                                        cost_tracker
                                                            .write()
                                                            .unwrap_or_else(|e| e.into_inner())
                                                            .record_usage(
                                                                &client_model,
                                                                trailing_in,
                                                                trailing_out,
                                                            );
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Usage {
                                                                query_id,
                                                                input_tokens: trailing_in,
                                                                output_tokens: trailing_out,
                                                                cost_usd:
                                                                    CostTracker::calculate_cost_with_cache(
                                                                        &client_model,
                                                                        trailing_in,
                                                                        trailing_out,
                                                                        trailing
                                                                            .cache_read_input_tokens
                                                                            as u64,
                                                                        trailing
                                                                            .cache_creation_input_tokens
                                                                            as u64,
                                                                    ),
                                                                cache_creation_tokens: trailing
                                                                    .cache_creation_input_tokens
                                                                    as u64,
                                                                cache_read_tokens: trailing
                                                                    .cache_read_input_tokens
                                                                    as u64,
                                                            }
                                                        );
                                                        break;
                                                    }
                                                }
                                                send_event!(
                                                    tx,
                                                    QueryEvent::TurnCompleted {
                                                        query_id,
                                                        turn_number: turn,
                                                        tokens_used: turn_tokens_used,
                                                    }
                                                );
                                                // Mark finalized so the post-loop safety net
                                                // doesn't short-circuit the next turn's API call.
                                                phase = StreamingPhase::Finalized;
                                                // A real tool turn happened — the model is
                                                // productively working. Reset the think-only
                                                // nudge budget so only *consecutive* think-only
                                                // responses exhaust it.
                                                think_only_nudges = 0;
                                                // Break from the streaming while-let loop so
                                                // tool results are processed on the next turn
                                                // iteration instead of consuming more events
                                                // (which could trigger the else branch and
                                                // save a duplicate assistant message).
                                                break;
                                            } else {
                                                // Parse-error recovery: model emitted a malformed
                                                // tool_call (no text content, no parsed tool inputs,
                                                // but a synthetic tool_result was queued and a
                                                // null-input ToolUse block with the real tool_use_id
                                                // was captured). Save the assistant message so the
                                                // next API call has the required
                                                // assistant(tool_use) → user(tool_result) sequence;
                                                // the agent loop drains tool_results on the next
                                                // iteration and the model retries with corrected JSON.
                                                //
                                                // This was the silent-task-loss root cause for 3/50
                                                // SWE-bench batch-3 tasks (matplotlib-23314,
                                                // sympy-12481, django-10914, all minimax provider).
                                                if assistant_text.is_empty()
                                                    && !assistant_tool_uses.is_empty()
                                                    && !tool_results.is_empty()
                                                {
                                                    let mut blocks: Vec<ContentBlock> = Vec::new();
                                                    blocks.append(&mut assistant_tool_uses);
                                                    conversation.messages.push(Message {
                                                        role: "assistant".to_string(),
                                                        content: MessageContent::Blocks(blocks),
                                                    });
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ConversationUpdate {
                                                            query_id,
                                                            messages: conversation.messages.clone(),
                                                        }
                                                    );
                                                    turn += 1;
                                                    phase = StreamingPhase::Finalized;
                                                    break;
                                                }

                                                // No tool uses — save assistant text to conversation
                                                if assistant_text.is_empty()
                                                    && total_output_tokens > 0
                                                {
                                                    tracing::warn!(
                                                        output_tokens = total_output_tokens,
                                                        "Model returned output tokens but no text content — context may be too small"
                                                    );
                                                    send_event!(tx, QueryEvent::Warning {
                                                        query_id,
                                                        message: "Model produced no text output — context window may be too small for this turn.".to_string(),
                                                    });
                                                }
                                                // Sentinel-usage guard: some providers
                                                // (notably MiniMax M-series) emit a
                                                // MessageDelta with usage={0,0,0} at the
                                                // finish_reason chunk and forward the real
                                                // usage in a SEPARATE later SSE frame. If
                                                // we finalize here on the zero-usage chunk,
                                                // the real numbers never reach the Cost
                                                // event. Defer when no tokens have been
                                                // seen yet on this turn — let the stream
                                                // keep going. When the real usage chunk
                                                // arrives (or MessageStop signals end of
                                                // stream) the safety-net path below will
                                                // catch up.
                                                let usage_had_real_tokens = usage.input_tokens > 0
                                                    || usage.output_tokens > 0;
                                                if !usage_had_real_tokens
                                                    && request_input_tokens == 0
                                                    && request_output_tokens == 0
                                                {
                                                    // Drop the empty-usage MessageDelta
                                                    // and keep reading. If the real usage
                                                    // chunk arrives, this arm will run
                                                    // again with real values and finalize
                                                    // normally. If the stream ends here
                                                    // (legacy OpenAI without usage), the
                                                    // safety net below will fire with
                                                    // totals=0.
                                                    continue;
                                                }
                                                // Truncated before finishing (output
                                                // token limit): reasoning-heavy models
                                                // (MiniMax M-series stream `<think>` in
                                                // content) can burn the entire output
                                                // budget on reasoning and get cut off
                                                // with zero tool calls, which used to
                                                // end the query as a silent no-op —
                                                // exit 0, nothing done (dogfood
                                                // 2026-08-23 l1-bulk-migrate). Keep the
                                                // partial response and ask the model to
                                                // pick up where it stopped instead.
                                                if is_truncation_stop(
                                                    assistant_stop_reason.as_deref(),
                                                ) && truncation_continuations
                                                    < MAX_TRUNCATION_CONTINUATIONS
                                                    && turn + 1 < config.max_turns
                                                {
                                                    truncation_continuations += 1;
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Warning {
                                                            query_id,
                                                            message: format!(
                                                                "Response cut off by the output token \
                                                             limit — continuing ({truncation_continuations}/{MAX_TRUNCATION_CONTINUATIONS})"
                                                            ),
                                                        }
                                                    );
                                                    if !assistant_text.is_empty()
                                                        || !assistant_tool_uses.is_empty()
                                                    {
                                                        // A13: persist the truncated
                                                        // assistant message with BOTH the
                                                        // text and any tool_use blocks
                                                        // captured before the cut (malformed
                                                        // tails arrive as null-input ToolUse
                                                        // paired with a synthetic result).
                                                        // Dropping the ToolUse here is what
                                                        // orphaned its tool_result on the
                                                        // next request (minimax 400 2013
                                                        // "tool result's tool id not found").
                                                        let mut blocks: Vec<ContentBlock> =
                                                            Vec::new();
                                                        if !assistant_text.is_empty() {
                                                            blocks.push(ContentBlock::Text {
                                                                text: std::mem::take(
                                                                    &mut assistant_text,
                                                                ),
                                                            });
                                                        }
                                                        blocks.append(&mut assistant_tool_uses);
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Blocks(blocks),
                                                        });
                                                    }
                                                    // A13: flush the results already executed
                                                    // for this truncated response BEFORE the
                                                    // continuation prompt, so the wire
                                                    // sequence is assistant(tool_use) →
                                                    // user(tool_result) →
                                                    // user(continuation). Draining at the
                                                    // next loop top (the old behavior)
                                                    // appended the result after the prompt,
                                                    // which strict providers reject as an
                                                    // orphaned tool_result. Pure-text
                                                    // truncations drain nothing here and are
                                                    // unchanged.
                                                    for entry in tool_results.drain(..) {
                                                        let content =
                                                            entry.to_tool_result_content();
                                                        conversation.messages.push(Message {
                                                            role: "user".to_string(),
                                                            content: MessageContent::Blocks(vec![
                                                                ContentBlock::ToolResult {
                                                                    tool_use_id: entry.tool_use_id,
                                                                    content,
                                                                    is_error: Some(entry.is_error),
                                                                },
                                                            ]),
                                                        });
                                                    }
                                                    conversation.messages.push(Message {
                                                        role: "user".to_string(),
                                                        content: MessageContent::Text(
                                                            TRUNCATION_CONTINUATION_PROMPT
                                                                .to_string(),
                                                        ),
                                                    });
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ConversationUpdate {
                                                            query_id,
                                                            messages: conversation.messages.clone(),
                                                        }
                                                    );
                                                    turn += 1;
                                                    phase = StreamingPhase::Finalized;
                                                    break;
                                                }
                                                // Think-only nudge (A1, eval-findings
                                                // 2026-09-glm): a response with no tool
                                                // calls and no substantive user-facing
                                                // answer (empty text, or reasoning-
                                                // dominated `<think>` output) used to
                                                // end the query as a silent no-op —
                                                // headless runs completed with an empty
                                                // patch. Re-prompt instead; bounded, and
                                                // the original end-of-query path below
                                                // still runs once the budget is used up.
                                                // Excluded: output-limit truncations —
                                                // the truncation continuation above owns
                                                // those (a `length`-cut unclosed
                                                // `<think>` looks think-only, but
                                                // nudging it would defeat the
                                                // truncation bound).
                                                if think_only_nudges < max_think_only_nudges
                                                    && turn + 1 < config.max_turns
                                                    && !is_truncation_stop(
                                                        assistant_stop_reason.as_deref(),
                                                    )
                                                    && is_think_only_response(
                                                        &assistant_text,
                                                        assistant_tool_uses.len(),
                                                        think_only_min_chars,
                                                    )
                                                {
                                                    think_only_nudges += 1;
                                                    // Persist the reasoning-dominated text
                                                    // so the next request keeps the
                                                    // assistant → user sequence for the
                                                    // nudge round.
                                                    if !assistant_text.is_empty() {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Text(
                                                                std::mem::take(&mut assistant_text),
                                                            ),
                                                        });
                                                    }
                                                    conversation.messages.push(Message {
                                                        role: "user".to_string(),
                                                        content: MessageContent::Text(
                                                            THINK_ONLY_NUDGE_PROMPT.to_string(),
                                                        ),
                                                    });
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Warning {
                                                            query_id,
                                                            message: format!(
                                                                "Model returned no tool call \
                                                                 and no final answer — \
                                                                 re-prompting \
                                                                 ({think_only_nudges}/{max_think_only_nudges})"
                                                            ),
                                                        }
                                                    );
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ConversationUpdate {
                                                            query_id,
                                                            messages: conversation.messages.clone(),
                                                        }
                                                    );
                                                    turn += 1;
                                                    phase = StreamingPhase::Finalized;
                                                    break;
                                                }
                                                if !assistant_text.is_empty() {
                                                    conversation.messages.push(Message {
                                                        role: "assistant".to_string(),
                                                        content: MessageContent::Text(
                                                            assistant_text,
                                                        ),
                                                    });
                                                }
                                                let total_cost = CostTracker::calculate_cost(
                                                    &client_model,
                                                    total_input_tokens,
                                                    total_output_tokens,
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Cost {
                                                        query_id,
                                                        total_cost_usd: total_cost,
                                                        input_tokens: total_input_tokens,
                                                        output_tokens: total_output_tokens,
                                                    }
                                                );
                                                let _ = tx
                                                    .send(Ok(QueryEvent::ConversationUpdate {
                                                        query_id,
                                                        messages: conversation.messages.clone(),
                                                    }))
                                                    .await;

                                                publish_stop_trigger(
                                                    &session_bus,
                                                    tool_results.len(),
                                                );
                                                let _ = tx
                                                    .send(Ok(QueryEvent::Completed { query_id }))
                                                    .await;

                                                return;
                                            }
                                        }
                                        StreamEvent::Error { message } => {
                                            // N-3: provider-reported mid-stream error,
                                            // surfaced typed by the API layer. Never
                                            // book this as a success: retry retryable
                                            // classes in place (same ladder as the A8
                                            // timeout path below), else fail the query —
                                            // even when partial text already streamed
                                            // (a provider-cut tail is not a usable
                                            // response; the old path recorded it as
                                            // Completed and headless exited 0).
                                            tracing::warn!(
                                                "LLM stream reported a provider error: {message}"
                                            );
                                            if recovery::provider_error_retryable(&message)
                                                && turn_retries_used < max_turn_retries
                                            {
                                                turn_retries_used += 1;
                                                tracing::warn!(
                                                    "Provider stream error is retryable; continuing turn {turn_retries_used}/{max_turn_retries}"
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Progress {
                                                        query_id,
                                                        message: format!(
                                                            "Provider stream error (upstream); continuing turn {turn_retries_used}/{max_turn_retries}"
                                                        ),
                                                    }
                                                );
                                                recovery::push_turn_continuation_nudge(
                                                    &mut conversation.messages,
                                                );
                                                continue 'agent_loop;
                                            }
                                            let has_partial = !assistant_text.is_empty()
                                                || !assistant_tool_uses.is_empty();
                                            if has_partial {
                                                let mut blocks: Vec<ContentBlock> = Vec::new();
                                                if !assistant_text.is_empty() {
                                                    blocks.push(ContentBlock::Text {
                                                        text: std::mem::take(&mut assistant_text),
                                                    });
                                                }
                                                blocks.append(&mut assistant_tool_uses);
                                                conversation.messages.push(Message {
                                                    role: "assistant".to_string(),
                                                    content: MessageContent::Blocks(blocks),
                                                });
                                                tracing::warn!(
                                                    "Provider stream error after partial response — preserving content, failing query"
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::ConversationUpdate {
                                                        query_id,
                                                        messages: conversation.messages.clone(),
                                                    }
                                                );
                                            }
                                            send_event!(
                                                tx,
                                                QueryEvent::Failed {
                                                    query_id,
                                                    error: format!(
                                                        "Provider stream error: {message}"
                                                    ),
                                                }
                                            );
                                            return;
                                        }
                                        StreamEvent::MessageStop => {}
                                        StreamEvent::Ping => {}
                                    }
                                }
                                Err(e) => {
                                    // A8/A8b: a timeout-class mid-stream death
                                    // (GLM coding-plan hard-cuts calls at
                                    // ~6min; smoke-2/4 RCA) or an abnormal
                                    // stream interruption (A8b/smoke-5: the
                                    // response body dies before any terminal
                                    // frame) is continued in place instead of
                                    // failing or masquerading as a completion.
                                    // This check intentionally precedes the
                                    // partial-content preservation below: a
                                    // cut tail is not a usable response
                                    // (saving it produced the empty/truncated
                                    // patches in the RCA), so on retry the
                                    // partial accumulation is discarded and
                                    // the model regenerates from the intact
                                    // history.
                                    if (e.is_timeout_class() || e.is_stream_interrupted())
                                        && turn_retries_used < max_turn_retries
                                    {
                                        turn_retries_used += 1;
                                        tracing::warn!(
                                            "Turn LLM call interrupted by timeout-class stream error ({e}); continuing turn {turn_retries_used}/{max_turn_retries}"
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::Progress {
                                                query_id,
                                                message: format!(
                                                    "Turn LLM call interrupted (upstream cutoff); continuing turn {turn_retries_used}/{max_turn_retries}"
                                                ),
                                            }
                                        );
                                        recovery::push_turn_continuation_nudge(
                                            &mut conversation.messages,
                                        );
                                        continue 'agent_loop;
                                    } // A14: escalate the stream-idle watchdog budget so the
                                    // continuation attempt is not killed by the same 420s
                                    // base budget that killed the previous attempt (w7
                                    // csstree/expr/superjson — three consecutive runs all
                                    // died at exactly 421s = base + 1s).
                                    recovery::escalate_stream_idle_override(
                                        &client,
                                        stream_idle_base_secs,
                                        turn_retries_used,
                                    );

                                    // Content-first: if partial content was streamed before the error,
                                    // preserve it immediately. Local models (Ollama) often generate
                                    // valid text before hitting a malformed tool-call error, and
                                    // retrying is expensive (new HTTP request) and may hang.
                                    let has_partial = !assistant_text.is_empty()
                                        || !assistant_tool_uses.is_empty();
                                    if has_partial {
                                        let partial_len = assistant_text.len();
                                        let mut blocks: Vec<ContentBlock> = Vec::new();
                                        if !assistant_text.is_empty() {
                                            blocks.push(ContentBlock::Text {
                                                text: std::mem::take(&mut assistant_text),
                                            });
                                        }
                                        blocks.append(&mut assistant_tool_uses);
                                        conversation.messages.push(Message {
                                            role: "assistant".to_string(),
                                            content: MessageContent::Blocks(blocks),
                                        });
                                        tracing::warn!(
                                            "Stream error after partial response ({partial_len} chars) — preserving content"
                                        );
                                        let suggestion = e
                                            .user_suggestion()
                                            .map(|s| format!(" {s}"))
                                            .unwrap_or_default();
                                        let warning_msg = if suggestion.is_empty() {
                                            "Stream ended unexpectedly. Partial response preserved."
                                                .to_string()
                                        } else {
                                            format!("Stream ended unexpectedly.{suggestion}")
                                        };
                                        send_event!(
                                            tx,
                                            QueryEvent::Warning {
                                                query_id,
                                                message: warning_msg,
                                            }
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::ConversationUpdate {
                                                query_id,
                                                messages: conversation.messages.clone(),
                                            }
                                        );
                                        publish_stop_trigger(&session_bus, tool_results.len());
                                        send_event!(tx, QueryEvent::Completed { query_id });

                                        return;
                                    }

                                    // No partial content — retry without tools for Ollama models
                                    // that can't handle tool-call formatting.
                                    // Use non-streaming mode: Ollama may return HTTP 200 with content
                                    // even when the model generates malformed output, unlike streaming
                                    // mode which can return HTTP 500 immediately.
                                    if e.is_ollama_malformed_output() {
                                        tracing::warn!(
                                            "Ollama malformed output (no partial content), retrying without tools (non-streaming): {e}"
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::Progress {
                                                query_id,
                                                message: "Retrying without tools...".to_string(),
                                            }
                                        );
                                        let no_tools: Option<
                                            Vec<shannon_engine::api::ToolDefinition>,
                                        > = None;
                                        let no_system: Option<String> = None;
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(60),
                                            client.send_message(
                                                messages.clone(),
                                                no_tools,
                                                no_system,
                                            ),
                                        )
                                        .await
                                        {
                                            Ok(Ok(content_blocks)) => {
                                                // If full-history retry returned an Ollama error
                                                // warning, try once more with just the last user
                                                // message — tiny models may choke on long history.
                                                let is_ollama_warning = content_blocks.iter().any(|b| {
                                                    matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                                });
                                                let final_blocks = if is_ollama_warning {
                                                    // Keep last 2 turns (up to 4 messages) so the
                                                    // model has enough context to answer the follow-up.
                                                    let minimal: Vec<Message> = {
                                                        let msgs: Vec<&Message> =
                                                            messages.iter().rev().take(4).collect();
                                                        msgs.into_iter().cloned().collect()
                                                    };
                                                    tracing::warn!(
                                                        "Ollama retry still errored, last-resort with last {} msgs (of {})",
                                                        minimal.len(),
                                                        messages.len()
                                                    );
                                                    match tokio::time::timeout(
                                                        std::time::Duration::from_secs(60),
                                                        client.send_message(minimal, None, None),
                                                    ).await {
                                                        Ok(Ok(blocks)) if !blocks.iter().any(|b| {
                                                            matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                                        }) => blocks,
                                                        _ => {
                                                            send_event!(tx, QueryEvent::Failed {
                                                                query_id,
                                                                error: "This model cannot produce valid output — it may be too small or incompatible. Try /model to switch to a larger model.".to_string(),
                                                            });
                                                            return;
                                                        }
                                                    }
                                                } else {
                                                    content_blocks
                                                };

                                                let mut retry_text = String::new();
                                                for block in &final_blocks {
                                                    if let ContentBlock::Text { text } = block {
                                                        retry_text.push_str(text);
                                                        let mut display = text.clone();
                                                        crate::secret_guard::restore_display_for_output(&mut display);
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Text {
                                                                query_id,
                                                                content: display,
                                                            }
                                                        );
                                                    }
                                                }
                                                if !retry_text.is_empty() {
                                                    let already_added = conversation.messages.last()
                                                        .map(|m| matches!(&m.content, MessageContent::Text(t) if t == &retry_text))
                                                        .unwrap_or(false);
                                                    if !already_added {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Text(
                                                                retry_text,
                                                            ),
                                                        });
                                                    }
                                                }
                                                let total_cost = CostTracker::calculate_cost(
                                                    &client_model,
                                                    total_input_tokens,
                                                    total_output_tokens,
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Cost {
                                                        query_id,
                                                        total_cost_usd: total_cost,
                                                        input_tokens: total_input_tokens,
                                                        output_tokens: total_output_tokens,
                                                    }
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::ConversationUpdate {
                                                        query_id,
                                                        messages: conversation.messages.clone(),
                                                    }
                                                );
                                                publish_stop_trigger(
                                                    &session_bus,
                                                    tool_results.len(),
                                                );
                                                send_event!(tx, QueryEvent::Completed { query_id });

                                                return;
                                            }
                                            Ok(Err(retry_err)) => {
                                                tracing::warn!(
                                                    "Non-streaming retry error: {retry_err}"
                                                );
                                                let error_msg = if retry_err
                                                    .is_ollama_malformed_output()
                                                {
                                                    "This model cannot produce valid output — it may be too small, corrupted, or incompatible. Try /model to switch.".to_string()
                                                } else {
                                                    format!(
                                                        "Local model error — retry without tools failed: {retry_err}"
                                                    )
                                                };
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Failed {
                                                        query_id,
                                                        error: error_msg,
                                                    }
                                                );
                                                return;
                                            }
                                            Err(_) => {
                                                tracing::warn!(
                                                    "Non-streaming retry timed out (60s)"
                                                );
                                                send_event!(tx, QueryEvent::Failed {
                                                    query_id,
                                                    error: "Local model error — retry timed out. The model may be loading, try again.".to_string(),
                                                });
                                                return;
                                            }
                                        }
                                    }

                                    // No partial content, non-recoverable — fail.
                                    // Keep the ORIGINAL error text (not just the
                                    // suggestion) so consumers can classify the
                                    // failure — headless exit codes match on
                                    // "rate limit" / "timed out" substrings of
                                    // the provider error.
                                    let suggestion = e
                                        .user_suggestion()
                                        .map(|s| format!(" {s}"))
                                        .unwrap_or_default();
                                    let user_error = if suggestion.is_empty() {
                                        format!("{e}")
                                    } else {
                                        format!("{e}.{suggestion}")
                                    };
                                    send_event!(
                                        tx,
                                        QueryEvent::Failed {
                                            query_id,
                                            error: user_error,
                                        }
                                    );
                                    return;
                                }
                            }
                        }

                        // The stream completed without a terminal error —
                        // this LLM call succeeded, so the A8 continuation
                        // budget is whole again for the next turn (per-turn
                        // budget, not per-query).
                        turn_retries_used = 0;
                        // A14: clear any watchdog escalation so the next
                        // fresh turn starts from the base budget again.
                        recovery::clear_stream_idle_override(&client);

                        // Parse-error recovery: when the model emitted a malformed
                        // tool_call (no text content, no successfully-parsed tool
                        // inputs, but a synthetic tool_result was queued and a
                        // null-input ToolUse block with the real tool_use_id was
                        // captured), persist the assistant message so the next API
                        // call has the required assistant(tool_use) →
                        // user(tool_result) sequence. The agent loop drains
                        // tool_results on the next iteration, the model sees
                        // "Malformed tool input" as a tool_result, and retries
                        // with corrected JSON.
                        //
                        // This was the silent-task-loss root cause for 3/50
                        // SWE-bench batch-3 tasks (matplotlib-23314, sympy-12481,
                        // django-10914, all minimax provider).
                        if !has_content
                            && tool_inputs.is_empty()
                            && !assistant_tool_uses.is_empty()
                            && !tool_results.is_empty()
                            && phase != StreamingPhase::Finalized
                        {
                            let mut blocks: Vec<ContentBlock> = Vec::new();
                            blocks.append(&mut assistant_tool_uses);
                            conversation.messages.push(Message {
                                role: "assistant".to_string(),
                                content: MessageContent::Blocks(blocks),
                            });
                            send_event!(
                                tx,
                                QueryEvent::ConversationUpdate {
                                    query_id,
                                    messages: conversation.messages.clone(),
                                }
                            );
                            turn += 1;
                            continue;
                        }

                        // Think-only nudge (A1): the stream produced no tool
                        // calls and no text at all — e.g. reasoning arrived
                        // only via thinking deltas, or the completion was
                        // empty (including the zero-usage deferred
                        // MessageDelta case). Re-prompt instead of ending the
                        // query as a silent no-op; when the budget is
                        // exhausted the bail-out below fires unchanged.
                        // Output-limit truncations stay with the truncation
                        // machinery and are never nudged.
                        if !has_content
                            && tool_inputs.is_empty()
                            && assistant_tool_uses.is_empty()
                            && phase != StreamingPhase::Finalized
                            && !is_truncation_stop(assistant_stop_reason.as_deref())
                            && think_only_nudges < max_think_only_nudges
                            && turn + 1 < config.max_turns
                        {
                            think_only_nudges += 1;
                            conversation.messages.push(Message {
                                role: "user".to_string(),
                                content: MessageContent::Text(THINK_ONLY_NUDGE_PROMPT.to_string()),
                            });
                            send_event!(
                                tx,
                                QueryEvent::Warning {
                                    query_id,
                                    message: format!(
                                        "Model returned no tool call and no final \
                                         answer — re-prompting \
                                         ({think_only_nudges}/{max_think_only_nudges})"
                                    ),
                                }
                            );
                            send_event!(
                                tx,
                                QueryEvent::ConversationUpdate {
                                    query_id,
                                    messages: conversation.messages.clone(),
                                }
                            );
                            turn += 1;
                            continue;
                        }

                        // Bail out only when the model produced NOTHING usable
                        // (no text content, no tool_uses, no parsed tool inputs).
                        // The parse-error recovery block above handles the case
                        // where a synthetic null-input ToolUse was queued.
                        if !has_content
                            && tool_inputs.is_empty()
                            && phase != StreamingPhase::Finalized
                        {
                            let total_cost = CostTracker::calculate_cost(
                                &client_model,
                                total_input_tokens,
                                total_output_tokens,
                            );
                            send_event!(
                                tx,
                                QueryEvent::Cost {
                                    query_id,
                                    total_cost_usd: total_cost,
                                    input_tokens: total_input_tokens,
                                    output_tokens: total_output_tokens,
                                }
                            );
                            send_event!(
                                tx,
                                QueryEvent::ConversationUpdate {
                                    query_id,
                                    messages: conversation.messages.clone(),
                                }
                            );
                            publish_stop_trigger(&session_bus, tool_results.len());
                            send_event!(tx, QueryEvent::Completed { query_id });

                            return;
                        }

                        // Safety net: if the stream had content but the MessageDelta
                        // handler didn't finalize (e.g. budget exceeded, premature
                        // stream close, or missing stop event), save the assistant
                        // response now so the next turn retains context.
                        if phase == StreamingPhase::Receiving && has_content {
                            let has_text = !assistant_text.is_empty();
                            let has_tool_uses = !assistant_tool_uses.is_empty();
                            // Decide the think-only nudge (A1) before the save
                            // block below moves `assistant_text`.
                            let think_only_hit = is_think_only_response(
                                &assistant_text,
                                assistant_tool_uses.len(),
                                think_only_min_chars,
                            );
                            if has_text || has_tool_uses {
                                // Check if the last message is already this assistant response
                                let already_saved = conversation.messages.last().is_some_and(|m| {
                                    matches!(&m.content, MessageContent::Text(t) if has_text && t == &assistant_text)
                                        || matches!(&m.content, MessageContent::Blocks(blocks)
                                            if blocks.len() == assistant_tool_uses.len() + if has_text { 1 } else { 0 })
                                });
                                if !already_saved {
                                    tracing::warn!(
                                        text_len = assistant_text.len(),
                                        tool_uses = assistant_tool_uses.len(),
                                        "Stream ended without finalization — saving assistant response as safety net"
                                    );
                                    let mut blocks: Vec<ContentBlock> = Vec::new();
                                    if has_text {
                                        blocks.push(ContentBlock::Text {
                                            text: assistant_text,
                                        });
                                    }
                                    blocks.append(&mut assistant_tool_uses);
                                    conversation.messages.push(Message {
                                        role: "assistant".to_string(),
                                        content: MessageContent::Blocks(blocks),
                                    });
                                }
                            }
                            // Same truncation recovery as the MessageDelta
                            // finalize path above, for providers whose stream
                            // ends without a usable usage frame (finalized
                            // here by the safety net instead).
                            if is_truncation_stop(assistant_stop_reason.as_deref())
                                && truncation_continuations < MAX_TRUNCATION_CONTINUATIONS
                                && turn + 1 < config.max_turns
                            {
                                truncation_continuations += 1;
                                send_event!(
                                    tx,
                                    QueryEvent::Warning {
                                        query_id,
                                        message: format!(
                                            "Response cut off by the output token limit — \
                                         continuing ({truncation_continuations}/{MAX_TRUNCATION_CONTINUATIONS})"
                                        ),
                                    }
                                );
                                // A13: the truncated response's tool results
                                // (already executed inline) must land BEFORE
                                // the continuation prompt — assistant(tool_use)
                                // → user(tool_result) → user(continuation) — or
                                // strict providers (minimax 400 2013) reject the
                                // result as orphaned. Pure-text truncations
                                // drain nothing here and are unchanged.
                                for entry in tool_results.drain(..) {
                                    let content = entry.to_tool_result_content();
                                    conversation.messages.push(Message {
                                        role: "user".to_string(),
                                        content: MessageContent::Blocks(vec![
                                            ContentBlock::ToolResult {
                                                tool_use_id: entry.tool_use_id,
                                                content,
                                                is_error: Some(entry.is_error),
                                            },
                                        ]),
                                    });
                                }
                                conversation.messages.push(Message {
                                    role: "user".to_string(),
                                    content: MessageContent::Text(
                                        TRUNCATION_CONTINUATION_PROMPT.to_string(),
                                    ),
                                });
                                send_event!(
                                    tx,
                                    QueryEvent::ConversationUpdate {
                                        query_id,
                                        messages: conversation.messages.clone(),
                                    }
                                );
                                turn += 1;
                                continue;
                            }
                            // Think-only nudge (A1): a reasoning-only response
                            // finalized by this safety net (stream closed
                            // without a usable MessageDelta). The assistant
                            // text was already persisted above; append the
                            // nudge. Same bounded budget as the finalize
                            // path; output-limit truncations (handled just
                            // above) are never nudged.
                            if think_only_hit
                                && !is_truncation_stop(assistant_stop_reason.as_deref())
                                && think_only_nudges < max_think_only_nudges
                                && turn + 1 < config.max_turns
                            {
                                think_only_nudges += 1;
                                conversation.messages.push(Message {
                                    role: "user".to_string(),
                                    content: MessageContent::Text(
                                        THINK_ONLY_NUDGE_PROMPT.to_string(),
                                    ),
                                });
                                send_event!(
                                    tx,
                                    QueryEvent::Warning {
                                        query_id,
                                        message: format!(
                                            "Model returned no tool call and no \
                                             final answer — re-prompting \
                                             ({think_only_nudges}/{max_think_only_nudges})"
                                        ),
                                    }
                                );
                                send_event!(
                                    tx,
                                    QueryEvent::ConversationUpdate {
                                        query_id,
                                        messages: conversation.messages.clone(),
                                    }
                                );
                                turn += 1;
                                continue;
                            }
                            let total_cost = CostTracker::calculate_cost(
                                &client_model,
                                total_input_tokens,
                                total_output_tokens,
                            );
                            send_event!(
                                tx,
                                QueryEvent::Cost {
                                    query_id,
                                    total_cost_usd: total_cost,
                                    input_tokens: total_input_tokens,
                                    output_tokens: total_output_tokens,
                                }
                            );
                            send_event!(
                                tx,
                                QueryEvent::ConversationUpdate {
                                    query_id,
                                    messages: conversation.messages.clone(),
                                }
                            );
                            publish_stop_trigger(&session_bus, tool_results.len());
                            send_event!(tx, QueryEvent::Completed { query_id });

                            return;
                        }
                    }
                    Err(e) => {
                        // A8/A8b: timeout-class stream-establishment death is
                        // continued in place (same rationale as the
                        // mid-stream site above; the GLM coding-plan gateway
                        // hard-cuts ~6min calls before a single byte of the
                        // response arrives). History and prior tool state
                        // are untouched; only the continuation nudge is
                        // added.
                        if (e.is_timeout_class() || e.is_stream_interrupted())
                            && turn_retries_used < max_turn_retries
                        {
                            turn_retries_used += 1;
                            tracing::warn!(
                                "Turn LLM call interrupted by timeout-class error ({e}); continuing turn {turn_retries_used}/{max_turn_retries}"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: format!(
                                        "Turn LLM call interrupted (upstream cutoff); continuing turn {turn_retries_used}/{max_turn_retries}"
                                    ),
                                }
                            );
                            recovery::push_turn_continuation_nudge(&mut conversation.messages);
                            continue 'agent_loop;
                        } // A14: escalate the stream-idle watchdog budget so the
                        // continuation attempt is not killed by the same 420s
                        // base budget that killed the previous attempt (w7
                        // csstree/expr/superjson — three consecutive runs all
                        // died at exactly 421s = base + 1s).
                        recovery::escalate_stream_idle_override(
                            &client,
                            stream_idle_base_secs,
                            turn_retries_used,
                        );

                        // Check if this is a token overflow — attempt auto-compaction and retry once
                        if e.is_token_overflow() {
                            let compact_keep = config.keep_recent_messages;
                            if messages.len() > compact_keep {
                                tracing::warn!(
                                    "Token overflow detected, auto-compacting and retrying"
                                );
                                let split = shannon_engine::compact::safe_split_point(
                                    &messages,
                                    messages.len() - compact_keep,
                                );
                                messages = messages.split_off(split);
                                // Re-inject system prompt at front
                                if let Some(ref sp) = system_prompt {
                                    if !sp.is_empty() {
                                        messages.insert(
                                            0,
                                            shannon_engine::api::Message {
                                                role: "system".to_string(),
                                                content: shannon_engine::api::MessageContent::Text(
                                                    sp.clone(),
                                                ),
                                            },
                                        );
                                    }
                                }
                                // Compaction rebuilt `messages` — re-apply the
                                // transform so the post-compaction send (and
                                // the history sync below) stay surrogate-
                                // consistent (blueprint §5.4).
                                let messages =
                                    crate::secret_guard::transform_outgoing_messages(messages);
                                // Sync compacted messages back to conversation
                                // so ConversationUpdate reflects the actual state
                                conversation.messages = messages.clone();

                                let retry_result = if let Some(ref blocks) = system_blocks_opt {
                                    client
                                        .send_message_stream_structured_with_retry(
                                            messages.clone(),
                                            tools_schema.clone(),
                                            blocks.clone(),
                                        )
                                        .await
                                } else {
                                    client
                                        .send_message_stream_with_retry(
                                            messages.clone(),
                                            tools_schema.clone(),
                                            system_prompt.clone(),
                                        )
                                        .await
                                };
                                match retry_result {
                                    Ok(mut retry_stream) => {
                                        // Re-process the retry stream — extract text content and
                                        // accumulate into conversation so the response isn't lost
                                        let mut retry_text = String::new();
                                        while let Some(event_result) = retry_stream.next().await {
                                            match event_result {
                                                Ok(StreamEvent::ContentBlockDelta {
                                                    delta,
                                                    ..
                                                }) => {
                                                    if let ContentDelta::TextDelta { text } = delta
                                                    {
                                                        retry_text.push_str(&text);
                                                        let mut display = text.clone();
                                                        crate::secret_guard::restore_display_for_output(&mut display);
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Text {
                                                                query_id,
                                                                content: display,
                                                            }
                                                        );
                                                    }
                                                }
                                                Ok(StreamEvent::MessageDelta { delta, .. }) => {
                                                    if delta.stop_reason.as_deref()
                                                        == Some("end_turn")
                                                    {
                                                        // Add the retry response to conversation before sending update
                                                        if !retry_text.is_empty() {
                                                            conversation.messages.push(Message {
                                                                role: "assistant".to_string(),
                                                                content: MessageContent::Text(
                                                                    retry_text.clone(),
                                                                ),
                                                            });
                                                        }
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ConversationUpdate {
                                                                query_id,
                                                                messages: conversation
                                                                    .messages
                                                                    .clone(),
                                                            }
                                                        );
                                                        publish_stop_trigger(
                                                            &session_bus,
                                                            tool_results.len(),
                                                        );
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Completed { query_id }
                                                        );
                                                    }
                                                }
                                                Ok(_) => {} // Ping, MessageStart, MessageStop, etc.
                                                Err(retry_err) => {
                                                    // Save partial retry text before failing
                                                    if !retry_text.is_empty() {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Text(
                                                                retry_text,
                                                            ),
                                                        });
                                                    }
                                                    let suggestion = retry_err
                                                        .user_suggestion()
                                                        .map(|s| format!(" {s}"))
                                                        .unwrap_or_default();
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ConversationUpdate {
                                                            query_id,
                                                            messages: conversation.messages.clone(),
                                                        }
                                                    );
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Failed {
                                                            query_id,
                                                            error: format!(
                                                                "Auto-compact retry also failed: {retry_err}.{suggestion}"
                                                            ),
                                                        }
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                        // Stream ended without end_turn — still add the response
                                        if !retry_text.is_empty() {
                                            conversation.messages.push(Message {
                                                role: "assistant".to_string(),
                                                content: MessageContent::Text(retry_text),
                                            });
                                        }
                                        send_event!(
                                            tx,
                                            QueryEvent::ConversationUpdate {
                                                query_id,
                                                messages: conversation.messages.clone(),
                                            }
                                        );
                                        publish_stop_trigger(&session_bus, tool_results.len());
                                        send_event!(tx, QueryEvent::Completed { query_id });
                                        return;
                                    }
                                    Err(retry_err) => {
                                        let suggestion = retry_err
                                            .user_suggestion()
                                            .map(|s| format!(" {s}"))
                                            .unwrap_or_default();
                                        send_event!(
                                            tx,
                                            QueryEvent::ConversationUpdate {
                                                query_id,
                                                messages: conversation.messages.clone(),
                                            }
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::Failed {
                                                query_id,
                                                error: format!(
                                                    "Token overflow — auto-compact retry failed: {retry_err}.{suggestion}"
                                                ),
                                            }
                                        );
                                        return;
                                    }
                                }
                            }
                        }
                        // Ollama HTTP 500 with malformed output — retry without tools.
                        // Use non-streaming mode: Ollama may return HTTP 200 with content
                        // even when the model generates malformed output, unlike streaming
                        // mode which can return HTTP 500 immediately.
                        // Strip system prompt to prevent small models from attempting tool calls.
                        if e.is_ollama_malformed_output() {
                            tracing::warn!(
                                "Ollama HTTP error (malformed output), retrying without tools (non-streaming): {e}"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: "Retrying without tools...".to_string(),
                                }
                            );
                            let no_tools: Option<Vec<shannon_engine::api::ToolDefinition>> = None;
                            let no_system: Option<String> = None;
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(60),
                                client.send_message(messages.clone(), no_tools, no_system),
                            )
                            .await
                            {
                                Ok(Ok(content_blocks)) => {
                                    // If full-history retry returned an Ollama error
                                    // warning, try once more with just the last user
                                    // message — tiny models may choke on long history.
                                    let is_ollama_warning = content_blocks.iter().any(|b| {
                                        matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                    });
                                    let final_blocks = if is_ollama_warning {
                                        let minimal: Vec<Message> = messages
                                            .iter()
                                            .rev()
                                            .find(|m| m.role == "user")
                                            .cloned()
                                            .map(|m| vec![m])
                                            .unwrap_or_else(|| messages.clone());
                                        tracing::warn!(
                                            "Ollama retry still errored, last-resort minimal input ({}/{} msgs)",
                                            minimal.len(),
                                            messages.len()
                                        );
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(60),
                                            client.send_message(minimal, None, None),
                                        ).await {
                                            Ok(Ok(blocks)) if !blocks.iter().any(|b| {
                                                matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                            }) => blocks,
                                            _ => {
                                                send_event!(tx, QueryEvent::Failed {
                                                    query_id,
                                                    error: "This model cannot produce valid output — it may be too small or incompatible. Try /model to switch to a larger model.".to_string(),
                                                });
                                                return;
                                            }
                                        }
                                    } else {
                                        content_blocks
                                    };

                                    let mut retry_text = String::new();
                                    for block in &final_blocks {
                                        if let ContentBlock::Text { text } = block {
                                            retry_text.push_str(text);
                                            let mut display = text.clone();
                                            crate::secret_guard::restore_display_for_output(
                                                &mut display,
                                            );
                                            send_event!(
                                                tx,
                                                QueryEvent::Text {
                                                    query_id,
                                                    content: display,
                                                }
                                            );
                                        }
                                    }
                                    if !retry_text.is_empty() {
                                        conversation.messages.push(Message {
                                            role: "assistant".to_string(),
                                            content: MessageContent::Text(retry_text),
                                        });
                                    }
                                    let total_cost = CostTracker::calculate_cost(
                                        &client_model,
                                        total_input_tokens,
                                        total_output_tokens,
                                    );
                                    send_event!(
                                        tx,
                                        QueryEvent::Cost {
                                            query_id,
                                            total_cost_usd: total_cost,
                                            input_tokens: total_input_tokens,
                                            output_tokens: total_output_tokens,
                                        }
                                    );
                                    send_event!(
                                        tx,
                                        QueryEvent::ConversationUpdate {
                                            query_id,
                                            messages: conversation.messages.clone(),
                                        }
                                    );
                                    publish_stop_trigger(&session_bus, tool_results.len());
                                    send_event!(tx, QueryEvent::Completed { query_id });

                                    return;
                                }
                                Ok(Err(retry_err)) => {
                                    tracing::warn!("Non-streaming retry error: {retry_err}");
                                    let error_msg = if retry_err.is_ollama_malformed_output() {
                                        "This model cannot produce valid output — it may be too small, corrupted, or incompatible. Try /model to switch.".to_string()
                                    } else {
                                        format!(
                                            "Local model error — retry without tools failed: {retry_err}"
                                        )
                                    };
                                    send_event!(
                                        tx,
                                        QueryEvent::Failed {
                                            query_id,
                                            error: error_msg,
                                        }
                                    );
                                    return;
                                }
                                Err(_) => {
                                    tracing::warn!("Non-streaming retry timed out (60s)");
                                    send_event!(tx, QueryEvent::Failed {
                                        query_id,
                                        error: "Local model error — retry timed out. The model may be loading, try again.".to_string(),
                                    });
                                    return;
                                }
                            }
                        }
                        let suggestion = e
                            .user_suggestion()
                            .map(|s| format!(" {s}"))
                            .unwrap_or_default();
                        let user_error = if suggestion.is_empty() {
                            format!("{e}")
                        } else {
                            suggestion
                        };
                        send_event!(
                            tx,
                            QueryEvent::ConversationUpdate {
                                query_id,
                                messages: conversation.messages.clone(),
                            }
                        );
                        send_event!(
                            tx,
                            QueryEvent::Failed {
                                query_id,
                                error: user_error,
                            }
                        );
                        return;
                    }
                }
            }

            // Post-query: memory extraction via AutoDreamService.
            // INCREMENTAL (P0-10): only the post-cursor delta is extracted.
            // The previous full-conversation rescan on every query re-matched
            // old facts each turn, refreshing `accessed_at` on noise and
            // spawning paraphrase siblings at a rate the 0.8 Jaccard dedup
            // could not absorb.
            //
            // Honors the `auto_memory` switches (feature flag / config.toml)
            // — previously extraction ran unconditionally and could not be
            // turned off. Runs on the blocking pool and logs outcomes:
            // the old `tokio::spawn` + `let _ =` silently discarded every
            // failure, and an async task could be cancelled mid-extraction
            // when the runtime shut down (dropped query stream), losing the
            // batch. Blocking tasks are not cancelled at shutdown, and the
            // extraction cursor only advances on success so a failed batch
            // is retried by the next query (dedup absorbs re-runs).
            if let Some(ref mem_store) = memory_for_extraction {
                if crate::memory::auto_memory_enabled() {
                    let store_arc = mem_store.clone();
                    let total = conversation.messages.len();
                    let cursor = memory_extract_cursor_cursor.min(total);
                    let delta: Vec<Message> = conversation.messages[cursor..].to_vec();
                    // P2-4 provenance: stamp extracted entries with the session
                    // that produced them so the Memory page can jump back.
                    let session_for_extraction = self_session_id.clone();
                    let project = memory_project_key.clone();
                    let cursor_cell = memory_extract_cursor_cell.clone();
                    tokio::spawn(async move {
                        if delta.is_empty() {
                            return;
                        }
                        let joined = tokio::task::spawn_blocking(move || {
                            let dream = AutoDreamService::new(store_arc);
                            let extracted = dream.process_conversation_with_session(
                                &delta,
                                &project,
                                Some(&session_for_extraction),
                            );
                            // Periodic compaction (ADR-0010 C5'): dedupe +
                            // prune + size control, gated by a persisted
                            // sidecar schedule. Each query is one session;
                            // compaction fires at ~24 h or ≥ 5 sessions.
                            let compacted =
                                dream.maybe_compact(&project, &SessionMemoryConfig::default());
                            (extracted, compacted)
                        })
                        .await;
                        match joined {
                            Ok((extracted, compaction)) => match extracted {
                                Ok(entries) => {
                                    cursor_cell.store(total, std::sync::atomic::Ordering::Relaxed);
                                    if let Err(e) = compaction {
                                        tracing::warn!(error = %e, "memory compaction failed");
                                    }
                                    tracing::debug!(
                                        extracted = entries.len(),
                                        "auto-memory extraction complete"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "auto-memory extraction failed");
                                }
                            },
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "auto-memory extraction task failed to join"
                                );
                            }
                        }
                    });
                }
            }
        }));

        // Convert the channel receiver into a stream that aborts the producer
        // task when dropped, so a consumer can cancel an in-progress query by
        // dropping the `QueryStream` (used by the API server's WS cancel path).
        let stream = stream::unfold(rx, move |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });

        Box::pin(AbortOnDropStream::new(stream, producer))
    }
}

// ─── Auto-test loop (P1-5) ──────────────────────────────────────────────────
//
// Wired into the main agent loop: after a successful file-modifying tool
// (`Edit`/`Write`) the engine invokes `maybe_run_auto_test` to run the
// configured test command and, on failure, inject the result into the next
// LLM context so the model can fix the code.

/// Run one auto-test iteration if appropriate.
///
/// Called after each successful file-modifying tool. Pushes a user notice
/// into `user_notices` describing what happened so the next API call sees
/// it. Returns `()`; loop-state lives in `auto_test_state`.
async fn maybe_run_auto_test(
    cfg: &crate::auto_test::AutoTestConfig,
    state: &mut crate::auto_test::AntiLoopState,
    user_notices: &mut Vec<String>,
    tx: &EventTx,
    query_id: Uuid,
) {
    // Emit a progress event so the UI shows the auto-test is running.
    send_event!(
        tx,
        QueryEvent::Progress {
            query_id,
            message: format!(
                "auto-test: running configured test command (iteration {})",
                state.iterations + 1
            ),
        }
    );

    let project_dir = crate::auto_test::project_dir();
    let outcome = match crate::auto_test::run_auto_test(cfg, &project_dir).await {
        Some(o) => o,
        None => {
            // No command could be resolved — silently skip. The user hasn't
            // configured a project that auto-detection can map to a test runner.
            return;
        }
    };

    let decision = state.record(cfg, &outcome);

    // Delivered as a plain user notice so the next API call sees it. A
    // synthetic `user(tool_result)` would reference a tool_use_id that never
    // appeared in any assistant message, which providers reject with
    // 400 "unexpected tool_use_id".
    let description = outcome.describe();
    user_notices.push(description);

    // Emit a structured progress event with the outcome so the UI can show
    // pass/fail badges without parsing the description string.
    send_event!(
        tx,
        QueryEvent::Progress {
            query_id,
            message: format!(
                "auto-test: {} ({} iter, reason: {})",
                match &outcome {
                    crate::auto_test::TestOutcome::Passed => "passed",
                    crate::auto_test::TestOutcome::Failed { .. } => "failed",
                    crate::auto_test::TestOutcome::TimedOut => "timed out",
                    crate::auto_test::TestOutcome::SpawnError(_) => "spawn error",
                },
                state.iterations,
                match &decision {
                    crate::auto_test::LoopDecision::Continue => "continue",
                    crate::auto_test::LoopDecision::Stop(r) => r.as_str(),
                }
            ),
        }
    );
}
