// Modal wrapper around the shared DiffReviewBody (single-file diff review
// with per-hunk accept/reject + Apply flow). The body lives in
// DiffReviewBody.tsx so the chat right dock's Diff tab can host the same
// review surface non-modally; this dialog keeps the modal entry point for
// surfaces that prefer it (multi-file "Review All" keeps DiffDialogMulti).
//
// T1.2 — built on the shared Modal primitive. Focus management, Esc-to-close,
// body scroll lock, and backdrop click are handled by the primitive. The
// close button's aria-label is supplied via Modal's `closeLabel` prop so the
// existing test contract ("Close diff") keeps working.

import { useIntl } from 'react-intl'
import { Modal } from '@/components/ui/modal'
import DiffReviewBody from '@/components/diff/DiffReviewBody'

interface DiffDialogProps {
  open: boolean
  filePath: string | null
  onClose: () => void
}

export default function DiffDialog({ open, filePath, onClose }: DiffDialogProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="2xl"
      title={t('diff.dialog.title')}
      description={filePath ?? undefined}
      closeLabel={t('diff.dialog.close.aria')}
      className="max-w-5xl flex flex-col max-h-[90vh]"
    >
      <DiffReviewBody filePath={filePath} onClose={onClose} active={open} />
    </Modal>
  )
}
