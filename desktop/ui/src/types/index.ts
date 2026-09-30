// TypeScript types matching Rust structs in shannon-desktop/src/events.rs and commands.rs

import type { VoiceLocalConfig } from '@/lib/tauri-api'

// --- Event Payloads ---

export interface QueryTextPayload {
  query_id: string
  content: string
}

export interface ToolStartPayload {
  query_id: string
  tool_use_id: string
  tool_name: string
  tool_input: unknown
}

export interface ToolResultPayload {
  query_id: string
  tool_use_id: string
  tool_name: string
  result: string
  is_error: boolean
  /** P1-⑤: engine tool metadata (e.g. sandbox classification). Absent on
   *  older engines. */
  meta?: unknown
  /** P1-⑤ telemetry: approximate per-tool token attribution collapsed by
   *  the desktop forwarder. */
  tokens_used?: number
}

export interface ToolProgressPayload {
  query_id: string
  tool_use_id: string
  tool_name: string
  progress: number
  message: string
}

export interface ThinkingPayload {
  query_id: string
  content: string
}

export interface UsagePayload {
  query_id: string
  input_tokens: number
  output_tokens: number
  cost_usd: number
  cache_hit_rate?: number
  max_tokens?: number
  /** P1-1: owner session for multi-window event filtering. */
  session_id?: string
}

/** R5-2: which retry the engine surfaced on `query:notice`. Both kinds mean
 *  the request CONTINUED — these are informational, never errors. */
export type QueryNoticeKind = 'failover' | 'key_rotation'

/** R5-2 wire payload for QUERY_NOTICE (`query:notice`). `message` is the
 *  verbatim engine line (e.g. "falling back to glm-5.3-flash@zhipu (rate
 *  limited)") — shown as the notice detail; `kind` drives icon + localized
 *  label. */
export interface QueryNoticeEvent {
  query_id: string
  kind: QueryNoticeKind
  message: string
  session_id?: string
}

export interface QueryCompletedPayload {
  query_id: string
}

export interface QueryFailedPayload {
  query_id: string
  error: string
}

export interface QueryCancelledPayload {
  query_id: string
}

/** P1-3: why a permission prompt was raised (frozen camelCase wire shape). */
export interface PermissionReason {
  /** `rule` (settings/profile rule matched) | `llm` (classifier verdict) | `default`. */
  source: 'rule' | 'llm' | 'default'
  /** Matched rule pattern (e.g. `Bash(git *)`), when known. */
  ruleName: string | null
  /** Classifier confidence in 0..1, when a classifier decided. */
  confidence: number | null
}

export interface PermissionRequest {
  tool: string
  input: unknown
  risk: string
  request_id: string
  /** P1-1: owner session for multi-window prompt filtering. */
  session_id?: string
  /** P1-3: why this prompt was raised. Absent on payloads from older engines. */
  reason?: PermissionReason
}

// --- Core Types ---

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system' | 'tool'
  content: string
  timestamp: number
  tool_calls?: ToolCall[]
  thinking?: string
  file_attachments?: FileAttachment[]
  research_report?: ResearchReport
}

export interface ToolCall {
  tool_use_id: string
  tool_name: string
  tool_input: unknown
  result?: string
  is_error?: boolean
  progress?: number
  progress_message?: string
  status: 'running' | 'completed' | 'error'
  /** P1-⑤ telemetry: wall-clock start (epoch ms) captured at tool-start. */
  started_at?: number
  /** P1-⑤ telemetry: client-measured duration (ms), set when the result
   *  arrives. Historical messages get durations from the L0 trace timeline
   *  instead (see MessageArea's duration lookup). */
  duration_ms?: number
  /** P1-⑤: engine tool metadata (e.g. `{ classification: 'sandbox_denied' }`). */
  meta?: unknown
  /** P1-⑤ telemetry: approximate per-tool token attribution (collapsed by
   *  the desktop forwarder). Historical cards don't carry this. */
  tokens_used?: number
}

/** B2: live registry state of a running sub-agent, bridged from the
 *  desktop agent-teams observer via `subagent:start` / `subagent:stop`. */
export interface SubAgentLive {
  agentId: string
  agentName: string
  team: string | null
}

/** B2 follow-up — desktop backend DTO mirroring
 *  `desktop/src/commands_agents.rs::SubAgentDto`. Stable wire shape used
 *  by the Tasks page panel + the `useSubagents` hook. */
export interface SubAgentDto {
  id: string
  name: string
  team: string | null
  status: string
  turnsUsed: number
  maxTurns: number
  model: string
  createdAtMs: number
}

export interface ResearchReport {
  title: string
  summary: string
  sections: ResearchSection[]
  citations: ResearchCitation[]
  generated_at: number
}

export interface ResearchSection {
  heading: string
  body: string
}

export interface ResearchCitation {
  id: number
  title: string
  url?: string
  snippet?: string
  source?: string
  accessed_at?: number
}

export interface FileAttachment {
  name: string
  path: string
  size: number
}

export interface SessionInfo {
  id: string
  title: string
  created_at: number
  message_count: number
  /** True if this conversation was initiated by an agent (not a direct user prompt). */
  is_agent_run?: boolean
  /** True if this conversation is tied to a scheduled/routine trigger. */
  is_scheduled?: boolean
  /** True if the user has pinned this conversation. */
  is_pinned?: boolean
  /** Per-session working directory override (absolute path). */
  working_dir?: string
  /** Parent session ID if this is a branch */
  parent_id?: string | null
  /** Message index in parent where this branch diverged */
  branch_point?: number | null
  /** P0 sidebar telemetry: live-query flag joined from the session registry.
   *  Absent on older engines — treat as unknown, not false. */
  running?: boolean
  /** P0 sidebar telemetry: epoch **ms** of the session's last activity
   *  (L0 log mtime). Absent on older engines / brand-new sessions. */
  updated_at?: number
}

/** Session archive (卡A): one archived session as the sidebar's 已归档
 *  section renders it — returned by the `list_archived_sessions` command. */
export interface ArchivedSessionRow {
  id: string
  /** Curated title; null → the UI renders its "untitled" placeholder. */
  title?: string | null
  /** Last activity, epoch ms; absent when unknown. */
  updated_at?: number | null
}

/**
 * P0 plan dock: one persisted plan file from the session working dir
 * (`<workingDir>/.shannon/plans/*.md`), parsed by the `get_session_plan`
 * command. Mirrors the engine `PlanManager::save_plan_to_file` format.
 */
export interface SessionPlan {
  /** Plan file stem (the engine's plan id). */
  id: string
  title: string
  /** `"approved" | "pending"` — raw header value. */
  status: string
  /** RFC3339 creation timestamp (raw header value). */
  created_at: string
  /** Markdown body (everything after the header block). */
  content: string
}

/**
 * P0 sidebar telemetry: live per-session activity derived from the query:*
 * event stream (and reconciled with `SessionInfo.running` on refresh).
 * `startedAt === null` means "running, start unknown" (e.g. a goal-owned run
 * that began before this window joined the event stream).
 */
export interface SessionActivity {
  running: boolean
  startedAt: number | null
  lastActivity: number
  activeTool: string | null
  /** Batch B2: last run ended with QUERY_FAILED — red dot on the rail until
   *  a new run starts or the session is opened (seen). */
  failed?: boolean
  /** Batch B2: a permission prompt is pending for this session — amber dot. */
  awaitingApproval?: boolean
}

export interface StatusResponse {
  model: string
  provider: string
  querying: boolean
  message_count: number
  working_dir: string
}

export interface ModelInfo {
  id: string
  name: string
  provider: string
  /** Tokens. 0 means unknown — the UI should render "unknown" instead of
   *  fabricating a number (ADR-0005 P0-2 honest cost/context). */
  context_window: number
  /** Per-million-token input price. null = unknown / fallback. */
  price_in?: number | null
  /** Per-million-token output price. null = unknown / fallback. */
  price_out?: number | null
  /** Optional tier label (`fast` / `standard` / `pro`). */
  tier?: string | null
  /** Whether this entry comes from the dynamic models.dev overlay (vs the
   *  static catalog). Surfaces a freshness indicator in the UI. */
  dynamic?: boolean
  /** Vision (image input) capability from the catalog metadata. `undefined`
   *  / null = unknown — the UI renders no capability dot rather than
   *  guessing (R2-3, honest metadata). */
  vision?: boolean | null
}

export interface ToolInfo {
  name: string
  description: string
  enabled: boolean
}

export interface ConfigUpdate {
  key: string
  value: string
}

/// Gateway (shannon-gateway) config — mirrors the on-disk shape of
/// `~/.shannon/gateway/config.json` (camelCase, same schema the gateway's own
/// loader validates). Kept structurally identical so the desktop can pass it
/// through unchanged.
export interface GatewayEngineConfig {
  wsUrl: string
  httpBaseUrl: string
  model?: string
}

export interface GatewayAdapterConfig {
  platform: string
  enabled: boolean
  options?: Record<string, unknown>
  /// adapter-local secret name → OS-keyring key (e.g. `botToken` → `slack/bot-token`).
  /// Values are keyring key NAMES, never the secrets themselves.
  secrets?: Record<string, string>
}

export interface GatewayMobileConfig {
  enabled?: boolean
  host?: string
  port?: number
  tokensFile?: string
  devicesFile?: string
  /// v0.12 LAN hardening: serve the mobile face over wss with the persisted
  /// self-signed cert; phones pin the QR-carried fingerprint. Default off.
  tls?: { enabled?: boolean }
}

/// `mobile_tls_status` result — config flag + live cert material info.
export interface MobileTlsStatus {
  enabled: boolean
  /// SHA-256 (lowercase hex) of the gateway's self-signed cert; present only
  /// after the gateway first boots with TLS on (null until then).
  fingerprint: string | null
}

export interface GatewayConfig {
  engine: GatewayEngineConfig
  adapters: GatewayAdapterConfig[]
  logLevel?: string
  mobile?: GatewayMobileConfig
}

/// One paired device entry. Mirrors `DeviceEntry` in
/// `shannon-gateway/src/mobile/pairing.ts` (camelCase, passed through).
export interface MobileDeviceEntry {
  deviceId: string
  publicKey: string
  label?: string | null
  addedAt: number
  lastSeenAt: number
}

/// `mobile_generate_pair_token` result — token + LAN endpoint + QR data URL.
export interface MobilePairToken {
  token: string
  expiresAt: number
  lanEndpoint: string
  qrDataUrl: string
}

/// One pending (or just-approved) IM pairing request (T9). Mirrors the Rust
/// `GatewayPairingRequest` in `desktop/src/gateway_pairing.rs` and the
/// gateway's `PairingRequestRecord` (camelCase, passed through).
export interface GatewayPairingRequest {
  /// The 6-digit code shown in the IM pairing challenge.
  code: string
  /// Chat platform the requester came from (slack/telegram/…).
  platform: string
  /// Platform sender id the allowlist entry carries.
  senderId: string
  /// Epoch ms when the challenge was issued.
  requestedAt: number
  /// Epoch ms after which the code expires (issue + 5 min).
  expiresAt: number
}

/// E-1 方案 C — supervised gateway process status. Mirrors the Rust
/// `GatewaySupervisorStatus` enum (externally-tagged serde, camelCase variants).
export type GatewaySupervisorStatus =
  | 'stopped'
  | 'notInstalled'
  | { running: { pid: number } }
  | { exited: { code: number | null; reason: string } }

/// `gateway_supervisor_*` command return shape: the 方案 C `managed` flag + the
/// process status in one round-trip.
export interface GatewayProcessState {
  managed: boolean
  status: GatewaySupervisorStatus
}

/// ADR-0011 B7: every surface self-identifies (routing / telemetry / support).
export interface SurfaceInfo {
  surface: string
  version: string
}

/// ADR-0011 B3: is the bundled `shannon` CLI reachable from a shell?
export interface CliInstallStatus {
  onPath: boolean
  onPathVersion: string | null
  bundledPath: string | null
  handledByInstaller: boolean
}

/// ADR-0011 B3: `install_cli_to_path` result.
export interface CliInstallResult {
  status: CliInstallStatus
  installedLink: string | null
  message: string
}

/// C1①: semi-automatic update check (GitHub latest vs. this build).
export interface AppUpdateInfo {
  currentVersion: string
  latestVersion: string | null
  updateAvailable: boolean
  releaseUrl: string
  error: string | null
}

/// A managed provider connection kind. `openai-compatible` covers any
/// OpenAI-style endpoint (GLM/Zhipu, Moonshot/Kimi, MiniMax, Together, Groq…).
export type ProviderKind =
  | 'anthropic'
  | 'openai'
  | 'deepseek'
  | 'ollama'
  | 'openai-compatible'

export interface ProviderConnection {
  id: string
  display_name: string
  kind: ProviderKind | string
  /// True when the credential store has a key for this id. Replaces the
  /// dead `api_key` field (TD-4).
  has_api_key: boolean
  base_url?: string | null
  /// v2 ProviderProfile fields surfaced in Phase 2 task 3. The backend
  /// deserializes them into `providers.json` + engine's `providers.toml`
  /// (see `desktop/src/commands_config.rs::apply_provider_update`).
  /// Optional on the wire because every field is `#[serde(default)]` on
  /// the Rust side for backward compat with legacy entries.
  models_url?: string | null
  extra_headers?: Record<string, string>
  default_max_tokens?: number | null
  fallback_models?: string[]
  quirks?: ProviderQuirks
  tiers?: ProviderTiers
}

/// Mirrors `shannon_types::provider_config::ProviderTiers`. Canonical
/// tier names are `fast` / `standard` / `pro` — aliases like `haiku` /
/// `sonnet` / `opus` are engine-resolved and never surfaced to the UI.
export interface ProviderTiers {
  fast?: string | null
  standard?: string | null
  pro?: string | null
}

/// Mirrors `shannon_types::provider_config::ProviderQuirks`. Out of scope
/// for the modal in Phase 2 task 3 — the Add Provider modal only edits
/// `extra_headers`, `default_max_tokens`, and `tiers`. Kept here so the
/// `ProviderConnection` shape round-trips when the backend serializes it.
export interface ProviderQuirks {
  temperature_strategy?: string | null
  max_tokens_override?: number | null
  send_temperature?: boolean
}

export interface ProvidersFile {
  active_provider_id?: string | null
  providers: ProviderConnection[]
}

/// Reliable provider-activation signal from `get_provider_status`
/// (2026-09-29 provider review §2-2). `DesktopConfig.provider`/`api_key`
/// are dead since ADR-0005 — all "is a provider configured" gating reads
/// this instead.
export interface ProviderStatus {
  /// Id of the active managed provider, `null` when nothing is active.
  active_provider_id: string | null
  /// Display name of the active provider, `null` when unset (fall back
  /// to `active_provider_id` for display).
  display_name: string | null
  /// Wire kind slug of the active provider (`anthropic` | `openai` |
  /// `deepseek` | `ollama` | `openai-compatible` | `gemini`).
  kind: string | null
  /// True when the credential store has a key for the active provider.
  has_api_key: boolean
  /// Active model id, `null` when unset (or the `"default"` sentinel).
  model: string | null
  /// Provider detected purely from env vars — only populated when the
  /// store has no active provider, so env-configured users are not
  /// nagged for a key.
  env_provider: string | null
}

/// Payload for adding or editing a managed provider. On edit, `id` identifies
/// the entry; an `api_key` of '***' or empty means "keep the existing key".
///
/// Phase 2 task 3: the three v2 ProviderProfile fields the Add Provider
/// modal authors. Empty-key header rows and empty tier strings are
/// silently dropped on the client before submit so the backend never sees
/// `""` overrides. `default_max_tokens` is `null` when the input is
/// blank; the engine falls back to `cfg.max_tokens` (then 4096) when
/// unset.
export interface ProviderInput {
  id?: string
  display_name: string
  kind: ProviderKind | string
  api_key?: string
  base_url?: string
  model?: string
  extra_headers?: Record<string, string>
  default_max_tokens?: number | null
  tiers?: ProviderTiers
  fallback_models?: string[]
}

export interface DesktopConfig {
  provider?: string
  api_key?: string
  base_url?: string
  model?: string
  working_dir?: string
  theme?: string
  mcp_servers?: McpServerConfig[]
  approval_mode?: string
  version?: string
  strategic_focus?: string
  performance_strategy?: string
  memory_enabled?: boolean
  telemetry_enabled?: boolean
  encryption_enabled?: boolean
  debug_console?: boolean
  temperature?: number
  max_tokens?: number
  plan?: string
  skill_loop_enabled?: boolean
  skill_loop_min_duration_secs?: number
  skill_loop_min_tool_calls?: number
  skill_detection_enabled?: boolean
  /** B2: real sub-agent execution (agent teams). Default off — placeholder
   *  agent_spawn only, until opted in (real LLM spend). */
  agent_teams_enabled?: boolean
  /** Dream pass (梦境提炼): master switch. Default off — when false the
   *  dream pass is skipped without reading any session or memory file. */
  dream_enabled?: boolean
  /** Dream pass L3: refine freshly detected skill candidates inside a
   *  dream pass. Default off; review remains the only write path. */
  dream_skill_distill_enabled?: boolean
  /** 卡A session GC: master switch for auto-cleaning **archived** sessions.
   *  Default off — nothing is ever auto-deleted until opted in, and an
   *  enabled GC still never touches active sessions. */
  session_gc_enabled?: boolean
  /** 卡A session GC: retention window in days, counted from each session's
   *  last activity. `null`/undefined = 永不 (never auto-delete), the
   *  standing default. */
  session_retention_days?: number | null
  stt?: SttConfig
  /** P2-5e local-only STT (whisper-rs). Independent of `stt`
   *  so a user can keep a cloud key for fallback while local
   *  is the primary. */
  voice_local?: VoiceLocalConfig
  /** P1-3: active permission profile (builtin id or custom name). Null/unset = plain approval_mode. */
  active_permission_profile?: string | null
  /** P1-3: command sandbox config — frozen key path `sandbox.mode`. */
  sandbox?: SandboxConfig
  /** P2-5: off-peak execution settings — frozen key path `offpeak.model_override`. */
  offpeak?: OffpeakConfig
  /** R3-3: plan-phase model tier (`fast` | `standard` | `pro`). null/undefined
   *  = inherit — the plan phase uses the global default model. */
  plan_tier?: string | null
  /** R3-3: act-phase model tier — same contract as `plan_tier` for the
   *  execution phase (every approval mode except `plan`). */
  act_tier?: string | null
}

/** P1-3: `sandbox.mode` payload. Engine vocabulary: off | local | landlock. */
export interface SandboxConfig {
  mode?: 'off' | 'local' | 'landlock' | null
}

/**
 * R3-2 (desktop slice): one row of the Settings → Models "Profiles" list —
 * a named providers.toml v2 `ModelProfile`. `active` mirrors the engine's
 * `active_profile` pointer; a freshly created profile has `provider_count: 0`
 * (the UI asks for confirmation before switching to it).
 */
export interface ProviderProfileSummary {
  name: string
  provider_count: number
  active: boolean
  /** The profile's `active_target.model_id` when set; null for an empty profile. */
  model?: string | null
}

/** R5 (profile rename/delete): result of `delete_provider_profile`. The
 *  fresh list rides along (its `active` marker reflects the engine's
 *  fallback) and `became_active` names the profile that took over when the
 *  deleted one was active. */
export interface DeleteProfileOutcome {
  profiles: ProviderProfileSummary[]
  became_active?: string | null
}

/**
 * R4-3 (desktop slice): one row of the per-provider "API keys" list — the
 * credential store's rotation order for that provider. `index` 0 is the
 * ACTIVE key; `masked_hint` is display-only (`sk-pri…aaaa`-style) and never
 * carries full key material.
 */
export interface ProviderKeySummary {
  index: number
  active: boolean
  masked_hint: string
}

/** P2-5: `offpeak` config payload. Empty/missing `model_override` = disabled. */
export interface OffpeakConfig {
  /** Model id used for routine executions inside their execution window. Empty = disabled. */
  model_override?: string | null
}

/** P1-3: result of `activate_permission_profile`. */
export interface ActiveProfileStatus {
  active: string | null
  approval_mode: string | null
}

export interface SttConfig {
  provider?: string | null
  api_key?: string | null
  base_url?: string | null
  model?: string | null
}

export interface TranscriptionResult {
  text: string
}

/**
 * P0-3 — why the backend refused an attachment path. Mirrors the Rust
 * `RejectedAttachmentReason` (snake_case serde tags). `no_working_dir`
 * comes from the `check_attachment_paths` preflight only; the send path
 * hard-rejects that state with an explicit error instead.
 */
export type RejectedAttachmentReason =
  | 'out_of_working_dir'
  | 'unresolvable'
  | 'too_large'
  | 'no_working_dir'

/** P0-3 — one attachment the send pipeline refused (partial success). */
export interface RejectedAttachment {
  path: string
  reason: RejectedAttachmentReason
}

/** P0-3 — one path's verdict from the `check_attachment_paths` preflight. */
export interface AttachmentPathCheck {
  path: string
  ok: boolean
  reason?: RejectedAttachmentReason
}

export interface SendMessageResponse {
  query_id: string
  /** P0-3 — files that were NOT sent, reported per file instead of dropped. */
  rejected_attachments?: RejectedAttachment[]
}

// --- Session multi-window (P1-1) ---

/** Result of `open_session_window` / entry of `list_session_windows`. */
export interface SessionWindowInfo {
  label: string
  sessionId: string
}

// --- Companion Quick Capture window (Office Wave 3 C3) ---

/** Result of `open_companion_window` (fixed `companion` label). */
export interface CompanionWindowInfo {
  label: string
}

// --- Diff Types ---

export interface FileDiff {
  old_content: string
  new_content: string
  file_name: string
  language: string
  /** B0 P0-3: fetch-time mtime (RFC3339) for the Apply-time conflict check. Optional so test fixtures can omit it. */
  mtime?: string
}

export interface DiffFileInfo {
  path: string
  status: 'modified' | 'added' | 'deleted'
  hunks: DiffHunk[]
}

export interface DiffHunk {
  oldStart: number
  oldLines: number
  newStart: number
  newLines: number
  content: string
}

export interface HunkAction {
  line_start: number
  line_end: number
  action: 'accept' | 'reject'
}

// --- File Index Types (office Wave 2 B9' — reference-style file library) ---

/** How an entry entered the index — from the composer's attach flow or an
 *  engine-generated file card. Wire format is a plain string so the Rust
 *  side can extend it without a frontend migration. */
export type FileIndexSource = 'attachment' | 'generated'

/// One row of `list_file_index` — every file the user has ever attached or
/// the agent produced, newest first. `size_bytes` is null when the file has
/// since vanished; `registered_at` is RFC3339.
export interface FileIndexEntry {
  path: string
  name: string
  size_bytes: number | null
  registered_at: string
  favorite: boolean
  source: string
}

// --- MCP Types ---

export interface McpServerConfig {
  name: string
  command: string
  args: string[]
  env: Record<string, string>
  enabled: boolean
}

export interface McpServerInfo {
  name: string
  command: string
  enabled: boolean
  connected: boolean
  tool_count: number
  tools: ToolInfo[]
  last_connected: string | null
}

// --- Skill Types ---

export interface SkillInfo {
  name: string
  description: string
  trigger: string
  source: string
  category?: string
}

export interface SkillDetail {
  name: string
  description: string
  trigger: string
  content: string
  parameters: string[]
  source: string
  category?: string
}

// --- Extensions Hub Types (P1) ---

export type AddonKind = 'mcp' | 'skill' | 'agent' | 'data_source' | 'plugin'

export type TrustLevel = 'unknown' | 'community' | 'official' | 'verified'

export interface InstalledAddonSummary {
  id: string
  kind: AddonKind
  name: string
  install_path?: string
  installed_at?: string
  version?: string
  enabled: boolean
}

/// Tagged union mirroring Rust `CatalogSource`. Discriminated via `type`.
export type CatalogSource =
  | { type: 'mcp_registry'; publisher: string }
  | { type: 'featured_vendor' }
  | { type: 'git_hub_repo'; repo: string; ref_?: string | null }
  | { type: 'custom'; url: string }
  | { type: 'native' }

/// One row in the marketplace catalog. Mirrors Rust `CatalogEntry`.
export interface CatalogEntry {
  id: string
  kind: AddonKind
  name: string
  description: string
  author?: string | null
  version?: string | null
  homepage_url?: string | null
  license?: string | null
  stars?: number | null
  last_updated?: string | null
  source: CatalogSource
  trust: TrustLevel
  metadata?: Record<string, unknown>
  tags?: string[]
}

/// Data source fetcher result — normalized shape across all sources.
export interface DataSourceResult {
  items: DataSourceItem[]
  total: number
  has_more: boolean
}

/// Single item from a data source query.
export interface DataSourceItem {
  id: string
  title: string
  body?: string | null
  url?: string | null
  kind: string
  updated_at?: string | null
}

// --- Task Types ---

export interface TaskItem {
  id: string
  title: string
  status: string
  assignee?: string
  priority?: string
  description?: string
  progress?: number
  /** IDs of tasks this task waits on. Backend JSON key: blockedBy. */
  blocked_by?: string[]
  /** IDs of tasks waiting on this task. Backend JSON key: blocks. */
  blocks?: string[]
  /** Optional due date as unix seconds. */
  due_date?: number | null
  /** Active-form label for in-progress status. */
  active_form?: string
  /** 'serial' (default) or 'parallel'. Controls scheduling of `blocks`. */
  execution_mode?: 'serial' | 'parallel' | null
  /** Team / session subdir name the task file lives in. */
  team?: string | null
}

/// Payload for `update_task`. All fields optional except `id`.
export interface UpdateTaskPayload {
  id: string
  status?: string
  assignee?: string
  priority?: string
  due_date?: number | null
  execution_mode?: 'serial' | 'parallel'
}

// --- OPC analytics ---

export interface OpcDayBucket {
  date: string
  created: number
  completed: number
}

export interface OpcStatusBucket {
  status: string
  count: number
}

export interface OpcAssigneeBucket {
  assignee: string
  total: number
  done: number
  in_progress: number
}

export interface OpcPriorityBucket {
  priority: string
  count: number
}

export interface OpcMetrics {
  total: number
  completion_rate: number
  by_status: OpcStatusBucket[]
  by_priority: OpcPriorityBucket[]
  by_assignee: OpcAssigneeBucket[]
  daily: OpcDayBucket[]
}

export interface BackgroundTaskInfo {
  task_id: string
  prompt: string
  status: string
  started_at: number
  completed_at: number | null
  output: string
}

export interface BackgroundTaskUpdate {
  task_id: string
  status: string
  prompt: string
  output: string
  started_at: number
  completed_at: number | null
}

// --- File Types ---

export interface FileNode {
  name: string
  path: string
  type: 'file' | 'directory'
  children?: FileNode[]
  modified?: boolean
  size?: number
}

export interface WorkingDirInfo {
  root: string
  branch: string
  modified_files: string[]
  status: 'clean' | 'dirty' | 'merge-conflict'
}

export interface AgentInfo {
  id: string
  name: string
  model: string
  status: string
  task?: string
  progress?: number
  tools_used?: number
  duration?: number
  worktree_path?: string
  session_id?: string
}

// --- P0-4 Cost Observability Types ---
//
// Field names mirror the Rust DTOs in shannon-desktop/src/cost_commands.rs
// exactly (serde camelCase on the wire).

/** One session's aggregated usage for the Usage page's per-session view. */
export interface SessionUsageRow {
  sessionId: string
  /** Session title when the sidecar has one; UI falls back to a short id. */
  title: string | null
  inputTokens: number
  outputTokens: number
  cacheCreationTokens: number
  cacheReadTokens: number
  costUsd: number
  requests: number
  /** Epoch ms of the session's most recent ledger event. */
  lastUsedAtMs: number
}

/** One category row of the context breakdown (`key` is a stable string). */
export interface ContextBreakdownCategory {
  key: 'system' | 'tools' | 'skills' | 'memory' | 'mcp' | 'conversation'
  tokens: number
}

/** Six-category context estimate (frozen wire shape, camelCase). */
export interface ContextBreakdown {
  totalTokens: number
  /** `null` when the model's window is genuinely unknown. */
  contextWindow: number | null
  categories: ContextBreakdownCategory[]
}

/** Payload of the `budget:warning` / `budget:exceeded` events (frozen). */
export interface BudgetStatusPayload {
  sessionId: string
  spentUsd: number
  budgetUsd: number
}

// --- X7 Extension Stats Types ---
//
// Field names mirror the Rust DTOs in shannon-desktop/src/cost_commands.rs
// exactly (serde camelCase on the wire).

/** One tool's invocation stats within the stats window. */
export interface ExtensionToolStatRow {
  name: string
  calls: number
  totalTokens: number
}

/** Per-server MCP rollup: server totals plus the per-tool detail. */
export interface ExtensionMcpServerStats {
  server: string
  calls: number
  totalTokens: number
  tools: ExtensionToolStatRow[]
}

/** Per-extension stats bucketed by engine tool name (skills / MCP / other). */
export interface ExtensionStats {
  days: number
  /** Skill ids with the `skill_` prefix stripped. */
  skills: ExtensionToolStatRow[]
  mcpServers: ExtensionMcpServerStats[]
  /** Non-extension tools keep the raw engine tool name. */
  other: ExtensionToolStatRow[]
}

// --- Usage Stats Types ---
//
// Field names mirror the Rust DTOs in shannon-desktop/src/commands_usage.rs
// (UsageStats, BucketTotals) exactly — serde serializes them verbatim.

export interface UsageBucket {
  label: string
  input_tokens: number
  output_tokens: number
  cache_creation_tokens: number
  cache_read_tokens: number
  cost_usd: number
  requests: number
}

export interface UsageStats {
  days: number
  totals: UsageBucket
  by_model: UsageBucket[]
  by_provider: UsageBucket[]
  by_day: UsageBucket[]
}

// --- Scheduled Tasks (Sprint 2) ---
//
// Field names mirror Rust structs in shannon-desktop/src/scheduled_commands.rs
// and shannon-core/src/scheduled_routines.rs exactly. The frontend passes
// these structs through verbatim — do NOT rename to "ScheduledTask".

/// Trigger type for scheduled routines (lowercase wire format).
export type TriggerType = 'interval' | 'cron' | 'webhook' | 'event'

/// Execution policy for scheduled tasks.
export interface ExecutionPolicy {
  max_retries: number
  timeout_secs: number
  worktree?: string | null
  notify_on_failure: boolean
  budget_usd?: number | null
  auto_archive_when_empty: boolean
  /// P2.3: Result routing channels. Each entry is a target spec like
  /// "slack:#ops", "email:ops@example.com", "notification", "log".
  /// Empty array = log only (default behavior).
  result_routing?: string[]
  /// P2-5: off-peak execution window (frozen contract
  /// `ExecutionPolicy.execution_window`). Hours are inclusive wall-clock
  /// hours in `timezone`; cross-midnight windows (start > end) wrap.
  /// null/undefined = execute immediately when due (legacy behavior).
  execution_window?: ExecutionWindow | null
}

/// P2-5: off-peak execution window. The window covers
/// [start_hour:00, (end_hour + 1):00) — both hours inclusive — in
/// `timezone` (IANA name, "UTC", a fixed offset like "+08:00"; null/empty =
/// machine-local timezone). start=0 & end=23 = the full 24-hour window.
export interface ExecutionWindow {
  start_hour: number
  end_hour: number
  timezone?: string | null
}

/// A single scheduled routine (wire-level type, matches Rust `ScheduledRoutine`).
export interface ScheduledRoutine {
  id: string
  name: string
  prompt: string
  interval_secs: number
  trigger_type: TriggerType
  cron_expr?: string | null
  timezone?: string | null
  next_fire_at?: number | null
  expires_at?: number | null
  created_at: number
  last_fired?: number | null
  enabled: boolean
  fire_count: number
  max_fires?: number | null
  policy?: ExecutionPolicy | null
  last_run_id?: string | null
  last_error?: string | null
  /// IDs of routines that must succeed before this one fires.
  depends_on?: string[]
  /// P-E1: project directory the routine belongs to (persisted as the task's
  /// `working_dir` sidecar, flattened onto this shape by the desktop
  /// `RoutineDto`). null/undefined = no project.
  working_dir?: string | null
  /// office B6' routing: when true the run-finished notification is also
  /// delivered to the configured webhook (Settings → Notifications).
  /// Absent = false (no webhook copy).
  notify_webhook?: boolean
}

/// Payload for `create_scheduled_task`.
export interface CreateTaskPayload {
  name: string
  prompt: string
  trigger_type?: TriggerType
  interval_secs?: number
  cron_expr?: string
  timezone?: string
  expires_at?: number
  max_fires?: number
  policy?: ExecutionPolicy
  /// P-E1: project directory; stored as the routine's working_dir sidecar.
  working_dir?: string | null
  /// office B6' routing: deliver the run-finished notification to the
  /// configured webhook as well. Default false.
  notify_webhook?: boolean
}

/// Payload for `update_scheduled_task`. All fields optional except `id`.
export interface UpdateTaskPayload {
  id: string
  name?: string
  prompt?: string
  trigger_type?: TriggerType
  interval_secs?: number
  cron_expr?: string
  timezone?: string
  enabled?: boolean
  expires_at?: number
  max_fires?: number
  policy?: ExecutionPolicy
  /// office B6' routing — same field as the create payload; omitted leaves
  /// the routine's current setting unchanged.
  notify_webhook?: boolean
  /// Replaces dependency list. Send the full list (add or remove); empty clears.
  depends_on?: string[]
  /// P-E1: non-empty replaces the routine's project, empty string clears it,
  /// omitted leaves it unchanged.
  working_dir?: string | null
}

/// Result of `preview_cron`.
export interface CronPreview {
  expression: string
  valid: boolean
  error?: string
  next_fires: number[]
}

/// Response from `trigger_task_now`.
export interface TriggerResponse {
  run_id: string
  task_id: string
  task_name: string
}

/// A single triage item needing user attention.
export interface TriageItem {
  id: string
  task_id?: string
  task_name?: string
  run_id?: string
  kind: string
  message: string
  created_at: number
  revision?: number
  read?: boolean
  archived?: boolean
}

/// Filters for `list_triage_items`. All fields optional.
export interface TriageFilter {
  unread_only?: boolean
  unarchived_only?: boolean
  kind?: string
  limit?: number
}

/// Aggregate triage counts for the sidebar badge.
export interface TriageStats {
  total: number
  unread: number
  archived: number
  by_kind: Record<string, number>
}

// --- Inbox (P0-3 SQLite inbox; serde contract is camelCase) ---

/// Where an inbox item came from. `routine`/`scheduled_task` are produced by
/// scheduled-task runs (and are the only rerunnable sources); `goal` and
/// `trigger` come from goal events / the external trigger endpoint; `batch`
/// is the aggregate completion record of a parallel batch run (T3). The T5
/// unified "needs attention" stream adds the session/agent events:
/// `session_approval` (a permission prompt is waiting on the user),
/// `session_failed` (the session's last turn failed), and `skill_candidate`
/// (a detected skill pattern awaits review). `dream_report` is the daily
/// dream-distillation summary card (at most one per day, deduped by the
/// backend writer).
export type InboxSource =
  | 'routine'
  | 'scheduled_task'
  | 'goal'
  | 'trigger'
  | 'batch'
  | 'session_approval'
  | 'session_failed'
  | 'skill_candidate'
  | 'dream_report'

/// Lifecycle status of an inbox item (`pending` → `read` → `archived`).
export type InboxItemStatus = 'pending' | 'read' | 'archived'

/// A single inbox row as returned by `list_inbox_items`.
export interface InboxItem {
  id: number
  source: InboxSource
  sourceId: string | null
  sessionId: string | null
  title: string
  summary: string
  error: string | null
  status: InboxItemStatus
  createdAtMs: number
  updatedAtMs: number
}

/// Optional filters for `list_inbox_items`. All fields optional.
export interface InboxListFilter {
  status?: InboxItemStatus
  source?: InboxSource
  limit?: number
}

/// Badge counts from `get_inbox_stats`.
export interface InboxStats {
  pending: number
  today: number
}

/// Lightweight execution record for the history list.
export interface TaskExecution {
  run_id: string
  task_id: string
  task_name: string
  started_at: number
  finished_at?: number
  status: string
  error_message?: string
  cost_usd?: number
  token_usage?: number
}

/// Full execution detail view (history list item + task metadata).
/// `execution` is flattened by Rust serde, so spread its fields inline.
export interface TaskExecutionDetail extends TaskExecution {
  prompt?: string
  cron_expr?: string
  next_fire_at?: number
}

/// Triggered routine row for the routines panel.
export interface TriggeredRoutineDto {
  name: string
  trigger: string
  matcher?: string
  pattern?: string
  command: string
  enabled: boolean
  description?: string
}

/// Hook event catalog entry. Mirrors `automation_commands::HookEventInfo`.
export interface HookEventInfo {
  name: string
  category: string
  description: string
  payload_fields: string[]
}

/// Built-in permission profile summary (Strict / Balanced / Permissive).
export interface BuiltinProfileInfo {
  id: string
  description: string
  auto_approve_read: boolean
  auto_approve_write: boolean
  auto_approve_bash: boolean
  auto_approve_delete: boolean
  auto_approve_network: boolean
  deny_destructive: string[]
}

/// User-defined custom profile row.
export interface CustomProfileInfo {
  name: string
  description: string
  auto_approve: string[]
  confirm: string[]
  deny: string[]
  source_path?: string
}

/// Response from `list_permission_profiles`.
export interface ProfilesList {
  builtin: BuiltinProfileInfo[]
  custom: CustomProfileInfo[]
}

/// DTO mirroring Rust `TaskWorktreeDto`.
export interface TaskWorktreeDto {
  task_id: string
  task_name: string
  path: string
  branch: string
}

// --- Enums ---

export type ViewMode = 'verbose' | 'normal' | 'summary'

export type ApprovalMode =
  | 'suggest'
  | 'plan'
  | 'auto'
  | 'auto_edit'
  | 'full_auto'
  | 'readonly'
  | 'plan_ro'
  | 'bypass_permissions'
  | 'dont_ask'
  | 'confirm'

// --- Goal runs (P0-2 desktop goal runner; serde contract is camelCase) ---

/// Lifecycle of a desktop goal run. `interrupted` marks a sidecar goal the
/// app restarted under (resumable); `paused` covers max/budget/anti-spin
/// pauses and engine failures.
export type GoalRunStatus =
  | 'running'
  | 'paused'
  | 'completed'
  | 'blocked'
  | 'stopped'
  | 'interrupted'

/// One goal run as rendered by the Tasks-page run card (payload of
/// `goal:updated`).
export interface GoalRunDto {
  sessionId: string
  title: string
  objective: string
  status: GoalRunStatus
  iterations: number
  maxTurns: number | null
  spentUsd: number
  budgetUsd: number | null
  stallStrikes: number
  lastError: string | null
  startedAtMs: number
  updatedAtMs: number
  /// P-E2: project directory inherited from the originating session.
  /// null on interrupted (restart-reconciled) cards — the sidecar cannot
  /// round-trip a working dir.
  workingDir: string | null
}

// --- Batch runs (P1-2 desktop best-of-N; serde contract is camelCase) ---

/// Lifecycle of one batch branch. Terminal: completed | failed.
export type BatchBranchStatus = 'running' | 'completed' | 'failed'

/** Lifecycle of a best-of-N batch run. `partially_failed` = mixed terminals;
/// `adopted`/`discarded` are the user-driven final states. */
export type BatchRunStatus =
  | 'running'
  | 'completed'
  | 'failed'
  | 'partially_failed'
  | 'adopted'
  | 'discarded'

/// Per-branch diff stat block.
export interface BatchDiffSummary {
  filesChanged: number
  additions: number
  deletions: number
}

/// One parallel candidate branch (frozen backend contract).
export interface BatchBranch {
  index: number
  branchName: string
  worktreePath: string
  status: BatchBranchStatus
  error: string | null
  summary: BatchDiffSummary | null
  spentUsd: number
}

/// One best-of-N batch run (payload of `batch:updated`). `adoptedIndex` is
/// additive: set once the batch is adopted.
export interface BatchRunDto {
  batchId: string
  title: string
  prompt: string
  count: number
  status: BatchRunStatus
  createdAtMs: number
  branches: BatchBranch[]
  adoptedIndex: number | null
}

// --- Event Names ---

export const EVENT_NAMES = {
  QUERY_TEXT: 'query:text',
  QUERY_TOOL_START: 'query:tool-start',
  QUERY_TOOL_RESULT: 'query:tool-result',
  QUERY_TOOL_PROGRESS: 'query:tool-progress',
  QUERY_THINKING: 'query:thinking',
  QUERY_USAGE: 'query:usage',
  /**
   * R5-2: the engine failed over to a fallback model/provider (R3-1) or
   * rotated the provider's API key (R4-3) and the request CONTINUED.
   * Payload: QueryNoticeEvent { query_id, kind, message, session_id? } —
   * rendered as a subtle system line in the conversation, never an error
   * banner.
   */
  QUERY_NOTICE: 'query:notice',
  QUERY_COMPLETED: 'query:completed',
  QUERY_FAILED: 'query:failed',
  QUERY_CANCELLED: 'query:cancelled',
  PERMISSION_REQUEST: 'permission-request',
  SESSIONS_UPDATED: 'sessions-updated',
  /** 卡A: switch_session auto-unarchived an archived session (toast cue). */
  SESSION_AUTO_UNARCHIVED: 'session-auto-unarchived',
  SESSION_LOADED: 'session-loaded',
  CONFIG_UPDATED: 'config-updated',
  DIFF_REVIEW_AVAILABLE: 'diff-review-available',
  BACKGROUND_TASK_UPDATE: 'background-task-update',
  BACKGROUND_TASKS_UPDATED: 'background-tasks-updated',
  AGENT_MESSAGES_UPDATED: 'agent-messages-updated',
  TRIAGE_UPDATED: 'triage-updated',
  INBOX_UPDATED: 'inbox-updated',
  GOAL_UPDATED: 'goal:updated',
  /** P1-2: a best-of-N batch run changed (payload: BatchRunDto). */
  BATCH_UPDATED: 'batch:updated',
  /** P0-4: session spend crossed 80% of its budget (yellow advisory bar). */
  BUDGET_WARNING: 'budget:warning',
  /** P0-4: budget cap hit — send rejected pre-turn or turn cancelled. */
  BUDGET_EXCEEDED: 'budget:exceeded',
  /** B2: the agent-teams registry accepted a new sub-agent (desktop bridge). */
  SUBAGENT_START: 'subagent:start',
  /** B2: a sub-agent run finished (ok or failed; desktop bridge). */
  SUBAGENT_STOP: 'subagent:stop',
  /** P1-5 D: PTY output for the integrated terminal (data is base64). */
  TERMINAL_OUTPUT: 'terminal:output',
  /**
   * P3-6: the terminal's process exited (backend emission lands with the
   * Task-4 pump change). Authoritative exit signal — the in-stream
   * "[shannon: process exited …" notice is display text only and must not
   * be parsed.
   */
  TERMINAL_EXIT: 'terminal:exit',
} as const

export type EventName = (typeof EVENT_NAMES)[keyof typeof EVENT_NAMES]

// --- Integrated terminal (P1-5 D, frozen backend contract) ---

/** One live PTY session (`terminal_list` item / event correlation key). */
export interface TerminalInfo {
  terminalId: string
  projectDir: string
  /**
   * Additive (review fix): the project dir EXACTLY as the spawn request
   * carried it, before the backend canonicalized `projectDir`. The
   * per-project tab filter matches this first — canonical-vs-raw
   * mismatches (symlinked segments on Unix, `\\?\C:\…` verbatim prefixes
   * on Windows) used to make a freshly spawned tab vanish into the empty
   * state. Absent/null on legacy payloads: fall back to `projectDir`.
   */
  projectDirRaw?: string | null
  shell: string
  startedAtMs: number
}

/** `terminal:output` payload — `data` is the raw pty bytes, base64. */
export interface TerminalOutputPayload {
  terminalId: string
  data: string
  /**
   * Additive (review fix): per-session monotonic chunk number assigned by
   * the backend pump in stream order. Replay stitching drops queued
   * events with `seq <= terminal_history.endSeq` (already replayed) and
   * flushes the rest — no loss, no duplication around (re)connect.
   * Absent on legacy/demo payloads: the consumer falls back to
   * flush-everything.
   */
  seq?: number
}

/** `terminal:exit` payload — the terminal's process has exited. */
export interface TerminalExitPayload {
  terminalId: string
}

/**
 * P3-1: persisted terminal preferences (`[terminal]` in
 * `~/.shannon/config.toml`; camelCase over the wire, frozen shape).
 * The backend clamps `fontSize` (8–32), `scrollback` (0–100000) and
 * `drawerHeight` (120–1200) and blanks the shell on read AND write —
 * after a set, render the values the response carries, not the ones the
 * caller sent.
 */
export interface TerminalSettings {
  shell: string | null
  fontSize: number
  scrollback: number
  drawerHeight: number
  screenReaderMode: boolean
}

// --- Inter-agent message history (Phase D C3) ---

export interface AgentMessageEntry {
  message_id: string
  team: string
  from: string
  to: string
  content_preview: string
  content_kind: 'text' | 'structured' | 'protocol'
  priority: 'low' | 'normal' | 'high' | 'critical'
  timestamp: number
}

// --- Skill Loop (E2) ---

export type ProposalStatus = 'Pending' | 'Approved' | 'Rejected'
export type TaskOutcome = 'Success' | 'Failure' | 'Partial'

export interface TaskEvaluation {
  duration_secs: number
  tool_call_count: number
  user_prompt: string
  outcome: TaskOutcome
  tool_names_used: string[]
}

export interface EvaluationResult {
  suggest: boolean
  reason: string
  confidence: number
}

export interface SkillProposal {
  id: string
  name: string
  slug: string
  description: string
  trigger_patterns: string[]
  example_workflow: string
  source_task_id: string | null
  created_at: string
  status: ProposalStatus
}

export interface SkillProposalCountPayload {
  pending_count: number
}

// ---------------------------------------------------------------------------
// Turn Timeline (§4.14) — projection of a session's L0 event log
// ---------------------------------------------------------------------------

/** One tool execution inside a timeline turn (waterfall row). */
export interface TimelineToolEntry {
  tool_use_id: string
  tool_name: string
  start_ts_ns: number
  end_ts_ns: number
  duration_ms?: number | null
  is_error: boolean
}

/** One user-visible turn with its tools and usage. */
export interface TimelineTurn {
  turn: number
  start_ts_ns: number
  end_ts_ns: number
  reason?: string | null
  error?: string | null
  input_tokens: number
  output_tokens: number
  cache_creation_tokens: number
  cache_read_tokens: number
  cost_usd?: number | null
  tools: TimelineToolEntry[]
}

/** One cumulative sample on the token/cost curve (at each turn/end). */
export interface TimelineCumulativePoint {
  ts_ns: number
  output_tokens_total: number
  cost_total_usd?: number | null
}

/** The whole Turn Timeline for one session (`trace_timeline` command). */
export interface TurnTimeline {
  session_id: string
  model?: string | null
  started_ts_ns: number
  ended_ts_ns: number
  turns: TimelineTurn[]
  cumulative: TimelineCumulativePoint[]
}

// ── Remote targets (SSH hosts / Docker containers) ──────────────────────

/** An SSH host candidate discovered from `~/.ssh/config` (read-only). */
export interface SshHostCandidate {
  alias: string
  user: string | null
  hostname: string | null
  port: number | null
}

/** A running Docker container from `docker ps`. */
export interface ContainerInfo {
  id: string
  names: string
  image: string
  status: string
}

/** A saved remote execution target (`~/.shannon/remotes.toml`). */
export interface RemoteTargetListItem {
  name: string
  kind: 'ssh' | 'docker'
  host: string | null
  port: number | null
  user: string | null
  container: string | null
  shell: string | null
  sshTarget: string | null
  workspaceDir: string
}

/** Result of a connectivity probe (`remote_test_target`). */
export interface RemoteHealth {
  ok: boolean
  platform: string
  home: string
  bashAvailable: boolean
  workspaceExists: boolean
  latencyMs: number
  error: string | null
}

/** A registered project (P-E3 project registry, `~/.shannon/projects.db`).
 *  `path` is the unique key (canonical working dir). `name`/`icon`/`color`
 *  are curation layers — `null` means the UI renders the default (the
 *  path's tail segment). Wire shape mirrors the Rust `ProjectRecord`
 *  (camelCase serde). */
export interface ProjectRecord {
  path: string
  name: string | null
  icon: string | null
  color: string | null
  archivedAtMs: number | null
  createdAtMs: number
}

/** X5 trust preview — wire shape of `inspect_plugin_source`'s
 *  `PluginBundleSummary`. Everything a plugin bundle will enable, read
 *  from its manifest + directories BEFORE the user confirms an install. */
export interface PluginBundleSummary {
  name: string
  source_format: 'shannon-toml' | 'claude-json' | 'unknown'
  skills: string[]
  agents: string[]
  commands: string[]
  mcp_servers: string[]
}
