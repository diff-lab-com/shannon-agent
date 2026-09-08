// QuickFixDrawer — side drawer hosting the LSP quick-fix panel.
// Migrated to the shared `<SidePanel>` primitive in R2 (T1.2/R1c tail).
// Focus trap, Escape close, scroll-lock, and backdrop click are all
// owned by the new Base-UI-backed SidePanel — no local state needed.

import { SidePanel, SidePanelBody } from '@/components/ui/side-panel'
import LspQuickFixPanel from '@/components/lsp/LspQuickFixPanel'
import type { DrawerDiag } from './types'

interface QuickFixDrawerProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  drawer: DrawerDiag
  onClose: () => void
}

export default function QuickFixDrawer({ t, drawer, onClose }: QuickFixDrawerProps) {
  return (
    <SidePanel
      open
      onClose={onClose}
      width="420px"
      ariaLabel={t('editor.quickFixDrawer')}
      className="flex flex-col gap-sm"
    >
      <SidePanelBody className="p-md flex flex-col gap-sm">
        <LspQuickFixPanel
          diagnostic={drawer}
          onApplied={() => {
            /* nothing — panel shows its own confirmation */
          }}
          onClose={onClose}
        />
      </SidePanelBody>
    </SidePanel>
  )
}
