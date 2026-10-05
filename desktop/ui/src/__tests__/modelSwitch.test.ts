// S3-1 (P-N23) — the shared global-default write helper: both the Header
// selector and the Settings quick switcher funnel through it, so the
// model+provider double-write convention lives in exactly one place.

import { describe, expect, it, vi, beforeEach } from 'vitest'

vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
}))

import { writeGlobalModelDefault } from '@/lib/modelSwitch'
import * as api from '@/lib/tauri-api'

describe('writeGlobalModelDefault (P-N23 double write)', () => {
  beforeEach(() => {
    // Calls leak across tests in this file (one shared module mock) — the
    // call-count assertion below needs a clean slate per test.
    vi.mocked(api.configure).mockClear()
    vi.mocked(api.configure).mockResolvedValue(undefined)
  })

  it('writes model THEN provider, both from the catalog row', async () => {
    await writeGlobalModelDefault({ id: 'gpt-5', provider: 'openai' })
    expect(api.configure).toHaveBeenNthCalledWith(1, { key: 'model', value: 'gpt-5' })
    expect(api.configure).toHaveBeenNthCalledWith(2, { key: 'provider', value: 'openai' })
  })

  it('propagates the first failed configure (callers own the toast)', async () => {
    vi.mocked(api.configure).mockRejectedValueOnce(new Error('no active provider'))
    await expect(writeGlobalModelDefault({ id: 'm', provider: 'p' })).rejects.toThrow('no active provider')
    // The provider write never fires after a failed model write.
    expect(api.configure).toHaveBeenCalledTimes(1)
  })
})
