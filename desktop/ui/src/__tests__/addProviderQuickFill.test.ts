import { describe, it, expect } from 'vitest'
import { QUICK_FILL } from '@/components/settings/add-provider-modal/types'

// S1-4b (2026-10 review P-N5): the quick-fill chips are the first thing a
// new user sees in the Add Provider modal — they must be CURRENT model ids,
// not a stale 2024 catalog. The catalog-presence half of the contract is
// pinned on the Rust side (`quick_fill_model_ids_exist_in_static_catalog`
// in desktop/src/commands_config.rs asserts every catalog-pinned id exists
// in MODEL_CATALOG); this file pins the TS list itself so the two sides
// can't drift silently.
describe('QUICK_FILL preset hygiene (S1-4b)', () => {
  it('prefills a non-empty model id on every chip except `custom`', () => {
    for (const qf of QUICK_FILL) {
      if (qf.id === 'custom') {
        expect(qf.model, 'the custom catch-all must not guess a model').toBeUndefined()
        continue
      }
      expect(qf.model, `chip "${qf.id}" must prefill a model id`).toBeTruthy()
      expect(qf.model!.trim().length, `chip "${qf.id}" model must be non-blank`).toBeGreaterThan(0)
    }
  })

  it('uses well-formed http(s) base URLs on the chips that carry one', () => {
    for (const qf of QUICK_FILL) {
      if (!qf.baseUrl) continue
      const url = new URL(qf.baseUrl) // throws on garbage
      expect(url.protocol, `chip "${qf.id}" baseUrl scheme`).toMatch(/^https?:$/)
      expect(url.hostname.length, `chip "${qf.id}" baseUrl host`).toBeGreaterThan(0)
    }
  })

  it('pins the exact chip → model list (keep in sync with the Rust pin test)', () => {
    // Catalog-pinned ids here are cross-checked against MODEL_CATALOG by
    // desktop/src/commands_config.rs::quick_fill_model_ids_exist_in_static_catalog.
    // `llama3.2` is the deliberate exception: Ollama ids come from the
    // user's local daemon at runtime (`detect_local_models`), so the static
    // catalog has no Ollama entry to pin — Fetch models prefills the real
    // list (S1-4c).
    expect(QUICK_FILL.map((qf) => [qf.id, qf.model ?? null])).toEqual([
      ['anthropic', 'claude-sonnet-4-6'],
      ['openai', 'gpt-5-mini'],
      ['deepseek', 'deepseek-chat'],
      ['glm', 'glm-5.1'],
      ['kimi', 'kimi-k2.6'],
      ['minimax', 'MiniMax-M3'],
      ['ollama', 'llama3.2'],
      ['custom', null],
    ])
  })
})
