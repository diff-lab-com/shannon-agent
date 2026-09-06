// React hooks wrapping the P0-3 inbox Tauri commands (SQLite inbox).
//
// Mirrors the shape of the triage hooks in `scheduled-tasks.ts`:
// each hook owns its loading/error state, exposes action functions that
// toast on success/failure, and re-fetches when the backend emits
// `inbox-updated` (fired after status changes and when a routine run
// appends a new item) so badges and the list stay fresh without polling.

import { useCallback, useEffect, useState } from 'react'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import { EVENT_NAMES } from '@/types'
import type { InboxItem, InboxItemStatus, InboxListFilter, InboxStats } from '@/types'
import { useTauriEvent } from '@/hooks/useTauriEvent'

// ─── Inbox items ───────────────────────────────────────────────────────────

export function useInboxItems(initialFilter?: InboxListFilter) {
  const t = useT()
  const [filter, setFilter] = useState<InboxListFilter | undefined>(initialFilter)
  const [items, setItems] = useState<InboxItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setItems((await api.listInboxItems(filter)) ?? [])
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useInboxItems.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [filter])

  const setStatus = useCallback(async (id: number, status: InboxItemStatus): Promise<boolean> => {
    try {
      await api.updateInboxItemStatus(id, status)
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('inbox.toast.failed.status')
      setError(msg)
      toastError(t('inbox.toast.failed.status'), e)
      return false
    }
  }, [refresh, t])

  const markRead = useCallback(async (id: number): Promise<boolean> => {
    const ok = await setStatus(id, 'read')
    if (ok) toast.success(t('inbox.toast.markRead'))
    return ok
  }, [setStatus, t])

  const archive = useCallback(async (id: number): Promise<boolean> => {
    const ok = await setStatus(id, 'archived')
    if (ok) toast.success(t('inbox.toast.archived'))
    return ok
  }, [setStatus, t])

  const rerun = useCallback(async (id: number): Promise<string | null> => {
    try {
      const runId = await api.rerunInboxItem(id)
      toast.success(t('inbox.toast.rerunStarted'))
      return runId
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('inbox.toast.failed.rerun')
      setError(msg)
      toastError(t('inbox.toast.failed.rerun'), e)
      return null
    }
  }, [t])

  // Resolve the session linked to an item so the page can switch to it.
  // Navigation stays in the page — the hook only does data + error toast.
  const getSessionId = useCallback(async (id: number): Promise<string | null> => {
    try {
      return await api.continueInboxItemSession(id)
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('inbox.toast.failed.continue')
      setError(msg)
      toastError(t('inbox.toast.failed.continue'), e)
      return null
    }
  }, [t])

  useEffect(() => { refresh() }, [refresh])

  // Backend pushes `inbox-updated` after status writes and when a routine
  // run appends an item — refresh so a mounted inbox never goes stale.
  useTauriEvent(EVENT_NAMES.INBOX_UPDATED, () => { void refresh() })

  return { items, loading, error, filter, setFilter, refresh, markRead, archive, rerun, getSessionId }
}

// ─── Inbox stats (sidebar badge / header chips) ────────────────────────────

const EMPTY_STATS: InboxStats = { pending: 0, today: 0 }

export function useInboxStats() {
  const [stats, setStats] = useState<InboxStats>(EMPTY_STATS)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      // `?? EMPTY_STATS` keeps badge consumers safe if a bridge returns a
      // nullish payload (e.g. mocked invoke in tests / demo edge cases).
      setStats((await api.getInboxStats()) ?? EMPTY_STATS)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useInboxStats.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { refresh() }, [refresh])

  useTauriEvent(EVENT_NAMES.INBOX_UPDATED, () => { void refresh() })

  return { stats, loading, error, refresh }
}
