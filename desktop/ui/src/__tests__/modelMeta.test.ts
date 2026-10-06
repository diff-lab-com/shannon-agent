// S2-2 — pure editing logic for the per-model metadata editor.
//
// Pins the wire contract with the engine:
//   - blank numeric field ⇒ null (clear the declaration; catalog applies),
//   - limits: positive whole numbers within u32 (mirror of
//     `ModelSpec::validate`); prices: finite, >= 0,
//   - vision/tool_use are the editor-managed bits; other capability names
//     ride along untouched,
//   - `upsertVaultModel` projects the WHOLE vault (overwrite semantics —
//     `set_provider_models` never field-merges),
//   - `activeVaultOf`: null when no managed slot is active.

import { describe, expect, it } from 'vitest'
import type { DeclaredModelSpec, ProvidersFile } from '@/types'
import {
  MODEL_META_ERROR_KEYS,
  activeVaultOf,
  draftFromSpec,
  draftToInput,
  hasErrors,
  isInVault,
  specToVaultInput,
  upsertVaultModel,
  validateDraft,
} from '../components/settings/models-settings/modelMeta'

const fullSpec: DeclaredModelSpec = {
  id: 'proxy-model-x',
  display_name: 'Proxy X',
  context_window: 198_000,
  max_output: 32_768,
  cost_per_m_input: 0.5,
  cost_per_m_output: 2.0,
  capabilities: ['reasoning', 'vision'],
}

describe('draftFromSpec', () => {
  it('pre-fills from an existing declaration', () => {
    const draft = draftFromSpec(fullSpec)
    expect(draft).toEqual({
      contextWindow: '198000',
      maxOutput: '32768',
      priceIn: '0.5',
      priceOut: '2',
      vision: true,
      tools: false,
    })
  })

  it('starts blank for an undeclared row', () => {
    expect(draftFromSpec(undefined)).toEqual({
      contextWindow: '',
      maxOutput: '',
      priceIn: '',
      priceOut: '',
      vision: false,
      tools: false,
    })
  })
})

describe('validateDraft (frontend mirror of ModelSpec::validate)', () => {
  it('accepts a fully valid draft', () => {
    expect(validateDraft(draftFromSpec(fullSpec))).toEqual({})
    expect(hasErrors({})).toBe(false)
  })

  it('accepts blank fields (clear-on-save)', () => {
    expect(validateDraft(draftFromSpec(undefined))).toEqual({})
  })

  it('rejects zero and negative limits', () => {
    expect(validateDraft({ ...draftFromSpec(undefined), contextWindow: '0' })).toEqual({
      contextWindow: MODEL_META_ERROR_KEYS.contextWindow,
    })
    expect(validateDraft({ ...draftFromSpec(undefined), maxOutput: '-5' })).toEqual({
      maxOutput: MODEL_META_ERROR_KEYS.maxOutput,
    })
  })

  it('rejects non-integer and out-of-u32 limits', () => {
    expect(validateDraft({ ...draftFromSpec(undefined), contextWindow: '1.5' })).toEqual({
      contextWindow: MODEL_META_ERROR_KEYS.contextWindow,
    })
    expect(validateDraft({ ...draftFromSpec(undefined), contextWindow: '4294967296' })).toEqual({
      contextWindow: MODEL_META_ERROR_KEYS.contextWindow,
    })
    // Exactly u32::MAX is fine.
    expect(validateDraft({ ...draftFromSpec(undefined), contextWindow: '4294967295' })).toEqual({})
  })

  it('rejects negative and non-finite prices', () => {
    expect(validateDraft({ ...draftFromSpec(undefined), priceIn: '-0.01' })).toEqual({
      priceIn: MODEL_META_ERROR_KEYS.priceIn,
    })
    expect(validateDraft({ ...draftFromSpec(undefined), priceOut: 'abc' })).toEqual({
      priceOut: MODEL_META_ERROR_KEYS.priceOut,
    })
    expect(validateDraft({ ...draftFromSpec(undefined), priceIn: 'Infinity' })).toEqual({
      priceIn: MODEL_META_ERROR_KEYS.priceIn,
    })
    // Zero is a legitimate price (free/local endpoints).
    expect(validateDraft({ ...draftFromSpec(undefined), priceIn: '0', priceOut: '0' })).toEqual({})
  })

  it('collects every offending field at once', () => {
    const errors = validateDraft({
      contextWindow: 'x',
      maxOutput: '0',
      priceIn: '-1',
      priceOut: '-1',
      vision: false,
      tools: false,
    })
    expect(Object.keys(errors).sort()).toEqual([
      'contextWindow',
      'maxOutput',
      'priceIn',
      'priceOut',
    ])
  })
})

describe('draftToInput (wire projection)', () => {
  it('preserves display_name and non-editor capabilities while updating the managed bits', () => {
    const draft = { ...draftFromSpec(fullSpec), vision: false, tools: true }
    const input = draftToInput('proxy-model-x', draft, fullSpec)
    expect(input).toEqual({
      id: 'proxy-model-x',
      display_name: 'Proxy X',
      context_window: 198000,
      max_output: 32768,
      cost_per_m_input: 0.5,
      cost_per_m_output: 2,
      // reasoning rides along; vision dropped, tool_use added.
      capabilities: ['reasoning', 'tool_use'],
    })
  })

  it('maps blank fields to null (clear the declaration)', () => {
    const input = draftToInput('m', draftFromSpec(undefined), fullSpec)
    expect(input.context_window).toBeNull()
    expect(input.max_output).toBeNull()
    expect(input.cost_per_m_input).toBeNull()
    expect(input.cost_per_m_output).toBeNull()
    // Capability-checked nothing ⇒ empty list (engine: unknown, defers to
    // the catalog for gating — never "provably unsupported").
    expect(input.capabilities).toEqual(['reasoning'])
  })

  it('works without an existing declaration (id-only + chosen metadata)', () => {
    const input = draftToInput(
      'm',
      { ...draftFromSpec(undefined), contextWindow: '8192', vision: true },
      undefined,
    )
    expect(input).toEqual({
      id: 'm',
      display_name: null,
      context_window: 8192,
      max_output: null,
      cost_per_m_input: null,
      cost_per_m_output: null,
      capabilities: ['vision'],
    })
  })
})

describe('specToVaultInput + upsertVaultModel (overwrite-semantics payload)', () => {
  it('replaces by id in place and normalizes untouched specs', () => {
    const vault: DeclaredModelSpec[] = [
      { id: 'a' },
      fullSpec,
      { id: 'c', context_window: 1000 },
    ]
    const edited = draftToInput(
      'proxy-model-x',
      { ...draftFromSpec(fullSpec), priceIn: '0.75', priceOut: '3' },
      fullSpec,
    )
    const payload = upsertVaultModel(vault, edited)
    expect(payload.map((s) => s.id)).toEqual(['a', 'proxy-model-x', 'c'])
    expect(payload[1].cost_per_m_input).toBe(0.75)
    // The whole vault is re-sent as wire inputs (overwrite semantics).
    expect(payload[0]).toEqual({ id: 'a', display_name: null, context_window: null, max_output: null, cost_per_m_input: null, cost_per_m_output: null, capabilities: [] })
  })

  it('appends an unknown id (add-to-vault) without disturbing the rest', () => {
    const vault: DeclaredModelSpec[] = [{ id: 'a' }]
    const payload = upsertVaultModel(vault, specToVaultInput({ id: 'new-one' }))
    expect(payload.map((s) => s.id)).toEqual(['a', 'new-one'])
  })

  it('does not mutate the input vault', () => {
    const vault: DeclaredModelSpec[] = [fullSpec]
    upsertVaultModel(vault, draftToInput('proxy-model-x', draftFromSpec(undefined), fullSpec))
    expect(vault[0].cost_per_m_input).toBe(0.5)
  })
})

describe('activeVaultOf / isInVault', () => {
  const file: ProvidersFile = {
    active_provider_id: 'prov-1',
    providers: [
      { id: 'prov-0', display_name: 'Other', kind: 'openai', has_api_key: true },
      {
        id: 'prov-1',
        display_name: 'Active',
        kind: 'anthropic',
        has_api_key: true,
        models: [fullSpec],
      },
    ],
  }

  it('returns the active slot and its vault', () => {
    expect(activeVaultOf(file)).toEqual({ providerId: 'prov-1', models: [fullSpec] })
    expect(isInVault([fullSpec], 'proxy-model-x')).toBe(true)
    expect(isInVault([fullSpec], 'other')).toBe(false)
    expect(isInVault(null, 'proxy-model-x')).toBe(false)
  })

  it('is null without an active managed slot (env-configured provider)', () => {
    expect(activeVaultOf({ active_provider_id: null, providers: file.providers })).toBeNull()
    expect(activeVaultOf({ active_provider_id: '', providers: file.providers })).toBeNull()
    expect(activeVaultOf({ active_provider_id: 'ghost', providers: [] })).toBeNull()
  })

  it('treats a slot without models as an empty vault', () => {
    const empty = activeVaultOf({
      active_provider_id: 'prov-0',
      providers: file.providers,
    })
    expect(empty).toEqual({ providerId: 'prov-0', models: [] })
  })
})
