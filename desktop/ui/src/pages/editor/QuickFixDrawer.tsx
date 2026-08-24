// QuickFixDrawer — side drawer hosting the LSP quick-fix panel. Owns its
// own Escape-key listener + focus-trap ref via useModalFocus. The drawer is
// "open" when `drawer` is non-null and renders nothing otherwise.

import { useEffect, useRef } from 'react'
import { Button } from '@/components/ui/button'
import LspQuickFixPanel from '@/components/lsp/LspQuickFixPanel'
import { useModalFocus } from '@/hooks/useModalFocus'
import type { DrawerDiag } from './types'

interface QuickFixDrawerProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  drawer: DrawerDiag
  onClose: () => void
}

export default function QuickFixDrawer({ t, drawer, onClose }: QuickFixDrawerProps) {
  const drawerRef = useRef<HTMLDivElement>(null)
  useModalFocus(true, drawerRef)

  // Close drawer on Escape
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div
      className="fixed inset-0 z-[80] flex"
      role="dialog"
      aria-label={t('editor.quickFixDrawer')}
    >
      <Button
        type="button"
        variant="ghost"
        onClick={onClose}
        aria-label={t('editor.closeDrawer')}
        className="flex-1 bg-black/30 hover:bg-black/40 rounded-none border-none"
      />
      <aside ref={drawerRef} className="w-[420px] max-w-[90vw] bg-surface-container-lowest h-full overflow-auto p-md border-l border-outline-variant/30 shadow-lg flex flex-col gap-sm">
        <LspQuickFixPanel
          diagnostic={drawer}
          onApplied={() => {
            /* nothing — panel shows its own confirmation */
          }}
          onClose={onClose}
        />
      </aside>
    </div>
  )
}