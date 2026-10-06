// R4-3 (desktop slice) — mock-handler state logic for the per-provider
// multi-key surface, driven DIRECTLY against `handlers` (the same
// "direct handler invocation" posture the R1-2 tripwire's handler record
// supports; no popup interactions, no DOM). Doubles as the demo-mode
// never-leak-a-key tripwire: every wire shape carries masked hints only.

import { describe, expect, it } from 'vitest'
import { handlers } from '@/lib/mock/handlers'
import type { ProviderKeySummary } from '@/types'

const FULL_KEYS = [
  'sk-ant-demo03-activekey0000000001',
  'sk-ant-demo03-sparekey000000002',
  'sk-glm-demo03-onlykey000000000003',
]

async function listKeys(providerId: string): Promise<ProviderKeySummary[]> {
  return (await handlers.list_provider_keys({ providerId })) as ProviderKeySummary[]
}

describe('mock provider-keys handlers (R4-3 desktop)', () => {
  it('seeds a rotation and never leaks full key material in list output', async () => {
    const rows = await listKeys('prov-anthropic')
    expect(rows).toHaveLength(2)
    expect(rows[0]).toMatchObject({ index: 0, active: true })
    expect(rows[1]).toMatchObject({ index: 1, active: false })
    for (const row of rows) {
      for (const full of FULL_KEYS) {
        expect(row.masked_hint).not.toContain(full)
      }
      expect(row.masked_hint).toMatch(/…/)
    }
  })

  it('add: appends a key, refuses duplicates with the offending slot, masks the wire', async () => {
    const before = await listKeys('prov-anthropic')
    const fresh = (await handlers.add_provider_key({
      providerId: 'prov-anthropic',
      key: 'sk-ant-demo03-addedkey0000000004',
    })) as ProviderKeySummary[]
    expect(fresh).toHaveLength(before.length + 1)
    expect(fresh[fresh.length - 1].masked_hint).not.toContain('sk-ant-demo03-addedkey0000000004')

    await expect(
      handlers.add_provider_key({ providerId: 'prov-anthropic', key: 'sk-ant-demo03-addedkey0000000004' }),
    ).rejects.toThrow(/already registered at slot/)

    await expect(handlers.add_provider_key({ providerId: 'prov-anthropic', key: '   ' })).rejects.toThrow(
      /empty key/,
    )
    // Cleanup so test order never depends on this addition.
    await handlers.remove_provider_key({ providerId: 'prov-anthropic', index: fresh.length - 1 })
  })

  it('remove: promotes the next key when the ACTIVE one goes; refuses the last', async () => {
    const before = await listKeys('prov-anthropic')
    const spareHint = before[1].masked_hint
    const fresh = (await handlers.remove_provider_key({ providerId: 'prov-anthropic', index: 0 })) as ProviderKeySummary[]
    expect(fresh).toHaveLength(before.length - 1)
    expect(fresh[0]).toMatchObject({ index: 0, active: true, masked_hint: spareHint })

    // Restore the seeded shape: re-add the removed key (appends at the end),
    // then rotate it back to the front.
    await handlers.add_provider_key({ providerId: 'prov-anthropic', key: FULL_KEYS[0] })
    const len = (await listKeys('prov-anthropic')).length
    await handlers.activate_provider_key({ providerId: 'prov-anthropic', index: len - 1 })
    const restored = await listKeys('prov-anthropic')
    expect(restored.map((r) => r.masked_hint)).toEqual(before.map((r) => r.masked_hint))

    // Last remaining key is refused (the single-key provider).
    await expect(handlers.remove_provider_key({ providerId: 'prov-glm', index: 0 })).rejects.toThrow(
      /last remaining key/,
    )
  })

  it('activate: swaps to slot 0; out-of-range is a clean error', async () => {
    const before = await listKeys('prov-anthropic')
    const fresh = (await handlers.activate_provider_key({
      providerId: 'prov-anthropic',
      index: 1,
    })) as ProviderKeySummary[]
    expect(fresh[0]).toMatchObject({ active: true, masked_hint: before[1].masked_hint })
    // Restore.
    await handlers.activate_provider_key({ providerId: 'prov-anthropic', index: 1 })
    await expect(
      handlers.activate_provider_key({ providerId: 'prov-anthropic', index: 99 }),
    ).rejects.toThrow(/out of range/)
    await expect(handlers.list_provider_keys({ providerId: 'ghost' })).rejects.toThrow(/not configured/)
  })
})

describe('mock profile rename/delete handlers (R5)', () => {
  it('rename: follows the active marker and refuses duplicates / unknowns', async () => {
    const renamed = (await handlers.rename_provider_profile({ old: 'research', new: 'lab' })) as unknown as {
      name: string
      active: boolean
    }[]
    expect(renamed.some((p) => p.name === 'lab')).toBe(true)
    expect(renamed.some((p) => p.name === 'research')).toBe(false)

    await expect(handlers.rename_provider_profile({ old: 'default', new: 'lab' })).rejects.toThrow(
      /already exists/,
    )
    await expect(handlers.rename_provider_profile({ old: 'ghost', new: 'x' })).rejects.toThrow(/not found/)

    // Restore for the other tests / demo determinism.
    await handlers.rename_provider_profile({ old: 'lab', new: 'research' })
  })

  it('delete: refuses the last profile and falls back on the ACTIVE one', async () => {
    // The engine's refusal: a config must keep at least one profile. The
    // roster has two — delete the INACTIVE one first to get there.
    const outcome = (await handlers.delete_provider_profile({ name: 'research', force: true })) as unknown as {
      profiles: { name: string; active: boolean }[]
      became_active: string | null
    }
    expect(outcome.became_active).toBeNull()
    expect(outcome.profiles.some((p) => p.name === 'research')).toBe(false)

    await expect(handlers.delete_provider_profile({ name: 'default', force: true })).rejects.toThrow(
      /only profile/,
    )

    // Restore the seeded roster (research is inactive again).
    await handlers.create_provider_profile({ name: 'research' })
  })

  it('delete of a seeded ACTIVE profile names the fallback (default)', async () => {
    // Seed a third profile and make it active, then delete it — the
    // fallback must be 'default' (engine rule: default survives → default).
    await handlers.create_provider_profile({ name: 'temp-active' })
    await handlers.set_active_provider_profile({ name: 'temp-active' })
    const outcome = (await handlers.delete_provider_profile({ name: 'temp-active', force: true })) as unknown as {
      profiles: { name: string; active: boolean }[]
      became_active: string | null
    }
    expect(outcome.became_active).toBe('default')
    expect(outcome.profiles.find((p) => p.name === 'default')?.active).toBe(true)
  })
})
