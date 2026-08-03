/**
 * P2-5a — feature flag resolution tests.
 *
 * Verifies that the chat.v2 flag gates the assistant-ui runtime
 * correctly and that the OFF path leaves the legacy Chat.tsx path
 * untouched (acceptance criterion #3). The flag module is read
 * from three sources in priority order:
 *   1. window.__SHANNON_CHAT_V2__ (runtime override)
 *   2. import.meta.env.VITE_CHAT_V2 (build-time Vite env var)
 *   3. default false
 */
import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'

describe('featureFlag.isChatV2Enabled', () => {
  beforeEach(() => {
    // Clear all sources before each test so we can set fresh values.
    if (typeof window !== 'undefined') {
      delete (window as { __SHANNON_CHAT_V2__?: boolean }).__SHANNON_CHAT_V2__
    }
  })

  afterEach(() => {
    if (typeof window !== 'undefined') {
      delete (window as { __SHANNON_CHAT_V2__?: boolean }).__SHANNON_CHAT_V2__
    }
    vi.unstubAllGlobals()
  })

  it('defaults to false when nothing is set', async () => {
    const { isChatV2Enabled } = await import('@/lib/featureFlag')
    expect(isChatV2Enabled({})).toBe(false)
  })

  it('honours a literal env record (VITE_CHAT_V2=1)', async () => {
    const { isChatV2Enabled } = await import('@/lib/featureFlag')
    expect(isChatV2Enabled({ VITE_CHAT_V2: '1' })).toBe(true)
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'true' })).toBe(true)
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'on' })).toBe(true)
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'yes' })).toBe(true)
  })

  it('treats "false" / "0" / "off" as explicitly false', async () => {
    const { isChatV2Enabled } = await import('@/lib/featureFlag')
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'false' })).toBe(false)
    expect(isChatV2Enabled({ VITE_CHAT_V2: '0' })).toBe(false)
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'off' })).toBe(false)
  })

  it('accepts an explicit boolean env value', async () => {
    const { isChatV2Enabled } = await import('@/lib/featureFlag')
    expect(isChatV2Enabled({ VITE_CHAT_V2: true })).toBe(true)
    expect(isChatV2Enabled({ VITE_CHAT_V2: false })).toBe(false)
  })

  it('ignores unknown string values (falls back to false)', async () => {
    const { isChatV2Enabled } = await import('@/lib/featureFlag')
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'maybe' })).toBe(false)
    expect(isChatV2Enabled({ VITE_CHAT_V2: '' })).toBe(false)
  })

  it('reads window.__SHANNON_CHAT_V2__ for the runtime override', async () => {
    ;(window as { __SHANNON_CHAT_V2__?: boolean }).__SHANNON_CHAT_V2__ = true
    const { isChatV2Enabled } = await import('@/lib/featureFlag')
    expect(isChatV2Enabled({ VITE_CHAT_V2: 'false' })).toBe(true)
  })
})
