//! P1-2 — Desktop best-of-N batch runs: Tauri commands + parallel worktree
//! orchestration.
//!
//! One batch = N (2..=4) unattended runs of the *same* prompt, each in its
//! own git worktree forked from the same base commit, executed in parallel.
//! When every branch reaches a terminal state the batch completes (with an
//! inbox item summarizing the N candidates); the user then diffs the
//! branches side by side, adopts one (merged back into the base, the rest
//! cleaned up) or discards the whole batch.
//!
//! Architecture mirrors the P0-3 [`crate::inbox_commands::spawn_routine_run`]
//! executor and the P0-2 goal runner, including their discipline:
//!
//! - a single [`finalize_branch`] choke point writes each branch's terminal
//!   state (summary computed from the worktree diff, DTO + `batch:updated`
//!   emit); every exit path — engine failure, panic, discard-mid-run — flows
//!   through it, so a branch can never stay `running` in a live process;
//! - the engine phase of each branch runs under a panic guard (nested
//!   `tokio::spawn` + `JoinHandle`); a panic maps to a failed branch;
//! - single-turn completion writes an inbox item (`source="batch"`), which
//!   adopt/discard later refresh **in place** (one item per batch, no noise).
//!
//! Persistence: each batch run is a JSON record under
//! `~/.shannon/batch-runs/<batchId>.json`. After an app restart
//! `list_batch_runs` reconciles records with no live runner: branches still
//! marked `running` become `failed` with the [`RESTART_INTERRUPTED_ERROR`]
//! error, their worktrees are preserved for manual inspection.
//!
//! Concurrency: a process-wide [`tokio::sync::Semaphore`] (4 permits) caps
//! concurrently *executing* batch branches; extra branches queue for a
//! permit. There is no existing desktop concurrency control to reuse (the
//! routine/goal executors are unbounded), so this is the simple-semaphore
//! option from the brief.
//!
//! Worktree/branch naming: `batch-<batchId8>-<n>` under
//! `<repo>/.shannon/scheduled-worktrees/`, forked at the repo HEAD captured
//! when the batch started (see [`shannon_core::scheduled_worktree::create_named`]).

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shannon_core::inbox_store::{InboxItemNew, SOURCE_BATCH};
use shannon_core::query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata};
use shannon_engine::api::client::LlmClient;
use shannon_engine::permissions::{ApprovalMode, PermissionManager, PermissionRuleChecker};
use shannon_engine::state::StateManager;
use tauri::Emitter;
use tokio::sync::RwLock;

use crate::commands::AppState;
use crate::config::DesktopConfig;
use crate::events::event_names;

/// Cap on concurrently *executing* batch branches across all batches
/// (brief: 全局同时 ≤4 个 batch 分支在跑，超过排队). The semaphore is held
/// from before the engine starts until the branch finalized.
pub(crate) const MAX_CONCURRENT_BATCH_BRANCHES: usize = 4;

/// Brief-frozen error text for branches that were still running when the
/// app restarted.
pub(crate) const RESTART_INTERRUPTED_ERROR: &str = "应用重启中断";

/// A chat-renderable diff, not a git bundle (mirrors `commands_slash`).
const MAX_PATCH_BYTES: usize = 200_000;

/// Error text cap (mirrors the goal runner's 500-char inbox discipline).
const ERROR_MAX_CHARS: usize = 500;

// ── Wire DTOs (frozen frontend contract — camelCase, do not reshape) ────

/// Per-branch diff stat block (frozen brief shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDiffSummary {
    pub files_changed: u64,
    pub additions: u64,
    pub deletions: u64,
}

/// One parallel candidate branch as rendered by the Tasks-page batch card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchBranchDto {
    pub index: u32,
    pub branch_name: String,
    pub worktree_path: String,
    /// `running | completed | failed`
    pub status: String,
    pub error: Option<String>,
    pub summary: Option<BranchDiffSummary>,
    pub spent_usd: f64,
}

/// One best-of-N batch run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRunDto {
    pub batch_id: String,
    pub title: String,
    pub prompt: String,
    pub count: u32,
    /// `running | completed | failed | partially_failed | adopted | discarded`
    pub status: String,
    pub created_at_ms: i64,
    pub branches: Vec<BatchBranchDto>,
    /// ADDITIVE contract surface (brief shape is the fields above): the
    /// index of the adopted branch once the batch is `adopted`, else `null`.
    pub adopted_index: Option<u32>,
}

/// `start_batch_run` response — `{ batchId }` (frozen).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRunStarted {
    pub batch_id: String,
}

/// `get_batch_branch_diff` response — `{ diff }` (frozen).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDiffDto {
    pub diff: String,
}

/// `adopt_batch_branch` response (frozen): `merged` + the conflicting file
/// list when the merge could not complete (worktrees stay untouched).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptBranchResult {
    pub merged: bool,
    pub conflicts: Option<Vec<String>>,
}

/// `discard_batch_run` response. `removed` is the frozen field;
/// `skipped` is ADDITIVE (brief: 跳过的分支要在返回中列明) — entries are
/// `"<branchName>: <reason>"`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscardResult {
    pub removed: u32,
    pub skipped: Vec<String>,
}

// ── Persisted record (disk shape — superset of the wire DTO) ────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchBranchRecord {
    pub index: u32,
    pub branch_name: String,
    pub worktree_path: String,
    pub status: String,
    pub error: Option<String>,
    pub summary: Option<BranchDiffSummary>,
    pub spent_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchRunRecord {
    pub batch_id: String,
    pub title: String,
    pub prompt: String,
    pub count: u32,
    pub status: String,
    pub created_at_ms: i64,
    pub branches: Vec<BatchBranchRecord>,
    pub adopted_index: Option<u32>,
    /// Branch that was checked out in the base repo when the batch started —
    /// adopt merges back into whatever is checked out there now.
    pub base_branch: String,
    pub base_commit: String,
    pub repo_root: String,
    /// Inbox item carrying the batch's outcome (created once at completion,
    /// refreshed in place on adopt/discard).
    pub inbox_item_id: Option<i64>,
}

impl BatchRunRecord {
    fn dto(&self) -> BatchRunDto {
        BatchRunDto {
            batch_id: self.batch_id.clone(),
            title: self.title.clone(),
            prompt: self.prompt.clone(),
            count: self.count,
            status: self.status.clone(),
            created_at_ms: self.created_at_ms,
            branches: self
                .branches
                .iter()
                .map(|b| BatchBranchDto {
                    index: b.index,
                    branch_name: b.branch_name.clone(),
                    worktree_path: b.worktree_path.clone(),
                    status: b.status.clone(),
                    error: b.error.clone(),
                    summary: b.summary,
                    spent_usd: b.spent_usd,
                })
                .collect(),
            adopted_index: self.adopted_index,
        }
    }

    fn branch(&self, index: u32) -> Option<&BatchBranchRecord> {
        self.branches.iter().find(|b| b.index == index)
    }
}

/// Aggregate the batch status from its branches (only meaningful once every
/// branch is terminal).
fn aggregate_status(branches: &[BatchBranchRecord]) -> &'static str {
    let failed = branches.iter().filter(|b| b.status == "failed").count();
    if failed == 0 {
        "completed"
    } else if failed == branches.len() {
        "failed"
    } else {
        "partially_failed"
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

// ── Registry + handle ────────────────────────────────────────────────────

/// Mutable batch state shared between the Tauri commands and the spawned
/// branch tasks. The JSON record under [`Self::store_dir`] is the durable
/// mirror; every mutation re-persists it.
pub(crate) struct BatchRunHandle {
    pub(crate) record: tokio::sync::Mutex<BatchRunRecord>,
    store_dir: PathBuf,
}

impl BatchRunHandle {
    async fn dto(&self) -> BatchRunDto {
        self.record.lock().await.dto()
    }

    /// Persist the current record to `store_dir/<batchId>.json`. Best-effort:
    /// a failed write is logged and never breaks the run (the in-memory
    /// record stays authoritative for the live process).
    fn persist(&self, record: &BatchRunRecord) {
        if let Err(e) = std::fs::create_dir_all(&self.store_dir) {
            tracing::warn!(error = %e, "batch: failed to create store dir");
            return;
        }
        let path = self.store_dir.join(format!("{}.json", record.batch_id));
        let body = match serde_json::to_string_pretty(record) {
            Ok(body) => body,
            Err(e) => {
                tracing::warn!(error = %e, "batch: failed to serialize record");
                return;
            }
        };
        if let Err(e) = std::fs::write(&path, body) {
            tracing::warn!(error = %e, path = %path.display(), "batch: failed to persist record");
        }
    }

    async fn persist_current(&self) {
        let record = self.record.lock().await;
        self.persist(&record);
    }
}

/// Process-wide batch registry: live handles + the semaphore that caps
/// concurrently executing branches + the on-disk record store.
pub(crate) struct BatchRunRegistry {
    runs: Mutex<HashMap<String, Arc<BatchRunHandle>>>,
    store_dir: PathBuf,
    branch_permits: tokio::sync::Semaphore,
}

impl BatchRunRegistry {
    /// Default registry: records under `~/.shannon/batch-runs/`.
    pub fn new() -> Self {
        Self::with_dir(default_batch_runs_dir())
    }

    /// Registry over a custom store dir (tests).
    pub(crate) fn with_dir(store_dir: PathBuf) -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            store_dir,
            branch_permits: tokio::sync::Semaphore::new(MAX_CONCURRENT_BATCH_BRANCHES),
        }
    }

    fn get_sync(&self, batch_id: &str) -> Option<Arc<BatchRunHandle>> {
        self.runs
            .lock()
            .expect("batch registry poisoned")
            .get(batch_id)
            .cloned()
    }

    async fn register(&self, record: BatchRunRecord) -> Arc<BatchRunHandle> {
        let handle = Arc::new(BatchRunHandle {
            record: tokio::sync::Mutex::new(record),
            store_dir: self.store_dir.clone(),
        });
        // Read the id before taking the (std) registry lock — the guard must
        // never be held across an await (the command futures must stay Send).
        let batch_id = handle.record.lock().await.batch_id.clone();
        self.runs
            .lock()
            .expect("batch registry poisoned")
            .insert(batch_id, handle.clone());
        handle
    }

    /// Look up a live run, falling back to the disk record store (so adopt /
    /// discard / diff keep working after a restart without listing first).
    /// Disk records go through restart reconciliation before registering —
    /// a record with no live runner can never still be `running` — so a
    /// `get_or_load` before a `list` cannot resurrect stale running state
    /// (and `list` would otherwise skip it via the seen-set).
    pub(crate) async fn get_or_load(&self, batch_id: &str) -> Result<Arc<BatchRunHandle>, String> {
        if let Some(handle) = self.get_sync(batch_id) {
            return Ok(handle);
        }
        let record = load_record(&self.store_dir, batch_id)
            .map_err(|e| format!("failed to load batch record {batch_id}: {e}"))?
            .ok_or_else(|| format!("batch not found: {batch_id}"))?;
        Ok(self.register(reconcile_restart(record)).await)
    }

    /// All runs (live + disk), newest first. Disk records with no live
    /// runner go through restart reconciliation first: branches still
    /// `running` become `failed` ([`RESTART_INTERRUPTED_ERROR`]); their
    /// worktrees are preserved for manual inspection.
    pub(crate) async fn list(&self) -> Vec<BatchRunDto> {
        let mut dtos: Vec<BatchRunDto> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Snapshot the live handles first — the std-MutexGuard temporary of a
        // `for x in self.runs.lock()...` iterator would be held across the
        // awaits in the loop body.
        let live: Vec<Arc<BatchRunHandle>> = self
            .runs
            .lock()
            .expect("batch registry poisoned")
            .values()
            .cloned()
            .collect();
        for handle in live {
            seen.insert(handle.record.lock().await.batch_id.clone());
            dtos.push(handle.dto().await);
        }
        for record in load_records(&self.store_dir) {
            if seen.contains(&record.batch_id) {
                continue;
            }
            let record = reconcile_restart(record);
            let handle = self.register(record).await;
            handle.persist_current().await;
            dtos.push(handle.dto().await);
        }
        dtos.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
        dtos
    }
}

impl Default for BatchRunRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Default on-disk record store: `~/.shannon/batch-runs/`.
pub fn default_batch_runs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("batch-runs")
}

fn load_record(store_dir: &Path, batch_id: &str) -> Result<Option<BatchRunRecord>, String> {
    // Defense against path tricks: batch ids are UUIDs.
    if batch_id.is_empty()
        || batch_id.contains('/')
        || batch_id.contains('\\')
        || batch_id.contains("..")
    {
        return Err("invalid batch id".into());
    }
    let path = store_dir.join(format!("{batch_id}.json"));
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    serde_json::from_str(&body)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn load_records(store_dir: &Path) -> Vec<BatchRunRecord> {
    let Ok(entries) = std::fs::read_dir(store_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        match serde_json::from_str::<BatchRunRecord>(&body) {
            Ok(record) => out.push(record),
            // A torn write from a crash is skipped, not fatal.
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "batch: skipping unreadable record")
            }
        }
    }
    out
}

/// Mark branches that were still `running` as failed (app restart
/// interrupted) and recompute the batch status. Worktrees are deliberately
/// left alone. Returns the (possibly unchanged) record.
fn reconcile_restart(mut record: BatchRunRecord) -> BatchRunRecord {
    if record.status != "running" {
        return record;
    }
    for branch in &mut record.branches {
        if branch.status == "running" {
            branch.status = "failed".into();
            branch.error = Some(RESTART_INTERRUPTED_ERROR.into());
        }
    }
    record.status = aggregate_status(&record.branches).to_string();
    record
}

// ── State slices + runner seam (mirrors GoalRunDeps / GoalTurnRunner) ───

#[derive(Clone)]
pub(crate) struct BatchRunDeps {
    pub(crate) inbox: Arc<shannon_core::inbox_store::InboxStore>,
    pub(crate) usage_store: Arc<crate::commands_usage::UsageStore>,
    pub(crate) client_config: Arc<RwLock<shannon_engine::api::types::LlmClientConfig>>,
    pub(crate) desktop_config: Arc<RwLock<DesktopConfig>>,
    pub(crate) tools: Arc<shannon_core::tools::ToolRegistry>,
}

impl BatchRunDeps {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            inbox: state.inbox_store(),
            usage_store: state.usage_store.clone(),
            client_config: state.client_config.clone(),
            desktop_config: state.desktop_config.clone(),
            tools: state.tools.clone(),
        }
    }
}

/// Inputs one branch execution needs (resolved from the batch record). The
/// prompt already carries the worktree directive, so the runner only needs
/// the branch index to fold live spend back onto the right row.
#[derive(Debug, Clone)]
pub(crate) struct BranchSpawn {
    pub(crate) index: u32,
    pub(crate) prompt: String,
}

/// What one branch run produced. Spend is deliberately NOT carried here:
/// the engine runner folds each `Usage` event into the branch's live
/// `spent_usd` as it streams (the batch card shows spend while the branch
/// still runs); a stub factory just reports the terminal outcome.
pub(crate) struct BranchObservation {
    pub(crate) completed: bool,
    pub(crate) error: Option<String>,
}

/// Single-branch execution, abstracted so the orchestration is testable
/// without an LLM: production wires [`EngineBatchBranchRunner`]; tests
/// inject stubs that script outcomes (and can touch the worktree to
/// simulate agent edits).
pub(crate) trait BatchBranchRunner: Send {
    fn run(
        &mut self,
        spawn: BranchSpawn,
    ) -> Pin<Box<dyn Future<Output = BranchObservation> + Send + '_>>;
}

/// Per-batch factory: the spawned task for branch `index` gets its runner
/// from here. Production wires [`EngineRunnerFactory`]; tests inject stub
/// factories (T4 GoalTurnRunner injection pattern).
pub(crate) trait BranchRunnerFactory: Send + Sync + 'static {
    fn build(&self, handle: &Arc<BatchRunHandle>, index: u32) -> Box<dyn BatchBranchRunner>;
}

/// The prompt each branch actually receives: the user prompt prefixed with
/// an unambiguous working-directory directive.
///
/// Why a prompt directive instead of `std::env::set_current_dir`: the
/// process CWD (and the engine's ambient reads) are process-global — with
/// up to 4 branches running concurrently inside one process, per-branch
/// chdir is a data race. This is the same constraint the CLI `/batch`
/// solves by handing each spawned agent a `working_directory` context; here
/// the directive tells the model to confine every operation to its
/// worktree (tools also accept absolute paths).
fn branch_prompt(prompt: &str, worktree_path: &str, branch_name: &str) -> String {
    format!(
        "[Working directory: {worktree_path}]\n\
         You are one of several parallel candidate attempts running in isolated git worktrees. \
         Work EXCLUSIVELY inside the working directory above: treat it as the repository root, \
         pass it (or absolute paths under it) to every file/shell operation, and never modify \
         anything outside it. When your changes are done, leave them in the worktree (commit \
         them on branch `{branch_name}` if you can).\n\n\
         Task:\n{prompt}"
    )
}

async fn branch_spawn(handle: &BatchRunHandle, index: u32) -> BranchSpawn {
    let record = handle.record.lock().await;
    BranchSpawn {
        index,
        prompt: branch_prompt(
            &record.prompt,
            record
                .branch(index)
                .map_or("", |b| b.worktree_path.as_str()),
            record.branch(index).map_or("", |b| b.branch_name.as_str()),
        ),
    }
}

// ── Production runner (mirrors spawn_routine_run) ───────────────────────

/// Production branch runner: fresh `QueryEngine` per branch (configured
/// approval mode + persisted rules, unattended → prompts auto-allowed, no
/// interactive permission channel), usage written to the shared ledger with
/// the branch's own session id, spend folded into the live batch record.
/// Query streaming events are deliberately **not** emitted — branch runs are
/// unattended and the main window renders every `query:*` event it receives,
/// so emitting them would leak branch output into the user's chat.
struct EngineBatchBranchRunner<R: tauri::Runtime> {
    deps: BatchRunDeps,
    app: tauri::AppHandle<R>,
    handle: Arc<BatchRunHandle>,
}

impl<R: tauri::Runtime> EngineBatchBranchRunner<R> {
    async fn stream_branch(&mut self, spawn: BranchSpawn) -> BranchObservation {
        let client_config = self.deps.client_config.read().await.clone();
        let approval_mode_str = self.deps.desktop_config.read().await.approval_mode.clone();
        let model = client_config.model.clone();
        let model_for_usage = model.clone();
        let provider = client_config.provider.to_string();
        let usage_store = self.deps.usage_store.clone();

        let mut permissions = PermissionManager::new();
        let mode = approval_mode_str
            .as_deref()
            .and_then(|s| match s {
                "full_auto" => Some(ApprovalMode::FullAuto),
                "auto_edit" => Some(ApprovalMode::AutoEdit),
                "auto" => Some(ApprovalMode::Auto),
                "plan" => Some(ApprovalMode::Plan),
                _ => None,
            })
            .unwrap_or(ApprovalMode::FullAuto);
        permissions.set_approval_mode(mode);
        let mut settings = shannon_core::settings::SettingsManager::new();
        if settings.load_from_files().is_ok() {
            let rules = &settings.settings_mut().permissions;
            permissions.set_rule_checker(PermissionRuleChecker::from_rule_strings(
                &rules.deny,
                &rules.ask,
                &rules.allow,
            ));
        }

        let engine = QueryEngine::with_defaults_arc(
            LlmClient::new(client_config),
            self.deps.tools.clone(),
            permissions,
            StateManager::new(),
        );

        let session_id = uuid::Uuid::new_v4();
        let context = QueryContext {
            query_id: uuid::Uuid::new_v4(),
            session_id,
            user_message: spawn.prompt.clone(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: None,
                model,
                temperature: None,
                top_p: None,
            },
        };

        let mut failure: Option<String> = None;

        let stream = engine.process_query(context, None).await;
        use futures::StreamExt;
        let mut pin_stream = std::pin::pin!(stream);
        while let Some(event_result) = pin_stream.next().await {
            match event_result {
                Ok(event) => match event {
                    QueryEvent::Text { .. } => {}
                    QueryEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cost_usd: event_cost,
                        cache_creation_tokens,
                        cache_read_tokens,
                        ..
                    } => {
                        // Best-effort ledger write, mirroring send_message.
                        let _ = usage_store.append(&crate::commands_usage::record_event(
                            &model_for_usage,
                            &provider,
                            crate::commands_usage::UsageTotals {
                                input_tokens,
                                output_tokens,
                                cache_creation_tokens,
                                cache_read_tokens,
                                cost_usd: event_cost,
                            },
                            Some(&session_id.to_string()),
                        ));
                        // Live spend on the batch card.
                        let dto = fold_branch_spend(&self.handle, spawn.index, event_cost).await;
                        let _ = self.app.emit(event_names::BATCH_UPDATED, dto);
                    }
                    QueryEvent::Completed { .. } => break,
                    QueryEvent::Failed { error, .. } => {
                        failure = Some(error);
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    failure = Some(e.to_string());
                    break;
                }
            }
        }

        BranchObservation {
            completed: failure.is_none(),
            error: failure,
        }
    }
}

impl<R: tauri::Runtime> BatchBranchRunner for EngineBatchBranchRunner<R> {
    fn run(
        &mut self,
        spawn: BranchSpawn,
    ) -> Pin<Box<dyn Future<Output = BranchObservation> + Send + '_>> {
        Box::pin(self.stream_branch(spawn))
    }
}

/// Fold one `Usage` event's cost into the branch's live `spent_usd` and
/// return the refreshed DTO for the `batch:updated` emit. Extracted from the
/// engine runner so the 累计 rule is unit-testable without an LLM.
async fn fold_branch_spend(handle: &BatchRunHandle, index: u32, cost_usd: f64) -> BatchRunDto {
    let mut record = handle.record.lock().await;
    if let Some(branch) = record.branches.iter_mut().find(|b| b.index == index) {
        branch.spent_usd += cost_usd;
    }
    record.dto()
}

/// Production factory.
struct EngineRunnerFactory<R: tauri::Runtime> {
    deps: BatchRunDeps,
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> BranchRunnerFactory for EngineRunnerFactory<R> {
    fn build(&self, handle: &Arc<BatchRunHandle>, _index: u32) -> Box<dyn BatchBranchRunner> {
        Box::new(EngineBatchBranchRunner {
            deps: self.deps.clone(),
            app: self.app.clone(),
            handle: handle.clone(),
        })
    }
}

// ── Git helpers (git-CLI only, off the async runtime) ───────────────────

fn git_out(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git {:?} failed: {e}", args.first().unwrap_or(&"")))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `git diff --numstat` rows and total them (binary rows report `-`
/// and count as one changed file with zero line deltas).
fn summarize_numstat(output: &str) -> BranchDiffSummary {
    let mut summary = BranchDiffSummary {
        files_changed: 0,
        additions: 0,
        deletions: 0,
    };
    for line in output.lines().filter(|l| !l.trim().is_empty()) {
        summary.files_changed += 1;
        let mut parts = line.splitn(3, '\t');
        summary.additions += parts
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        summary.deletions += parts
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
    }
    summary
}

/// Stage everything (so untracked agent output shows up in the diff) and
/// compute the branch's changes relative to the batch's base commit.
fn branch_summary_sync(worktree: &str, base_commit: &str) -> Result<BranchDiffSummary, String> {
    let wt = Path::new(worktree);
    if !wt.is_dir() {
        return Err(format!("worktree does not exist: {worktree}"));
    }
    // Best-effort: a failed add (e.g. .gitignore conflicts) must not hide
    // the diff we already have.
    let _ = git_out(wt, &["add", "-A"]);
    let numstat = git_out(wt, &["diff", "--numstat", base_commit])?;
    Ok(summarize_numstat(&numstat))
}

fn branch_patch_sync(worktree: &str, base_commit: &str) -> Result<String, String> {
    let wt = Path::new(worktree);
    if !wt.is_dir() {
        return Err(format!("worktree does not exist: {worktree}"));
    }
    let _ = git_out(wt, &["add", "-A"]);
    let patch = git_out(wt, &["diff", base_commit])?;
    if patch.len() > MAX_PATCH_BYTES {
        let mut cut = MAX_PATCH_BYTES;
        while !patch.is_char_boundary(cut) {
            cut -= 1;
        }
        return Ok(format!("{}\n… truncated", &patch[..cut]));
    }
    Ok(patch)
}

/// True when the branch worktree still has uncommitted changes.
fn worktree_dirty(worktree: &str) -> bool {
    if !Path::new(worktree).is_dir() {
        return false;
    }
    git_out(Path::new(worktree), &["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Number of commits on `branch` that the `base_branch` does not have.
fn unmerged_commit_count(repo_root: &str, base_branch: &str, branch: &str) -> u64 {
    git_out(
        Path::new(repo_root),
        &["rev-list", "--count", &format!("{base_branch}..{branch}")],
    )
    .ok()
    .and_then(|s| s.trim().parse::<u64>().ok())
    .unwrap_or(0)
}

/// Commit whatever the agent left uncommitted in the branch worktree onto
/// the branch, so `git merge` has commits to merge. Only runs during adopt.
fn auto_commit_worktree(worktree: &str, branch_name: &str) -> Result<(), String> {
    let wt = Path::new(worktree);
    if !wt.is_dir() {
        return Err(format!("worktree does not exist: {worktree}"));
    }
    let status = git_out(wt, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(()); // everything already committed
    }
    git_out(wt, &["add", "-A"])?;
    git_out(
        wt,
        &[
            "-c",
            "user.email=shannon-batch@localhost",
            "-c",
            "user.name=Shannon Batch",
            "commit",
            "-m",
            &format!("batch: capture branch {branch_name} state for adoption"),
        ],
    )?;
    Ok(())
}

enum MergeOutcome {
    Merged,
    /// The merge conflicted; `git merge --abort` was run so the base repo is
    /// left clean, and the conflicting files are listed for the user.
    Conflicts(Vec<String>),
}

/// Merge `branch` into whatever the base repo has checked out. Never force-
/// pushes, never deletes anything. On conflict the repo is restored with
/// `git merge --abort` (the batch branch + its worktree stay untouched for
/// manual handling) and the conflicting file list is returned.
fn merge_branch_sync(repo_root: &str, branch: &str) -> Result<MergeOutcome, String> {
    let repo = Path::new(repo_root);
    let merge = git_out(
        repo,
        &[
            "merge",
            "--no-ff",
            branch,
            "-m",
            &format!("Adopt batch branch {branch}"),
        ],
    );
    if merge.is_ok() {
        return Ok(MergeOutcome::Merged);
    }
    let conflicts = git_out(repo, &["diff", "--name-only", "--diff-filter=U"])
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Restore a clean base repo — the worktree/branch stay for manual work.
    let _ = git_out(repo, &["merge", "--abort"]);
    if conflicts.is_empty() {
        // The merge refused to start (dirty base tree, unrelated histories…)
        // — surface the git error instead of pretending it was a conflict.
        return Err(merge.err().unwrap_or_else(|| "git merge failed".into()));
    }
    Ok(MergeOutcome::Conflicts(conflicts))
}

fn delete_branch_sync(repo_root: &str, branch: &str) -> Result<(), String> {
    // Defence in depth: only ever delete batch-owned branches.
    if !branch.starts_with("batch-") {
        return Err(format!("refusing to delete non-batch branch {branch}"));
    }
    git_out(Path::new(repo_root), &["branch", "-D", branch]).map(|_| ())
}

// ── Cleanup rules ────────────────────────────────────────────────────────
//
// Adopt semantics = "the unselected candidates are abandoned", so a
// `completed` branch is removed (worktree + branch) even when its changes
// were never merged. A `failed` branch, however, is only removed when it
// holds no unmerged work (no commits ahead of the base branch and a clean
// worktree) — otherwise it is skipped and preserved for manual inspection.
// Running branches are always skipped.

enum DiscardDecision {
    Remove,
    Skip(&'static str),
}

fn discard_decision(record: &BatchRunRecord, branch: &BatchBranchRecord) -> DiscardDecision {
    if Some(branch.index) == record.adopted_index {
        return DiscardDecision::Skip("adopted branch is kept");
    }
    match branch.status.as_str() {
        "running" => DiscardDecision::Skip("still running"),
        "completed" => DiscardDecision::Remove,
        "failed" => {
            if worktree_dirty(&branch.worktree_path)
                || unmerged_commit_count(
                    &record.repo_root,
                    &record.base_branch,
                    &branch.branch_name,
                ) > 0
            {
                DiscardDecision::Skip(
                    "has unmerged changes — worktree preserved for manual inspection",
                )
            } else {
                DiscardDecision::Remove
            }
        }
        _ => DiscardDecision::Skip("unknown status"),
    }
}

// ── Orchestration ────────────────────────────────────────────────────────

/// Spawn one task per branch: acquire a semaphore permit, run the branch
/// under the panic guard, finalize through the choke point.
async fn spawn_branch_tasks<R: tauri::Runtime, F: BranchRunnerFactory>(
    registry: Arc<BatchRunRegistry>,
    deps: BatchRunDeps,
    app: tauri::AppHandle<R>,
    handle: Arc<BatchRunHandle>,
    factory: Arc<F>,
) {
    let count = handle.record.lock().await.branches.len();
    for index in 0..count {
        let registry = registry.clone();
        let deps = deps.clone();
        let app = app.clone();
        let handle = handle.clone();
        let factory = factory.clone();
        tokio::spawn(async move {
            // Queue for a permit: at most MAX_CONCURRENT_BATCH_BRANCHES
            // branches execute at once (brief: 超过排队).
            let permit = match registry.branch_permits.acquire().await {
                Ok(permit) => permit,
                Err(e) => {
                    // The registry never closes the semaphore; this arm is
                    // defence against future refactors, not a live path —
                    // but it must still finalize, never wedge `running`.
                    let observation = BranchObservation {
                        completed: false,
                        error: Some(format!("batch branch queue unavailable: {e}")),
                    };
                    finalize_branch(&deps, &app, &handle, index as u32, observation).await;
                    return;
                }
            };
            let guarded = {
                let factory = factory.clone();
                let handle = handle.clone();
                async move {
                    let mut runner = factory.build(&handle, index as u32);
                    let spawn = branch_spawn(&handle, index as u32).await;
                    let observation = runner.run(spawn).await;
                    drop(runner);
                    observation
                }
            };
            // Panic guard: a branch task panicking must not leave the branch
            // `running` forever (T2/T4 discipline).
            let observation = match tokio::spawn(guarded).await {
                Ok(observation) => observation,
                Err(join_error) => BranchObservation {
                    completed: false,
                    error: Some(format!("batch branch task panicked: {join_error}")),
                },
            };
            // The engine phase is over — release the slot before the
            // (fast) finalize so a queued branch can start.
            drop(permit);
            finalize_branch(&deps, &app, &handle, index as u32, observation).await;
        });
    }
}

/// Single choke point for branch terminal state: summary from the worktree
/// diff, terminal status, batch aggregation + completion inbox item,
/// persistence, `batch:updated` emit. Every branch exit path reaches this.
async fn finalize_branch<R: tauri::Runtime>(
    deps: &BatchRunDeps,
    app: &tauri::AppHandle<R>,
    handle: &Arc<BatchRunHandle>,
    index: u32,
    observation: BranchObservation,
) {
    // Phase 1: read the diff inputs (fast, lock held only for the read).
    let (worktree, base_commit) = {
        let record = handle.record.lock().await;
        match record.branch(index) {
            Some(branch) => (
                Some(branch.worktree_path.clone()),
                record.base_commit.clone(),
            ),
            None => (None, String::new()),
        }
    };
    // Phase 2: compute the worktree diff off the runtime AND off the record
    // lock — sibling branches keep streaming spend updates while git runs.
    let summary = match worktree {
        Some(worktree) => {
            tokio::task::spawn_blocking(move || branch_summary_sync(&worktree, &base_commit))
                .await
                .ok()
                .and_then(|r| r.ok())
        }
        None => None,
    };
    // Phase 3: single locked write of the terminal state.
    let (dto, wrote_inbox) = {
        let mut record = handle.record.lock().await;
        let branch = record.branches.iter_mut().find(|b| b.index == index);
        if let Some(branch) = branch {
            branch.status = if observation.completed {
                "completed".into()
            } else {
                "failed".into()
            };
            branch.error = observation
                .error
                .as_deref()
                .map(|e| truncate_chars(e, ERROR_MAX_CHARS));
            branch.summary = summary;
        }

        let was_running = record.status == "running";
        let all_terminal = record.branches.iter().all(|b| b.status != "running");
        let became_terminal = was_running && all_terminal;
        if became_terminal {
            record.status = aggregate_status(&record.branches).to_string();
        }
        let should_write_inbox =
            became_terminal && record.status != "discarded" && record.inbox_item_id.is_none();
        if should_write_inbox {
            let summary_text = completion_summary(&record);
            match deps.inbox.append_item(InboxItemNew {
                source: SOURCE_BATCH.into(),
                source_id: Some(record.batch_id.clone()),
                session_id: None,
                title: record.title.clone(),
                summary: truncate_chars(&summary_text, 500),
                error: None,
            }) {
                Ok(item) => record.inbox_item_id = Some(item.id),
                Err(e) => {
                    tracing::warn!(batch = %record.batch_id, error = %e, "batch: failed to append inbox item")
                }
            }
        }
        handle.persist(&record);
        (record.dto(), should_write_inbox)
    };

    let _ = app.emit(event_names::BATCH_UPDATED, dto);
    if wrote_inbox {
        let _ = app.emit(
            event_names::INBOX_UPDATED,
            handle.record.lock().await.batch_id.clone(),
        );
    }
}

/// Inbox summary: one line per branch with its outcome + changed-file count
/// (brief: N 份结果概览 + 每份 filesChanged).
fn completion_summary(record: &BatchRunRecord) -> String {
    let mut lines = vec![format!("{} branch(es) finished", record.branches.len())];
    for branch in &record.branches {
        let files = branch.summary.map(|s| s.files_changed).unwrap_or(0);
        let mut line = format!(
            "#{} {} — {} file(s) changed",
            branch.index, branch.status, files
        );
        if let Some(error) = branch.error.as_deref() {
            line.push_str(" · ");
            line.push_str(error);
        }
        lines.push(line);
    }
    lines.join("\n")
}

// ── Start ────────────────────────────────────────────────────────────────

/// `start_batch_run` (frozen contract).
#[tauri::command]
pub async fn start_batch_run(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    title: String,
    prompt: String,
    count: u32,
    base_session_id: Option<String>,
) -> Result<BatchRunStarted, String> {
    let deps = BatchRunDeps::from_state(&state);
    let repo_hint = resolve_session_working_dir(&state, base_session_id.as_deref()).await?;
    let factory = Arc::new(EngineRunnerFactory {
        deps: deps.clone(),
        app: app_handle.clone(),
    });
    start_batch_run_inner(
        &deps,
        &state.batch_runs.clone(),
        &app_handle,
        BatchStartRequest::new(title, prompt, count, repo_hint),
        factory,
    )
    .await
}

/// When `baseSessionId` is given, the batch runs against that session's
/// working directory (the project) instead of the process CWD.
async fn resolve_session_working_dir(
    state: &AppState,
    base_session_id: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(raw) = base_session_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let sessions = state.sessions.lock().await;
    let dir = sessions
        .iter()
        .find(|s| s.id == raw)
        .and_then(|s| s.working_dir.clone())
        .ok_or_else(|| format!("baseSessionId {raw} has no session or working directory"))?;
    Ok(Some(PathBuf::from(dir)))
}

/// One branch worktree creation, abstracted so the start-failure ROLLBACK
/// path is testable: production wires
/// [`shannon_core::scheduled_worktree::create_named`]; tests inject a stub
/// that delegates then fails at a chosen call (review fix: the rollback used
/// to be untestable because batch branch names embed the internal batch id).
pub(crate) type WorktreeCreator = Arc<
    dyn Fn(
            &Path,
            &Path,
            &str,
            &str,
            &str,
        ) -> Result<PathBuf, shannon_core::scheduled_worktree::WorktreeError>
        + Send
        + Sync,
>;

/// Production worktree creator.
pub(crate) fn production_worktree_creator() -> WorktreeCreator {
    Arc::new(shannon_core::scheduled_worktree::create_named)
}

/// Everything [`start_batch_run_inner`] needs beyond the shared state
/// slices (bundled to keep the orchestration signature readable).
pub(crate) struct BatchStartRequest {
    pub(crate) title: String,
    pub(crate) prompt: String,
    pub(crate) count: u32,
    /// Working directory to run against — resolved from `baseSessionId`
    /// (the session's project) by the command, `None` = process CWD.
    pub(crate) repo_hint: Option<PathBuf>,
    /// Per-branch worktree creation seam (production: `create_named`).
    pub(crate) worktree_creator: WorktreeCreator,
}

impl BatchStartRequest {
    /// Production request: real worktree creation.
    pub(crate) fn new(
        title: String,
        prompt: String,
        count: u32,
        repo_hint: Option<PathBuf>,
    ) -> Self {
        Self {
            title,
            prompt,
            count,
            repo_hint,
            worktree_creator: production_worktree_creator(),
        }
    }
}

/// Batch orchestration entry point. Generic over the runner factory so
/// tests drive the whole lifecycle with scripted branches.
pub(crate) async fn start_batch_run_inner<R: tauri::Runtime, F: BranchRunnerFactory>(
    deps: &BatchRunDeps,
    registry: &Arc<BatchRunRegistry>,
    app: &tauri::AppHandle<R>,
    request: BatchStartRequest,
    factory: Arc<F>,
) -> Result<BatchRunStarted, String> {
    let BatchStartRequest {
        title,
        prompt,
        count,
        repo_hint,
        worktree_creator,
    } = request;
    if !(2..=4).contains(&count) {
        return Err(format!("count must be between 2 and 4, got {count}"));
    }
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("batch prompt must not be empty".into());
    }
    let title = {
        let t = title.trim();
        if t.is_empty() {
            prompt.chars().take(50).collect::<String>()
        } else {
            t.to_string()
        }
    };

    // Resolve the base repo: the hinted working directory (baseSessionId)
    // or the process CWD. Capture branch + commit so every worktree forks
    // from the SAME base.
    let repo_root = {
        let hint = repo_hint.clone();
        tokio::task::spawn_blocking(move || -> Result<(String, String, String), String> {
            let dir = hint.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            if !dir.is_dir() {
                return Err(format!(
                    "working directory does not exist: {}",
                    dir.display()
                ));
            }
            // A failing rev-parse IS the not-a-repo case — answer with the
            // friendly error, not git's (possibly localized) stderr.
            let inside = git_out(&dir, &["rev-parse", "--is-inside-work-tree"])
                .map(|out| out.trim().eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if !inside {
                return Err(format!(
                    "not a git repository: {} — batch runs need a git project",
                    dir.display()
                ));
            }
            let root = git_out(&dir, &["rev-parse", "--show-toplevel"])?
                .trim()
                .to_string();
            let branch = git_out(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])?
                .trim()
                .to_string();
            let commit = git_out(&dir, &["rev-parse", "HEAD"])?.trim().to_string();
            Ok((root, branch, commit))
        })
        .await
        .map_err(|e| format!("git probe task failed: {e}"))??
    };
    let (repo_root, base_branch, base_commit) = repo_root;

    let batch_id = uuid::Uuid::new_v4().to_string();
    let id8: String = batch_id.chars().take(8).collect();
    let base_dir = Path::new(&repo_root).join(shannon_core::scheduled_worktree::DEFAULT_BASE_DIR);

    // Create the N worktrees up front; on failure roll back the ones that
    // were already created so a failed start leaves nothing behind.
    let creation = tokio::task::spawn_blocking({
        let repo_root = repo_root.clone();
        let base_dir = base_dir.clone();
        let base_commit = base_commit.clone();
        move || -> Result<Vec<(u32, String, String)>, String> {
            let mut out: Vec<(u32, String, String)> = Vec::new();
            for n in 0..count {
                let name = format!("batch-{id8}-{n}");
                let path = (worktree_creator)(
                    Path::new(&repo_root),
                    &base_dir,
                    &name,
                    &name,
                    &base_commit,
                )
                .map_err(|e| {
                    // Roll back earlier worktrees AND their branches before
                    // surfacing. `out` rows are (index, BRANCH NAME, path) —
                    // the branch name is the second element (review fix:
                    // passing the path here tripped the batch- prefix guard
                    // and branch deletion silently no-op'd).
                    for (_, prev_branch, prev_path) in &out {
                        let _ = shannon_core::scheduled_worktree::remove_in(
                            Path::new(&repo_root),
                            Path::new(prev_path),
                        );
                        let _ = delete_branch_sync(&repo_root, prev_branch);
                    }
                    format!("failed to create worktree for branch {n}: {e}")
                })?;
                out.push((n, name, path.to_string_lossy().into_owned()));
            }
            Ok(out)
        }
    })
    .await
    .map_err(|e| format!("worktree creation task failed: {e}"))?;
    let worktrees = creation?;

    let record = BatchRunRecord {
        batch_id: batch_id.clone(),
        title,
        prompt: prompt.clone(),
        count,
        status: "running".into(),
        created_at_ms: now_ms(),
        branches: worktrees
            .into_iter()
            .map(|(index, branch_name, worktree_path)| BatchBranchRecord {
                index,
                branch_name,
                worktree_path,
                status: "running".into(),
                error: None,
                summary: None,
                spent_usd: 0.0,
            })
            .collect(),
        adopted_index: None,
        base_branch,
        base_commit,
        repo_root,
        inbox_item_id: None,
    };

    let handle = registry.register(record).await;
    handle.persist_current().await;
    let _ = app.emit(event_names::BATCH_UPDATED, handle.dto().await);

    spawn_branch_tasks(
        registry.clone(),
        deps.clone(),
        app.clone(),
        handle.clone(),
        factory,
    )
    .await;

    Ok(BatchRunStarted { batch_id })
}

// ── Query / diff ─────────────────────────────────────────────────────────

/// `list_batch_runs` (frozen contract).
#[tauri::command]
pub async fn list_batch_runs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BatchRunDto>, String> {
    Ok(state.batch_runs.list().await)
}

/// `get_batch_branch_diff` (frozen contract): the branch worktree's diff
/// against the batch's base commit.
#[tauri::command]
pub async fn get_batch_branch_diff(
    state: tauri::State<'_, AppState>,
    batch_id: String,
    index: u32,
) -> Result<BranchDiffDto, String> {
    let handle = state.batch_runs.get_or_load(&batch_id).await?;
    let (worktree, base_commit) = {
        let record = handle.record.lock().await;
        let branch = record
            .branch(index)
            .ok_or_else(|| format!("batch {batch_id} has no branch {index}"))?;
        (branch.worktree_path.clone(), record.base_commit.clone())
    };
    tokio::task::spawn_blocking(move || branch_patch_sync(&worktree, &base_commit))
        .await
        .map_err(|e| format!("diff task failed: {e}"))?
        .map(|diff| BranchDiffDto { diff })
}

// ── Adopt / discard ──────────────────────────────────────────────────────

/// `adopt_batch_branch` (frozen contract): merge the branch back into the
/// base repo's checked-out branch and clean up the other branches. On merge
/// conflicts NOTHING is pushed or deleted — the conflicting file list is
/// returned and the branch + worktree stay untouched for manual handling.
#[tauri::command]
pub async fn adopt_batch_branch(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    batch_id: String,
    index: u32,
) -> Result<AdoptBranchResult, String> {
    adopt_batch_branch_inner(
        &state.batch_runs,
        &state.inbox_store(),
        &app_handle,
        &batch_id,
        index,
    )
    .await
}

pub(crate) async fn adopt_batch_branch_inner<R: tauri::Runtime>(
    registry: &Arc<BatchRunRegistry>,
    inbox: &Arc<shannon_core::inbox_store::InboxStore>,
    app: &tauri::AppHandle<R>,
    batch_id: &str,
    index: u32,
) -> Result<AdoptBranchResult, String> {
    let handle = registry.get_or_load(batch_id).await?;
    let (branch, record_snapshot) = {
        let record = handle.record.lock().await;
        match record.status.as_str() {
            "running" => {
                return Err(
                    "batch is still running — wait for all branches to finish before adopting"
                        .into(),
                );
            }
            "adopted" => return Err("batch was already adopted".into()),
            "discarded" => return Err("batch was discarded".into()),
            _ => {}
        }
        let branch = record
            .branch(index)
            .cloned()
            .ok_or_else(|| format!("batch {batch_id} has no branch {index}"))?;
        if branch.status != "completed" {
            return Err(format!(
                "branch {index} is {} — only completed branches can be adopted",
                branch.status
            ));
        }
        (branch, record.clone())
    };

    // 1. Capture whatever the agent left uncommitted so the merge has
    //    commits to work with.
    // 2. Merge into the base repo. Conflicts abort cleanly here.
    let outcome = {
        let worktree = branch.worktree_path.clone();
        let branch_name = branch.branch_name.clone();
        let repo_root = record_snapshot.repo_root.clone();
        tokio::task::spawn_blocking(move || -> Result<MergeOutcome, String> {
            auto_commit_worktree(&worktree, &branch_name)?;
            merge_branch_sync(&repo_root, &branch_name)
        })
        .await
        .map_err(|e| format!("merge task failed: {e}"))??
    };
    let conflicts = match outcome {
        MergeOutcome::Merged => None,
        MergeOutcome::Conflicts(files) => Some(files),
    };
    let Some(conflicts) = conflicts else {
        // Success: mark adopted, clean up the other branches (blocking git —
        // off the runtime and off the record lock, same as discard), refresh
        // the completion inbox item in place.
        let (removed, _skipped) = {
            let snapshot = handle.record.lock().await.clone();
            tokio::task::spawn_blocking(move || cleanup_branches_sync(&snapshot, Some(index)))
                .await
                .map_err(|e| format!("cleanup task failed: {e}"))?
        };
        tracing::info!(batch = %batch_id, removed, "batch: cleaned up non-adopted branches");
        let dto = {
            let mut record = handle.record.lock().await;
            record.status = "adopted".into();
            record.adopted_index = Some(index);
            if let Some(item_id) = record.inbox_item_id {
                let summary = format!(
                    "Adopted branch #{index} ({}) · {} other branch(es) cleaned up",
                    branch.branch_name,
                    record.count.saturating_sub(1)
                );
                if let Err(e) = inbox.update_item_content(item_id, &summary, None) {
                    tracing::warn!(batch = %record.batch_id, error = %e, "batch: failed to refresh inbox item");
                }
            }
            handle.persist(&record);
            record.dto()
        };
        let _ = app.emit(event_names::BATCH_UPDATED, dto);
        let _ = app.emit(event_names::INBOX_UPDATED, batch_id.to_string());
        return Ok(AdoptBranchResult {
            merged: true,
            conflicts: None,
        });
    };

    // Conflicts: the batch stays as-is (worktrees intact) and the user gets
    // the file list plus guidance to resolve manually.
    Ok(AdoptBranchResult {
        merged: false,
        conflicts: Some(conflicts),
    })
}

/// `discard_batch_run` (frozen contract, plus the additive `skipped` list).
#[tauri::command]
pub async fn discard_batch_run(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    batch_id: String,
) -> Result<DiscardResult, String> {
    discard_batch_run_inner(
        &state.batch_runs,
        &state.inbox_store(),
        &app_handle,
        &batch_id,
    )
    .await
}

pub(crate) async fn discard_batch_run_inner<R: tauri::Runtime>(
    registry: &Arc<BatchRunRegistry>,
    inbox: &Arc<shannon_core::inbox_store::InboxStore>,
    app: &tauri::AppHandle<R>,
    batch_id: &str,
) -> Result<DiscardResult, String> {
    let handle = registry.get_or_load(batch_id).await?;
    {
        let record = handle.record.lock().await;
        match record.status.as_str() {
            "adopted" => return Err("batch was already adopted — nothing to discard".into()),
            "discarded" => return Err("batch was already discarded".into()),
            _ => {}
        }
    }

    // Snapshot for the blocking cleanup (decisions need branch + repo info).
    let record_snapshot = handle.record.lock().await.clone();
    let (removed, skipped) = tokio::task::spawn_blocking({
        let record = record_snapshot.clone();
        move || cleanup_branches_sync(&record, None)
    })
    .await
    .map_err(|e| format!("cleanup task failed: {e}"))?;

    let dto = {
        let mut record = handle.record.lock().await;
        record.status = "discarded".into();
        if let Some(item_id) = record.inbox_item_id {
            let summary = format!(
                "Discarded · {} branch(es) removed, {} skipped",
                removed,
                skipped.len()
            );
            if let Err(e) = inbox.update_item_content(item_id, &summary, None) {
                tracing::warn!(batch = %record.batch_id, error = %e, "batch: failed to refresh inbox item");
            }
        }
        handle.persist(&record);
        record.dto()
    };
    let _ = app.emit(event_names::BATCH_UPDATED, dto);
    let _ = app.emit(event_names::INBOX_UPDATED, batch_id.to_string());

    Ok(DiscardResult { removed, skipped })
}

/// Worktree+branch removal for every removable branch, skipping the adopted
/// one when `except` is set. Blocking (git subprocesses) — run inside
/// `spawn_blocking`.
fn cleanup_branches_sync(record: &BatchRunRecord, except: Option<u32>) -> (u32, Vec<String>) {
    let mut removed = 0u32;
    let mut skipped = Vec::new();
    let repo_root = Path::new(&record.repo_root);
    for branch in &record.branches {
        if except == Some(branch.index) {
            continue;
        }
        match discard_decision(record, branch) {
            DiscardDecision::Skip(reason) => {
                skipped.push(format!("{}: {reason}", branch.branch_name));
            }
            DiscardDecision::Remove => {
                let mut failure: Option<String> = None;
                if Path::new(&branch.worktree_path).is_dir() {
                    if let Err(e) = shannon_core::scheduled_worktree::remove_in(
                        repo_root,
                        Path::new(&branch.worktree_path),
                    ) {
                        failure = Some(e.to_string());
                    }
                }
                if failure.is_none() {
                    if let Err(e) = delete_branch_sync(&record.repo_root, &branch.branch_name) {
                        failure = Some(e);
                    }
                }
                match failure {
                    Some(e) => skipped.push(format!("{}: {e}", branch.branch_name)),
                    None => removed += 1,
                }
            }
        }
    }
    (removed, skipped)
}

// ── Tests ────────────────────────────────────────────────────────────────
//
// Git-touching paths run against real throwaway repositories (the same
// fixture discipline as the scheduled-worktree tests); orchestration is
// driven through the injectable [`BranchRunnerFactory`] (T4 GoalTurnRunner
// pattern) so no test needs an LLM.

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    // ── fixtures ─────────────────────────────────────────────────────────

    /// Run git in `dir`, asserting success; returns trimmed stdout.
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git binary must be on PATH");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct TestEnv {
        _dir: tempfile::TempDir,
        repo_root: PathBuf,
        store_dir: PathBuf,
        registry: Arc<BatchRunRegistry>,
        inbox: Arc<shannon_core::inbox_store::InboxStore>,
        deps: BatchRunDeps,
        app: tauri::AppHandle<tauri::test::MockRuntime>,
    }

    fn env() -> TestEnv {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo_root = dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        git(&repo_root, &["init"]);
        git(
            &repo_root,
            &["config", "user.email", "batch-test@shannon.local"],
        );
        git(&repo_root, &["config", "user.name", "Shannon Batch Test"]);
        std::fs::write(repo_root.join("base.txt"), "base v1\n").unwrap();
        git(&repo_root, &["add", "-A"]);
        git(&repo_root, &["commit", "-q", "-m", "init"]);

        let store_dir = dir.path().join("batch-runs");
        let inbox = Arc::new(
            shannon_core::inbox_store::InboxStore::open_with_legacy(
                &dir.path().join("inbox.db"),
                None,
            )
            .unwrap(),
        );
        TestEnv {
            store_dir,
            registry: Arc::new(BatchRunRegistry::with_dir(dir.path().join("batch-runs"))),
            inbox: inbox.clone(),
            deps: BatchRunDeps {
                inbox,
                usage_store: Arc::new(crate::commands_usage::UsageStore::with_path(
                    dir.path().join("usage.jsonl"),
                )),
                client_config: Arc::new(RwLock::new(
                    shannon_engine::api::types::LlmClientConfig::default(),
                )),
                desktop_config: Arc::new(RwLock::new(DesktopConfig::default())),
                tools: Arc::new(shannon_core::tools::ToolRegistry::new()),
            },
            repo_root,
            app: tauri::test::mock_app().handle().clone(),
            _dir: dir,
        }
    }

    impl TestEnv {
        fn base_commit(&self) -> String {
            git(&self.repo_root, &["rev-parse", "HEAD"])
        }

        fn base_branch(&self) -> String {
            git(&self.repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])
        }

        fn branch_exists(&self, branch: &str) -> bool {
            !git(&self.repo_root, &["branch", "--list", branch]).is_empty()
        }
    }

    /// Per-branch edit applied while fabricating a batch:
    /// `None` = clean, `Some(true)` = committed change, `Some(false)` =
    /// uncommitted change (left for adopt's auto-commit).
    fn edit_kind(n: usize, commit: bool, worktree: &Path) {
        std::fs::write(worktree.join("base.txt"), format!("branch #{n} edit\n")).unwrap();
        git(worktree, &["add", "-A"]);
        if commit {
            git(
                worktree,
                &[
                    "-c",
                    "user.email=batch-test@shannon.local",
                    "-c",
                    "user.name=Shannon Batch Test",
                    "commit",
                    "-q",
                    "-m",
                    &format!("branch {n} work"),
                ],
            );
        }
    }

    /// Build a batch record with REAL worktrees forked from the repo HEAD,
    /// register + persist it. `statuses`/`edits` drive the branch table.
    async fn fabricate(
        env: &TestEnv,
        statuses: &[&str],
        edits: &[Option<bool>],
    ) -> (String, Arc<BatchRunHandle>) {
        assert_eq!(statuses.len(), edits.len());
        let batch_id = uuid::Uuid::new_v4().to_string();
        let id8: String = batch_id.chars().take(8).collect();
        let commit = env.base_commit();
        let base_dir = env
            .repo_root
            .join(shannon_core::scheduled_worktree::DEFAULT_BASE_DIR);
        let mut branches = Vec::new();
        for (n, status) in statuses.iter().enumerate() {
            let name = format!("batch-{id8}-{n}");
            let path = shannon_core::scheduled_worktree::create_named(
                &env.repo_root,
                &base_dir,
                &name,
                &name,
                &commit,
            )
            .unwrap();
            if let Some(committed) = edits[n] {
                edit_kind(n, committed, &path);
            }
            branches.push(BatchBranchRecord {
                index: n as u32,
                branch_name: name,
                worktree_path: path.to_string_lossy().into_owned(),
                status: (*status).into(),
                error: (*status == "failed").then(|| "engine exploded".to_string()),
                summary: None,
                spent_usd: 0.0,
            });
        }
        let mut record = BatchRunRecord {
            batch_id: batch_id.clone(),
            title: "Test batch".into(),
            prompt: "do the thing".into(),
            count: branches.len() as u32,
            status: "running".into(),
            created_at_ms: now_ms(),
            branches,
            adopted_index: None,
            base_branch: env.base_branch(),
            base_commit: commit,
            repo_root: env.repo_root.to_string_lossy().into_owned(),
            inbox_item_id: None,
        };
        // Batch-status consistency: a fabricated batch with no running
        // branches is already terminal.
        if record.branches.iter().all(|b| b.status != "running") {
            record.status = aggregate_status(&record.branches).to_string();
        }
        let handle = env.registry.register(record).await;
        handle.persist_current().await;
        (batch_id, handle)
    }

    async fn set_inbox_item_id(handle: &BatchRunHandle, item_id: i64) {
        handle.record.lock().await.inbox_item_id = Some(item_id);
    }

    /// Scripted per-branch outcome for the stub factory.
    #[derive(Clone)]
    struct StubSpec {
        completed: bool,
        error: Option<String>,
        /// Write a file into the branch worktree before "running" (agent edit).
        edit: bool,
        delay_ms: u64,
    }

    impl Default for StubSpec {
        fn default() -> Self {
            Self {
                completed: true,
                error: None,
                edit: false,
                delay_ms: 0,
            }
        }
    }

    #[derive(Default)]
    struct ConcurrencyProbe {
        current: AtomicUsize,
        max: AtomicUsize,
    }

    struct StubFactoryInner {
        specs: std::sync::Mutex<HashMap<u32, StubSpec>>,
        prompts: std::sync::Mutex<Vec<(u32, String)>>,
        probe: ConcurrencyProbe,
    }

    /// Stub [`BranchRunnerFactory`]: missing indices default to a successful
    /// no-edit branch; every runner records the prompt it was handed and
    /// tracks executor concurrency through the shared probe.
    struct StubFactory {
        inner: Arc<StubFactoryInner>,
    }

    impl StubFactory {
        fn new(specs: HashMap<u32, StubSpec>) -> Self {
            Self {
                inner: Arc::new(StubFactoryInner {
                    specs: std::sync::Mutex::new(specs),
                    prompts: std::sync::Mutex::new(Vec::new()),
                    probe: ConcurrencyProbe::default(),
                }),
            }
        }

        fn prompts(&self) -> Vec<(u32, String)> {
            self.inner.prompts.lock().unwrap().clone()
        }

        fn max_observed_concurrency(&self) -> usize {
            self.inner.probe.max.load(Ordering::SeqCst)
        }
    }

    struct StubRunner {
        spec: StubSpec,
        index: u32,
        /// The branch worktree, resolved from the handle at build time (the
        /// spawn's prompt already embeds it; the stub edits the directory).
        worktree: String,
        inner: Arc<StubFactoryInner>,
    }

    impl BatchBranchRunner for StubRunner {
        fn run(
            &mut self,
            spawn: BranchSpawn,
        ) -> Pin<Box<dyn Future<Output = BranchObservation> + Send + '_>> {
            Box::pin(async move {
                self.inner
                    .prompts
                    .lock()
                    .unwrap()
                    .push((self.index, spawn.prompt.clone()));
                if self.spec.edit && !self.worktree.is_empty() {
                    std::fs::write(
                        Path::new(&self.worktree).join("branch.txt"),
                        format!("branch #{} output\n", self.index),
                    )
                    .unwrap();
                }
                let now = self.inner.probe.current.fetch_add(1, Ordering::SeqCst) + 1;
                self.inner.probe.max.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(self.spec.delay_ms)).await;
                self.inner.probe.current.fetch_sub(1, Ordering::SeqCst);
                BranchObservation {
                    completed: self.spec.completed,
                    error: self.spec.error.clone(),
                }
            })
        }
    }

    impl BranchRunnerFactory for StubFactory {
        fn build(&self, handle: &Arc<BatchRunHandle>, index: u32) -> Box<dyn BatchBranchRunner> {
            let spec = self
                .inner
                .specs
                .lock()
                .unwrap()
                .get(&index)
                .cloned()
                .unwrap_or_default();
            // Uncontended at build time — no branch is executing yet.
            let worktree = handle
                .record
                .try_lock()
                .map(|record| {
                    record
                        .branch(index)
                        .map(|b| b.worktree_path.clone())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            Box::new(StubRunner {
                spec,
                index,
                worktree,
                inner: self.inner.clone(),
            })
        }
    }

    /// Poll until the batch reaches a terminal aggregate status.
    async fn wait_terminal(handle: &BatchRunHandle) -> BatchRunDto {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let dto = handle.dto().await;
            if dto.status != "running" {
                return dto;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "batch never reached a terminal state"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    // ── pure helpers / DTO contract ──────────────────────────────────────

    #[test]
    fn dto_serializes_camel_case_frozen_shape() {
        let record = BatchRunRecord {
            batch_id: "b-1234".into(),
            title: "T".into(),
            prompt: "P".into(),
            count: 2,
            status: "completed".into(),
            created_at_ms: 42,
            branches: vec![BatchBranchRecord {
                index: 0,
                branch_name: "batch-b-1234-0".into(),
                worktree_path: "/wt".into(),
                status: "completed".into(),
                error: None,
                summary: Some(BranchDiffSummary {
                    files_changed: 3,
                    additions: 10,
                    deletions: 2,
                }),
                spent_usd: 0.5,
            }],
            adopted_index: None,
            base_branch: "main".into(),
            base_commit: "c0ffee".into(),
            repo_root: "/repo".into(),
            inbox_item_id: Some(7),
        };
        let json = serde_json::to_value(record.dto()).unwrap();
        for key in [
            "batchId",
            "title",
            "prompt",
            "count",
            "status",
            "createdAtMs",
            "branches",
            "adoptedIndex", // additive
        ] {
            assert!(json.get(key).is_some(), "missing frozen field {key}");
        }
        assert!(json.get("batch_id").is_none(), "no snake_case leakage");
        let branch = &json["branches"][0];
        for key in [
            "index",
            "branchName",
            "worktreePath",
            "status",
            "error",
            "summary",
            "spentUsd",
        ] {
            assert!(
                branch.get(key).is_some(),
                "missing frozen branch field {key}"
            );
        }
        assert!(branch["error"].is_null(), "None error serializes as null");
        for key in ["filesChanged", "additions", "deletions"] {
            assert!(
                branch["summary"].get(key).is_some(),
                "missing summary field {key}"
            );
        }
    }

    #[test]
    fn aggregate_status_rules() {
        let branch = |status: &str| BatchBranchRecord {
            status: status.into(),
            ..batch_branch_template()
        };
        assert_eq!(aggregate_status(&[branch("completed")]), "completed");
        assert_eq!(
            aggregate_status(&[branch("completed"), branch("failed")]),
            "partially_failed"
        );
        assert_eq!(
            aggregate_status(&[branch("failed"), branch("failed")]),
            "failed"
        );
    }

    fn batch_branch_template() -> BatchBranchRecord {
        BatchBranchRecord {
            index: 0,
            branch_name: "b".into(),
            worktree_path: "/wt".into(),
            status: "completed".into(),
            error: None,
            summary: None,
            spent_usd: 0.0,
        }
    }

    #[test]
    fn summarize_numstat_counts_files_and_lines() {
        let s = summarize_numstat("3\t1\tsrc/a.rs\n-\t-\tlogo.bin\n12\t0\tsrc/b.rs\n");
        assert_eq!(s.files_changed, 3, "binary rows count as one file");
        assert_eq!(s.additions, 15, "binary `-` parses as 0");
        assert_eq!(s.deletions, 1);
        assert_eq!(summarize_numstat("").files_changed, 0);
    }

    #[test]
    fn branch_prompt_carries_worktree_directive() {
        let p = branch_prompt(
            "ship it",
            "/repo/.shannon/scheduled-worktrees/batch-x-0",
            "batch-x-0",
        );
        assert!(p.contains("/repo/.shannon/scheduled-worktrees/batch-x-0"));
        assert!(p.contains("batch-x-0"));
        assert!(p.contains("Task:\nship it"), "{p}");
    }

    // ── start: validation + orchestration ────────────────────────────────

    #[tokio::test]
    async fn start_validates_count_prompt_and_repo() {
        let env = env();
        let factory = Arc::new(StubFactory::new(HashMap::new()));

        for bad in [1u32, 0, 5] {
            let err = start_batch_run_inner(
                &env.deps,
                &env.registry,
                &env.app,
                BatchStartRequest::new("t".into(), "p".into(), bad, Some(env.repo_root.clone())),
                factory.clone(),
            )
            .await
            .unwrap_err();
            assert!(err.contains("count must be between 2 and 4"), "{err}");
        }

        let err = start_batch_run_inner(
            &env.deps,
            &env.registry,
            &env.app,
            BatchStartRequest::new("t".into(), "   ".into(), 2, Some(env.repo_root.clone())),
            factory.clone(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("prompt must not be empty"), "{err}");

        let plain = tempfile::tempdir().unwrap();
        let err = start_batch_run_inner(
            &env.deps,
            &env.registry,
            &env.app,
            BatchStartRequest::new("t".into(), "p".into(), 2, Some(plain.path().to_path_buf())),
            factory,
        )
        .await
        .unwrap_err();
        assert!(err.contains("not a git repository"), "{err}");
    }

    #[tokio::test]
    async fn start_rolls_back_created_worktrees_and_branches_on_failure() {
        let env = env();
        // Delegate to the real creator, then fail from the SECOND call on:
        // branch 0's worktree+branch exist and MUST be rolled back.
        let calls = Arc::new(AtomicUsize::new(0));
        let creator: WorktreeCreator = {
            let calls = calls.clone();
            Arc::new(move |repo, base, dir, branch, commit| {
                if calls.fetch_add(1, Ordering::SeqCst) >= 1 {
                    return Err(shannon_core::scheduled_worktree::WorktreeError::GitFailed {
                        stderr: "injected: second creation fails".into(),
                    });
                }
                shannon_core::scheduled_worktree::create_named(repo, base, dir, branch, commit)
            })
        };

        let err = start_batch_run_inner(
            &env.deps,
            &env.registry,
            &env.app,
            BatchStartRequest {
                title: "t".into(),
                prompt: "p".into(),
                count: 3,
                repo_hint: Some(env.repo_root.clone()),
                worktree_creator: creator,
            },
            Arc::new(StubFactory::new(HashMap::new())),
        )
        .await
        .unwrap_err();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "creation stops at the failed call"
        );
        assert!(
            err.contains("failed to create worktree for branch 1"),
            "the failing branch is surfaced: {err}"
        );

        // Rollback removed branch 0's worktree AND its batch- branch. No
        // scheduled-worktrees entries survive in `git worktree list`, and no
        // batch-* refs survive (review fix: the deletion used to receive the
        // worktree PATH instead of the branch name, tripping the batch-
        // prefix guard and silently no-op'ing).
        let worktrees = git(&env.repo_root, &["worktree", "list", "--porcelain"]);
        assert!(
            !worktrees.contains("scheduled-worktrees"),
            "created worktrees must be rolled back: {worktrees}"
        );
        assert!(
            git(&env.repo_root, &["branch", "--list", "batch-*"]).is_empty(),
            "batch branches must be deleted on rollback"
        );
        // Nothing was registered or persisted — the failed start is invisible.
        assert!(env.registry.list().await.is_empty());
    }

    #[tokio::test]
    async fn start_spawns_two_parallel_branches_to_completion() {
        let env = env();
        // Both stub branches edit their worktree (uncommitted agent output).
        let mut specs = HashMap::new();
        specs.insert(
            0,
            StubSpec {
                edit: true,
                ..StubSpec::default()
            },
        );
        specs.insert(
            1,
            StubSpec {
                edit: true,
                delay_ms: 40,
                ..StubSpec::default()
            },
        );
        let factory = Arc::new(StubFactory::new(specs));

        let started = start_batch_run_inner(
            &env.deps,
            &env.registry,
            &env.app,
            BatchStartRequest::new(
                "Fix the login bug".into(),
                "Make login resilient".into(),
                2,
                Some(env.repo_root.clone()),
            ),
            factory.clone(),
        )
        .await
        .unwrap();

        let handle = env.registry.get_or_load(&started.batch_id).await.unwrap();
        let dto = wait_terminal(&handle).await;

        assert_eq!(dto.status, "completed");
        assert_eq!(dto.count, 2);
        let id8: String = started.batch_id.chars().take(8).collect();
        for (n, branch) in dto.branches.iter().enumerate() {
            assert_eq!(branch.index as usize, n);
            assert_eq!(branch.branch_name, format!("batch-{id8}-{n}"));
            assert_eq!(branch.status, "completed");
            assert!(branch.error.is_none());
            assert!(Path::new(&branch.worktree_path).is_dir());
            let summary = branch.summary.expect("stub edited the worktree");
            assert!(
                summary.files_changed >= 1,
                "branch {n} summary: {summary:?}"
            );
        }
        assert!(
            dto.branches
                .iter()
                .all(|b| b.worktree_path.contains(".shannon/scheduled-worktrees"))
        );

        // Every branch was directed at its own worktree.
        let prompts = factory.prompts();
        assert_eq!(prompts.len(), 2);
        for (index, prompt) in &prompts {
            let branch = dto.branches.iter().find(|b| b.index == *index).unwrap();
            assert!(prompt.contains(&branch.worktree_path), "{prompt}");
            assert!(prompt.contains("Make login resilient"), "{prompt}");
        }

        // Completion inbox item: source=batch, per-branch file counts.
        let items = env
            .inbox
            .list(None, Some(shannon_core::inbox_store::SOURCE_BATCH), 10)
            .unwrap();
        assert_eq!(items.len(), 1, "one completion item per batch");
        assert_eq!(items[0].title, "Fix the login bug");
        assert!(
            items[0].summary.contains("file(s) changed"),
            "{}",
            items[0].summary
        );
        // The completion item is back-linked on the record.
        let record = handle.record.lock().await;
        assert!(record.inbox_item_id.is_some());
    }

    #[tokio::test]
    async fn partially_failed_branches_aggregate_to_partially_failed() {
        let env = env();
        let mut specs = HashMap::new();
        specs.insert(
            1,
            StubSpec {
                completed: false,
                error: Some("provider exploded".into()),
                ..StubSpec::default()
            },
        );
        specs.insert(
            2,
            StubSpec {
                edit: true,
                ..StubSpec::default()
            },
        );
        let factory = Arc::new(StubFactory::new(specs));

        let started = start_batch_run_inner(
            &env.deps,
            &env.registry,
            &env.app,
            BatchStartRequest::new(
                "Three ways".into(),
                "do it".into(),
                3,
                Some(env.repo_root.clone()),
            ),
            factory,
        )
        .await
        .unwrap();
        let handle = env.registry.get_or_load(&started.batch_id).await.unwrap();
        let dto = wait_terminal(&handle).await;

        assert_eq!(dto.status, "partially_failed");
        assert_eq!(dto.branches[0].status, "completed");
        assert_eq!(dto.branches[1].status, "failed");
        assert_eq!(dto.branches[1].error.as_deref(), Some("provider exploded"));
        assert_eq!(dto.branches[2].status, "completed");

        // The completion inbox item is still written for a partial batch.
        let items = env
            .inbox
            .list(None, Some(shannon_core::inbox_store::SOURCE_BATCH), 10)
            .unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].summary.contains("failed"), "{}", items[0].summary);
    }

    #[tokio::test]
    async fn all_failed_branches_aggregate_to_failed() {
        let env = env();
        let mut specs = HashMap::new();
        for index in 0..2 {
            specs.insert(
                index,
                StubSpec {
                    completed: false,
                    error: Some(format!("boom {index}")),
                    ..StubSpec::default()
                },
            );
        }
        let factory = Arc::new(StubFactory::new(specs));

        let started = start_batch_run_inner(
            &env.deps,
            &env.registry,
            &env.app,
            BatchStartRequest::new(
                "Doomed".into(),
                "fail".into(),
                2,
                Some(env.repo_root.clone()),
            ),
            factory,
        )
        .await
        .unwrap();
        let handle = env.registry.get_or_load(&started.batch_id).await.unwrap();
        let dto = wait_terminal(&handle).await;

        assert_eq!(dto.status, "failed");
        assert!(dto.branches.iter().all(|b| b.status == "failed"));
    }

    // ── live spend ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn fold_branch_spend_accumulates_per_branch() {
        let env = env();
        let (_, handle) = fabricate(&env, &["running", "running"], &[None, None]).await;

        let _ = fold_branch_spend(&handle, 1, 0.25).await;
        let dto = fold_branch_spend(&handle, 1, 0.5).await;
        assert_eq!(dto.branches[1].spent_usd, 0.75, "branch 1 accumulates");

        let final_dto = fold_branch_spend(&handle, 0, 2.0).await;
        assert_eq!(
            final_dto.branches[0].spent_usd, 2.0,
            "branch 0 accumulates separately"
        );
    }

    // ── concurrency cap ──────────────────────────────────────────────────

    #[tokio::test]
    async fn semaphore_queues_branches_beyond_the_global_cap() {
        let env = env();
        // 6 fabricated branches — more than MAX_CONCURRENT_BATCH_BRANCHES.
        let statuses = ["running"; 6];
        let edits = [None; 6];
        let (_, handle) = fabricate(&env, &statuses, &edits).await;

        let mut specs = HashMap::new();
        for index in 0..6u32 {
            specs.insert(
                index,
                StubSpec {
                    delay_ms: 80,
                    ..StubSpec::default()
                },
            );
        }
        let factory = Arc::new(StubFactory::new(specs));

        spawn_branch_tasks(
            env.registry.clone(),
            env.deps.clone(),
            env.app.clone(),
            handle.clone(),
            factory.clone(),
        )
        .await;

        let dto = wait_terminal(&handle).await;
        assert_eq!(dto.branches.len(), 6);
        assert!(
            dto.branches.iter().all(|b| b.status == "completed"),
            "all queued branches must eventually run"
        );
        // The probe never observed more executors than permits.
        assert!(
            factory.max_observed_concurrency() <= MAX_CONCURRENT_BATCH_BRANCHES,
            "executor concurrency capped at {} (observed {})",
            MAX_CONCURRENT_BATCH_BRANCHES,
            factory.max_observed_concurrency()
        );
    }

    // ── adopt ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn adopt_success_merges_and_cleans_up_others() {
        let env = env();
        // Branch 0: completed with a COMMITTED change; branch 1: completed, clean.
        let (batch_id, handle) =
            fabricate(&env, &["completed", "completed"], &[Some(true), None]).await;

        // Simulate the completion inbox item so adopt refreshes it in place.
        let item = env
            .inbox
            .append_item(InboxItemNew {
                source: shannon_core::inbox_store::SOURCE_BATCH.into(),
                source_id: Some(batch_id.clone()),
                session_id: None,
                title: "Test batch".into(),
                summary: "2 branch(es) finished".into(),
                error: None,
            })
            .unwrap();
        set_inbox_item_id(&handle, item.id).await;

        let branch1_name = handle.dto().await.branches[1].branch_name.clone();
        let branch1_path = handle.dto().await.branches[1].worktree_path.clone();

        let result = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &batch_id, 0)
            .await
            .unwrap();

        assert!(result.merged);
        assert!(result.conflicts.is_none());

        // The merge landed in the base repo's checked-out branch.
        let merged = std::fs::read_to_string(env.repo_root.join("base.txt")).unwrap();
        assert_eq!(merged, "branch #0 edit\n");

        let dto = handle.dto().await;
        assert_eq!(dto.status, "adopted");
        assert_eq!(dto.adopted_index, Some(0));

        // The other branch was cleaned up: worktree dir gone, ref gone.
        assert!(!Path::new(&branch1_path).exists(), "worktree removed");
        assert!(!env.branch_exists(&branch1_name), "branch deleted");
        // The adopted branch + its worktree are preserved (per-brief cleanup
        // covers the OTHER branches only).
        assert!(Path::new(&dto.branches[0].worktree_path).exists());
        assert!(env.branch_exists(&dto.branches[0].branch_name));

        // The completion inbox item was refreshed in place — no new item.
        let items = env
            .inbox
            .list(None, Some(shannon_core::inbox_store::SOURCE_BATCH), 10)
            .unwrap();
        assert_eq!(items.len(), 1);
        assert!(
            items[0].summary.contains("Adopted branch #0"),
            "{}",
            items[0].summary
        );

        // A second adopt is rejected.
        let err = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &batch_id, 0)
            .await
            .unwrap_err();
        assert!(err.contains("already adopted"), "{err}");
    }

    #[tokio::test]
    async fn adopt_conflict_lists_files_and_preserves_worktrees() {
        let env = env();
        let (batch_id, handle) =
            fabricate(&env, &["completed", "completed"], &[Some(true), None]).await;

        // Move the base branch forward with a conflicting edit to base.txt.
        std::fs::write(env.repo_root.join("base.txt"), "base moved on\n").unwrap();
        git(&env.repo_root, &["add", "-A"]);
        git(&env.repo_root, &["commit", "-q", "-m", "base advances"]);

        let before = handle.dto().await;
        let result = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &batch_id, 0)
            .await
            .unwrap();

        assert!(!result.merged, "conflicting merge must not report success");
        assert_eq!(result.conflicts, Some(vec!["base.txt".to_string()]));

        // Nothing was adopted, nothing deleted — manual handling stays open.
        let after = handle.dto().await;
        assert_eq!(after.status, before.status, "batch status unchanged");
        assert_eq!(after.adopted_index, None);
        for branch in &after.branches {
            assert!(
                Path::new(&branch.worktree_path).is_dir(),
                "{} kept",
                branch.branch_name
            );
            assert!(
                env.branch_exists(&branch.branch_name),
                "{} kept",
                branch.branch_name
            );
        }
        // The base repo was restored (merge aborted) — no conflict markers.
        let base_txt = std::fs::read_to_string(env.repo_root.join("base.txt")).unwrap();
        assert_eq!(base_txt, "base moved on\n");
        assert!(git(&env.repo_root, &["status", "--porcelain"]).is_empty());
    }

    #[tokio::test]
    async fn adopt_rejects_running_batches_and_noncompleted_branches() {
        let env = env();
        let (running_id, _) = fabricate(&env, &["running", "running"], &[None, None]).await;
        let err = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &running_id, 0)
            .await
            .unwrap_err();
        assert!(err.contains("still running"), "{err}");

        let (mixed_id, _) = fabricate(&env, &["completed", "failed"], &[None, None]).await;
        let err = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &mixed_id, 1)
            .await
            .unwrap_err();
        assert!(err.contains("only completed branches"), "{err}");

        let err = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, "nope", 0)
            .await
            .unwrap_err();
        assert!(err.contains("batch not found"), "{err}");
    }

    #[tokio::test]
    async fn adopt_captures_uncommitted_worktree_changes() {
        let env = env();
        // Branch 0's "agent" left changes UNCOMMITTED — adopt must commit
        // them so the merge has something to merge.
        let (batch_id, _) =
            fabricate(&env, &["completed", "completed"], &[Some(false), None]).await;

        let result = adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &batch_id, 0)
            .await
            .unwrap();
        assert!(
            result.merged,
            "uncommitted work must be auto-committed: {result:?}"
        );
        let merged = std::fs::read_to_string(env.repo_root.join("base.txt")).unwrap();
        assert_eq!(merged, "branch #0 edit\n");
    }

    // ── discard ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn discard_removes_finished_branches_and_skips_unmerged_or_running() {
        let env = env();
        // 0: completed + clean → removed.
        // 1: failed + committed (unmerged) change → skipped, preserved.
        // 2: failed + clean → removed.
        // 3: running → skipped.
        let (batch_id, handle) = fabricate(
            &env,
            &["completed", "failed", "failed", "running"],
            &[None, Some(true), None, None],
        )
        .await;

        let dto = handle.dto().await;
        let paths: Vec<String> = dto
            .branches
            .iter()
            .map(|b| b.worktree_path.clone())
            .collect();
        let names: Vec<String> = dto.branches.iter().map(|b| b.branch_name.clone()).collect();

        let result = discard_batch_run_inner(&env.registry, &env.inbox, &env.app, &batch_id)
            .await
            .unwrap();
        assert_eq!(result.removed, 2, "branches 0 and 2 removed");
        assert_eq!(
            result.skipped.len(),
            2,
            "branches 1 and 3 skipped: {:?}",
            result.skipped
        );
        assert!(result.skipped.iter().any(|s| s.starts_with(&names[1])));
        assert!(result.skipped.iter().any(|s| s.starts_with(&names[3])));

        assert!(
            !Path::new(&paths[0]).exists(),
            "clean completed worktree removed"
        );
        assert!(!env.branch_exists(&names[0]));
        assert!(
            Path::new(&paths[1]).exists(),
            "failed branch with unmerged work preserved"
        );
        assert!(
            env.branch_exists(&names[1]),
            "unmerged branch ref preserved"
        );
        assert!(
            !Path::new(&paths[2]).exists(),
            "clean failed worktree removed"
        );
        assert!(
            Path::new(&paths[3]).exists(),
            "running branch worktree preserved"
        );

        assert_eq!(handle.dto().await.status, "discarded");

        // Discarding again is rejected.
        let err = discard_batch_run_inner(&env.registry, &env.inbox, &env.app, &batch_id)
            .await
            .unwrap_err();
        assert!(err.contains("already discarded"), "{err}");
    }

    #[tokio::test]
    async fn discard_after_adopt_is_rejected() {
        let env = env();
        let (batch_id, _) = fabricate(&env, &["completed", "completed"], &[Some(true), None]).await;
        adopt_batch_branch_inner(&env.registry, &env.inbox, &env.app, &batch_id, 0)
            .await
            .unwrap();
        let err = discard_batch_run_inner(&env.registry, &env.inbox, &env.app, &batch_id)
            .await
            .unwrap_err();
        assert!(err.contains("already adopted"), "{err}");
    }

    #[tokio::test]
    async fn discard_refreshes_the_completion_inbox_item() {
        let env = env();
        let (batch_id, handle) = fabricate(&env, &["completed", "completed"], &[None, None]).await;
        let item = env
            .inbox
            .append_item(InboxItemNew {
                source: shannon_core::inbox_store::SOURCE_BATCH.into(),
                source_id: Some(batch_id.clone()),
                session_id: None,
                title: "Test batch".into(),
                summary: "2 branch(es) finished".into(),
                error: None,
            })
            .unwrap();
        set_inbox_item_id(&handle, item.id).await;

        discard_batch_run_inner(&env.registry, &env.inbox, &env.app, &batch_id)
            .await
            .unwrap();

        let items = env
            .inbox
            .list(None, Some(shannon_core::inbox_store::SOURCE_BATCH), 10)
            .unwrap();
        assert_eq!(items.len(), 1, "in-place refresh, no noisy tail item");
        assert!(
            items[0].summary.contains("Discarded"),
            "{}",
            items[0].summary
        );
    }

    // ── restart reconciliation ───────────────────────────────────────────

    #[tokio::test]
    async fn restart_reconciliation_marks_running_branches_failed() {
        let env = env();
        let (batch_id, _) = fabricate(&env, &["completed", "running"], &[None, None]).await;

        // Simulate the restart: a fresh registry over the same store dir has
        // no live runner for this batch.
        let fresh = Arc::new(BatchRunRegistry::with_dir(env.store_dir.clone()));
        let runs = fresh.list().await;
        assert_eq!(runs.len(), 1);
        let dto = &runs[0];
        assert_eq!(dto.batch_id, batch_id);
        assert_eq!(
            dto.branches[0].status, "completed",
            "finished branch untouched"
        );
        assert_eq!(dto.branches[1].status, "failed");
        assert_eq!(
            dto.branches[1].error.as_deref(),
            Some(RESTART_INTERRUPTED_ERROR)
        );
        assert_eq!(dto.status, "partially_failed");
        // Worktrees are preserved for manual inspection.
        assert!(Path::new(&dto.branches[1].worktree_path).is_dir());

        // The reconciled state was persisted back to disk.
        let raw = std::fs::read_to_string(env.store_dir.join(format!("{batch_id}.json"))).unwrap();
        assert!(raw.contains(RESTART_INTERRUPTED_ERROR), "{raw}");

        // get_or_load reconciles too — a diff/adopt/discard before a list
        // cannot resurrect stale running state.
        let third = Arc::new(BatchRunRegistry::with_dir(env.store_dir.clone()));
        let handle = third.get_or_load(&batch_id).await.unwrap();
        assert_eq!(handle.dto().await.branches[1].status, "failed");
    }

    #[tokio::test]
    async fn terminal_disk_records_are_not_touched_by_reconciliation() {
        let env = env();
        let (batch_id, _) = fabricate(&env, &["completed", "failed"], &[None, None]).await;
        let raw_before =
            std::fs::read_to_string(env.store_dir.join(format!("{batch_id}.json"))).unwrap();

        let fresh = Arc::new(BatchRunRegistry::with_dir(env.store_dir.clone()));
        let runs = fresh.list().await;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "partially_failed");
        let raw_after =
            std::fs::read_to_string(env.store_dir.join(format!("{batch_id}.json"))).unwrap();
        assert_eq!(
            raw_before, raw_after,
            "terminal records round-trip untouched"
        );
    }

    #[tokio::test]
    async fn store_roundtrip_survives_record_reloads() {
        let env = env();
        let (batch_id, _) = fabricate(&env, &["completed", "completed"], &[Some(true), None]).await;
        // A fresh registry loads the record from disk and can adopt it —
        // the frozen DTO shape must survive the JSON roundtrip.
        let fresh = Arc::new(BatchRunRegistry::with_dir(env.store_dir.clone()));
        let result = adopt_batch_branch_inner(&fresh, &env.inbox, &env.app, &batch_id, 0)
            .await
            .unwrap();
        assert!(result.merged);
    }

    // ── record store safety ──────────────────────────────────────────────

    #[tokio::test]
    async fn get_or_load_rejects_path_tricks() {
        let env = env();
        for evil in ["../escape", "a/b", "a\\b", "..", ""] {
            let loaded = env.registry.get_or_load(evil).await;
            assert!(loaded.is_err(), "{evil}: must not load");
        }
    }
}
