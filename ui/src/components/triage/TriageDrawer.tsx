import { useState, useCallback, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { Drawer, DrawerContent, DrawerHeader, DrawerTitle } from '@/components/ui/drawer'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { TriageItem, TriageFilter } from '@/types'

interface TriageDrawerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onStatsRefresh?: () => void
}

export function TriageDrawer({ open, onOpenChange, onStatsRefresh }: TriageDrawerProps) {
  const intl = useIntl()
  const [items, setItems] = useState<TriageItem[]>([])
  const [filter, setFilter] = useState<TriageFilter>({ unarchived_only: true })
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      setItems(await api.listTriageItems(filter))
    } catch (e) {
      console.error('Failed to load triage items:', e)
      toast.error(intl.formatMessage({ id: 'triage.loadError' }))
    } finally {
      setLoading(false)
    }
  }, [filter, intl])

  const markRead = useCallback(async (id: string) => {
    try {
      await api.markTriageRead(id)
      toast.success(intl.formatMessage({ id: 'triage.markReadSuccess' }))
      await refresh()
      onStatsRefresh?.()
    } catch (e) {
      console.error('Failed to mark item read:', e)
      toast.error(intl.formatMessage({ id: 'triage.markReadError' }))
    }
  }, [intl, refresh, onStatsRefresh])

  const archive = useCallback(async (id: string) => {
    try {
      await api.archiveTriageItem(id)
      toast.success(intl.formatMessage({ id: 'triage.archiveSuccess' }))
      await refresh()
      onStatsRefresh?.()
    } catch (e) {
      console.error('Failed to archive item:', e)
      toast.error(intl.formatMessage({ id: 'triage.archiveError' }))
    }
  }, [intl, refresh, onStatsRefresh])

  const openLinked = useCallback((item: TriageItem) => {
    if (item.task_id) {
      toast.info(intl.formatMessage({ id: 'triage.openTask' }, { taskId: item.task_id }))
    } else if (item.run_id) {
      toast.info(intl.formatMessage({ id: 'triage.openRun' }, { runId: item.run_id }))
    }
  }, [intl])

  const setFilterUnreadOnly = useCallback((unreadOnly: boolean) => {
    setFilter(prev => ({ ...prev, unread_only: unreadOnly }))
  }, [])

  const setFilterKind = useCallback((kind: string | null) => {
    setFilter(prev => ({ ...prev, kind: kind ?? undefined }))
  }, [])

  // Load items when drawer opens or filter changes
  useEffect(() => {
    if (open) {
      refresh()
    }
  }, [open, filter, refresh])

  const kindIcon = (kind: string) => {
    switch (kind) {
      case 'failed_run': return 'error'
      case 'budget_exceeded': return 'payments'
      case 'needs_review': return 'rate_review'
      case 'timeout': return 'schedule'
      default: return 'notification'
    }
  }

  const formatDate = (ts: number) => {
    return new Date(ts * 1000).toLocaleString()
  }

  return (
    <Drawer open={open} onOpenChange={onOpenChange}>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>{intl.formatMessage({ id: 'triage.title' })}</DrawerTitle>
        </DrawerHeader>

        <div className="px-4 pb-2 flex gap-2 flex-wrap">
          <Button
            variant={filter.unread_only ? "default" : "outline"}
            size="sm"
            onClick={() => setFilterUnreadOnly(!filter.unread_only)}
          >
            {intl.formatMessage({ id: filter.unread_only ? 'triage.filterUnread' : 'triage.filterAll' })}
          </Button>

          <Button
            variant={filter.kind === 'failed_run' ? "default" : "outline"}
            size="sm"
            onClick={() => setFilterKind(filter.kind === 'failed_run' ? null : 'failed_run')}
          >
            <span className="material-symbols-outlined text-sm mr-1">error</span>
            {intl.formatMessage({ id: 'triage.kindFailedRun' })}
          </Button>

          <Button
            variant={filter.kind === 'budget_exceeded' ? "default" : "outline"}
            size="sm"
            onClick={() => setFilterKind(filter.kind === 'budget_exceeded' ? null : 'budget_exceeded')}
          >
            <span className="material-symbols-outlined text-sm mr-1">payments</span>
            {intl.formatMessage({ id: 'triage.kindBudget' })}
          </Button>

          <Button
            variant={filter.kind === 'needs_review' ? "default" : "outline"}
            size="sm"
            onClick={() => setFilterKind(filter.kind === 'needs_review' ? null : 'needs_review')}
          >
            <span className="material-symbols-outlined text-sm mr-1">rate_review</span>
            {intl.formatMessage({ id: 'triage.kindReview' })}
          </Button>
        </div>

        <ScrollArea className="flex-1 px-4">
          {loading ? (
            <div className="text-center py-8 text-on-surface-variant">
              {intl.formatMessage({ id: 'triage.loading' })}
            </div>
          ) : items.length === 0 ? (
            <div className="text-center py-8 text-on-surface-variant">
              {intl.formatMessage({ id: 'triage.empty' })}
            </div>
          ) : (
            <div className="space-y-2">
              {items.map(item => (
                <div
                  key={item.id}
                  className={`p-3 rounded-lg border transition-all ${
                    item.read
                      ? 'bg-surface-container-low border-outline-variant/30'
                      : 'bg-surface-container-high border-primary/50 shadow-sm'
                  }`}
                >
                  <div className="flex items-start gap-3">
                    <span className="material-symbols-outlined text-[20px] text-tertiary mt-0.5">
                      {kindIcon(item.kind)}
                    </span>

                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-xs font-mono text-on-surface-variant">
                          {item.kind}
                        </span>
                        {!item.read && (
                          <span className="w-2 h-2 rounded-full bg-error" />
                        )}
                      </div>

                      <p className="text-sm font-medium text-on-surface mb-1">
                        {item.message}
                      </p>

                      <div className="flex items-center gap-2 text-xs text-on-surface-variant">
                        <span>{formatDate(item.created_at)}</span>
                        {(item.task_name || item.task_id) && (
                          <>
                            <span>•</span>
                            <span className="text-primary">{item.task_name || item.task_id}</span>
                          </>
                        )}
                      </div>
                    </div>

                    <div className="flex flex-col gap-1 shrink-0">
                      {!item.read && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 px-2 text-xs"
                          onClick={() => markRead(item.id)}
                        >
                          {intl.formatMessage({ id: 'triage.markRead' })}
                        </Button>
                      )}

                      {(item.task_id || item.run_id) && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-8 px-2 text-xs"
                          onClick={() => openLinked(item)}
                        >
                          {intl.formatMessage({ id: 'triage.openLinked' })}
                        </Button>
                      )}

                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-8 px-2 text-xs text-error"
                        onClick={() => archive(item.id)}
                      >
                        {intl.formatMessage({ id: 'triage.archive' })}
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </ScrollArea>
      </DrawerContent>
    </Drawer>
  )
}
