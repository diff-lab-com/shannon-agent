// B1 §4-9: removable chips for prompts queued while the session was still
// streaming. Rendered by ComposerPanel above the input — the queue itself
// lives in AppContext keyed by session and drains automatically when the
// run finishes.
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { useChat } from '@/context/ChatContext'

export default function QueueChips({ className }: { className?: string }) {
  const { promptQueue, removeQueuedPrompt } = useChat()
  const t = useT()
  if (promptQueue.length === 0) return null
  return (
    <div
      data-testid="prompt-queue"
      role="list"
      aria-label={t('chat.queue.aria')}
      className={cn('flex flex-wrap items-center gap-xs px-md pt-md', className)}
    >
      <span className="font-label-xs text-on-surface-variant flex items-center gap-1 shrink-0">
        <span className="material-symbols-outlined text-[14px]" aria-hidden="true">low_priority</span>
        {t('chat.queue.title', { count: promptQueue.length })}
      </span>
      {promptQueue.map(item => (
        <span
          key={item.id}
          role="listitem"
          data-testid="prompt-queue-chip"
          className="inline-flex items-center gap-1 max-w-[260px] px-sm py-[2px] rounded-full bg-secondary-container/50 border border-outline-variant/30 text-on-surface font-label-sm"
        >
          <span className="material-symbols-outlined text-[14px] shrink-0" aria-hidden="true">schedule</span>
          <span className="truncate" title={item.text}>
            {item.text.trim() || t('chat.queue.attachmentsOnly', { count: item.attachments.length })}
          </span>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={t('chat.queue.remove.aria')}
            title={t('chat.queue.remove.aria')}
            className="shrink-0 size-4 rounded-full hover:bg-error/10 hover:text-error"
            onClick={() => removeQueuedPrompt(item.id)}
          >
            <span className="material-symbols-outlined text-[12px]" aria-hidden="true">close</span>
          </Button>
        </span>
      ))}
    </div>
  )
}
