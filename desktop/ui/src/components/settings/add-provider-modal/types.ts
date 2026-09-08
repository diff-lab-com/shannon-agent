// Add/Edit Provider modal — shared types, lookup tables, and pure helpers.
// Extracted from AddProviderModal.tsx (T3.1). Kept as a separate dir
// (add-provider-modal/) so the orchestrator and its sub-components can be
// imported in isolation, and so the modal's KIND_INFO does not collide with
// the ModelsSettings KIND_INFO copy under models-settings/.
import type { useIntl } from 'react-intl'
import type { ProviderConnection, ProviderKind } from '@/types'

export interface KindInfo {
  labelKey: string
  icon: string
  baseUrlRequired: boolean
  needsKey: boolean
}

export const KIND_INFO: Record<string, KindInfo> = {
  anthropic: { labelKey: 'settings.models.providers.kinds.anthropic', icon: 'auto_awesome', baseUrlRequired: false, needsKey: true },
  openai: { labelKey: 'settings.models.providers.kinds.openai', icon: 'bolt', baseUrlRequired: false, needsKey: true },
  deepseek: { labelKey: 'settings.models.providers.kinds.deepseek', icon: 'psychology', baseUrlRequired: false, needsKey: true },
  ollama: { labelKey: 'settings.models.providers.kinds.ollama', icon: 'dns', baseUrlRequired: false, needsKey: false },
  'openai-compatible': { labelKey: 'settings.models.providers.kinds.openaiCompatible', icon: 'hub', baseUrlRequired: true, needsKey: true },
}

export interface QuickFill {
  id: string
  label: string
  icon: string
  kind: ProviderKind
  baseUrl?: string
  model?: string
}

// Quick-fill chips in the Add/Edit modal. The built-in providers map to their
// kind; GLM / Kimi / MiniMax are OpenAI-compatible endpoints (kind =
// `openai-compatible`), which the Rust layer tests with a Bearer token.
export const QUICK_FILL: QuickFill[] = [
  { id: 'anthropic', label: 'Anthropic', icon: 'auto_awesome', kind: 'anthropic', model: 'claude-sonnet-4-6' },
  { id: 'openai', label: 'OpenAI', icon: 'bolt', kind: 'openai', model: 'gpt-4.1-mini' },
  { id: 'deepseek', label: 'DeepSeek', icon: 'psychology', kind: 'deepseek', model: 'deepseek-chat' },
  { id: 'glm', label: 'GLM (Zhipu)', icon: 'auto_awesome', kind: 'openai-compatible', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-plus' },
  { id: 'kimi', label: 'Kimi (Moonshot)', icon: 'dark_mode', kind: 'openai-compatible', baseUrl: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
  { id: 'minimax', label: 'MiniMax', icon: 'group', kind: 'openai-compatible', baseUrl: 'https://api.minimax.chat/v1', model: 'abab6.5s-chat' },
  { id: 'ollama', label: 'Ollama (local)', icon: 'dns', kind: 'ollama', baseUrl: 'http://localhost:11434', model: 'llama3.2' },
  { id: 'custom', label: 'settings.models.providers.customOpenAI', icon: 'hub', kind: 'openai-compatible' },
]

export function kindLabel(intl: ReturnType<typeof useIntl>, kind: string): string {
  return intl.formatMessage({ id: KIND_INFO[kind]?.labelKey ?? 'settings.models.providers.kinds.openaiCompatible' })
}

/// Key/value pair for the `extra_headers` advanced row. An empty `key` row
/// is silently dropped at submit time so the engine never sees `""`.
export interface HeaderRow {
  key: string
  value: string
}

/// Internal state for the advanced disclosure. Kept as plain state — the
/// payload only needs the final `Record<string, string>`, `number | null`,
/// and `Tiers` shape; this view-model exists to keep the JSX simple.
export interface AdvancedState {
  headers: HeaderRow[]
  defaultMaxTokensInput: string
  tiers: { fast: string; standard: string; pro: string }
  fallbackModels: string[]
}

export function advancedFromEditing(editing: ProviderConnection | null): AdvancedState {
  if (!editing) {
    return {
      headers: [],
      defaultMaxTokensInput: '',
      tiers: { fast: '', standard: '', pro: '' },
      fallbackModels: [],
    }
  }
  return {
    headers: Object.entries(editing.extra_headers ?? {}).map(([key, value]) => ({ key, value })),
    defaultMaxTokensInput: editing.default_max_tokens == null ? '' : String(editing.default_max_tokens),
    tiers: {
      fast: editing.tiers?.fast ?? '',
      standard: editing.tiers?.standard ?? '',
      pro: editing.tiers?.pro ?? '',
    },
    fallbackModels: editing.fallback_models ? [...editing.fallback_models] : [],
  }
}

export function headersToRecord(rows: HeaderRow[]): Record<string, string> {
  const out: Record<string, string> = {}
  for (const r of rows) {
    const k = r.key.trim()
    if (!k) continue
    out[k] = r.value
  }
  return out
}

export function parseDefaultMaxTokens(s: string): number | null {
  const trimmed = s.trim()
  if (!trimmed) return null
  const n = Number(trimmed)
  if (!Number.isFinite(n) || n <= 0) return null
  return Math.floor(n)
}
