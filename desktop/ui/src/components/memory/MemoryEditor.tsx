// Memory panel — create/edit modal form for a single MemoryEntry. Extracted
// from MemoryPanel.tsx (T3.1). Hosts its own state, dirty-tracking, and an
// inline discard-confirmation dialog.
import { useState, type FormEvent } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/ui/modal'
import type { MemoryCategory, MemoryEntry } from '@/lib/tauri-api'
import { Field } from './Field'

export interface MemorySaveInput {
  id?: string
  project: string
  category: MemoryCategory
  content: string
  tags: string[]
  confidence: number
}

interface MemoryEditorProps {
  initial: MemoryEntry | null
  onCancel: () => void
  onSave: (input: MemorySaveInput) => Promise<void>
}

export function MemoryEditor({ initial, onCancel, onSave }: MemoryEditorProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [project, setProject] = useState(initial?.project ?? '.')
  const [category, setCategory] = useState<MemoryCategory>(initial?.category ?? 'context')
  const [content, setContent] = useState(initial?.content ?? '')
  const [tagsInput, setTagsInput] = useState((initial?.tags ?? []).join(', '))
  const [confidence, setConfidence] = useState(initial?.confidence ?? 1.0)
  const [saving, setSaving] = useState(false)
  const [confirmDiscard, setConfirmDiscard] = useState(false)

  const isDirty = () =>
    project !== (initial?.project ?? '.') ||
    category !== (initial?.category ?? 'context') ||
    content !== (initial?.content ?? '') ||
    tagsInput !== (initial?.tags ?? []).join(', ') ||
    confidence !== (initial?.confidence ?? 1.0)

  const attemptCancel = () => {
    if (isDirty()) setConfirmDiscard(true)
    else onCancel()
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    if (!content.trim() || !project.trim()) return
    setSaving(true)
    try {
      await onSave({
        id: initial?.id,
        project: project.trim(),
        category,
        content: content.trim(),
        tags: tagsInput
          .split(',')
          .map((tag) => tag.trim())
          .filter(Boolean),
        confidence,
      })
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal
      open
      onClose={attemptCancel}
      size="2xl"
      title={initial ? t('memory.editor.edit') : t('memory.editor.create')}
      showCloseButton={false}
      className="overflow-hidden"
    >
      <form onSubmit={handleSubmit}>
        <header className="flex items-center justify-between px-lg py-md border-b border-outline-variant/30">
          <h2 className="text-label-lg font-bold text-on-surface">
            {initial ? t('memory.editor.edit') : t('memory.editor.create')}
          </h2>
          <Button
            variant="ghost"
            size="icon-sm"
            type="button"
            onClick={attemptCancel}
            className="rounded hover:bg-surface-container-high"
            aria-label={t('memory.action.close')}
          >
            <span className="material-symbols-outlined icon-md text-on-surface-variant">close</span>
          </Button>
        </header>

        <div className="p-lg space-y-md max-h-[60vh] overflow-y-auto">
          <div className="grid grid-cols-2 gap-md">
            <Field label={t('memory.editor.project')}>
              <input
                value={project}
                onChange={(e) => setProject(e.target.value)}
                required
                className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-label-md"
                placeholder={t('memory.editor.project.placeholder')}
              />
            </Field>
            <Field label={t('memory.editor.category')}>
              <select
                value={category}
                onChange={(e) => setCategory(e.target.value as MemoryCategory)}
                className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-label-md"
              >
                {(['preference', 'pattern', 'decision', 'error', 'context'] as MemoryCategory[]).map((c) => (
                  <option key={c} value={c}>
                    {t(`memory.category.${c}`)}
                  </option>
                ))}
              </select>
            </Field>
          </div>

          <Field label={t('memory.editor.content')}>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              required
              rows={5}
              className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-body-md font-mono"
              placeholder={t('memory.editor.contentPlaceholder')}
            />
          </Field>

          <div className="grid grid-cols-2 gap-md">
            <Field label={t('memory.editor.tags')}>
              <input
                value={tagsInput}
                onChange={(e) => setTagsInput(e.target.value)}
                className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-label-md"
                placeholder={t('memory.editor.tags.placeholder')}
              />
            </Field>
            {!initial && (
              <Field label={`${t('memory.editor.confidence')} (${confidence.toFixed(2)})`}>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={confidence}
                  onChange={(e) => setConfidence(Number(e.target.value))}
                  className="w-full"
                />
              </Field>
            )}
          </div>
        </div>

        <footer className="flex justify-end gap-sm px-lg py-md border-t border-outline-variant/30 bg-surface-container-lowest">
          <Button
            variant="secondary"
            type="button"
            onClick={attemptCancel}
            className="px-md py-sm rounded-lg text-label-md font-bold"
          >
            {t('memory.editor.cancel')}
          </Button>
          <Button
            type="submit"
            disabled={saving || !content.trim() || !project.trim()}
            className="px-md py-sm rounded-lg text-label-md font-bold"
          >
            {saving ? t('memory.editor.saving') : t('memory.editor.save')}
          </Button>
        </footer>

        {confirmDiscard && (
          <div
            role="alertdialog"
            aria-label={t('memory.editor.discard.title')}
            className="absolute inset-0 z-10 bg-black/40 flex items-center justify-center p-md"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full">
              <div className="flex items-center gap-sm mb-md">
                <span className="material-symbols-outlined text-error text-[24px]">warning</span>
                <h3 className="font-headline-md text-on-surface">{t('memory.editor.discard.title')}</h3>
              </div>
              <p className="text-body-md text-on-surface-variant mb-lg">
                {t('memory.editor.discard.message')}
              </p>
              <div className="flex justify-end gap-sm">
                <Button
                  variant="secondary"
                  type="button"
                  onClick={() => setConfirmDiscard(false)}
                  className="px-md py-sm rounded-lg text-label-md font-bold"
                >
                  {t('memory.editor.discard.keep')}
                </Button>
                <Button
                  type="button"
                  onClick={() => {
                    setConfirmDiscard(false)
                    onCancel()
                  }}
                  className="px-md py-sm rounded-lg bg-error text-on-error text-label-md font-bold hover:bg-error/90"
                >
                  {t('memory.editor.discard.confirm')}
                </Button>
              </div>
            </div>
          </div>
        )}
      </form>
    </Modal>
  )
}
