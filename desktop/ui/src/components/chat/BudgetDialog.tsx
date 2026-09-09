// BudgetDialog — P0-4 "Set session budget…" input dialog.
//
// Enter a positive USD amount to cap the session's spend (the backend
// enforces pre-turn reject + mid-turn cancel), or clear the cap. Saves
// through `set_session_budget` (sidecar-backed) and reports back via
// `onSaved` so callers can refresh their badges.

import { useEffect, useState } from 'react'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'

export interface BudgetDialogProps {
  open: boolean
  sessionId: string | null
  /** Current cap (rendered into the input when opening). */
  budget: number | null
  onClose: () => void
  /** Called after a successful save/clear with the new cap value. */
  onSaved: (budget: number | null) => void
}

export default function BudgetDialog({ open, sessionId, budget, onClose, onSaved }: BudgetDialogProps) {
  const t = useT()
  const [value, setValue] = useState('')
  const [error, setError] = useState(false)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (open) {
      setValue(budget != null ? String(budget) : '')
      setError(false)
    }
  }, [open, budget])

  const save = async (raw: string) => {
    if (!sessionId) return
    const trimmed = raw.trim()
    let next: number | null = null
    if (trimmed !== '') {
      const parsed = Number(trimmed)
      if (!Number.isFinite(parsed) || parsed <= 0) {
        setError(true)
        return
      }
      next = parsed
    }
    setSaving(true)
    try {
      await api.setSessionBudget(sessionId, next)
      onSaved(next)
      onClose()
    } catch (e) {
      toastError(t('budget.dialog.title'), e)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={t('budget.dialog.title')} size="sm">
      <ModalBody className="pt-0">
        <label className="block">
          <span className="font-label-md text-on-surface-variant">{t('budget.dialog.label')}</span>
          <Input
            autoFocus
            type="number"
            min="0"
            step="0.01"
            placeholder={t('budget.dialog.placeholder')}
            value={value}
            aria-invalid={error || undefined}
            onChange={e => { setValue(e.target.value); setError(false) }}
            onKeyDown={e => { if (e.key === 'Enter') void save(value) }}
            className="mt-xs font-mono"
          />
        </label>
        {error && (
          <p role="alert" className="text-body-sm text-error mt-xs">{t('budget.dialog.invalid')}</p>
        )}
      </ModalBody>
      <ModalFooter className="pt-0">
        <Button
          variant="ghost"
          disabled={saving}
          className="px-md py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
          onClick={onClose}
        >
          {t('budget.dialog.cancel')}
        </Button>
        <Button
          variant="ghost"
          disabled={saving || budget == null}
          className="px-md py-sm rounded-xl text-error hover:bg-error/10 cursor-pointer disabled:opacity-40"
          onClick={() => void save('')}
        >
          {t('budget.dialog.clear')}
        </Button>
        <Button
          disabled={saving}
          className="px-md py-sm rounded-xl bg-primary text-on-primary hover:bg-primary/90 cursor-pointer"
          onClick={() => void save(value)}
        >
          {t('budget.dialog.save')}
        </Button>
      </ModalFooter>
    </Modal>
  )
}
