// S4 hygiene batch — the demo mock's provider-surface completions (P-N21)
// and the e2e scenario hooks (P-N20). The handlers previously lived on the
// coverage tripwire's UNMOCKED_ALLOWLIST ("demo never reaches") while the
// Settings → Models pages demonstrably reach them; these tests pin the
// contracts the journeys and demo mode now rely on:
//
//   test_all_providers      — one classified row per managed connection,
//                             roster order, masked everything else.
//   get_provider_allowlist  — the desktop override persisted through
//                             configure('enabled_providers'), null when
//                             unset (engine env vars decide).
//   save_provider           — the backend's active-target repoint + the v2
//                             models_url mirror on the connection.
//   scenario hooks          — envProvider / unconfigured /
//                             catalogRefreshFails, each defaulting to the
//                             pre-hook behavior when unarmed.

import { afterEach, describe, expect, it } from 'vitest'
import { handlers } from '@/lib/mock/handlers'
import { MOCK_PROVIDERS } from '@/lib/mock/data/config'
import { NON_PROBEABLE_PROVIDER_KINDS } from '@/components/settings/models-settings/types'
import type { ProviderInput } from '@/types'

/** The scenario hooks read localStorage — arm/disarm per test. */
function armHook(name: string, value: string | null) {
  if (value === null) window.localStorage.removeItem(name)
  else window.localStorage.setItem(name, value)
}

afterEach(() => {
  armHook('shannon.demo.envProvider', null)
  armHook('shannon.demo.unconfigured', null)
  armHook('shannon.demo.catalogRefreshFails', null)
})

describe('mock handlers — test_all_providers (P-N21)', () => {
  it('returns one classified success row per managed connection, in roster order', async () => {
    const rows = await handlers.test_all_providers() as Array<{
      id: string
      label: string
      provider_kind: string
      result: { kind: string }
      latency_ms: number | null
    }>
    expect(rows).toHaveLength(MOCK_PROVIDERS.providers.length)
    expect(rows.map((r) => r.id)).toEqual(MOCK_PROVIDERS.providers.map((p) => p.id))
    for (const row of rows) {
      expect(row.result.kind).toBe('success')
      expect(row.latency_ms).not.toBeNull()
      expect(Object.keys(row).sort()).toEqual(
        ['id', 'label', 'latency_ms', 'provider_kind', 'result'].sort(),
      )
    }
  })
})

describe('mock handlers — get_provider_allowlist + configure mirror (P-N21)', () => {
  it('answers null while no desktop override is persisted', async () => {
    await handlers.configure({ key: 'enabled_providers', value: 'null' })
    expect(await handlers.get_provider_allowlist()).toBeNull()
  })

  it('round-trips an explicit override through configure', async () => {
    await handlers.configure({
      key: 'enabled_providers',
      value: JSON.stringify(['anthropic', 'ollama']),
    })
    expect(await handlers.get_provider_allowlist()).toEqual(['anthropic', 'ollama'])
    // Clearing restores the env-decides default.
    await handlers.configure({ key: 'enabled_providers', value: 'null' })
    expect(await handlers.get_provider_allowlist()).toBeNull()
  })

  it('rejects malformed payloads like the backend arm does', async () => {
    await expect(
      handlers.configure({ key: 'enabled_providers', value: '["anthropic", 42]' }),
    ).rejects.toThrow(/invalid enabled_providers/)
  })
})

describe('mock handlers — save_provider backend contracts (P-N21/P2-23 残留)', () => {
  it('repoints the active target at the saved slot and mirrors models_url', async () => {
    const input: ProviderInput = {
      display_name: 'E2E Vault Provider',
      kind: 'openai-compatible',
      api_key: 'sk-demo',
      base_url: 'https://api.example.com/v1',
      models_url: 'https://api.example.com/v1/models',
    }
    const file = await handlers.save_provider({ input }) as typeof MOCK_PROVIDERS
    const saved = file.providers.at(-1)!
    expect(file.active_provider_id).toBe(saved.id)
    expect(saved.models_url).toBe('https://api.example.com/v1/models')

    // An edit keeps the stored models_url when the input omits it.
    const edited = await handlers.save_provider({
      input: { ...input, id: saved.id, models_url: undefined },
    }) as typeof MOCK_PROVIDERS
    expect((edited.providers.find((p) => p.id === saved.id) ?? {}).models_url).toBe(
      'https://api.example.com/v1/models',
    )
    expect(edited.active_provider_id).toBe(saved.id)

    // Cleanup so roster-dependent suites stay pristine.
    await handlers.delete_provider({ id: saved.id })
    await handlers.configure({ key: 'enabled_providers', value: 'null' })
  })
})

describe('mock handlers — S4 scenario hooks (P-N20)', () => {
  it('detect_provider_from_env answers the envProvider hook, null when unarmed', async () => {
    expect(await handlers.detect_provider_from_env()).toBeNull()
    armHook('shannon.demo.envProvider', JSON.stringify({ provider: 'ollama' }))
    expect(await handlers.detect_provider_from_env()).toEqual({ provider: 'ollama', has_api_key: false })
    armHook('shannon.demo.envProvider', JSON.stringify({ provider: 'anthropic', has_api_key: true }))
    expect(await handlers.detect_provider_from_env()).toEqual({ provider: 'anthropic', has_api_key: true })
    // Malformed payloads degrade to the unarmed default.
    armHook('shannon.demo.envProvider', 'not-json')
    expect(await handlers.detect_provider_from_env()).toBeNull()
  })

  it('get_provider_status flips to the fresh-user snapshot when unconfigured is armed', async () => {
    const configured = await handlers.get_provider_status() as { active_provider_id: string | null }
    expect(configured.active_provider_id).not.toBeNull()
    armHook('shannon.demo.unconfigured', '1')
    const fresh = await handlers.get_provider_status() as {
      active_provider_id: string | null
      env_provider: string | null
    }
    expect(fresh.active_provider_id).toBeNull()
    expect(fresh.env_provider).toBeNull()
  })

  it('refresh_model_catalog rejects with the upstream reason when the failure hook is armed', async () => {
    await expect(handlers.refresh_model_catalog()).resolves.toMatchObject({ count: 7 })
    armHook('shannon.demo.catalogRefreshFails', '1')
    await expect(handlers.refresh_model_catalog()).rejects.toThrow(/models\.dev upstream unreachable/)
  })
})

describe('NON_PROBEABLE_PROVIDER_KINDS — Rust mirror pin (P-N25)', () => {
  it('matches the desktop const (commands_config.rs) verbatim', () => {
    // commands_config.rs: NON_PROBEABLE_PROVIDER_KINDS = &["azure", "gemini"].
    // Its Rust pin test (probeable ⇔ not in this list) keeps is_probeable_kind
    // in sync; this pin keeps the frontend hint in the same lockstep.
    expect(NON_PROBEABLE_PROVIDER_KINDS).toEqual(['azure', 'gemini'])
  })

  it('classifies the KIND_INFO kinds consistently with is_probeable_kind', () => {
    const kindInfoKinds = ['anthropic', 'openai', 'deepseek', 'ollama', 'gemini', 'openai-compatible']
    for (const kind of kindInfoKinds) {
      expect(NON_PROBEABLE_PROVIDER_KINDS.includes(kind)).toBe(kind === 'gemini')
    }
  })
})
