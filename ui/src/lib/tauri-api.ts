import { invoke } from '@tauri-apps/api/core'
import type {
  ChatMessage,
  StatusResponse,
  ModelInfo,
  ToolInfo,
  ConfigUpdate,
  ProviderSwitchRequest,
  ProviderConnection,
  ProvidersFile,
  ProviderInput,
  DesktopConfig,
  SendMessageResponse,
  HunkAction,
  SessionInfo,
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
  WorkingDirInfo,
  CatalogEntry,
  DataSourceResult,
} from '@/types'
import type {
  ScheduledRoutine,
  CreateTaskPayload,
  UpdateTaskPayload,
  CronPreview,
  TriageItem,
  TriageFilter,
  TriageStats,
  TaskExecution,
  TaskExecutionDetail,
  TriggeredRoutineDto,
  TriggerResponse,
  TaskWorktreeDto,
} from '@/types'

// --- Chat ---

export async function sendMessage(message: string, filePaths?: string[]): Promise<SendMessageResponse> {
  return invoke('send_message', { message, filePaths: filePaths ?? null })
}

export async function getConversation(): Promise<ChatMessage[]> {
  return invoke('get_conversation')
}

export async function cancelQuery(): Promise<void> {
  await invoke('cancel_query')
}

// --- Config ---

export async function getConfig(): Promise<DesktopConfig> {
  return invoke('get_config')
}

export async function configure(update: ConfigUpdate): Promise<void> {
  await invoke('configure', { update })
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

export interface SlackInboundDto {
  bot_token: string
  trigger_word: string
  allowed_channels: string[]
}

export interface TelegramInboundDto {
  bot_token: string
  trigger_word: string
  allowed_chats: string[]
}

export interface InboundConfigDto {
  slack?: SlackInboundDto | null
  telegram?: TelegramInboundDto | null
}

export async function getInboundConfig(): Promise<InboundConfigDto> {
  return invoke('get_inbound_config')
}

export async function saveInboundConfig(dto: InboundConfigDto): Promise<void> {
  await invoke('save_inbound_config', { dto })
}

export async function clearInboundConfig(): Promise<void> {
  await invoke('clear_inbound_config')
}

export interface InboundListenerStatus {
  telegram_running: boolean
  slack_running: boolean
}

export async function getInboundListenerStatus(): Promise<InboundListenerStatus> {
  return invoke('get_inbound_listener_status')
}

export async function stopInboundListener(): Promise<void> {
  await invoke('stop_inbound_listener')
}

export interface SlackOutboundDto {
  bot_token: string
  channel: string
}

export interface TelegramOutboundDto {
  bot_token: string
  chat_id: string
}

export interface OutboundConfigDto {
  slack?: SlackOutboundDto | null
  telegram?: TelegramOutboundDto | null
}

export interface ChannelResult {
  provider: string
  ok: boolean
  error?: string | null
}

export interface SendResultDto {
  results: ChannelResult[]
}

export async function getOutboundConfig(): Promise<OutboundConfigDto> {
  return invoke('get_outbound_config')
}

export async function saveOutboundConfig(dto: OutboundConfigDto): Promise<void> {
  await invoke('save_outbound_config', { dto })
}

export async function clearOutboundConfig(): Promise<void> {
  await invoke('clear_outbound_config')
}

export async function sendOutboundTest(message: string): Promise<SendResultDto> {
  return invoke('send_outbound_test', { message })
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

/** Desktop-notification preferences — master enable + quiet-hours (DND) window. */
export interface NotificationPrefs {
  master_enabled: boolean
  dnd_enabled: boolean
  /** `"HH:MM"` (24h, system-local) or null. */
  dnd_start: string | null
  dnd_end: string | null
}

export async function getNotificationPrefs(): Promise<NotificationPrefs> {
  return invoke('get_notification_prefs')
}

export async function setNotificationPrefs(prefs: NotificationPrefs): Promise<void> {
  await invoke('set_notification_prefs', { prefs })
}

export interface InboundMessage {
  provider: 'slack' | 'telegram'
  source_id: string
  source_name: string
  sender_id: string
  sender_name: string
  text: string
  timestamp: number
}

export async function switchProvider(req: ProviderSwitchRequest): Promise<void> {
  await invoke('switch_provider', { request: req })
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
  | { kind: 'provider_error'; status: number }
  | { kind: 'network_unreachable' }
  | { kind: 'unknown'; message: string }

export async function testProviderConnection(
  provider: string,
  apiKey: string,
  baseUrl?: string,
): Promise<TestConnectionResult> {
  return invoke('test_provider_connection', { provider, apiKey, baseUrl })
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

export type { ProviderConnection, ProvidersFile, ProviderInput }

// --- Models & Status ---

export async function listModels(): Promise<ModelInfo[]> {
  return invoke('list_models')
}

export async function getStatus(): Promise<StatusResponse> {
  return invoke('get_status')
}

export async function getTools(): Promise<ToolInfo[]> {
  return invoke('list_tools')
}

// --- Sessions ---

export async function newSession(): Promise<string> {
  return invoke('new_session')
}

export async function listSessions(): Promise<SessionInfo[]> {
  return invoke('list_sessions')
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

export async function setSessionWorkingDir(id: string, path: string): Promise<void> {
  await invoke('set_session_working_dir', { id, path })
}

export async function createSessionWorktree(id: string, title: string): Promise<TaskWorktreeDto> {
  return invoke('create_session_worktree', { id, title })
}

export async function deleteSession(id: string): Promise<boolean> {
  return invoke('delete_session', { id })
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
export async function saveTextFile(path: string, content: string): Promise<void> {
  await invoke('save_text_file', { path, content })
}

// --- Permissions ---

export async function requestPermission(tool: string, input: unknown, risk: string): Promise<boolean> {
  return invoke('request_permission', { tool, input, risk })
}

export async function respondPermission(requestId: string, allow: boolean, note?: string): Promise<void> {
  await invoke('respond_permission', { requestId, allow, note: note ?? null })
}

// --- Files & Diffs ---

export async function getFileDiff(path: string): Promise<FileDiff> {
  return invoke('get_file_diff', { path })
}

export async function applyDiff(filePath: string, hunks: HunkAction[]): Promise<void> {
  return invoke('apply_diff', { filePath, hunks })
}

export async function getFileTree(path: string): Promise<FileNode> {
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

export async function installNativeAgent(
  pluginName: string,
  body: string,
): Promise<InstallResult> {
  return invoke('install_native_agent', { pluginName, body })
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
}

export async function listPlugins(): Promise<PluginInfo[]> {
  return invoke('list_plugins')
}

export async function installPlugin(sourcePath: string): Promise<string> {
  return invoke('install_plugin', { sourcePath })
}

export async function installPluginFromGit(repoUrl: string): Promise<string> {
  return invoke('install_plugin_from_git', { repoUrl })
}

export async function uninstallPlugin(name: string): Promise<void> {
  await invoke('uninstall_plugin', { name })
}

export async function enablePlugin(name: string): Promise<void> {
  await invoke('enable_plugin', { name })
}

export async function disablePlugin(name: string): Promise<void> {
  await invoke('disable_plugin', { name })
}

export async function updatePlugin(name: string): Promise<void> {
  await invoke('update_plugin', { name })
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
): Promise<import('@/types').AgentMessageEntry[]> {
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

export async function updateTask(payload: import('@/types').UpdateTaskPayload): Promise<TaskItem> {
  return invoke('update_task', { payload })
}

// --- Billing ---

export async function getBillingPlan(): Promise<import('@/types').BillingPlan> {
  return invoke('get_billing_plan')
}

export async function getCostHistory(days: number): Promise<import('@/types').CostRecord[]> {
  return invoke('get_cost_history', { days })
}

export async function getBillingHistory(): Promise<import('@/types').BillingHistory[]> {
  return invoke('get_billing_history')
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

export async function toggleScheduledTask(id: string, enabled: boolean): Promise<ScheduledRoutine> {
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

// History

export async function listTaskExecutions(taskId?: string, limit?: number): Promise<TaskExecution[]> {
  return invoke('list_task_executions', { taskId: taskId ?? null, limit: limit ?? null })
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

export async function listHookEvents(): Promise<import('@/types').HookEventInfo[]> {
  return invoke('list_hook_events')
}

export async function listPermissionProfiles(): Promise<import('@/types').ProfilesList> {
  return invoke('list_permission_profiles')
}

export async function saveCustomProfile(payload: {
  name: string
  description?: string
  auto_approve: string[]
  confirm: string[]
  deny: string[]
}): Promise<import('@/types').CustomProfileInfo> {
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

export async function getOpcMetrics(): Promise<import('@/types').OpcMetrics> {
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
}

export interface MemoryStats {
  total: number
  by_category: Record<string, number>
  by_project: Record<string, number>
  most_recent_at: string | null
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
}): Promise<MemoryEntry> {
  return invoke('update_memory', input)
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

// --- Skill Loop (E2) ---

export const skillLoop = {
  evaluate: (evaluation: import('@/types').TaskEvaluation) =>
    invoke<import('@/types').EvaluationResult>('skill_loop_evaluate', { evaluation }),

  generate: (evaluation: import('@/types').TaskEvaluation) =>
    invoke<import('@/types').SkillProposal>('skill_loop_generate', { evaluation }),

  listProposals: () =>
    invoke<import('@/types').SkillProposal[]>('skill_loop_list_proposals'),

  approve: (proposalId: string) =>
    invoke<string>('skill_loop_approve', { proposalId }),

  reject: (proposalId: string) =>
    invoke<void>('skill_loop_reject', { proposalId }),
}

