import { useRef } from 'react'
import { useIntl } from 'react-intl'
import { useModalFocus } from '@/hooks/useModalFocus'
import { formatShortcut } from '@/lib/platform'

const SHORTCUTS = [
  { keys: () => formatShortcut('K'), action: 'shortcuts.help.openPalette' },
  { keys: () => formatShortcut('N'), action: 'shortcuts.help.newChat' },
  { keys: () => formatShortcut('D'), action: 'shortcuts.help.changeWorkingDir' },
  { keys: () => formatShortcut('1'), action: 'shortcuts.help.goChat' },
  { keys: () => formatShortcut('2'), action: 'shortcuts.help.goTasks' },
  { keys: () => formatShortcut('3'), action: 'shortcuts.help.goExtensions' },
  { keys: () => formatShortcut('4'), action: 'shortcuts.help.goMemory' },
  { keys: () => formatShortcut('5'), action: 'shortcuts.help.goEditor' },
  { keys: () => formatShortcut('6'), action: 'shortcuts.help.goSettings' },
  { keys: () => formatShortcut('/'), action: 'shortcuts.help.toggle' },
  { keys: () => 'Enter', action: 'shortcuts.help.send' },
  { keys: () => 'Shift + Enter', action: 'shortcuts.help.newline' },
  { keys: () => 'Escape', action: 'shortcuts.help.cancel' },
  { keys: () => '?', action: 'shortcuts.help.show' },
]

export default function KeyboardShortcutsHelp({ open, onClose }: { open: boolean; onClose: () => void }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)
  if (!open) return null

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div ref={containerRef} className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl p-xl max-w-md w-full mx-md" role="dialog" aria-modal="true" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-on-surface">{t('shortcutsHelp.title')}</h3>
          <button autoFocus className="p-xs rounded-lg hover:bg-surface-container text-on-surface-variant" onClick={onClose}>
            <span className="material-symbols-outlined icon-md">close</span>
          </button>
        </div>
        <div className="grid grid-cols-2 gap-sm">
          {SHORTCUTS.map(s => {
            const keys = s.keys()
            return (
              <div key={keys} className="flex items-center justify-between gap-sm py-xs">
                <span className="text-body-sm text-on-surface-variant">{t(s.action)}</span>
                <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0">{keys}</kbd>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
