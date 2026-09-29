import { lazy, Suspense, useEffect, useState } from 'react'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'
import { useT } from '@/i18n'
import { toastError } from '@/lib/errorToast'
import { registerFileIndexEntry, copyFile, openWithDefaultApp, revealInFolder } from '@/lib/tauri-api'

// B8b: the pdf.js preview (and its ~1 MB pdfjs-dist chunk) only loads when a
// preview button is actually clicked — FileCard itself stays on the chat
// page's critical path.
const PdfPreview = lazy(() => import('./PdfPreview').then((m) => ({ default: m.PdfPreview })))
// office Wave 3 C2: per-row batch execution over csv tables (tiny dialog).
import { BatchRunDialog } from './BatchRunDialog'

/**
 * FileCard — office Wave 1 (docs/research/2026-09-29-office-scenario-
 * competitive-research.md §10 v2, items A5 + B8a): one card shared by
 * user-message attachments and engine-generated files.
 *
 * Replaces the non-image attachment chip → lightbox detour (whose only
 * action was "open externally") with semantic actions:
 *   0. preview (PDF only, Wave 1.5 B8b) — inline pdf.js preview modal, so a
 *      generated PDF can be eyeballed without leaving the chat or spawning
 *      an external viewer;
 *   1. open — hand the file to the OS default app (P2-5 §4 convention,
 *      never a webview asset URL);
 *   2. reveal — show the file in the platform file manager;
 *   3. save as — native `save` dialog, then `copy_file` on confirmation.
 *      The user cancelling the dialog (null resolution) backs out silently.
 *
 * Visual language mirrors the old attachment chip: secondary surface
 * (`bg-surface-container-low` → `hover:bg-surface-container`), muted icon
 * and text, press-scale ghost buttons with the Button primitive's built-in
 * focus-visible ring.
 */

export interface FileCardProps {
  /** Display name (usually the basename) — truncated; the tooltip carries it in full. */
  name: string
  /** Path handed to the open / reveal / copy wrappers. */
  path: string
  /** Optional byte size, rendered as a human-readable B/KB/MB/GB label. */
  sizeBytes?: number
  /**
   * B9' — how this card entered the chat ('attachment' | 'generated').
   * Drives the Files-page index registration fired on mount. Defaults to
   * 'generated' (the engine-write case the card was built for).
   */
  source?: string
  /**
   * B7' — present only for engine-written files: adds the "Review changes"
   * secondary button that docks the single-file diff review in the
   * RightDock (Chat.tsx routes onViewDiff → setDiffPath → dock Diff tab).
   * User attachments never carry it.
   */
  onReviewDiff?: () => void
}

/** 1024-based compact size — one decimal under 10, rounded above. */
export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const kb = bytes / 1024
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)} KB`
  const mb = kb / 1024
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`
  const gb = mb / 1024
  return `${gb < 10 ? gb.toFixed(1) : Math.round(gb)} GB`
}

/** B8b: inline preview is a PDF-only affordance — other types keep open/reveal/save-as. */
function isPdfPath(path: string): boolean {
  return path.split('.').pop()?.toLowerCase() === 'pdf'
}

/** C2: "Batch run" is a csv-table affordance — the per-row instruction flow
 *  assumes a header + data rows spreadsheet shape. */
function isCsvPath(path: string): boolean {
  return path.split('.').pop()?.toLowerCase() === 'csv'
}

export function FileCard({ name, path, sizeBytes, source = 'generated', onReviewDiff }: FileCardProps) {
  const t = useT()
  const [previewOpen, setPreviewOpen] = useState(false)
  // C2: csv cards grow a "Batch run" affordance (dialog → composer draft).
  const [batchOpen, setBatchOpen] = useState(false)

  // B9' Files page: every rendered card is a durable file reference — index
  // it (upsert Rust-side) so the library lists it. Fire-and-forget with a
  // swallowed rejection: an index write failure must never surface in chat.
  useEffect(() => {
    registerFileIndexEntry(path, source).catch(() => {})
  }, [path, source])

  const handleOpen = () => {
    openWithDefaultApp(path).catch((err) => toastError(t('link.open.failed'), err))
  }

  const handleReveal = () => {
    revealInFolder(path).catch((err) => toastError(t('link.open.failed'), err))
  }

  const handleSaveAs = async () => {
    // `save` resolves null when the user cancels — back out silently.
    const destPath = await save({ defaultPath: name })
    if (destPath == null || destPath === '') return
    try {
      await copyFile(path, destPath)
      toast.success(t('chat.message.filecard.saved', { path: destPath }))
    } catch (err) {
      toastError(t('chat.message.filecard.saveFailed'), err)
    }
  }

  return (
    <>
      <div
        data-testid="file-card"
        title={path}
        className="group/filecard flex w-full max-w-sm items-center gap-sm rounded-lg border border-outline-variant/20 bg-surface-container-low px-sm py-xs transition-colors hover:bg-surface-container"
      >
        <Icon
          name="draft"
          size="md"
          className="shrink-0 text-on-surface-variant transition-colors group-hover/filecard:text-primary"
        />
        <div className="min-w-0 flex-1">
          <span className="block font-label-sm text-on-surface truncate" title={name}>
            {name}
          </span>
          {sizeBytes != null && (
            <span className="block font-label-xs text-on-surface-variant tabular-nums">
              {formatFileSize(sizeBytes)}
            </span>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-xs">
          {/* B7' — engine-written files only: dock the single-file diff review
              (RightDock Diff tab). Secondary emphasis next to the OS actions. */}
          {onReviewDiff && (
            <Button
              variant="ghost"
              size="sm"
              data-testid="file-card-review-diff"
              aria-label={t('office.diff.review')}
              title={t('office.diff.review')}
              onClick={onReviewDiff}
              className="gap-xs px-xs py-[2px] text-tertiary hover:bg-tertiary-container/40"
            >
              <span className="material-symbols-outlined icon-sm">difference</span>
              <span className="hidden md:inline">{t('office.diff.review')}</span>
            </Button>
          )}
          {/* C2 — csv only: opens the per-row batch instruction dialog. The
              visible label is the dialog's own "Build prompt" verb (the
              affordance IS building the prompt); title carries the full
              "batch run" context. */}
          {isCsvPath(path) && (
            <Button
              variant="ghost"
              size="sm"
              data-testid="file-card-batch-run"
              aria-label={t('office.batch.title')}
              title={t('office.batch.title')}
              onClick={() => setBatchOpen(true)}
              className="gap-xs px-xs py-[2px] text-tertiary hover:bg-tertiary-container/40"
            >
              <span className="material-symbols-outlined icon-sm">checklist</span>
              <span className="hidden md:inline">{t('office.batch.build')}</span>
            </Button>
          )}
          {isPdfPath(path) && (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t('chat.message.filecard.preview')}
              title={t('chat.message.filecard.preview')}
              onClick={() => setPreviewOpen(true)}
              className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
            >
              <Icon name="visibility" />
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('chat.message.attachment.open')}
            title={t('chat.message.attachment.open')}
            onClick={handleOpen}
            className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
          >
            <Icon name="open_in_new" />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('chat.message.filecard.reveal')}
            title={t('chat.message.filecard.reveal')}
            onClick={handleReveal}
            className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
          >
            <Icon name="folder_open" />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('chat.message.filecard.saveAs')}
            title={t('chat.message.filecard.saveAs')}
            onClick={() => void handleSaveAs()}
            className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
          >
            <Icon name="save" />
          </Button>
        </div>
      </div>
      {/* B8b: inline PDF preview — lazy chunk, mounted only while open. */}
      {previewOpen && (
        <Suspense fallback={null}>
          <PdfPreview path={path} name={name} onClose={() => setPreviewOpen(false)} />
        </Suspense>
      )}
      {/* C2: batch instruction dialog — pushes a composer draft, never sends. */}
      <BatchRunDialog open={batchOpen} path={path} name={name} onClose={() => setBatchOpen(false)} />
    </>
  )
}
