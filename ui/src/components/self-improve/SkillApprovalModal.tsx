import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Modal } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Input } from '@/components/ui/input'
import * as api from '@/lib/tauri-api'
import type { SkillCandidate, AgentAuthoredSkill } from '@/lib/tauri-api'

interface SkillApprovalModalProps {
  open: boolean
  candidate: SkillCandidate | null
  onClose: () => void
  onApproved?: (skill: AgentAuthoredSkill) => void
  onRejected?: (id: string) => void
}

export function SkillApprovalModal({ open, candidate, onClose, onApproved, onRejected }: SkillApprovalModalProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [name, setName] = useState('')
  const [trigger, setTrigger] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (candidate) {
      setName(candidate.proposed_name)
      setTrigger(candidate.proposed_trigger)
    }
  }, [candidate])

  if (!candidate) return null

  const handleApprove = async () => {
    setBusy(true)
    try {
      const skill = await api.approveSkillCandidate(candidate.id, { name, trigger })
      toast.success(t('skills.approval.approved'))
      onApproved?.(skill)
      onClose()
    } catch (err) {
      console.warn('Approve failed:', err)
      toast.error(t('skills.approval.approveFailed'))
    } finally {
      setBusy(false)
    }
  }

  const handleReject = async () => {
    setBusy(true)
    try {
      await api.rejectSkillCandidate(candidate.id)
      toast.success(t('skills.approval.rejected'))
      onRejected?.(candidate.id)
      onClose()
    } catch (err) {
      console.warn('Reject failed:', err)
      toast.error(t('skills.approval.rejectFailed'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t('skills.approval.title')}
      description={intl.formatMessage(
        { id: 'skills.approval.description' },
        { count: candidate.occurrence_count }
      )}
      size="md"
      busy={busy}
    >
      <div className="space-y-md p-md">
        <div className="space-y-xs">
          <label className="font-label-sm text-on-surface-variant" htmlFor="skill-name">
            {t('skills.approval.nameLabel')}
          </label>
          <Input
            id="skill-name"
            value={name}
            onChange={e => setName(e.target.value)}
            disabled={busy}
          />
        </div>
        <div className="space-y-xs">
          <label className="font-label-sm text-on-surface-variant" htmlFor="skill-trigger">
            {t('skills.approval.triggerLabel')}
          </label>
          <Textarea
            id="skill-trigger"
            value={trigger}
            onChange={e => setTrigger(e.target.value)}
            rows={2}
            disabled={busy}
            className="resize-none"
          />
        </div>
        <div className="space-y-xs">
          <label className="font-label-sm text-on-surface-variant">
            {t('skills.approval.procedureLabel')}
          </label>
          <ol className="list-decimal list-inside space-y-xs text-body-sm text-on-surface bg-surface-container-low rounded-lg p-md">
            {candidate.procedure.map((step, i) => (
              <li key={i}>{step}</li>
            ))}
          </ol>
        </div>
      </div>
      <div className="flex items-center justify-end gap-sm p-md border-t border-outline-variant/20">
        <Button variant="ghost" onClick={handleReject} disabled={busy}>
          {t('skills.approval.reject')}
        </Button>
        <Button onClick={handleApprove} disabled={busy || !name.trim() || !trigger.trim()}>
          {t('skills.approval.approve')}
        </Button>
      </div>
    </Modal>
  )
}
