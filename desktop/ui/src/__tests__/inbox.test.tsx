// Tests for the P0-3 inbox hooks (useInboxItems / useInboxStats):
// initial load + filter args, status transitions hitting the right command,
// rerun/continue-session plumbing, and refresh on the `inbox-updated` event.
// The api layer is mocked (no Tauri IPC); the event bus is the global
// `@tauri-apps/api/event` mock from setup.ts.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import type { Mock } from 'vitest'
import { listen } from '@tauri-apps/api/event'
import { I18nProvider } from '@/i18n'
import { useInboxItems, useInboxStats } from '@/hooks/inbox'
import * as api from '@/lib/tauri-api'
import type { InboxItem } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listInboxItems: vi.fn(),
  updateInboxItemStatus: vi.fn(),
  getInboxStats: vi.fn(),
  rerunInboxItem: vi.fn(),
  continueInboxItemSession: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

function makeItem(o: Partial<InboxItem> & { id: number }): InboxItem {
  return {
    source: 'routine',
    sourceId: 'sched-001',
    sessionId: null,
    title: `Item ${o.id}`,
    summary: 'summary',
    error: null,
    status: 'pending',
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    ...o,
  }
}

const listenMock = listen as unknown as Mock

function fireInboxUpdated() {
  const call = listenMock.mock.calls.find(c => c[0] === 'inbox-updated')
  expect(call).toBeDefined()
  act(() => { call![1]({ payload: 1, event: 'inbox-updated', id: 0 }) })
}

describe('useInboxItems', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.listInboxItems).mockResolvedValue([])
  })

  it('loads items on mount via list_inbox_items args', async () => {
    vi.mocked(api.listInboxItems).mockResolvedValue([makeItem({ id: 1 })])
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    expect(result.current.loading).toBe(true)
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.items).toHaveLength(1)
    expect(api.listInboxItems).toHaveBeenCalledWith(undefined)
  })

  it('surfaces the error message when the command fails', async () => {
    vi.mocked(api.listInboxItems).mockRejectedValue('db locked')
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe('db locked')
    expect(result.current.items).toEqual([])
  })

  it('refetches with the new filter when setFilter changes', async () => {
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    act(() => result.current.setFilter({ status: 'read', source: 'goal' }))
    await waitFor(() =>
      expect(api.listInboxItems).toHaveBeenLastCalledWith({ status: 'read', source: 'goal' }),
    )
  })

  it('markRead sends status=read then refreshes', async () => {
    vi.mocked(api.updateInboxItemStatus).mockResolvedValue(undefined)
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    const ok = await result.current.markRead(7)
    expect(ok).toBe(true)
    expect(api.updateInboxItemStatus).toHaveBeenCalledWith(7, 'read')
    expect(api.listInboxItems).toHaveBeenCalledTimes(2)
  })

  it('archive sends status=archived then refreshes', async () => {
    vi.mocked(api.updateInboxItemStatus).mockResolvedValue(undefined)
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    const ok = await result.current.archive(7)
    expect(ok).toBe(true)
    expect(api.updateInboxItemStatus).toHaveBeenCalledWith(7, 'archived')
    expect(api.listInboxItems).toHaveBeenCalledTimes(2)
  })

  it('markRead reports failure and does not refresh when the command rejects', async () => {
    vi.mocked(api.updateInboxItemStatus).mockRejectedValue('db locked')
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    const ok = await result.current.markRead(7)
    expect(ok).toBe(false)
    // String rejections (Tauri serialises errors to strings) surface the
    // i18n fallback message; the raw cause goes to the error toast.
    await waitFor(() => expect(result.current.error).toBe('Failed to update item status'))
    expect(api.listInboxItems).toHaveBeenCalledTimes(1)
  })

  it('surfaces Error rejections verbatim on status transitions', async () => {
    vi.mocked(api.updateInboxItemStatus).mockRejectedValue(new Error('db locked'))
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await result.current.markRead(7)
    await waitFor(() => expect(result.current.error).toBe('db locked'))
  })

  it('rerun returns the new run id', async () => {
    vi.mocked(api.rerunInboxItem).mockResolvedValue('run-42')
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    const runId = await result.current.rerun(3)
    expect(runId).toBe('run-42')
    expect(api.rerunInboxItem).toHaveBeenCalledWith(3)
  })

  it('rerun returns null when the backend rejects (e.g. goal/trigger source)', async () => {
    vi.mocked(api.rerunInboxItem).mockRejectedValue("inbox item source 'goal' cannot be rerun")
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    const runId = await result.current.rerun(3)
    expect(runId).toBeNull()
  })

  it('getSessionId resolves the linked session', async () => {
    vi.mocked(api.continueInboxItemSession).mockResolvedValue('sess-006')
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await expect(result.current.getSessionId(2)).resolves.toBe('sess-006')
    expect(api.continueInboxItemSession).toHaveBeenCalledWith(2)
  })

  it('getSessionId returns null when no session is linked', async () => {
    vi.mocked(api.continueInboxItemSession).mockRejectedValue('inbox item 2 has no linked session')
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await expect(result.current.getSessionId(2)).resolves.toBeNull()
  })

  it('refreshes when the backend emits inbox-updated', async () => {
    const { result } = renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(api.listInboxItems).toHaveBeenCalledTimes(1)
    fireInboxUpdated()
    await waitFor(() => expect(api.listInboxItems).toHaveBeenCalledTimes(2))
  })

  it('subscribes exactly once per hook instance', async () => {
    renderHook(() => useInboxItems(), { wrapper })
    await waitFor(() => expect(api.listInboxItems).toHaveBeenCalledTimes(1))
    const subs = listenMock.mock.calls.filter(c => c[0] === 'inbox-updated')
    expect(subs).toHaveLength(1)
  })
})

describe('useInboxStats', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('loads stats on mount', async () => {
    vi.mocked(api.getInboxStats).mockResolvedValue({ pending: 2, today: 5 })
    const { result } = renderHook(() => useInboxStats(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.stats).toEqual({ pending: 2, today: 5 })
  })

  it('falls back to zeroed stats on a nullish payload', async () => {
    vi.mocked(api.getInboxStats).mockResolvedValue(undefined)
    const { result } = renderHook(() => useInboxStats(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.stats).toEqual({ pending: 0, today: 0 })
  })

  it('keeps zeroed stats when the command fails (badge stays sane)', async () => {
    vi.mocked(api.getInboxStats).mockRejectedValue('no backend')
    const { result } = renderHook(() => useInboxStats(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.error).toBe('no backend')
    expect(result.current.stats).toEqual({ pending: 0, today: 0 })
  })

  it('refreshes when the backend emits inbox-updated', async () => {
    vi.mocked(api.getInboxStats).mockResolvedValue({ pending: 1, today: 1 })
    renderHook(() => useInboxStats(), { wrapper })
    await waitFor(() => expect(api.getInboxStats).toHaveBeenCalledTimes(1))
    vi.mocked(api.getInboxStats).mockResolvedValue({ pending: 4, today: 6 })
    fireInboxUpdated()
    await waitFor(() => expect(api.getInboxStats).toHaveBeenCalledTimes(2))
  })
})
