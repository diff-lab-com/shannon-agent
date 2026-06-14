import { invoke } from '@tauri-apps/api/core'
import type {
  ChatMessage,
  StatusResponse,
  ModelInfo,
  ToolInfo,
  ConfigUpdate,
  ProviderSwitchRequest,
  DesktopConfig,
  SendMessageResponse,
  HunkAction,
  SessionInfo,
  McpServerInfo,
  McpServerConfig,
  SkillInfo,
  SkillDetail,
  TaskItem,
  BackgroundTaskInfo,
  AgentInfo,
  FileDiff,
  FileNode,
  WorkingDirInfo,
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

export async function switchProvider(req: ProviderSwitchRequest): Promise<void> {
  await invoke('switch_provider', { request: req })
}

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

export async function deleteSession(id: string): Promise<boolean> {
  return invoke('delete_session', { id })
}

export async function renameSession(id: string, title: string): Promise<boolean> {
  return invoke('rename_session', { id, title })
}

export async function duplicateSession(id: string): Promise<SessionInfo> {
  return invoke('duplicate_session', { id })
}

export async function exportSession(id: string, format: 'markdown' | 'json'): Promise<string> {
  return invoke('export_session', { id, format })
}

// --- Permissions ---

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

export async function listPluginMarketplace(): Promise<unknown[]> {
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

// --- File Context ---

export async function getFileContext(): Promise<import('@/types').FileContext[]> {
  return invoke('get_file_context')
}

// --- Task Detail ---

export async function getTaskDetail(id: string): Promise<TaskItem> {
  return invoke('get_task_detail', { id })
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
