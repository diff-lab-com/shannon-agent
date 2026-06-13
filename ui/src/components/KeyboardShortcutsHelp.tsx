const SHORTCUTS = [
  { keys: '⌘ K', action: 'Open command palette' },
  { keys: '⌘ N', action: 'New chat' },
  { keys: '⌘ 1', action: 'Go to Chat' },
  { keys: '⌘ 2', action: 'Go to Goals' },
  { keys: '⌘ 3', action: 'Go to Tasks' },
  { keys: '⌘ /', action: 'Toggle sidebar' },
  { keys: 'Enter', action: 'Send message' },
  { keys: 'Shift + Enter', action: 'New line in input' },
  { keys: 'Escape', action: 'Cancel query' },
  { keys: '?', action: 'Show this help' },
]

export default function KeyboardShortcutsHelp({ open, onClose }: { open: boolean; onClose: () => void }) {
  if (!open) return null

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl p-xl max-w-md w-full mx-md" role="dialog" aria-modal="true" onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-on-surface">Keyboard Shortcuts</h3>
          <button autoFocus className="p-xs rounded-lg hover:bg-surface-container text-on-surface-variant" onClick={onClose}>
            <span className="material-symbols-outlined text-[20px]">close</span>
          </button>
        </div>
        <div className="grid grid-cols-2 gap-sm">
          {SHORTCUTS.map(s => (
            <div key={s.keys} className="flex items-center justify-between gap-sm py-xs">
              <span className="text-body-sm text-on-surface-variant">{s.action}</span>
              <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0">{s.keys}</kbd>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
