// Pure types, constants, and small helpers shared by ModelsSettings sub-components.
// No React state, no side effects — kept here so individual sub-components can
// import them without dragging the orchestrator along.

import type { useIntl } from 'react-intl'
import type { ModelInfo } from '@/types'

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
  gemini: { labelKey: 'settings.models.providers.kinds.gemini', icon: 'spark', baseUrlRequired: false, needsKey: true },
  'openai-compatible': { labelKey: 'settings.models.providers.kinds.openaiCompatible', icon: 'hub', baseUrlRequired: true, needsKey: true },
}

export function kindLabel(intl: ReturnType<typeof useIntl>, kind: string): string {
  return intl.formatMessage({ id: KIND_INFO[kind]?.labelKey ?? 'settings.models.providers.kinds.openaiCompatible' })
}

/**
 * Format a per-million-token USD price for the model list. Returns the
 * i18n "unknown" placeholder for null / non-finite values so the UI
 * never invents a number (ADR-0005 P0-2 honest-cost). Two decimals
 * are enough resolution for a sidebar; the engine pricing SSOT is the
 * canonical source.
 */
export function formatPrice(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) {
    return '—'
  }
  return value.toFixed(2)
}

/**
 * R2-3 — compact single-line meta suffix for the composer picker's model
 * rows: context window ("200k") + per-million-token prices ("$3.00/$15.00"),
 * reusing `formatPrice` so the picker and the Settings catalog list render
 * identically. Unknown values render as "—" on their side — never fabricated
 * (ADR-0005 P0-2 honest cost/context). Pure — vitest-covered.
 */
export function modelPickerMeta(
  m: Pick<ModelInfo, 'context_window' | 'price_in' | 'price_out'>,
): string {
  const context = m.context_window > 0 ? `${(m.context_window / 1000).toFixed(0)}k` : '—'
  return `${context} · $${formatPrice(m.price_in)}/$${formatPrice(m.price_out)}`
}

// === Provider visibility (ADR-0005 P4.9) ===
//
// Settings panel that toggles which provider kinds appear in the model
// picker. Backed by `DesktopConfig.enabled_providers` (persisted to
// `~/.shannon/desktop/config.json`); the engine's `SHANNON_*_PROVIDERS`
// env vars are honoured only when the override is `null`.
//
// `null` (no override) is rendered as every checkbox checked — the user
// sees the engine env-var state would apply, even though no explicit
// state is persisted. `[]` is rendered as none checked. Otherwise only
// the listed slugs are checked.

export const PROVIDER_KINDS_FOR_VISIBILITY = [
  'anthropic',
  'openai',
  'deepseek',
  'ollama',
  'gemini',
  'openai-compatible',
] as const