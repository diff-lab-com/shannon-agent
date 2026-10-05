// S3-3 — utility slot option-value encoding: pure derivation tests. The
// select encodes its selection as `provider::model` (or '' for follow-default);
// these pin the round-trip and the malformed-input degradation.

import { describe, expect, it } from 'vitest'
import { UTILITY_SLOTS, utilitySlotValueOf, utilitySlotValueParts } from '@/lib/utilitySlots'
import type { UtilitySlotStatus } from '@/lib/tauri-api'

describe('utilitySlots (S3-3 display derivations)', () => {
  it('defines exactly the two in-scope slots with schema role names', () => {
    expect(UTILITY_SLOTS.map((s) => s.role)).toEqual(['compression', 'title_generation'])
    expect(UTILITY_SLOTS.map((s) => s.id)).toEqual(['compaction', 'summary'])
  })

  it('encodes a configured slot as provider::model and unset as blank', () => {
    expect(utilitySlotValueOf(undefined)).toBe('')
    const unset = { role: 'compression', provider: null, model: null, resolves: false }
    expect(utilitySlotValueOf(unset as UtilitySlotStatus)).toBe('')
    const set = { role: 'compression', provider: 'zhipu', model: 'glm-5.3-air', resolves: true }
    expect(utilitySlotValueOf(set as UtilitySlotStatus)).toBe('zhipu::glm-5.3-air')
  })

  it('round-trips value ↔ parts', () => {
    const value = 'zhipu::glm-5.3-air'
    expect(utilitySlotValueParts(value)).toEqual({ provider: 'zhipu', model: 'glm-5.3-air' })
  })

  it('degrades malformed values to null, never to a partial target', () => {
    expect(utilitySlotValueParts('')).toBeNull()
    expect(utilitySlotValueParts('zhipu')).toBeNull()
    expect(utilitySlotValueParts('::model')).toBeNull()
    expect(utilitySlotValueParts('zhipu::')).toBeNull()
  })
})
