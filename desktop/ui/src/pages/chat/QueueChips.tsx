// B1 §4-9: removable chips for prompts queued while the session was still
// streaming. Rendered by ComposerPanel above the input — the queue itself
// lives in AppContext keyed by session and drains automatically when the
// run finishes.
// GB P2-10a: the FIFO is steerable — each chip carries up/down controls
// (move toward the head = sends sooner) next to its dismiss button.
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { useChat } from '@/context/ChatContext'

export default function QueueChips({ className }: { className?: string }) {
  const { promptQueue, removeQueuedPrompt, moveQueuedPrompt } = useChat()
  const t = useT()
  if (promptQueue.length === 0) return null
  return (
    <div
      data-testid="prompt-queue"
      role="list"
      aria-label={t('chat.queue.aria')}
      className={cn('flex flex-wrap items-center gap-xs px-md pt-md', className)}
    >
      <span className="font-label-xs text-on-surface-variant flex items-center gap-xs shrink-0">
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">low_priority</span>
        {t('chat.queue.title', { count: promptQueue.length })}
      </span>
      {promptQueue.map((item, i) => (
        <span
          key={item.id}
          role="listitem"
          data-testid="prompt-queue-chip"
          className="inline-flex items-center gap-xs max-w-[260px] px-sm py-[2px] rounded-full bg-secondary-container/50 border border-outline-variant/30 text-on-surface font-label-sm"
        >
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">schedule</span>
          <span className="truncate" title={item.text}>
            {item.text.trim() || t('chat.queue.attachmentsOnly', { count: item.attachments.length })}
          </span>
          {promptQueue.length > 1 && (
            <span className="flex items-center shrink-0">
              <Button
                variant="ghost"
                size="icon-xs"
                disabled={i === 0}
                aria-label={t('chat.queue.up.aria')}
                title={t('chat.queue.up.aria')}
                className="size-4 rounded-full hover:bg-surface-container-high"
                onClick={() => moveQueuedPrompt(item.id, -1)}
              >
                <span className="material-symbols-outlined icon-xs" aria-hidden="true">keyboard_arrow_up</span>
              </Button>
              <Button
                variant="ghost"
                size="icon-xs"
                disabled={i === promptQueue.length - 1}
                aria-label={t('chat.queue.down.aria')}
                title={t('chat.queue.down.aria')}
                className="size-4 rounded-full hover:bg-surface-container-high"
                onClick={() => moveQueuedPrompt(item.id, 1)}
              >
                <span className="material-symbols-outlined icon-xs" aria-hidden="true">keyboard_arrow_down</span>
              </Button>
            </span>
          )}
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={t('chat.queue.remove.aria')}
            title={t('chat.queue.remove.aria')}
            className="shrink-0 size-4 rounded-full hover:bg-error/10 hover:text-error"
            onClick={() => removeQueuedPrompt(item.id)}
          >
            <span className="material-symbols-outlined icon-xs" aria-hidden="true">close</span>
          </Button>
        </span>
      ))}
    </div>
  )
}
