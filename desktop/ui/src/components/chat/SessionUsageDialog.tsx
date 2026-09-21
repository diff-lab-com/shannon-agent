// SessionUsageDialog — composer 侧的会话用量弹框(2026-09 三项 UX 修复 #3)。
// 模型 chip 旁一次点击即可看到"本会话"的 token 去向,无需绕道用量页。
// 纯组合:六类 breakdown 由 ContextBreakdownCard 自取数(usageTick 驱动
// 流式刷新),预算读取/写入复用 useSessionBudget + BudgetDialog。

import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { useSessions } from '@/context/SessionContext'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import ContextBreakdownCard from '@/components/chat/ContextBreakdownCard'
import BudgetDialog from '@/components/chat/BudgetDialog'

export interface SessionUsageDialogProps {
  open: boolean
  onClose: () => void
  /** 最近一次流式 Usage payload — 变化时 breakdown 重新估算。 */
  usageTick?: unknown
}

export default function SessionUsageDialog({ open, onClose, usageTick }: SessionUsageDialogProps) {
  const t = useT()
  const navigate = useNavigate()
  const { currentSessionId } = useSessions()
  const { budget, usage: sessionUsage, refresh: refreshBudget } = useSessionBudget(currentSessionId)
  const [budgetOpen, setBudgetOpen] = useState(false)

  if (!open) return null

  const spent = sessionUsage?.cost_usd ?? 0
  const hasCap = budget != null && budget > 0

  return (
    <>
      <Modal open onClose={onClose} title={t('chat.input.usage.title')} size="md">
        <ModalBody className="pt-0 space-y-md">
          {currentSessionId ? (
            <>
              <ContextBreakdownCard sessionId={currentSessionId} usageTick={usageTick} />
              {/* 预算摘要一行 — 编辑走既有 BudgetDialog,与右侧 Dock 同源。 */}
              <div className="flex items-center justify-between gap-sm px-md py-sm rounded-xl bg-surface-container border border-outline-variant/10">
                <span className="font-body-sm text-on-surface-variant truncate tabular-nums">
                  {hasCap ? `$${spent.toFixed(4)} / $${budget!.toFixed(2)}` : t('budget.dialog.label')}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  className="cursor-pointer shrink-0"
                  onClick={() => setBudgetOpen(true)}
                >
                  <span className="material-symbols-outlined icon-sm mr-xs" aria-hidden="true">payments</span>
                  {t('budget.menu.set')}
                </Button>
              </div>
            </>
          ) : (
            <p className="font-body-sm text-on-surface-variant text-center py-lg">
              {t('chat.input.usage.noSession')}
            </p>
          )}
        </ModalBody>
        <ModalFooter className="pt-0">
          <Button
            variant="ghost"
            disabled={!currentSessionId}
            title={t('chat.input.usage.viewAll')}
            className="mr-auto px-md py-sm rounded-xl text-primary hover:bg-primary/10 cursor-pointer disabled:opacity-40"
            onClick={() => { onClose(); navigate('/usage') }}
          >
            <span className="material-symbols-outlined icon-md mr-xs" aria-hidden="true">monitoring</span>
            {t('chat.input.usage.viewAll')}
          </Button>
          <Button variant="ghost" onClick={onClose} className="px-md py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer">
            {t('budget.dialog.cancel')}
          </Button>
        </ModalFooter>
      </Modal>
      <BudgetDialog
        open={budgetOpen}
        sessionId={currentSessionId}
        budget={budget}
        onClose={() => setBudgetOpen(false)}
        onSaved={() => refreshBudget()}
      />
    </>
  )
}
