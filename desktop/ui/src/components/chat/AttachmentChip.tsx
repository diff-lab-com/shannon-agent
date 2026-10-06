import { useIntl } from 'react-intl'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Button } from '@/components/ui/button'
import { rejectionReasonMessage } from '@/lib/attachmentFeedback'
import { extractionMessageParams } from '@/lib/pasteImage'
import type { AttachmentExtractionReport, RejectedAttachmentReason } from '@/types'

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
  /**
   * G3b P1-4 — extraction summary from the same preflight (parseable
   * documents only): a small badge whose tooltip says what the model will
   * actually receive ("extracted 23 sections, first 8 inlined" / "PDF shows
   * the first 50 KiB — the model can read the rest"). Advisory, like `issue`.
   */
  extraction?: AttachmentExtractionReport
  /**
   * R7-③ threshold hybrid — parseable document over the large-file
   * threshold: the preflight skipped the attach-time full parse, so instead
   * of an extraction badge the chip shows the honest "large file — parsed on
   * send" placeholder. The real badge lights from the send receipt.
   */
  deferredParse?: boolean
}

// B4 P2-11: the remove affordance is announced with the file's name; the
// dead `size`/`formatSize` display (no consumer ever passed one) is gone.
export function AttachmentChip({ path, onRemove, issue, extraction, deferredParse }: AttachmentChipProps) {
  const intl = useIntl()
  const name = path.split(/[/\\]/).pop() || path
  const image = /\.(png|jpe?g|webp|gif)$/i.test(name)
  // Badges only for real extraction results; a parse failure keeps the chip
  // unbadged (the send-time placeholder block still tells the model why).
  const extractionLabel =
    extraction && extraction.extracted
      ? intl.formatMessage(
          { id: extractionMessageParams(extraction).id },
          extractionMessageParams(extraction).values,
        )
      : null
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
      {extractionLabel && (
        <span
          data-testid="attachment-chip-extraction"
          aria-label={extractionLabel}
          title={extractionLabel}
          className="material-symbols-outlined icon-sm text-tertiary shrink-0"
        >
          text_snippet
        </span>
      )}
      {deferredParse && (
        <span
          data-testid="attachment-chip-deferred"
          aria-label={intl.formatMessage({ id: 'chat.attach.deferredParse.tooltip' })}
          title={intl.formatMessage({ id: 'chat.attach.deferredParse.tooltip' })}
          className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0"
        >
          hourglass_top
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
