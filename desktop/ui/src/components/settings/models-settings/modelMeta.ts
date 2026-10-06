// S2-2 (per-model metadata editor, review P-N6 / R2-4 收尾) — pure editing
// logic for the curated vault's declarations.
//
// The engine side is already complete (providers.toml v2 `ModelSpec` +
// `set_provider_models`): declared pricing feeds billing (both halves
// required), declared context feeds compaction budgets, declared
// capabilities feed tier badges and the vision gate. What was missing is
// the desktop half — a way to author those declarations outside the TOML.
//
// Contract notes pinned here (mirrors of `ModelSpec::validate` +
// `ProviderConfigStore::set_provider_models` overwrite semantics):
//   - a token limit must be a positive whole number (u32) or blank (= clear
//     the declaration; the catalog value applies again),
//   - a price must be finite and >= 0 or blank,
//   - `set_provider_models` REPLACES a provider's vault wholesale and does
//     NOT field-merge — an edit must be projected onto the full vault
//     client-side (`upsertVaultModel`), never sent as a lone field.
//
// Pure + unit-tested (`__tests__/modelMeta.test.ts`); no React, no API.

import type { DeclaredModelInput, DeclaredModelSpec, ProvidersFile } from '@/types'

/** Editable draft fields (raw input strings — '' means "clear on save"). */
export interface ModelMetaDraft {
  contextWindow: string
  maxOutput: string
  priceIn: string
  priceOut: string
  vision: boolean
  tools: boolean
}

/** Draft fields that can carry a validation error → the i18n message id. */
export type ModelMetaErrors = Partial<
  Record<'contextWindow' | 'maxOutput' | 'priceIn' | 'priceOut', string>
>

export const MODEL_META_ERROR_KEYS = {
  contextWindow: 'settings.models.vault.errContextWindow',
  maxOutput: 'settings.models.vault.errMaxOutput',
  priceIn: 'settings.models.vault.errPriceIn',
  priceOut: 'settings.models.vault.errPriceOut',
} as const

/** The editor manages the vision/tool_use bits; any other capability names
 *  already on the declaration (reasoning/coding/…) ride along untouched. */
const EDITOR_MANAGED_CAPS = ['vision', 'tool_use'] as const

/** Build the initial draft from a declaration (blank when undeclared). */
export function draftFromSpec(spec?: DeclaredModelSpec | null): ModelMetaDraft {
  const caps = spec?.capabilities ?? []
  return {
    contextWindow: spec?.context_window != null ? String(spec.context_window) : '',
    maxOutput: spec?.max_output != null ? String(spec.max_output) : '',
    priceIn: spec?.cost_per_m_input != null ? String(spec.cost_per_m_input) : '',
    priceOut: spec?.cost_per_m_output != null ? String(spec.cost_per_m_output) : '',
    vision: caps.includes('vision'),
    tools: caps.includes('tool_use'),
  }
}

/** Validate against the engine's rules (frontend mirror of
 *  `ModelSpec::validate`): limits are positive whole numbers within u32,
 *  prices finite and non-negative. Blank = clear (valid). */
export function validateDraft(draft: ModelMetaDraft): ModelMetaErrors {
  const errors: ModelMetaErrors = {}

  const token = (raw: string): number | null | false => {
    const v = raw.trim()
    if (v === '') return null
    if (!/^\d+$/.test(v)) return false
    const n = Number(v)
    // u32 ceiling — the schema field is `Option<u32>`; anything above is a
    // guaranteed command rejection, so catch it inline instead.
    if (n < 1 || n > 4_294_967_295) return false
    return n
  }
  const price = (raw: string): number | null | false => {
    const v = raw.trim()
    if (v === '') return null
    const n = Number(v)
    if (!Number.isFinite(n) || n < 0) return false
    return n
  }

  if (token(draft.contextWindow) === false) {
    errors.contextWindow = MODEL_META_ERROR_KEYS.contextWindow
  }
  if (token(draft.maxOutput) === false) {
    errors.maxOutput = MODEL_META_ERROR_KEYS.maxOutput
  }
  if (price(draft.priceIn) === false) {
    errors.priceIn = MODEL_META_ERROR_KEYS.priceIn
  }
  if (price(draft.priceOut) === false) {
    errors.priceOut = MODEL_META_ERROR_KEYS.priceOut
  }
  return errors
}

export function hasErrors(errors: ModelMetaErrors): boolean {
  return Object.keys(errors).length > 0
}

/** Project a valid draft onto the wire input. Fields left blank become
 *  `null` (clear the declaration); display name and any non-editor-managed
 *  capability names are preserved from the existing declaration — the
 *  editor only authors the fields it renders. */
export function draftToInput(
  modelId: string,
  draft: ModelMetaDraft,
  existing?: DeclaredModelSpec | null,
): DeclaredModelInput {
  const token = (raw: string): number | null => {
    const v = raw.trim()
    if (v === '') return null
    return Number(v)
  }
  const price = (raw: string): number | null => {
    const v = raw.trim()
    if (v === '') return null
    return Number(v)
  }
  // Preserve the other capability names in their stored order, then apply
  // the editor's vision/tool_use bits deterministically at the end.
  const preserved = (existing?.capabilities ?? []).filter(
    (c) => !(EDITOR_MANAGED_CAPS as readonly string[]).includes(c),
  )
  const capabilities = [...preserved]
  if (draft.vision && !capabilities.includes('vision')) capabilities.push('vision')
  if (draft.tools && !capabilities.includes('tool_use')) capabilities.push('tool_use')

  return {
    id: modelId,
    display_name: existing?.display_name ?? null,
    context_window: token(draft.contextWindow),
    max_output: token(draft.maxOutput),
    cost_per_m_input: price(draft.priceIn),
    cost_per_m_output: price(draft.priceOut),
    capabilities,
  }
}

/// Lossless `DeclaredModelSpec` → wire input projection (the vault re-send
/// path: editing one entry still submits the WHOLE vault, so untouched
/// declarations need this).
export function specToVaultInput(spec: DeclaredModelSpec): DeclaredModelInput {
  return {
    id: spec.id,
    display_name: spec.display_name ?? null,
    context_window: spec.context_window ?? null,
    max_output: spec.max_output ?? null,
    cost_per_m_input: spec.cost_per_m_input ?? null,
    cost_per_m_output: spec.cost_per_m_output ?? null,
    capabilities: spec.capabilities ?? [],
  }
}

/** Overwrite-semantics projection: replace (or append) `input` by id in the
 *  vault snapshot, preserving order — the payload `set_provider_models`
 *  receives. Pure (returns a new array). */
export function upsertVaultModel(
  vault: readonly DeclaredModelSpec[],
  input: DeclaredModelInput,
): DeclaredModelInput[] {
  const next = vault.map((spec) =>
    spec.id === input.id ? input : specToVaultInput(spec),
  )
  if (!vault.some((spec) => spec.id === input.id)) {
    next.push(input)
  }
  return next
}

/** The active provider slot's curated vault, or `null` when no managed
 *  slot is active (env-configured providers have no vault to write to —
 *  the row affordances stay hidden rather than pretending). */
export function activeVaultOf(
  file: Pick<ProvidersFile, 'active_provider_id' | 'providers'>,
): { providerId: string; models: DeclaredModelSpec[] } | null {
  const id = file.active_provider_id?.trim()
  if (!id) return null
  const slot = file.providers.find((p) => p.id === id)
  if (!slot) return null
  return { providerId: id, models: slot.models ?? [] }
}

/** Whether `modelId` already has a declaration in the vault. */
export function isInVault(
  vault: readonly DeclaredModelSpec[] | null | undefined,
  modelId: string,
): boolean {
  return !!vault?.some((spec) => spec.id === modelId)
}
