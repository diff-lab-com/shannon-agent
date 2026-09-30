import { useIntl } from 'react-intl'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { rejectionReasonMessage } from '@/lib/attachmentFeedback'
import type { RejectedAttachmentReason } from '@/types'

interface AttachmentChipProps {
  path: string
  onRemove: () => void
  /**
   * P0-3 preflight verdict — when the backend would refuse this path on
   * send, the chip shows a warning icon and its tooltip names the reason,
   * so the refusal is visible BEFORE the send instead of the chip silently
   * vanishing after it.
   */
  issue?: RejectedAttachmentReason
}

// B4 P2-11: the remove affordance is announced with the file's name; the
// dead `size`/`formatSize` display (no consumer ever passed one) is gone.
export function AttachmentChip({ path, onRemove, issue }: AttachmentChipProps) {
  const intl = useIntl()
  const name = path.split(/[/\\]/).pop() || path
  const image = /\.(png|jpe?g|webp|gif)$/i.test(name)
  return (
    <span className="inline-flex max-w-[240px] items-center gap-xs rounded-lg bg-primary-container px-sm py-xs text-on-primary-container font-label-sm">
      {image ? <img src={convertFileSrc(path)} alt={name} className="h-5 w-5 shrink-0 rounded-sm object-cover" /> : <span className="material-symbols-outlined icon-sm">description</span>}
      {issue && (
        <span
          data-testid="attachment-chip-issue"
          aria-label={intl.formatMessage({ id: 'chat.attach.issue.tooltip' }, { reason: rejectionReasonMessage(issue) })}
          title={intl.formatMessage({ id: 'chat.attach.issue.tooltip' }, { reason: rejectionReasonMessage(issue) })}
          className="material-symbols-outlined icon-sm text-warning shrink-0"
        >
          warning
        </span>
      )}
      <span className="truncate">{name}</span>
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={intl.formatMessage({ id: 'chat.input.attach.remove' }, { name })}
        title={intl.formatMessage({ id: 'chat.input.attach.remove' }, { name })}
        onClick={onRemove}
        className="hover:text-error"
      >
        <span className="material-symbols-outlined icon-sm">close</span>
      </Button>
    </span>
  )
}

export default AttachmentChip
