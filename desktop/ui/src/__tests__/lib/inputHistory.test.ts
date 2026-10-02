// A-22 — the global composer input-history ring (`shannon.inputHistory`).
// Storage contract: MRU order (newest last), consecutive-duplicate no-op,
// earlier duplicates moved to the end, cap 50 dropping the oldest, and
// best-effort persistence (corrupted payloads / quota failures never throw).

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import {
  INPUT_HISTORY_KEY,
  INPUT_HISTORY_LIMIT,
  loadInputHistory,
  recordInputHistory,
} from '@/lib/inputHistory'

describe('inputHistory (A-22)', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('starts empty when the key is absent', () => {
    expect(loadInputHistory()).toEqual([])
  })

  it('records sends newest-last and persists them for the next read', () => {
    expect(recordInputHistory('first prompt')).toEqual(['first prompt'])
    expect(recordInputHistory('second prompt')).toEqual(['first prompt', 'second prompt'])
    // Cross-"restart": a fresh read from localStorage sees the same ring.
    expect(loadInputHistory()).toEqual(['first prompt', 'second prompt'])
    expect(localStorage.getItem(INPUT_HISTORY_KEY)).toContain('second prompt')
  })

  it('repeating the current last entry is a no-op (no consecutive duplicates)', () => {
    recordInputHistory('same again')
    recordInputHistory('same again')
    recordInputHistory('same again')
    expect(loadInputHistory()).toEqual(['same again'])
  })

  it('moves an earlier duplicate to the end (MRU), keeping single occurrence', () => {
    recordInputHistory('alpha')
    recordInputHistory('beta')
    recordInputHistory('gamma')
    expect(recordInputHistory('alpha')).toEqual(['beta', 'gamma', 'alpha'])
    expect(loadInputHistory()).toEqual(['beta', 'gamma', 'alpha'])
  })

  it('caps at 50 entries and drops the OLDEST first', () => {
    for (let i = 0; i < INPUT_HISTORY_LIMIT + 5; i++) recordInputHistory(`m-${i}`)
    const history = loadInputHistory()
    expect(history).toHaveLength(INPUT_HISTORY_LIMIT)
    expect(history[0]).toBe('m-5') // m-0..m-4 evicted
    expect(history[history.length - 1]).toBe(`m-${INPUT_HISTORY_LIMIT + 4}`)
  })

  it('ignores empty entries', () => {
    expect(recordInputHistory('')).toEqual([])
    expect(localStorage.getItem(INPUT_HISTORY_KEY)).toBeNull()
  })

  it('degrades to [] on corrupted JSON and still records afterwards', () => {
    localStorage.setItem(INPUT_HISTORY_KEY, '{not json')
    expect(loadInputHistory()).toEqual([])
    expect(recordInputHistory('fresh')).toEqual(['fresh'])
  })

  it('degrades to [] when the stored payload is not an array or holds non-strings', () => {
    localStorage.setItem(INPUT_HISTORY_KEY, JSON.stringify({ nope: true }))
    expect(loadInputHistory()).toEqual([])
    localStorage.setItem(INPUT_HISTORY_KEY, JSON.stringify(['ok', 42, null, 'fine']))
    expect(loadInputHistory()).toEqual(['ok', 'fine'])
  })

  it('a failed persistence write still returns the in-memory ring (best-effort)', () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded')
    })
    expect(recordInputHistory('kept in memory')).toEqual(['kept in memory'])
    expect(setItem).toHaveBeenCalled()
  })
})
