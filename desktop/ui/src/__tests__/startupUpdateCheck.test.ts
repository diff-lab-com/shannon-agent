// 启动时检查更新 (批 1) — the launch-time check-only probe's contract:
// gated by `update_check_at_launch` (default ON), exactly one run per app
// run, toast only when a newer release exists, silent on every failure.

import { describe, it, expect, vi, beforeEach } from 'vitest'

// vi.mock factories hoist above module-level consts — declare through
// vi.hoisted so the factory can reach the spy (same pattern as
// ChatStatusBar.test.tsx).
const { toastMock } = vi.hoisted(() => ({ toastMock: vi.fn() }))
vi.mock('sonner', () => ({ toast: { success: toastMock } }))

import { runStartupUpdateCheck, resetStartupUpdateCheckForTests } from '@/lib/startupUpdateCheck'
import type * as api from '@/lib/tauri-api'

type ApiStub = Pick<typeof api, 'getConfig' | 'checkAppUpdate'>

function stubApi(config: unknown, update: unknown): ApiStub & { getConfig: ReturnType<typeof vi.fn>; checkAppUpdate: ReturnType<typeof vi.fn> } {
  return {
    getConfig: vi.fn().mockResolvedValue(config),
    checkAppUpdate: vi.fn().mockResolvedValue(update),
  }
}

beforeEach(() => {
  resetStartupUpdateCheckForTests()
  toastMock.mockClear()
})

describe('startupUpdateCheck (批 1)', () => {
  it('toasts the About pane copy once when a newer release exists', async () => {
    const api = stubApi({}, { updateAvailable: true, latestVersion: 'v9.9.9', releaseUrl: 'https://example.com' })
    await runStartupUpdateCheck(api)
    expect(api.getConfig).toHaveBeenCalledTimes(1)
    expect(api.checkAppUpdate).toHaveBeenCalledTimes(1)
    expect(toastMock).toHaveBeenCalledTimes(1)
    expect(toastMock).toHaveBeenCalledWith(expect.stringContaining('v9.9.9'))
  })

  it('stays silent when the running version is the latest', async () => {
    const api = stubApi({}, { updateAvailable: false, latestVersion: 'v1.0.0' })
    await runStartupUpdateCheck(api)
    expect(toastMock).not.toHaveBeenCalled()
  })

  it('skips the probe entirely when the toggle is off (absent key still checks)', async () => {
    const off = stubApi({ update_check_at_launch: false }, { updateAvailable: true, latestVersion: 'v9' })
    await runStartupUpdateCheck(off)
    expect(off.checkAppUpdate).not.toHaveBeenCalled()
    expect(toastMock).not.toHaveBeenCalled()

    resetStartupUpdateCheckForTests()
    const defaulted = stubApi({}, { updateAvailable: false, latestVersion: 'v1' })
    await runStartupUpdateCheck(defaulted)
    expect(defaulted.checkAppUpdate).toHaveBeenCalledTimes(1)
  })

  it('runs at most once per app run, even across repeated calls', async () => {
    const api = stubApi({}, { updateAvailable: false, latestVersion: 'v1' })
    await runStartupUpdateCheck(api)
    await runStartupUpdateCheck(api)
    expect(api.getConfig).toHaveBeenCalledTimes(1)
  })

  it('swallows every failure (config read, probe) without throwing', async () => {
    const failing = {
      getConfig: vi.fn().mockRejectedValue(new Error('backend gone')),
      checkAppUpdate: vi.fn(),
    }
    await expect(runStartupUpdateCheck(failing as ApiStub)).resolves.toBeUndefined()
    expect(failing.checkAppUpdate).not.toHaveBeenCalled()
    expect(toastMock).not.toHaveBeenCalled()

    resetStartupUpdateCheckForTests()
    const probeFails = {
      getConfig: vi.fn().mockResolvedValue({}),
      checkAppUpdate: vi.fn().mockRejectedValue(new Error('offline')),
    }
    await expect(runStartupUpdateCheck(probeFails as ApiStub)).resolves.toBeUndefined()
    expect(toastMock).not.toHaveBeenCalled()
  })
})
