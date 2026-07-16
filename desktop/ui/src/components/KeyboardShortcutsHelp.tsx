import { useMemo, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { useModalFocus } from '@/hooks/useModalFocus'
import { formatShortcut, formatShortcutShift } from '@/lib/platform'

interface ShortcutEntry {
  keys: string
  actionKey: string
}

interface ShortcutSection {
  titleKey: string
  entries: ShortcutEntry[]
}

const SECTIONS: ShortcutSection[] = [
  {
    titleKey: 'shortcutsHelp.section.global',
    entries: [
      { keys: '?', actionKey: 'shortcuts.help.show' },
      { keys: formatShortcut('K'), actionKey: 'shortcuts.help.openPalette' },
      { keys: formatShortcut('/'), actionKey: 'shortcuts.help.toggle' },
    ],
  },
  {
    titleKey: 'shortcutsHelp.section.navigation',
    entries: [
      { keys: formatShortcut('1'), actionKey: 'shortcuts.help.goChat' },
      { keys: formatShortcut('2'), actionKey: 'shortcuts.help.goTasks' },
      { keys: formatShortcut('3'), actionKey: 'shortcuts.help.goExtensions' },
      { keys: formatShortcut('4'), actionKey: 'shortcuts.help.goMemory' },
      { keys: formatShortcut('5'), actionKey: 'shortcuts.help.goEditor' },
      { keys: formatShortcut('6'), actionKey: 'shortcuts.help.goSettings' },
    ],
  },
  {
    titleKey: 'shortcutsHelp.section.chat',
    entries: [
      { keys: formatShortcut('N'), actionKey: 'shortcuts.help.newChat' },
      { keys: formatShortcut('D'), actionKey: 'shortcuts.help.changeWorkingDir' },
      { keys: 'Enter', actionKey: 'shortcuts.help.send' },
      { keys: 'Shift + Enter', actionKey: 'shortcuts.help.newline' },
      { keys: 'Escape', actionKey: 'shortcuts.help.cancel' },
      { keys: formatShortcutShift('P'), actionKey: 'shortcuts.help.planMode' },
      { keys: formatShortcutShift('A'), actionKey: 'shortcuts.help.cycleArtifact' },
    ],
  },
  {
    titleKey: 'shortcutsHelp.section.diffReview',
    entries: [
      { keys: 'j / ↓', actionKey: 'shortcuts.help.diffNext' },
      { keys: 'k / ↑', actionKey: 'shortcuts.help.diffPrev' },
      { keys: 'a', actionKey: 'shortcuts.help.diffAccept' },
      { keys: 'r', actionKey: 'shortcuts.help.diffReject' },
      { keys: 'u', actionKey: 'shortcuts.help.diffUndecided' },
      { keys: 'Enter', actionKey: 'shortcuts.help.diffApply' },
    ],
  },
]

export default function KeyboardShortcutsHelp({ open, onClose }: { open: boolean; onClose: () => void }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const containerRef = useRef<HTMLDivElement>(null)
  const [query, setQuery] = useState('')
  useModalFocus(open, containerRef)

  const filteredSections = useMemo(() => {
    if (!query.trim()) return SECTIONS
    const q = query.toLowerCase()
    return SECTIONS.map(section => ({
      ...section,
      entries: section.entries.filter(e =>
        t(e.actionKey).toLowerCase().includes(q) || e.keys.toLowerCase().includes(q),
      ),
    })).filter(section => section.entries.length > 0)
  }, [query, intl.locale])

  if (!open) return null

  return (
    <div className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm" onClick={onClose}>
      <div
        ref={containerRef}
        className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl p-lg max-w-lg w-full mx-md max-h-[80vh] flex flex-col"
        role="dialog"
        aria-modal="true"
        aria-label={t('shortcutsHelp.title')}
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-md gap-sm">
          <h3 className="font-headline-md text-on-surface">{t('shortcutsHelp.title')}</h3>
          <button
            className="p-xs rounded-lg hover:bg-surface-container text-on-surface-variant focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
            onClick={onClose}
            aria-label={t('shortcutsHelp.close')}
          >
            <span className="material-symbols-outlined icon-md">close</span>
          </button>
        </div>
        <div className="mb-md">
          <div className="flex items-center gap-xs px-sm py-xs rounded-lg border border-outline-variant/30 bg-surface-container-lowest focus-within:border-primary/50 transition-colors">
            <span className="material-symbols-outlined icon-sm text-on-surface-variant">search</span>
            <input
              type="text"
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder={t('shortcutsHelp.search')}
              aria-label={t('shortcutsHelp.search')}
              className="flex-1 bg-transparent border-none outline-none text-body-sm text-on-surface placeholder:text-outline-variant"
            />
          </div>
        </div>
        <div className="overflow-y-auto -mx-xs px-xs">
          {filteredSections.length === 0 ? (
            <p className="text-body-sm text-on-surface-variant italic py-md text-center">{t('shortcutsHelp.noResults')}</p>
          ) : (
            filteredSections.map(section => (
              <section key={section.titleKey} className="mb-md last:mb-0">
                <h4 className="font-label-md text-on-surface-variant uppercase tracking-wider text-[11px] mb-xs px-xs">
                  {t(section.titleKey)}
                </h4>
                <ul className="grid grid-cols-1 sm:grid-cols-2 gap-xs">
                  {section.entries.map(entry => (
                    <li
                      key={`${section.titleKey}-${entry.actionKey}-${entry.keys}`}
                      className="flex items-center justify-between gap-sm py-xs px-sm rounded-md hover:bg-surface-container-low/60"
                    >
                      <span className="text-body-sm text-on-surface truncate">{t(entry.actionKey)}</span>
                      <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0 border border-outline-variant/30">
                        {entry.keys}
                      </kbd>
                    </li>
                  ))}
                </ul>
              </section>
            ))
          )}
        </div>
      </div>
    </div>
  )
}
