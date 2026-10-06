// PptOutlineDialog — office Wave 2 B2 v1: the PPT outline confirmation step.
//
// The "+" menu's "Build a presentation" entry opens this dialog; the user
// edits a one-slide-per-line outline and "Generate with agent" pushes a
// structured draft into the composer via pushComposerDraft. Trust contract
// (Wave 1 theme): the dialog NEVER sends — the draft lands in the composer
// for the user to review and send themselves.

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
 * The exact instruction prefix of the pushed draft — stable wording the
 * document skills can recognize downstream. Not localized on purpose: it is
 * a prompt to the agent, not UI copy.
 */
export const PPT_DRAFT_PREFIX = 'Build a presentation from exactly this outline (one slide per line):'

/** Compose the composer draft for an outline (exported for tests). */
export function pptDraftOf(outline: string): string {
  return `${PPT_DRAFT_PREFIX}\n${outline}`
}

/** Initial 6-line sample outline — every line editable, one slide per line. */
export const SAMPLE_PPT_OUTLINE = [
  'Title — Product overview',
  'The problem: busywork agents create',
  'What Shannon does differently',
  'Live demo: from prompt to deck',
  'Architecture and trust model',
  'Roadmap and Q&A',
].join('\n')

export interface PptOutlineDialogProps {
  open: boolean
  /** Close request from Generate, the X button, Escape or the backdrop. */
  onClose: () => void
}

export function PptOutlineDialog({ open, onClose }: PptOutlineDialogProps) {
  const t = useT()
  const [outline, setOutline] = useState(SAMPLE_PPT_OUTLINE)

  // Re-arm the sample each open — a dialog reopened days later should not
  // resurrect last time's half-edited outline.
  useEffect(() => {
    if (open) setOutline(SAMPLE_PPT_OUTLINE)
  }, [open])

  const generate = () => {
    const trimmed = outline.trim()
    if (!trimmed) return
    pushComposerDraft(pptDraftOf(trimmed))
    onClose()
  }

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) onClose() }}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t('office.ppt.title')}</DialogTitle>
          <DialogDescription className="sr-only">{t('office.ppt.outlineLabel')}</DialogDescription>
        </DialogHeader>
        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface-variant">{t('office.ppt.outlineLabel')}</span>
          <textarea
            aria-label={t('office.ppt.outlineLabel')}
            data-testid="ppt-outline-input"
            value={outline}
            onChange={e => setOutline(e.target.value)}
            rows={8}
            className="w-full p-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm resize-y focus:outline-none focus:ring-2 focus:ring-primary/30 font-body-md"
          />
          <span className="font-label-xs text-on-surface-variant">{t('office.ppt.outlineHint')}</span>
        </label>
        <DialogFooter>
          <Button
            onClick={generate}
            disabled={!outline.trim()}
            className="px-md py-sm rounded-lg bg-primary text-on-primary font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50"
          >
            {t('office.ppt.generate')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default PptOutlineDialog
