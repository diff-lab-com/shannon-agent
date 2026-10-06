// BatchRunDialog — office Wave 3 C2: batch execution over a table's rows.
//
// The FileCard "Batch run" button (csv attachments only) opens this dialog;
// the user writes ONE per-row instruction and "Build prompt" pushes a
// structured draft into the composer via pushComposerDraft. Trust contract
// (Wave 1/2 theme): the dialog NEVER sends — the draft lands in the composer
// for the user to review and send themselves, and the engine reads the file
// and iterates the data rows.

import { useEffect, useState } from 'react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { pushComposerDraft } from '@/lib/composerBridge'

/**
 * The exact draft template — stable wording the engine-side batch flow can
 * recognize downstream. Not localized on purpose: it is a prompt to the
 * agent, not UI copy.
 */
export function batchDraftOf(path: string, name: string, instruction: string): string {
  // `<name>-enriched.<ext>`: base name of the card label, extension from the
  // real path (they agree in practice; the path wins for the suffix).
  const dot = name.lastIndexOf('.')
  const base = dot > 0 ? name.slice(0, dot) : name
  const pathDot = path.lastIndexOf('.')
  const ext = pathDot > -1 ? path.slice(pathDot + 1).toLowerCase() : 'csv'
  return (
    `Read ${path} and apply the following instruction to each data row, ` +
    `writing results to a new file ${base}-enriched.${ext}:\n${instruction.trim()}`
  )
}

export interface BatchRunDialogProps {
  open: boolean
  /** The table file the batch runs over (the card's path). */
  path: string
  /** Card display name — contributes the `<name>-enriched` output name. */
  name: string
  /** Close request from Build, the X button, Escape or the backdrop. */
  onClose: () => void
}

export function BatchRunDialog({ open, path, name, onClose }: BatchRunDialogProps) {
  const t = useT()
  const [instruction, setInstruction] = useState('')

  // Re-arm empty each open — a dialog reopened later should not resurrect a
  // stale instruction aimed at a different table.
  useEffect(() => {
    if (open) setInstruction('')
  }, [open])

  const build = () => {
    const trimmed = instruction.trim()
    if (!trimmed) return
    pushComposerDraft(batchDraftOf(path, name, trimmed))
    onClose()
  }

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) onClose() }}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t('office.batch.title')}</DialogTitle>
          <DialogDescription className="sr-only">{t('office.batch.instructionLabel')}</DialogDescription>
        </DialogHeader>
        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface-variant">{t('office.batch.instructionLabel')}</span>
          <textarea
            aria-label={t('office.batch.instructionLabel')}
            data-testid="batch-instruction-input"
            value={instruction}
            onChange={e => setInstruction(e.target.value)}
            rows={6}
            className="w-full p-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm resize-y focus:outline-none focus:ring-2 focus:ring-primary/30 font-body-md"
          />
          <span className="font-label-xs text-on-surface-variant">{t('office.batch.hint')}</span>
        </label>
        <DialogFooter>
          <Button
            onClick={build}
            disabled={!instruction.trim()}
            data-testid="batch-build-prompt"
            className="px-md py-sm rounded-lg bg-primary text-on-primary font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50"
          >
            {t('office.batch.build')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default BatchRunDialog
