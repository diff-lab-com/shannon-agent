// S3-3 — utility tier slots: the two auxiliary background-task model slots
// (compaction, session summary) and their pure display derivations.
//
// The BACKEND is authoritative: `desktop/src/utility_tier.rs` owns the slot
// vocabulary (the schema's snake_case role names), the roster candidates and
// the fallback semantics; this module only mirrors the two in-scope roles so
// the Settings section renders the same state the backend reads. The slots
// live OUTSIDE the interactive precedence chain (session override > phase
// tier > global default) by design (review裁定⑦) — they serve the
// background-task channel only.

import type { UtilitySlotStatus } from '@/lib/tauri-api'

/** The two slots of the batch ruling (裁定②), in display order:
 *  compaction first (the consumed slot — the agent loop's background
 *  summarization request), session summary second. */
export interface UtilitySlotDef {
  /** Stable test/ DOM id fragment. */
  id: 'compaction' | 'summary'
  /** The backend role slug (the schema's serde name). */
  role: 'compression' | 'title_generation'
  /** i18n key of the slot label. */
  labelKey: string
}

/** A slot's stable id (the DOM/test identifier fragment). */
export type UtilitySlotId = UtilitySlotDef['id']

export const UTILITY_SLOTS: readonly UtilitySlotDef[] = [
  { id: 'compaction', role: 'compression', labelKey: 'settings.models.utility.compaction' },
  { id: 'summary', role: 'title_generation', labelKey: 'settings.models.utility.summary' },
] as const

/** Option-value encoding of one slot selection: `provider::model`, or the
 *  empty string for "follow the global default" (the select's blank option).
 *  `::` is safe — provider slot ids are slug tokens, never contain colons. */
export function utilitySlotValueOf(status: UtilitySlotStatus | undefined): string {
  return status?.provider && status?.model ? `${status.provider}::${status.model}` : ''
}

/** Lenient split of an option value back to its (provider, model) pair, or
 *  null for the follow-default blank. Malformed values (hand-crafted DOM
 *  events) degrade to null — a broken selection must never write a partial
 *  target. */
export function utilitySlotValueParts(
  value: string,
): { provider: string; model: string } | null {
  if (!value) return null
  const idx = value.indexOf('::')
  if (idx <= 0 || idx === value.length - 2) return null
  const provider = value.slice(0, idx)
  const model = value.slice(idx + 2)
  return provider && model ? { provider, model } : null
}
