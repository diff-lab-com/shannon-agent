import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type {
  ChatMessage,
  StatusResponse,
  ModelInfo,
  ToolInfo,
  ConfigUpdate,
  ProviderConnection,
  ProvidersFile,
  ProviderInput,
  ProviderProfileSummary,
  DeleteProfileOutcome,
  ProviderKeySummary,
  DesktopConfig,
  GatewayConfig,
  GatewayPairingRequest,
  GatewayProcessState,
  SurfaceInfo,
  CliInstallStatus,
  AppUpdateInfo,
  CliInstallResult,
  MobileDeviceEntry,
  MobilePairToken,
  ContainerInfo,
  SessionWindowInfo,
  CompanionWindowInfo,
  RemoteHealth,
  RemoteTargetListItem,
  SshHostCandidate,
  SttConfig,
  TranscriptionResult,
  SendMessageResponse,
  AttachmentPathCheck,
  HunkAction,
  SessionInfo,
  SessionPlan,
  ArchivedSessionRow,
  TurnTimeline,
  McpServerInfo,
  McpServerConfig,
  SkillInfo,
  SkillDetail,
  InstalledAddonSummary,
  TaskItem,
  BackgroundTaskInfo,
  AgentInfo,
  FileDiff,
  FileNode,
  TerminalInfo,
  TerminalSettings,
  WorkingDirInfo,
  CatalogEntry,
  PluginBundleSummary,
  DataSourceResult,
  MobileTlsStatus,
  ProjectRecord,
  ProviderStatus,
  FileIndexEntry,
} from '@/types'
import type {
  ScheduledRoutine,
  CreateTaskPayload,
  UpdateTaskPayload,
  CronPreview,
  TriageItem,
  TriageFilter,
  TriageStats,
  GoalRunDto,
  BatchRunDto,
  InboxItem,
  InboxListFilter,
  InboxItemStatus,
  InboxStats,
  TaskExecution,
  TaskExecutionDetail,
  AgentRunRow,
  TriggeredRoutineDto,
  TriggerResponse,
  TaskWorktreeDto,
  AgentMessageEntry,
  UsageStats,
  UsageGovernance,
  TaskCostEstimate,
  SessionUsageRow,
  ContextBreakdown,
  ExtensionStats,
  HookEventInfo,
  ProfilesList,
  ActiveProfileStatus,
  CustomProfileInfo,
  OpcMetrics,
  TaskEvaluation,
  EvaluationResult,
  SkillProposal,
  SubAgentDto,
} from '@/types'

export async function readAttachment(path: string): Promise<AttachmentPayload> {
  return invoke('read_attachment', { path })
}

export async function readAttachments(paths: string[]): Promise<AttachmentPayload[]> {
  return invoke('read_attachments', { paths })
}

export const MAX_ATTACHMENT_COUNT = 10

export interface AttachmentPayload {
  mime: string
  base64?: string
  text?: string
  name: string
  size: number
}


export async function sendMessage(
  message: string,
  filePaths?: string[],
  budgetBypass?: boolean,
  // P1-1 fix: explicit session routing (multi-window). Undefined keeps the
  // backend's legacy active-session fallback.
  sessionId?: string,
): Promise<SendMessageResponse> {
  return invoke('send_message', {
    message,
    filePaths: filePaths ?? null,
    budgetBypass: budgetBypass ?? null,
    sessionId: sessionId ?? null,
  })
}

/**
 * P0-3 preflight — classify attachment paths the way `send_message` will,
 * at attach time, so the composer can flag bad chips before the user hits
 * send. Always resolves (one entry per path); a rejection here is treated
 * as "no marking", never as an error toast.
 */
export async function checkAttachmentPaths(paths: string[]): Promise<AttachmentPathCheck[]> {
  return invoke('check_attachment_paths', { paths })
}

/**
 * G3b P1-6 — persist a clipboard image (base64, no data-URL prefix) to
 * `~/.shannon/cache/pasted/<timestamp>-<rand>.<ext>` and return its absolute
 * path, so a pasted image can ride the regular attachment pipeline. The
 * backend validates the 10 MiB cap and magic-bytes-vs-extension match.
 */
export async function savePastedImage(dataBase64: string, ext: string): Promise<string> {
  return invoke('save_pasted_image', { dataBase64, ext })
}

export async function getConversation(): Promise<ChatMessage[]> {
  return invoke('get_conversation')
}

/**
 * A-6 fix — the id of the backend's ACTIVE session (the one
 * `get_conversation` answers for), or null before any session exists.
 * Read-only: unlike get_conversation this never materializes a session.
 * The main window's cold start calls it AFTER get_conversation to bind
 * `currentSessionId` to the conversation it just rendered.
 */
export async function getActiveSessionId(): Promise<string | null> {
  return invoke<string | null>('get_active_session_id')
}

export async function cancelQuery(sessionId?: string): Promise<void> {
  await invoke('cancel_query', { sessionId: sessionId ?? null })
}

/**
 * B1-4 (P1-3) — whether the backend still has a live query on THIS session.
 * The stop button's settle watchdog reconciles against this when the
 * `query:cancelled` terminal event never arrives (backend emits are
 * fire-and-forget). Read-only: an unknown session reports idle and the
 * backend never materializes a registry entry for it.
 */
export async function getSessionQuerying(sessionId: string): Promise<boolean> {
  return invoke<boolean>('get_session_querying', { sessionId })
}

// --- Webview file drag-drop (Tauri v2) ---
//
// B0 P0-2: with the webview's `dragDropEnabled` (default on), HTML5
// dragover/drop events never reach the page and `File.path` — the Tauri v1
// injection the composer used to read — no longer exists, so the old drop
// handler silently produced zero paths. The only live signal is the
// webview's own onDragDropEvent, so the composer consumes it through this
// normalized wrapper. Note the @tauri-apps/api DragDropEvent union gives
// `over` a position only — paths ride on `enter` and `drop`.

export type WebviewFileDropEvent =
  | { type: 'enter'; paths: string[] }
  | { type: 'over' }
  | { type: 'drop'; paths: string[] }
  | { type: 'leave' }

export async function onWebviewFileDrop(
  handler: (event: WebviewFileDropEvent) => void,
): Promise<() => void> {
  const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload
    if (p.type === 'enter' || p.type === 'drop') handler({ type: p.type, paths: p.paths })
    else if (p.type === 'over') handler({ type: 'over' })
    else handler({ type: 'leave' })
  })
  return unlisten
}

// --- Config ---

export async function getConfig(): Promise<DesktopConfig> {
  return invoke('get_config')
}

export async function configure(update: ConfigUpdate): Promise<void> {
  await invoke('configure', { update })
}

// --- Gateway social connections (T5) — OS keyring + gateway config.json ---

/** Store a credential in the OS keyring under `<service>/<account>`. */
export async function gatewaySetSecret(key: string, value: string): Promise<void> {
  await invoke('gateway_set_secret', { key, value })
}

/** Fetch a credential, or null if no keyring entry exists. */
export async function gatewayGetSecret(key: string): Promise<string | null> {
  return invoke('gateway_get_secret', { key })
}

/** Whether a keyring entry exists for `key` (cheaper than fetching the value). */
export async function gatewayHasSecret(key: string): Promise<boolean> {
  return invoke('gateway_has_secret', { key })
}

/** Delete a keyring entry (idempotent — missing entry is success). */
export async function gatewayDeleteSecret(key: string): Promise<void> {
  await invoke('gateway_delete_secret', { key })
}

/** Read the gateway config; returns a loopback default on first run. */
export async function gatewayReadConfig(): Promise<GatewayConfig> {
  return invoke('gateway_read_config')
}

/** Validate + atomically persist the gateway config; returns what was written. */
export async function gatewayWriteConfig(config: GatewayConfig): Promise<GatewayConfig> {
  return invoke('gateway_write_config', { config })
}

// --- E-1 方案 C — supervised gateway process lifecycle ---

/** Spawn (or no-op if already running) the local gateway under supervision. */
export async function gatewaySupervisorStart(): Promise<GatewayProcessState> {
  return invoke('gateway_supervisor_start')
}

/** Gracefully stop the supervised gateway (idempotent). */
export async function gatewaySupervisorStop(): Promise<GatewayProcessState> {
  return invoke('gateway_supervisor_stop')
}

/** Snapshot of `managed` + the process status. */
export async function gatewaySupervisorStatus(): Promise<GatewayProcessState> {
  return invoke('gateway_supervisor_status')
}

/**
 * Persist the 方案 C `managed` flag. Toggling it off also stops a running
 * gateway. Toggling it on does NOT auto-start — the user clicks Start, or the
 * next app launch auto-starts via setup().
 */
export async function gatewaySetManaged(managed: boolean): Promise<GatewayProcessState> {
  return invoke('gateway_set_managed', { managed })
}

// --- P1.3 mobile device pairing (Design D shared-file channel) ---

/** Mint a one-time 75s pair token + QR (LAN endpoint + token) for the phone. */
// ── ADR-0011 Phase B B3/B7 — surface identity + bundled CLI install ────────

export async function getSurfaceInfo(): Promise<SurfaceInfo> {
  return invoke('get_surface_info')
}

export async function getCliInstallStatus(): Promise<CliInstallStatus> {
  return invoke('get_cli_install_status')
}

export async function installCliToPath(): Promise<CliInstallResult> {
  return invoke('install_cli_to_path')
}

// ── C1① — semi-automatic update check ────────────────────────────────

export async function checkAppUpdate(): Promise<AppUpdateInfo> {
  return invoke('check_app_update')
}

export async function openReleasePage(url: string): Promise<void> {
  return invoke('open_release_page', { url })
}

// ── Settings R3 — About section: read-only data directory ────────────

/** Absolute path of the Shannon data directory ($SHANNON_HOME or ~/.shannon). */
export async function getShannonHome(): Promise<string> {
  return invoke('get_shannon_home')
}

// ── Settings R3 T3 — hardware acceleration + prevent sleep ───────────

/** Result of `get_power_capabilities` (serde camelCase). */
export interface PowerCapabilities {
  /** `std::env::consts::OS`: 'macos' | 'windows' | 'linux' | … */
  platform: string
  /** Whether the prevent-sleep backend is usable on this machine. */
  keepAwakeSupported: boolean
}

/**
 * Platform + keep-awake capability probe. The General settings' System
 * cards use `platform` to hide the hardware-acceleration card on macOS and
 * `keepAwakeSupported` to disable the prevent-sleep switches where no
 * backend exists (Linux without systemd-inhibit).
 */
export async function getPowerCapabilities(): Promise<PowerCapabilities> {
  return invoke('get_power_capabilities')
}

// ── Batch-3 follow-up — export diagnostics bundle ────────────────────

/** Summary of a written diagnostics zip (logs + crash reports + doctor). */
export interface ExportDiagnosticsResult {
  path: string
  log_files: number
  log_bytes: number
  doctor_ok: boolean
  truncated: boolean
}

/**
 * Bundle local logs, crash reports and a fresh `shannon doctor --json --deep`
 * report into the zip at `dest` (an absolute path from the save dialog).
 * Sessions/provider config/credentials are never included.
 */
export async function exportDiagnostics(dest: string): Promise<ExportDiagnosticsResult> {
  return invoke('export_diagnostics', { dest })
}


export async function mobileGeneratePairToken(): Promise<MobilePairToken> {
  return invoke('mobile_generate_pair_token')
}

/** List currently paired devices (read-only; the gateway writes the file). */
export async function mobileListPairedDevices(): Promise<MobileDeviceEntry[]> {
  return invoke('mobile_list_paired_devices')
}

/** Remove a paired device by id; returns true if a device was removed. */
export async function mobileRevokeDevice(deviceId: string): Promise<boolean> {
  return invoke('mobile_revoke_device', { deviceId })
}

/** Current `mobile.tls` state + cert fingerprint (v0.12 LAN hardening). */
/** Current `mobile.tls` state + cert fingerprint (v0.12 LAN hardening). */
export async function mobileTlsStatus(): Promise<MobileTlsStatus> {
  return invoke('mobile_tls_status')
}

// --- T9 — gateway IM pairing approval (the desktop entry for review F42) ---

/** Pending IM pairing requests on the running gateway (mints a pair token). */
export async function gatewayPairingPending(): Promise<GatewayPairingRequest[]> {
  return invoke('gateway_pairing_pending')
}

/** Approve one pending pairing by code; returns the approved request. */
export async function gatewayPairingApprove(code: string): Promise<GatewayPairingRequest> {
  return invoke('gateway_pairing_approve', { code })
}

export interface WebhookConfigDto {
  url: string
  template: string
  secret: string | null
  timeout_ms: number
  include_body: boolean
}

export async function getWebhookConfig(): Promise<WebhookConfigDto | null> {
  return invoke('get_webhook_config')
}

export async function saveWebhookConfig(dto: WebhookConfigDto): Promise<void> {
  await invoke('save_webhook_config', { dto })
}

export async function clearWebhookConfig(): Promise<void> {
  await invoke('clear_webhook_config')
}

export type NotificationLevel = 'info' | 'warning' | 'error' | 'success'

export interface NotificationPayload {
  title: string
  body: string
  level?: NotificationLevel
}

export async function sendNotification(payload: NotificationPayload): Promise<void> {
  await invoke('send_notification', { payload })
}

/** Desktop-notification preferences — master enable, quiet-hours (DND) window,
 *  and per-event-type toggles (completions vs failures vs needs-attention)
 *  plus the frontend task-chime opt-in. */
export interface NotificationPrefs {
  master_enabled: boolean
  dnd_enabled: boolean
  /** `"HH:MM"` (24h, system-local) or null. */
  dnd_start: string | null
  dnd_end: string | null
  /** Surface OS notifications for non-error events (query/task completion). */
  on_completed: boolean
  /** Surface OS notifications for error events (query/task failure). */
  on_failed: boolean
  /** Surface OS notifications for attention requests (approval waits, budget
   *  alerts). Backend defaults this to true for older payloads. */
  on_needs_attention: boolean
  /** Play the frontend-composited task chime (Web Audio) on completed /
   *  failed / needs-attention events. Independent of the OS notification
   *  sound. Defaults to false. */
  sound_enabled: boolean
}

export async function getNotificationPrefs(): Promise<NotificationPrefs> {
  return invoke('get_notification_prefs')
}

export async function setNotificationPrefs(prefs: NotificationPrefs): Promise<void> {
  await invoke('set_notification_prefs', { prefs })
}

export interface DetectedProvider {
  provider: string
  has_api_key: boolean
}

export async function detectProviderFromEnv(): Promise<DetectedProvider | null> {
  return invoke('detect_provider_from_env')
}

export type TestConnectionResult =
  | { kind: 'success' }
  | { kind: 'invalid_key' }
  | { kind: 'rate_limited' }
  // R2-P1-10: HTTP 402 — account out of credits / over plan quota.
  | { kind: 'quota_exhausted' }
  | { kind: 'provider_error'; status: number }
  | { kind: 'network_unreachable' }
  | { kind: 'unknown'; message: string }

/// Outcome of the settings "send test webhook" button (P1-7). `status` is the
/// HTTP status the receiver answered with (null on transport failure / URL
/// block); `detail` is a human-readable summary line for the failure toast.
export interface WebhookTestResult {
  success: boolean
  status: number | null
  detail: string
}

/// Fire one test payload at the currently configured webhook URL (P1-7).
/// Uses the saved preset template/secret/timeout and a single synchronous
/// POST (no retries) so the UI can show an immediate verdict.
export async function testWebhook(title: string, body: string): Promise<WebhookTestResult> {
  return invoke('test_webhook', { title, body })
}

export async function testProviderConnection(
  provider: string,
  apiKey: string,
  baseUrl?: string,
): Promise<TestConnectionResult> {
  return invoke('test_provider_connection', { provider, apiKey, baseUrl })
}

/// Test raw (unsaved) credentials from inside the Add/Edit Provider modal
/// (review §2-12): `apiKey: null` + a `providerId` tests the STORED key
/// (edit mode — the modal never re-displays the secret). Mirrors
/// `test_provider_connection` internals; never persists anything.
export async function testProviderCredentials(
  kind: string,
  baseUrl: string | null,
  apiKey: string | null,
  providerId: string | null,
): Promise<TestConnectionResult> {
  return invoke('test_provider_credentials', { kind, baseUrl, apiKey, providerId })
}

// --- Fetch model list (review §2-9) ---
//
// `fetch_provider_models` returns `Err(String)` with a stable category
// token prefix for categorizable failures (`invalid_key`, `rate_limited`,
// `provider_error:<status>`, `network_unreachable`, `unsupported_kind:<kind>`,
// `missing_key`, `invalid_base_url:<detail>`); anything else is the raw
// provider message. The parser below turns that into the discriminated
// union the modal renders inline (utils.ts categorization style).

export type FetchModelsFailure =
  | { kind: 'invalid_key' }
  | { kind: 'rate_limited' }
  | { kind: 'network_unreachable' }
  | { kind: 'missing_key' }
  | { kind: 'provider_error'; status: number }
  | { kind: 'unsupported_kind' }
  | { kind: 'invalid_base_url'; detail: string }
  | { kind: 'unknown'; message: string }

export function parseFetchModelsError(message: string): FetchModelsFailure {
  const sep = message.indexOf(':')
  const token = sep === -1 ? message : message.slice(0, sep)
  const rest = sep === -1 ? '' : message.slice(sep + 1)
  switch (token) {
    case 'invalid_key':
      return { kind: 'invalid_key' }
    case 'rate_limited':
      return { kind: 'rate_limited' }
    case 'network_unreachable':
      return { kind: 'network_unreachable' }
    case 'missing_key':
      return { kind: 'missing_key' }
    case 'provider_error': {
      const status = Number(rest)
      return Number.isFinite(status) && status > 0
        ? { kind: 'provider_error', status }
        : { kind: 'provider_error', status: 0 }
    }
    case 'unsupported_kind':
      return { kind: 'unsupported_kind' }
    case 'invalid_base_url':
      return { kind: 'invalid_base_url', detail: rest }
    default:
      return { kind: 'unknown', message }
  }
}

/// Fetch the live model list from a provider endpoint. In-memory only.
/// `providerId` (a saved connection id) lets the backend fall back to the
/// stored credential when `apiKey` is null — the modal never round-trips
/// the existing secret.
export async function fetchProviderModels(
  providerId: string | null,
  kind: string,
  baseUrl: string,
  apiKey: string | null,
): Promise<string[]> {
  return invoke('fetch_provider_models', { providerId, kind, baseUrl, apiKey })
}

/// One row in the response from `testAllProviders`. Mirrors the Rust
/// `ProviderTestRow` shape; the Settings → Models "Test all" UI renders one
/// per managed connection with a status pill.
export interface ProviderTestRow {
  id: string
  label: string
  provider_kind: string
  result: TestConnectionResult
  latency_ms: number | null
}

/// Probe every configured provider connection in parallel (ADR-0005 P4.12).
///
/// Reuses the engine `probe_provider_endpoint` so the per-row verdict is the
/// same shape the single-provider `testProviderConnection` returns. Rows are
/// returned in the same order as `listProviders()` so the UI can join them
/// without an extra index lookup.
export async function testAllProviders(): Promise<ProviderTestRow[]> {
  return invoke('test_all_providers')
}

// --- Managed providers (Models P2) ---

/// List all managed providers (API keys masked). Lazily migrates the legacy
/// singular config into a seeded entry on first call.
export async function listProviders(): Promise<ProvidersFile> {
  return invoke('list_providers')
}

/// Insert or update a managed provider. Returns the updated (masked) file.
export async function saveProvider(input: ProviderInput): Promise<ProvidersFile> {
  return invoke('save_provider', { input })
}

/// Delete a managed provider by id. Returns the updated (masked) file.
export async function deleteProvider(id: string): Promise<ProvidersFile> {
  return invoke('delete_provider', { id })
}

/// Activate a managed provider — mirrors it into the active config + rebuilds
/// the engine client config. Emits `CONFIG_UPDATED`.
export async function setActiveProvider(id: string): Promise<void> {
  await invoke('set_active_provider', { id })
}

export type {
  ProviderConnection,
  ProvidersFile,
  ProviderInput,
  ProviderStatus,
}
export type { SurfaceInfo, CliInstallStatus, CliInstallResult, AppUpdateInfo }

/// Reliable provider-activation signal (ADR-0005-safe replacement for the
/// dead `config.provider` / `config.api_key` gating).
export async function getProviderStatus(): Promise<ProviderStatus> {
  return invoke('get_provider_status')
}

// --- Models & Status ---

export async function listModels(): Promise<ModelInfo[]> {
  return invoke('list_models')
}

/**
 * Effective provider allowlist for the desktop UI (ADR-0005 P4.9).
 * Returns:
 *   - `Some(slugs)` when the desktop has an explicit override, or the
 *     engine's `SHANNON_*_PROVIDERS` env vars set one. `Some([])` means
 *     "user toggled every provider off".
 *   - `null` when no restriction is in effect (full catalog visible).
 *
 * The Settings → Provider visibility panel renders this state; a
 * "Reset to default" button sends `null` to clear the desktop override
 * (falls back to env vars).
 */
export async function getProviderAllowlist(): Promise<string[] | null> {
  return invoke<string[] | null>('get_provider_allowlist')
}

export async function getStatus(): Promise<StatusResponse> {
  return invoke('get_status')
}

// --- R2-1: session-level model override (composer chip) ---

/** A per-session model override (R2-1). `provider` is the desktop
 *  provider-kind slug (`anthropic` | `openai` | … | `openai-compatible`),
 *  `model` the canonical catalog id. `null` results mean "session inherits
 *  the global default". */
export interface SessionModelOverride {
  provider: string
  model: string
}

/** Pin the CURRENT session's model: subsequent queries of this session use
 *  `provider` + `model`; other sessions and new chats keep the global
 *  default. The chip's "Set as default" action goes through `configure`
 *  instead (global semantics). */
export async function setSessionModel(
  sessionId: string | null | undefined,
  provider: string,
  model: string,
): Promise<void> {
  await invoke('set_session_model', { sessionId: sessionId ?? null, provider, model })
}

/** Clear the session override — the session inherits the global default
 *  again (including future default changes). Idempotent. */
export async function clearSessionModel(sessionId: string | null | undefined): Promise<void> {
  await invoke('clear_session_model', { sessionId: sessionId ?? null })
}

/** Read the session's model override, `null` when none is set. */
export async function getSessionModel(
  sessionId: string | null | undefined,
): Promise<SessionModelOverride | null> {
  return invoke<SessionModelOverride | null>('get_session_model', { sessionId: sessionId ?? null })
}

// --- P2-5: session-level "temporary chat" (no-memory bypass) ---

/** Pin the CURRENT session's memory bypass: `disabled = true` builds this
 *  session's subsequent queries without the memory layer (no injection of
 *  past memories, no auto-extraction of new ones). Other sessions are
 *  untouched; takes effect on the next send. Persisted per session
 *  (Rust-side sidecar) so it survives a restart. */
export async function setSessionMemoryBypass(
  sessionId: string | null | undefined,
  disabled: boolean,
): Promise<void> {
  await invoke('set_session_memory_bypass', { sessionId: sessionId ?? null, disabled })
}

/** Read the session's memory bypass flag (`false` = memory in use). */
export async function getSessionMemoryBypass(
  sessionId: string | null | undefined,
): Promise<boolean> {
  return invoke<boolean>('get_session_memory_bypass', { sessionId: sessionId ?? null })
}

// --- R2-2: Settings "Refresh model catalog" ---

/** Result of `refresh_model_catalog`: how many models the dynamic
 *  models.dev overlay now carries + its monotonic generation counter. */
export interface ModelCatalogRefreshResult {
  count: number
  generation: number
}

/** Re-fetch the models.dev dynamic catalog (same path as CLI
 *  `/model refresh`). Throws with the upstream failure reason. */
export async function refreshModelCatalog(): Promise<ModelCatalogRefreshResult> {
  return invoke('refresh_model_catalog')
}

// --- R3-2 (desktop slice): provider model profiles ---

/** List the engine store's model profiles (`"default"` pinned first, rest
 *  alphabetical; `active` marks the engine's `active_profile`). */
export async function listProviderProfiles(): Promise<ProviderProfileSummary[]> {
  return invoke('list_provider_profiles')
}

/** Create an empty named model profile (inactive — switching is explicit).
 *  Returns the refreshed list. Throws on empty/too-long/whitespace names
 *  (the engine's shared `validate_profile_name` contract) and duplicates. */
export async function createProviderProfile(name: string): Promise<ProviderProfileSummary[]> {
  return invoke('create_provider_profile', { name })
}

/** Switch the engine's active model profile and re-point the global
 *  default. Returns the refreshed list. The UI confirms before switching
 *  to a profile with no providers. */
export async function setActiveProviderProfile(name: string): Promise<ProviderProfileSummary[]> {
  return invoke('set_active_provider_profile', { name })
}

/** R5: rename a model profile. Engine errors (duplicate target, unknown
 *  source) surface verbatim; the fresh list comes back. When the renamed
 *  profile was active, the backend re-points the global default and
 *  re-announces it. */
export async function renameProviderProfile(
  oldName: string,
  newName: string,
): Promise<ProviderProfileSummary[]> {
  return invoke('rename_provider_profile', { old: oldName, new: newName })
}

/** R5: delete a model profile. `force` is the desktop's standing `true` —
 *  the UI's ConfirmDialog is the consent (and names the fallback when the
 *  target is active); the backend reports which profile became active. */
export async function deleteProviderProfile(name: string, force = true): Promise<DeleteProfileOutcome> {
  return invoke('delete_provider_profile', { name, force })
}

// --- R4-3 (desktop slice): per-provider multi-key management ---

/** List a provider's stored keys in rotation order (index 0 = ACTIVE).
 *  Hints are masked server-side; full key material never crosses the wire.
 *  A provider with no stored key yet lists as empty. */
export async function listProviderKeys(providerId: string): Promise<ProviderKeySummary[]> {
  return invoke('list_provider_keys', { providerId })
}

/** Add a key to a provider's rotation list (plaintext — the same trust
 *  level as the Add/Edit provider modal). Returns the fresh list. */
export async function addProviderKey(providerId: string, key: string): Promise<ProviderKeySummary[]> {
  return invoke('add_provider_key', { providerId, key })
}

/** Remove the key at `index`. Removing the ACTIVE key promotes the next
 *  stored one; the last remaining key is refused by the backend. Returns
 *  the fresh list. */
export async function removeProviderKey(providerId: string, index: number): Promise<ProviderKeySummary[]> {
  return invoke('remove_provider_key', { providerId, index })
}

/** Make the key at `index` the ACTIVE one (swap-to-slot-0 semantics); the
 *  running client is hot-reloaded when this provider is active. Returns
 *  the fresh list. */
export async function activateProviderKey(providerId: string, index: number): Promise<ProviderKeySummary[]> {
  return invoke('activate_provider_key', { providerId, index })
}

export async function getTools(): Promise<ToolInfo[]> {
  return invoke('list_tools')
}

// --- Rewind (/rewind desktop) ---
export interface CheckpointInfo {
  turn_index: number
  timestamp: number
  description: string
  files_changed: string[]
  prompt_preview: string | null
}

export async function listCheckpoints(sessionId: string): Promise<CheckpointInfo[]> {
  return invoke('list_checkpoints', { sessionId })
}

/** Drops `turnIndex` and everything after it; returns the surviving messages. */
export async function rewindSession(sessionId: string, turnIndex: number): Promise<ChatMessage[]> {
  return invoke('rewind_session', { sessionId, turnIndex })
}

// --- Message feedback (PM-12) ---
export type FeedbackRating = 'up' | 'down'

export async function recordMessageFeedback(
  sessionId: string,
  key: string,
  rating: FeedbackRating | null,
): Promise<void> {
  return invoke('record_message_feedback', { sessionId, key, rating })
}

export async function listMessageFeedback(
  sessionId: string,
): Promise<Record<string, FeedbackRating>> {
  return invoke('list_message_feedback', { sessionId })
}

export interface FeedbackSessionSummary {
  session_id: string
  up: number
  down: number
  updated_at: number
}

export async function listFeedbackSessions(): Promise<FeedbackSessionSummary[]> {
  return invoke('list_feedback_sessions')
}

// --- Slash-command backends (/context · /cost · /diff) ---

export interface SessionContextStats {
  /** CJK-aware estimate of the projected conversation, incl. system prompt. */
  estimated_tokens: number
  /** Null when the window is genuinely unknown (no fabricated fallback). */
  context_window: number | null
}

export async function getSessionContextStats(sessionId: string): Promise<SessionContextStats> {
  return invoke('get_session_context_stats', { sessionId })
}

export interface SessionUsageSummary {
  input_tokens: number
  output_tokens: number
  cache_creation_tokens: number
  cache_read_tokens: number
  cost_usd: number
  /** Ledger events seen for this session (0 may mean a pre-attribution log). */
  events: number
}

export async function getSessionUsage(sessionId: string): Promise<SessionUsageSummary> {
  return invoke('get_session_usage', { sessionId })
}

export interface GitDiffFile {
  path: string
  insertions: number
  deletions: number
}

export interface GitDiffSummary {
  is_repo: boolean
  files: GitDiffFile[]
  patch: string
  truncated: boolean
}

export async function getSessionGitDiff(workingDir: string): Promise<GitDiffSummary> {
  return invoke('get_session_git_diff', { workingDir })
}

export interface CompactSessionSummary {
  performed: boolean
  /** True when the session had no compactable history. */
  nothing_to_compact: boolean
  original_tokens: number
  compacted_tokens: number
  reduction_ratio: number
  messages_removed: number
  /** Turns the compacted L0 log now holds (summary turn + kept recents). */
  kept_turns: number
}

export interface CompactSessionResult extends CompactSessionSummary {
  messages: ChatMessage[]
}

/** /compact — summarize history and persist the compacted conversation. */
export async function compactSession(sessionId: string): Promise<CompactSessionResult> {
  return invoke('compact_session', { sessionId })
}

// --- Sessions ---

export async function newSession(): Promise<string> {
  return invoke('new_session')
}

export async function listSessions(): Promise<SessionInfo[]> {
  return invoke('list_sessions')
}

/** P0 plan dock — the session working dir's most recent persisted plan
 *  (`<workingDir>/.shannon/plans/*.md`, newest by mtime); null when none. */
export async function getSessionPlan(workingDir: string): Promise<SessionPlan | null> {
  return invoke('get_session_plan', { workingDir })
}

/** Turn Timeline (§4.14) — L0-derived turns/tools/token-cost view of one session. */
export async function getTraceTimeline(sessionId: string): Promise<TurnTimeline> {
  return invoke('trace_timeline', { sessionId })
}

export async function searchSessions(query: string): Promise<SessionInfo[]> {
  return invoke('search_sessions', { query })
}

export async function loadSession(id: string): Promise<ChatMessage[]> {
  return invoke('load_session', { id })
}

export async function switchSession(id: string): Promise<ChatMessage[]> {
  return invoke('switch_session', { id })
}

// --- P1-1 session multi-window (frozen backend contract) ---

export async function openSessionWindow(sessionId: string): Promise<SessionWindowInfo> {
  return invoke('open_session_window', { sessionId })
}

export async function listSessionWindows(): Promise<SessionWindowInfo[]> {
  return invoke('list_session_windows')
}

export async function closeSessionWindow(label: string): Promise<void> {
  await invoke('close_session_window', { label })
}

/** Focus the main window and have it switch to `sessionId`. */
export async function revealSessionInMain(sessionId: string): Promise<void> {
  await invoke('reveal_session_in_main', { sessionId })
}

// --- Office Wave 3 C3 companion Quick Capture window (frozen backend contract) ---

/** Create (or focus) the always-on-top-capable `companion` window. */
export async function openCompanionWindow(): Promise<CompanionWindowInfo> {
  return invoke('open_companion_window')
}

/** Toggle the companion window's stay-on-top flag (only acts on `companion`). */
export async function setCompanionAlwaysOnTop(enabled: boolean): Promise<void> {
  await invoke('set_companion_always_on_top', { enabled })
}

export async function setSessionWorkingDir(id: string, path: string): Promise<void> {
  await invoke('set_session_working_dir', { id, path })
}

export async function createSessionWorktree(id: string, title: string): Promise<TaskWorktreeDto> {
  return invoke('create_session_worktree', { id, title })
}

export async function deleteSession(id: string): Promise<boolean> {
  return invoke('delete_session', { id })
}

/** Session archive (卡A): write the archived curation flag — the session
 *  leaves the active rail and every cross-session input layer. `true` when
 *  this call flipped the flag (false = already archived). */
export async function archiveSession(id: string): Promise<boolean> {
  return invoke('archive_session', { id })
}

/** Session archive (卡A): clear the archived flag; the rail repopulates
 *  from the store projection without a restart. `true` when flipped. */
export async function unarchiveSession(id: string): Promise<boolean> {
  return invoke('unarchive_session', { id })
}

/** Session archive (卡A): the archived lens — every archived session, most
 *  recently active first. */
export async function listArchivedSessions(): Promise<ArchivedSessionRow[]> {
  return invoke('list_archived_sessions')
}

export async function renameSession(id: string, title: string): Promise<boolean> {
  return invoke('rename_session', { id, title })
}

export async function duplicateSession(id: string): Promise<SessionInfo> {
  return invoke('duplicate_session', { id })
}

export async function branchSession(parentId: string, branchPoint: number): Promise<SessionInfo> {
  return invoke('branch_session', { parentId, branchPoint })
}

export async function exportSession(id: string, format: 'markdown' | 'json'): Promise<string> {
  return invoke('export_session', { id, format })
}

// Save a UTF-8 text payload (e.g. an exported Markdown blob) to an absolute
// path chosen by the user via @tauri-apps/plugin-dialog's save().
// B0 P0-3: `expectedMtime` opts into a stale-write conflict check — the
// command rejects with `{ code: 'mtime_conflict' }` when the file changed
// since it was read.
export async function saveTextFile(path: string, content: string, expectedMtime?: string): Promise<void> {
  await invoke('save_text_file', { path, content, expectedMtime })
}

// G5 P0-8: save text via a BACKEND-driven native save dialog. The user's
// pick in the dialog is the explicit authorization, so destinations outside
// the working directory (Downloads, Documents, …) work — `save_text_file`
// is working-dir-scoped by design and would reject them. Resolves to the
// final path written, or null when the user cancelled the dialog.
export async function saveTextFileViaDialog(content: string, defaultName: string): Promise<string | null> {
  return invoke<string | null>('save_text_file_via_dialog', { content, defaultName })
}

// --- 2026-09-25 open pipeline (docs/plans/2026-09-25-desktop-chat-ui-
// open-and-artifact-design.md §4 P0-A / P0-B / P1-C / P1-D / P1-E) ---

/** Open an http/https URL in the system browser (Rust validates the scheme). */
export async function openExternal(url: string): Promise<void> {
  await invoke('open_external', { url })
}

/** Open a local file with its OS default application (Rust scopes to $HOME/$TEMP). */
export async function openWithDefaultApp(path: string): Promise<void> {
  await invoke('open_with_default_app', { path })
}

/** Reveal a local file in the OS file manager. */
export async function revealInFolder(path: string): Promise<void> {
  await invoke('reveal_in_folder', { path })
}

/** Write a text artifact to $TEMP and open it with the default app; returns the path. */
export async function openArtifactExternally(title: string, source: string, ext: string): Promise<string> {
  return invoke('open_artifact_externally', { title, source, ext })
}

// --- 2026-09-29 office Wave 1 (docs/research/2026-09-29-office-scenario-
// competitive-research.md §10 v2): host-runtime probe + file copy (save-as) ---

/** Availability of host-run tools used by built-in document skills. */
export interface HostRuntimeProbe {
  python3: boolean
  pythonVersion: string | null
  pandoc: boolean
  libreoffice: boolean
}

/** Probe the host for python3/pandoc/libreoffice (short timeouts, no side effects). */
export async function probeHostRuntime(): Promise<HostRuntimeProbe> {
  return invoke<HostRuntimeProbe>('probe_host_runtime')
}

/** Copy a local file to a caller-chosen destination path (save-as). */
export async function copyFile(srcPath: string, destPath: string): Promise<void> {
  await invoke('copy_file', { srcPath, destPath })
}

// --- 2026-09-26 round2 §5-1 A — artifact:// interactive HTML (design doc
// docs/plans/2026-09-26-desktop-chat-ui-round2-design.md) ---

/** Result of registering an interactive HTML artifact with the Rust-side
 * registry: the id (pass to {@link unregisterInteractiveArtifact}) and the
 * ready-to-load iframe URL (platform-shaped, computed Rust-side). */
export interface ArtifactRegistration {
  id: string
  url: string
}

/**
 * Store an interactive HTML artifact in the Rust-side registry and get back
 * the `artifact://` (or `http://artifact.localhost/` on Windows) URL to load
 * in a sandboxed iframe. The response carries its own strict CSP and the
 * document runs in an opaque origin — that is what unlocks real scripts
 * where srcdoc iframes could never have them.
 */
export async function registerInteractiveHtml(html: string): Promise<ArtifactRegistration> {
  return invoke('register_interactive_artifact', { html })
}

/** Remove a previously registered artifact (unknown/expired ids are a
 * silent no-op Rust-side). */
export async function unregisterInteractiveArtifact(id: string): Promise<void> {
  await invoke('unregister_interactive_artifact', { id })
}


export interface FrameProbe {
  frameable: boolean
  status: number
  reason: string | null
}

/** Server-side X-Frame-Options / frame-ancestors probe backing the web tab. */
export async function probeUrlFrameable(url: string): Promise<FrameProbe> {
  return invoke('probe_url_frameable', { url })
}

/** Existence probe for chat file references (anti-hallucination backstop). */
export async function pathExists(path: string): Promise<boolean> {
  return invoke('path_exists', { path })
}

// --- 2026-09-30 office Wave 2 (B9' Files page): reference-style file index.
// The Rust side owns the on-disk index; these wrappers are the whole
// frontend contract. Registration is fire-and-forget from the UI (attach
// flow / FileCard render) — callers swallow rejections so a failed index
// write can never interrupt a chat.

/** Every indexed file, `registered_at` descending. */
export async function listFileIndex(): Promise<FileIndexEntry[]> {
  return invoke('list_file_index')
}

/** Upsert one file into the index (`source`: 'attachment' | 'generated'). */
export async function registerFileIndexEntry(path: string, source: string): Promise<void> {
  await invoke('register_file_index_entry', { path, source })
}

/** Toggle an entry's favorite flag (persisted Rust-side). */
export async function setFileIndexFavorite(path: string, favorite: boolean): Promise<void> {
  await invoke('set_file_index_favorite', { path, favorite })
}

export interface TextFileContent {
  path: string
  content: string
  sizeBytes: number
}

/**
 * Machine-readable failure codes for `readTextFile` (§P2-24): the Rust
 * command rejects with a structured `{ code, message }` payload instead of
 * English prose the frontend had to substring-match. Branch on the code —
 * never on the message.
 */
export type ReadTextFileErrorCode =
  | 'out_of_scope'
  | 'not_a_file'
  | 'file_too_large'
  | 'binary_file'
  | 'not_utf8'
  | 'io_error'

export interface ReadTextFileError {
  code: ReadTextFileErrorCode
  message: string
}

const READ_TEXT_FILE_CODES: ReadonlySet<string> = new Set([
  'out_of_scope',
  'not_a_file',
  'file_too_large',
  'binary_file',
  'not_utf8',
  'io_error',
])

/**
 * Normalize a `readTextFile` rejection to its code. Anything without the
 * structured payload (mock environments, unexpected throws) degrades to
 * `io_error` so callers keep a safe default branch.
 */
export function readTextFileErrorCode(e: unknown): ReadTextFileErrorCode {
  if (e && typeof e === 'object' && 'code' in e) {
    const code = (e as { code: unknown }).code
    if (typeof code === 'string' && READ_TEXT_FILE_CODES.has(code)) return code as ReadTextFileErrorCode
  }
  return 'io_error'
}

/** Capped, scope-checked text read (disk artifacts / the dock's manual tab).
 * Rejects with a `ReadTextFileError` payload — see `readTextFileErrorCode`. */
export async function readTextFile(path: string, maxBytes?: number): Promise<TextFileContent> {
  return invoke('read_text_file', { path, maxBytes: maxBytes ?? null })
}

// --- Permissions ---

export async function requestPermission(tool: string, input: unknown, risk: string): Promise<boolean> {
  return invoke('request_permission', { tool, input, risk })
}

export type PermissionScope = 'once' | 'always_tool'

export async function respondPermission(
  requestId: string,
  allow: boolean,
  options?: { note?: string; scope?: PermissionScope },
): Promise<void> {
  await invoke('respond_permission', {
    requestId,
    allow,
    note: options?.note ?? null,
    scope: options?.scope ?? null,
  })
}

// --- Files & Diffs ---

export async function getFileDiff(path: string): Promise<FileDiff> {
  return invoke('get_file_diff', { path })
}

export async function applyDiff(filePath: string, hunks: HunkAction[]): Promise<void> {
  return invoke('apply_diff', { filePath, hunks })
}

// GB P2-10b: the backend returns Vec<FileTreeNode> (root entries with
// nested children, walk bounded at depth 12 / 5000 entries) — the old
// single-node signature never matched the Rust command and had no callers.
export async function getFileTree(path: string): Promise<FileNode[]> {
  return invoke('get_file_tree', { path })
}

export async function getWorkingDirInfo(): Promise<WorkingDirInfo> {
  return invoke('get_working_dir_info')
}

// --- MCP Servers ---

export async function listMcpServers(): Promise<McpServerInfo[]> {
  return invoke('list_mcp_servers')
}

export async function addMcpServer(name: string, command: string, args: string[], env: Record<string, string>): Promise<McpServerInfo> {
  return invoke('add_mcp_server', { name, command, args, env })
}

export async function removeMcpServer(name: string): Promise<boolean> {
  return invoke('remove_mcp_server', { name })
}

export async function restartMcpServer(name: string): Promise<McpServerInfo> {
  return invoke('restart_mcp_server', { name })
}

// W2-A: inline enable/disable toggle — persists to settings.json and
// reconciles the pool (stop on disable, start on enable).
export async function setMcpServerEnabled(name: string, enabled: boolean): Promise<McpServerInfo> {
  return invoke('set_mcp_server_enabled', { name, enabled })
}

// W3-B (A2): replay the OAuth loopback flow for an existing remote entry
// (the NeedsAuth state's recovery action) and reconnect the pool.
export async function reauthenticateMcpServer(name: string): Promise<McpServerInfo> {
  return invoke('reauthenticate_mcp_server', { name })
}

export async function getMcpServerConfig(name: string): Promise<McpServerConfig> {
  return invoke('get_mcp_server_config', { name })
}

// --- Skills ---

export async function listSkills(): Promise<SkillInfo[]> {
  return invoke('list_skills')
}

export async function getSkillDetail(name: string): Promise<SkillDetail> {
  return invoke('get_skill_detail', { name })
}

// --- Extensions Hub (P1) ---

export async function listInstalledAddons(): Promise<InstalledAddonSummary[]> {
  return invoke('list_installed_addons')
}

export interface CatalogUpstream {
  kind: 'skill' | 'agent' | 'mcp' | 'data_source' | 'native'
  slug: string
  display_name: string
  repo: string | null
  trust: 'verified' | 'official' | 'community' | 'unknown'
  entry_count: number
}

export async function listCatalogUpstreams(): Promise<CatalogUpstream[]> {
  return invoke('list_catalog_upstreams')
}

// --- Extensions Hub (P2: MCP installers) ---

export interface FeaturedVendor {
  slug: string
  display_name: string
  description: string
  icon: string
  category: 'productivity' | 'communication' | 'developer_tools' | 'data_sources'
  trust: 'unknown' | 'community' | 'official' | 'verified'
  install_kind:
    | { type: 'oauth_remote'; authorize_url: string; token_url: string; mcp_endpoint: string; client_id_env: string; default_scopes: string[]; display_name: string }
    | { type: 'stdio'; command: string; args: string[]; env_vars: [string, string][]; display_name: string }
  homepage_url: string
}

export interface RegistryServer {
  id: string
  name: string
  description: string | null
  repository: string | null
  version: string | null
  homepage_url: string | null
  license: string | null
  stars: number | null
  last_updated: string | null
  verified: boolean
}

export interface InstallResult {
  id: string
  name: string
  install_path: string | null
}

export interface OAuthAuthorizeUrl {
  url: string
  verifier: string
  state: string
}

export interface StdioMcpSpecPayload {
  server_name: string
  command: string
  args: string[]
  env: [string, string][]
}

export async function listFeaturedVendors(): Promise<FeaturedVendor[]> {
  return invoke('list_featured_vendors')
}

export async function featuredVendorToEntry(slug: string): Promise<CatalogEntry> {
  return invoke('featured_vendor_to_entry', { slug })
}

export async function listMcpRegistryServers(): Promise<RegistryServer[]> {
  return invoke('list_mcp_registry_servers')
}

export async function installMcpStdio(spec: StdioMcpSpecPayload): Promise<InstallResult> {
  return invoke('install_mcp_stdio', { spec })
}

export async function installMcpMcpb(serverName: string, archiveBytes: number[]): Promise<InstallResult> {
  return invoke('install_mcp_mcpb', { serverName, archiveBytes })
}

export async function installMcpOAuthAuthorizeUrl(vendorSlug: string, redirectUri: string): Promise<OAuthAuthorizeUrl> {
  return invoke('install_mcp_oauth_authorize_url', { vendorSlug, redirectUri })
}

export async function installMcpOAuthComplete(vendorSlug: string, accessToken: string): Promise<InstallResult> {
  return invoke('install_mcp_oauth_complete', { vendorSlug, accessToken })
}

/**
 * One-click OAuth loopback installer (RFC 6749 §3.1.2.4 + RFC 7636 PKCE).
 *
 * The Rust side binds an ephemeral loopback port, opens the vendor's
 * authorize URL in the default browser, accepts the callback, exchanges
 * the code for a token, and writes the MCP server config. Resolves with
 * the InstallResult; rejects on any failure (bind / browse / callback /
 * token exchange / write).
 *
 * UI should show a busy state for the whole await — no manual token
 * paste step is needed.
 */
export async function installMcpOAuthLoopback(vendorSlug: string): Promise<InstallResult> {
  return invoke('install_mcp_oauth_loopback', { vendorSlug })
}

export async function uninstallMcpServer(serverName: string): Promise<void> {
  return invoke('uninstall_mcp_server', { serverName })
}

// --- Extensions Hub (P3: Skills catalog + installer) ---

export interface SkillCatalogEntry {
  id: string
  kind: 'skill'
  name: string
  description: string
  author: string | null
  version: string | null
  homepage_url: string | null
  license: string | null
  stars: number | null
  last_updated: string | null
  source:
    | { type: 'mcp_registry'; publisher: string }
    | { type: 'featured_vendor' }
    | { type: 'git_hub_repo'; repo: string; ref_?: string | null }
    | { type: 'custom'; url: string }
    | { type: 'native' }
  trust: 'unknown' | 'community' | 'official' | 'verified'
  metadata: Record<string, unknown>
  tags: string[]
}

export interface InstalledSkill {
  name: string
  path: string
  installed_at: string | null
}

export async function listSkillCatalog(): Promise<SkillCatalogEntry[]> {
  return invoke('list_skill_catalog')
}

export async function installSkillFromRepo(
  pluginName: string,
  repo: string,
  ref_: string,
): Promise<InstallResult> {
  return invoke('install_skill_from_repo', { pluginName, repo, ref_ })
}

export async function installNativeSkill(
  pluginName: string,
  body: string,
): Promise<InstallResult> {
  return invoke('install_native_skill', { pluginName, body })
}

export async function listInstalledSkillPlugins(): Promise<InstalledSkill[]> {
  return invoke('list_installed_skill_plugins')
}

export async function uninstallSkillPlugin(name: string): Promise<void> {
  return invoke('uninstall_skill_plugin', { name })
}

// --- Self-improvement (D6 Phase 1+: skill candidates + agent-authored) ---

export interface SkillCandidate {
  id: string
  detected_at: string
  occurrence_count: number
  example_session_ids: string[]
  proposed_name: string
  proposed_trigger: string
  procedure: string[]
  source_tool_calls: Array<{ tool: string; args_summary: Record<string, unknown> }>
  refined?: boolean
}

export interface AgentAuthoredSkill {
  id: string
  name: string
  description: string
  trigger: string
  procedure: string[]
  created_at: string
  originating_sessions: string[]
}

export async function listSkillCandidates(): Promise<SkillCandidate[]> {
  return invoke('list_skill_candidates')
}

export async function approveSkillCandidate(id: string, edits?: Partial<AgentAuthoredSkill>): Promise<AgentAuthoredSkill> {
  return invoke('approve_skill_candidate', { id, edits: edits ?? null })
}

export async function rejectSkillCandidate(id: string): Promise<void> {
  return invoke('reject_skill_candidate', { id })
}

export async function refineSkillCandidate(id: string): Promise<string> {
  return invoke('refine_skill_candidate', { id })
}

export async function listAgentAuthoredSkills(): Promise<AgentAuthoredSkill[]> {
  return invoke('list_agent_authored_skills')
}

// --- Extensions Hub (P4: Agents catalog + installer) ---

export interface AgentCatalogEntry {
  id: string
  kind: 'agent'
  name: string
  description: string
  author: string | null
  version: string | null
  homepage_url: string | null
  license: string | null
  stars: number | null
  last_updated: string | null
  source:
    | { type: 'mcp_registry'; publisher: string }
    | { type: 'featured_vendor' }
    | { type: 'git_hub_repo'; repo: string; ref_?: string | null }
    | { type: 'custom'; url: string }
    | { type: 'native' }
  trust: 'unknown' | 'community' | 'official' | 'verified'
  metadata: {
    trigger?: string
    model?: string
    tools?: string[]
    system_prompt?: string
    upstream?: string
    [k: string]: unknown
  }
  tags: string[]
}

export interface InstalledAgent {
  name: string
  path: string
  installed_at: string | null
}

export async function listAgentCatalog(): Promise<AgentCatalogEntry[]> {
  return invoke('list_agent_catalog')
}

export async function installAgentFromRepo(
  pluginName: string,
  repo: string,
  ref_: string,
): Promise<InstallResult> {
  return invoke('install_agent_from_repo', { pluginName, repo, ref_ })
}

/**
 * G1 P1-9: install a native agent as a FLAT `~/.shannon/agents/<name>.toml`
 * `AgentDefinition` (the shape the runtime loader reads). The catalog
 * entry's description/system_prompt map onto the definition fields; tool
 * hints become capabilities.
 */
export async function installNativeAgent(
  pluginName: string,
  description: string,
  systemPrompt: string,
  model: string | null,
  tools: string[],
): Promise<InstallResult> {
  return invoke('install_native_agent', { pluginName, description, systemPrompt, model, tools })
}

export async function listInstalledAgentPlugins(): Promise<InstalledAgent[]> {
  return invoke('list_installed_agent_plugins')
}

export async function uninstallAgentPlugin(name: string): Promise<void> {
  return invoke('uninstall_agent_plugin', { name })
}

// --- Extensions Hub (P5: Native data sources — Obsidian + Email IMAP) ---

export interface DataSourceCatalogEntry {
  id: string
  kind: 'data_source'
  name: string
  description: string
  author: string | null
  version: string | null
  homepage_url: string | null
  license: string | null
  stars: number | null
  last_updated: string | null
  source: { type: 'native' }
  trust: 'verified' | 'official' | 'community' | 'unknown'
  metadata: {
    kind?: string
    fields?: DataSourceField[]
    [k: string]: unknown
  }
  tags: string[]
}

export interface DataSourceField {
  key: string
  label: string
  kind: 'text' | 'password' | 'path' | 'number' | string
  required: boolean
  placeholder?: string | null
  help?: string | null
}

export interface DataSourceAdapter {
  slug: string
  kind: string
  name: string
  description: string
  homepage_url: string | null
  fields: DataSourceField[]
}

export interface InstalledDataSource {
  slug: string
  kind: string
  name: string
  path: string
  installed_at: string | null
  /**
   * F5 (A8): where this source's credentials live — `keyring` (OS keyring)
   * or `plaintext_file` (degraded owner-only 0600 TOML), `null` when the
   * source has no credential fields. Drives the page's credential-storage
   * status line.
   */
  credential_storage?: 'keyring' | 'plaintext_file' | null
}

export async function listDataSourceCatalog(): Promise<DataSourceCatalogEntry[]> {
  return invoke('list_data_source_catalog')
}

export async function listDataSourceAdapters(): Promise<DataSourceAdapter[]> {
  return invoke('list_data_source_adapters')
}

export async function installDataSource(
  slug: string,
  kind: string,
  name: string,
  config: Record<string, string>,
): Promise<InstallResult> {
  return invoke('install_data_source', {
    slug,
    kind,
    name,
    config,
  })
}

export async function listInstalledDataSources(): Promise<InstalledDataSource[]> {
  return invoke('list_installed_data_sources')
}

export async function uninstallDataSource(slug: string): Promise<void> {
  return invoke('uninstall_data_source', { slug })
}

export async function readDataSourceConfig(
  slug: string,
): Promise<Record<string, string>> {
  return invoke('read_data_source_config', { slug })
}

export async function queryDataSource(
  slug: string,
  query: string,
): Promise<DataSourceResult> {
  return invoke('query_data_source', { slug, query })
}

// --- Extensions Hub (P6: Security hardening) ---

export type InjectionRisk = 'clean' | 'suspicious' | 'dangerous'

export interface InjectionMatch {
  pattern: string
  matched_substring: string
  category: string
}

export interface InjectionReport {
  risk: InjectionRisk
  matches: InjectionMatch[]
  match_count: number
}

export type SignatureStatus =
  | 'trusted'
  | 'untrusted_signature'
  | 'unsigned'
  | 'malformed'

export interface SignatureReport {
  status: SignatureStatus
  signer: string | null
  note: string
}

export interface CatalogReport {
  entry_id: string
  reason: string
  created_at: string
}

export async function scanPromptInjection(text: string): Promise<InjectionReport> {
  return invoke('scan_prompt_injection', { text })
}

export async function scanPromptInjectionWithReadme(
  description: string,
  readmeUrl: string | null,
): Promise<InjectionReport> {
  return invoke('scan_prompt_injection_with_readme', {
    description,
    readmeUrl: readmeUrl ?? null,
  })
}

export async function verifySignature(
  signatureBody: string | null,
): Promise<SignatureReport> {
  return invoke('verify_signature', { signatureBody })
}

export async function reportCatalogEntry(
  entryId: string,
  reason: string,
): Promise<CatalogReport> {
  return invoke('report_catalog_entry', { entryId, reason })
}

export async function listCatalogReports(): Promise<CatalogReport[]> {
  return invoke('list_catalog_reports')
}

export async function clearCatalogReport(entryId: string): Promise<number> {
  return invoke('clear_catalog_report', { entryId })
}

// --- Plugins (A.3 ecosystem compatibility) ---

export interface PluginInfo {
  name: string
  version: string
  description: string
  author: string | null
  plugin_type: string
  enabled: boolean
  path: string
  source_format: 'shannon-toml' | 'claude-json' | 'unknown'
  /** Install origin for the X6 source badge, derived desktop-side:
   *  - `migration` — thin `imported-<source>` record (`migration_imported`);
   *  - `git` — the plugin directory carries a `.git` checkout (exactly the
   *    condition `update` needs, so 更新 is offered only for these);
   *  - `local` — copied in from a local directory / .dxt/.mcpb/.zip archive.
   *  There is no `registry` origin: marketplace plugin bundles are git
   *  clones and badge as `git`. */
  source: 'git' | 'local' | 'migration'
  /** Thin `imported-<source>` migration record (X5): the UI suppresses
   *  uninstall/enable/disable on it. */
  migration_imported: boolean
}

export async function listPlugins(): Promise<PluginInfo[]> {
  return invoke('list_plugins')
}

/** Result of a plugin install: the registered name plus best-effort
 *  materialization warnings (X5). */
export interface PluginInstallResult {
  name: string
  warnings: string[]
}

/** Result of a plugin lifecycle op (uninstall/enable/disable/update):
 *  per-artifact warnings from (reverse-)materialization. Empty = clean. */
export interface PluginLifecycleResult {
  warnings: string[]
}

export async function installPlugin(sourcePath: string): Promise<PluginInstallResult> {
  return invoke('install_plugin', { sourcePath })
}

/** `allowUnverified` is the SEC-1 opt-in — pass `true` only after the user
 *  explicitly confirmed installing a plugin whose manifest declares no
 *  permissions. */
export async function installPluginFromGit(
  repoUrl: string,
  allowUnverified?: boolean,
): Promise<PluginInstallResult> {
  return invoke('install_plugin_from_git', { repoUrl, allowUnverified: allowUnverified ?? false })
}

export async function uninstallPlugin(name: string): Promise<PluginLifecycleResult> {
  return invoke('uninstall_plugin', { name })
}

export async function enablePlugin(name: string): Promise<PluginLifecycleResult> {
  return invoke('enable_plugin', { name })
}

export async function disablePlugin(name: string): Promise<PluginLifecycleResult> {
  return invoke('disable_plugin', { name })
}

export async function updatePlugin(name: string): Promise<PluginLifecycleResult> {
  return invoke('update_plugin', { name })
}

/** X5 trust preview: inspect a plugin source (local dir, .dxt/.mcpb/.zip
 *  archive, or git URL) and return its bundle summary BEFORE install. */
export async function inspectPluginSource(path: string): Promise<PluginBundleSummary> {
  return invoke('inspect_plugin_source', { path })
}

export async function listPluginMarketplace(): Promise<CatalogEntry[]> {
  return invoke('list_plugin_marketplace')
}

// --- Background Tasks ---

export async function startBackgroundTask(prompt: string): Promise<string> {
  return invoke('start_background_task', { prompt })
}

export async function getBackgroundTasks(): Promise<BackgroundTaskInfo[]> {
  return invoke('get_background_tasks')
}

export async function cancelBackgroundTask(id: string): Promise<boolean> {
  return invoke('cancel_background_task', { id })
}

// --- Agents & Tasks ---

export async function listAgents(): Promise<AgentInfo[]> {
  return invoke('list_agents')
}

// --- Inter-agent message history (Phase D C3) ---

export async function listAgentMessages(
  team?: string,
  limit?: number,
): Promise<AgentMessageEntry[]> {
  return invoke('list_agent_messages', { team: team ?? null, limit: limit ?? null })
}

export async function listAgentMessageTeams(): Promise<string[]> {
  return invoke('list_agent_message_teams')
}

export async function recordAgentMessage(
  team: string,
  from: string,
  to: string,
  content: string,
  priority?: 'low' | 'normal' | 'high' | 'critical',
): Promise<string> {
  return invoke('record_agent_message', {
    team,
    from,
    to,
    content,
    priority: priority ?? null,
  })
}

export interface AgentDefinitionInfo {
  name: string
  description: string
  tools: string[]
  model: string
  prompt: string
  source_path: string
}

export async function listAgentDefinitions(): Promise<AgentDefinitionInfo[]> {
  return invoke('list_agent_definitions')
}

export async function createAgentDefinition(
  name: string,
  model: string | undefined,
  systemPrompt: string | undefined,
  tools: string[],
): Promise<string> {
  return invoke('create_agent_definition', { name, model: model ?? null, systemPrompt: systemPrompt ?? null, tools })
}

export async function deleteAgentDefinition(name: string): Promise<boolean> {
  return invoke('delete_agent_definition', { name })
}

export async function listTasks(): Promise<TaskItem[]> {
  return invoke('list_tasks')
}

export async function updateTask(payload: UpdateTaskPayload): Promise<TaskItem> {
  return invoke('update_task', { payload })
}

export async function getUsageStats(days: number): Promise<UsageStats> {
  return invoke('get_usage_stats', { days })
}

// --- P2-1/P2-6 Usage governance + pre-task cost estimate ---
//
// Rust: shannon-desktop/src/usage_governance.rs.

/** Sidebar % bar / budget-card snapshot: month spend, budget, threshold state.
 *  Side effect on the backend: fires the once-per-month 80/100% desktop
 *  notification when a threshold is newly reached — safe to poll. */
export async function getUsageGovernance(): Promise<UsageGovernance> {
  return invoke('get_usage_governance')
}

/** Historical run-cost range for a routine; `taskId = null` aggregates across
 *  all routines (the "similar tasks" baseline for a brand-new one). */
export async function estimateTaskCost(taskId?: string | null): Promise<TaskCostEstimate> {
  return invoke('estimate_task_cost', { taskId: taskId ?? null })
}

// --- P0-4 Cost observability ---
//
// Session budget + six-category context breakdown + per-session usage
// aggregation (Rust: shannon-desktop/src/cost_commands.rs).

/** Set (or clear with `null`) the session's USD spend cap. */
export async function setSessionBudget(sessionId: string, budgetUsd: number | null): Promise<void> {
  await invoke('set_session_budget', { sessionId, budgetUsd })
}

/** Read the session's budget cap (`null` when none is set). */
export async function getSessionBudget(sessionId: string): Promise<number | null> {
  return invoke('get_session_budget', { sessionId })
}

/** Six-category context estimate for the session's current state. */
export async function getSessionContextBreakdown(sessionId: string): Promise<ContextBreakdown> {
  return invoke('get_session_context_breakdown', { sessionId })
}

/** Per-session usage aggregation for the last `days` days (recency order). */
export async function getUsageBySession(days: number): Promise<SessionUsageRow[]> {
  return invoke('get_usage_by_session', { days })
}

/** X7 per-extension (skill / MCP tool) invocation + token stats. */
export async function getExtensionStats(days: number): Promise<ExtensionStats> {
  return invoke('get_extension_stats', { days })
}

// --- Scheduled Tasks (Sprint 2) ---
//
// Thin invoke() wrappers over the 19 Tauri commands in
// shannon-desktop/src/scheduled_commands.rs. Field names match the Rust DTOs
// exactly (no rename to "ScheduledTask").

// Scheduled tasks (CRUD)

export async function listScheduledTasks(): Promise<ScheduledRoutine[]> {
  return invoke('list_scheduled_tasks')
}

export async function createScheduledTask(payload: CreateTaskPayload): Promise<ScheduledRoutine> {
  return invoke('create_scheduled_task', { payload })
}

export async function updateScheduledTask(payload: UpdateTaskPayload): Promise<ScheduledRoutine> {
  return invoke('update_scheduled_task', { payload })
}

export async function deleteScheduledTask(id: string): Promise<boolean> {
  return invoke('delete_scheduled_task', { id })
}

// P1-1: the backend persists the requested state and returns the persisted
// bool (read-back fool-proofing — see toggle_scheduled_task).
export async function toggleScheduledTask(id: string, enabled: boolean): Promise<boolean> {
  return invoke('toggle_scheduled_task', { id, enabled })
}

export async function triggerTaskNow(id: string): Promise<TriggerResponse> {
  return invoke('trigger_task_now', { id })
}

export async function previewCron(expr: string): Promise<CronPreview> {
  return invoke('preview_cron', { expr })
}

// Triage

export async function listTriageItems(filter?: TriageFilter): Promise<TriageItem[]> {
  return invoke('list_triage_items', { filter: filter ?? null })
}

export async function markTriageRead(id: string): Promise<boolean> {
  return invoke('mark_triage_read', { id })
}

export async function archiveTriageItem(id: string): Promise<boolean> {
  return invoke('archive_triage_item', { id })
}

export async function getTriageStats(): Promise<TriageStats> {
  return invoke('get_triage_stats')
}

// Inbox (P0-3 SQLite inbox)

export async function listInboxItems(filter?: InboxListFilter): Promise<InboxItem[]> {
  return invoke('list_inbox_items', {
    status: filter?.status ?? null,
    source: filter?.source ?? null,
    limit: filter?.limit ?? null,
  })
}

export async function updateInboxItemStatus(id: number, status: InboxItemStatus): Promise<void> {
  return invoke('update_inbox_item_status', { id, status })
}

export async function getInboxStats(): Promise<InboxStats> {
  return invoke('get_inbox_stats')
}

export async function rerunInboxItem(id: number): Promise<string> {
  return invoke('rerun_inbox_item', { id })
}

export async function continueInboxItemSession(id: number): Promise<string> {
  return invoke('continue_inbox_item_session', { id })
}

// Goal runs (P0-2 desktop goal runner)

export interface GoalRunStartInput {
  title: string
  objective: string
  maxTurns?: number
  budgetUsd?: number
}

/// Start an unattended goal run on `sessionId` (a new session is created
/// when omitted). Resolves once the run is registered (status `running`).
export async function startGoalRun(
  input: GoalRunStartInput & { sessionId?: string | null },
): Promise<{ sessionId: string }> {
  return invoke('start_goal_run', {
    sessionId: input.sessionId ?? null,
    title: input.title,
    objective: input.objective,
    maxTurns: input.maxTurns ?? null,
    budgetUsd: input.budgetUsd ?? null,
  })
}

export async function listGoalRuns(): Promise<GoalRunDto[]> {
  return invoke('list_goal_runs')
}

// B2 follow-up — list sub-agents currently registered in the agent-teams
// context. Returns an empty array when the user has not enabled agent
// teams; the Tasks-page panel renders its empty state.
export async function listSubagents(): Promise<SubAgentDto[]> {
  return invoke('list_subagents')
}

export async function getGoalRun(sessionId: string): Promise<GoalRunDto | null> {
  return invoke('get_goal_run', { sessionId })
}

export async function stopGoalRun(sessionId: string): Promise<void> {
  await invoke('stop_goal_run', { sessionId })
}

export async function pauseGoalRun(sessionId: string): Promise<void> {
  await invoke('pause_goal_run', { sessionId })
}

export async function resumeGoalRun(sessionId: string): Promise<void> {
  await invoke('resume_goal_run', { sessionId })
}

export async function updateGoalObjective(sessionId: string, objective: string): Promise<void> {
  await invoke('update_goal_objective', { sessionId, objective })
}

// Batch runs (P1-2 desktop best-of-N worktree parallelism)

export interface BatchRunStartInput {
  title: string
  prompt: string
  count: number
  /** Session whose working directory the batch runs against. */
  baseSessionId?: string | null
}

/// Start a best-of-N batch: N (2..=4) parallel unattended runs of the same
/// prompt, each in its own git worktree forked from the current HEAD.
/// Resolves once the batch is registered (branches `running`).
export async function startBatchRun(input: BatchRunStartInput): Promise<{ batchId: string }> {
  return invoke('start_batch_run', {
    title: input.title,
    prompt: input.prompt,
    count: input.count,
    baseSessionId: input.baseSessionId ?? null,
  })
}

export async function listBatchRuns(): Promise<BatchRunDto[]> {
  return invoke('list_batch_runs')
}

/// The branch worktree's full diff (raw unified patch) against the batch's
/// base commit.
export async function getBatchBranchDiff(batchId: string, index: number): Promise<{ diff: string }> {
  return invoke('get_batch_branch_diff', { batchId, index })
}

/// Merge the branch back into the base repo and clean up the other
/// branches. On conflicts nothing is merged or deleted — `conflicts` lists
/// the files and the worktrees stay for manual handling.
export async function adoptBatchBranch(
  batchId: string,
  index: number,
): Promise<{ merged: boolean; conflicts: string[] | null }> {
  return invoke('adopt_batch_branch', { batchId, index })
}

/// Remove the batch's un-adopted branches. `skipped` lists branches that
/// were left alone (`"<branchName>: <reason>"`).
export async function discardBatchRun(
  batchId: string,
): Promise<{ removed: number; skipped: string[] }> {
  return invoke('discard_batch_run', { batchId })
}

// History

export async function listTaskExecutions(taskId?: string, limit?: number): Promise<TaskExecution[]> {
  return invoke('list_task_executions', { taskId: taskId ?? null, limit: limit ?? null })
}

// P2-8 — cross-agent run table (OPC "runs" view)

/** The newest `limit` runs across ALL routines/agents, each joined with its
 *  back-linked session id and that session's latest usage-ledger model. */
export async function listAgentRuns(limit?: number): Promise<AgentRunRow[]> {
  return invoke('list_agent_runs', { limit: limit ?? null })
}

export async function getExecutionDetail(id: string): Promise<TaskExecutionDetail> {
  return invoke('get_execution_detail', { id })
}

// Triggered routines

export async function listTriggeredRoutines(): Promise<TriggeredRoutineDto[]> {
  return invoke('list_triggered_routines')
}

export async function toggleTriggeredRoutine(name: string, enabled: boolean): Promise<boolean> {
  return invoke('toggle_triggered_routine', { name, enabled })
}

export async function createTriggeredRoutine(payload: {
  name: string
  trigger: string
  command: string
  matcher?: string
  pattern?: string
  description?: string
}): Promise<TriggeredRoutineDto> {
  return invoke('create_triggered_routine', {
    name: payload.name,
    trigger: payload.trigger,
    command: payload.command,
    matcher: payload.matcher ?? null,
    pattern: payload.pattern ?? null,
    description: payload.description ?? null,
  })
}

// Hook events + permission profiles

export async function listHookEvents(): Promise<HookEventInfo[]> {
  return invoke('list_hook_events')
}

export async function listPermissionProfiles(): Promise<ProfilesList> {
  return invoke('list_permission_profiles')
}

/**
 * P1-3: activate (or deactivate) the session-wide permission profile.
 * Frozen contract: `activate_permission_profile(name: string|null)`.
 * Passing `null` clears the active profile; builtin ids and custom profile
 * names sync `approval_mode` per the mode-switcher mapping.
 */
export async function activatePermissionProfile(
  name: string | null,
): Promise<ActiveProfileStatus> {
  return invoke('activate_permission_profile', { name })
}

export async function saveCustomProfile(payload: {
  name: string
  description?: string
  auto_approve: string[]
  confirm: string[]
  deny: string[]
}): Promise<CustomProfileInfo> {
  return invoke('save_custom_profile', {
    name: payload.name,
    description: payload.description ?? null,
    auto_approve: payload.auto_approve,
    confirm: payload.confirm,
    deny: payload.deny,
  })
}

export async function deleteCustomProfile(name: string): Promise<string[]> {
  return invoke('delete_custom_profile', { name })
}

// --- OPC analytics ---

export async function getOpcMetrics(): Promise<OpcMetrics> {
  return invoke('get_opc_metrics')
}

// --- LSP quick-fix ---

export interface CodeActionDto {
  title: string
  kind?: string
  is_preferred: boolean
  edit?: unknown
  command?: string
}

export interface CodeActionRequest {
  file_path: string
  server_cmd: string
  server_args: string[]
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  language_id: string
  diagnostic_messages: string[]
}

export async function lspCodeActions(req: CodeActionRequest): Promise<{ actions: CodeActionDto[] }> {
  return invoke('lsp_code_actions', { req })
}

export async function applyCodeAction(edit: unknown): Promise<number> {
  return invoke('apply_code_action', { edit })
}

export interface SourceFile {
  path: string
  content: string
  language_id: string
}

export async function readSourceFile(path: string): Promise<SourceFile> {
  return invoke('read_source_file', { path })
}

export interface FileDiagnostic {
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  message: string
  severity: string
  source?: string
  code?: string
}

export interface FileDiagnosticsRequest {
  file_path: string
  server_cmd: string
  server_args: string[]
  language_id: string
  content: string
}

export interface FileDiagnosticsResponse {
  diagnostics: FileDiagnostic[]
  timed_out: boolean
}

const DEFAULT_DIAGNOSTICS_SERVERS: Record<
  string,
  { cmd: string; args: string[] }
> = {
  rust: { cmd: 'rust-analyzer', args: [] },
  typescript: { cmd: 'typescript-language-server', args: ['--stdio'] },
  typescriptreact: { cmd: 'typescript-language-server', args: ['--stdio'] },
  javascript: { cmd: 'typescript-language-server', args: ['--stdio'] },
  go: { cmd: 'gopls', args: [] },
  python: { cmd: 'pylsp', args: [] },
}

export function defaultDiagnosticsServer(languageId: string): {
  cmd: string
  args: string[]
} {
  return (
    DEFAULT_DIAGNOSTICS_SERVERS[languageId] ?? { cmd: '', args: [] }
  )
}

export async function runFileDiagnostics(
  req: FileDiagnosticsRequest,
): Promise<FileDiagnosticsResponse> {
  return invoke('run_file_diagnostics', { req })
}

// Worktrees (B9)

export async function createTaskWorktree(taskId: string): Promise<TaskWorktreeDto> {
  return invoke('create_task_worktree', { taskId })
}

export async function listTaskWorktrees(): Promise<TaskWorktreeDto[]> {
  return invoke('list_task_worktrees')
}

export async function removeTaskWorktree(path: string): Promise<void> {
  return invoke('remove_task_worktree', { path })
}

export async function pruneTaskWorktrees(): Promise<string[]> {
  return invoke('prune_task_worktrees')
}

// ─── Onboarding seed (#75) ────────────────────────────────────────────────
//
// First-run sample tasks so the Tasks / Today surfaces aren't empty. The Rust
// command is idempotent — no-op when `.claude/tasks/` already holds any JSON.

export interface SeedReport {
  /** Number of sample task files written. Zero when tasks already existed. */
  tasks_seeded: number
}

export async function seedSampleData(): Promise<SeedReport> {
  return invoke('seed_sample_data')
}

// ─── Migration wizard (P1-6) ───────────────────────────────────────────────
//
// Frozen contract with desktop/src/migration_commands.rs: scan a Claude Code
// or ZCode install, preview per-item conflicts, then apply the user-approved
// imports. Read-scan + copy/merge only — nothing from the source side is
// ever executed, and the backend only ever reads its known source paths.

export type MigrationSourceId = 'claude-code' | 'zcode'

export type MigrationAssetKind = 'mcp' | 'skill' | 'command' | 'memory' | 'settings-rules'

export type MigrationConflictState = 'none' | 'overwrite' | 'skip-existing'

export interface MigrationAsset {
  /** Stable `<source>:<kind>:<slug>` id — deterministic across scans. */
  id: string
  kind: MigrationAssetKind
  name: string
  sourcePath: string
  targetPath: string
  /** `none` (target free) | `overwrite` (exists, differs) | `skip-existing` (identical). */
  conflict: MigrationConflictState
  /** Approximate source size in bytes. */
  sizeHint: number
}

export interface MigrationScanError {
  path: string
  error: string
}

export interface MigrationScanResult {
  source: MigrationSourceId
  items: MigrationAsset[]
  /** Well-known slots probed but absent (「未发现」). */
  notFound: string[]
  /** Non-fatal per-file problems (corrupted JSON, unsupported servers…). */
  errors: MigrationScanError[]
}

export interface MigrationPreviewItem {
  id: string
  diffSummary: string
}

export interface MigrationPreviewResult {
  perItem: MigrationPreviewItem[]
}

/** `action` is frozen; `conflict` is an additive hint for existing, differing
 *  targets (default `rename`, i.e. write `<name>-imported`). */
export interface MigrationItemInput {
  id: string
  action: 'import' | 'skip'
  conflict?: 'overwrite' | 'rename' | 'skip'
}

export interface MigrationApplyFailure {
  id: string
  error: string
}

export interface MigrationApplyReport {
  imported: number
  skipped: number
  failed: MigrationApplyFailure[]
}

export async function migrationScan(source: MigrationSourceId): Promise<MigrationScanResult> {
  return invoke('migration_scan', { source })
}

export async function migrationPreview(
  source: MigrationSourceId,
  items: MigrationItemInput[],
): Promise<MigrationPreviewResult> {
  return invoke('migration_preview', { source, items })
}

export async function migrationApply(
  source: MigrationSourceId,
  items: MigrationItemInput[],
): Promise<MigrationApplyReport> {
  return invoke('migration_apply', { source, items })
}

// ─── Persona / profile pack (P2-2) ─────────────────────────────────────────
//
// Frozen contract with desktop/src/persona_pack_commands.rs: pack Shannon's
// personalization surfaces (skills, commands, memories, routines, profiles,
// persona) into one secret-stripped .tar.gz, preview it, and import it with a
// user-chosen conflict strategy.

/** Category selection shared by export and import; omitted = false. */
export interface PersonaPackInclude {
  skills: boolean
  commands: boolean
  memory: boolean
  routines: boolean
  profiles: boolean
  persona: boolean
}

export interface PersonaPackCounts {
  skills: number
  commands: number
  memories: number
  routines: number
  profiles: number
  persona: number
}

export interface PersonaPackExportResult {
  path: string
  counts: PersonaPackCounts
  /** Total secret redactions applied while packing. */
  stripped: number
}

export type PersonaPackConflict = 'skip' | 'overwrite' | 'rename'

export interface PersonaPackFailure {
  item: string
  error: string
}

export interface PersonaPackImportReport {
  imported: PersonaPackCounts
  skipped: PersonaPackCounts
  failed: PersonaPackFailure[]
}

export interface PersonaPackInspectResult {
  version: number
  counts: PersonaPackCounts
  createdAtMs: number
  generator: string
}

export async function personaPackExport(
  path: string,
  include: PersonaPackInclude,
): Promise<PersonaPackExportResult> {
  return invoke('persona_pack_export', { path, include })
}

export async function personaPackImport(
  path: string,
  conflict: PersonaPackConflict,
  include: PersonaPackInclude,
): Promise<PersonaPackImportReport> {
  return invoke('persona_pack_import', { path, conflict, include })
}

export async function personaPackInspect(path: string): Promise<PersonaPackInspectResult> {
  return invoke('persona_pack_inspect', { path })
}

// --- Routine templates (P1.4) ---

export interface RoutineTemplate {
  id: string
  name: string
  description: string
  category: string
  prompt: string
  trigger_type: string
  cron_expr?: string | null
  interval_secs?: number | null
  github_event?: string | null
  github_repo?: string | null
  github_action?: string | null
  timezone?: string | null
}

export async function listRoutineTemplates(): Promise<RoutineTemplate[]> {
  return invoke('list_routine_templates')
}

export async function instantiateRoutineTemplate(
  templateId: string,
  nameOverride?: string | null,
): Promise<ScheduledRoutine> {
  return invoke('instantiate_routine_template', {
    templateId,
    nameOverride: nameOverride ?? null,
  })
}

// ---------------------------------------------------------------------------
// P2.1 — Persistent memory layer (wraps shannon_core::memory::MemoryStore)
// ---------------------------------------------------------------------------

export type MemoryCategory = 'preference' | 'pattern' | 'decision' | 'error' | 'context'

export interface MemoryEntry {
  id: string
  project: string
  category: MemoryCategory
  content: string
  tags: string[]
  confidence: number
  created_at: string
  accessed_at: string
  access_count: number
  /** P2-4 provenance: session that produced this entry, when known. */
  source_session_id?: string | null
  /** P2-4 provenance: 'manual' | 'import' | 'auto-extract'. */
  source_kind?: string | null
}

export interface MemoryStats {
  total: number
  by_category: Record<string, number>
  by_project: Record<string, number>
  most_recent_at: string | null
}

// --- P2-4 memory provenance + graph ---

export type MemorySourceKind = 'manual' | 'import' | 'auto-extract'

/** Frozen contract payload of `get_memory_source`. */
export interface MemorySource {
  sessionId: string
}

// --- P2-5: injected-memory introspection ("which memories did this turn use") ---

/** One memory entry injected into a session's current context (P2-5). */
export interface InjectedMemory {
  id: string
  /** First line of the entry's content, char-capped for display. */
  title: string
  /** `preference | pattern | decision | error | context`. */
  category: MemoryCategory
  /** Session that produced the entry — the jump target; null = no jump. */
  sourceSessionId?: string | null
}

/** The memories injected into THIS session's current context (same selection
 *  the system prompt uses). Empty when the memory layer is off (including the
 *  session-level "temporary chat" bypass) or nothing qualified. */
export async function getSessionInjectedMemories(
  sessionId: string | null | undefined,
): Promise<InjectedMemory[]> {
  return invoke<InjectedMemory[]>('get_session_injected_memories', {
    sessionId: sessionId ?? '',
  })
}

export interface MemoryGraphNode {
  /** `project:<path>` | `category:<project>|<category>` | `entry:<id>` */
  id: string
  kind: 'project' | 'category' | 'entry'
  label: string
  category?: MemoryCategory | null
  /** Entry count for project/category nodes, confidence for entries. */
  weight: number
  /** Entry tags (empty for project/category nodes). */
  tags?: string[] | null
  sourceKind?: MemorySourceKind | null
  sourceSessionId?: string | null
}

export interface MemoryGraphEdge {
  source: string
  target: string
  /** 'cluster' (project→category→entry) | 'session' (same source session). */
  kind: 'cluster' | 'session'
}

export interface MemoryGraph {
  project: string | null
  nodes: MemoryGraphNode[]
  edges: MemoryGraphEdge[]
  entryCount: number
  maxEntries: number
  truncated: boolean
}

export async function listMemoryProjects(): Promise<string[]> {
  return invoke('list_memory_projects')
}

export async function listMemories(opts?: {
  project?: string | null
  category?: string | null
  query?: string | null
}): Promise<MemoryEntry[]> {
  return invoke('list_memories', {
    project: opts?.project ?? null,
    category: opts?.category ?? null,
    query: opts?.query ?? null,
  })
}

export async function createMemory(input: {
  project: string
  category: string
  content: string
  tags?: string[]
  confidence?: number
}): Promise<MemoryEntry> {
  return invoke('create_memory', input)
}

export async function updateMemory(input: {
  id: string
  content?: string | null
  tags?: string[] | null
  category?: string | null
  /** B3-24 (decision 3-A): moving an entry between projects is a real
   *  backend move now — omit/null keeps the current project. */
  project?: string | null
}): Promise<MemoryEntry> {
  return invoke('update_memory', {
    ...input,
    project: input.project ?? null,
  })
}

export async function deleteMemory(id: string): Promise<boolean> {
  return invoke('delete_memory', { id })
}

export async function searchMemories(query: string, project?: string | null): Promise<MemoryEntry[]> {
  return invoke('search_memories', { query, project: project ?? null })
}

export async function getMemoryStats(): Promise<MemoryStats> {
  return invoke('get_memory_stats')
}

/** Frozen contract (P2-4): source session of a memory, null when untracked. */
export async function getMemorySource(sessionId: string | null, memoryId: string): Promise<MemorySource | null> {
  return invoke('get_memory_source', { sessionId: sessionId ?? '', memoryId })
}

/** P2-4 graph payload for the Memory page's graph view. */
export async function getMemoryGraph(project?: string | null): Promise<MemoryGraph> {
  return invoke('get_memory_graph', { project: project ?? null })
}

// --- Dream Pass (梦境提炼 — review-gated memory distillation) ---
//
// Frozen contract with desktop/src/commands_dream.rs. The Rust DTOs do NOT
// use serde rename_all, so every field below is snake_case on the wire.
// Nothing here writes to ~/.shannon/memories/ directly — the only write
// path is applyDreamProposal (user-approved actions).

/** One merge/remove/add action inside a DreamProposal. */
export interface DreamAction {
  id: string
  kind: 'merge' | 'remove' | 'add'
  /** Memory ids this action targets (the merge/remove group). */
  entry_ids: string[]
  /** Populated for `add` actions only — the proposed new memory. */
  add_entry: {
    category: string
    content: string
    confidence: number
    source_session_ids: string[]
    verified: boolean
  } | null
  rationale: string
}

/// A review-gated distillation proposal for one project (shadow copy under
/// ~/.shannon/dreams/ — applying is the only way it touches real memories).
export interface DreamProposal {
  id: string
  project: string
  created_at: string
  actions: DreamAction[]
}

/// Outcome of one dream pass. `skipped_reason` is null when the pass ran;
/// `"disabled" | "throttled" | "in-progress"` otherwise (nothing was read).
export interface DreamPassResult {
  skipped_reason: 'disabled' | 'throttled' | 'in-progress' | null
  scanned_sessions: number
  projects: string[]
  merge_proposed: number
  remove_proposed: number
  add_proposed: number
  candidates_detected: number
  candidates_refined: number
  proposal_ids: string[]
  report_path: string | null
  duration_ms: number
}

/// Result of applying the selected actions of one proposal. The proposal
/// file is deleted either way — a partial apply discards the rest
/// (“应用所选，其余丢弃”).
export interface DreamApplyOutcome {
  applied: string[]
  skipped: string[]
}

/// Run one dream pass (Memory panel button / `/dream`). `daysBack` defaults
/// to the backend's 3-day manual window when null.
export async function runDreamPass(daysBack?: number | null): Promise<DreamPassResult> {
  return invoke('run_dream_pass', { daysBack: daysBack ?? null })
}

/// Every pending proposal across projects, newest first.
export async function listDreamProposals(): Promise<DreamProposal[]> {
  return invoke('list_dream_proposals')
}

/// One pass report's markdown; `ts = null` reads the newest.
export async function readDreamReport(ts?: string | null): Promise<string> {
  return invoke('read_dream_report', { ts: ts ?? null })
}

/// Apply the selected actions of a proposal to the memory store, then
/// delete the proposal (remaining actions are discarded with it).
export async function applyDreamProposal(proposalId: string, actionIds: string[]): Promise<DreamApplyOutcome> {
  return invoke('apply_dream_proposal', { proposalId, actionIds })
}

/// Discard a proposal without touching the memory store.
export async function discardDreamProposal(proposalId: string): Promise<void> {
  return invoke('discard_dream_proposal', { proposalId })
}

/// Full counters of one completed pass, as persisted in the shared
/// detection-state file (`DreamState.last_stats`). `Partial` on the wire —
/// the file is shared and a writer may have recorded only the timestamp.
export interface DreamPassStats {
  scanned_sessions: number
  entries_reviewed: number
  merge_proposed: number
  remove_proposed: number
  add_proposed: number
  candidates_detected: number
  candidates_refined: number
  redactions_applied: number
  duration_ms: number
  projects: string[]
  token_estimate: number
}

/// Persisted dream state (read_dream_state): the last pass's timestamp and
/// stats, for cold-start display. Both fields null when no pass ever ran.
export interface DreamState {
  last_dream_at: string | null
  last_stats: Partial<DreamPassStats> | null
}

/// Read the persisted dream state — the cold-start 「上次提炼」 line's data.
export async function readDreamState(): Promise<DreamState> {
  return invoke('read_dream_state')
}

/// `/detect-skills` backend — heuristic pattern detection only (zero LLM),
/// bypasses dream throttles by design. Returns the number of newly appended
/// candidates (dedup by the backend's sig-hash id).
export async function detectSkillsSlash(): Promise<number> {
  return invoke('detect_slash')
}

// --- Skill Loop (E2) ---

export const skillLoop = {
  evaluate: (evaluation: TaskEvaluation) =>
    invoke<EvaluationResult>('skill_loop_evaluate', { evaluation }),

  generate: (evaluation: TaskEvaluation) =>
    invoke<SkillProposal>('skill_loop_generate', { evaluation }),

  listProposals: () =>
    invoke<SkillProposal[]>('skill_loop_list_proposals'),

  approve: (proposalId: string) =>
    invoke<string>('skill_loop_approve', { proposalId }),

  reject: (proposalId: string) =>
    invoke<void>('skill_loop_reject', { proposalId }),
}

// --- Speech-to-text (D4 voice input) ---

export async function transcribeAudio(
  audioBase64: string,
  mimeType: string,
  language?: string | null,
): Promise<TranscriptionResult> {
  return invoke('transcribe_audio', {
    audioBase64,
    mimeType,
    language: language ?? null,
  })
}

export async function getSttConfig(): Promise<SttConfig | null> {
  return invoke('get_stt_config')
}

export async function saveSttConfig(sttConfig: SttConfig): Promise<void> {
  await invoke('save_stt_config', { sttConfig })
}

// --- P2-5e: local whisper-rs STT (offline / privacy) ---

/** Wire shape for `get_voice_local_config` / `save_voice_local_config`.
 *  Mirrors the Rust `VoiceLocalConfig` struct. */
export interface VoiceLocalConfig {
  enabled: boolean
  /** Model slug: `tiny.en` | `base` | `small`. `null` ⇒ auto-pick
   *  the smallest downloaded model at call time. */
  model: string | null
  /** BCP-47 language hint for whisper-rs. `null` ⇒ auto-detect. */
  language: string | null
  /** When true (default), a missing model is auto-downloaded on
   *  first use. When false, the user must download explicitly
   *  from Settings → Voice. */
  auto_download: boolean
}

/** Catalog entry from `list_whisper_models`. */
export interface WhisperModelInfo {
  model: string
  filename: string
  approx_size_mb: number
  downloaded: boolean
  verified: boolean
  size_bytes: number | null
}

export async function getVoiceLocalConfig(): Promise<VoiceLocalConfig> {
  return invoke('get_voice_local_config')
}

export async function saveVoiceLocalConfig(
  voiceLocal: VoiceLocalConfig,
): Promise<void> {
  await invoke('save_voice_local_config', { voiceLocal })
}

export async function listWhisperModels(): Promise<WhisperModelInfo[]> {
  return invoke('list_whisper_models')
}

/** Start (or restart) a model download. Resolves to the final
 *  on-disk path; throws `STT_DOWNLOAD_FAILED: ...` on failure.
 *  Subscribe to the `voice:model-download-progress` Tauri event
 *  for live progress updates. */
export async function downloadWhisperModel(
  model: string,
): Promise<string> {
  return invoke('download_whisper_model', { model })
}

/** Delete a downloaded model. `true` if the file was present,
 *  `false` if it wasn't there. */
export async function deleteWhisperModel(model: string): Promise<boolean> {
  return invoke('delete_whisper_model', { model })
}

/** Transcribe a WAV file at the given path with the local
 *  whisper-rs model. Throws `STT_*:` on failure (mapped to typed
 *  toast codes by the caller). Path-based; tests + power users
 *  can write the file themselves via the desktop's `fs` plugin. */
export async function transcribeAudioLocal(
  audioPath: string,
  model: string | null,
  language?: string | null,
): Promise<TranscriptionResult> {
  return invoke('transcribe_audio_local', {
    audioPath,
    model,
    language: language ?? null,
  })
}

/** Same as the cloud `transcribeAudio` — takes a base64 audio
 *  blob + mime. The Rust side writes the bytes to a temp file
 *  (only `audio/wav` is supported by the local path) and runs
 *  inference locally. This is what the `localProvider` actually
 *  invokes. */
export async function transcribeAudioLocalBase64(
  audioBase64: string,
  mimeType: string,
  model: string | null,
  language?: string | null,
): Promise<TranscriptionResult> {
  return invoke('transcribe_audio_local_base64', {
    audioBase64,
    mimeType,
    model,
    language: language ?? null,
  })
}


// --- Remote targets (SSH hosts / Docker containers) ---

/**
 * Response of `remote_list_targets`: the saved targets plus the persisted
 * default (P1-16 — the UI reads it back instead of treating reloads as a
 * no-op).
 */
export interface RemoteTargetsList {
  targets: RemoteTargetListItem[]
  defaultTarget: string | null
}

/** List saved remote targets (and the default) from ~/.shannon/remotes.toml. */
export async function remoteListTargets(): Promise<RemoteTargetsList> {
  return invoke('remote_list_targets')
}

/** Discover SSH host candidates from ~/.ssh/config (read-only). */
export async function remoteDiscoverSshHosts(): Promise<SshHostCandidate[]> {
  return invoke('remote_discover_ssh_hosts')
}

/** List running Docker containers (best-effort). */
export async function remoteListDockerContainers(): Promise<ContainerInfo[]> {
  return invoke('remote_list_docker_containers')
}

/** Add or replace a remote target (validated server-side). */
export async function remoteAddTarget(target: RemoteTargetListItem): Promise<void> {
  await invoke('remote_add_target', { target })
}

/** Remove a remote target by name. */
export async function remoteRemoveTarget(name: string): Promise<void> {
  await invoke('remote_remove_target', { name })
}

/** Set (or clear with null) the default target for new sessions. */
export async function remoteSetDefaultTarget(name: string | null): Promise<void> {
  await invoke('remote_set_default_target', { name })
}

/** Probe a target's connectivity and platform facts. */
export async function remoteTestTarget(name: string): Promise<RemoteHealth> {
  return invoke('remote_test_target', { name })
}

// Dev-server preview (P1-5 C-1 — live preview in the artifact panel).
// `projectDir` may be omitted: the backend then resolves the current
// session's working directory (desktop config mirror).

/** A detected dev-server launch recipe (frozen backend contract). */
export interface PreviewDevServerInfo {
  command: string
  url: string
}

export interface PreviewDetectResponse {
  devServer: PreviewDevServerInfo | null
}

export interface PreviewStatusResponse {
  running: boolean
  url: string | null
  startedAtMs: number | null
}

export interface PreviewLogLine {
  tsMs: number
  /** `stdout` | `stderr` | `system` */
  stream: string
  text: string
}

export interface PreviewCaptureResponse {
  imageBase64: string
  mediaType: string
  width: number
  height: number
  /** Set when the whole monitor was captured instead of the app window. */
  fallback?: string
}

export async function previewDetect(projectDir?: string | null): Promise<PreviewDetectResponse> {
  return invoke('preview_detect', { projectDir: projectDir ?? null })
}

export async function previewStart(projectDir?: string | null): Promise<{ url: string }> {
  return invoke('preview_start', { projectDir: projectDir ?? null })
}

export async function previewStop(): Promise<void> {
  await invoke('preview_stop')
}

export async function previewStatus(): Promise<PreviewStatusResponse> {
  return invoke('preview_status')
}

export async function previewCapture(): Promise<PreviewCaptureResponse> {
  return invoke('preview_capture')
}

export async function previewLogs(limit?: number | null): Promise<PreviewLogLine[]> {
  return invoke('preview_logs', { limit: limit ?? null })
}

// Integrated terminal (P1-5 D — frozen contract). `data` on the wire is
// UTF-8 (xterm.js onData output incl. control bytes); PTY output arrives
// base64-encoded on the `terminal:output` event (see runtime/terminalEvents).

/** Spawn a PTY session rooted at `projectDir` (≤4 live instances). */
export async function terminalSpawn(projectDir?: string | null, shell?: string): Promise<{ terminalId: string }> {
  return invoke('terminal_spawn', { projectDir: projectDir ?? null, shell: shell ?? null })
}

/** Write to the terminal's stdin (keystrokes, paste, control bytes). */
export async function terminalWrite(terminalId: string, data: string): Promise<void> {
  await invoke('terminal_write', { terminalId, data })
}

/** Resize the pty (cols/rows from the fit addon). */
export async function terminalResize(terminalId: string, cols: number, rows: number): Promise<void> {
  await invoke('terminal_resize', { terminalId, cols, rows })
}

/** Kill the terminal's whole process tree. */
export async function terminalKill(terminalId: string): Promise<void> {
  await invoke('terminal_kill', { terminalId })
}

/** Live terminals, oldest first. */
export async function terminalList(): Promise<TerminalInfo[]> {
  return invoke('terminal_list')
}

/**
 * P3-1: persisted terminal preferences (`[terminal]` in
 * `~/.shannon/config.toml`). The backend clamps numerics (fontSize 8–32,
 * scrollback 0–100000, drawerHeight 120–1200) and blanks the shell —
 * callers must render the values returned here, not what they sent.
 */
export async function terminalGetSettings(): Promise<TerminalSettings> {
  return invoke('terminal_get_settings')
}

/** Persist preferences; returns the sanitized (effective) values. */
export async function terminalSetSettings(settings: TerminalSettings): Promise<TerminalSettings> {
  return invoke('terminal_set_settings', { settings })
}

/**
 * Replay bytes for one session, base64 (US6). Empty string when the id is
 * unknown or the session already ended — the frontend calls it
 * speculatively on reconnect, so a missing ring must not be an error.
 * `endSeq` (additive, review fix) is the highest output-chunk seq fully
 * contained in `data`: the replay consumer drops queued `terminal:output`
 * events with `seq <= endSeq` and flushes the rest, so the snapshot and
 * the live stream stitch without loss or duplication. Absent on the demo
 * backend → flush-everything fallback.
 *
 * `truncated` (additive) is true when the backend's 1 MiB replay ring
 * evicted older bytes: `data` is only the newest tail, and the replay
 * consumer prepends an in-stream dim notice so the gap is visible.
 * Absent on legacy/demo payloads → treated as false (nothing known lost).
 */
export async function terminalHistory(terminalId: string): Promise<{ data: string; endSeq?: number; truncated?: boolean }> {
  return invoke('terminal_history', { terminalId })
}

// --- P-E3 project registry (projects.db, adopt-not-migrate) ---

/** Every registered project, path-ascending. Archived rows are included
 *  only with `includeArchived`. The registry is back-filled from session
 *  working dirs (and, on first seed, memory project labels) before the
 *  read, so a fresh install already knows its projects. */
export async function listProjects(includeArchived?: boolean): Promise<ProjectRecord[]> {
  return invoke('list_projects', { includeArchived: includeArchived ?? false })
}

/** Register a project path. Idempotent: an already-registered path (any
 *  name, archived or not) is returned unchanged — registration never
 *  overwrites an existing row. */
export async function registerProject(path: string): Promise<ProjectRecord> {
  return invoke('register_project', { path })
}

/** Set a project's custom display name (`null` clears it, falling back to
 *  the path's tail segment in the UI). */
export async function renameProject(path: string, name: string | null): Promise<ProjectRecord> {
  return invoke('rename_project', { path, name })
}

/** Set a project's custom icon and color (`null` clears a field). */
export async function setProjectAppearance(
  path: string,
  icon: string | null,
  color: string | null,
): Promise<ProjectRecord> {
  return invoke('set_project_appearance', { path, icon, color })
}

/** Archive a project (stamps archivedAtMs; hidden from the default list). */
export async function archiveProject(path: string): Promise<ProjectRecord> {
  return invoke('archive_project', { path })
}

/** Unarchive a project (clears archivedAtMs). */
export async function unarchiveProject(path: string): Promise<ProjectRecord> {
  return invoke('unarchive_project', { path })
}
