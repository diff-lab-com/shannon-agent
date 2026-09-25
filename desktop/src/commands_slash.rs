//! Slash-command backends — the desktop counterparts of the REPL's session
//! commands that have no other Tauri surface yet:
//!   /context       → [`get_session_context_stats`] (tokens used vs context
//!                    window)
//!   /diff          → [`get_session_git_diff`] (working-tree diff of the
//!                    session's working directory)
//!   /dream `days`  → [`dream_slash`] (one dream pass; default 3-day window)
//!   /detect-skills → [`detect_slash`] (heuristic candidate detection)
//! /cost lives in `commands_usage` (it owns the ledger), /export and the
//! navigation commands are pure frontend.
//!
//! `/context` and `/diff` are read-only diagnostics. `/dream` and
//! `/detect-skills` deliberately write review-gated artifacts only (shadow
//! proposals / reports / candidate queue entries) — nothing lands in the
//! memory store or the skill catalog without explicit user approval.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use shannon_core::query_engine::QueryEngine;
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::PermissionManager;
use shannon_engine::state::StateManager;

use crate::commands::AppState;

/// Context-window usage of one session, for the /context slash command.
#[derive(Debug, Clone, Serialize)]
pub struct SessionContextStats {
    /// CJK-aware estimate of the projected conversation (incl. system prompt),
    /// the same estimator that drives the engine's compression threshold.
    pub estimated_tokens: usize,
    /// `None` when the window is genuinely unknown (no override, no live
    /// Ollama num_ctx, model absent from the registry) — the UI renders
    /// "unknown" instead of a fabricated fallback.
    pub context_window: Option<usize>,
}

/// Build a throwaway engine bound to `session_id` and restore its L0
/// projection. Reuses the engine stashed on the session when present (it
/// already carries the resolved system prompt / context-window overrides);
/// otherwise constructs a minimal one — the client is never contacted, the
/// engine is only consulted for its local estimators.
pub(crate) async fn restored_engine(
    state: &AppState,
    session_id: uuid::Uuid,
) -> Result<QueryEngine, String> {
    let session = state
        .registry
        .get(crate::session_registry::SessionKey(session_id))
        .ok_or_else(|| format!("unknown session {session_id}"))?;

    let stashed = session.query_engine.lock().await.clone();
    let mut engine = match stashed {
        Some(engine) => engine,
        None => crate::commands_memory::attach_shared_memory(
            QueryEngine::with_defaults_arc(
                LlmClient::new(state.client_config.read().await.clone()),
                state.tools.clone(),
                PermissionManager::new(),
                StateManager::new(),
            ),
            &state.memory_store,
        ),
    };
    engine.set_session_id(session_id);
    engine
        .restore_session(session_id)
        .map_err(|e| format!("failed to project session history: {e}"))?;
    Ok(engine)
}

/// `/context` — token estimate + context window for the session's history.
#[tauri::command]
pub async fn get_session_context_stats(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<SessionContextStats, String> {
    let uuid = uuid::Uuid::parse_str(&session_id)
        .map_err(|_| format!("invalid session id {session_id}"))?;
    let engine = restored_engine(&state, uuid).await?;
    Ok(SessionContextStats {
        estimated_tokens: engine.estimate_conversation_tokens(),
        context_window: engine.resolved_context_window_opt(),
    })
}

/// One file's `--numstat` row (binary files report `None` counts).
#[derive(Debug, Clone, Serialize)]
pub struct GitDiffFile {
    pub path: String,
    pub insertions: u64,
    pub deletions: u64,
}

/// Whole-working-tree diff for the /diff slash command.
#[derive(Debug, Clone, Serialize)]
pub struct GitDiffSummary {
    pub is_repo: bool,
    pub files: Vec<GitDiffFile>,
    /// Unified patch, capped at `MAX_PATCH_BYTES` (larger diffs are
    /// truncated; the UI says so instead of silently clipping).
    pub patch: String,
    pub truncated: bool,
}

/// Patch size cap — a chat-renderable diff, not a git bundle.
const MAX_PATCH_BYTES: usize = 200_000;

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `git diff HEAD --numstat` rows: `<ins>\t<del>\t<path>`. Binary
/// entries print `-` for the counts and are kept with zero counts so the
/// file still shows up in the summary; rename rows keep their `{} => {}`
/// path syntax verbatim.
fn parse_numstat(output: &str) -> Vec<GitDiffFile> {
    let count = |s: Option<&str>| s.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let insertions = count(parts.next());
            let deletions = count(parts.next());
            let path = parts.next()?.to_string();
            Some(GitDiffFile {
                path,
                insertions,
                deletions,
            })
        })
        .collect()
}

/// `/diff` — uncommitted changes (`git diff HEAD`) in the session's working
/// directory. Not a repository (or git unavailable) surfaces as
/// `is_repo: false` rather than an error so the UI can show a calm notice.
#[tauri::command]
pub async fn get_session_git_diff(working_dir: String) -> Result<GitDiffSummary, String> {
    let dir = Path::new(&working_dir).to_path_buf();
    if !dir.is_dir() {
        return Err(format!("working directory does not exist: {working_dir}"));
    }
    // Spawn the short-lived git processes off the async runtime's workers.
    let summary = tokio::task::spawn_blocking(move || {
        let inside = git_output(&dir, &["rev-parse", "--is-inside-work-tree"])
            .map(|s| s.trim() == "true")
            .unwrap_or(false);
        if !inside {
            return GitDiffSummary {
                is_repo: false,
                files: Vec::new(),
                patch: String::new(),
                truncated: false,
            };
        }
        let files = git_output(&dir, &["diff", "HEAD", "--numstat"])
            .map(|s| parse_numstat(&s))
            .unwrap_or_default();
        let (patch, truncated) = match git_output(&dir, &["diff", "HEAD"]) {
            Some(full) => {
                if full.len() > MAX_PATCH_BYTES {
                    let mut cut = MAX_PATCH_BYTES;
                    while !full.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    (full[..cut].to_string(), true)
                } else {
                    (full, false)
                }
            }
            None => (String::new(), false),
        };
        GitDiffSummary {
            is_repo: true,
            files,
            patch,
            truncated,
        }
    })
    .await
    .map_err(|e| format!("git diff task failed: {e}"))?;
    Ok(summary)
}

/// `/compact` — summarize the session's history and persist the compacted
/// conversation back to L0 ("compact as summary turn"). The summary call may
/// use the LLM (mirrors the REPL), so this is the one slash backend that can
/// take seconds; the composer disables itself for the duration via the
/// frontend.
#[derive(Debug, Clone, Serialize)]
pub struct CompactSessionSummary {
    pub performed: bool,
    /// True when the session had no compactable history.
    pub nothing_to_compact: bool,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub reduction_ratio: f32,
    pub messages_removed: usize,
    /// Turns the compacted L0 log now holds (summary turn + kept recents).
    pub kept_turns: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactSessionResult {
    #[serde(flatten)]
    pub summary: CompactSessionSummary,
    /// The session's message list as `load_session` would return it.
    pub messages: Vec<crate::commands::ChatMessage>,
}

/// Extract displayable text from an engine message (text blocks joined);
/// `None` for messages with no textual content (e.g. pure tool_use).
fn message_text(message: &shannon_engine::api::Message) -> Option<String> {
    match &message.content {
        shannon_engine::api::MessageContent::Text(t) => {
            if t.trim().is_empty() {
                None
            } else {
                Some(t.clone())
            }
        }
        shannon_engine::api::MessageContent::Blocks(blocks) => {
            let text = blocks
                .iter()
                .map(|b| match b {
                    shannon_engine::api::ContentBlock::Text { text } => text.as_str(),
                    _ => "",
                })
                .collect::<Vec<_>>()
                .join("");
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
}

/// Fold a compacted engine message list into `(user, assistant)` turns for
/// the L0 rewrite. Runs of non-user roles fill the open turn's assistant
/// slot; assistant-only leading messages open the first turn with an empty
/// prompt (the projector treats a bare user/message as a turn opener, so an
/// empty user text would inject a phantom prompt — pairs like that are
/// folded forward instead).
fn build_turns(messages: &[shannon_engine::api::Message]) -> Vec<(String, String)> {
    let mut turns: Vec<(String, String)> = Vec::new();
    for m in messages {
        let Some(text) = message_text(m) else {
            continue;
        };
        if m.role == "user" {
            turns.push((text, String::new()));
        } else if let Some(last) = turns.last_mut() {
            if last.1.is_empty() {
                last.1 = text;
            } else {
                last.1.push('\n');
                last.1.push_str(&text);
            }
        } else {
            // Leading assistant summary — represent it as the answer to an
            // implicit compact request.
            turns.push(("[context compacted]".to_string(), text));
        }
    }
    turns.retain(|(user, assistant)| !user.is_empty() || !assistant.is_empty());
    turns
}

#[tauri::command]
pub async fn compact_session(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
) -> Result<CompactSessionResult, String> {
    let uuid = uuid::Uuid::parse_str(&session_id)
        .map_err(|_| format!("invalid session id {session_id}"))?;

    // Mirror /rewind: refuse to compact while a query runs on the session.
    if let Some(session) = state
        .registry
        .get(crate::session_registry::SessionKey(uuid))
    {
        let querying = session.querying.lock().await;
        if *querying {
            return Err("A query is in progress — wait for it to finish before compacting".into());
        }
    }

    let engine = restored_engine(&state, uuid).await?;
    let history = engine.conversation_history();
    if history.is_empty() {
        return Ok(CompactSessionResult {
            summary: CompactSessionSummary {
                performed: false,
                nothing_to_compact: true,
                original_tokens: 0,
                compacted_tokens: 0,
                reduction_ratio: 0.0,
                messages_removed: 0,
                kept_turns: 0,
            },
            messages: Vec::new(),
        });
    }

    // LLM summarizer on the current runtime (mirrors the REPL), falling back
    // to the extractive summarizer when the client can't serve one.
    let client = engine.client().clone();
    let mut compact_engine = shannon_engine::compact::CompactEngine::with_llm_summarizer(client)
        .or_else(|_| shannon_engine::compact::CompactEngine::with_defaults())
        .map_err(|e| format!("compact engine error: {e}"))?;

    let mut messages = history.clone();
    let result = compact_engine
        .compact(&mut messages)
        .map_err(|e| format!("compaction failed: {e}"))?;

    let turns = build_turns(&messages);
    let kept_turns = turns.len();
    state
        .l0_store()
        .rewrite_with_conversation(&uuid, &turns)
        .map_err(|e| format!("failed to persist compacted history: {e}"))?;

    // Compaction invalidates rewind checkpoints (turn indices shifted).
    let manager = shannon_core::checkpoint::CheckpointManager::for_session(&session_id);
    manager.clear();
    let _ = manager.save_to_disk();

    // Replace the live session's in-memory buffer with the compacted turns.
    {
        let session = state
            .registry
            .get(crate::session_registry::SessionKey(uuid));
        if let Some(session) = session {
            let mut buf = session.messages.lock().await;
            buf.clear();
            for (user, assistant) in &turns {
                let now = chrono::Utc::now().timestamp_millis();
                if !user.is_empty() {
                    buf.push(crate::commands::ChatMessage {
                        role: "user".into(),
                        content: user.clone(),
                        timestamp: now,
                        file_attachments: None,
                    });
                }
                if !assistant.is_empty() {
                    buf.push(crate::commands::ChatMessage {
                        role: "assistant".into(),
                        content: assistant.clone(),
                        timestamp: now,
                        file_attachments: None,
                    });
                }
            }
        }
    }

    let messages = crate::commands_sessions::load_session(state, app_handle, session_id).await?;

    Ok(CompactSessionResult {
        summary: CompactSessionSummary {
            performed: true,
            nothing_to_compact: false,
            original_tokens: result.original_tokens,
            compacted_tokens: result.compacted_tokens,
            reduction_ratio: result.reduction_ratio,
            messages_removed: result.messages_removed,
            kept_turns,
        },
        messages,
    })
}

// ── /dream and /detect-skills backends ──────────────────────────────────

/// Parse the optional `/dream [days]` argument: `None` or blank → the manual
/// default of [`crate::commands_dream::DEFAULT_MANUAL_DAYS_BACK`] (3), an
/// integer → clamped to [`crate::commands_dream::DREAM_MAX_DAYS_BACK`] (30)
/// (`execute_dream_pass` re-clamps centrally), anything else → `Err` so the
/// UI can surface the typo instead of silently running the wrong window.
fn parse_days_arg(args: Option<&str>) -> Result<u32, String> {
    let Some(raw) = args.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(crate::commands_dream::DEFAULT_MANUAL_DAYS_BACK);
    };
    let days: u32 = raw
        .parse()
        .map_err(|_| format!("invalid days argument {raw:?} — usage: /dream [days]"))?;
    Ok(days.min(crate::commands_dream::DREAM_MAX_DAYS_BACK))
}

/// `/dream [days]` — run one dream pass through the same orchestration as
/// the Memory panel button and the nightly scheduler (privacy gates, 6h
/// throttle and single-flight lock included). Produces review-gated shadow
/// proposals + a report; nothing is applied without user approval.
#[tauri::command]
pub async fn dream_slash(
    // Injected by Tauri; `execute_dream_pass` resolves state from the app
    // handle so every entry point shares one body.
    _state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    args: Option<String>,
) -> Result<crate::commands_dream::DreamPassResult, String> {
    let days = parse_days_arg(args.as_deref())?;
    crate::commands_dream::execute_dream_pass(app, days).await
}

/// `/detect-skills` — run the heuristic skill-pattern detector on demand,
/// bypassing the dream pass's throttles by design (detection is purely
/// local: zero LLM cost, no proposals). New candidates land in the review
/// queue + inbox only; `skill_detection_enabled` (the privacy main switch)
/// still applies.
#[tauri::command]
pub async fn detect_slash(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    let dir = crate::skill_pattern_detection::default_sessions_dir()?;
    let inbox = state.inbox_store();
    let appended = crate::skill_pattern_detection::detect_and_record(
        &app,
        &inbox,
        &dir,
        crate::skill_pattern_detection::DEFAULT_DETECT_DAYS_BACK,
    )
    .await?;
    Ok(appended.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, text: &str) -> shannon_engine::api::Message {
        shannon_engine::api::Message {
            role: role.into(),
            content: shannon_engine::api::MessageContent::Text(text.into()),
        }
    }

    #[test]
    fn build_turns_pairs_user_and_assistant_messages() {
        let msgs = vec![
            msg("user", "first question"),
            msg("assistant", "first answer"),
            msg("assistant", "summary of older turns"),
            msg("user", "follow-up"),
            msg("assistant", "kept answer"),
        ];
        // A consecutive assistant run merges into the open turn's answer.
        let turns = build_turns(&msgs);
        assert_eq!(
            turns,
            vec![
                (
                    "first question".to_string(),
                    "first answer\nsummary of older turns".to_string()
                ),
                ("follow-up".to_string(), "kept answer".to_string()),
            ]
        );
    }

    #[test]
    fn build_turns_handles_leading_summary_and_skips_empty() {
        let msgs = vec![
            msg("assistant", "summary text"),
            msg("user", "   "),
            msg("assistant", "kept"),
        ];
        let turns = build_turns(&msgs);
        assert_eq!(
            turns,
            vec![(
                "[context compacted]".to_string(),
                "summary text\nkept".to_string()
            )]
        );
    }

    #[test]
    fn numstat_rows_parse_into_files() {
        let out = "12\t3\tsrc/lib.rs\n0\t0\timg.png\n-\t-\tbinary.bin\n";
        let files = parse_numstat(out);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!((files[0].insertions, files[0].deletions), (12, 3));
        assert_eq!((files[1].insertions, files[1].deletions), (0, 0));
        assert_eq!(files[2].path, "binary.bin");
        assert_eq!((files[2].insertions, files[2].deletions), (0, 0));
    }

    #[test]
    fn empty_numstat_yields_no_files() {
        assert!(parse_numstat("").is_empty());
    }

    #[test]
    fn diff_of_non_repo_reports_not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let summary = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_session_git_diff_inner(tmp.path()));
        assert!(!summary.is_repo);
        assert!(summary.files.is_empty());
        assert!(summary.patch.is_empty());
    }

    #[test]
    fn diff_of_git_repo_lists_changed_files() {
        if !Command::new("git")
            .args(["--version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            eprintln!("git unavailable — skipping");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let run = |args: &[&str]| {
            Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .unwrap()
        };
        // Seed the repo with a committed file, then modify it so `HEAD`
        // exists and the working tree actually differs from it.
        std::fs::write(dir.join("a.txt"), "base\n").unwrap();
        run(&["init", "-q"]);
        run(&["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
        run(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ]);
        std::fs::write(dir.join("a.txt"), "line1\nline2\n").unwrap();

        let summary = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(get_session_git_diff_inner(dir));
        assert!(summary.is_repo);
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].path, "a.txt");
        assert_eq!(summary.files[0].insertions, 2);
        assert!(summary.patch.contains("diff --git"));
        assert!(!summary.truncated);
    }

    // The command is `async` for the Tauri runtime; tests drive the inner
    // future directly with their own runtime.
    async fn get_session_git_diff_inner(dir: &Path) -> GitDiffSummary {
        let working_dir = dir.to_string_lossy().into_owned();
        super::get_session_git_diff(working_dir).await.unwrap()
    }

    #[test]
    fn dream_days_arg_defaults_when_missing_or_blank() {
        assert_eq!(parse_days_arg(None).unwrap(), 3);
        assert_eq!(parse_days_arg(Some("")).unwrap(), 3);
        assert_eq!(parse_days_arg(Some("   ")).unwrap(), 3);
    }

    #[test]
    fn dream_days_arg_parses_and_caps_integers() {
        assert_eq!(parse_days_arg(Some("7")).unwrap(), 7);
        assert_eq!(parse_days_arg(Some(" 14 ")).unwrap(), 14);
        assert_eq!(parse_days_arg(Some("0")).unwrap(), 0);
        assert_eq!(parse_days_arg(Some("30")).unwrap(), 30);
        assert_eq!(parse_days_arg(Some("999")).unwrap(), 30, "capped at 30");
    }

    #[test]
    fn dream_days_arg_rejects_garbage() {
        for bad in ["abc", "-1", "7.5", "3 days"] {
            assert!(
                parse_days_arg(Some(bad)).is_err(),
                "{bad:?} must be invalid"
            );
        }
    }
}
