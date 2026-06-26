// TypeScript types matching Rust structs in shannon-desktop/src/events.rs and commands.rs

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
}

export interface QueryCompletedPayload {
  query_id: string
}

export interface QueryFailedPayload {
  query_id: string
  error: string
}

export interface PermissionRequest {
  tool: string
  input: unknown
  risk: string
  request_id: string
}

// --- Core Types ---

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
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
  context_window: number
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

export interface ProviderSwitchRequest {
  provider: string
  api_key?: string
  base_url?: string
  model: string
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
}

export interface SendMessageResponse {
  query_id: string
}

// --- Diff Types ---

export interface FileDiff {
  old_content: string
  new_content: string
  file_name: string
  language: string
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

// --- Billing Types ---

export interface BillingPlan {
  name: string
  price: number
  token_limit: number
  features: string[]
}

export interface CostRecord {
  date: string
  input_tokens: number
  output_tokens: number
  cost_usd: number
}

export interface BillingHistory {
  id: string
  date: string
  description: string
  amount: number
  status: 'paid' | 'pending' | 'failed'
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
  /// Replaces dependency list. Send the full list (add or remove); empty clears.
  depends_on?: string[]
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

// --- Event Names ---

export const EVENT_NAMES = {
  QUERY_TEXT: 'query:text',
  QUERY_TOOL_START: 'query:tool-start',
  QUERY_TOOL_RESULT: 'query:tool-result',
  QUERY_TOOL_PROGRESS: 'query:tool-progress',
  QUERY_THINKING: 'query:thinking',
  QUERY_USAGE: 'query:usage',
  QUERY_COMPLETED: 'query:completed',
  QUERY_FAILED: 'query:failed',
  QUERY_CANCELLED: 'query:cancelled',
  PERMISSION_REQUEST: 'permission-request',
  SESSIONS_UPDATED: 'sessions-updated',
  SESSION_LOADED: 'session-loaded',
  CONFIG_UPDATED: 'config-updated',
  DIFF_REVIEW_AVAILABLE: 'diff-review-available',
  BACKGROUND_TASK_UPDATE: 'background-task-update',
  BACKGROUND_TASKS_UPDATED: 'background-tasks-updated',
} as const

export type EventName = (typeof EVENT_NAMES)[keyof typeof EVENT_NAMES]

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
